//! Generic persistence for plain synced rows (the Phase 6 menu, floor and
//! open-order tables): a row struct serializes to its column map, exactly
//! the shape the outbox sends. `upsert` writes it (booleans as 0/1, JSON
//! values as text, like the sync apply path) and records the outbox event in
//! the caller's transaction; `select` reads rows back through serde.
//!
//! Row structs mark JSON-text columns with `#[serde(with = "rows::json_text")]`
//! and booleans with `#[serde(deserialize_with = "rows::int_bool")]`.

use pos_core::time::Timestamp;
use rusqlite::types::{Value, ValueRef};
use rusqlite::{params_from_iter, Connection, Params};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value as Json;
use uuid::Uuid;

use super::outbox::{self, EventType};

fn conversion(e: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(e))
}

fn to_sql(value: &Json) -> Value {
    match value {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Integer(i64::from(*b)),
        Json::Number(n) => n.as_i64().map_or(Value::Null, Value::Integer),
        Json::String(s) => Value::Text(s.clone()),
        other @ (Json::Array(_) | Json::Object(_)) => Value::Text(other.to_string()),
    }
}

/// Inserts or replaces the row (by `id`) and records an upsert event.
pub fn upsert<T: Serialize>(
    conn: &Connection,
    table: &str,
    row: &T,
    now: Timestamp,
) -> rusqlite::Result<()> {
    let Json::Object(map) = serde_json::to_value(row).map_err(conversion)? else {
        return Err(conversion(std::io::Error::other(
            "row must serialize to an object",
        )));
    };
    let id = map
        .get("id")
        .and_then(Json::as_str)
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| conversion(std::io::Error::other("row has no id")))?;
    let columns: Vec<&String> = map.keys().collect();
    let placeholders = (1..=columns.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let updates = columns
        .iter()
        .filter(|c| !matches!(c.as_str(), "id" | "created_at"))
        .map(|c| format!("{c} = excluded.{c}"))
        .collect::<Vec<_>>()
        .join(", ");
    let names = columns
        .iter()
        .map(|c| c.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    conn.execute(
        &format!("INSERT INTO {table} ({names}) VALUES ({placeholders}) ON CONFLICT (id) DO UPDATE SET {updates}"),
        params_from_iter(map.values().map(to_sql)),
    )?;
    outbox::record(conn, table, EventType::Upsert, id, row, now)
}

/// Runs `sql` and deserializes each row from its column map.
pub fn select<T: DeserializeOwned, P: Params>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> rusqlite::Result<Vec<T>> {
    let mut stmt = conn.prepare(sql)?;
    let names: Vec<String> = stmt.column_names().into_iter().map(str::to_owned).collect();
    let mut rows = stmt.query(params)?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut map = serde_json::Map::with_capacity(names.len());
        for (i, name) in names.iter().enumerate() {
            let value = match row.get_ref(i)? {
                ValueRef::Null => Json::Null,
                ValueRef::Integer(n) => Json::from(n),
                ValueRef::Text(t) => Json::String(String::from_utf8_lossy(t).into_owned()),
                ValueRef::Real(_) | ValueRef::Blob(_) => {
                    return Err(rusqlite::Error::InvalidColumnType(
                        i,
                        name.clone(),
                        row.get_ref(i)?.data_type(),
                    ))
                }
            };
            map.insert(name.clone(), value);
        }
        out.push(serde_json::from_value(Json::Object(map)).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?);
    }
    Ok(out)
}

/// SQLite 0/1 (or a real bool) → bool.
pub fn int_bool<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Bool(bool),
        Int(i64),
    }
    Ok(match Raw::deserialize(d)? {
        Raw::Bool(b) => b,
        Raw::Int(n) => n != 0,
    })
}

/// A JSON value stored as TEXT; serializes as the value itself.
pub mod json_text {
    use serde::de::DeserializeOwned;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use serde_json::Value as Json;

    pub fn serialize<T: Serialize, S: Serializer>(value: &T, s: S) -> Result<S::Ok, S::Error> {
        value.serialize(s)
    }

    pub fn deserialize<'de, T: DeserializeOwned, D: Deserializer<'de>>(
        d: D,
    ) -> Result<T, D::Error> {
        match Json::deserialize(d)? {
            Json::String(text) => serde_json::from_str(&text).map_err(serde::de::Error::custom),
            other => serde_json::from_value(other).map_err(serde::de::Error::custom),
        }
    }
}

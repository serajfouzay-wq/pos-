//! Device-local JSON settings (`settings` table).

use pos_core::time::Timestamp;
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;

use super::new_id;

pub fn get<T: DeserializeOwned>(conn: &Connection, key: &str) -> rusqlite::Result<Option<T>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1 AND deleted_at IS NULL",
            [key],
            |row| row.get(0),
        )
        .optional()?;
    Ok(raw.and_then(|v| serde_json::from_str(&v).ok()))
}

pub fn put<T: Serialize>(
    conn: &Connection,
    key: &str,
    value: &T,
    now: Timestamp,
) -> rusqlite::Result<()> {
    let json = serde_json::to_string(value)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(
        "INSERT INTO settings (id, created_at, updated_at, key, value) VALUES (?1, ?2, ?2, ?3, ?4)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at, deleted_at = NULL",
        params![new_id().to_string(), now.to_string(), key, json],
    )?;
    Ok(())
}

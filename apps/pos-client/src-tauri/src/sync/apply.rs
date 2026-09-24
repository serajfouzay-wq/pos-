//! Applying pulled changes to the local database.
//!
//! Writes here bypass the repo layer on purpose: a pulled row is already in
//! the cloud, so it must NOT produce an outbox event (that would echo it back
//! forever).
//!
//! - last-write-wins: take the server's version unless this device holds a
//!   *pending* edit of the row that beats it on `(updated_at, event_id)` —
//!   the same comparison the server makes, so both sides pick one winner.
//! - append-only: insert once.
//! - additive delta: insert once, then add the delta to its cached aggregate
//!   (`products.stock_on_hand_milli`, `customers.loyalty_points`).

use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection, OptionalExtension};
use serde_json::Value as Json;

use super::protocol::Change;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    LastWriteWins,
    AppendOnly,
    AdditiveDelta,
}

/// Mirrors `SYNC_ENTITY_STRATEGY`; pinned to `contracts/db-schema.json`.
pub const ENTITIES: &[(&str, Strategy)] = &[
    ("categories", Strategy::LastWriteWins),
    ("products", Strategy::LastWriteWins),
    ("customers", Strategy::LastWriteWins),
    ("users", Strategy::LastWriteWins),
    ("discount_rules", Strategy::LastWriteWins),
    ("shifts", Strategy::LastWriteWins),
    ("suppliers", Strategy::LastWriteWins),
    ("purchase_orders", Strategy::LastWriteWins),
    ("purchase_order_items", Strategy::LastWriteWins),
    ("modifier_groups", Strategy::LastWriteWins),
    ("modifiers", Strategy::LastWriteWins),
    ("product_modifier_groups", Strategy::LastWriteWins),
    ("combos", Strategy::LastWriteWins),
    ("combo_items", Strategy::LastWriteWins),
    ("dining_tables", Strategy::LastWriteWins),
    ("open_orders", Strategy::LastWriteWins),
    ("transactions", Strategy::AppendOnly),
    ("transaction_items", Strategy::AppendOnly),
    ("transaction_payments", Strategy::AppendOnly),
    ("audit_log", Strategy::AppendOnly),
    ("stock_movements", Strategy::AdditiveDelta),
    ("loyalty_ledger", Strategy::AdditiveDelta),
];

pub fn strategy(entity: &str) -> Option<Strategy> {
    ENTITIES
        .iter()
        .find(|(name, _)| *name == entity)
        .map(|(_, s)| *s)
}

/// Aggregates maintained from delta tables — never taken from a synced row.
fn derived_columns(entity: &str) -> &'static [&'static str] {
    match entity {
        "products" => &["stock_on_hand_milli"],
        "customers" => &["loyalty_points"],
        _ => &[],
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApplyError {
    #[error("unknown entity type {0}")]
    UnknownEntity(String),
    #[error("{entity}: {message}")]
    Invalid { entity: String, message: String },
}

fn invalid(entity: &str, message: impl Into<String>) -> ApplyError {
    ApplyError::Invalid {
        entity: entity.to_owned(),
        message: message.into(),
    }
}

fn columns(conn: &Connection, table: &str) -> rusqlite::Result<Vec<String>> {
    conn.prepare(&format!(
        "SELECT name FROM pragma_table_info('{table}') ORDER BY cid"
    ))?
    .query_map([], |r| r.get(0))?
    .collect()
}

fn to_sql(entity: &str, column: &str, value: Option<&Json>) -> Result<Value, ApplyError> {
    Ok(match value {
        None | Some(Json::Null) => Value::Null,
        Some(Json::Bool(b)) => Value::Integer(i64::from(*b)),
        Some(Json::Number(n)) => Value::Integer(
            n.as_i64()
                .ok_or_else(|| invalid(entity, format!("{column} must be an integer, got {n}")))?,
        ),
        Some(Json::String(s)) => Value::Text(s.clone()),
        Some(other @ (Json::Array(_) | Json::Object(_))) => Value::Text(other.to_string()),
    })
}

/// Outcome for one change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    Written,
    /// Already present (append) or a pending local edit wins (LWW).
    Kept,
}

pub fn apply(conn: &Connection, change: &Change) -> Result<Applied, ApplyError> {
    let entity = change.entity_type.as_str();
    let strategy = strategy(entity).ok_or_else(|| ApplyError::UnknownEntity(entity.to_owned()))?;
    let row = change
        .row
        .as_object()
        .ok_or_else(|| invalid(entity, "row is not an object"))?;
    let id = row
        .get("id")
        .and_then(Json::as_str)
        .ok_or_else(|| invalid(entity, "row has no id"))?
        .to_owned();
    let cols = columns(conn, entity).map_err(|e| invalid(entity, e.to_string()))?;
    let derived = derived_columns(entity);

    if strategy == Strategy::LastWriteWins && local_edit_wins(conn, entity, &id, row, change)? {
        return Ok(Applied::Kept);
    }

    let mut values = Vec::with_capacity(cols.len());
    for column in &cols {
        values.push(if derived.contains(&column.as_str()) {
            Value::Integer(0) // recomputed below from the local deltas
        } else {
            to_sql(entity, column, row.get(column))?
        });
    }
    let placeholders = (1..=cols.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let conflict = match strategy {
        Strategy::LastWriteWins => {
            let updates = cols
                .iter()
                .filter(|c| c.as_str() != "id" && !derived.contains(&c.as_str()))
                .map(|c| format!("{c} = excluded.{c}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("DO UPDATE SET {updates}")
        }
        Strategy::AppendOnly | Strategy::AdditiveDelta => "DO NOTHING".to_owned(),
    };
    let sql = format!(
        "INSERT INTO {entity} ({}) VALUES ({placeholders}) ON CONFLICT (id) {conflict}",
        cols.join(", ")
    );
    let changed = conn
        .execute(&sql, params_from_iter(values))
        .map_err(|e| invalid(entity, e.to_string()))?;

    match (entity, strategy) {
        ("products", _) => {
            conn.execute(
                "UPDATE products SET stock_on_hand_milli =
                   (SELECT COALESCE(SUM(quantity_delta_milli), 0) FROM stock_movements WHERE product_id = ?1)
                 WHERE id = ?1",
                [&id],
            )
            .map_err(|e| invalid(entity, e.to_string()))?;
        }
        ("customers", _) => {
            conn.execute(
                "UPDATE customers SET loyalty_points =
                   (SELECT COALESCE(SUM(points_delta), 0) FROM loyalty_ledger WHERE customer_id = ?1)
                 WHERE id = ?1",
                [&id],
            )
            .map_err(|e| invalid(entity, e.to_string()))?;
        }
        ("stock_movements", Strategy::AdditiveDelta) if changed == 1 => {
            conn.execute(
                "UPDATE products SET stock_on_hand_milli = stock_on_hand_milli + ?1 WHERE id = ?2",
                rusqlite::params![
                    int(row.get("quantity_delta_milli")),
                    str_of(row.get("product_id"))
                ],
            )
            .map_err(|e| invalid(entity, e.to_string()))?;
        }
        ("loyalty_ledger", Strategy::AdditiveDelta) if changed == 1 => {
            conn.execute(
                "UPDATE customers SET loyalty_points = loyalty_points + ?1 WHERE id = ?2",
                rusqlite::params![int(row.get("points_delta")), str_of(row.get("customer_id"))],
            )
            .map_err(|e| invalid(entity, e.to_string()))?;
        }
        _ => {}
    }
    Ok(if changed == 1 {
        Applied::Written
    } else {
        Applied::Kept
    })
}

fn int(value: Option<&Json>) -> i64 {
    value.and_then(Json::as_i64).unwrap_or(0)
}

fn str_of(value: Option<&Json>) -> String {
    value.and_then(Json::as_str).unwrap_or_default().to_owned()
}

/// True when this device holds an unsent edit that beats the server version.
fn local_edit_wins(
    conn: &Connection,
    entity: &str,
    id: &str,
    row: &serde_json::Map<String, Json>,
    change: &Change,
) -> Result<bool, ApplyError> {
    let pending: Option<(String, String)> = conn
        .query_row(
            &format!(
                "SELECT t.updated_at, q.id FROM {entity} t
                 JOIN sync_queue q ON q.entity_type = ?1 AND q.entity_id = t.id
                 WHERE t.id = ?2 AND q.sent_at IS NULL AND q.deleted_at IS NULL
                 ORDER BY q.created_at DESC, q.id DESC LIMIT 1"
            ),
            [entity, id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| invalid(entity, e.to_string()))?;
    let Some((local_updated, local_event)) = pending else {
        return Ok(false);
    };
    let incoming_updated = row
        .get("updated_at")
        .and_then(Json::as_str)
        .unwrap_or_default();
    let incoming_event = change.event_id.map(|e| e.to_string()).unwrap_or_default();
    // Fixed-width UTC timestamps and lower-case UUIDs compare correctly as strings,
    // exactly like the server's `(timestamptz, uuid)` tuple comparison.
    Ok(
        (local_updated.as_str(), local_event.as_str())
            > (incoming_updated, incoming_event.as_str()),
    )
}

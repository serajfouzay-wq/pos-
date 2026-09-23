//! Transactional outbox: every synced mutation writes a `sync_queue` event in
//! the same SQLite transaction. The Phase 4 worker drains it; `id` doubles as
//! the idempotency key on the server.

use pos_core::time::Timestamp;
use rusqlite::{params, Connection};
use serde::Serialize;
use uuid::Uuid;

use super::new_id;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    /// Full-row snapshot of a last-write-wins table.
    Upsert,
    /// Insert-once row of an append-only / additive-delta table.
    Append,
}

/// Records `row` (which must serialize with `id` and the base columns).
pub fn record<T: Serialize>(
    conn: &Connection,
    entity_type: &str,
    event: EventType,
    entity_id: Uuid,
    row: &T,
    now: Timestamp,
) -> rusqlite::Result<()> {
    let payload = serde_json::to_string(row)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(
        "INSERT INTO sync_queue (id, created_at, updated_at, event_type, entity_type, entity_id, payload)
         VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6)",
        params![
            new_id().to_string(),
            now.to_string(),
            match event {
                EventType::Upsert => "upsert",
                EventType::Append => "append",
            },
            entity_type,
            entity_id.to_string(),
            payload,
        ],
    )?;
    Ok(())
}

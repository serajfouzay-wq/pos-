//! Offline receipt queue.

use pos_core::time::Timestamp;
use rusqlite::{params, Connection};
use uuid::Uuid;

use super::{new_id, uuid_at};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintJob {
    pub id: Uuid,
    pub transaction_id: Uuid,
    pub copy: bool,
}

pub fn enqueue(
    conn: &Connection,
    transaction_id: Uuid,
    copy: bool,
    now: Timestamp,
) -> rusqlite::Result<Uuid> {
    let id = new_id();
    conn.execute(
        "INSERT INTO print_jobs (id, created_at, updated_at, transaction_id, copy) VALUES (?1, ?2, ?2, ?3, ?4)",
        params![id.to_string(), now.to_string(), transaction_id.to_string(), copy],
    )?;
    Ok(id)
}

/// Oldest first: receipts come out in the order the sales happened.
pub fn pending(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<PrintJob>> {
    conn.prepare(
        "SELECT id, transaction_id, copy FROM print_jobs
         WHERE printed_at IS NULL AND deleted_at IS NULL ORDER BY created_at, id LIMIT ?1",
    )?
    .query_map([limit], |row| {
        Ok(PrintJob {
            id: uuid_at(row, 0)?,
            transaction_id: uuid_at(row, 1)?,
            copy: row.get(2)?,
        })
    })?
    .collect()
}

pub fn pending_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT count(*) FROM print_jobs WHERE printed_at IS NULL AND deleted_at IS NULL",
        [],
        |r| r.get(0),
    )
}

pub fn mark_printed(conn: &Connection, id: Uuid, now: Timestamp) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE print_jobs SET printed_at = ?1, updated_at = ?1, attempt_count = attempt_count + 1, last_error = NULL
         WHERE id = ?2",
        params![now.to_string(), id.to_string()],
    )?;
    Ok(())
}

pub fn mark_failed(
    conn: &Connection,
    id: Uuid,
    error: &str,
    now: Timestamp,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE print_jobs SET updated_at = ?1, attempt_count = attempt_count + 1, last_error = ?2 WHERE id = ?3",
        params![now.to_string(), error, id.to_string()],
    )?;
    Ok(())
}

/// Has a receipt for this transaction ever come out of a printer?
pub fn ever_printed(conn: &Connection, transaction_id: Uuid) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM print_jobs WHERE transaction_id = ?1 AND printed_at IS NOT NULL)",
        [transaction_id.to_string()],
        |r| r.get(0),
    )
}

/// Is a job for this transaction still waiting?
pub fn has_pending(conn: &Connection, transaction_id: Uuid) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM print_jobs WHERE transaction_id = ?1 AND printed_at IS NULL AND deleted_at IS NULL)",
        [transaction_id.to_string()],
        |r| r.get(0),
    )
}

// ── Kitchen tickets ─────────────────────────────────────────────────────────

/// A kitchen ticket waiting for this till's kitchen printer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitchenJob {
    pub id: Uuid,
    pub ticket: pos_hardware::kitchen::KitchenTicket,
}

pub fn enqueue_kitchen(
    conn: &Connection,
    ticket: &pos_hardware::kitchen::KitchenTicket,
    now: Timestamp,
) -> rusqlite::Result<Uuid> {
    let id = new_id();
    let json = serde_json::to_string(ticket)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    conn.execute(
        "INSERT INTO kitchen_print_jobs (id, created_at, updated_at, ticket) VALUES (?1, ?2, ?2, ?3)",
        params![id.to_string(), now.to_string(), json],
    )?;
    Ok(id)
}

/// Oldest first, so the kitchen gets tickets in the order they were sent.
pub fn pending_kitchen(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<KitchenJob>> {
    conn.prepare(
        "SELECT id, ticket FROM kitchen_print_jobs
         WHERE printed_at IS NULL AND deleted_at IS NULL ORDER BY created_at, id LIMIT ?1",
    )?
    .query_map([limit], |row| {
        let json: String = row.get(1)?;
        Ok(KitchenJob {
            id: uuid_at(row, 0)?,
            ticket: serde_json::from_str(&json).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?,
        })
    })?
    .collect()
}

pub fn kitchen_pending_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT count(*) FROM kitchen_print_jobs WHERE printed_at IS NULL AND deleted_at IS NULL",
        [],
        |r| r.get(0),
    )
}

pub fn mark_kitchen(
    conn: &Connection,
    id: Uuid,
    error: Option<&str>,
    now: Timestamp,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE kitchen_print_jobs
         SET updated_at = ?1, attempt_count = attempt_count + 1, last_error = ?2,
             printed_at = CASE WHEN ?2 IS NULL THEN ?1 ELSE printed_at END
         WHERE id = ?3",
        params![now.to_string(), error, id.to_string()],
    )?;
    Ok(())
}

/// Tickets nobody will cook any more: dropped (soft-deleted) after a day.
pub fn expire_kitchen(
    conn: &Connection,
    before: Timestamp,
    now: Timestamp,
) -> rusqlite::Result<usize> {
    conn.execute(
        "UPDATE kitchen_print_jobs SET deleted_at = ?1, updated_at = ?1
         WHERE printed_at IS NULL AND deleted_at IS NULL AND created_at < ?2",
        params![now.to_string(), before.to_string()],
    )
}

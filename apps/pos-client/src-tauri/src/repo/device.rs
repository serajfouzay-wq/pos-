use rusqlite::{Connection, OptionalExtension};
use uuid::Uuid;

use super::uuid_at;

/// This till's device id (the row is created when the license is accepted).
pub fn id(conn: &Connection) -> rusqlite::Result<Uuid> {
    conn.query_row(
        "SELECT id FROM device WHERE deleted_at IS NULL ORDER BY created_at LIMIT 1",
        [],
        |row| uuid_at(row, 0),
    )
    .optional()?
    .ok_or(rusqlite::Error::QueryReturnedNoRows)
}

/// Short, stable prefix for receipt numbers, e.g. `7F3A` → `7F3A-000123`.
pub fn receipt_prefix(device_id: Uuid) -> String {
    device_id.simple().to_string()[..4].to_uppercase()
}

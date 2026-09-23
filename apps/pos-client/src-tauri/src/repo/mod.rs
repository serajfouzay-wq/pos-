//! Data access. Every function takes a `&Connection` (usually inside a
//! transaction opened by the caller) so multi-table writes — a sale, its
//! lines, payments, stock movements, outbox events and audit entry — commit
//! or roll back together.
//!
//! Conventions: ids are v7 UUIDs stored as TEXT; timestamps use the fixed
//! `Timestamp` format; enums are stored by their serde (snake_case) names.

pub mod audit;
pub mod catalog;
pub mod device;
pub mod outbox;
pub mod print_jobs;
pub mod sales;
pub mod settings;
pub mod shifts;
pub mod users;

use pos_core::time::Timestamp;
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use rusqlite::types::Type;
use rusqlite::Row;
use serde::de::DeserializeOwned;
use serde::Serialize;
use uuid::Uuid;

/// Maps storage errors onto the IPC error shape. SQL text never reaches the UI.
pub trait SqlResultExt<T> {
    fn ipc(self) -> IpcResult<T>;
}

impl<T> SqlResultExt<T> for rusqlite::Result<T> {
    fn ipc(self) -> IpcResult<T> {
        self.map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                IpcError::new(IpcErrorCode::NotFound, "Not found.")
            }
            rusqlite::Error::SqliteFailure(f, _)
                if f.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                IpcError::new(
                    IpcErrorCode::Conflict,
                    "That change conflicts with existing data.",
                )
            }
            other => IpcError::internal(format!("storage error: {other}")),
        })
    }
}

fn conversion(i: usize, e: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(i, Type::Text, Box::new(e))
}

pub fn uuid_at(row: &Row<'_>, i: usize) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&row.get::<_, String>(i)?).map_err(|e| conversion(i, e))
}

pub fn opt_uuid_at(row: &Row<'_>, i: usize) -> rusqlite::Result<Option<Uuid>> {
    row.get::<_, Option<String>>(i)?
        .map(|s| Uuid::parse_str(&s).map_err(|e| conversion(i, e)))
        .transpose()
}

pub fn ts_at(row: &Row<'_>, i: usize) -> rusqlite::Result<Timestamp> {
    row.get::<_, String>(i)?
        .parse()
        .map_err(|e| conversion(i, e))
}

pub fn opt_ts_at(row: &Row<'_>, i: usize) -> rusqlite::Result<Option<Timestamp>> {
    row.get::<_, Option<String>>(i)?
        .map(|s| s.parse().map_err(|e| conversion(i, e)))
        .transpose()
}

/// Enum stored as its serde name (`"cashier"`, `"card"`, …).
pub fn enum_at<T: DeserializeOwned>(row: &Row<'_>, i: usize) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(row.get(i)?)).map_err(|e| conversion(i, e))
}

pub fn json_at(row: &Row<'_>, i: usize) -> rusqlite::Result<serde_json::Value> {
    serde_json::from_str(&row.get::<_, String>(i)?).map_err(|e| conversion(i, e))
}

/// Serde name of an enum value, for storage.
pub fn enum_str<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}

pub fn new_id() -> Uuid {
    Uuid::now_v7()
}

/// Columns every table has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Meta {
    pub id: Uuid,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub deleted_at: Option<Timestamp>,
}

impl Meta {
    pub fn new(now: Timestamp) -> Self {
        Self {
            id: new_id(),
            created_at: now,
            updated_at: now,
            deleted_at: None,
        }
    }

    /// Reads the four base columns starting at `i`.
    pub fn read(row: &Row<'_>, i: usize) -> rusqlite::Result<Self> {
        Ok(Self {
            id: uuid_at(row, i)?,
            created_at: ts_at(row, i + 1)?,
            updated_at: ts_at(row, i + 2)?,
            deleted_at: opt_ts_at(row, i + 3)?,
        })
    }
}

pub const META_COLUMNS: &str = "id, created_at, updated_at, deleted_at";

#[cfg(test)]
mod tests;

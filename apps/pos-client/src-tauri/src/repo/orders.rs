//! Open orders: cafe tabs and restaurant tables, held until paid.

use pos_core::sales::OrderType;
use pos_core::time::Timestamp;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::rows::{self, json_text};
use super::Meta;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComboRef {
    pub combo_id: Uuid,
    pub instance: Uuid,
}

/// Mirrors `OpenOrderItemSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenOrderItem {
    pub line_id: Uuid,
    pub product_id: Uuid,
    pub quantity_milli: i64,
    #[serde(default)]
    pub modifier_ids: Vec<Uuid>,
    pub course: Option<i64>,
    pub note: Option<String>,
    #[serde(default)]
    pub combo: Option<ComboRef>,
    pub fired_at: Option<Timestamp>,
    pub added_by: Uuid,
    pub added_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenOrderStatus {
    Open,
    Settled,
    Cancelled,
}

/// Mirrors `OpenOrderSchema`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpenOrder {
    #[serde(flatten)]
    pub meta: Meta,
    pub device_id: Uuid,
    pub order_type: OrderType,
    pub table_id: Option<Uuid>,
    pub label: Option<String>,
    pub guests: i64,
    pub status: OpenOrderStatus,
    #[serde(with = "json_text")]
    pub items: Vec<OpenOrderItem>,
    #[serde(with = "json_text")]
    pub transaction_ids: Vec<Uuid>,
    pub opened_by: Uuid,
    pub opened_at: Timestamp,
    pub closed_at: Option<Timestamp>,
    pub notes: Option<String>,
}

pub fn open(conn: &Connection) -> rusqlite::Result<Vec<OpenOrder>> {
    rows::select(
        conn,
        "SELECT * FROM open_orders WHERE status = 'open' AND deleted_at IS NULL ORDER BY opened_at",
        [],
    )
}

pub fn get(conn: &Connection, id: Uuid) -> rusqlite::Result<Option<OpenOrder>> {
    Ok(rows::select(
        conn,
        "SELECT * FROM open_orders WHERE id = ?1 AND deleted_at IS NULL",
        [id.to_string()],
    )?
    .into_iter()
    .next())
}

/// The open order seated at `table_id`, if any.
pub fn at_table(conn: &Connection, table_id: Uuid) -> rusqlite::Result<Option<OpenOrder>> {
    Ok(rows::select(
        conn,
        "SELECT * FROM open_orders WHERE table_id = ?1 AND status = 'open' AND deleted_at IS NULL
         ORDER BY opened_at LIMIT 1",
        [table_id.to_string()],
    )?
    .into_iter()
    .next())
}

pub fn save(conn: &Connection, order: &OpenOrder, now: Timestamp) -> rusqlite::Result<()> {
    rows::upsert(conn, "open_orders", order, now)
}

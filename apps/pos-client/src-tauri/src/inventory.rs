//! Stock adjustments (retail back office). Every change is an additive
//! `stock_movements` row, so counts on two tills never overwrite each other.

use pos_core::time::Timestamp;
use pos_core::{IpcError, IpcResult};
use rusqlite::Connection;
use serde::Deserialize;
use uuid::Uuid;

use crate::repo::audit::{self, Actor};
use crate::repo::catalog::{self, Product, StockReason};
use crate::repo::SqlResultExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StockMode {
    /// Goods received: +quantity.
    Receive,
    /// Correction: ±quantity.
    Adjust,
    /// Damaged / expired: −quantity.
    Waste,
    /// Physical count: set on-hand to quantity.
    Count,
}

/// Mirrors `StockAdjustmentSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct StockAdjustment {
    pub product_id: Uuid,
    pub mode: StockMode,
    pub quantity_milli: i64,
    pub note: Option<String>,
}

pub fn adjust(
    conn: &Connection,
    actor: &Actor,
    input: &StockAdjustment,
    now: Timestamp,
) -> IpcResult<Product> {
    let product = catalog::get(conn, input.product_id)
        .ipc()?
        .ok_or_else(|| IpcError::validation("That product no longer exists."))?;
    if !product.track_stock {
        return Err(IpcError::validation(format!(
            "{} does not track stock.",
            product.name
        )));
    }
    let q = input.quantity_milli;
    if q.abs() > 1_000_000_000_000 {
        return Err(IpcError::validation("That quantity is too large."));
    }
    let (delta, reason) = match input.mode {
        StockMode::Receive if q > 0 => (q, StockReason::PurchaseReceipt),
        StockMode::Waste if q > 0 => (-q, StockReason::Waste),
        StockMode::Adjust if q != 0 => (q, StockReason::Adjustment),
        StockMode::Count if q >= 0 => (q - product.stock_on_hand_milli, StockReason::StockCount),
        _ => return Err(IpcError::validation("Enter a quantity.")),
    };
    if delta == 0 {
        return Err(IpcError::validation(
            "The count already matches the stock on hand.",
        ));
    }
    catalog::move_stock(conn, product.meta.id, delta, reason, None, actor, now).ipc()?;
    audit::record(
        conn,
        actor,
        "inventory.adjust",
        "products",
        Some(product.meta.id),
        Some(serde_json::json!({ "stock_on_hand_milli": product.stock_on_hand_milli })),
        Some(serde_json::json!({
            "delta_milli": delta,
            "mode": format!("{:?}", input.mode).to_lowercase(),
            "note": input.note.as_deref().map(str::trim).filter(|n| !n.is_empty()),
        })),
        now,
    )
    .ipc()?;
    catalog::get(conn, product.meta.id)
        .ipc()?
        .ok_or_else(|| IpcError::internal("product vanished"))
}

//! Stock (retail back office) and product labels.

use std::sync::Arc;

use pos_core::money::to_decimal_string;
use pos_core::rbac::Permission;
use pos_core::time::{Clock, SystemClock};
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use pos_hardware::label::{symbology, ProductLabel};
use tauri::State;
use uuid::Uuid;

use super::{authorize, blocking};
use crate::inventory::{self, StockAdjustment};
use crate::repo::catalog::{self, Product};
use crate::repo::SqlResultExt;
use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub async fn adjust_stock(
    state: State<'_, AppState>,
    adjustment: StockAdjustment,
) -> IpcResult<Product> {
    let auth = authorize(&state, Permission::InventoryAdjust)?;
    blocking(move || {
        let actor = auth.actor()?;
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        let product = inventory::adjust(&tx, &actor, &adjustment, SystemClock.now())?;
        tx.commit().ipc()?;
        Ok(product)
    })
    .await
    .inspect(|_| state.sync.nudge())
}

/// Prints `copies` shelf labels (name, price, barcode) on the receipt printer.
#[tauri::command(rename_all = "snake_case")]
pub async fn print_product_labels(
    state: State<'_, AppState>,
    product_id: Uuid,
    copies: u8,
) -> IpcResult<()> {
    let auth = authorize(&state, Permission::InventoryView)?;
    if !(1..=50).contains(&copies) {
        return Err(IpcError::validation("Print 1–50 labels at a time."));
    }
    let client = Arc::clone(&state.client);
    let printer = Arc::clone(&state.printer);
    blocking(move || {
        let product = catalog::get(&auth.db.conn(), product_id)
            .ipc()?
            .ok_or_else(|| {
                IpcError::new(IpcErrorCode::NotFound, "That product no longer exists.")
            })?;
        let barcode = product.barcode.clone().filter(|b| symbology(b).is_some());
        let unit = if product.sold_by_weight {
            format!(
                " / {}",
                serde_json::to_value(product.unit)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default()
            )
        } else {
            String::new()
        };
        let label = ProductLabel {
            name: product.name,
            price: format!(
                "{} {}{unit}",
                to_decimal_string(product.price, client.currency.base),
                client.currency.base.as_str()
            ),
            barcode,
        };
        printer.print_labels(&auth.db, &label, copies)
    })
    .await
}

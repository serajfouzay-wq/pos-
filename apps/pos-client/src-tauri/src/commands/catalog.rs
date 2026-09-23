use pos_core::rbac::Permission;
use pos_core::time::{Clock, SystemClock};
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use serde::Deserialize;
use tauri::State;
use uuid::Uuid;

use super::{authorize, blocking};
use crate::repo::catalog::{self, Category, Product, ProductFilter, Unit};
use crate::repo::{audit, Meta, SqlResultExt};
use crate::sample_catalog;
use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub async fn get_products(
    state: State<'_, AppState>,
    filter: ProductFilter,
) -> IpcResult<Vec<Product>> {
    let auth = authorize(&state, Permission::CatalogView)?;
    blocking(move || catalog::query(&auth.db.conn(), &filter).ipc()).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_categories(state: State<'_, AppState>) -> IpcResult<Vec<Category>> {
    let auth = authorize(&state, Permission::CatalogView)?;
    blocking(move || catalog::categories(&auth.db.conn()).ipc()).await
}

/// Mirrors `ProductInputSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct ProductInput {
    pub id: Option<Uuid>,
    pub name: String,
    pub category_id: Option<Uuid>,
    pub sku: Option<String>,
    pub barcode: Option<String>,
    pub price: i64,
    pub tax_rate_bps: i64,
    pub unit: Unit,
    pub track_stock: bool,
    pub reorder_threshold_milli: Option<i64>,
    pub quick_key_position: Option<i64>,
    pub is_active: bool,
}

fn clean(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn save_product(state: State<'_, AppState>, product: ProductInput) -> IpcResult<Product> {
    let auth = authorize(&state, Permission::CatalogManage)?;
    let name = product.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(IpcError::validation("Name must be 1–120 characters."));
    }
    if product.price < 0 || !(0..=10_000).contains(&product.tax_rate_bps) {
        return Err(IpcError::validation(
            "Price must be ≥ 0 and tax between 0 and 100 %.",
        ));
    }
    blocking(move || {
        let now = SystemClock.now();
        let actor = auth.actor()?;
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        let barcode = clean(product.barcode);
        if let Some(code) = &barcode {
            if catalog::barcode_taken(&tx, code, product.id).ipc()? {
                return Err(IpcError::new(
                    IpcErrorCode::Conflict,
                    "Another product already uses that barcode.",
                ));
            }
        }
        let before = match product.id {
            Some(id) => Some(catalog::get(&tx, id).ipc()?.ok_or_else(|| {
                IpcError::new(IpcErrorCode::NotFound, "That product no longer exists.")
            })?),
            None => None,
        };
        let meta = match &before {
            Some(existing) => Meta {
                updated_at: now,
                ..existing.meta.clone()
            },
            None => Meta::new(now),
        };
        let saved = Product {
            meta,
            name,
            name_localized: before
                .as_ref()
                .map_or_else(|| serde_json::json!({}), |b| b.name_localized.clone()),
            category_id: product.category_id,
            sku: clean(product.sku),
            barcode,
            price: product.price,
            cost: before.as_ref().and_then(|b| b.cost),
            tax_rate_bps: product.tax_rate_bps,
            unit: product.unit,
            sold_by_weight: product.unit != Unit::Each,
            track_stock: product.track_stock,
            stock_on_hand_milli: before.as_ref().map_or(0, |b| b.stock_on_hand_milli),
            reorder_threshold_milli: product.reorder_threshold_milli.filter(|t| *t >= 0),
            reorder_quantity_milli: before.as_ref().and_then(|b| b.reorder_quantity_milli),
            image_asset: before.as_ref().and_then(|b| b.image_asset.clone()),
            quick_key_position: product.quick_key_position.filter(|p| *p >= 0),
            is_active: product.is_active,
        };
        catalog::save(&tx, &saved, now).ipc()?;
        audit::record(
            &tx,
            &actor,
            "catalog.manage",
            "products",
            Some(saved.meta.id),
            before.as_ref().and_then(|b| serde_json::to_value(b).ok()),
            serde_json::to_value(&saved).ok(),
            now,
        )
        .ipc()?;
        tx.commit().ipc()?;
        Ok(saved)
    })
    .await
}

/// Seeds a starter catalogue for the client's business type (empty catalogue only).
#[tauri::command(rename_all = "snake_case")]
pub async fn load_sample_catalog(state: State<'_, AppState>) -> IpcResult<usize> {
    let auth = authorize(&state, Permission::CatalogManage)?;
    let client = std::sync::Arc::clone(&state.client);
    blocking(move || {
        let now = SystemClock.now();
        let actor = auth.actor()?;
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        if catalog::count(&tx).ipc()? > 0 {
            return Err(IpcError::new(
                IpcErrorCode::Conflict,
                "The catalogue already has products.",
            ));
        }
        let created = sample_catalog::load(
            &tx,
            client.business_type,
            client.currency.base,
            i64::from(client.tax.default_rate_bps),
            &actor,
            now,
        )
        .ipc()?;
        audit::record(
            &tx,
            &actor,
            "catalog.manage",
            "products",
            None,
            None,
            Some(serde_json::json!({ "sample_catalog": created })),
            now,
        )
        .ipc()?;
        tx.commit().ipc()?;
        Ok(created)
    })
    .await
}

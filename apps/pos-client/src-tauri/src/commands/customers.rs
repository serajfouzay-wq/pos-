//! Customers and the loyalty programme. Looking up and registering a
//! customer at the till: `customer.lookup` (every role). Editing, removing
//! and adjusting points: `customer.manage`. The programme: `settings.manage`.

use std::sync::Arc;

use pos_core::loyalty::LoyaltySettings;
use pos_core::rbac::Permission;
use pos_core::time::{Clock, SystemClock};
use pos_core::{IpcError, IpcResult};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use super::{authorize, blocking};
use crate::loyalty::{self, CustomerInput, PointsAdjustment};
use crate::repo::customers::{self, Customer, CustomerDetail};
use crate::repo::{audit, shop, SqlResultExt};
use crate::state::AppState;

/// Mirrors `CustomerSearchSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct CustomerSearch {
    pub query: String,
    pub limit: i64,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn search_customers(
    state: State<'_, AppState>,
    search: CustomerSearch,
) -> IpcResult<Vec<Customer>> {
    let auth = authorize(&state, Permission::CustomerLookup)?;
    blocking(move || customers::search(&auth.db.conn(), &search.query, search.limit).ipc()).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_customer(
    state: State<'_, AppState>,
    customer_id: Uuid,
) -> IpcResult<CustomerDetail> {
    let auth = authorize(&state, Permission::CustomerLookup)?;
    blocking(move || {
        let conn = auth.db.conn();
        let customer = loyalty::live_customer(&conn, customer_id)?;
        customers::detail(&conn, customer).ipc()
    })
    .await
}

/// Registering is part of selling; changing an existing customer is not.
#[tauri::command(rename_all = "snake_case")]
pub async fn save_customer(
    state: State<'_, AppState>,
    customer: CustomerInput,
) -> IpcResult<Customer> {
    let permission = if customer.id.is_some() {
        Permission::CustomerManage
    } else {
        Permission::CustomerLookup
    };
    let auth = authorize(&state, permission)?;
    blocking(move || {
        let actor = auth.actor()?;
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        let saved = loyalty::save(&tx, &actor, customer, SystemClock.now())?;
        tx.commit().ipc()?;
        Ok(saved)
    })
    .await
    .inspect(|_| state.sync.nudge())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_customer(state: State<'_, AppState>, customer_id: Uuid) -> IpcResult<()> {
    let auth = authorize(&state, Permission::CustomerManage)?;
    blocking(move || {
        let actor = auth.actor()?;
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        loyalty::delete(&tx, &actor, customer_id, SystemClock.now())?;
        tx.commit().ipc()
    })
    .await
    .inspect(|()| state.sync.nudge())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn adjust_loyalty_points(
    state: State<'_, AppState>,
    adjustment: PointsAdjustment,
) -> IpcResult<Customer> {
    let auth = authorize(&state, Permission::CustomerManage)?;
    blocking(move || {
        let actor = auth.actor()?;
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        let customer = loyalty::adjust(&tx, &actor, &adjustment, SystemClock.now())?;
        tx.commit().ipc()?;
        Ok(customer)
    })
    .await
    .inspect(|_| state.sync.nudge())
}

/// Mirrors `LoyaltyProgramSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct LoyaltyProgram {
    pub available: bool,
    pub settings: LoyaltySettings,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_loyalty_settings(state: State<'_, AppState>) -> IpcResult<LoyaltyProgram> {
    let auth = authorize(&state, Permission::SaleCreate)?;
    let client = Arc::clone(&state.client);
    blocking(move || {
        Ok(LoyaltyProgram {
            available: client.features.loyalty,
            settings: shop::loyalty(&auth.db.conn(), &client).ipc()?,
        })
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn save_loyalty_settings(
    state: State<'_, AppState>,
    settings: LoyaltySettings,
) -> IpcResult<LoyaltyProgram> {
    let auth = authorize(&state, Permission::SettingsManage)?;
    settings.validate().map_err(IpcError::validation)?;
    let client = Arc::clone(&state.client);
    blocking(move || {
        let actor = auth.actor()?;
        let now = SystemClock.now();
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        let before = shop::loyalty(&tx, &client).ipc()?;
        shop::save_loyalty(&tx, &settings, now).ipc()?;
        audit::record(
            &tx,
            &actor,
            "settings.loyalty",
            "shop_settings",
            Some(shop::LOYALTY_ID),
            serde_json::to_value(before).ok(),
            serde_json::to_value(settings).ok(),
            now,
        )
        .ipc()?;
        tx.commit().ipc()?;
        Ok(LoyaltyProgram {
            available: client.features.loyalty,
            settings,
        })
    })
    .await
    .inspect(|_| state.sync.nudge())
}

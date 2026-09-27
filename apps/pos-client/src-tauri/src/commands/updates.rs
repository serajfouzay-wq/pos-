//! Updates. Reading the status and checking: any signed-in user. Installing
//! now restarts the till: `shift.close` (managers and owners). Otherwise a
//! downloaded update installs when the till is next closed.

use std::sync::Arc;

use pos_core::rbac::Permission;
use pos_core::time::{Clock, SystemClock};
use pos_core::IpcResult;
use tauri::{AppHandle, State};

use super::authorize;
use crate::state::AppState;
use crate::updater::UpdateStatus;

#[tauri::command(rename_all = "snake_case")]
pub async fn update_status(state: State<'_, AppState>) -> IpcResult<UpdateStatus> {
    authorize(&state, Permission::SaleCreate)?;
    Ok(state.updates.status())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn check_for_updates(
    app: AppHandle,
    state: State<'_, AppState>,
) -> IpcResult<UpdateStatus> {
    authorize(&state, Permission::SaleCreate)?;
    let (updates, license, client) = (
        Arc::clone(&state.updates),
        Arc::clone(&state.license),
        Arc::clone(&state.client),
    );
    Ok(updates.check(&app, &license, &client).await)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn install_update(state: State<'_, AppState>) -> IpcResult<()> {
    authorize(&state, Permission::ShiftClose)?;
    state.updates.install_now()
}

#[tauri::command(rename_all = "snake_case")]
pub async fn dismiss_update_notice(state: State<'_, AppState>) -> IpcResult<UpdateStatus> {
    let auth = authorize(&state, Permission::SaleCreate)?;
    state.updates.dismiss(&auth.db, SystemClock.now())
}

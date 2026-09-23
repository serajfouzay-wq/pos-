//! Cloud sync. Any signed-in user may run a round or read the status (the
//! status-bar indicator); what is synced is decided by the outbox, not the UI.

use std::sync::Arc;

use pos_core::rbac::Permission;
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use tauri::State;

use super::{authorize, blocking};
use crate::state::AppState;
use crate::sync::{self, SyncError, SyncReport, SyncStatus};

fn ipc_error(err: SyncError) -> IpcError {
    match err {
        SyncError::Offline(_) => IpcError::new(IpcErrorCode::Offline, "The cloud is unreachable."),
        SyncError::Unauthorized(reason) => IpcError::new(
            IpcErrorCode::Forbidden,
            format!("The cloud refused this till: {reason}"),
        ),
        SyncError::Protocol(reason) | SyncError::Local(reason) => {
            IpcError::internal(format!("sync failed: {reason}"))
        }
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn sync_to_cloud(state: State<'_, AppState>) -> IpcResult<SyncReport> {
    authorize(&state, Permission::CatalogView)?;
    let (license, engine) = (Arc::clone(&state.license), Arc::clone(&state.sync));
    blocking(move || sync::round(&license, &engine).map_err(ipc_error)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn sync_status(state: State<'_, AppState>) -> IpcResult<SyncStatus> {
    let auth = authorize(&state, Permission::CatalogView)?;
    let engine = Arc::clone(&state.sync);
    blocking(move || Ok(engine.status(&auth.db))).await
}

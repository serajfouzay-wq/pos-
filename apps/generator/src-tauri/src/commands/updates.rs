//! The update signing key: status, backup and restore. The secret half
//! leaves the credential store only as the backup file the operator asks
//! for; it never reaches the webview.

use std::sync::Arc;

use pos_core::time::{Clock, SystemClock};
use pos_core::{IpcError, IpcResult};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use super::blocking;
use crate::state::AppState;
use crate::store::Store;
use crate::updates::{self, UpdateKey, UpdateKeyStatus};

/// When the operator last saved a backup file (the Start checklist shows it).
const BACKED_UP_AT: &str = "update_key.backed_up_at";

fn status_with_backup(key: &UpdateKey, store: &Store) -> IpcResult<UpdateKeyStatus> {
    let mut status = key.status()?;
    if status.configured {
        status.backed_up_at = store.setting(BACKED_UP_AT)?;
    }
    Ok(status)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn update_key_status(state: State<'_, AppState>) -> IpcResult<UpdateKeyStatus> {
    let (key, store) = (Arc::clone(&state.update_key), Arc::clone(&state.store));
    blocking(move || status_with_backup(&key, &store)).await
}

/// Creates the key now (otherwise the first build does).
#[tauri::command(rename_all = "snake_case")]
pub async fn create_update_key(state: State<'_, AppState>) -> IpcResult<UpdateKeyStatus> {
    let (key, store) = (Arc::clone(&state.update_key), Arc::clone(&state.store));
    blocking(move || {
        key.ensure()?;
        status_with_backup(&key, &store)
    })
    .await
}

/// Saves the backup under `<Downloads>/POS Factory/` and shows it; returns
/// its path.
#[tauri::command(rename_all = "snake_case")]
pub async fn export_update_key(app: AppHandle, state: State<'_, AppState>) -> IpcResult<String> {
    let (key, store) = (Arc::clone(&state.update_key), Arc::clone(&state.store));
    let dir = state.downloads.join("POS Factory");
    let path = blocking(move || {
        let status = key.status()?;
        let backup = key.backup()?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| IpcError::internal(format!("create {}: {e}", dir.display())))?;
        let id = status.key_id.unwrap_or_default();
        let path = dir.join(format!("update-key-backup-{id}.txt"));
        updates::write_file(&path, backup.as_bytes())?;
        let now = SystemClock.now();
        store.put_setting(BACKED_UP_AT, &now, now)?;
        Ok(path)
    })
    .await?;
    let _ = app.opener().reveal_item_in_dir(&path);
    Ok(path.display().to_string())
}

/// Puts a backed-up key on this PC (the text of the backup file).
#[tauri::command(rename_all = "snake_case")]
pub async fn restore_update_key(
    state: State<'_, AppState>,
    backup: String,
) -> IpcResult<UpdateKeyStatus> {
    let (key, store) = (Arc::clone(&state.update_key), Arc::clone(&state.store));
    blocking(move || {
        key.restore(&backup)?;
        status_with_backup(&key, &store)
    })
    .await
}

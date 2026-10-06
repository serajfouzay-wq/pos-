//! Updates. Reading the status and checking: any signed-in user. Installing
//! now restarts the till: `shift.close` (managers and owners). Otherwise a
//! downloaded update installs when the till is next closed. Update files
//! (from a USB stick) are checked by managers and owners too.

use std::sync::Arc;

use pos_core::rbac::Permission;
use pos_core::time::{Clock, SystemClock};
use pos_core::IpcResult;
use tauri::{AppHandle, State};

use super::{authorize, blocking};
use crate::state::AppState;
use crate::updater::offline::UpdateFileInfo;
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
    let auth = authorize(&state, Permission::ShiftClose)?;
    // Whatever the new version does with the data, today's is kept.
    let backups = Arc::clone(&state.backups);
    blocking(move || {
        backups
            .create(&auth.db, crate::backup::Reason::BeforeUpdate)
            .map(|_| ())
    })
    .await?;
    state.updates.install_now()
}

#[tauri::command(rename_all = "snake_case")]
pub async fn dismiss_update_notice(state: State<'_, AppState>) -> IpcResult<UpdateStatus> {
    let auth = authorize(&state, Permission::SaleCreate)?;
    state.updates.dismiss(&auth.db, SystemClock.now())
}

/// Update files on USB sticks and in Downloads (newest first), so nobody has
/// to browse for them.
#[tauri::command(rename_all = "snake_case")]
pub async fn find_update_files(
    app: AppHandle,
    state: State<'_, AppState>,
) -> IpcResult<Vec<String>> {
    use tauri::Manager;
    authorize(&state, Permission::ShiftClose)?;
    let downloads = app.path().download_dir().ok();
    blocking(move || {
        let roots = crate::updater::offline::search_roots(downloads);
        // The generator names each file for its system: skip the other one.
        let other = if cfg!(windows) {
            "-linux."
        } else {
            "-windows."
        };
        Ok(crate::updater::offline::find_files(&roots, "posupdate")
            .into_iter()
            .map(|p| p.display().to_string())
            .filter(|p| !p.to_lowercase().contains(other))
            .collect())
    })
    .await
}

/// Opens and verifies an update file; installs nothing.
#[tauri::command(rename_all = "snake_case")]
pub async fn inspect_update_file(
    state: State<'_, AppState>,
    path: String,
) -> IpcResult<UpdateFileInfo> {
    authorize(&state, Permission::ShiftClose)?;
    let (updates, client) = (Arc::clone(&state.updates), Arc::clone(&state.client));
    blocking(move || {
        updates
            .inspect_file(std::path::Path::new(&path), &client)
            .map(|u| u.info)
    })
    .await
}

/// Backs up today's data, starts the update file's installer, and closes
/// the till (it restarts on the new version).
#[tauri::command(rename_all = "snake_case")]
pub async fn install_update_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> IpcResult<()> {
    let auth = authorize(&state, Permission::ShiftClose)?;
    let (updates, client, backups) = (
        Arc::clone(&state.updates),
        Arc::clone(&state.client),
        Arc::clone(&state.backups),
    );
    blocking(move || {
        // Verified again: the file may have changed since it was inspected.
        let update = updates.inspect_file(std::path::Path::new(&path), &client)?;
        if update.info.newer {
            backups.create(&auth.db, crate::backup::Reason::BeforeUpdate)?;
        }
        updates.install_file(&auth.db, &update, SystemClock.now())
    })
    .await?;
    app.exit(0);
    Ok(())
}

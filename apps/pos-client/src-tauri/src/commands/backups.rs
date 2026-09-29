//! Backups. Status, settings, the backup password and restoring: the owner
//! (`settings.manage`). A backup now: managers too (`shift.close`, they
//! close the day). When the database cannot be opened at all (the license
//! gate halts with a storage error) restoring needs no sign-in: there is no
//! user table left to sign in against, and the backup is still encrypted.

use std::path::PathBuf;
use std::sync::Arc;

use pos_core::rbac::Permission;
use pos_core::time::{Clock, SystemClock};
use pos_core::{IpcError, IpcResult};
use pos_license::status::{HaltState, LicenseStatus};
use serde::Deserialize;
use tauri::{AppHandle, State};

use super::{authorize, blocking};
use crate::backup::{BackupInfo, BackupMeta, BackupService, BackupSettings, BackupStatus, Reason};
use crate::repo::{audit, settings, SqlResultExt};
use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub async fn backup_status(state: State<'_, AppState>) -> IpcResult<BackupStatus> {
    let auth = authorize(&state, Permission::SettingsManage)?;
    let backups = Arc::clone(&state.backups);
    blocking(move || backups.status(&auth.db)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn backup_now(state: State<'_, AppState>) -> IpcResult<BackupInfo> {
    let auth = authorize(&state, Permission::ShiftClose)?;
    let backups = Arc::clone(&state.backups);
    blocking(move || backups.create(&auth.db, Reason::Manual)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn save_backup_settings(
    state: State<'_, AppState>,
    settings: BackupSettings,
) -> IpcResult<BackupStatus> {
    let auth = authorize(&state, Permission::SettingsManage)?;
    settings.validate()?;
    let backups = Arc::clone(&state.backups);
    blocking(move || {
        let actor = auth.actor()?;
        let now = SystemClock.now();
        {
            let conn = auth.db.conn();
            let before = BackupService::settings(&auth.db)?;
            settings::put(&conn, crate::backup::SETTINGS_KEY, &settings, now).ipc()?;
            audit::record(
                &conn,
                &actor,
                "settings.manage",
                "settings",
                None,
                serde_json::to_value(&before).ok(),
                serde_json::to_value(&settings).ok(),
                now,
            )
            .ipc()?;
        }
        backups.status(&auth.db)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn set_backup_password(
    state: State<'_, AppState>,
    password: String,
) -> IpcResult<BackupStatus> {
    let auth = authorize(&state, Permission::SettingsManage)?;
    let backups = Arc::clone(&state.backups);
    blocking(move || {
        backups.set_password(&auth.db, &password)?;
        let actor = auth.actor()?;
        audit::record(
            &auth.db.conn(),
            &actor,
            "settings.manage",
            "settings",
            None,
            None,
            Some(serde_json::json!({ "backup_password": "changed" })),
            SystemClock.now(),
        )
        .ipc()?;
        // The next backup is the first that restores on another PC.
        let _ = backups.create(&auth.db, Reason::Manual);
        backups.status(&auth.db)
    })
    .await
}

fn storage_halted(state: &AppState) -> bool {
    matches!(
        state.license.status(),
        LicenseStatus::Halted(h) if h.state == HaltState::StorageError
    )
}

/// Backups found in a folder (a USB stick), newest first.
#[tauri::command(rename_all = "snake_case")]
pub async fn list_backups_in(
    state: State<'_, AppState>,
    dir: String,
) -> IpcResult<Vec<BackupInfo>> {
    if !storage_halted(&state) {
        authorize(&state, Permission::SettingsManage)?;
    }
    let backups = Arc::clone(&state.backups);
    blocking(move || Ok(backups.list(Some(&PathBuf::from(dir))))).await
}

/// Mirrors `RestoreRequestSchema`.
#[derive(Debug, Deserialize)]
pub struct RestoreRequest {
    pub path: String,
    pub password: Option<String>,
}

/// Stages a backup to replace the database; it takes effect when the app
/// restarts (`restart_app`). A backup of the current state is made first.
#[tauri::command(rename_all = "snake_case")]
pub async fn restore_backup(
    state: State<'_, AppState>,
    request: RestoreRequest,
) -> IpcResult<BackupMeta> {
    let auth = if storage_halted(&state) {
        None
    } else {
        Some(authorize(&state, Permission::SettingsManage)?)
    };
    let machine_key = state.license.machine_key().ok_or_else(|| {
        IpcError::validation("This PC's hardware cannot be read, so no backup can be restored.")
    })?;
    let backups = Arc::clone(&state.backups);
    blocking(move || {
        if let Some(auth) = &auth {
            // Whatever happens next, today's data is kept.
            backups.create(&auth.db, Reason::BeforeRestore)?;
        }
        let meta = backups.stage_restore(
            &PathBuf::from(&request.path),
            request.password.as_deref().filter(|p| !p.is_empty()),
            &machine_key,
        )?;
        if let Some(auth) = &auth {
            let actor = auth.actor()?;
            audit::record(
                &auth.db.conn(),
                &actor,
                "settings.manage",
                "backup",
                None,
                None,
                serde_json::to_value(&meta).ok(),
                SystemClock.now(),
            )
            .ipc()?;
        }
        Ok(meta)
    })
    .await
}

/// Restarts the app (after staging a restore).
#[tauri::command(rename_all = "snake_case")]
pub async fn restart_app(app: AppHandle, state: State<'_, AppState>) -> IpcResult<()> {
    if !storage_halted(&state) {
        authorize(&state, Permission::SettingsManage)?;
    }
    app.restart()
}

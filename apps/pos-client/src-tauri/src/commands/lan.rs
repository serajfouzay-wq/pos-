//! The shop network (tills syncing with a hub on the local network, no
//! internet). Set up by the owner (`settings.manage`), per till.

use std::sync::Arc;

use pos_core::rbac::Permission;
use pos_core::time::{Clock, SystemClock};
use pos_core::{IpcError, IpcResult};
use tauri::State;

use super::{authorize, blocking};
use crate::repo::{audit, settings, SqlResultExt};
use crate::state::AppState;
use crate::sync::lan::{
    self, FoundHub, HubHello, LanRole, LanService, LanSettings, LanStatus, LanTransport,
};

#[tauri::command(rename_all = "snake_case")]
pub async fn lan_status(state: State<'_, AppState>) -> IpcResult<LanStatus> {
    let auth = authorize(&state, Permission::SettingsManage)?;
    let service = Arc::clone(&state.lan);
    blocking(move || service.status(&auth.db)).await
}

/// Saves this till's role and applies it at once (starts or stops the hub,
/// points sync at the hub or back at the cloud).
#[tauri::command(rename_all = "snake_case")]
pub async fn save_lan_settings(
    state: State<'_, AppState>,
    settings: LanSettings,
) -> IpcResult<LanStatus> {
    let auth = authorize(&state, Permission::SettingsManage)?;
    if settings.port < 1024 {
        return Err(IpcError::validation("Use a port from 1024 to 65534."));
    }
    let settings = LanSettings {
        hub_address: match (&settings.role, &settings.hub_address) {
            (LanRole::Client, Some(address)) => {
                Some(lan::normalize_address(address, settings.port)?)
            }
            (LanRole::Client, None) => {
                return Err(IpcError::validation("Enter the hub's address."))
            }
            _ => settings.hub_address.clone(),
        },
        hub_code: settings
            .hub_code
            .as_deref()
            .map(lan::normalize_code)
            .filter(|c| !c.is_empty()),
        ..settings
    };
    if settings.role == LanRole::Client && settings.hub_code.is_none() {
        return Err(IpcError::validation(
            "Enter the pairing code shown on the hub.",
        ));
    }
    let (service, license, sync) = (
        Arc::clone(&state.lan),
        Arc::clone(&state.license),
        Arc::clone(&state.sync),
    );
    blocking(move || {
        let actor = auth.actor()?;
        let now = SystemClock.now();
        {
            let conn = auth.db.conn();
            let before = LanService::settings(&auth.db)?;
            settings::put(&conn, lan::SETTINGS_KEY, &settings, now).ipc()?;
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
        service.apply(&auth.db, &license, &sync)?;
        service.status(&auth.db)
    })
    .await
    .inspect(|_| state.sync.nudge())
}

/// Looks for this shop's hub on the local network (1.5 s).
#[tauri::command(rename_all = "snake_case")]
pub async fn discover_hubs(state: State<'_, AppState>, port: u16) -> IpcResult<Vec<FoundHub>> {
    authorize(&state, Permission::SettingsManage)?;
    let client_id = state.client.client_id;
    blocking(move || Ok(lan::discover(client_id, port))).await
}

/// Contacts a hub with a code before saving.
#[tauri::command(rename_all = "snake_case")]
pub async fn test_hub(
    state: State<'_, AppState>,
    address: String,
    code: String,
    port: u16,
) -> IpcResult<HubHello> {
    authorize(&state, Permission::SettingsManage)?;
    let client_id = state.client.client_id;
    blocking(move || {
        let address = lan::normalize_address(&address, port)?;
        LanTransport::new(&address, client_id, &code)
            .hello()
            .map_err(|e| IpcError::validation(format!("The hub did not accept this till: {e}")))
    })
    .await
}

/// A new pairing code on the hub (every till must enter it again).
#[tauri::command(rename_all = "snake_case")]
pub async fn new_hub_code(state: State<'_, AppState>) -> IpcResult<LanStatus> {
    let auth = authorize(&state, Permission::SettingsManage)?;
    let (service, license, sync) = (
        Arc::clone(&state.lan),
        Arc::clone(&state.license),
        Arc::clone(&state.sync),
    );
    blocking(move || {
        service.new_code(&auth.db)?;
        service.apply(&auth.db, &license, &sync)?;
        service.status(&auth.db)
    })
    .await
}

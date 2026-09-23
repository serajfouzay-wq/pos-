use pos_core::config::ClientConfig;
use pos_core::IpcResult;
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::state::{AppState, LICENSE_KEY_ID, LICENSE_KEY_IS_DEV};

/// Mirrors `PosAppInfoSchema`.
#[derive(Debug, Serialize)]
pub struct AppInfo {
    app: &'static str,
    version: String,
    build_profile: &'static str,
    target: &'static str,
    client: ClientConfig,
    dev_license_key: bool,
    license_key_id: &'static str,
}

/// Public, unauthenticated: identifies the build so the UI can theme itself
/// and pick the business-type layout. Contains no secrets.
#[tauri::command(rename_all = "snake_case")]
pub fn app_info(app: AppHandle, state: State<'_, AppState>) -> IpcResult<AppInfo> {
    Ok(AppInfo {
        app: "pos-client",
        version: app.package_info().version.to_string(),
        build_profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        target: env!("POS_TARGET_TRIPLE"),
        client: (*state.client).clone(),
        dev_license_key: LICENSE_KEY_IS_DEV,
        license_key_id: LICENSE_KEY_ID,
    })
}

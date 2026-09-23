//! License commands. All three are pre-authentication: they run before any
//! user can log in, and none of them exposes business data.

use std::sync::Arc;

use pos_core::{IpcError, IpcErrorCode, IpcResult};
use pos_license::LicenseStatus;
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use super::blocking;
use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub async fn verify_license(state: State<'_, AppState>) -> IpcResult<LicenseStatus> {
    let license = Arc::clone(&state.license);
    blocking(move || Ok(license.evaluate())).await
}

/// Mirrors `ActivationRequestInfoSchema`.
#[derive(Debug, Serialize)]
pub struct ActivationRequestInfo {
    code: String,
    client_id: Uuid,
    device_name: String,
    fingerprint: String,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_activation_request(
    state: State<'_, AppState>,
) -> IpcResult<ActivationRequestInfo> {
    let license = Arc::clone(&state.license);
    blocking(move || {
        let request = license
            .activation_request()
            .map_err(|e| IpcError::new(IpcErrorCode::Hardware, e.to_string()))?;
        Ok(ActivationRequestInfo {
            code: request.encode(),
            client_id: request.client_id,
            device_name: request.device_name,
            fingerprint: request.fingerprint,
        })
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn activate_license(
    state: State<'_, AppState>,
    token: String,
) -> IpcResult<LicenseStatus> {
    if token.len() > pos_license::jwt::MAX_TOKEN_LEN {
        return Err(IpcError::validation("license token is too long"));
    }
    let license = Arc::clone(&state.license);
    let status = blocking({
        let license = Arc::clone(&license);
        move || Ok(license.activate(&token))
    })
    .await?;
    if status.is_valid() {
        // Register with the cloud straight away; the result arrives as an event.
        tauri::async_runtime::spawn_blocking(move || license.cloud_check());
    }
    Ok(status)
}

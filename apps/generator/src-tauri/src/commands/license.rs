//! License-issuing commands. Key generation and scrypt run on the blocking pool.

use std::sync::Arc;

use pos_core::time::{Clock, SystemClock};
use pos_core::{IpcError, IpcResult};
use pos_license::activation::ActivationRequest;
use tauri::State;
use uuid::Uuid;

use super::blocking;
use crate::signing::{IssueLicenseRequest, IssuedLicense, SigningKeyStatus};
use crate::state::AppState;
use crate::store::IssuedLicenseRecord;

#[tauri::command(rename_all = "snake_case")]
pub fn license_key_status(state: State<'_, AppState>) -> IpcResult<SigningKeyStatus> {
    state.keys.status()
}

#[tauri::command(rename_all = "snake_case")]
pub async fn create_license_key(
    state: State<'_, AppState>,
    passphrase: String,
) -> IpcResult<SigningKeyStatus> {
    let keys = Arc::clone(&state.keys);
    blocking(move || keys.create(&passphrase)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn unlock_license_key(
    state: State<'_, AppState>,
    passphrase: String,
) -> IpcResult<SigningKeyStatus> {
    let keys = Arc::clone(&state.keys);
    blocking(move || keys.unlock(&passphrase)).await
}

#[tauri::command(rename_all = "snake_case")]
pub fn lock_license_key(state: State<'_, AppState>) -> IpcResult<SigningKeyStatus> {
    state.keys.lock()
}

#[tauri::command(rename_all = "snake_case")]
pub fn decode_activation_request(code: String) -> IpcResult<ActivationRequest> {
    ActivationRequest::decode(&code).map_err(|e| IpcError::validation(e.to_string()))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn issue_license(
    state: State<'_, AppState>,
    request: IssueLicenseRequest,
) -> IpcResult<IssuedLicense> {
    let (keys, store) = (Arc::clone(&state.keys), Arc::clone(&state.store));
    blocking(move || keys.issue(&store, &request, SystemClock.now())).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_issued_licenses(
    state: State<'_, AppState>,
    client_id: Uuid,
) -> IpcResult<Vec<IssuedLicenseRecord>> {
    let store = Arc::clone(&state.store);
    blocking(move || store.licenses(client_id)).await
}

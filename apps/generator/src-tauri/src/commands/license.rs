//! License-issuing commands. Key generation and scrypt run on the blocking pool.

use std::sync::Arc;

use pos_core::time::{Clock, SystemClock};
use pos_core::{IpcError, IpcResult};
use pos_license::activation::ActivationRequest;
use tauri::State;

use crate::signing::{IssueLicenseRequest, IssuedLicense, KeyStore, SigningKeyStatus};

async fn blocking<T, F>(f: F) -> IpcResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> IpcResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| IpcError::internal(format!("background task failed: {e}")))?
}

#[tauri::command(rename_all = "snake_case")]
pub fn license_key_status(keys: State<'_, Arc<KeyStore>>) -> IpcResult<SigningKeyStatus> {
    keys.status()
}

#[tauri::command(rename_all = "snake_case")]
pub async fn create_license_key(
    keys: State<'_, Arc<KeyStore>>,
    passphrase: String,
) -> IpcResult<SigningKeyStatus> {
    let keys = Arc::clone(&keys);
    blocking(move || keys.create(&passphrase)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn unlock_license_key(
    keys: State<'_, Arc<KeyStore>>,
    passphrase: String,
) -> IpcResult<SigningKeyStatus> {
    let keys = Arc::clone(&keys);
    blocking(move || keys.unlock(&passphrase)).await
}

#[tauri::command(rename_all = "snake_case")]
pub fn lock_license_key(keys: State<'_, Arc<KeyStore>>) -> IpcResult<SigningKeyStatus> {
    keys.lock()
}

#[tauri::command(rename_all = "snake_case")]
pub fn decode_activation_request(code: String) -> IpcResult<ActivationRequest> {
    ActivationRequest::decode(&code).map_err(|e| IpcError::validation(e.to_string()))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn issue_license(
    keys: State<'_, Arc<KeyStore>>,
    request: IssueLicenseRequest,
) -> IpcResult<IssuedLicense> {
    let keys = Arc::clone(&keys);
    blocking(move || keys.issue(&request, SystemClock.now())).await
}

//! Client registry: configs, uploaded assets and the receipt preview.

use std::sync::Arc;

use pos_core::config::ClientConfig;
use pos_core::time::{Clock, SystemClock};
use pos_core::{IpcError, IpcResult};
use tauri::State;
use uuid::Uuid;

use super::blocking;
use crate::clients::{
    decode_base64, encode_base64, new_client_config, preview_receipt as render_preview,
    validate_asset, NewClientInput, ReceiptPreview,
};
use crate::state::AppState;
use crate::store::{AssetKind, ClientDetail, ClientSummary};

#[tauri::command(rename_all = "snake_case")]
pub async fn list_clients(state: State<'_, AppState>) -> IpcResult<Vec<ClientSummary>> {
    let store = Arc::clone(&state.store);
    blocking(move || store.list_clients()).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_client(state: State<'_, AppState>, client_id: Uuid) -> IpcResult<ClientDetail> {
    let store = Arc::clone(&state.store);
    blocking(move || store.client(client_id)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn create_client(
    state: State<'_, AppState>,
    input: NewClientInput,
) -> IpcResult<ClientDetail> {
    let store = Arc::clone(&state.store);
    blocking(move || {
        let config = new_client_config(&input);
        store.create_client(&config, SystemClock.now())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn save_client(
    state: State<'_, AppState>,
    client_id: Uuid,
    config: ClientConfig,
    notes: String,
) -> IpcResult<ClientDetail> {
    let store = Arc::clone(&state.store);
    blocking(move || store.save_client(client_id, config, &notes, SystemClock.now())).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn archive_client(state: State<'_, AppState>, client_id: Uuid) -> IpcResult<()> {
    let store = Arc::clone(&state.store);
    blocking(move || store.archive_client(client_id, SystemClock.now())).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn upload_client_asset(
    state: State<'_, AppState>,
    client_id: Uuid,
    kind: AssetKind,
    data_base64: String,
) -> IpcResult<ClientDetail> {
    // ~5.4 MB of base64 covers the 4 MB icon limit; refuse anything bigger
    // before decoding.
    if data_base64.len() > 6 * 1024 * 1024 {
        return Err(IpcError::validation("The image is too large."));
    }
    let store = Arc::clone(&state.store);
    blocking(move || {
        let bytes = decode_base64(&data_base64)?;
        let size = validate_asset(kind, &bytes)?;
        store.put_asset(client_id, kind, &bytes, size, SystemClock.now())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn remove_client_asset(
    state: State<'_, AppState>,
    client_id: Uuid,
    kind: AssetKind,
) -> IpcResult<ClientDetail> {
    let store = Arc::clone(&state.store);
    blocking(move || store.remove_asset(client_id, kind, SystemClock.now())).await
}

/// The uploaded image itself (for thumbnails), or `null`.
#[tauri::command(rename_all = "snake_case")]
pub async fn get_client_asset(
    state: State<'_, AppState>,
    client_id: Uuid,
    kind: AssetKind,
) -> IpcResult<Option<String>> {
    let store = Arc::clone(&state.store);
    blocking(move || Ok(store.asset(client_id, kind)?.map(|b| encode_base64(&b)))).await
}

/// Renders a sample receipt from a draft config (unsaved edits included)
/// with the client's uploaded logo, exactly as a till would print it.
#[tauri::command(rename_all = "snake_case")]
pub async fn preview_receipt(
    state: State<'_, AppState>,
    client_id: Uuid,
    config: ClientConfig,
) -> IpcResult<ReceiptPreview> {
    config
        .validate()
        .map_err(|e| IpcError::validation(e.to_string()))?;
    let store = Arc::clone(&state.store);
    blocking(move || {
        let logo = store.asset(client_id, AssetKind::ReceiptLogo)?;
        render_preview(&config, logo.as_deref(), SystemClock.now())
    })
    .await
}

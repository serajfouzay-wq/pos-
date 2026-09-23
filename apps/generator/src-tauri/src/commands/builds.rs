//! Build repository settings and client builds (GitHub Actions).

use std::path::PathBuf;
use std::sync::Arc;

use pos_core::time::{Clock, SystemClock};
use pos_core::{IpcError, IpcResult};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

use super::blocking;
use crate::builds::{BuildSettings, BuildSettingsView};
use crate::github::RepoCheck;
use crate::state::AppState;
use crate::store::BuildRecord;

#[tauri::command(rename_all = "snake_case")]
pub async fn get_build_settings(state: State<'_, AppState>) -> IpcResult<BuildSettingsView> {
    let builds = Arc::clone(&state.builds);
    blocking(move || builds.settings()).await
}

/// `github_token: null` keeps the stored token.
#[tauri::command(rename_all = "snake_case")]
pub async fn save_build_settings(
    state: State<'_, AppState>,
    settings: BuildSettings,
    github_token: Option<String>,
) -> IpcResult<BuildSettingsView> {
    let builds = Arc::clone(&state.builds);
    blocking(move || builds.save_settings(&settings, github_token.as_deref(), SystemClock.now()))
        .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn clear_github_token(state: State<'_, AppState>) -> IpcResult<BuildSettingsView> {
    let builds = Arc::clone(&state.builds);
    blocking(move || builds.clear_token()).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn check_build_settings(state: State<'_, AppState>) -> IpcResult<RepoCheck> {
    let builds = Arc::clone(&state.builds);
    blocking(move || builds.check()).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn start_build(state: State<'_, AppState>, client_id: Uuid) -> IpcResult<BuildRecord> {
    let builds = Arc::clone(&state.builds);
    blocking(move || builds.start(client_id, SystemClock.now())).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_builds(
    state: State<'_, AppState>,
    client_id: Option<Uuid>,
) -> IpcResult<Vec<BuildRecord>> {
    let store = Arc::clone(&state.store);
    blocking(move || store.builds(client_id, 100)).await
}

/// Follows every build still in flight; returns the refreshed ones.
#[tauri::command(rename_all = "snake_case")]
pub async fn refresh_builds(state: State<'_, AppState>) -> IpcResult<Vec<BuildRecord>> {
    let builds = Arc::clone(&state.builds);
    blocking(move || builds.refresh_active(SystemClock.now())).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn download_build(state: State<'_, AppState>, build_id: Uuid) -> IpcResult<BuildRecord> {
    let builds = Arc::clone(&state.builds);
    blocking(move || builds.download(build_id, SystemClock.now())).await
}

/// Opens the build's GitHub Actions run in the default browser.
#[tauri::command(rename_all = "snake_case")]
pub async fn open_build_run(
    app: AppHandle,
    state: State<'_, AppState>,
    build_id: Uuid,
) -> IpcResult<()> {
    let store = Arc::clone(&state.store);
    let url = blocking(move || store.build(build_id).map(|b| b.run_url)).await?;
    let url = url
        .filter(|u| u.starts_with("https://"))
        .ok_or_else(|| IpcError::validation("This build has no GitHub run yet."))?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| IpcError::internal(e.to_string()))
}

/// Shows the downloaded installer in the file manager.
#[tauri::command(rename_all = "snake_case")]
pub async fn reveal_build_download(
    app: AppHandle,
    state: State<'_, AppState>,
    build_id: Uuid,
) -> IpcResult<()> {
    let store = Arc::clone(&state.store);
    let path = blocking(move || store.build(build_id).map(|b| b.download_path)).await?;
    let path = path
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .ok_or_else(|| IpcError::validation("Download the installer first."))?;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| IpcError::internal(e.to_string()))
}

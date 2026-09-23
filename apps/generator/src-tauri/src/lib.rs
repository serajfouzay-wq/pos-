//! POS Factory generator core: the client registry, license signing and
//! client builds on GitHub Actions — all behind typed IPC commands. The
//! signing key and the GitHub token never reach the webview.

mod builds;
mod clients;
mod commands;
#[cfg(test)]
mod contract_tests;
mod github;
mod secrets;
mod signing;
mod state;
mod store;

use std::sync::Arc;

use tauri::Manager;

use crate::builds::BuildService;
use crate::github::HttpGitHub;
use crate::signing::KeyStore;
use crate::state::AppState;
use crate::store::Store;

const WORKSPACE_FILE: &str = "generator.db";

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        // Used from Rust only (open a run page, reveal a download); the
        // webview is granted no opener permission.
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            let keys = Arc::new(KeyStore::new(data.join("keys")));
            let store = Arc::new(Store::open(&data.join(WORKSPACE_FILE)).map_err(|e| e.message)?);
            let downloads = app
                .path()
                .download_dir()
                .unwrap_or_else(|_| data.join("downloads"));
            let builds = Arc::new(BuildService::new(
                Arc::clone(&store),
                Arc::new(HttpGitHub::new()),
                secrets::github_token_store(data.clone()),
                Arc::clone(&keys),
                app.package_info().version.to_string(),
                downloads,
            ));
            app.manage(AppState {
                store,
                keys,
                builds,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info::app_info,
            commands::license::license_key_status,
            commands::license::create_license_key,
            commands::license::unlock_license_key,
            commands::license::lock_license_key,
            commands::license::decode_activation_request,
            commands::license::issue_license,
            commands::license::list_issued_licenses,
            commands::clients::list_clients,
            commands::clients::get_client,
            commands::clients::create_client,
            commands::clients::save_client,
            commands::clients::archive_client,
            commands::clients::upload_client_asset,
            commands::clients::remove_client_asset,
            commands::clients::get_client_asset,
            commands::clients::preview_receipt,
            commands::builds::get_build_settings,
            commands::builds::save_build_settings,
            commands::builds::clear_github_token,
            commands::builds::check_build_settings,
            commands::builds::start_build,
            commands::builds::list_builds,
            commands::builds::refresh_builds,
            commands::builds::download_build,
            commands::builds::open_build_run,
            commands::builds::reveal_build_download,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the POS Factory generator");
}

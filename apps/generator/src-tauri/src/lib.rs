//! POS Factory generator core. Client management and GitHub Actions build
//! triggers arrive in Phase 5 — all behind typed IPC commands. License signing
//! (Phase 2) keeps the private key inside this process.

mod commands;
mod signing;

use std::sync::Arc;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            let dir = app.path().app_data_dir()?.join("keys");
            app.manage(Arc::new(signing::KeyStore::new(dir)));
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
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the POS Factory generator");
}

//! POS Factory generator core. Later phases add client management, license
//! signing (the RSA private key never leaves this process) and GitHub Actions
//! build triggers — all behind typed IPC commands.

mod commands;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .invoke_handler(tauri::generate_handler![commands::app_info::app_info])
        .run(tauri::generate_context!())
        .expect("failed to start the POS Factory generator");
}

//! POS client core. The React frontend reaches SQLite, hardware and the cloud
//! only through the commands registered here.

mod commands;
mod state;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        // Must be registered first: a second launch focuses the running till
        // instead of opening another process against the same database.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .manage(state::AppState::load())
        .invoke_handler(tauri::generate_handler![commands::app_info::app_info])
        .run(tauri::generate_context!())
        .expect("failed to start the POS client");
}

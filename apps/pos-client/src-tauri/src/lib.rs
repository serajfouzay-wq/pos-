//! POS client core. The React frontend reaches SQLite, hardware and the cloud
//! only through the commands registered here.

mod commands;
mod db;
mod license;
mod state;

use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{Emitter, Manager};

use crate::license::{CloudCheck, LicenseService};

/// Event name of `POS_EVENTS.license_status`.
const LICENSE_STATUS_EVENT: &str = "license://status";
/// How often the gate is re-evaluated, so expiry and grace take effect on a
/// till that is never restarted.
const REEVALUATE_EVERY: Duration = Duration::from_secs(15 * 60);
const CLOUD_CHECK_AFTER_SUCCESS: Duration = Duration::from_secs(6 * 60 * 60);

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
        .setup(|app| {
            let state = state::AppState::load(app.handle())?;
            let handle = app.handle().clone();
            state.license.set_listener(move |status| {
                let _ = handle.emit(LICENSE_STATUS_EVENT, status);
            });
            spawn_license_worker(Arc::clone(&state.license));
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info::app_info,
            commands::license::verify_license,
            commands::license::get_activation_request,
            commands::license::activate_license,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start the POS client");
}

/// Background license upkeep: evaluates at start-up (reading hardware off the
/// UI thread), re-evaluates every 15 minutes, and validates with the cloud
/// every 6 hours — or every 15 minutes while it is unreachable.
fn spawn_license_worker(license: Arc<LicenseService>) {
    tauri::async_runtime::spawn(async move {
        let mut next_cloud_check = Instant::now();
        loop {
            let service = Arc::clone(&license);
            if Instant::now() >= next_cloud_check {
                let outcome = tauri::async_runtime::spawn_blocking(move || service.cloud_check())
                    .await
                    .unwrap_or(CloudCheck::Unreachable);
                next_cloud_check = Instant::now()
                    + match outcome {
                        CloudCheck::Reached => CLOUD_CHECK_AFTER_SUCCESS,
                        CloudCheck::Unreachable | CloudCheck::Skipped => REEVALUATE_EVERY,
                    };
            } else {
                let _ = tauri::async_runtime::spawn_blocking(move || service.evaluate()).await;
            }
            tokio::time::sleep(REEVALUATE_EVERY).await;
        }
    });
}

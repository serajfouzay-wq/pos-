//! POS client core. The React frontend reaches SQLite, hardware and the cloud
//! only through the commands registered here.

mod commands;
mod db;
mod inventory;
mod license;
mod open_orders;
mod printing;
mod repo;
mod sample_catalog;
mod session;
mod state;
mod sync;

use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{Emitter, Manager};

use crate::license::{CloudCheck, LicenseService};
use crate::printing::PrintService;
use crate::sync::SyncEngine;

/// Event name of `POS_EVENTS.license_status`.
const LICENSE_STATUS_EVENT: &str = "license://status";
/// Event name of `POS_EVENTS.printer_status`.
const PRINTER_STATUS_EVENT: &str = "printer://status";
/// Event name of `POS_EVENTS.sync_status`.
const SYNC_STATUS_EVENT: &str = "sync://status";
const PRINT_QUEUE_EVERY: Duration = Duration::from_secs(30);
/// `SYNC_INTERVAL_MS`; changes also nudge the worker immediately.
const SYNC_EVERY: Duration = Duration::from_secs(60);
/// Lets the license worker read hardware and open the database first.
const SYNC_FIRST_AFTER: Duration = Duration::from_secs(5);
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
            let handle = app.handle().clone();
            state.printer.set_listener(move |status| {
                let _ = handle.emit(PRINTER_STATUS_EVENT, status);
            });
            let handle = app.handle().clone();
            state.sync.set_listener(move |status| {
                let _ = handle.emit(SYNC_STATUS_EVENT, status);
            });
            spawn_license_worker(Arc::clone(&state.license));
            spawn_sync_worker(Arc::clone(&state.license), Arc::clone(&state.sync));
            spawn_print_queue_worker(Arc::clone(&state.license), Arc::clone(&state.printer));
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info::app_info,
            commands::license::verify_license,
            commands::license::get_activation_request,
            commands::license::activate_license,
            commands::session::session_status,
            commands::session::list_login_users,
            commands::session::bootstrap_owner,
            commands::session::login,
            commands::session::logout,
            commands::users::list_users,
            commands::users::create_user,
            commands::catalog::get_products,
            commands::catalog::get_categories,
            commands::catalog::save_product,
            commands::catalog::load_sample_catalog,
            commands::shifts::current_shift,
            commands::shifts::open_shift,
            commands::shifts::close_shift,
            commands::sales::quote_transaction,
            commands::sales::create_transaction,
            commands::sales::print_receipt,
            commands::sales::kick_cash_drawer,
            commands::hardware::printer_status,
            commands::hardware::list_printers,
            commands::hardware::get_printer_settings,
            commands::hardware::save_printer_settings,
            commands::hardware::test_printer,
            commands::sync::sync_to_cloud,
            commands::sync::sync_status,
            commands::menu::get_menu,
            commands::menu::save_modifier_group,
            commands::menu::delete_modifier_group,
            commands::menu::set_product_modifier_groups,
            commands::menu::save_combo,
            commands::menu::delete_combo,
            commands::menu::save_dining_table,
            commands::menu::delete_dining_table,
            commands::orders::list_open_orders,
            commands::orders::open_order,
            commands::orders::update_open_order,
            commands::orders::split_order_line,
            commands::orders::fire_course,
            commands::orders::cancel_open_order,
            commands::orders::pay_open_order,
            commands::inventory::adjust_stock,
            commands::inventory::print_product_labels,
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

/// Offline receipt queue: retries pending receipts every 30 s while licensed,
/// so tickets printed during an outage come out once the printer is back.
fn spawn_print_queue_worker(license: Arc<LicenseService>, printer: Arc<PrintService>) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(PRINT_QUEUE_EVERY).await;
            let (license, printer) = (Arc::clone(&license), Arc::clone(&printer));
            let _ = tauri::async_runtime::spawn_blocking(move || {
                if let Ok(db) = license.database() {
                    let _ = printer.drain(&db, None);
                }
            })
            .await;
        }
    });
}

/// Offline sync: a round every 60 s, and right away when a change is made
/// (nudge) or the UI reports the network is back (`sync_to_cloud`). Rounds
/// while unlicensed or offline are cheap no-ops; changes wait in the outbox.
fn spawn_sync_worker(license: Arc<LicenseService>, sync: Arc<SyncEngine>) {
    if !sync.enabled() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(SYNC_FIRST_AFTER).await;
        loop {
            let (service, engine) = (Arc::clone(&license), Arc::clone(&sync));
            let _ =
                tauri::async_runtime::spawn_blocking(move || sync::round(&service, &engine)).await;
            tokio::select! {
                () = tokio::time::sleep(SYNC_EVERY) => {}
                () = sync.nudged() => {}
            }
        }
    });
}

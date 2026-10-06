//! POS client core. The React frontend reaches SQLite, hardware and the cloud
//! only through the commands registered here.

mod backup;
mod commands;
mod db;
mod history;
mod inventory;
mod kitchen;
mod license;
mod loyalty;
mod open_orders;
mod printing;
mod refunds;
mod repo;
mod reports;
mod sample_catalog;
mod session;
mod state;
mod sync;
mod updater;

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
/// Event name of `POS_EVENTS.kitchen_changed`.
const KITCHEN_CHANGED_EVENT: &str = "kitchen://changed";
/// Event name of `POS_EVENTS.update_status`.
const UPDATE_STATUS_EVENT: &str = "updater://status";
const PRINT_QUEUE_EVERY: Duration = Duration::from_secs(30);
/// `SYNC_INTERVAL_MS`; changes also nudge the worker immediately.
const SYNC_EVERY: Duration = Duration::from_secs(60);
/// While a kitchen display is open on this till.
const SYNC_FAST_EVERY: Duration = Duration::from_secs(5);
/// Lets the license worker read hardware and open the database first.
const SYNC_FIRST_AFTER: Duration = Duration::from_secs(5);
/// How often the gate is re-evaluated, so expiry and grace take effect on a
/// till that is never restarted.
const REEVALUATE_EVERY: Duration = Duration::from_secs(15 * 60);
const BACKUP_CHECK_EVERY: Duration = Duration::from_secs(30 * 60);
const CLOUD_CHECK_AFTER_SUCCESS: Duration = Duration::from_secs(6 * 60 * 60);

pub fn run() {
    let mut builder = tauri::Builder::default()
        // Must be registered first: a second launch focuses the running till
        // instead of opening another process against the same database.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        // Folder and file pickers (backups on a USB stick, update files).
        .plugin(tauri_plugin_dialog::init());
    // Only builds signed for updates can verify (and so install) them.
    if let Some(key) = updater::PUBLIC_KEY.filter(|k| !k.trim().is_empty()) {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().pubkey(key).build());
    }
    let app = builder
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
            let handle = app.handle().clone();
            state.kitchen.set_listener(move |change| {
                let _ = handle.emit(KITCHEN_CHANGED_EVENT, change);
            });
            let handle = app.handle().clone();
            state.updates.set_listener(move |status| {
                let _ = handle.emit(UPDATE_STATUS_EVENT, status);
            });
            if state.updates.configured() {
                updater::spawn_worker(
                    app.handle().clone(),
                    Arc::clone(&state.updates),
                    Arc::clone(&state.license),
                    Arc::clone(&state.client),
                );
            }
            spawn_license_worker(Arc::clone(&state.license));
            spawn_backup_worker(Arc::clone(&state.license), Arc::clone(&state.backups));
            spawn_sync_worker(
                Arc::clone(&state.license),
                Arc::clone(&state.sync),
                Arc::clone(&state.lan),
            );
            spawn_print_queue_worker(Arc::clone(&state.license), Arc::clone(&state.printer));
            commands::kitchen::restore_window(app.handle().clone(), &state);
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info::app_info,
            commands::license::verify_license,
            commands::license::get_activation_request,
            commands::license::activate_license,
            commands::license::save_activation_file,
            commands::license::find_license_files,
            commands::license::activate_license_file,
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
            commands::history::list_transactions,
            commands::history::quote_refund,
            commands::history::get_transaction,
            commands::history::refund_transaction,
            commands::history::void_transaction,
            commands::reports::get_x_report,
            commands::reports::run_z_report,
            commands::reports::list_z_reports,
            commands::reports::get_z_report,
            commands::reports::print_report,
            commands::reports::list_shifts,
            commands::reports::get_dashboard_metrics,
            commands::reports::list_audit_log,
            commands::customers::search_customers,
            commands::customers::get_customer,
            commands::customers::save_customer,
            commands::customers::delete_customer,
            commands::customers::adjust_loyalty_points,
            commands::customers::get_loyalty_settings,
            commands::customers::save_loyalty_settings,
            commands::kitchen::kitchen_display_status,
            commands::kitchen::set_kitchen_display,
            commands::kitchen::list_kitchen_tickets,
            commands::kitchen::bump_kitchen_ticket,
            commands::kitchen::set_kitchen_item_done,
            commands::updates::update_status,
            commands::updates::check_for_updates,
            commands::updates::install_update,
            commands::updates::find_update_files,
            commands::updates::inspect_update_file,
            commands::updates::install_update_file,
            commands::updates::dismiss_update_notice,
            commands::discounts::list_discount_rules,
            commands::discounts::save_discount_rule,
            commands::discounts::delete_discount_rule,
            commands::memberships::list_membership_plans,
            commands::memberships::save_membership_plan,
            commands::memberships::delete_membership_plan,
            commands::memberships::list_members,
            commands::memberships::customer_memberships,
            commands::memberships::grant_membership,
            commands::memberships::cancel_membership,
            commands::backups::backup_status,
            commands::backups::backup_now,
            commands::backups::save_backup_settings,
            commands::backups::set_backup_password,
            commands::backups::list_backups_in,
            commands::backups::restore_backup,
            commands::backups::restart_app,
            commands::lan::lan_status,
            commands::lan::save_lan_settings,
            commands::lan::discover_hubs,
            commands::lan::test_hub,
            commands::lan::new_hub_code,
        ])
        .build(tauri::generate_context!())
        .expect("failed to start the POS client");
    app.run(|handle, event| {
        // A downloaded update installs as the till closes.
        if let tauri::RunEvent::Exit = event {
            if let Some(state) = handle.try_state::<state::AppState>() {
                if state.updates.status().state == updater::UpdateState::Ready {
                    if let Ok(db) = state.license.database() {
                        let _ = state.backups.create(&db, backup::Reason::BeforeUpdate);
                    }
                }
                state.updates.install_on_exit();
            }
        }
    });
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

/// Backups: an integrity check and a backup (if due) once the database is
/// open, then a backup whenever the interval has passed. Checked every 30
/// minutes; a till that is off simply catches up at the next start.
fn spawn_backup_worker(license: Arc<LicenseService>, backups: Arc<backup::BackupService>) {
    tauri::async_runtime::spawn(async move {
        let mut started = false;
        loop {
            tokio::time::sleep(if started {
                BACKUP_CHECK_EVERY
            } else {
                SYNC_FIRST_AFTER
            })
            .await;
            let (license, backups) = (Arc::clone(&license), Arc::clone(&backups));
            let first = !started;
            let ran = tauri::async_runtime::spawn_blocking(move || {
                let Ok(db) = license.database() else {
                    return false;
                };
                if first {
                    backups.check_integrity(&db);
                }
                let reason = if first {
                    backup::Reason::Start
                } else {
                    backup::Reason::Scheduled
                };
                let _ = backups.maybe_backup(&db, reason);
                true
            })
            .await
            .unwrap_or(false);
            started |= ran;
        }
    });
}

/// Offline print queues: retries pending receipts and kitchen tickets every
/// 30 s while licensed, so what was sent during an outage comes out once the
/// printer is back.
fn spawn_print_queue_worker(license: Arc<LicenseService>, printer: Arc<PrintService>) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(PRINT_QUEUE_EVERY).await;
            let (license, printer) = (Arc::clone(&license), Arc::clone(&printer));
            let _ = tauri::async_runtime::spawn_blocking(move || {
                if let Ok(db) = license.database() {
                    let _ = printer.drain(&db, None);
                    let _ = printer.drain_kitchen(&db);
                }
            })
            .await;
        }
    });
}

/// Offline sync: a round every 60 s, and right away when a change is made
/// (nudge) or the UI reports the network is back (`sync_to_cloud`). Rounds
/// while unlicensed or offline are cheap no-ops; changes wait in the outbox.
fn spawn_sync_worker(
    license: Arc<LicenseService>,
    sync: Arc<SyncEngine>,
    lan: Arc<sync::lan::LanService>,
) {
    // Runs even without a target: the shop network can be set up later.
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(SYNC_FIRST_AFTER).await;
        loop {
            // The shop-network role lives in the database: apply it once
            // the license has opened it (hub server, or the hub as target).
            if !lan.applied() {
                let (service, engine, lan) =
                    (Arc::clone(&license), Arc::clone(&sync), Arc::clone(&lan));
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    if let Ok(db) = service.database() {
                        let _ = lan.apply(&db, &service, &engine);
                    }
                })
                .await;
            }
            if sync.enabled() {
                let (service, engine) = (Arc::clone(&license), Arc::clone(&sync));
                let _ =
                    tauri::async_runtime::spawn_blocking(move || sync::round(&service, &engine))
                        .await;
            }
            let every = if sync.is_fast() {
                SYNC_FAST_EVERY
            } else {
                SYNC_EVERY
            };
            tokio::select! {
                () = tokio::time::sleep(every) => {}
                () = sync.nudged() => {}
            }
        }
    });
}

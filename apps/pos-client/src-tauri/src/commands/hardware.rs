use std::sync::Arc;

use pos_core::config::Locale;
use pos_core::rbac::Permission;
use pos_core::time::{Clock, SystemClock};
use pos_core::{IpcError, IpcResult};
use pos_hardware::doc::PrintMode;
use pos_hardware::transport::{DiscoveredPrinter, PrinterTarget};
use tauri::State;

use super::{authorize, blocking};
use crate::printing::{PrintService, PrinterSettings, PrinterStatus, SETTINGS_KEY};
use crate::repo::{audit, settings, SqlResultExt};
use crate::state::AppState;

/// Any signed-in user sees printer health (the status-bar indicator).
#[tauri::command(rename_all = "snake_case")]
pub async fn printer_status(state: State<'_, AppState>) -> IpcResult<PrinterStatus> {
    let auth = authorize(&state, Permission::CatalogView)?;
    let printer = Arc::clone(&state.printer);
    blocking(move || printer.status(&auth.db)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_printers(state: State<'_, AppState>) -> IpcResult<Vec<DiscoveredPrinter>> {
    authorize(&state, Permission::SettingsManage)?;
    let printer = Arc::clone(&state.printer);
    blocking(move || Ok(printer.discover())).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_printer_settings(state: State<'_, AppState>) -> IpcResult<PrinterSettings> {
    let auth = authorize(&state, Permission::SettingsManage)?;
    blocking(move || PrintService::settings(&auth.db.conn())).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn save_printer_settings(
    state: State<'_, AppState>,
    settings: PrinterSettings,
) -> IpcResult<PrinterSettings> {
    let auth = authorize(&state, Permission::SettingsManage)?;
    settings.validate()?;
    let printer = Arc::clone(&state.printer);
    blocking(move || {
        let now = SystemClock.now();
        let actor = auth.actor()?;
        {
            let conn = auth.db.conn();
            let before = PrintService::settings(&conn)?;
            settings::put(&conn, SETTINGS_KEY, &settings, now).ipc()?;
            audit::record(
                &conn,
                &actor,
                "settings.manage",
                "settings",
                None,
                serde_json::to_value(&before).ok(),
                serde_json::to_value(&settings).ok(),
                now,
            )
            .ipc()?;
        }
        // A newly reachable printer may have queued receipts waiting.
        let _ = printer.drain(&auth.db, None);
        let _ = printer.drain_kitchen(&auth.db);
        Ok(settings)
    })
    .await
}

/// A test page; `language`, `mode` and `paper_width_mm` try settings before
/// they are saved (the saved ones otherwise).
#[tauri::command(rename_all = "snake_case")]
pub async fn test_printer(
    state: State<'_, AppState>,
    target: PrinterTarget,
    language: Option<Locale>,
    mode: Option<PrintMode>,
    paper_width_mm: Option<u16>,
) -> IpcResult<()> {
    let auth = authorize(&state, Permission::SettingsManage)?;
    if paper_width_mm.is_some_and(|w| w != 58 && w != 80) {
        return Err(IpcError::validation("Paper is 58 mm or 80 mm wide."));
    }
    let printer = Arc::clone(&state.printer);
    blocking(move || printer.test_print(&auth.db, &target, language, mode, paper_width_mm)).await
}

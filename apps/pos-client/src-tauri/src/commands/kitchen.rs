//! The kitchen display: its window and the ticket board.
//!
//! The window (`kds`) is opened by the owner (`settings.manage`) and then
//! reopens at every start of this till. It has its own capability
//! (`capabilities/kitchen.json`) that grants ONLY the board commands, and
//! those commands accept a call from that window without a signed-in user:
//! a kitchen screen stays usable while the tills sign in and out. From any
//! other window they need `sale.create`.

use std::sync::Arc;

use pos_core::config::ClientConfig;
use pos_core::rbac::Permission;
use pos_core::time::{Clock, SystemClock};
use pos_core::{IpcError, IpcResult};
use serde::Serialize;
use tauri::{
    AppHandle, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use uuid::Uuid;

use super::{authorize, blocking, check_session};
use crate::db::Database;
use crate::kitchen::{self, Board, KitchenTicket};
use crate::repo::{settings, SqlResultExt};
use crate::state::AppState;
use crate::sync::SyncEngine;

pub const WINDOW_LABEL: &str = "kds";
const SETTING: &str = "kitchen_display.enabled";

/// Board access: the kitchen window, or a signed-in user who can sell.
fn board_access(state: &AppState, window: &WebviewWindow) -> IpcResult<Arc<Database>> {
    let db = state.license.database()?;
    if window.label() != WINDOW_LABEL {
        check_session(state.session.current(), Permission::SaleCreate)?;
    }
    Ok(db)
}

/// Mirrors `KitchenDisplayStatusSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct KitchenDisplayStatus {
    pub available: bool,
    pub enabled: bool,
    pub open: bool,
}

fn status(
    app: &AppHandle,
    db: &Database,
    client: &ClientConfig,
) -> IpcResult<KitchenDisplayStatus> {
    Ok(KitchenDisplayStatus {
        available: kitchen::enabled(client),
        enabled: settings::get::<bool>(&db.conn(), SETTING)
            .ipc()?
            .unwrap_or(false),
        open: app.get_webview_window(WINDOW_LABEL).is_some(),
    })
}

fn open_window(app: &AppHandle, client: &ClientConfig, sync: &Arc<SyncEngine>) -> IpcResult<()> {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(
        app,
        WINDOW_LABEL,
        WebviewUrl::App("index.html?window=kds".into()),
    )
    .title(format!("Kitchen — {}", client.display_name))
    .inner_size(1280.0, 800.0)
    .min_inner_size(800.0, 600.0)
    .build()
    .map_err(|e| IpcError::internal(format!("could not open the kitchen window: {e}")))?;
    sync.set_fast(true);
    let sync = Arc::clone(sync);
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            sync.set_fast(false);
        }
    });
    Ok(())
}

/// Reopens the kitchen window at start-up once the database is unlocked
/// (the license check runs in the background first).
pub fn restore_window(app: AppHandle, state: &AppState) {
    if !kitchen::enabled(&state.client) {
        return;
    }
    let (license, client, sync) = (
        Arc::clone(&state.license),
        Arc::clone(&state.client),
        Arc::clone(&state.sync),
    );
    tauri::async_runtime::spawn(async move {
        for _ in 0..300 {
            if let Ok(db) = license.database() {
                let enabled = settings::get::<bool>(&db.conn(), SETTING)
                    .ok()
                    .flatten()
                    .unwrap_or(false);
                if enabled {
                    let _ = open_window(&app, &client, &sync);
                }
                return;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });
}

#[tauri::command(rename_all = "snake_case")]
pub async fn kitchen_display_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> IpcResult<KitchenDisplayStatus> {
    let auth = authorize(&state, Permission::SaleCreate)?;
    status(&app, &auth.db, &state.client)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn set_kitchen_display(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> IpcResult<KitchenDisplayStatus> {
    let auth = authorize(&state, Permission::SettingsManage)?;
    if enabled && !kitchen::enabled(&state.client) {
        return Err(IpcError::validation(
            "This till was built without the kitchen display.",
        ));
    }
    settings::put(&auth.db.conn(), SETTING, &enabled, SystemClock.now()).ipc()?;
    if enabled {
        open_window(&app, &state.client, &state.sync)?;
    } else if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.destroy();
        state.sync.set_fast(false);
    }
    let mut result = status(&app, &auth.db, &state.client)?;
    // `destroy` completes on the event loop; report what was asked.
    result.open = enabled;
    Ok(result)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_kitchen_tickets(
    window: WebviewWindow,
    state: State<'_, AppState>,
    recent_minutes: i64,
) -> IpcResult<Board> {
    let db = board_access(&state, &window)?;
    blocking(move || kitchen::board(&db.conn(), recent_minutes, SystemClock.now())).await
}

fn changed(state: &AppState, ticket: &KitchenTicket) {
    state.kitchen.changed(ticket);
    state.sync.nudge();
}

#[tauri::command(rename_all = "snake_case")]
pub async fn bump_kitchen_ticket(
    window: WebviewWindow,
    state: State<'_, AppState>,
    ticket_id: Uuid,
    ready: bool,
) -> IpcResult<KitchenTicket> {
    let db = board_access(&state, &window)?;
    blocking(move || kitchen::bump(&db.conn(), ticket_id, ready, SystemClock.now()))
        .await
        .inspect(|ticket| changed(&state, ticket))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn set_kitchen_item_done(
    window: WebviewWindow,
    state: State<'_, AppState>,
    ticket_id: Uuid,
    line_id: Uuid,
    done: bool,
) -> IpcResult<KitchenTicket> {
    let db = board_access(&state, &window)?;
    blocking(move || kitchen::set_done(&db.conn(), ticket_id, line_id, done, SystemClock.now()))
        .await
        .inspect(|ticket| changed(&state, ticket))
}

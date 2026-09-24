//! Open orders: cafe tabs and restaurant tables. Everything needs
//! `sale.create`; changing or cancelling items already sent to the kitchen
//! also needs `sale.void` (checked in `open_orders`).

use std::sync::Arc;

use pos_core::rbac::{self, Permission};
use pos_core::time::{Clock, SystemClock, Timestamp};
use pos_core::IpcResult;
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use super::sales::{complete, SaleReceipt};
use super::{authorize, blocking, Authorized};
use crate::open_orders::{self, OpenInput, OpenOrderView, OrderActor, PayInput, UpdateInput};
use crate::repo::sales::SaleActor;
use crate::repo::{device, SqlResultExt};
use crate::state::AppState;

fn actor(auth: &Authorized) -> IpcResult<OrderActor> {
    Ok(OrderActor {
        user_id: auth.session.user_id,
        display_name: auth.session.display_name.clone(),
        role: auth.session.role,
        device_id: device::id(&auth.db.conn()).ipc()?,
    })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_open_orders(state: State<'_, AppState>) -> IpcResult<Vec<OpenOrderView>> {
    let auth = authorize(&state, Permission::SaleCreate)?;
    let client = Arc::clone(&state.client);
    blocking(move || open_orders::views(&auth.db.conn(), &client, SystemClock.now())).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn open_order(state: State<'_, AppState>, input: OpenInput) -> IpcResult<OpenOrderView> {
    let auth = authorize(&state, Permission::SaleCreate)?;
    let client = Arc::clone(&state.client);
    blocking(move || {
        let actor = actor(&auth)?;
        let now = SystemClock.now();
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        let order = open_orders::open(&tx, &actor, input, now)?;
        let view = open_orders::view(&tx, order, &client, now)?;
        tx.commit().ipc()?;
        Ok(view)
    })
    .await
    .inspect(|_| state.sync.nudge())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn update_open_order(
    state: State<'_, AppState>,
    input: UpdateInput,
) -> IpcResult<OpenOrderView> {
    let auth = authorize(&state, Permission::SaleCreate)?;
    let client = Arc::clone(&state.client);
    blocking(move || {
        let actor = actor(&auth)?;
        let now = SystemClock.now();
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        let order = open_orders::update(&tx, &actor, input, &client, now)?;
        let view = open_orders::view(&tx, order, &client, now)?;
        tx.commit().ipc()?;
        Ok(view)
    })
    .await
    .inspect(|_| state.sync.nudge())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn split_order_line(
    state: State<'_, AppState>,
    order_id: Uuid,
    line_id: Uuid,
    expected_updated_at: Timestamp,
) -> IpcResult<OpenOrderView> {
    let auth = authorize(&state, Permission::SaleCreate)?;
    let client = Arc::clone(&state.client);
    blocking(move || {
        let now = SystemClock.now();
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        let order = open_orders::split_line(&tx, order_id, line_id, expected_updated_at, now)?;
        let view = open_orders::view(&tx, order, &client, now)?;
        tx.commit().ipc()?;
        Ok(view)
    })
    .await
    .inspect(|_| state.sync.nudge())
}

/// Mirrors `FireOutcomeSchema`.
#[derive(Debug, Serialize)]
pub struct FireOutcome {
    pub(crate) order: OpenOrderView,
    /// A kitchen printer is configured and took the ticket.
    pub(crate) printed: bool,
    pub(crate) print_error: Option<String>,
    /// The ticket as text (shown when there is no kitchen printer).
    pub(crate) ticket_text: String,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn fire_course(
    state: State<'_, AppState>,
    order_id: Uuid,
    course: Option<i64>,
    expected_updated_at: Timestamp,
) -> IpcResult<FireOutcome> {
    let auth = authorize(&state, Permission::SaleCreate)?;
    let client = Arc::clone(&state.client);
    let printer = Arc::clone(&state.printer);
    blocking(move || {
        let actor = actor(&auth)?;
        let now = SystemClock.now();
        let (fired, view) = {
            let mut conn = auth.db.conn();
            let tx = conn.transaction().ipc()?;
            let fired = open_orders::fire(&tx, &actor, order_id, course, expected_updated_at, now)?;
            let view = open_orders::view(&tx, fired.order.clone(), &client, now)?;
            tx.commit().ipc()?;
            (fired, view)
        };
        // The order is marked as sent even if the printer is down: the
        // ticket text is returned so the till can show or reprint it.
        let (printed, print_error) = match printer.print_kitchen(&auth.db, &fired.ticket) {
            Ok(printed) => (printed, None),
            Err(e) => (false, Some(e.message)),
        };
        Ok(FireOutcome {
            order: view,
            printed,
            print_error,
            ticket_text: pos_hardware::kitchen::render_text(
                &fired.ticket,
                client.receipt.paper_width_mm,
            ),
        })
    })
    .await
    .inspect(|_| state.sync.nudge())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn cancel_open_order(
    state: State<'_, AppState>,
    order_id: Uuid,
    expected_updated_at: Timestamp,
) -> IpcResult<()> {
    let auth = authorize(&state, Permission::SaleCreate)?;
    blocking(move || {
        let actor = actor(&auth)?;
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        open_orders::cancel(
            &tx,
            &actor,
            order_id,
            expected_updated_at,
            SystemClock.now(),
        )?;
        tx.commit().ipc()?;
        Ok(())
    })
    .await
    .inspect(|_| state.sync.nudge())
}

/// Mirrors `PaidOrderSchema`.
#[derive(Debug, Serialize)]
pub struct PaidOrder {
    pub(crate) sale: SaleReceipt,
    /// `null` once the order is fully paid.
    pub(crate) order: Option<OpenOrderView>,
}

/// Pays the chosen lines (split bill) or the whole order.
#[tauri::command(rename_all = "snake_case")]
pub async fn pay_open_order(state: State<'_, AppState>, input: PayInput) -> IpcResult<PaidOrder> {
    let auth = authorize(&state, Permission::SaleCreate)?;
    if !input.discount_rule_ids.is_empty() {
        rbac::authorize(auth.session.role, Permission::DiscountApply)?;
    }
    let client = Arc::clone(&state.client);
    let printer = Arc::clone(&state.printer);
    blocking(move || {
        let sale_actor = SaleActor {
            user_id: auth.session.user_id,
            role: auth.session.role,
        };
        let now = SystemClock.now();
        let (order, created) = {
            let mut conn = auth.db.conn();
            let tx = conn.transaction().ipc()?;
            let (order, created) = open_orders::pay(&tx, &sale_actor, &input, &client, now)?;
            let view = match order.status {
                crate::repo::orders::OpenOrderStatus::Open => {
                    Some(open_orders::view(&tx, order, &client, now)?)
                }
                _ => None,
            };
            tx.commit().ipc()?;
            (view, created)
        };
        Ok(PaidOrder {
            sale: complete(&auth.db, &printer, created)?,
            order,
        })
    })
    .await
    .inspect(|_| state.sync.nudge())
}

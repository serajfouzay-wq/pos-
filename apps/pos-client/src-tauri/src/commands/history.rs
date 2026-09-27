//! Sales history, refunds and voids.

use std::sync::Arc;

use pos_core::rbac::Permission;
use pos_core::time::{Clock, SystemClock};
use pos_core::IpcResult;
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use super::sales::{complete, SaleReceipt};
use super::{authorize, blocking};
use crate::history::{self, TransactionDetail, TransactionFilter, TransactionSummary};
use crate::refunds::{self, RefundInput, ReverseActor, VoidInput};
use crate::state::AppState;

#[tauri::command(rename_all = "snake_case")]
pub async fn list_transactions(
    state: State<'_, AppState>,
    filter: TransactionFilter,
) -> IpcResult<Vec<TransactionSummary>> {
    let auth = authorize(&state, Permission::ReceiptReprint)?;
    blocking(move || history::list(&auth.db.conn(), &filter)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_transaction(
    state: State<'_, AppState>,
    transaction_id: Uuid,
) -> IpcResult<TransactionDetail> {
    let auth = authorize(&state, Permission::ReceiptReprint)?;
    blocking(move || history::detail(&auth.db.conn(), transaction_id)).await
}

/// Mirrors `RefundQuoteSchema`.
#[derive(Debug, Serialize)]
pub struct RefundQuote {
    pub(crate) total: i64,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn quote_refund(
    state: State<'_, AppState>,
    input: RefundInput,
) -> IpcResult<RefundQuote> {
    let auth = authorize(&state, Permission::SaleRefund)?;
    blocking(move || {
        Ok(RefundQuote {
            total: refunds::quote(&auth.db.conn(), &input)?,
        })
    })
    .await
}

/// Takes back chosen quantities; the refund receipt prints and a cash
/// refund opens the drawer, like a sale.
#[tauri::command(rename_all = "snake_case")]
pub async fn refund_transaction(
    state: State<'_, AppState>,
    input: RefundInput,
) -> IpcResult<SaleReceipt> {
    let auth = authorize(&state, Permission::SaleRefund)?;
    let printer = Arc::clone(&state.printer);
    blocking(move || {
        let actor = ReverseActor {
            user_id: auth.session.user_id,
            role: auth.session.role,
        };
        let created = refunds::refund(&mut auth.db.conn(), &actor, &input, SystemClock.now())?;
        complete(&auth.db, &printer, created)
    })
    .await
    .inspect(|_| state.sync.nudge())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn void_transaction(
    state: State<'_, AppState>,
    input: VoidInput,
) -> IpcResult<SaleReceipt> {
    let auth = authorize(&state, Permission::SaleVoid)?;
    let printer = Arc::clone(&state.printer);
    blocking(move || {
        let actor = ReverseActor {
            user_id: auth.session.user_id,
            role: auth.session.role,
        };
        let created = refunds::void(&mut auth.db.conn(), &actor, &input, SystemClock.now())?;
        complete(&auth.db, &printer, created)
    })
    .await
    .inspect(|_| state.sync.nudge())
}

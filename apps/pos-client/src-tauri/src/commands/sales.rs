use std::sync::Arc;

use pos_core::rbac::{self, Permission};
use pos_core::receipt::Receipt;
use pos_core::time::{Clock, SystemClock};
use pos_core::IpcResult;
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use super::{authorize, blocking};
use crate::printing::PrintService;
use crate::repo::sales::{self, QuoteRequest, QuoteView, SaleActor, TransactionPayload};
use crate::repo::{audit, print_jobs, SqlResultExt};
use crate::state::AppState;

/// Prices the cart exactly as `create_transaction` will. The UI shows this
/// instead of computing totals itself.
#[tauri::command(rename_all = "snake_case")]
pub async fn quote_transaction(
    state: State<'_, AppState>,
    request: QuoteRequest,
) -> IpcResult<QuoteView> {
    let auth = authorize(&state, Permission::SaleCreate)?;
    if !request.discount_rule_ids.is_empty() {
        rbac::authorize(auth.session.role, Permission::DiscountApply)?;
    }
    let client = Arc::clone(&state.client);
    blocking(move || {
        let conn = auth.db.conn();
        let quote = sales::quote(
            &conn,
            &request.items,
            &request.discount_rule_ids,
            &client,
            SystemClock.now(),
        )?;
        Ok(sales::quote_view(quote, client.currency.base))
    })
    .await
}

/// Mirrors `SaleReceiptSchema`: the receipt plus what the hardware did.
#[derive(Debug, Serialize)]
pub struct SaleReceipt {
    #[serde(flatten)]
    pub(crate) receipt: Receipt,
    pub(crate) drawer_opened: bool,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn create_transaction(
    state: State<'_, AppState>,
    payload: TransactionPayload,
) -> IpcResult<SaleReceipt> {
    let auth = authorize(&state, Permission::SaleCreate)?;
    if !payload.discount_rule_ids.is_empty() {
        rbac::authorize(auth.session.role, Permission::DiscountApply)?;
    }
    let client = Arc::clone(&state.client);
    let printer = Arc::clone(&state.printer);
    blocking(move || {
        let actor = SaleActor {
            user_id: auth.session.user_id,
            role: auth.session.role,
        };
        let created = sales::create(
            &mut auth.db.conn(),
            &actor,
            &payload,
            &client,
            SystemClock.now(),
        )?;

        // Hardware is best-effort and happens after the sale is committed:
        // a jammed printer must never lose a sale.
        let mut drawer_opened = false;
        let printed = if created.is_new {
            let settings = PrintService::settings(&auth.db.conn())?;
            if created.includes_cash && settings.open_drawer_on_cash {
                drawer_opened = printer.kick_drawer(&auth.db).is_ok();
            }
            printer
                .drain(&auth.db, Some(created.transaction_id))
                .unwrap_or(false)
        } else {
            print_jobs::ever_printed(&auth.db.conn(), created.transaction_id).ipc()?
        };
        let receipt = sales::load_receipt(&auth.db.conn(), created.transaction_id, printed)?;
        Ok(SaleReceipt {
            receipt,
            drawer_opened,
        })
    })
    .await
}

/// Mirrors `PrintOutcomeSchema`.
#[derive(Debug, Serialize)]
pub struct PrintOutcome {
    pub(crate) printed: bool,
    /// Still waiting in the offline queue.
    pub(crate) queued: bool,
}

/// First print of a receipt needs `receipt.print`; any later copy is a
/// reprint (`receipt.reprint`, manager+) and is marked COPY.
#[tauri::command(rename_all = "snake_case")]
pub async fn print_receipt(
    state: State<'_, AppState>,
    transaction_id: Uuid,
) -> IpcResult<PrintOutcome> {
    let auth = authorize(&state, Permission::ReceiptPrint)?;
    let printer = Arc::clone(&state.printer);
    blocking(move || {
        // Resolve the actor before taking the connection lock (not re-entrant).
        let actor = auth.actor()?;
        {
            let conn = auth.db.conn();
            // Existence check (NotFound) before anything else.
            sales::load_receipt(&conn, transaction_id, false)?;
            if !print_jobs::has_pending(&conn, transaction_id).ipc()? {
                let copy = print_jobs::ever_printed(&conn, transaction_id).ipc()?;
                if copy {
                    rbac::authorize(auth.session.role, Permission::ReceiptReprint)?;
                    audit::record(
                        &conn,
                        &actor,
                        "receipt.reprint",
                        "transactions",
                        Some(transaction_id),
                        None,
                        None,
                        SystemClock.now(),
                    )
                    .ipc()?;
                }
                print_jobs::enqueue(&conn, transaction_id, copy, SystemClock.now()).ipc()?;
            }
        }
        let printed = printer.drain(&auth.db, Some(transaction_id))?;
        let queued = print_jobs::has_pending(&auth.db.conn(), transaction_id).ipc()?;
        Ok(PrintOutcome { printed, queued })
    })
    .await
}

/// "No sale" drawer open. Audited: an unexplained drawer open is exactly
/// what a cash-variance investigation looks for.
#[tauri::command(rename_all = "snake_case")]
pub async fn kick_cash_drawer(state: State<'_, AppState>) -> IpcResult<()> {
    let auth = authorize(&state, Permission::DrawerKick)?;
    let printer = Arc::clone(&state.printer);
    blocking(move || {
        let actor = auth.actor()?;
        audit::record(
            &auth.db.conn(),
            &actor,
            "drawer.kick",
            "device",
            Some(actor.device_id),
            None,
            None,
            SystemClock.now(),
        )
        .ipc()?;
        printer.kick_drawer(&auth.db)
    })
    .await
}

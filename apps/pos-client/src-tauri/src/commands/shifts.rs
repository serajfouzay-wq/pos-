use pos_core::rbac::Permission;
use pos_core::time::{Clock, SystemClock};
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use serde::Serialize;
use tauri::State;

use super::{authorize, blocking};
use crate::repo::shifts::{self, Shift, ShiftTotals};
use crate::repo::{audit, device, SqlResultExt};
use crate::state::AppState;

/// Mirrors `ShiftSummarySchema`.
#[derive(Debug, Serialize)]
pub struct ShiftSummary {
    pub(crate) shift: Shift,
    pub(crate) totals: ShiftTotals,
    /// opening float + net cash taken.
    pub(crate) expected_cash: i64,
}

fn summarize(conn: &rusqlite::Connection, shift: Shift) -> IpcResult<ShiftSummary> {
    let totals = shifts::totals(conn, shift.meta.id).ipc()?;
    let expected_cash = shift.opening_float + totals.cash_total;
    Ok(ShiftSummary {
        shift,
        totals,
        expected_cash,
    })
}

/// Any signed-in user may see whether a shift is open (cashiers need to know).
#[tauri::command(rename_all = "snake_case")]
pub async fn current_shift(state: State<'_, AppState>) -> IpcResult<Option<ShiftSummary>> {
    let auth = authorize(&state, Permission::SaleCreate)?;
    blocking(move || {
        let conn = auth.db.conn();
        let device_id = device::id(&conn).ipc()?;
        shifts::current_open(&conn, device_id)
            .ipc()?
            .map(|s| summarize(&conn, s))
            .transpose()
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn open_shift(state: State<'_, AppState>, opening_float: i64) -> IpcResult<ShiftSummary> {
    let auth = authorize(&state, Permission::ShiftOpen)?;
    if opening_float < 0 {
        return Err(IpcError::validation(
            "The opening float cannot be negative.",
        ));
    }
    blocking(move || {
        let now = SystemClock.now();
        let actor = auth.actor()?;
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        if shifts::current_open(&tx, actor.device_id).ipc()?.is_some() {
            return Err(IpcError::new(
                IpcErrorCode::Conflict,
                "A shift is already open on this till.",
            ));
        }
        let shift = shifts::open(&tx, actor.device_id, actor.user_id, opening_float, now).ipc()?;
        audit::record(
            &tx,
            &actor,
            "shift.open",
            "shifts",
            Some(shift.meta.id),
            None,
            serde_json::to_value(&shift).ok(),
            now,
        )
        .ipc()?;
        let summary = summarize(&tx, shift)?;
        tx.commit().ipc()?;
        Ok(summary)
    })
    .await
    .inspect(|_| state.sync.nudge())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn close_shift(
    state: State<'_, AppState>,
    actual_cash: i64,
    closing_float: i64,
    notes: Option<String>,
) -> IpcResult<ShiftSummary> {
    let auth = authorize(&state, Permission::ShiftClose)?;
    if actual_cash < 0 || closing_float < 0 || closing_float > actual_cash {
        return Err(IpcError::validation(
            "Counted cash and the float left in the drawer must be ≥ 0, and the float cannot exceed the count.",
        ));
    }
    blocking(move || {
        let now = SystemClock.now();
        let actor = auth.actor()?;
        let mut conn = auth.db.conn();
        let tx = conn.transaction().ipc()?;
        let open = shifts::current_open(&tx, actor.device_id)
            .ipc()?
            .ok_or_else(|| IpcError::new(IpcErrorCode::Conflict, "No shift is open."))?;
        let notes = notes.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty());
        let closed = shifts::close(
            &tx,
            &open,
            actor.user_id,
            actual_cash,
            closing_float,
            notes,
            now,
        )
        .ipc()?;
        audit::record(
            &tx,
            &actor,
            "shift.close",
            "shifts",
            Some(closed.meta.id),
            serde_json::to_value(&open).ok(),
            serde_json::to_value(&closed).ok(),
            now,
        )
        .ipc()?;
        let summary = summarize(&tx, closed)?;
        tx.commit().ipc()?;
        Ok(summary)
    })
    .await
    .inspect(|_| state.sync.nudge())
}

//! X/Z reports, shift history, the analytics dashboard and the audit trail.

use std::sync::Arc;

use pos_core::rbac::Permission;
use pos_core::time::{Clock, SystemClock, Timestamp, Zone};
use pos_core::IpcResult;
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use super::{authorize, blocking, Authorized};
use crate::printing::PrintService;
use crate::repo::audit::{self, AuditFilter, AuditPage};
use crate::repo::shifts::{self, Shift, ShiftTotals};
use crate::repo::{device, SqlResultExt};
use crate::reports::{self, DashboardData, PeriodReport, ReportActor, Window, ZReportSummary};
use crate::state::AppState;

fn by(auth: &Authorized) -> ReportActor {
    ReportActor {
        user_id: auth.session.user_id,
        role: auth.session.role,
        display_name: auth.session.display_name.clone(),
    }
}

/// Mirrors `ReportPrintSchema`.
#[derive(Debug, Serialize)]
pub struct ReportPrint {
    pub(crate) report: PeriodReport,
    pub(crate) printed: bool,
    pub(crate) print_error: Option<String>,
    pub(crate) text: String,
}

fn print(
    auth: &Authorized,
    printer: &PrintService,
    client: &pos_core::config::ClientConfig,
    report: PeriodReport,
) -> ReportPrint {
    let language = printer.current_language(&auth.db);
    let doc = reports::to_doc(&report, client, Zone::System, language);
    let text = pos_hardware::report::render_text(&doc, printer.paper_width_mm(&auth.db));
    let (printed, print_error) = match printer.print_report(&auth.db, &doc) {
        Ok(()) => (true, None),
        Err(e) => (false, Some(e.message)),
    };
    ReportPrint {
        report,
        printed,
        print_error,
        text,
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_x_report(state: State<'_, AppState>) -> IpcResult<PeriodReport> {
    let auth = authorize(&state, Permission::ReportView)?;
    let currency = state.client.currency.base;
    blocking(move || reports::x_report(&auth.db.conn(), &by(&auth), currency, SystemClock.now()))
        .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn run_z_report(state: State<'_, AppState>) -> IpcResult<ReportPrint> {
    let auth = authorize(&state, Permission::ReportZRun)?;
    let client = Arc::clone(&state.client);
    let printer = Arc::clone(&state.printer);
    let backups = Arc::clone(&state.backups);
    blocking(move || {
        let report = reports::run_z(
            &mut auth.db.conn(),
            &by(&auth),
            client.currency.base,
            SystemClock.now(),
        )?;
        // The closed day goes into a backup straight away.
        backups.in_background(Arc::clone(&auth.db), crate::backup::Reason::ZReport);
        Ok(print(&auth, &printer, &client, report))
    })
    .await
    .inspect(|_| state.sync.nudge())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_z_reports(
    state: State<'_, AppState>,
    device_id: Option<Uuid>,
    limit: i64,
    offset: i64,
) -> IpcResult<Vec<ZReportSummary>> {
    let auth = authorize(&state, Permission::ReportView)?;
    blocking(move || reports::z_list(&auth.db.conn(), device_id, limit, offset)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_z_report(
    state: State<'_, AppState>,
    z_report_id: Uuid,
) -> IpcResult<PeriodReport> {
    let auth = authorize(&state, Permission::ReportView)?;
    blocking(move || reports::z_get(&auth.db.conn(), z_report_id)).await
}

/// A stored Z again (identical), or the X report now (`None`).
#[tauri::command(rename_all = "snake_case")]
pub async fn print_report(
    state: State<'_, AppState>,
    z_report_id: Option<Uuid>,
) -> IpcResult<ReportPrint> {
    let auth = authorize(&state, Permission::ReportView)?;
    let client = Arc::clone(&state.client);
    let printer = Arc::clone(&state.printer);
    blocking(move || {
        let report = match z_report_id {
            Some(id) => reports::z_get(&auth.db.conn(), id)?,
            None => reports::x_report(
                &auth.db.conn(),
                &by(&auth),
                client.currency.base,
                SystemClock.now(),
            )?,
        };
        Ok(print(&auth, &printer, &client, report))
    })
    .await
}

/// Mirrors `ShiftFilterSchema`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ShiftFilter {
    pub from: Option<Timestamp>,
    pub to: Option<Timestamp>,
    pub device_id: Option<Uuid>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

/// Mirrors `ShiftHistoryItemSchema`.
#[derive(Debug, Serialize)]
pub struct ShiftHistoryItem {
    pub(crate) shift: Shift,
    pub(crate) device_label: String,
    pub(crate) opened_by_name: String,
    pub(crate) closed_by_name: Option<String>,
    pub(crate) totals: ShiftTotals,
    /// Stored at close; while open, the float plus net cash so far.
    pub(crate) expected_cash: i64,
}

pub(crate) fn shift_history(
    conn: &rusqlite::Connection,
    filter: &ShiftFilter,
) -> IpcResult<Vec<ShiftHistoryItem>> {
    let name = |id: Uuid| -> IpcResult<String> {
        Ok(conn
            .query_row(
                "SELECT display_name FROM users WHERE id = ?1",
                [id.to_string()],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .unwrap_or_default())
    };
    shifts::list(
        conn,
        filter.from,
        filter.to,
        filter.device_id,
        filter.limit.clamp(1, 500),
        filter.offset.max(0),
    )
    .ipc()?
    .into_iter()
    .map(|shift| {
        let totals = shifts::totals(conn, shift.meta.id).ipc()?;
        Ok(ShiftHistoryItem {
            device_label: device::receipt_prefix(shift.device_id),
            opened_by_name: name(shift.opened_by)?,
            closed_by_name: shift.closed_by.map(name).transpose()?,
            expected_cash: shift
                .expected_cash
                .unwrap_or(shift.opening_float + totals.cash_total),
            totals,
            shift,
        })
    })
    .collect()
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_shifts(
    state: State<'_, AppState>,
    filter: ShiftFilter,
) -> IpcResult<Vec<ShiftHistoryItem>> {
    let auth = authorize(&state, Permission::ReportView)?;
    blocking(move || shift_history(&auth.db.conn(), &filter)).await
}

/// Mirrors `DashboardRequestSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct DashboardRequest {
    pub range: DateRange,
    pub device_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct DateRange {
    pub from: Timestamp,
    pub to: Timestamp,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn get_dashboard_metrics(
    state: State<'_, AppState>,
    request: DashboardRequest,
) -> IpcResult<DashboardData> {
    let auth = authorize(&state, Permission::AnalyticsView)?;
    let currency = state.client.currency.base;
    blocking(move || {
        reports::dashboard(
            &auth.db.conn(),
            Window {
                from: request.range.from,
                to: request.range.to,
                device_id: request.device_id,
            },
            Zone::System,
            currency,
        )
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_audit_log(
    state: State<'_, AppState>,
    filter: AuditFilter,
) -> IpcResult<AuditPage> {
    let auth = authorize(&state, Permission::AuditView)?;
    blocking(move || audit::list(&auth.db.conn(), &filter).ipc()).await
}

//! Reports from the immutable transaction rows: period totals (shared by X/Z
//! reports and the dashboard), X and Z reports per till, and the analytics
//! dashboard for the whole shop or one till.
//!
//! Periods are half-open `[from, to)`. A Z covers this till from the instant
//! after the previous Z's `period_end` up to and including its own; local
//! hours and days use the till's time zone.

use std::collections::BTreeMap;

use chrono::{Duration, NaiveDate, Timelike};
use pos_core::config::{ClientConfig, Locale};
use pos_core::currency::CurrencyCode;
use pos_core::money::to_decimal_string;
use pos_core::rbac::Role;
use pos_core::sales::{OrderType, PaymentMethod, TransactionKind};
use pos_core::time::{Timestamp, Zone};
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use pos_hardware::report::{ReportDoc, ReportRow, ReportSection};
use pos_hardware::words::words;
use rusqlite::{params, Connection, OptionalExtension, Params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::repo::audit::{self, Actor};
use crate::repo::outbox::{self, EventType};
use crate::repo::{device, enum_at, opt_uuid_at, shifts, ts_at, uuid_at, Meta, SqlResultExt};

const MILLI: Duration = Duration::milliseconds(1);

fn plus(ts: Timestamp, d: Duration) -> Timestamp {
    ts.checked_add(d).unwrap_or(ts)
}

/// Which transactions a figure covers.
#[derive(Debug, Clone, Copy)]
pub struct Window {
    pub from: Timestamp,
    pub to: Timestamp,
    pub device_id: Option<Uuid>,
}

impl Window {
    /// The same length of time just before this one.
    fn previous(self) -> Self {
        let length = self.to.signed_duration_since(self.from);
        Self {
            from: plus(self.from, -length),
            to: self.from,
            device_id: self.device_id,
        }
    }
}

const W: &str = "t.occurred_at >= ?1 AND t.occurred_at < ?2 AND (?3 IS NULL OR t.device_id = ?3)";
/// +1 for sales, −1 for refunds and voids (their item rows are positive).
const SIGN: &str = "(CASE WHEN t.kind = 'sale' THEN 1 ELSE -1 END)";

fn query<T, P: Params>(
    conn: &Connection,
    sql: &str,
    params: P,
    map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> IpcResult<Vec<T>> {
    conn.prepare(sql)
        .ipc()?
        .query_map(params, map)
        .ipc()?
        .collect::<Result<_, _>>()
        .ipc()
}

macro_rules! window_params {
    ($w:expr) => {
        params![
            $w.from.to_string(),
            $w.to.to_string(),
            $w.device_id.map(|d| d.to_string())
        ]
    };
}

// ── period totals ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxRateTotal {
    pub rate_bps: i64,
    pub taxable_amount: i64,
    pub tax_amount: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MethodTotal {
    pub method: PaymentMethod,
    pub amount: i64,
    pub count: i64,
}

/// Mirrors `PeriodTotalsSchema`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeriodTotals {
    pub sale_count: i64,
    pub gross_sales: i64,
    pub discount_total: i64,
    pub refund_count: i64,
    pub refund_total: i64,
    pub void_count: i64,
    pub void_total: i64,
    pub net_sales: i64,
    pub tax_total: i64,
    pub by_tax_rate: Vec<TaxRateTotal>,
    pub by_payment_method: Vec<MethodTotal>,
    pub first_receipt: Option<String>,
    pub last_receipt: Option<String>,
}

pub fn totals(conn: &Connection, w: Window) -> IpcResult<PeriodTotals> {
    let mut t = conn
        .query_row(
            &format!(
                "SELECT COALESCE(SUM(t.kind = 'sale'), 0),
                        COALESCE(SUM(CASE WHEN t.kind = 'sale' THEN t.subtotal END), 0),
                        COALESCE(SUM(CASE WHEN t.kind = 'sale' THEN t.discount_total END), 0),
                        COALESCE(SUM(t.kind = 'refund'), 0),
                        COALESCE(-SUM(CASE WHEN t.kind = 'refund' THEN t.total END), 0),
                        COALESCE(SUM(t.kind = 'void'), 0),
                        COALESCE(-SUM(CASE WHEN t.kind = 'void' THEN t.total END), 0),
                        COALESCE(SUM(t.total), 0), COALESCE(SUM(t.tax_total), 0)
                 FROM transactions t WHERE {W}"
            ),
            window_params!(w),
            |r| {
                Ok(PeriodTotals {
                    sale_count: r.get(0)?,
                    gross_sales: r.get(1)?,
                    discount_total: r.get(2)?,
                    refund_count: r.get(3)?,
                    refund_total: r.get(4)?,
                    void_count: r.get(5)?,
                    void_total: r.get(6)?,
                    net_sales: r.get(7)?,
                    tax_total: r.get(8)?,
                    ..PeriodTotals::default()
                })
            },
        )
        .ipc()?;
    t.by_tax_rate = query(
        conn,
        &format!(
            "SELECT i.tax_rate_bps, SUM({SIGN} * (i.line_total - i.tax_amount)), SUM({SIGN} * i.tax_amount)
             FROM transaction_items i JOIN transactions t ON t.id = i.transaction_id
             WHERE {W} GROUP BY i.tax_rate_bps ORDER BY i.tax_rate_bps"
        ),
        window_params!(w),
        |r| {
            Ok(TaxRateTotal {
                rate_bps: r.get(0)?,
                taxable_amount: r.get(1)?,
                tax_amount: r.get(2)?,
            })
        },
    )?;
    t.by_payment_method = query(
        conn,
        &format!(
            "SELECT p.method, SUM(p.amount), COUNT(*)
             FROM transaction_payments p JOIN transactions t ON t.id = p.transaction_id
             WHERE {W} GROUP BY p.method ORDER BY SUM(p.amount) DESC, p.method"
        ),
        window_params!(w),
        |r| {
            Ok(MethodTotal {
                method: enum_at(r, 0)?,
                amount: r.get(1)?,
                count: r.get(2)?,
            })
        },
    )?;
    let receipt = |order: &str| -> IpcResult<Option<String>> {
        conn.query_row(
            &format!(
                "SELECT t.receipt_number FROM transactions t WHERE {W}
                 ORDER BY t.occurred_at {order}, t.id {order} LIMIT 1"
            ),
            window_params!(w),
            |r| r.get(0),
        )
        .optional()
        .ipc()
    };
    t.first_receipt = receipt("ASC")?;
    t.last_receipt = receipt("DESC")?;
    Ok(t)
}

// ── X / Z reports ───────────────────────────────────────────────────────────

/// Mirrors `CashSummarySchema`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CashSummary {
    pub shift_count: i64,
    pub opening_floats: i64,
    pub cash_sales: i64,
    pub cash_refunds: i64,
    pub expected: i64,
    pub counted: Option<i64>,
    pub variance: Option<i64>,
}

/// Mirrors `ReportShiftSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportShift {
    pub shift_id: Uuid,
    pub opened_at: Timestamp,
    pub closed_at: Option<Timestamp>,
    pub opened_by_name: String,
    pub closed_by_name: Option<String>,
    pub expected_cash: i64,
    pub actual_cash: Option<i64>,
    pub variance: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportKind {
    X,
    Z,
}

/// Mirrors `PeriodReportSchema`. Stored as the Z snapshot, so it must read
/// back exactly as it was printed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeriodReport {
    pub kind: ReportKind,
    pub z_report_id: Option<Uuid>,
    pub z_number: Option<i64>,
    pub device_id: Uuid,
    pub device_label: String,
    pub period_start: Timestamp,
    pub period_end: Timestamp,
    pub generated_at: Timestamp,
    pub generated_by_name: String,
    pub currency: CurrencyCode,
    pub totals: PeriodTotals,
    pub cash: CashSummary,
    pub shifts: Vec<ReportShift>,
    pub grand_total: i64,
}

pub struct ReportActor {
    pub user_id: Uuid,
    pub role: Role,
    pub display_name: String,
}

fn user_name(conn: &Connection, id: Uuid) -> IpcResult<String> {
    Ok(conn
        .query_row(
            "SELECT display_name FROM users WHERE id = ?1",
            [id.to_string()],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .ipc()?
        .unwrap_or_default())
}

struct LastZ {
    z_number: i64,
    period_end: Timestamp,
    grand_total: i64,
}

fn last_z(conn: &Connection, device_id: Uuid) -> IpcResult<Option<LastZ>> {
    conn.query_row(
        "SELECT z_number, period_end, grand_total FROM z_reports
         WHERE device_id = ?1 AND deleted_at IS NULL ORDER BY z_number DESC LIMIT 1",
        [device_id.to_string()],
        |r| {
            Ok(LastZ {
                z_number: r.get(0)?,
                period_end: ts_at(r, 1)?,
                grand_total: r.get(2)?,
            })
        },
    )
    .optional()
    .ipc()
}

/// The first instant a new report of this till covers.
fn period_start(
    conn: &Connection,
    device_id: Uuid,
    last: Option<&LastZ>,
    now: Timestamp,
) -> IpcResult<Timestamp> {
    if let Some(last) = last {
        return Ok(plus(last.period_end, MILLI));
    }
    let first: Option<String> = conn
        .query_row(
            "SELECT MIN(at) FROM (
               SELECT MIN(occurred_at) AS at FROM transactions WHERE device_id = ?1
               UNION ALL SELECT MIN(opened_at) FROM shifts WHERE device_id = ?1)",
            [device_id.to_string()],
            |r| r.get(0),
        )
        .ipc()?;
    Ok(first
        .and_then(|s| s.parse().ok())
        .map_or(now, |first: Timestamp| first.min(now)))
}

fn build(
    conn: &Connection,
    kind: ReportKind,
    device_id: Uuid,
    by: &ReportActor,
    currency: CurrencyCode,
    now: Timestamp,
) -> IpcResult<(PeriodReport, Option<LastZ>)> {
    let last = last_z(conn, device_id)?;
    let start = period_start(conn, device_id, last.as_ref(), now)?;
    let window = Window {
        from: start,
        to: plus(now, MILLI),
        device_id: Some(device_id),
    };
    let totals = totals(conn, window)?;
    let period_shifts = shifts::in_period(
        conn,
        device_id,
        window.from,
        window.to,
        kind == ReportKind::X,
    )
    .ipc()?;

    let (cash_sales, cash_refunds): (i64, i64) = conn
        .query_row(
            &format!(
                "SELECT COALESCE(SUM(CASE WHEN p.amount > 0 THEN p.amount END), 0),
                        COALESCE(-SUM(CASE WHEN p.amount < 0 THEN p.amount END), 0)
                 FROM transaction_payments p JOIN transactions t ON t.id = p.transaction_id
                 WHERE {W} AND p.method = 'cash'"
            ),
            window_params!(window),
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ipc()?;
    let mut report_shifts = Vec::new();
    for s in &period_shifts {
        let expected = match s.expected_cash {
            Some(expected) => expected,
            None => s.opening_float + shifts::totals(conn, s.meta.id).ipc()?.cash_total,
        };
        report_shifts.push(ReportShift {
            shift_id: s.meta.id,
            opened_at: s.opened_at,
            closed_at: s.closed_at,
            opened_by_name: user_name(conn, s.opened_by)?,
            closed_by_name: s.closed_by.map(|id| user_name(conn, id)).transpose()?,
            expected_cash: expected,
            actual_cash: s.actual_cash,
            variance: s.variance,
        });
    }
    let opening_floats: i64 = period_shifts.iter().map(|s| s.opening_float).sum();
    let expected = opening_floats + cash_sales - cash_refunds;
    let all_closed =
        !period_shifts.is_empty() && period_shifts.iter().all(|s| s.closed_at.is_some());
    let counted = all_closed.then(|| {
        period_shifts
            .iter()
            .filter_map(|s| s.actual_cash)
            .sum::<i64>()
    });
    let cash = CashSummary {
        shift_count: i64::try_from(period_shifts.len()).unwrap_or(i64::MAX),
        opening_floats,
        cash_sales,
        cash_refunds,
        expected,
        counted,
        variance: counted.map(|c| c - expected),
    };
    let grand_total = last.as_ref().map_or(0, |l| l.grand_total) + totals.net_sales;
    let report = PeriodReport {
        kind,
        z_report_id: None,
        z_number: None,
        device_id,
        device_label: device::receipt_prefix(device_id),
        period_start: start,
        period_end: now,
        generated_at: now,
        generated_by_name: by.display_name.clone(),
        currency,
        totals,
        cash,
        shifts: report_shifts,
        grand_total,
    };
    Ok((report, last))
}

/// This till since its last Z, closing nothing.
pub fn x_report(
    conn: &Connection,
    by: &ReportActor,
    currency: CurrencyCode,
    now: Timestamp,
) -> IpcResult<PeriodReport> {
    let device_id = device::id(conn).ipc()?;
    Ok(build(conn, ReportKind::X, device_id, by, currency, now)?.0)
}

/// Closes the period: numbers it, stores the snapshot (append-only,
/// synced) and audits it. Every shift of this till must be closed.
pub fn run_z(
    conn: &mut Connection,
    by: &ReportActor,
    currency: CurrencyCode,
    now: Timestamp,
) -> IpcResult<PeriodReport> {
    let tx = conn.transaction().ipc()?;
    let device_id = device::id(&tx).ipc()?;
    if shifts::current_open(&tx, device_id).ipc()?.is_some() {
        return Err(IpcError::new(
            IpcErrorCode::Conflict,
            "Close the shift before running the Z report.",
        ));
    }
    let (mut report, last) = build(&tx, ReportKind::Z, device_id, by, currency, now)?;
    let meta = Meta::new(now);
    let z_number = last.as_ref().map_or(1, |l| l.z_number + 1);
    report.z_report_id = Some(meta.id);
    report.z_number = Some(z_number);
    let snapshot = serde_json::to_value(&report)
        .map_err(|e| IpcError::internal(format!("report snapshot: {e}")))?;
    let t = &report.totals;
    let row = ZReportRow {
        meta: meta.clone(),
        device_id,
        z_number,
        period_start: report.period_start,
        period_end: report.period_end,
        run_by: by.user_id,
        currency,
        sale_count: t.sale_count,
        refund_count: t.refund_count,
        void_count: t.void_count,
        gross_sales: t.gross_sales,
        discount_total: t.discount_total,
        refund_total: t.refund_total,
        void_total: t.void_total,
        net_sales: t.net_sales,
        tax_total: t.tax_total,
        grand_total: report.grand_total,
        report: snapshot,
    };
    tx.execute(
        "INSERT INTO z_reports (id, created_at, updated_at, device_id, z_number, period_start, period_end,
            run_by, currency, sale_count, refund_count, void_count, gross_sales, discount_total,
            refund_total, void_total, net_sales, tax_total, grand_total, report)
         VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
        params![
            row.meta.id.to_string(),
            now.to_string(),
            device_id.to_string(),
            z_number,
            row.period_start.to_string(),
            row.period_end.to_string(),
            by.user_id.to_string(),
            currency.as_str(),
            row.sale_count,
            row.refund_count,
            row.void_count,
            row.gross_sales,
            row.discount_total,
            row.refund_total,
            row.void_total,
            row.net_sales,
            row.tax_total,
            row.grand_total,
            row.report.to_string(),
        ],
    )
    .ipc()?;
    outbox::record(&tx, "z_reports", EventType::Append, row.meta.id, &row, now).ipc()?;
    audit::record(
        &tx,
        &Actor {
            user_id: by.user_id,
            role: by.role,
            device_id,
        },
        "report.z_run",
        "z_reports",
        Some(row.meta.id),
        None,
        Some(serde_json::json!({
            "z_number": z_number,
            "net_sales": row.net_sales,
            "grand_total": row.grand_total,
            "variance": report.cash.variance,
        })),
        now,
    )
    .ipc()?;
    tx.commit().ipc()?;
    Ok(report)
}

/// Mirrors `ZReportSchema` (the synced row).
#[derive(Debug, Clone, Serialize)]
struct ZReportRow {
    #[serde(flatten)]
    meta: Meta,
    device_id: Uuid,
    z_number: i64,
    period_start: Timestamp,
    period_end: Timestamp,
    run_by: Uuid,
    currency: CurrencyCode,
    sale_count: i64,
    refund_count: i64,
    void_count: i64,
    gross_sales: i64,
    discount_total: i64,
    refund_total: i64,
    void_total: i64,
    net_sales: i64,
    tax_total: i64,
    grand_total: i64,
    report: serde_json::Value,
}

/// Mirrors `ZReportSummarySchema`.
#[derive(Debug, Clone, Serialize)]
pub struct ZReportSummary {
    pub id: Uuid,
    pub device_id: Uuid,
    pub device_label: String,
    pub z_number: i64,
    pub period_start: Timestamp,
    pub period_end: Timestamp,
    pub run_by_name: String,
    pub sale_count: i64,
    pub net_sales: i64,
    pub grand_total: i64,
}

pub fn z_list(
    conn: &Connection,
    device_id: Option<Uuid>,
    limit: i64,
    offset: i64,
) -> IpcResult<Vec<ZReportSummary>> {
    query(
        conn,
        "SELECT z.id, z.device_id, z.z_number, z.period_start, z.period_end, COALESCE(u.display_name, ''),
                z.sale_count, z.net_sales, z.grand_total
         FROM z_reports z LEFT JOIN users u ON u.id = z.run_by
         WHERE z.deleted_at IS NULL AND (?1 IS NULL OR z.device_id = ?1)
         ORDER BY z.period_end DESC, z.id DESC LIMIT ?2 OFFSET ?3",
        params![device_id.map(|d| d.to_string()), limit.clamp(1, 500), offset.max(0)],
        |r| {
            let device_id = uuid_at(r, 1)?;
            Ok(ZReportSummary {
                id: uuid_at(r, 0)?,
                device_id,
                device_label: device::receipt_prefix(device_id),
                z_number: r.get(2)?,
                period_start: ts_at(r, 3)?,
                period_end: ts_at(r, 4)?,
                run_by_name: r.get(5)?,
                sale_count: r.get(6)?,
                net_sales: r.get(7)?,
                grand_total: r.get(8)?,
            })
        },
    )
}

/// A stored Z exactly as it was printed.
pub fn z_get(conn: &Connection, id: Uuid) -> IpcResult<PeriodReport> {
    let raw: String = conn
        .query_row(
            "SELECT report FROM z_reports WHERE id = ?1 AND deleted_at IS NULL",
            [id.to_string()],
            |r| r.get(0),
        )
        .optional()
        .ipc()?
        .ok_or_else(|| IpcError::new(IpcErrorCode::NotFound, "That Z report does not exist."))?;
    serde_json::from_str(&raw).map_err(|e| IpcError::internal(format!("stored Z report: {e}")))
}

fn money(amount: i64, currency: CurrencyCode) -> String {
    to_decimal_string(amount, currency)
}

fn percent(bps: i64) -> String {
    let whole = bps / 100;
    let frac = bps % 100;
    if frac == 0 {
        format!("{whole}%")
    } else {
        format!("{whole}.{}%", format!("{frac:02}").trim_end_matches('0'))
    }
}

/// The printed layout of an X or Z report, worded in `language`.
pub fn to_doc(
    report: &PeriodReport,
    client: &ClientConfig,
    zone: Zone,
    language: Locale,
) -> ReportDoc {
    let w = words(language);
    let c = report.currency;
    let t = &report.totals;
    let title = match (report.kind, report.z_number) {
        (ReportKind::Z, Some(n)) => format!("{} #{n}", w.z_report),
        (ReportKind::Z, None) => w.z_report.into(),
        (ReportKind::X, _) => w.x_report.into(),
    };
    let mut lines = vec![
        client.display_name.clone(),
        format!("{} {}", w.till, report.device_label),
        format!("{} {}", w.from, zone.format_minutes(report.period_start)),
        format!("{} {}", w.to, zone.format_minutes(report.period_end)),
    ];
    if report.kind == ReportKind::X {
        lines.push(w.not_closed.into());
    }
    let mut sales = vec![
        ReportRow::pair(
            format!("{} ({})", w.sales, t.sale_count),
            money(t.gross_sales, c),
        ),
        ReportRow::pair(w.discounts, money(-t.discount_total, c)),
        ReportRow::pair(
            format!("{} ({})", w.refunds, t.refund_count),
            money(-t.refund_total, c),
        ),
        ReportRow::pair(
            format!("{} ({})", w.voids, t.void_count),
            money(-t.void_total, c),
        ),
    ];
    sales.push(ReportRow::total(w.net_sales, money(t.net_sales, c)));
    let mut tax: Vec<ReportRow> = t
        .by_tax_rate
        .iter()
        .filter(|r| r.rate_bps > 0)
        .map(|r| {
            ReportRow::pair(
                format!(
                    "{} {} {}",
                    percent(r.rate_bps),
                    w.on,
                    money(r.taxable_amount, c)
                ),
                money(r.tax_amount, c),
            )
        })
        .collect();
    tax.push(ReportRow::total(w.tax, money(t.tax_total, c)));
    let payments: Vec<ReportRow> = if t.by_payment_method.is_empty() {
        vec![ReportRow::Text(w.no_payments.into())]
    } else {
        t.by_payment_method
            .iter()
            .map(|m| {
                ReportRow::pair(
                    format!("{} ({})", w.method(m.method), m.count),
                    money(m.amount, c),
                )
            })
            .collect()
    };
    let cash = &report.cash;
    let mut drawer = vec![
        ReportRow::pair(w.opening_floats, money(cash.opening_floats, c)),
        ReportRow::pair(w.cash_sales, money(cash.cash_sales, c)),
        ReportRow::pair(w.cash_refunds, money(-cash.cash_refunds, c)),
        ReportRow::total(w.expected, money(cash.expected, c)),
    ];
    match (cash.counted, cash.variance) {
        (Some(counted), Some(variance)) => {
            drawer.push(ReportRow::pair(w.counted, money(counted, c)));
            drawer.push(ReportRow::total(w.variance, money(variance, c)));
        }
        _ => drawer.push(ReportRow::Text(w.shift_open.into())),
    }
    let shift_rows: Vec<ReportRow> = report
        .shifts
        .iter()
        .map(|s| {
            let span = format!(
                "{}-{} {}",
                zone.format_time(s.opened_at).trim_end_matches(" UTC"),
                s.closed_at
                    .map_or_else(|| w.open.into(), |at| zone.format_time(at)),
                s.opened_by_name
            );
            ReportRow::pair(span, s.variance.map_or_else(|| "-".into(), |v| money(v, c)))
        })
        .collect();
    let mut sections = vec![
        ReportSection {
            heading: Some(w.sales.into()),
            rows: sales,
        },
        ReportSection {
            heading: Some(w.tax.into()),
            rows: tax,
        },
        ReportSection {
            heading: Some(w.payments_net.into()),
            rows: payments,
        },
        ReportSection {
            heading: Some(w.cash_drawer.into()),
            rows: drawer,
        },
    ];
    if !shift_rows.is_empty() {
        sections.push(ReportSection {
            heading: Some(format!("{} ({})", w.shifts, shift_rows.len())),
            rows: shift_rows,
        });
    }
    sections.push(ReportSection {
        heading: None,
        rows: vec![
            ReportRow::pair(
                w.first_receipt,
                t.first_receipt.clone().unwrap_or_else(|| "-".into()),
            ),
            ReportRow::pair(
                w.last_receipt,
                t.last_receipt.clone().unwrap_or_else(|| "-".into()),
            ),
            ReportRow::total(w.grand_total, money(report.grand_total, c)),
            ReportRow::Text(format!(
                "{} {}",
                report.generated_by_name,
                zone.format_minutes(report.generated_at)
            )),
        ],
    });
    ReportDoc {
        title,
        lines,
        sections,
    }
}

// ── dashboard ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct HourBucket {
    pub hour: u32,
    pub amount: i64,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DayBucket {
    pub date: String,
    pub amount: i64,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProductTotal {
    pub product_id: Uuid,
    pub name: String,
    pub quantity_milli: i64,
    pub amount: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryTotal {
    pub category_id: Option<Uuid>,
    pub name: Option<String>,
    pub quantity_milli: i64,
    pub amount: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CashierTotal {
    pub user_id: Uuid,
    pub name: String,
    pub amount: i64,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderTypeTotal {
    pub order_type: OrderType,
    pub amount: i64,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Previous {
    pub net_sales: i64,
    pub sale_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Range {
    pub from: Timestamp,
    pub to: Timestamp,
}

/// Mirrors `DashboardDataSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct DashboardData {
    pub range: Range,
    pub currency: CurrencyCode,
    pub totals: PeriodTotals,
    pub average_ticket: i64,
    pub by_hour: Vec<HourBucket>,
    pub by_day: Vec<DayBucket>,
    pub top_products: Vec<ProductTotal>,
    pub by_category: Vec<CategoryTotal>,
    pub by_cashier: Vec<CashierTotal>,
    pub by_order_type: Vec<OrderTypeTotal>,
    pub previous: Previous,
    pub low_stock_count: i64,
    pub this_device_id: Uuid,
    pub devices: Vec<DeviceRef>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceRef {
    pub device_id: Uuid,
    pub label: String,
}

/// Longest range the dashboard charts day by day.
pub const MAX_DAYS: i64 = 400;

pub fn dashboard(
    conn: &Connection,
    w: Window,
    zone: Zone,
    currency: CurrencyCode,
) -> IpcResult<DashboardData> {
    if w.to <= w.from {
        return Err(IpcError::validation("The range must end after it starts."));
    }
    if w.to.signed_duration_since(w.from) > Duration::days(MAX_DAYS) {
        return Err(IpcError::validation(format!(
            "Choose at most {MAX_DAYS} days."
        )));
    }
    let totals = totals(conn, w)?;
    let sales_sum: i64 = conn
        .query_row(
            &format!("SELECT COALESCE(SUM(t.total), 0) FROM transactions t WHERE {W} AND t.kind = 'sale'"),
            window_params!(w),
            |r| r.get(0),
        )
        .ipc()?;
    let average_ticket = if totals.sale_count == 0 {
        0
    } else {
        let n = i128::from(totals.sale_count);
        i64::try_from((i128::from(sales_sum) * 2 + n) / (n * 2)).unwrap_or(0)
    };

    // Local hours and days.
    let rows = query(
        conn,
        &format!("SELECT t.occurred_at, t.kind, t.total FROM transactions t WHERE {W}"),
        window_params!(w),
        |r| {
            Ok((
                ts_at(r, 0)?,
                enum_at::<TransactionKind>(r, 1)?,
                r.get::<_, i64>(2)?,
            ))
        },
    )?;
    let mut by_hour: Vec<HourBucket> = (0..24)
        .map(|hour| HourBucket {
            hour,
            amount: 0,
            count: 0,
        })
        .collect();
    let first_day = zone.local(w.from).date();
    let last_day = zone.local(plus(w.to, -MILLI)).date();
    let mut days: BTreeMap<NaiveDate, (i64, i64)> = BTreeMap::new();
    let mut day = first_day;
    while day <= last_day {
        days.insert(day, (0, 0));
        day = day.succ_opt().unwrap_or(last_day + Duration::days(1));
    }
    for (at, kind, total) in rows {
        let local = zone.local(at);
        let sale = i64::from(kind == TransactionKind::Sale);
        if let Some(bucket) = by_hour.get_mut(local.hour() as usize) {
            bucket.amount += total;
            bucket.count += sale;
        }
        let entry = days.entry(local.date()).or_insert((0, 0));
        entry.0 += total;
        entry.1 += sale;
    }
    let by_day = days
        .into_iter()
        .map(|(date, (amount, count))| DayBucket {
            date: date.format("%Y-%m-%d").to_string(),
            amount,
            count,
        })
        .collect();

    let top_products = query(
        conn,
        &format!(
            "SELECT i.product_id, MAX(i.product_name), SUM({SIGN} * i.quantity_milli), SUM({SIGN} * i.line_total)
             FROM transaction_items i JOIN transactions t ON t.id = i.transaction_id
             WHERE {W} GROUP BY i.product_id ORDER BY 4 DESC, 2 LIMIT 10"
        ),
        window_params!(w),
        |r| {
            Ok(ProductTotal {
                product_id: uuid_at(r, 0)?,
                name: r.get(1)?,
                quantity_milli: r.get(2)?,
                amount: r.get(3)?,
            })
        },
    )?;
    let by_category = query(
        conn,
        &format!(
            "SELECT p.category_id, c.name, SUM({SIGN} * i.quantity_milli), SUM({SIGN} * i.line_total)
             FROM transaction_items i JOIN transactions t ON t.id = i.transaction_id
             LEFT JOIN products p ON p.id = i.product_id
             LEFT JOIN categories c ON c.id = p.category_id
             WHERE {W} GROUP BY p.category_id ORDER BY 4 DESC"
        ),
        window_params!(w),
        |r| {
            Ok(CategoryTotal {
                category_id: opt_uuid_at(r, 0)?,
                name: r.get(1)?,
                quantity_milli: r.get(2)?,
                amount: r.get(3)?,
            })
        },
    )?;
    // Sales rung up by each cashier (refunds are the manager's, not theirs).
    let by_cashier = query(
        conn,
        &format!(
            "SELECT t.cashier_id, COALESCE(u.display_name, ''), SUM(t.total), COUNT(*)
             FROM transactions t LEFT JOIN users u ON u.id = t.cashier_id
             WHERE {W} AND t.kind = 'sale' GROUP BY t.cashier_id ORDER BY 3 DESC"
        ),
        window_params!(w),
        |r| {
            Ok(CashierTotal {
                user_id: uuid_at(r, 0)?,
                name: r.get(1)?,
                amount: r.get(2)?,
                count: r.get(3)?,
            })
        },
    )?;
    let by_order_type = query(
        conn,
        &format!(
            "SELECT t.order_type, SUM(t.total), SUM(t.kind = 'sale')
             FROM transactions t WHERE {W} GROUP BY t.order_type ORDER BY 2 DESC"
        ),
        window_params!(w),
        |r| {
            Ok(OrderTypeTotal {
                order_type: enum_at(r, 0)?,
                amount: r.get(1)?,
                count: r.get(2)?,
            })
        },
    )?;
    let previous = conn
        .query_row(
            &format!(
                "SELECT COALESCE(SUM(t.total), 0), COALESCE(SUM(t.kind = 'sale'), 0)
                 FROM transactions t WHERE {W}"
            ),
            window_params!(w.previous()),
            |r| {
                Ok(Previous {
                    net_sales: r.get(0)?,
                    sale_count: r.get(1)?,
                })
            },
        )
        .ipc()?;
    let low_stock_count = conn
        .query_row(
            "SELECT count(*) FROM products WHERE deleted_at IS NULL AND is_active = 1 AND track_stock = 1
               AND reorder_threshold_milli IS NOT NULL AND stock_on_hand_milli <= reorder_threshold_milli",
            [],
            |r| r.get(0),
        )
        .ipc()?;
    let this_device_id = device::id(conn).ipc()?;
    let mut ids = query(
        conn,
        "SELECT DISTINCT device_id FROM transactions",
        [],
        |r| uuid_at(r, 0),
    )?;
    if !ids.contains(&this_device_id) {
        ids.push(this_device_id);
    }
    let mut devices: Vec<DeviceRef> = ids
        .into_iter()
        .map(|device_id| DeviceRef {
            device_id,
            label: device::receipt_prefix(device_id),
        })
        .collect();
    devices.sort_by(|a, b| a.label.cmp(&b.label));
    Ok(DashboardData {
        range: Range {
            from: w.from,
            to: w.to,
        },
        currency,
        totals,
        average_ticket,
        by_hour,
        by_day,
        top_products,
        by_category,
        by_cashier,
        by_order_type,
        previous,
        low_stock_count,
        this_device_id,
        devices,
    })
}

#[cfg(test)]
mod tests;

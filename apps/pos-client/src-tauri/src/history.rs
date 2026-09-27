//! Sales history: every sale, refund and void of the shop (other tills'
//! arrive through sync), newest first, and one transaction in detail with
//! what can still be refunded or voided.

use pos_core::currency::CurrencyCode;
use pos_core::receipt::Receipt;
use pos_core::sales::{OrderType, PaymentMethod, TransactionKind};
use pos_core::time::Timestamp;
use pos_core::IpcResult;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::refunds::{self, LineView};
use crate::repo::{
    enum_at, enum_str, opt_uuid_at, print_jobs, sales, ts_at, uuid_at, SqlResultExt,
};

/// Mirrors `TransactionFilterSchema`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TransactionFilter {
    pub from: Option<Timestamp>,
    pub to: Option<Timestamp>,
    pub search: Option<String>,
    pub kind: Option<TransactionKind>,
    pub device_id: Option<Uuid>,
    #[serde(default = "default_limit")]
    pub limit: i64,
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

/// Mirrors `TransactionSummarySchema`.
#[derive(Debug, Clone, Serialize)]
pub struct TransactionSummary {
    pub id: Uuid,
    pub kind: TransactionKind,
    pub receipt_number: String,
    pub original_id: Option<Uuid>,
    pub original_receipt_number: Option<String>,
    pub occurred_at: Timestamp,
    pub device_id: Uuid,
    pub shift_id: Uuid,
    pub cashier_name: String,
    pub approved_by_name: Option<String>,
    pub order_type: OrderType,
    pub table_label: Option<String>,
    pub currency: CurrencyCode,
    pub total: i64,
    pub line_count: i64,
    pub payment_methods: Vec<PaymentMethod>,
    pub reversed_total: i64,
    pub notes: Option<String>,
}

const SUMMARY: &str = "
    SELECT t.id, t.kind, t.receipt_number, t.original_transaction_id, o.receipt_number, t.occurred_at,
           t.device_id, t.shift_id, COALESCE(u.display_name, ''), a.display_name, t.order_type,
           t.table_label, t.currency, t.total,
           (SELECT count(*) FROM transaction_items i WHERE i.transaction_id = t.id),
           (SELECT group_concat(DISTINCT p.method) FROM transaction_payments p WHERE p.transaction_id = t.id),
           COALESCE((SELECT -SUM(r.total) FROM transactions r WHERE r.original_transaction_id = t.id), 0),
           t.notes
    FROM transactions t
    LEFT JOIN transactions o ON o.id = t.original_transaction_id
    LEFT JOIN users u ON u.id = t.cashier_id
    LEFT JOIN users a ON a.id = t.approved_by";

fn read(r: &rusqlite::Row<'_>) -> rusqlite::Result<TransactionSummary> {
    let methods: Option<String> = r.get(15)?;
    let mut payment_methods: Vec<PaymentMethod> = methods
        .unwrap_or_default()
        .split(',')
        .filter(|m| !m.is_empty())
        .filter_map(|m| serde_json::from_value(serde_json::Value::String(m.to_owned())).ok())
        .collect();
    payment_methods.sort_by_key(enum_str);
    Ok(TransactionSummary {
        id: uuid_at(r, 0)?,
        kind: enum_at(r, 1)?,
        receipt_number: r.get(2)?,
        original_id: opt_uuid_at(r, 3)?,
        original_receipt_number: r.get(4)?,
        occurred_at: ts_at(r, 5)?,
        device_id: uuid_at(r, 6)?,
        shift_id: uuid_at(r, 7)?,
        cashier_name: r.get(8)?,
        approved_by_name: r.get(9)?,
        order_type: enum_at(r, 10)?,
        table_label: r.get(11)?,
        currency: enum_at(r, 12)?,
        total: r.get(13)?,
        line_count: r.get(14)?,
        payment_methods,
        reversed_total: r.get(16)?,
        notes: r.get(17)?,
    })
}

/// `%` and `_` typed by the user match themselves.
fn like_pattern(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

pub fn list(conn: &Connection, filter: &TransactionFilter) -> IpcResult<Vec<TransactionSummary>> {
    let search = filter
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(like_pattern);
    conn.prepare(&format!(
        "{SUMMARY}
         WHERE (?1 IS NULL OR t.occurred_at >= ?1) AND (?2 IS NULL OR t.occurred_at < ?2)
           AND (?3 IS NULL OR t.kind = ?3) AND (?4 IS NULL OR t.device_id = ?4)
           AND (?5 IS NULL OR t.receipt_number LIKE ?5 ESCAPE '\\'
                OR EXISTS (SELECT 1 FROM transaction_items i
                           WHERE i.transaction_id = t.id AND i.product_name LIKE ?5 ESCAPE '\\'))
         ORDER BY t.occurred_at DESC, t.id DESC LIMIT ?6 OFFSET ?7"
    ))
    .ipc()?
    .query_map(
        params![
            filter.from.map(|t| t.to_string()),
            filter.to.map(|t| t.to_string()),
            filter.kind.map(|k| enum_str(&k)),
            filter.device_id.map(|d| d.to_string()),
            search,
            filter.limit.clamp(1, 500),
            filter.offset.max(0),
        ],
        read,
    )
    .ipc()?
    .collect::<Result<_, _>>()
    .ipc()
}

pub fn summary(conn: &Connection, id: Uuid) -> IpcResult<TransactionSummary> {
    conn.query_row(
        &format!("{SUMMARY} WHERE t.id = ?1"),
        [id.to_string()],
        read,
    )
    .ipc()
}

/// Mirrors `TransactionDetailSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct TransactionDetail {
    pub summary: TransactionSummary,
    pub receipt: Receipt,
    pub lines: Vec<LineView>,
    pub reversals: Vec<TransactionSummary>,
    pub can_refund: bool,
    pub void_blocker: Option<String>,
}

pub fn detail(conn: &Connection, id: Uuid) -> IpcResult<TransactionDetail> {
    let header = refunds::header(conn, id)?;
    let summary = summary(conn, id)?;
    let printed = print_jobs::ever_printed(conn, id).ipc()?;
    let receipt = sales::load_receipt(conn, id, printed)?;
    let (lines, reversals, can_refund, void_blocker) = if header.kind == TransactionKind::Sale {
        let lines = refunds::original_lines(conn, id)?;
        let reversals: Vec<TransactionSummary> = conn
            .prepare(&format!(
                "{SUMMARY} WHERE t.original_transaction_id = ?1 ORDER BY t.occurred_at, t.id"
            ))
            .ipc()?
            .query_map([id.to_string()], read)
            .ipc()?
            .collect::<Result<_, _>>()
            .ipc()?;
        let can_refund = lines.iter().any(|l| l.refundable_milli() > 0);
        let blocker = refunds::void_blocker(conn, &header)?;
        (
            lines.iter().map(LineView::from).collect(),
            reversals,
            can_refund,
            blocker,
        )
    } else {
        (
            Vec::new(),
            Vec::new(),
            false,
            Some("Refunds and voids cannot be reversed.".into()),
        )
    };
    Ok(TransactionDetail {
        summary,
        receipt,
        lines,
        reversals,
        can_refund,
        void_blocker,
    })
}

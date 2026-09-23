//! Shifts and cash-float reconciliation.
//!
//! expected_cash = opening_float + Σ cash applied to sales in the shift
//! (applied = tendered − change, so change handed back is already netted).
//! variance = actual_cash − expected_cash (negative = drawer is short).

use pos_core::time::Timestamp;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use uuid::Uuid;

use super::outbox::{self, EventType};
use super::{opt_ts_at, opt_uuid_at, ts_at, uuid_at, Meta, META_COLUMNS};

/// Mirrors `ShiftSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Shift {
    #[serde(flatten)]
    pub meta: Meta,
    pub device_id: Uuid,
    pub opened_by: Uuid,
    pub closed_by: Option<Uuid>,
    pub opened_at: Timestamp,
    pub closed_at: Option<Timestamp>,
    pub opening_float: i64,
    pub closing_float: Option<i64>,
    pub expected_cash: Option<i64>,
    pub actual_cash: Option<i64>,
    pub variance: Option<i64>,
    pub notes: Option<String>,
}

const COLUMNS: &str =
    "device_id, opened_by, closed_by, opened_at, closed_at, opening_float, closing_float,
    expected_cash, actual_cash, variance, notes";

fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Shift> {
    Ok(Shift {
        meta: Meta::read(row, 0)?,
        device_id: uuid_at(row, 4)?,
        opened_by: uuid_at(row, 5)?,
        closed_by: opt_uuid_at(row, 6)?,
        opened_at: ts_at(row, 7)?,
        closed_at: opt_ts_at(row, 8)?,
        opening_float: row.get(9)?,
        closing_float: row.get(10)?,
        expected_cash: row.get(11)?,
        actual_cash: row.get(12)?,
        variance: row.get(13)?,
        notes: row.get(14)?,
    })
}

pub fn current_open(conn: &Connection, device_id: Uuid) -> rusqlite::Result<Option<Shift>> {
    conn.query_row(
        &format!(
            "SELECT {META_COLUMNS}, {COLUMNS} FROM shifts
             WHERE device_id = ?1 AND closed_at IS NULL AND deleted_at IS NULL"
        ),
        [device_id.to_string()],
        read,
    )
    .optional()
}

fn persist(conn: &Connection, s: &Shift, now: Timestamp) -> rusqlite::Result<()> {
    conn.execute(
        &format!(
            "INSERT INTO shifts ({META_COLUMNS}, {COLUMNS})
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT (id) DO UPDATE SET updated_at = excluded.updated_at, closed_by = excluded.closed_by,
               closed_at = excluded.closed_at, closing_float = excluded.closing_float,
               expected_cash = excluded.expected_cash, actual_cash = excluded.actual_cash,
               variance = excluded.variance, notes = excluded.notes"
        ),
        params![
            s.meta.id.to_string(),
            s.meta.created_at.to_string(),
            s.meta.updated_at.to_string(),
            s.meta.deleted_at.map(|t| t.to_string()),
            s.device_id.to_string(),
            s.opened_by.to_string(),
            s.closed_by.map(|id| id.to_string()),
            s.opened_at.to_string(),
            s.closed_at.map(|t| t.to_string()),
            s.opening_float,
            s.closing_float,
            s.expected_cash,
            s.actual_cash,
            s.variance,
            s.notes,
        ],
    )?;
    outbox::record(conn, "shifts", EventType::Upsert, s.meta.id, s, now)
}

pub fn open(
    conn: &Connection,
    device_id: Uuid,
    opened_by: Uuid,
    opening_float: i64,
    now: Timestamp,
) -> rusqlite::Result<Shift> {
    let shift = Shift {
        meta: Meta::new(now),
        device_id,
        opened_by,
        closed_by: None,
        opened_at: now,
        closed_at: None,
        opening_float,
        closing_float: None,
        expected_cash: None,
        actual_cash: None,
        variance: None,
        notes: None,
    };
    persist(conn, &shift, now)?;
    Ok(shift)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ShiftTotals {
    pub transaction_count: i64,
    pub sales_total: i64,
    pub cash_total: i64,
    pub card_total: i64,
    pub wallet_total: i64,
}

pub fn totals(conn: &Connection, shift_id: Uuid) -> rusqlite::Result<ShiftTotals> {
    let id = shift_id.to_string();
    let (transaction_count, sales_total) = conn.query_row(
        "SELECT count(*), COALESCE(SUM(total), 0) FROM transactions WHERE shift_id = ?1",
        [&id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let by_method = |method: &str| -> rusqlite::Result<i64> {
        conn.query_row(
            "SELECT COALESCE(SUM(p.amount), 0) FROM transaction_payments p
             JOIN transactions t ON t.id = p.transaction_id
             WHERE t.shift_id = ?1 AND p.method = ?2",
            params![id, method],
            |r| r.get(0),
        )
    };
    Ok(ShiftTotals {
        transaction_count,
        sales_total,
        cash_total: by_method("cash")?,
        card_total: by_method("card")?,
        wallet_total: by_method("wallet")?,
    })
}

pub fn close(
    conn: &Connection,
    shift: &Shift,
    closed_by: Uuid,
    actual_cash: i64,
    closing_float: i64,
    notes: Option<String>,
    now: Timestamp,
) -> rusqlite::Result<Shift> {
    let totals = totals(conn, shift.meta.id)?;
    let expected = shift.opening_float + totals.cash_total;
    let mut closed = shift.clone();
    closed.meta.updated_at = now;
    closed.closed_by = Some(closed_by);
    closed.closed_at = Some(now);
    closed.expected_cash = Some(expected);
    closed.actual_cash = Some(actual_cash);
    closed.variance = Some(actual_cash - expected);
    closed.closing_float = Some(closing_float);
    closed.notes = notes;
    persist(conn, &closed, now)?;
    Ok(closed)
}

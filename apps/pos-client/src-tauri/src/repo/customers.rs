//! Customers and their loyalty ledger.
//!
//! `customers` syncs last-write-wins; `loyalty_points` on it is a cached sum
//! of `loyalty_ledger`, which is append-only and syncs as additive deltas
//! (like stock), so points earned on two tills offline both count.

use pos_core::time::Timestamp;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::audit::Actor;
use super::catalog::like_pattern;
use super::outbox::{self, EventType};
use super::{enum_str, opt_ts_at, rows, ts_at, Meta};

/// Mirrors `CustomerSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Customer {
    #[serde(flatten)]
    pub meta: Meta,
    pub display_name: String,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub loyalty_points: i64,
    pub notes: Option<String>,
}

const COLUMNS: &str =
    "id, created_at, updated_at, deleted_at, display_name, phone, email, loyalty_points, notes";

/// Mirrors `LOYALTY_REASONS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LedgerReason {
    Earn,
    Redeem,
    Adjust,
    Expire,
    RefundReversal,
}

/// Mirrors `LoyaltyLedgerEntrySchema`.
#[derive(Debug, Clone, Serialize)]
pub struct LedgerEntry {
    #[serde(flatten)]
    pub meta: Meta,
    pub customer_id: Uuid,
    pub transaction_id: Option<Uuid>,
    pub points_delta: i64,
    pub reason: LedgerReason,
    pub device_id: Uuid,
    pub user_id: Uuid,
}

/// Phone numbers are stored as typed digits (a leading + kept), so a
/// lookup matches however the number was spaced.
pub fn normalize_phone(phone: &str) -> Option<String> {
    let trimmed = phone.trim();
    let digits: String = trimmed.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    Some(if trimmed.starts_with('+') {
        format!("+{digits}")
    } else {
        digits
    })
}

pub fn get(conn: &Connection, id: Uuid) -> rusqlite::Result<Option<Customer>> {
    Ok(rows::select(
        conn,
        &format!("SELECT {COLUMNS} FROM customers WHERE id = ?1 AND deleted_at IS NULL"),
        [id.to_string()],
    )?
    .into_iter()
    .next())
}

pub fn by_phone(
    conn: &Connection,
    phone: &str,
    except: Option<Uuid>,
) -> rusqlite::Result<Option<Customer>> {
    Ok(rows::select::<Customer, _>(
        conn,
        &format!(
            "SELECT {COLUMNS} FROM customers WHERE phone = ?1 AND deleted_at IS NULL ORDER BY created_at"
        ),
        [phone],
    )?
    .into_iter()
    .find(|c| Some(c.meta.id) != except))
}

/// By name, phone digits, email or membership card number; an empty query
/// lists the latest.
pub fn search(conn: &Connection, query: &str, limit: i64) -> rusqlite::Result<Vec<Customer>> {
    let limit = limit.clamp(1, 100);
    let query = query.trim();
    if query.is_empty() {
        return rows::select(
            conn,
            &format!(
                "SELECT {COLUMNS} FROM customers WHERE deleted_at IS NULL
                 ORDER BY updated_at DESC LIMIT {limit}"
            ),
            [],
        );
    }
    let digits: String = query.chars().filter(char::is_ascii_digit).collect();
    let phone = if digits.len() >= 3 {
        like_pattern(&digits)
    } else {
        // Never matches: too few digits to look up by phone.
        "\u{0}".to_owned()
    };
    rows::select(
        conn,
        &format!(
            "SELECT {COLUMNS} FROM customers
             WHERE deleted_at IS NULL
               AND (display_name LIKE ?1 ESCAPE '\\' OR email LIKE ?1 ESCAPE '\\'
                    OR phone LIKE ?2 ESCAPE '\\'
                    OR id IN (SELECT customer_id FROM memberships
                              WHERE card_number = ?3 AND deleted_at IS NULL))
             ORDER BY (phone LIKE ?2 ESCAPE '\\') DESC, display_name COLLATE NOCASE
             LIMIT {limit}"
        ),
        params![like_pattern(query), phone, query],
    )
}

/// Writes the row (the cached balance is kept as stored, never taken from
/// the caller).
pub fn save(conn: &Connection, customer: &Customer, now: Timestamp) -> rusqlite::Result<()> {
    let balance: Option<i64> = conn
        .query_row(
            "SELECT loyalty_points FROM customers WHERE id = ?1",
            [customer.meta.id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    let row = Customer {
        loyalty_points: balance.unwrap_or(0),
        ..customer.clone()
    };
    rows::upsert(conn, "customers", &row, now)
}

/// Appends a ledger entry and moves the cached balance.
pub fn add_points(
    conn: &Connection,
    customer_id: Uuid,
    delta: i64,
    reason: LedgerReason,
    transaction_id: Option<Uuid>,
    actor: &Actor,
    now: Timestamp,
) -> rusqlite::Result<()> {
    if delta == 0 {
        return Ok(());
    }
    let entry = LedgerEntry {
        meta: Meta::new(now),
        customer_id,
        transaction_id,
        points_delta: delta,
        reason,
        device_id: actor.device_id,
        user_id: actor.user_id,
    };
    conn.execute(
        "INSERT INTO loyalty_ledger (id, created_at, updated_at, customer_id, transaction_id, points_delta,
                                     reason, device_id, user_id)
         VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            entry.meta.id.to_string(),
            now.to_string(),
            customer_id.to_string(),
            transaction_id.map(|id| id.to_string()),
            delta,
            enum_str(&reason),
            actor.device_id.to_string(),
            actor.user_id.to_string(),
        ],
    )?;
    conn.execute(
        "UPDATE customers SET loyalty_points = loyalty_points + ?1 WHERE id = ?2",
        params![delta, customer_id.to_string()],
    )?;
    outbox::record(
        conn,
        "loyalty_ledger",
        EventType::Append,
        entry.meta.id,
        &entry,
        now,
    )
}

/// The balance right after `at` (receipts show the balance of their day,
/// also when reprinted later).
pub fn balance_at(conn: &Connection, customer_id: Uuid, at: Timestamp) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COALESCE(SUM(points_delta), 0) FROM loyalty_ledger
         WHERE customer_id = ?1 AND created_at <= ?2",
        params![customer_id.to_string(), at.to_string()],
        |r| r.get(0),
    )
}

/// Mirrors `LoyaltyLedgerViewSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct LedgerView {
    pub id: Uuid,
    pub occurred_at: Timestamp,
    pub points_delta: i64,
    pub reason: LedgerReason,
    pub transaction_id: Option<Uuid>,
    pub receipt_number: Option<String>,
    pub user_name: String,
}

/// Mirrors `CustomerDetailSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct CustomerDetail {
    pub customer: Customer,
    pub visits: i64,
    pub spent: i64,
    pub last_visit_at: Option<Timestamp>,
    pub ledger: Vec<LedgerView>,
}

pub fn detail(conn: &Connection, customer: Customer) -> rusqlite::Result<CustomerDetail> {
    let id = customer.meta.id.to_string();
    let (visits, spent, last_visit_at) = conn.query_row(
        "SELECT COALESCE(SUM(t.kind = 'sale' AND NOT EXISTS (
                    SELECT 1 FROM transactions v
                    WHERE v.original_transaction_id = t.id AND v.kind = 'void')), 0),
                COALESCE(SUM(t.total), 0),
                MAX(CASE WHEN t.kind = 'sale' THEN t.occurred_at END)
         FROM transactions t WHERE t.customer_id = ?1",
        [&id],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, opt_ts_at(r, 2)?)),
    )?;
    let ledger = conn
        .prepare(
            "SELECT l.id, l.created_at, l.points_delta, l.reason, l.transaction_id, t.receipt_number,
                    COALESCE(u.display_name, '')
             FROM loyalty_ledger l
             LEFT JOIN transactions t ON t.id = l.transaction_id
             LEFT JOIN users u ON u.id = l.user_id
             WHERE l.customer_id = ?1
             ORDER BY l.created_at DESC, l.id DESC LIMIT 100",
        )?
        .query_map([&id], |r| {
            Ok(LedgerView {
                id: super::uuid_at(r, 0)?,
                occurred_at: ts_at(r, 1)?,
                points_delta: r.get(2)?,
                reason: super::enum_at(r, 3)?,
                transaction_id: super::opt_uuid_at(r, 4)?,
                receipt_number: r.get(5)?,
                user_name: r.get(6)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(CustomerDetail {
        customer,
        visits,
        spent,
        last_visit_at,
        ledger,
    })
}

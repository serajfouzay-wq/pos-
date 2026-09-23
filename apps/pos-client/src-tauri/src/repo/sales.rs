//! Sales: quote, create (one SQLite transaction), and receipt reconstruction.
//!
//! The payload carries product ids and quantities only; every price, name and
//! tax rate is read here from the catalogue and snapshotted onto the lines.

use std::collections::HashMap;

use pos_core::config::ClientConfig;
use pos_core::currency::CurrencyCode;
use pos_core::pricing::{self, Discount, DiscountScope, DiscountValue, PriceLine, Quote, TaxLine};
use pos_core::rbac::Role;
use pos_core::receipt::{Receipt, ReceiptLine, ReceiptPayment};
use pos_core::sales::{OrderType, PaymentMethod, TransactionKind};
use pos_core::tender::{self, Tender};
use pos_core::time::Timestamp;
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::audit::{self, Actor};
use super::catalog::{self, StockReason};
use super::outbox::{self, EventType};
use super::{
    device, enum_at, enum_str, opt_ts_at, print_jobs, shifts, ts_at, uuid_at, Meta, SqlResultExt,
};

pub const MAX_LINES: usize = 500;

#[derive(Debug, Clone, Deserialize)]
pub struct PayloadItem {
    pub product_id: Uuid,
    pub quantity_milli: i64,
    #[serde(default)]
    pub modifier_ids: Vec<Uuid>,
    pub course: Option<i64>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PayloadPayment {
    pub method: PaymentMethod,
    pub tendered_currency: CurrencyCode,
    pub tendered_amount: i64,
    pub reference: Option<String>,
}

/// Mirrors `TransactionPayloadSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct TransactionPayload {
    pub idempotency_key: Uuid,
    pub customer_id: Option<Uuid>,
    pub order_type: OrderType,
    pub table_label: Option<String>,
    pub items: Vec<PayloadItem>,
    #[serde(default)]
    pub discount_rule_ids: Vec<Uuid>,
    #[serde(default)]
    pub loyalty_points_to_redeem: i64,
    pub payments: Vec<PayloadPayment>,
    pub notes: Option<String>,
}

/// Mirrors `QuoteRequestSchema` — the cart without payments.
#[derive(Debug, Clone, Deserialize)]
pub struct QuoteRequest {
    pub items: Vec<PayloadItem>,
    #[serde(default)]
    pub discount_rule_ids: Vec<Uuid>,
}

/// Mirrors `QuoteSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct QuoteView {
    pub currency: CurrencyCode,
    pub lines: Vec<pricing::PricedLine>,
    pub subtotal: i64,
    pub discount_total: i64,
    pub tax_total: i64,
    pub total: i64,
    pub tax_lines: Vec<TaxLine>,
}

fn invalid(message: impl Into<String>) -> IpcError {
    IpcError::validation(message)
}

fn load_discounts(conn: &Connection, ids: &[Uuid], now: Timestamp) -> IpcResult<Vec<Discount>> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let row = conn
            .query_row(
                "SELECT kind, value, scope, target_id, min_subtotal, starts_at, ends_at, is_active
                 FROM discount_rules WHERE id = ?1 AND deleted_at IS NULL",
                [id.to_string()],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<i64>>(4)?,
                        opt_ts_at(r, 5)?,
                        opt_ts_at(r, 6)?,
                        r.get::<_, bool>(7)?,
                    ))
                },
            )
            .optional()
            .ipc()?
            .ok_or_else(|| invalid("That discount does not exist."))?;
        let (kind, value, scope, target, min_subtotal, starts, ends, active) = row;
        if !active || starts.is_some_and(|s| now < s) || ends.is_some_and(|e| now >= e) {
            return Err(invalid("That discount is not currently active."));
        }
        let target = target.and_then(|t| Uuid::parse_str(&t).ok());
        let scope = match (scope.as_str(), target) {
            ("order", _) => DiscountScope::Order,
            ("product", Some(t)) => DiscountScope::Product(t),
            ("category", Some(t)) => DiscountScope::Category(t),
            _ => return Err(invalid("That discount is misconfigured.")),
        };
        let value = match kind.as_str() {
            "percentage" => DiscountValue::Percentage(value),
            _ => DiscountValue::Fixed(value),
        };
        out.push(Discount {
            id: *id,
            value,
            scope,
            min_subtotal,
        });
    }
    Ok(out)
}

/// Prices a cart from the catalogue. Shared by `quote_transaction` and
/// `create_transaction`, so the preview and the sale can never disagree.
pub fn quote(
    conn: &Connection,
    items: &[PayloadItem],
    discount_rule_ids: &[Uuid],
    config: &ClientConfig,
    now: Timestamp,
) -> IpcResult<Quote> {
    if items.is_empty() {
        return Err(invalid("Add at least one item."));
    }
    if items.len() > MAX_LINES {
        return Err(invalid("Too many lines on one sale."));
    }
    let mut lines = Vec::with_capacity(items.len());
    for item in items {
        if !item.modifier_ids.is_empty() {
            return Err(invalid("Modifiers are not available yet."));
        }
        let product = catalog::get(conn, item.product_id)
            .ipc()?
            .filter(|p| p.is_active)
            .ok_or_else(|| invalid("An item in the cart is no longer available."))?;
        if item.quantity_milli <= 0 || item.quantity_milli > 1_000_000_000 {
            return Err(invalid(format!("Invalid quantity for {}.", product.name)));
        }
        if !product.sold_by_weight && item.quantity_milli % 1000 != 0 {
            return Err(invalid(format!("{} is sold in whole units.", product.name)));
        }
        lines.push(PriceLine {
            product_id: product.meta.id,
            category_id: product.category_id,
            name: product.name,
            sku: product.sku,
            unit_price: product.price,
            quantity_milli: item.quantity_milli,
            tax_rate_bps: product.tax_rate_bps,
        });
    }
    let discounts = load_discounts(conn, discount_rule_ids, now)?;
    pricing::price(&lines, &discounts, config.tax.prices_include_tax)
        .map_err(|e| invalid(e.to_string()))
}

pub fn quote_view(quote: Quote, currency: CurrencyCode) -> QuoteView {
    QuoteView {
        currency,
        subtotal: quote.subtotal,
        discount_total: quote.discount_total,
        tax_total: quote.tax_total,
        total: quote.total,
        tax_lines: quote.tax_lines,
        lines: quote.lines,
    }
}

#[derive(Debug, Clone, Serialize)]
struct TransactionRow {
    #[serde(flatten)]
    meta: Meta,
    kind: TransactionKind,
    original_transaction_id: Option<Uuid>,
    receipt_number: String,
    device_id: Uuid,
    shift_id: Uuid,
    cashier_id: Uuid,
    approved_by: Option<Uuid>,
    customer_id: Option<Uuid>,
    order_type: OrderType,
    table_label: Option<String>,
    currency: CurrencyCode,
    subtotal: i64,
    discount_total: i64,
    tax_total: i64,
    total: i64,
    loyalty_points_earned: i64,
    loyalty_points_redeemed: i64,
    notes: Option<String>,
    idempotency_key: Uuid,
    occurred_at: Timestamp,
}

#[derive(Debug, Clone, Serialize)]
struct ItemRow {
    #[serde(flatten)]
    meta: Meta,
    transaction_id: Uuid,
    line_number: i64,
    product_id: Uuid,
    product_name: String,
    sku: Option<String>,
    unit_price: i64,
    quantity_milli: i64,
    modifiers: Vec<serde_json::Value>,
    discount_amount: i64,
    tax_rate_bps: i64,
    tax_amount: i64,
    line_total: i64,
    course: Option<i64>,
    note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct PaymentRow {
    #[serde(flatten)]
    meta: Meta,
    transaction_id: Uuid,
    method: PaymentMethod,
    amount: i64,
    tendered_currency: CurrencyCode,
    tendered_amount: i64,
    rate_numerator: Option<i64>,
    rate_denominator: Option<i64>,
    change_given: i64,
    reference: Option<String>,
}

pub struct SaleActor {
    pub user_id: Uuid,
    pub role: Role,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreatedSale {
    pub transaction_id: Uuid,
    /// `false` when the idempotency key matched an existing sale (a retry).
    pub is_new: bool,
    pub includes_cash: bool,
}

fn find_by_key(conn: &Connection, key: Uuid) -> IpcResult<Option<Uuid>> {
    conn.query_row(
        "SELECT id FROM transactions WHERE idempotency_key = ?1",
        [key.to_string()],
        |r| uuid_at(r, 0),
    )
    .optional()
    .ipc()
}

/// Records a sale atomically: transaction, lines, payments, stock movements,
/// outbox events, audit entry and the receipt print job.
pub fn create(
    conn: &mut Connection,
    actor: &SaleActor,
    payload: &TransactionPayload,
    config: &ClientConfig,
    now: Timestamp,
) -> IpcResult<CreatedSale> {
    if let Some(existing) = find_by_key(conn, payload.idempotency_key)? {
        let includes_cash = payments_include_cash(conn, existing)?;
        return Ok(CreatedSale {
            transaction_id: existing,
            is_new: false,
            includes_cash,
        });
    }
    if payload.customer_id.is_some() || payload.loyalty_points_to_redeem != 0 {
        return Err(invalid("Customers and loyalty are not available yet."));
    }
    let base = config.currency.base;
    if payload.payments.iter().any(|p| p.tendered_currency != base) {
        return Err(invalid("Foreign-currency payments are not available yet."));
    }

    let tx = conn.transaction().ipc()?;
    let device_id = device::id(&tx).ipc()?;
    let shift = shifts::current_open(&tx, device_id)
        .ipc()?
        .ok_or_else(|| IpcError::new(IpcErrorCode::Conflict, "Open a shift before selling."))?;

    let quote = quote(&tx, &payload.items, &payload.discount_rule_ids, config, now)?;
    let tenders: Vec<Tender> = payload
        .payments
        .iter()
        .map(|p| Tender {
            method: p.method,
            tendered: p.tendered_amount,
            reference: p.reference.clone().filter(|r| !r.trim().is_empty()),
        })
        .collect();
    let settlement = tender::settle(quote.total, &tenders).map_err(|e| invalid(e.to_string()))?;

    let sequence: i64 = tx
        .query_row(
            "SELECT count(*) FROM transactions WHERE device_id = ?1",
            [device_id.to_string()],
            |r| r.get(0),
        )
        .ipc()?;
    let row = TransactionRow {
        meta: Meta::new(now),
        kind: TransactionKind::Sale,
        original_transaction_id: None,
        receipt_number: format!("{}-{:06}", device::receipt_prefix(device_id), sequence + 1),
        device_id,
        shift_id: shift.meta.id,
        cashier_id: actor.user_id,
        approved_by: None,
        customer_id: None,
        order_type: payload.order_type,
        table_label: payload.table_label.clone().filter(|t| !t.trim().is_empty()),
        currency: base,
        subtotal: quote.subtotal,
        discount_total: quote.discount_total,
        tax_total: quote.tax_total,
        total: quote.total,
        loyalty_points_earned: 0,
        loyalty_points_redeemed: 0,
        notes: payload.notes.clone().filter(|n| !n.trim().is_empty()),
        idempotency_key: payload.idempotency_key,
        occurred_at: now,
    };
    tx.execute(
        "INSERT INTO transactions (id, created_at, updated_at, kind, original_transaction_id, receipt_number,
            device_id, shift_id, cashier_id, approved_by, customer_id, order_type, table_label, currency,
            subtotal, discount_total, tax_total, total, loyalty_points_earned, loyalty_points_redeemed,
            notes, idempotency_key, occurred_at)
         VALUES (?1, ?2, ?2, ?3, NULL, ?4, ?5, ?6, ?7, NULL, NULL, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 0, 0, ?15, ?16, ?2)",
        params![
            row.meta.id.to_string(),
            now.to_string(),
            enum_str(&row.kind),
            row.receipt_number,
            device_id.to_string(),
            shift.meta.id.to_string(),
            actor.user_id.to_string(),
            enum_str(&row.order_type),
            row.table_label,
            base.as_str(),
            row.subtotal,
            row.discount_total,
            row.tax_total,
            row.total,
            row.notes,
            row.idempotency_key.to_string(),
        ],
    )
    .ipc()?;
    outbox::record(
        &tx,
        "transactions",
        EventType::Append,
        row.meta.id,
        &row,
        now,
    )
    .ipc()?;

    let audit_actor = Actor {
        user_id: actor.user_id,
        role: actor.role,
        device_id,
    };
    for (i, (line, item)) in quote.lines.iter().zip(&payload.items).enumerate() {
        let item_row = ItemRow {
            meta: Meta::new(now),
            transaction_id: row.meta.id,
            line_number: i64::try_from(i + 1).unwrap_or(i64::MAX),
            product_id: line.product_id,
            product_name: line.name.clone(),
            sku: line.sku.clone(),
            unit_price: line.unit_price,
            quantity_milli: line.quantity_milli,
            modifiers: Vec::new(),
            discount_amount: line.discount_amount,
            tax_rate_bps: line.tax_rate_bps,
            tax_amount: line.tax_amount,
            line_total: line.line_total,
            course: item.course,
            note: item.note.clone().filter(|n| !n.trim().is_empty()),
        };
        tx.execute(
            "INSERT INTO transaction_items (id, created_at, updated_at, transaction_id, line_number, product_id,
                product_name, sku, unit_price, quantity_milli, modifiers, discount_amount, tax_rate_bps,
                tax_amount, line_total, course, note)
             VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, '[]', ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                item_row.meta.id.to_string(),
                now.to_string(),
                row.meta.id.to_string(),
                item_row.line_number,
                item_row.product_id.to_string(),
                item_row.product_name,
                item_row.sku,
                item_row.unit_price,
                item_row.quantity_milli,
                item_row.discount_amount,
                item_row.tax_rate_bps,
                item_row.tax_amount,
                item_row.line_total,
                item_row.course,
                item_row.note,
            ],
        )
        .ipc()?;
        outbox::record(
            &tx,
            "transaction_items",
            EventType::Append,
            item_row.meta.id,
            &item_row,
            now,
        )
        .ipc()?;

        let tracks_stock = catalog::get(&tx, line.product_id)
            .ipc()?
            .is_some_and(|p| p.track_stock);
        if tracks_stock {
            catalog::move_stock(
                &tx,
                line.product_id,
                -line.quantity_milli,
                StockReason::Sale,
                Some(row.meta.id),
                &audit_actor,
                now,
            )
            .ipc()?;
        }
    }

    for applied in &settlement.tenders {
        let payment = PaymentRow {
            meta: Meta::new(now),
            transaction_id: row.meta.id,
            method: applied.method,
            amount: applied.amount,
            tendered_currency: base,
            tendered_amount: applied.tendered,
            rate_numerator: None,
            rate_denominator: None,
            change_given: applied.change_given,
            reference: applied.reference.clone(),
        };
        tx.execute(
            "INSERT INTO transaction_payments (id, created_at, updated_at, transaction_id, method, amount,
                tendered_currency, tendered_amount, change_given, reference)
             VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                payment.meta.id.to_string(),
                now.to_string(),
                row.meta.id.to_string(),
                enum_str(&payment.method),
                payment.amount,
                base.as_str(),
                payment.tendered_amount,
                payment.change_given,
                payment.reference,
            ],
        )
        .ipc()?;
        outbox::record(
            &tx,
            "transaction_payments",
            EventType::Append,
            payment.meta.id,
            &payment,
            now,
        )
        .ipc()?;
    }

    audit::record(
        &tx,
        &audit_actor,
        "sale.create",
        "transactions",
        Some(row.meta.id),
        None,
        Some(serde_json::json!({ "receipt_number": row.receipt_number, "total": row.total })),
        now,
    )
    .ipc()?;
    print_jobs::enqueue(&tx, row.meta.id, false, now).ipc()?;
    tx.commit().ipc()?;

    Ok(CreatedSale {
        transaction_id: row.meta.id,
        is_new: true,
        includes_cash: settlement.includes_cash(),
    })
}

fn payments_include_cash(conn: &Connection, transaction_id: Uuid) -> IpcResult<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM transaction_payments WHERE transaction_id = ?1 AND method = 'cash')",
        [transaction_id.to_string()],
        |r| r.get(0),
    )
    .ipc()
}

/// Rebuilds the receipt from the immutable stored rows.
pub fn load_receipt(conn: &Connection, transaction_id: Uuid, printed: bool) -> IpcResult<Receipt> {
    let id = transaction_id.to_string();
    let (kind, receipt_number, issued_at, cashier_name, currency, subtotal, discount_total, total) = conn
        .query_row(
            "SELECT t.kind, t.receipt_number, t.occurred_at, COALESCE(u.display_name, ''), t.currency,
                    t.subtotal, t.discount_total, t.total
             FROM transactions t LEFT JOIN users u ON u.id = t.cashier_id WHERE t.id = ?1",
            [&id],
            |r| {
                Ok((
                    enum_at::<TransactionKind>(r, 0)?,
                    r.get::<_, String>(1)?,
                    ts_at(r, 2)?,
                    r.get::<_, String>(3)?,
                    enum_at::<CurrencyCode>(r, 4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, i64>(7)?,
                ))
            },
        )
        .ipc()?;

    let items: Vec<(ReceiptLine, i64, i64)> = conn
        .prepare(
            "SELECT product_name, quantity_milli, unit_price, discount_amount, line_total, tax_rate_bps, tax_amount
             FROM transaction_items WHERE transaction_id = ?1 ORDER BY line_number",
        )
        .ipc()?
        .query_map([&id], |r| {
            Ok((
                ReceiptLine {
                    name: r.get(0)?,
                    quantity_milli: r.get(1)?,
                    unit_price: r.get(2)?,
                    modifiers: Vec::new(),
                    discount_amount: r.get(3)?,
                    line_total: r.get(4)?,
                },
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })
        .ipc()?
        .collect::<Result<_, _>>()
        .ipc()?;

    let mut tax_by_rate: HashMap<i64, TaxLine> = HashMap::new();
    for (line, rate, tax) in &items {
        if *rate == 0 {
            continue;
        }
        let entry = tax_by_rate.entry(*rate).or_insert(TaxLine {
            rate_bps: *rate,
            taxable_amount: 0,
            tax_amount: 0,
        });
        entry.taxable_amount += line.line_total - tax;
        entry.tax_amount += tax;
    }
    let mut tax_lines: Vec<TaxLine> = tax_by_rate.into_values().collect();
    tax_lines.sort_by_key(|t| t.rate_bps);

    let payments: Vec<(ReceiptPayment, i64)> = conn
        .prepare(
            "SELECT method, amount, tendered_currency, tendered_amount, change_given
             FROM transaction_payments WHERE transaction_id = ?1 ORDER BY created_at, id",
        )
        .ipc()?
        .query_map([&id], |r| {
            Ok((
                ReceiptPayment {
                    method: enum_at(r, 0)?,
                    amount: r.get(1)?,
                    tendered_currency: enum_at(r, 2)?,
                    tendered_amount: r.get(3)?,
                },
                r.get::<_, i64>(4)?,
            ))
        })
        .ipc()?
        .collect::<Result<_, _>>()
        .ipc()?;

    Ok(Receipt {
        transaction_id,
        kind,
        receipt_number,
        issued_at,
        cashier_name,
        customer_name: None,
        currency,
        lines: items.into_iter().map(|(line, _, _)| line).collect(),
        subtotal,
        discount_total,
        tax_lines,
        total,
        change_due: payments.iter().map(|(_, change)| change).sum(),
        payments: payments.into_iter().map(|(p, _)| p).collect(),
        loyalty: None,
        printed,
    })
}

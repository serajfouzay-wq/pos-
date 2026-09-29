//! Refunds and voids: new, append-only transactions that reverse all or part
//! of a sale. The sale itself is never touched.
//!
//! Signs: the reversing transaction stores negative header totals and
//! negative payments (money going back); its item rows are positive
//! quantities and amounts, like any item row. Reports multiply items by the
//! transaction kind.
//!
//! Partial quantities take a pro-rata share of the line's stored amounts,
//! computed cumulatively (share of everything reversed so far minus what was
//! already taken back), so partial refunds always add up to the line exactly.

use std::collections::{HashMap, HashSet};

use pos_core::currency::CurrencyCode;
use pos_core::loyalty;
use pos_core::rbac::Role;
use pos_core::receipt::ModifierLine;
use pos_core::sales::{OrderType, PaymentMethod, TransactionKind};
use pos_core::time::Timestamp;
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::repo::audit::{self, Actor};
use crate::repo::catalog::{self, StockReason};
use crate::repo::customers::{self, LedgerReason};
use crate::repo::sales::{self, CreatedSale, ItemRow, PaymentRow, TransactionRow};
use crate::repo::{device, enum_at, opt_uuid_at, print_jobs, shifts, uuid_at, Meta, SqlResultExt};

fn invalid(message: impl Into<String>) -> IpcError {
    IpcError::validation(message)
}

fn conflict(message: impl Into<String>) -> IpcError {
    IpcError::new(IpcErrorCode::Conflict, message)
}

/// Mirrors `RefundMethodSchema`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefundMethod {
    Cash,
    Card,
    Wallet,
}

impl RefundMethod {
    fn payment_method(self) -> PaymentMethod {
        match self {
            Self::Cash => PaymentMethod::Cash,
            Self::Card => PaymentMethod::Card,
            Self::Wallet => PaymentMethod::Wallet,
        }
    }
}

/// Mirrors `RefundInputSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct RefundInput {
    pub transaction_id: Uuid,
    pub idempotency_key: Uuid,
    pub lines: Vec<RefundLine>,
    pub method: RefundMethod,
    pub restock: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RefundLine {
    pub item_id: Uuid,
    pub quantity_milli: i64,
}

/// Mirrors `VoidInputSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct VoidInput {
    pub transaction_id: Uuid,
    pub idempotency_key: Uuid,
    pub reason: String,
}

pub struct ReverseActor {
    pub user_id: Uuid,
    pub role: Role,
}

/// The header of the transaction being reversed.
#[derive(Debug, Clone)]
pub struct SaleHeader {
    pub id: Uuid,
    pub kind: TransactionKind,
    pub receipt_number: String,
    pub device_id: Uuid,
    pub shift_id: Uuid,
    pub order_type: OrderType,
    pub table_label: Option<String>,
    pub currency: CurrencyCode,
    pub subtotal: i64,
    pub discount_total: i64,
    pub total: i64,
    pub customer_id: Option<Uuid>,
    pub points_earned: i64,
    pub points_redeemed: i64,
    /// Gross (before discounts) already taken back by earlier reversals.
    pub reversed_subtotal: i64,
}

pub fn header(conn: &Connection, id: Uuid) -> IpcResult<SaleHeader> {
    conn.query_row(
        "SELECT id, kind, receipt_number, device_id, shift_id, order_type, table_label, currency,
                subtotal, discount_total, total, customer_id, loyalty_points_earned, loyalty_points_redeemed,
                (SELECT COALESCE(-SUM(r.subtotal), 0) FROM transactions r WHERE r.original_transaction_id = t.id)
         FROM transactions t WHERE id = ?1",
        [id.to_string()],
        |r| {
            Ok(SaleHeader {
                id: uuid_at(r, 0)?,
                kind: enum_at(r, 1)?,
                receipt_number: r.get(2)?,
                device_id: uuid_at(r, 3)?,
                shift_id: uuid_at(r, 4)?,
                order_type: enum_at(r, 5)?,
                table_label: r.get(6)?,
                currency: enum_at(r, 7)?,
                subtotal: r.get(8)?,
                discount_total: r.get(9)?,
                total: r.get(10)?,
                customer_id: opt_uuid_at(r, 11)?,
                points_earned: r.get(12)?,
                points_redeemed: r.get(13)?,
                reversed_subtotal: r.get(14)?,
            })
        },
    )
    .optional()
    .ipc()?
    .ok_or_else(|| IpcError::new(IpcErrorCode::NotFound, "That sale does not exist."))
}

/// A line of the sale with what has already been reversed.
#[derive(Debug, Clone)]
pub struct OriginalLine {
    pub item_id: Uuid,
    pub line_number: i64,
    pub product_id: Uuid,
    pub name: String,
    pub sku: Option<String>,
    pub unit_price: i64,
    pub quantity_milli: i64,
    pub modifiers: Vec<ModifierLine>,
    pub discount_amount: i64,
    pub tax_rate_bps: i64,
    pub tax_amount: i64,
    pub line_total: i64,
    pub course: Option<i64>,
    pub note: Option<String>,
    pub reversed_milli: i64,
    /// Weighed goods can go back by weight; everything else by the unit.
    pub whole_units: bool,
}

impl OriginalLine {
    pub fn refundable_milli(&self) -> i64 {
        (self.quantity_milli - self.reversed_milli).max(0)
    }
}

pub fn original_lines(conn: &Connection, sale_id: Uuid) -> IpcResult<Vec<OriginalLine>> {
    let id = sale_id.to_string();
    let reversed: HashMap<i64, i64> = conn
        .prepare(
            "SELECT i.line_number, SUM(i.quantity_milli) FROM transaction_items i
             JOIN transactions t ON t.id = i.transaction_id
             WHERE t.original_transaction_id = ?1 GROUP BY i.line_number",
        )
        .ipc()?
        .query_map([&id], |r| Ok((r.get(0)?, r.get(1)?)))
        .ipc()?
        .collect::<Result<_, _>>()
        .ipc()?;
    let mut lines: Vec<OriginalLine> = conn
        .prepare(
            "SELECT id, line_number, product_id, product_name, sku, unit_price, quantity_milli, modifiers,
                    discount_amount, tax_rate_bps, tax_amount, line_total, course, note
             FROM transaction_items WHERE transaction_id = ?1 ORDER BY line_number",
        )
        .ipc()?
        .query_map([&id], |r| {
            Ok(OriginalLine {
                item_id: uuid_at(r, 0)?,
                line_number: r.get(1)?,
                product_id: uuid_at(r, 2)?,
                name: r.get(3)?,
                sku: r.get(4)?,
                unit_price: r.get(5)?,
                quantity_milli: r.get(6)?,
                modifiers: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default(),
                discount_amount: r.get(8)?,
                tax_rate_bps: r.get(9)?,
                tax_amount: r.get(10)?,
                line_total: r.get(11)?,
                course: r.get(12)?,
                note: r.get(13)?,
                reversed_milli: 0,
                whole_units: true,
            })
        })
        .ipc()?
        .collect::<Result<_, _>>()
        .ipc()?;
    for line in &mut lines {
        line.reversed_milli = reversed.get(&line.line_number).copied().unwrap_or(0);
        line.whole_units = match catalog::get(conn, line.product_id).ipc()? {
            Some(product) => !product.sold_by_weight,
            None => line.quantity_milli % 1000 == 0,
        };
    }
    Ok(lines)
}

/// Net amount per tender still held for the sale (paid minus reversed).
fn held_by_method(conn: &Connection, sale_id: Uuid) -> IpcResult<HashMap<PaymentMethod, i64>> {
    let rows: Vec<(PaymentMethod, i64)> = conn
        .prepare(
            "SELECT p.method, SUM(p.amount) FROM transaction_payments p
             JOIN transactions t ON t.id = p.transaction_id
             WHERE t.id = ?1 OR t.original_transaction_id = ?1 GROUP BY p.method",
        )
        .ipc()?
        .query_map([sale_id.to_string()], |r| Ok((enum_at(r, 0)?, r.get(1)?)))
        .ipc()?
        .collect::<Result<_, _>>()
        .ipc()?;
    Ok(rows.into_iter().collect())
}

/// `total × part ÷ whole`, rounded half up, in integers.
fn share(total: i64, part: i64, whole: i64) -> i64 {
    if whole == 0 {
        return 0;
    }
    let num = i128::from(total) * i128::from(part);
    let den = i128::from(whole);
    i64::try_from((num * 2 + den) / (den * 2)).unwrap_or(i64::MAX)
}

/// This slice of `amount` when `qty` more of `whole` goes back after `before`.
fn slice(amount: i64, before: i64, qty: i64, whole: i64) -> i64 {
    share(amount, before + qty, whole) - share(amount, before, whole)
}

struct Pick<'a> {
    line: &'a OriginalLine,
    quantity_milli: i64,
}

#[allow(clippy::too_many_arguments)]
fn reverse(
    tx: &Connection,
    actor: &ReverseActor,
    sale: &SaleHeader,
    picks: &[Pick<'_>],
    tenders: &[(PaymentMethod, i64)],
    kind: TransactionKind,
    restock: bool,
    reason: &str,
    idempotency_key: Uuid,
    now: Timestamp,
) -> IpcResult<CreatedSale> {
    let device_id = device::id(tx).ipc()?;
    let shift = shifts::current_open(tx, device_id).ipc()?.ok_or_else(|| {
        conflict("Open a shift first: the money goes back out of this till's drawer.")
    })?;
    // Tax-exclusive prices add tax on top of the line total's taxable part.
    let exclusive = sale.total != sale.subtotal - sale.discount_total;

    let id = Meta::new(now);
    let mut items = Vec::new();
    let (mut subtotal, mut discount, mut tax, mut total) = (0, 0, 0, 0);
    for pick in picks {
        let l = pick.line;
        let (before, q, whole) = (l.reversed_milli, pick.quantity_milli, l.quantity_milli);
        let line_total = slice(l.line_total, before, q, whole);
        let tax_amount = slice(l.tax_amount, before, q, whole);
        let discount_amount = slice(l.discount_amount, before, q, whole);
        let gross = line_total + discount_amount - if exclusive { tax_amount } else { 0 };
        subtotal += gross;
        discount += discount_amount;
        tax += tax_amount;
        total += line_total;
        items.push(ItemRow {
            meta: Meta::new(now),
            transaction_id: id.id,
            // Same number as the sale's line: reversals are matched by it.
            line_number: l.line_number,
            product_id: l.product_id,
            product_name: l.name.clone(),
            sku: l.sku.clone(),
            unit_price: l.unit_price,
            quantity_milli: q,
            modifiers: l.modifiers.clone(),
            discount_amount,
            tax_rate_bps: l.tax_rate_bps,
            tax_amount,
            line_total,
            course: l.course,
            note: l.note.clone(),
        });
    }
    let tender_sum: i64 = tenders.iter().map(|(_, amount)| amount).sum();
    if tender_sum != total {
        return Err(IpcError::internal("refund tenders do not add up"));
    }
    // The same share of the sale's points comes back: earned points are
    // taken away, redeemed points are returned (cumulative, like money). The
    // share is of the goods (gross), so a bill paid entirely with points —
    // total zero — still gives its points back.
    let (points_back, points_returned) = match sale.customer_id {
        Some(_) => (
            loyalty::reversal(
                sale.points_earned,
                sale.subtotal,
                sale.reversed_subtotal,
                subtotal,
            ),
            loyalty::reversal(
                sale.points_redeemed,
                sale.subtotal,
                sale.reversed_subtotal,
                subtotal,
            ),
        ),
        None => (0, 0),
    };

    let row = TransactionRow {
        meta: id.clone(),
        kind,
        original_transaction_id: Some(sale.id),
        receipt_number: sales::next_receipt_number(tx, device_id)?,
        device_id,
        shift_id: shift.meta.id,
        cashier_id: actor.user_id,
        approved_by: Some(actor.user_id),
        customer_id: sale.customer_id,
        order_type: sale.order_type,
        table_label: sale.table_label.clone(),
        currency: sale.currency,
        subtotal: -subtotal,
        discount_total: -discount,
        tax_total: -tax,
        total: -total,
        // On a reversal: the points taken back and the points returned.
        loyalty_points_earned: points_back,
        loyalty_points_redeemed: points_returned,
        notes: Some(reason.trim().to_owned()),
        idempotency_key,
        occurred_at: now,
    };
    sales::insert_transaction(tx, &row)?;

    let audit_actor = Actor {
        user_id: actor.user_id,
        role: actor.role,
        device_id,
    };
    for item in &items {
        sales::insert_item(tx, item)?;
        let tracked = catalog::get(tx, item.product_id)
            .ipc()?
            .is_some_and(|p| p.track_stock);
        if restock && tracked {
            catalog::move_stock(
                tx,
                item.product_id,
                item.quantity_milli,
                StockReason::Refund,
                Some(row.meta.id),
                &audit_actor,
                now,
            )
            .ipc()?;
        }
    }
    if let Some(customer) = sale.customer_id {
        for delta in [points_returned, -points_back] {
            customers::add_points(
                tx,
                customer,
                delta,
                LedgerReason::RefundReversal,
                Some(row.meta.id),
                &audit_actor,
                now,
            )
            .ipc()?;
        }
    }
    for (method, amount) in tenders.iter().filter(|(_, amount)| *amount != 0) {
        sales::insert_payment(
            tx,
            &PaymentRow {
                meta: Meta::new(now),
                transaction_id: row.meta.id,
                method: *method,
                amount: -amount,
                tendered_currency: sale.currency,
                tendered_amount: -amount,
                rate_numerator: None,
                rate_denominator: None,
                change_given: 0,
                reference: None,
            },
        )?;
    }

    audit::record(
        tx,
        &audit_actor,
        match kind {
            TransactionKind::Void => "sale.void",
            _ => "sale.refund",
        },
        "transactions",
        Some(row.meta.id),
        Some(serde_json::json!({ "receipt_number": sale.receipt_number, "total": sale.total })),
        Some(serde_json::json!({
            "receipt_number": row.receipt_number,
            "total": row.total,
            "reason": reason.trim(),
            "restock": restock,
            "lines": items.iter().map(|i| serde_json::json!({
                "name": i.product_name, "quantity_milli": i.quantity_milli, "line_total": i.line_total,
            })).collect::<Vec<_>>(),
        })),
        now,
    )
    .ipc()?;
    if crate::printing::PrintService::settings(tx)?.auto_print_receipt {
        print_jobs::enqueue(tx, row.meta.id, false, now).ipc()?;
    }

    Ok(CreatedSale {
        transaction_id: row.meta.id,
        is_new: true,
        includes_cash: tenders
            .iter()
            .any(|(m, a)| *m == PaymentMethod::Cash && *a != 0),
    })
}

fn replay(tx: &Connection, key: Uuid) -> IpcResult<Option<CreatedSale>> {
    Ok(match sales::find_by_key(tx, key)? {
        Some(existing) => Some(CreatedSale {
            transaction_id: existing,
            is_new: false,
            includes_cash: sales::payments_include_cash(tx, existing)?,
        }),
        None => None,
    })
}

fn check_reason(reason: &str) -> IpcResult<()> {
    let len = reason.trim().chars().count();
    if len == 0 || len > 200 {
        return Err(invalid("Give a reason (up to 200 characters)."));
    }
    Ok(())
}

/// Checks a refund against the stored sale and prices it. Nothing is written.
fn plan<'a>(
    conn: &Connection,
    input: &RefundInput,
    lines: &'a [OriginalLine],
) -> IpcResult<(Vec<Pick<'a>>, i64)> {
    if input.lines.is_empty() {
        return Err(invalid("Choose what to refund."));
    }
    let mut seen = HashSet::new();
    let mut picks = Vec::new();
    for wanted in &input.lines {
        if !seen.insert(wanted.item_id) {
            return Err(invalid("A line is listed twice."));
        }
        let line = lines
            .iter()
            .find(|l| l.item_id == wanted.item_id)
            .ok_or_else(|| invalid("That line is not on this sale."))?;
        if wanted.quantity_milli <= 0 || wanted.quantity_milli > line.refundable_milli() {
            return Err(invalid(format!(
                "{}: at most {} can still be refunded.",
                line.name,
                pos_hardware::receipt::format_quantity(line.refundable_milli())
            )));
        }
        if line.whole_units && wanted.quantity_milli % 1000 != 0 {
            return Err(invalid(format!("{} is refunded by the unit.", line.name)));
        }
        picks.push(Pick {
            line,
            quantity_milli: wanted.quantity_milli,
        });
    }
    // The amount is decided by the stored sale, never by the UI.
    let amount: i64 = picks
        .iter()
        .map(|p| {
            slice(
                p.line.line_total,
                p.line.reversed_milli,
                p.quantity_milli,
                p.line.quantity_milli,
            )
        })
        .sum();
    let method = input.method.payment_method();
    if method != PaymentMethod::Cash {
        let held = held_by_method(conn, input.transaction_id)?
            .get(&method)
            .copied()
            .unwrap_or(0);
        if amount > held {
            return Err(invalid(
                "More than was paid that way: refund the rest in cash.",
            ));
        }
    }
    Ok((picks, amount))
}

fn refundable_sale(conn: &Connection, id: Uuid) -> IpcResult<SaleHeader> {
    let sale = header(conn, id)?;
    if sale.kind != TransactionKind::Sale {
        return Err(invalid("Only a sale can be refunded."));
    }
    Ok(sale)
}

/// What [`refund`] would pay back.
pub fn quote(conn: &Connection, input: &RefundInput) -> IpcResult<i64> {
    let sale = refundable_sale(conn, input.transaction_id)?;
    let lines = original_lines(conn, sale.id)?;
    Ok(plan(conn, input, &lines)?.1)
}

pub fn refund(
    conn: &mut Connection,
    actor: &ReverseActor,
    input: &RefundInput,
    now: Timestamp,
) -> IpcResult<CreatedSale> {
    let tx = conn.transaction().ipc()?;
    if let Some(done) = replay(&tx, input.idempotency_key)? {
        return Ok(done);
    }
    check_reason(&input.reason)?;
    let sale = refundable_sale(&tx, input.transaction_id)?;
    let lines = original_lines(&tx, sale.id)?;
    let (picks, amount) = plan(&tx, input, &lines)?;
    let created = reverse(
        &tx,
        actor,
        &sale,
        &picks,
        &[(input.method.payment_method(), amount)],
        TransactionKind::Refund,
        input.restock,
        &input.reason,
        input.idempotency_key,
        now,
    )?;
    tx.commit().ipc()?;
    Ok(created)
}

/// Why this sale cannot be voided right now (`None` = it can).
pub fn void_blocker(conn: &Connection, sale: &SaleHeader) -> IpcResult<Option<String>> {
    if sale.kind != TransactionKind::Sale {
        return Ok(Some("Only a sale can be voided.".into()));
    }
    let device_id = device::id(conn).ipc()?;
    let open = shifts::current_open(conn, device_id).ipc()?;
    if sale.device_id != device_id || open.map_or(true, |s| s.meta.id != sale.shift_id) {
        return Ok(Some(
            "Only sales of this till's open shift can be voided; refund it instead.".into(),
        ));
    }
    let reversed: i64 = conn
        .query_row(
            "SELECT count(*) FROM transactions WHERE original_transaction_id = ?1",
            [sale.id.to_string()],
            |r| r.get(0),
        )
        .ipc()?;
    if reversed > 0 {
        return Ok(Some(
            "Part of this sale was already refunded; refund the rest instead.".into(),
        ));
    }
    Ok(None)
}

pub fn void(
    conn: &mut Connection,
    actor: &ReverseActor,
    input: &VoidInput,
    now: Timestamp,
) -> IpcResult<CreatedSale> {
    let tx = conn.transaction().ipc()?;
    if let Some(done) = replay(&tx, input.idempotency_key)? {
        return Ok(done);
    }
    check_reason(&input.reason)?;
    let sale = header(&tx, input.transaction_id)?;
    if let Some(blocker) = void_blocker(&tx, &sale)? {
        return Err(conflict(blocker));
    }
    let lines = original_lines(&tx, sale.id)?;
    let picks: Vec<Pick<'_>> = lines
        .iter()
        .map(|line| Pick {
            line,
            quantity_milli: line.quantity_milli,
        })
        .collect();
    // Every tender goes back the way it came.
    let mut tenders: Vec<(PaymentMethod, i64)> = held_by_method(&tx, sale.id)?
        .into_iter()
        .filter(|(_, amount)| *amount != 0)
        .collect();
    tenders.sort_by_key(|(method, _)| method_order(*method));
    let created = reverse(
        &tx,
        actor,
        &sale,
        &picks,
        &tenders,
        TransactionKind::Void,
        true,
        &input.reason,
        input.idempotency_key,
        now,
    )?;
    // Food already sent to the kitchen for this sale is called off.
    if created.is_new {
        let server = crate::repo::users::get(&tx, actor.user_id)
            .ipc()?
            .map(|u| u.display_name)
            .unwrap_or_default();
        crate::kitchen::void_sale(&tx, sale.id, &server, now)?;
    }
    tx.commit().ipc()?;
    Ok(created)
}

fn method_order(method: PaymentMethod) -> u8 {
    match method {
        PaymentMethod::Cash => 0,
        PaymentMethod::Card => 1,
        PaymentMethod::Wallet => 2,
        PaymentMethod::Loyalty => 3,
        PaymentMethod::Voucher => 4,
    }
}

/// Mirrors `TransactionLineSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct LineView {
    pub item_id: Uuid,
    pub line_number: i64,
    pub product_id: Uuid,
    pub name: String,
    pub quantity_milli: i64,
    pub unit_price: i64,
    pub line_total: i64,
    pub reversed_quantity_milli: i64,
    pub refundable_quantity_milli: i64,
    pub whole_units: bool,
}

impl From<&OriginalLine> for LineView {
    fn from(l: &OriginalLine) -> Self {
        Self {
            item_id: l.item_id,
            line_number: l.line_number,
            product_id: l.product_id,
            name: l.name.clone(),
            quantity_milli: l.quantity_milli,
            unit_price: l.unit_price,
            line_total: l.line_total,
            reversed_quantity_milli: l.reversed_milli,
            refundable_quantity_milli: l.refundable_milli(),
            whole_units: l.whole_units,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_shares_add_up_to_the_line() {
        // 3 items for 1.000: 333 + 334 + 333 (cumulative rounding).
        let parts: Vec<i64> = (0..3).map(|i| slice(1_000, i * 1000, 1000, 3000)).collect();
        assert_eq!(parts.iter().sum::<i64>(), 1_000);
        assert_eq!(parts, vec![333, 334, 333]);
        assert_eq!(slice(1_000, 0, 3000, 3000), 1_000);
        assert_eq!(share(5, 1, 2), 3, "half rounds up");
    }
}

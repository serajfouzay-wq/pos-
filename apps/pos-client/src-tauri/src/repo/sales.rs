//! Sales: quote, create (one SQLite transaction), and receipt reconstruction.
//!
//! The payload carries product ids and quantities only; every price, name and
//! tax rate is read here from the catalogue and snapshotted onto the lines.

use std::collections::HashMap;

use pos_core::config::ClientConfig;
use pos_core::currency::CurrencyCode;
use pos_core::money;
use pos_core::pricing::{self, Discount, DiscountScope, DiscountValue, PriceLine, Quote, TaxLine};
use pos_core::rbac::Role;
use pos_core::receipt::{LoyaltySummary, ModifierLine, Receipt, ReceiptLine, ReceiptPayment};
use pos_core::sales::{OrderType, PaymentMethod, TransactionKind};
use pos_core::tender::{self, Tender};
use pos_core::time::Timestamp;
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::audit::{self, Actor};
use super::catalog::{self, StockReason};
use super::customers;
use super::menu;
pub use super::orders::ComboRef;
use super::outbox::{self, EventType};
use super::{
    device, enum_at, enum_str, opt_ts_at, print_jobs, shifts, ts_at, uuid_at, Meta, SqlResultExt,
};
use crate::loyalty::{self, LoyaltyQuote, LoyaltyRequest};

pub const MAX_LINES: usize = 500;

#[derive(Debug, Clone, Deserialize)]
pub struct PayloadItem {
    pub product_id: Uuid,
    pub quantity_milli: i64,
    #[serde(default)]
    pub modifier_ids: Vec<Uuid>,
    pub course: Option<i64>,
    pub note: Option<String>,
    /// Set on every line added from one combo (same `instance`).
    #[serde(default)]
    pub combo: Option<ComboRef>,
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
    #[serde(default)]
    pub loyalty: Option<LoyaltyRequest>,
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
    pub loyalty: Option<LoyaltyQuote>,
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

/// A priced cart plus what each line was sold with (for the snapshots).
#[derive(Debug)]
pub struct PricedCart {
    pub quote: Quote,
    pub modifiers: Vec<Vec<ModifierLine>>,
    /// What was priced, kept so a redemption can be priced on top.
    pub lines: Vec<PriceLine>,
    pub discounts: Vec<Discount>,
}

/// Modifiers of one line, validated against the product's groups: options
/// must be live and belong to a group the product asks; each group's count
/// must be within its min/max (so required choices are made).
fn resolve_modifiers(
    product: &catalog::Product,
    item: &PayloadItem,
    catalog_menu: &MenuIndex,
) -> IpcResult<Vec<ModifierLine>> {
    let asked = catalog_menu
        .product_groups
        .get(&product.meta.id)
        .cloned()
        .unwrap_or_default();
    let mut counts: HashMap<Uuid, i64> = HashMap::new();
    let mut lines = Vec::with_capacity(item.modifier_ids.len());
    for (i, id) in item.modifier_ids.iter().enumerate() {
        if item.modifier_ids[..i].contains(id) {
            return Err(invalid(format!(
                "{}: an option was chosen twice.",
                product.name
            )));
        }
        let option = catalog_menu
            .modifiers
            .get(id)
            .filter(|m| asked.contains(&m.group_id))
            .ok_or_else(|| {
                invalid(format!(
                    "{}: an option is no longer available.",
                    product.name
                ))
            })?;
        *counts.entry(option.group_id).or_default() += 1;
        lines.push(ModifierLine {
            modifier_id: Some(option.meta.id),
            name: option.name.clone(),
            price_delta: option.price_delta,
        });
    }
    for group_id in &asked {
        let Some(group) = catalog_menu.groups.get(group_id) else {
            continue;
        };
        let chosen = counts.get(group_id).copied().unwrap_or(0);
        if chosen < group.min_select {
            return Err(invalid(format!("{}: choose {}.", product.name, group.name)));
        }
        if chosen > group.max_select {
            return Err(invalid(format!(
                "{}: at most {} for {}.",
                product.name, group.max_select, group.name
            )));
        }
    }
    Ok(lines)
}

/// Live modifier groups/options and product links, loaded once per quote.
struct MenuIndex {
    groups: HashMap<Uuid, menu::ModifierGroup>,
    modifiers: HashMap<Uuid, menu::Modifier>,
    product_groups: HashMap<Uuid, Vec<Uuid>>,
}

impl MenuIndex {
    fn load(conn: &Connection) -> IpcResult<Self> {
        let groups: HashMap<Uuid, menu::ModifierGroup> = menu::modifier_groups(conn)
            .ipc()?
            .into_iter()
            .filter(|g| g.is_active)
            .map(|g| (g.meta.id, g))
            .collect();
        let modifiers = menu::modifiers(conn)
            .ipc()?
            .into_iter()
            .filter(|m| m.is_active && groups.contains_key(&m.group_id))
            .map(|m| (m.meta.id, m))
            .collect();
        let mut product_groups: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
        for link in menu::product_groups(conn).ipc()? {
            if groups.contains_key(&link.group_id) {
                product_groups
                    .entry(link.product_id)
                    .or_default()
                    .push(link.group_id);
            }
        }
        Ok(Self {
            groups,
            modifiers,
            product_groups,
        })
    }
}

/// Each combo instance must be exactly the combo's components; its lines are
/// then priced as a group down to the combo price (plus any option
/// surcharges, which a combo never swallows).
fn combo_discounts(
    conn: &Connection,
    items: &[PayloadItem],
    lines: &[PriceLine],
    modifiers: &[Vec<ModifierLine>],
) -> IpcResult<Vec<Discount>> {
    let mut instances: Vec<(ComboRef, Vec<usize>)> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let Some(combo) = item.combo else { continue };
        match instances
            .iter_mut()
            .find(|(c, _)| c.instance == combo.instance)
        {
            Some((c, members)) if c.combo_id == combo.combo_id => members.push(i),
            Some(_) => return Err(invalid("A combo in the cart is inconsistent.")),
            None => instances.push((combo, vec![i])),
        }
    }
    if instances.is_empty() {
        return Ok(Vec::new());
    }
    let combos = menu::combos(conn).ipc()?;
    let components = menu::combo_items(conn).ipc()?;
    let mut out = Vec::with_capacity(instances.len());
    for (combo_ref, members) in instances {
        let combo = combos
            .iter()
            .find(|c| c.meta.id == combo_ref.combo_id && c.is_active)
            .ok_or_else(|| invalid("A combo in the cart is no longer available."))?;
        let mut expected: Vec<(Uuid, i64)> = components
            .iter()
            .filter(|c| c.combo_id == combo.meta.id)
            .map(|c| (c.product_id, c.quantity_milli))
            .collect();
        let mut actual: Vec<(Uuid, i64)> = members
            .iter()
            .map(|&i| (items[i].product_id, items[i].quantity_milli))
            .collect();
        expected.sort();
        actual.sort();
        if expected.is_empty() || expected != actual {
            return Err(invalid(format!(
                "{} must be sold with exactly its items.",
                combo.name
            )));
        }
        let mut surcharge = 0;
        for &i in &members {
            let delta: i64 = modifiers[i].iter().map(|m| m.price_delta).sum();
            surcharge += money::multiply_by_quantity(
                delta,
                lines[i].quantity_milli,
                money::RoundingMode::HalfUp,
            )
            .map_err(|e| invalid(e.to_string()))?;
        }
        out.push(Discount {
            id: combo.meta.id,
            value: DiscountValue::Target(combo.price.saturating_add(surcharge).max(0)),
            scope: DiscountScope::Group(combo_ref.instance),
            min_subtotal: None,
        });
    }
    Ok(out)
}

/// Prices a cart from the catalogue. Shared by `quote_transaction`,
/// `create_transaction` and open orders, so a preview and the sale can never
/// disagree.
pub fn price_cart(
    conn: &Connection,
    items: &[PayloadItem],
    discount_rule_ids: &[Uuid],
    config: &ClientConfig,
    now: Timestamp,
) -> IpcResult<PricedCart> {
    if items.is_empty() {
        return Err(invalid("Add at least one item."));
    }
    if items.len() > MAX_LINES {
        return Err(invalid("Too many lines on one sale."));
    }
    let catalog_menu = MenuIndex::load(conn)?;
    let mut lines = Vec::with_capacity(items.len());
    let mut modifiers = Vec::with_capacity(items.len());
    for item in items {
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
        if item.course.is_some_and(|c| !(1..=9).contains(&c)) {
            return Err(invalid("Courses are numbered 1 to 9."));
        }
        let chosen = resolve_modifiers(&product, item, &catalog_menu)?;
        let unit_price = product.price + chosen.iter().map(|m| m.price_delta).sum::<i64>();
        if unit_price < 0 {
            return Err(invalid(format!(
                "{}: the options make the price negative.",
                product.name
            )));
        }
        lines.push(PriceLine {
            product_id: product.meta.id,
            category_id: product.category_id,
            name: product.name,
            sku: product.sku,
            unit_price,
            quantity_milli: item.quantity_milli,
            tax_rate_bps: product.tax_rate_bps,
            group: item.combo.map(|c| c.instance),
        });
        modifiers.push(chosen);
    }
    let mut discounts = combo_discounts(conn, items, &lines, &modifiers)?;
    discounts.extend(load_discounts(conn, discount_rule_ids, now)?);
    let quote = pricing::price(&lines, &discounts, config.tax.prices_include_tax)
        .map_err(|e| invalid(e.to_string()))?;
    Ok(PricedCart {
        quote,
        modifiers,
        lines,
        discounts,
    })
}

pub fn quote(
    conn: &Connection,
    items: &[PayloadItem],
    discount_rule_ids: &[Uuid],
    config: &ClientConfig,
    now: Timestamp,
) -> IpcResult<Quote> {
    price_cart(conn, items, discount_rule_ids, config, now).map(|p| p.quote)
}

pub fn quote_view(
    quote: Quote,
    currency: CurrencyCode,
    loyalty: Option<LoyaltyQuote>,
) -> QuoteView {
    QuoteView {
        currency,
        loyalty,
        subtotal: quote.subtotal,
        discount_total: quote.discount_total,
        tax_total: quote.tax_total,
        total: quote.total,
        tax_lines: quote.tax_lines,
        lines: quote.lines,
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TransactionRow {
    #[serde(flatten)]
    pub(crate) meta: Meta,
    pub(crate) kind: TransactionKind,
    pub(crate) original_transaction_id: Option<Uuid>,
    pub(crate) receipt_number: String,
    pub(crate) device_id: Uuid,
    pub(crate) shift_id: Uuid,
    pub(crate) cashier_id: Uuid,
    pub(crate) approved_by: Option<Uuid>,
    pub(crate) customer_id: Option<Uuid>,
    pub(crate) order_type: OrderType,
    pub(crate) table_label: Option<String>,
    pub(crate) currency: CurrencyCode,
    pub(crate) subtotal: i64,
    pub(crate) discount_total: i64,
    pub(crate) tax_total: i64,
    pub(crate) total: i64,
    pub(crate) loyalty_points_earned: i64,
    pub(crate) loyalty_points_redeemed: i64,
    pub(crate) notes: Option<String>,
    pub(crate) idempotency_key: Uuid,
    pub(crate) occurred_at: Timestamp,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ItemRow {
    #[serde(flatten)]
    pub(crate) meta: Meta,
    pub(crate) transaction_id: Uuid,
    pub(crate) line_number: i64,
    pub(crate) product_id: Uuid,
    pub(crate) product_name: String,
    pub(crate) sku: Option<String>,
    pub(crate) unit_price: i64,
    pub(crate) quantity_milli: i64,
    pub(crate) modifiers: Vec<ModifierLine>,
    pub(crate) discount_amount: i64,
    pub(crate) tax_rate_bps: i64,
    pub(crate) tax_amount: i64,
    pub(crate) line_total: i64,
    pub(crate) course: Option<i64>,
    pub(crate) note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct PaymentRow {
    #[serde(flatten)]
    pub(crate) meta: Meta,
    pub(crate) transaction_id: Uuid,
    pub(crate) method: PaymentMethod,
    pub(crate) amount: i64,
    pub(crate) tendered_currency: CurrencyCode,
    pub(crate) tendered_amount: i64,
    pub(crate) rate_numerator: Option<i64>,
    pub(crate) rate_denominator: Option<i64>,
    pub(crate) change_given: i64,
    pub(crate) reference: Option<String>,
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

pub(crate) fn find_by_key(conn: &Connection, key: Uuid) -> IpcResult<Option<Uuid>> {
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
    let tx = conn.transaction().ipc()?;
    let created = create_in(&tx, actor, payload, config, now)?;
    // A pay-now sale goes straight to the kitchen.
    if created.is_new && crate::kitchen::records(config) {
        crate::kitchen::for_sale(&tx, config, created.transaction_id, now)?;
    }
    tx.commit().ipc()?;
    Ok(created)
}

/// [`create`] inside the caller's SQLite transaction (paying an open order
/// updates the order in the same commit).
pub fn create_in(
    tx: &Connection,
    actor: &SaleActor,
    payload: &TransactionPayload,
    config: &ClientConfig,
    now: Timestamp,
) -> IpcResult<CreatedSale> {
    if let Some(existing) = find_by_key(tx, payload.idempotency_key)? {
        let includes_cash = payments_include_cash(tx, existing)?;
        return Ok(CreatedSale {
            transaction_id: existing,
            is_new: false,
            includes_cash,
        });
    }
    if payload.customer_id.is_none() && payload.loyalty_points_to_redeem != 0 {
        return Err(invalid("Choose the customer whose points to use."));
    }
    let base = config.currency.base;
    if payload.payments.iter().any(|p| p.tendered_currency != base) {
        return Err(invalid("Foreign-currency payments are not available yet."));
    }

    let device_id = device::id(tx).ipc()?;
    let shift = shifts::current_open(tx, device_id)
        .ipc()?
        .ok_or_else(|| IpcError::new(IpcErrorCode::Conflict, "Open a shift before selling."))?;

    let priced = loyalty::price(
        tx,
        &payload.items,
        &payload.discount_rule_ids,
        payload.customer_id.map(|customer_id| LoyaltyRequest {
            customer_id,
            redeem_points: payload.loyalty_points_to_redeem,
        }),
        config,
        now,
    )?;
    let points = priced.loyalty;
    let PricedCart {
        quote, modifiers, ..
    } = priced.cart;
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

    let row = TransactionRow {
        meta: Meta::new(now),
        kind: TransactionKind::Sale,
        original_transaction_id: None,
        receipt_number: next_receipt_number(tx, device_id)?,
        device_id,
        shift_id: shift.meta.id,
        cashier_id: actor.user_id,
        approved_by: None,
        customer_id: points.as_ref().map(|p| p.customer_id),
        order_type: payload.order_type,
        table_label: payload.table_label.clone().filter(|t| !t.trim().is_empty()),
        currency: base,
        subtotal: quote.subtotal,
        discount_total: quote.discount_total,
        tax_total: quote.tax_total,
        total: quote.total,
        loyalty_points_earned: points.as_ref().map_or(0, |p| p.points_earned),
        loyalty_points_redeemed: points.as_ref().map_or(0, |p| p.redeem_points),
        notes: payload.notes.clone().filter(|n| !n.trim().is_empty()),
        idempotency_key: payload.idempotency_key,
        occurred_at: now,
    };
    insert_transaction(tx, &row)?;

    let audit_actor = Actor {
        user_id: actor.user_id,
        role: actor.role,
        device_id,
    };
    for (i, ((line, item), chosen)) in quote
        .lines
        .iter()
        .zip(&payload.items)
        .zip(&modifiers)
        .enumerate()
    {
        let item_row = ItemRow {
            meta: Meta::new(now),
            transaction_id: row.meta.id,
            line_number: i64::try_from(i + 1).unwrap_or(i64::MAX),
            product_id: line.product_id,
            product_name: line.name.clone(),
            sku: line.sku.clone(),
            unit_price: line.unit_price,
            quantity_milli: line.quantity_milli,
            modifiers: chosen.clone(),
            discount_amount: line.discount_amount,
            tax_rate_bps: line.tax_rate_bps,
            tax_amount: line.tax_amount,
            line_total: line.line_total,
            course: item.course,
            note: item.note.clone().filter(|n| !n.trim().is_empty()),
        };
        insert_item(tx, &item_row)?;

        let tracks_stock = catalog::get(tx, line.product_id)
            .ipc()?
            .is_some_and(|p| p.track_stock);
        if tracks_stock {
            catalog::move_stock(
                tx,
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

    // A bill paid entirely with points has nothing tendered.
    for applied in settlement
        .tenders
        .iter()
        .filter(|t| t.amount != 0 || t.tendered != 0)
    {
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
        insert_payment(tx, &payment)?;
    }

    if let Some(points) = &points {
        loyalty::record_sale_points(tx, points, row.meta.id, &audit_actor, now)?;
    }

    audit::record(
        tx,
        &audit_actor,
        "sale.create",
        "transactions",
        Some(row.meta.id),
        None,
        Some(serde_json::json!({ "receipt_number": row.receipt_number, "total": row.total })),
        now,
    )
    .ipc()?;
    if crate::printing::PrintService::settings(tx)?.auto_print_receipt {
        print_jobs::enqueue(tx, row.meta.id, false, now).ipc()?;
    }

    Ok(CreatedSale {
        transaction_id: row.meta.id,
        is_new: true,
        includes_cash: settlement.includes_cash(),
    })
}

/// `PREFIX-000124`: per till, one sequence for sales, refunds and voids.
pub(crate) fn next_receipt_number(tx: &Connection, device_id: Uuid) -> IpcResult<String> {
    let sequence: i64 = tx
        .query_row(
            "SELECT count(*) FROM transactions WHERE device_id = ?1",
            [device_id.to_string()],
            |r| r.get(0),
        )
        .ipc()?;
    Ok(format!(
        "{}-{:06}",
        device::receipt_prefix(device_id),
        sequence + 1
    ))
}

pub(crate) fn insert_transaction(tx: &Connection, row: &TransactionRow) -> IpcResult<()> {
    tx.execute(
        "INSERT INTO transactions (id, created_at, updated_at, kind, original_transaction_id, receipt_number,
            device_id, shift_id, cashier_id, approved_by, customer_id, order_type, table_label, currency,
            subtotal, discount_total, tax_total, total, loyalty_points_earned, loyalty_points_redeemed,
            notes, idempotency_key, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
            ?21, ?22, ?23)",
        params![
            row.meta.id.to_string(),
            row.meta.created_at.to_string(),
            row.meta.updated_at.to_string(),
            enum_str(&row.kind),
            row.original_transaction_id.map(|id| id.to_string()),
            row.receipt_number,
            row.device_id.to_string(),
            row.shift_id.to_string(),
            row.cashier_id.to_string(),
            row.approved_by.map(|id| id.to_string()),
            row.customer_id.map(|id| id.to_string()),
            enum_str(&row.order_type),
            row.table_label,
            row.currency.as_str(),
            row.subtotal,
            row.discount_total,
            row.tax_total,
            row.total,
            row.loyalty_points_earned,
            row.loyalty_points_redeemed,
            row.notes,
            row.idempotency_key.to_string(),
            row.occurred_at.to_string(),
        ],
    )
    .ipc()?;
    outbox::record(
        tx,
        "transactions",
        EventType::Append,
        row.meta.id,
        row,
        row.meta.created_at,
    )
    .ipc()
}

pub(crate) fn insert_item(tx: &Connection, row: &ItemRow) -> IpcResult<()> {
    tx.execute(
        "INSERT INTO transaction_items (id, created_at, updated_at, transaction_id, line_number, product_id,
            product_name, sku, unit_price, quantity_milli, modifiers, discount_amount, tax_rate_bps,
            tax_amount, line_total, course, note)
         VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?16, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            row.meta.id.to_string(),
            row.meta.created_at.to_string(),
            row.transaction_id.to_string(),
            row.line_number,
            row.product_id.to_string(),
            row.product_name,
            row.sku,
            row.unit_price,
            row.quantity_milli,
            row.discount_amount,
            row.tax_rate_bps,
            row.tax_amount,
            row.line_total,
            row.course,
            row.note,
            serde_json::to_string(&row.modifiers).unwrap_or_else(|_| "[]".into()),
        ],
    )
    .ipc()?;
    outbox::record(
        tx,
        "transaction_items",
        EventType::Append,
        row.meta.id,
        row,
        row.meta.created_at,
    )
    .ipc()
}

pub(crate) fn insert_payment(tx: &Connection, row: &PaymentRow) -> IpcResult<()> {
    tx.execute(
        "INSERT INTO transaction_payments (id, created_at, updated_at, transaction_id, method, amount,
            tendered_currency, tendered_amount, change_given, reference)
         VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            row.meta.id.to_string(),
            row.meta.created_at.to_string(),
            row.transaction_id.to_string(),
            enum_str(&row.method),
            row.amount,
            row.tendered_currency.as_str(),
            row.tendered_amount,
            row.change_given,
            row.reference,
        ],
    )
    .ipc()?;
    outbox::record(
        tx,
        "transaction_payments",
        EventType::Append,
        row.meta.id,
        row,
        row.meta.created_at,
    )
    .ipc()
}

pub(crate) fn payments_include_cash(conn: &Connection, transaction_id: Uuid) -> IpcResult<bool> {
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
    let (customer_id, customer_name, earned, redeemed): (Option<Uuid>, Option<String>, i64, i64) =
        conn.query_row(
            "SELECT t.customer_id, c.display_name, t.loyalty_points_earned, t.loyalty_points_redeemed
             FROM transactions t LEFT JOIN customers c ON c.id = t.customer_id WHERE t.id = ?1",
            [&id],
            |r| Ok((super::opt_uuid_at(r, 0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ipc()?;
    // Balance as of this transaction, so a reprint shows the same figure.
    let loyalty = match customer_id {
        Some(customer) if earned > 0 || redeemed > 0 => Some(LoyaltySummary {
            earned,
            redeemed,
            balance: customers::balance_at(conn, customer, issued_at).ipc()?,
        }),
        _ => None,
    };

    let items: Vec<(ReceiptLine, i64, i64)> = conn
        .prepare(
            "SELECT product_name, quantity_milli, unit_price, discount_amount, line_total, tax_rate_bps, tax_amount,
                    modifiers
             FROM transaction_items WHERE transaction_id = ?1 ORDER BY line_number",
        )
        .ipc()?
        .query_map([&id], |r| {
            Ok((
                ReceiptLine {
                    name: r.get(0)?,
                    quantity_milli: r.get(1)?,
                    unit_price: r.get(2)?,
                    modifiers: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default(),
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
        customer_name,
        currency,
        lines: items.into_iter().map(|(line, _, _)| line).collect(),
        subtotal,
        discount_total,
        tax_lines,
        total,
        change_due: payments.iter().map(|(_, change)| change).sum(),
        payments: payments.into_iter().map(|(p, _)| p).collect(),
        loyalty,
        printed,
    })
}

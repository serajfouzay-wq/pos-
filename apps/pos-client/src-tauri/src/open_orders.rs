//! Open orders (cafe tabs, restaurant tables): open, edit, fire courses to
//! the kitchen, split and pay, cancel. Every change is one SQLite
//! transaction with its outbox event; paying writes the sale and the order
//! update in the same commit.
//!
//! Edits carry the `updated_at` the till last saw. A mismatch means another
//! till changed the order since, and the edit is refused rather than
//! silently overwriting it (sync itself is last-write-wins).

use std::collections::{HashMap, HashSet};

use pos_core::config::ClientConfig;
use pos_core::rbac::{self, Permission, Role};
use pos_core::sales::OrderType;
use pos_core::time::Timestamp;
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use pos_hardware::kitchen::{KitchenLine, KitchenTicket};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::repo::audit::{self, Actor};
use crate::repo::orders::{self, ComboRef, OpenOrder, OpenOrderItem, OpenOrderStatus};
use crate::repo::sales::{
    self, CreatedSale, PayloadItem, PayloadPayment, SaleActor, TransactionPayload,
};
use crate::repo::{catalog, menu, Meta, SqlResultExt};

/// Who is acting, as the order rules need it.
#[derive(Debug, Clone)]
pub struct OrderActor {
    pub user_id: Uuid,
    pub display_name: String,
    pub role: Role,
    pub device_id: Uuid,
}

impl OrderActor {
    fn can_void(&self) -> bool {
        rbac::authorize(self.role, Permission::SaleVoid).is_ok()
    }

    fn audit(&self) -> Actor {
        Actor {
            user_id: self.user_id,
            role: self.role,
            device_id: self.device_id,
        }
    }
}

fn invalid(message: impl Into<String>) -> IpcError {
    IpcError::validation(message)
}

fn conflict(message: impl Into<String>) -> IpcError {
    IpcError::new(IpcErrorCode::Conflict, message)
}

fn load_open(conn: &Connection, order_id: Uuid) -> IpcResult<OpenOrder> {
    let order = orders::get(conn, order_id)
        .ipc()?
        .ok_or_else(|| IpcError::new(IpcErrorCode::NotFound, "That order no longer exists."))?;
    if order.status != OpenOrderStatus::Open {
        return Err(conflict("That order is already closed."));
    }
    Ok(order)
}

fn check_version(order: &OpenOrder, expected: Timestamp) -> IpcResult<()> {
    if order.meta.updated_at != expected {
        return Err(conflict(
            "This order was just changed on another till. It has been reloaded; try again.",
        ));
    }
    Ok(())
}

fn table_label(conn: &Connection, table_id: Uuid) -> IpcResult<String> {
    menu::dining_tables(conn)
        .ipc()?
        .into_iter()
        .find(|t| t.meta.id == table_id && t.is_active)
        .map(|t| t.label)
        .ok_or_else(|| invalid("That table does not exist."))
}

fn ensure_table_free(conn: &Connection, table_id: Uuid, except: Option<Uuid>) -> IpcResult<()> {
    if let Some(other) = orders::at_table(conn, table_id).ipc()? {
        if Some(other.meta.id) != except {
            return Err(conflict(format!(
                "Table {} already has an open order.",
                table_label(conn, table_id)?
            )));
        }
    }
    Ok(())
}

fn clean(text: Option<String>, max: usize) -> IpcResult<Option<String>> {
    let text = text.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
    if text.as_ref().is_some_and(|t| t.chars().count() > max) {
        return Err(invalid(format!("Keep it under {max} characters.")));
    }
    Ok(text)
}

/// Mirrors `OpenOrderInputSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct OpenInput {
    pub table_id: Option<Uuid>,
    pub label: Option<String>,
    pub guests: i64,
    pub order_type: OrderType,
}

pub fn open(
    conn: &Connection,
    actor: &OrderActor,
    input: OpenInput,
    now: Timestamp,
) -> IpcResult<OpenOrder> {
    let label = clean(input.label, 40)?;
    if let Some(table_id) = input.table_id {
        table_label(conn, table_id)?;
        ensure_table_free(conn, table_id, None)?;
    } else if label.is_none() {
        return Err(invalid("Give the tab a name."));
    }
    if !(0..=99).contains(&input.guests) {
        return Err(invalid("Guests must be between 0 and 99."));
    }
    let order = OpenOrder {
        meta: Meta::new(now),
        device_id: actor.device_id,
        order_type: if input.table_id.is_some() {
            OrderType::DineIn
        } else {
            input.order_type
        },
        table_id: input.table_id,
        label,
        guests: input.guests,
        status: OpenOrderStatus::Open,
        items: Vec::new(),
        transaction_ids: Vec::new(),
        opened_by: actor.user_id,
        opened_at: now,
        closed_at: None,
        notes: None,
    };
    orders::save(conn, &order, now).ipc()?;
    Ok(order)
}

/// A line as the till sends it; stamps (who/when/fired) are the core's.
#[derive(Debug, Clone, Deserialize)]
pub struct ItemInput {
    pub line_id: Uuid,
    pub product_id: Uuid,
    pub quantity_milli: i64,
    #[serde(default)]
    pub modifier_ids: Vec<Uuid>,
    pub course: Option<i64>,
    pub note: Option<String>,
    #[serde(default)]
    pub combo: Option<ComboRef>,
}

/// Mirrors `OpenOrderUpdateSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateInput {
    pub order_id: Uuid,
    pub expected_updated_at: Timestamp,
    pub items: Vec<ItemInput>,
    pub table_id: Option<Uuid>,
    pub label: Option<String>,
    pub guests: i64,
    pub notes: Option<String>,
}

fn same_content(item: &OpenOrderItem, input: &ItemInput) -> bool {
    item.product_id == input.product_id
        && item.quantity_milli == input.quantity_milli
        && item.modifier_ids == input.modifier_ids
        && item.course == input.course
        && item.note == input.note
        && item.combo == input.combo
}

pub fn payload_items(items: &[OpenOrderItem]) -> Vec<PayloadItem> {
    items
        .iter()
        .map(|i| PayloadItem {
            product_id: i.product_id,
            quantity_milli: i.quantity_milli,
            modifier_ids: i.modifier_ids.clone(),
            course: i.course,
            note: i.note.clone(),
            combo: i.combo,
        })
        .collect()
}

pub fn update(
    conn: &Connection,
    actor: &OrderActor,
    input: UpdateInput,
    config: &ClientConfig,
    now: Timestamp,
) -> IpcResult<OpenOrder> {
    let current = load_open(conn, input.order_id)?;
    check_version(&current, input.expected_updated_at)?;
    let mut seen = HashSet::new();
    if !input.items.iter().all(|i| seen.insert(i.line_id)) {
        return Err(invalid("Duplicate line in the order."));
    }

    let previous: HashMap<Uuid, &OpenOrderItem> =
        current.items.iter().map(|i| (i.line_id, i)).collect();
    let mut voided = Vec::new();
    for old in current.items.iter().filter(|i| i.fired_at.is_some()) {
        let kept = input.items.iter().find(|i| i.line_id == old.line_id);
        if !kept.is_some_and(|k| same_content(old, k)) {
            voided.push(old.clone());
        }
    }
    if !voided.is_empty() && !actor.can_void() {
        return Err(IpcError::new(
            IpcErrorCode::Forbidden,
            "Items already sent to the kitchen can only be changed by a manager.",
        ));
    }

    let items: Vec<OpenOrderItem> = input
        .items
        .iter()
        .map(|i| {
            let old = previous.get(&i.line_id);
            let unchanged = old.is_some_and(|o| same_content(o, i));
            Ok(OpenOrderItem {
                line_id: i.line_id,
                product_id: i.product_id,
                quantity_milli: i.quantity_milli,
                modifier_ids: i.modifier_ids.clone(),
                course: i.course,
                note: clean(i.note.clone(), 200)?,
                combo: i.combo,
                fired_at: if unchanged {
                    old.and_then(|o| o.fired_at)
                } else {
                    None
                },
                added_by: old.map_or(actor.user_id, |o| o.added_by),
                added_at: old.map_or(now, |o| o.added_at),
            })
        })
        .collect::<IpcResult<_>>()?;
    if !items.is_empty() {
        // Same validation as a sale: live products, required options, whole combos.
        sales::price_cart(conn, &payload_items(&items), &[], config, now)?;
    }
    if input.table_id != current.table_id {
        if let Some(table_id) = input.table_id {
            table_label(conn, table_id)?;
            ensure_table_free(conn, table_id, Some(current.meta.id))?;
        }
    }
    if !(0..=99).contains(&input.guests) {
        return Err(invalid("Guests must be between 0 and 99."));
    }
    let label = clean(input.label, 40)?;
    if input.table_id.is_none() && label.is_none() {
        return Err(invalid("Give the tab a name."));
    }

    let updated = OpenOrder {
        meta: Meta {
            updated_at: now,
            ..current.meta.clone()
        },
        order_type: if input.table_id.is_some() {
            OrderType::DineIn
        } else {
            current.order_type
        },
        table_id: input.table_id,
        label,
        guests: input.guests,
        items,
        notes: clean(input.notes, 500)?,
        ..current.clone()
    };
    orders::save(conn, &updated, now).ipc()?;
    if !voided.is_empty() {
        audit::record(
            conn,
            &actor.audit(),
            "sale.void",
            "open_orders",
            Some(updated.meta.id),
            serde_json::to_value(&voided).ok(),
            None,
            now,
        )
        .ipc()?;
    }
    Ok(updated)
}

/// Splits a whole-unit line of quantity N into N lines of one (to pay them
/// separately). Kitchen status is kept.
pub fn split_line(
    conn: &Connection,
    order_id: Uuid,
    line_id: Uuid,
    expected_updated_at: Timestamp,
    now: Timestamp,
) -> IpcResult<OpenOrder> {
    let current = load_open(conn, order_id)?;
    check_version(&current, expected_updated_at)?;
    let mut items = Vec::with_capacity(current.items.len() + 8);
    let mut found = false;
    for item in &current.items {
        if item.line_id != line_id {
            items.push(item.clone());
            continue;
        }
        found = true;
        if item.combo.is_some() || item.quantity_milli % 1000 != 0 || item.quantity_milli < 2000 {
            return Err(invalid("Only a line of several whole items can be split."));
        }
        for n in 0..item.quantity_milli / 1000 {
            items.push(OpenOrderItem {
                line_id: if n == 0 { item.line_id } else { Uuid::now_v7() },
                quantity_milli: 1000,
                ..item.clone()
            });
        }
    }
    if !found {
        return Err(invalid("That line is not on the order."));
    }
    let updated = OpenOrder {
        meta: Meta {
            updated_at: now,
            ..current.meta.clone()
        },
        items,
        ..current
    };
    orders::save(conn, &updated, now).ipc()?;
    Ok(updated)
}

/// Mirrors `FireOutcomeSchema` (the order part; printing is the caller's).
#[derive(Debug)]
pub struct Fired {
    pub order: OpenOrder,
    pub ticket: KitchenTicket,
}

/// Sends the unsent lines of `course` (or all unsent lines) to the kitchen.
pub fn fire(
    conn: &Connection,
    actor: &OrderActor,
    order_id: Uuid,
    course: Option<i64>,
    expected_updated_at: Timestamp,
    now: Timestamp,
) -> IpcResult<Fired> {
    let current = load_open(conn, order_id)?;
    check_version(&current, expected_updated_at)?;
    let options: HashMap<Uuid, String> = menu::modifiers(conn)
        .ipc()?
        .into_iter()
        .map(|m| (m.meta.id, m.name))
        .collect();
    let mut items = current.items.clone();
    let mut lines = Vec::new();
    for item in items
        .iter_mut()
        .filter(|i| i.fired_at.is_none() && course.map_or(true, |c| i.course == Some(c)))
    {
        item.fired_at = Some(now);
        let name = catalog::get(conn, item.product_id)
            .ipc()?
            .map_or_else(|| "?".to_owned(), |p| p.name);
        lines.push(KitchenLine {
            quantity_milli: item.quantity_milli,
            name,
            modifiers: item
                .modifier_ids
                .iter()
                .filter_map(|id| options.get(id).cloned())
                .collect(),
            note: item.note.clone(),
            course: item.course,
        });
    }
    if lines.is_empty() {
        return Err(invalid("Nothing new to send to the kitchen."));
    }
    lines.sort_by_key(|l| l.course.unwrap_or(0));
    let title = match (current.table_id, &current.label) {
        (Some(table_id), _) => format!("Table {}", table_label(conn, table_id)?),
        (None, Some(label)) => format!("Tab {label}"),
        (None, None) => "Order".to_owned(),
    };
    let order = OpenOrder {
        meta: Meta {
            updated_at: now,
            ..current.meta.clone()
        },
        items,
        ..current.clone()
    };
    orders::save(conn, &order, now).ipc()?;
    Ok(Fired {
        ticket: KitchenTicket {
            title,
            course,
            server: actor.display_name.clone(),
            guests: current.guests,
            at: now,
            lines,
        },
        order,
    })
}

pub fn cancel(
    conn: &Connection,
    actor: &OrderActor,
    order_id: Uuid,
    expected_updated_at: Timestamp,
    now: Timestamp,
) -> IpcResult<OpenOrder> {
    let current = load_open(conn, order_id)?;
    check_version(&current, expected_updated_at)?;
    if current.items.iter().any(|i| i.fired_at.is_some()) && !actor.can_void() {
        return Err(IpcError::new(
            IpcErrorCode::Forbidden,
            "This order has items sent to the kitchen; a manager must cancel it.",
        ));
    }
    let cancelled = OpenOrder {
        meta: Meta {
            updated_at: now,
            ..current.meta.clone()
        },
        status: OpenOrderStatus::Cancelled,
        closed_at: Some(now),
        ..current.clone()
    };
    orders::save(conn, &cancelled, now).ipc()?;
    audit::record(
        conn,
        &actor.audit(),
        if current.items.is_empty() {
            "order.cancel"
        } else {
            "sale.void"
        },
        "open_orders",
        Some(current.meta.id),
        serde_json::to_value(&current.items).ok(),
        None,
        now,
    )
    .ipc()?;
    Ok(cancelled)
}

/// Mirrors `PayOpenOrderSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct PayInput {
    pub order_id: Uuid,
    pub idempotency_key: Uuid,
    /// The lines this bill pays; `None` = everything left.
    pub line_ids: Option<Vec<Uuid>>,
    #[serde(default)]
    pub discount_rule_ids: Vec<Uuid>,
    pub payments: Vec<PayloadPayment>,
}

/// Pays part (split bill) or all of an order as one sale, in the caller's
/// transaction. A retried payment (same key) changes nothing.
pub fn pay(
    tx: &Connection,
    actor: &SaleActor,
    input: &PayInput,
    config: &ClientConfig,
    now: Timestamp,
) -> IpcResult<(OpenOrder, CreatedSale)> {
    let order = orders::get(tx, input.order_id)
        .ipc()?
        .ok_or_else(|| IpcError::new(IpcErrorCode::NotFound, "That order no longer exists."))?;
    if let Some(existing) = sales_by_key(tx, input.idempotency_key)? {
        return Ok((order, existing));
    }
    if order.status != OpenOrderStatus::Open {
        return Err(conflict("That order is already closed."));
    }
    let selected: Vec<OpenOrderItem> = match &input.line_ids {
        None => order.items.clone(),
        Some(ids) => {
            if ids
                .iter()
                .any(|id| !order.items.iter().any(|i| i.line_id == *id))
            {
                return Err(conflict("A line on this bill is no longer on the order."));
            }
            order
                .items
                .iter()
                .filter(|i| ids.contains(&i.line_id))
                .cloned()
                .collect()
        }
    };
    if selected.is_empty() {
        return Err(invalid("Choose what this bill pays for."));
    }
    let instances: HashSet<Uuid> = selected
        .iter()
        .filter_map(|i| i.combo.map(|c| c.instance))
        .collect();
    if order
        .items
        .iter()
        .any(|i| i.combo.is_some_and(|c| instances.contains(&c.instance)) && !selected.contains(i))
    {
        return Err(invalid("A combo is paid for as a whole."));
    }
    let table_label = match order.table_id {
        Some(table_id) => Some(table_label(tx, table_id).unwrap_or_default()),
        None => order.label.clone(),
    };
    let payload = TransactionPayload {
        idempotency_key: input.idempotency_key,
        customer_id: None,
        order_type: order.order_type,
        table_label: table_label.map(|t| t.chars().take(32).collect()),
        items: payload_items(&selected),
        discount_rule_ids: input.discount_rule_ids.clone(),
        loyalty_points_to_redeem: 0,
        payments: input.payments.clone(),
        notes: order.notes.clone(),
    };
    let created = sales::create_in(tx, actor, &payload, config, now)?;
    let remaining: Vec<OpenOrderItem> = order
        .items
        .iter()
        .filter(|i| !selected.contains(i))
        .cloned()
        .collect();
    let settled = remaining.is_empty();
    let mut transaction_ids = order.transaction_ids.clone();
    transaction_ids.push(created.transaction_id);
    let updated = OpenOrder {
        meta: Meta {
            updated_at: now,
            ..order.meta.clone()
        },
        status: if settled {
            OpenOrderStatus::Settled
        } else {
            OpenOrderStatus::Open
        },
        closed_at: settled.then_some(now),
        items: remaining,
        transaction_ids,
        ..order
    };
    orders::save(tx, &updated, now).ipc()?;
    Ok((updated, created))
}

fn sales_by_key(conn: &Connection, key: Uuid) -> IpcResult<Option<CreatedSale>> {
    let found: Option<(String, bool)> = conn
        .query_row(
            "SELECT t.id, EXISTS (SELECT 1 FROM transaction_payments p WHERE p.transaction_id = t.id AND p.method = 'cash')
             FROM transactions t WHERE t.idempotency_key = ?1",
            [key.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map(Some)
        .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })
        .ipc()?;
    Ok(found.and_then(|(id, cash)| {
        Uuid::parse_str(&id).ok().map(|transaction_id| CreatedSale {
            transaction_id,
            is_new: false,
            includes_cash: cash,
        })
    }))
}

/// Mirrors `OpenOrderViewSchema`: an open order plus what the floor plan
/// and tab list show.
#[derive(Debug, Clone, Serialize)]
pub struct OpenOrderView {
    #[serde(flatten)]
    pub order: OpenOrder,
    pub table_label: Option<String>,
    /// Priced like the sale would be; `None` when empty or no longer priceable.
    pub total: Option<i64>,
    pub unfired: i64,
}

pub fn views(
    conn: &Connection,
    config: &ClientConfig,
    now: Timestamp,
) -> IpcResult<Vec<OpenOrderView>> {
    let tables: HashMap<Uuid, String> = menu::dining_tables(conn)
        .ipc()?
        .into_iter()
        .map(|t| (t.meta.id, t.label))
        .collect();
    orders::open(conn)
        .ipc()?
        .into_iter()
        .map(|order| {
            let total = if order.items.is_empty() {
                None
            } else {
                sales::quote(conn, &payload_items(&order.items), &[], config, now)
                    .ok()
                    .map(|q| q.total)
            };
            Ok(OpenOrderView {
                table_label: order.table_id.and_then(|t| tables.get(&t).cloned()),
                total,
                unfired: i64::try_from(order.items.iter().filter(|i| i.fired_at.is_none()).count())
                    .unwrap_or(0),
                order,
            })
        })
        .collect()
}

pub fn view(
    conn: &Connection,
    order: OpenOrder,
    config: &ClientConfig,
    now: Timestamp,
) -> IpcResult<OpenOrderView> {
    let table_label = match order.table_id {
        Some(t) => table_label(conn, t).ok(),
        None => None,
    };
    let total = if order.items.is_empty() {
        None
    } else {
        sales::quote(conn, &payload_items(&order.items), &[], config, now)
            .ok()
            .map(|q| q.total)
    };
    Ok(OpenOrderView {
        unfired: i64::try_from(order.items.iter().filter(|i| i.fired_at.is_none()).count())
            .unwrap_or(0),
        table_label,
        total,
        order,
    })
}

#[cfg(test)]
mod tests;

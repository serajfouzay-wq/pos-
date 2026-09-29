//! Kitchen tickets: what the kitchen display shows and the kitchen printer
//! prints. Written when a course is sent, when items already sent are
//! changed or removed (a `void` ticket), when an order is paid with lines
//! never sent, and for pay-now sales. [`send`] queues each one for this
//! till's kitchen printer (any cafe or restaurant with one) and, in builds
//! with the kitchen display (`features.kitchen_display`), writes the
//! display's row.
//!
//! Tickets sync last-write-wins: the till creates one, the kitchen (this
//! machine or another) strikes items and bumps it.

use chrono::Duration;
use pos_core::config::{BusinessType, ClientConfig};
use pos_core::sales::OrderType;
use pos_core::time::Timestamp;
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use pos_hardware::kitchen::KitchenLine;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::printing::PrintService;
use crate::repo::{rows, uuid_at, Meta, SqlResultExt};

/// Tickets older than this drop off the board even if never bumped.
const BOARD_HOURS: i64 = 12;

/// The build has a kitchen display.
pub fn enabled(config: &ClientConfig) -> bool {
    config.features.kitchen_display && records(config)
}

/// The business has a kitchen: food sent to it goes to the kitchen printer.
pub fn records(config: &ClientConfig) -> bool {
    config.business_type != BusinessType::Retail
}

/// Mirrors `KitchenTicketKindSchema`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketKind {
    Order,
    Void,
}

/// Mirrors `KitchenTicketStatusSchema`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketStatus {
    Open,
    Ready,
}

/// Mirrors `KitchenTicketItemSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketItem {
    pub line_id: Uuid,
    pub quantity_milli: i64,
    pub name: String,
    pub modifiers: Vec<String>,
    pub note: Option<String>,
    pub course: Option<i64>,
    pub done_at: Option<Timestamp>,
}

impl TicketItem {
    pub fn new(line_id: Uuid, line: &KitchenLine) -> Self {
        Self {
            line_id,
            quantity_milli: line.quantity_milli,
            name: line.name.chars().take(120).collect(),
            modifiers: line
                .modifiers
                .iter()
                .take(20)
                .map(|m| m.chars().take(80).collect())
                .collect(),
            note: line
                .note
                .clone()
                .filter(|n| !n.trim().is_empty())
                .map(|n| n.chars().take(200).collect()),
            course: line.course,
            done_at: None,
        }
    }
}

/// Mirrors `KitchenTicketSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KitchenTicket {
    #[serde(flatten)]
    pub meta: Meta,
    pub device_id: Uuid,
    pub ticket_number: i64,
    pub kind: TicketKind,
    pub order_id: Option<Uuid>,
    pub transaction_id: Option<Uuid>,
    pub title: String,
    pub order_type: OrderType,
    pub course: Option<i64>,
    pub server_name: String,
    pub guests: i64,
    #[serde(with = "rows::json_text")]
    pub items: Vec<TicketItem>,
    pub status: TicketStatus,
    pub fired_at: Timestamp,
    pub ready_at: Option<Timestamp>,
}

const COLUMNS: &str = "id, created_at, updated_at, deleted_at, device_id, ticket_number, kind, order_id,
    transaction_id, title, order_type, course, server_name, guests, items, status, fired_at, ready_at";

/// A ticket to write.
pub struct Draft {
    pub kind: TicketKind,
    pub order_id: Option<Uuid>,
    pub transaction_id: Option<Uuid>,
    pub title: String,
    pub order_type: OrderType,
    pub course: Option<i64>,
    pub server_name: String,
    pub guests: i64,
    pub items: Vec<TicketItem>,
}

pub fn create(
    conn: &Connection,
    device_id: Uuid,
    draft: Draft,
    now: Timestamp,
) -> IpcResult<KitchenTicket> {
    if draft.items.is_empty() {
        return Err(IpcError::validation("A kitchen ticket needs items."));
    }
    let number: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(ticket_number), 0) + 1 FROM kitchen_tickets WHERE device_id = ?1",
            [device_id.to_string()],
            |r| r.get(0),
        )
        .ipc()?;
    let ticket = KitchenTicket {
        meta: Meta::new(now),
        device_id,
        ticket_number: number,
        kind: draft.kind,
        order_id: draft.order_id,
        transaction_id: draft.transaction_id,
        title: draft.title.chars().take(80).collect(),
        order_type: draft.order_type,
        course: draft.course,
        server_name: draft.server_name.chars().take(80).collect(),
        guests: draft.guests.clamp(0, 99),
        items: draft.items.into_iter().take(200).collect(),
        status: TicketStatus::Open,
        fired_at: now,
        ready_at: None,
    };
    rows::upsert(conn, "kitchen_tickets", &ticket, now).ipc()?;
    Ok(ticket)
}

/// Sends food to the kitchen: queues the ticket for this till's kitchen
/// printer (if it has one) and writes the display's row (builds with the
/// display). Returns the row.
pub fn send(
    conn: &Connection,
    config: &ClientConfig,
    device_id: Uuid,
    draft: Draft,
    now: Timestamp,
) -> IpcResult<Option<KitchenTicket>> {
    if !records(config) || draft.items.is_empty() {
        return Ok(None);
    }
    PrintService::queue_kitchen(conn, &draft.printed(now), now)?;
    if !enabled(config) {
        return Ok(None);
    }
    create(conn, device_id, draft, now).map(Some)
}

impl Draft {
    /// The ticket as the kitchen printer prints it.
    pub fn printed(&self, at: Timestamp) -> pos_hardware::kitchen::KitchenTicket {
        pos_hardware::kitchen::KitchenTicket {
            title: self.title.clone(),
            course: self.course,
            server: self.server_name.clone(),
            guests: self.guests,
            at,
            zone: pos_core::time::Zone::System,
            lines: self.items.iter().map(TicketItem::line).collect(),
            void: self.kind == TicketKind::Void,
        }
    }
}

impl TicketItem {
    fn line(&self) -> KitchenLine {
        KitchenLine {
            quantity_milli: self.quantity_milli,
            name: self.name.clone(),
            modifiers: self.modifiers.clone(),
            note: self.note.clone(),
            course: self.course,
        }
    }
}

pub fn get(conn: &Connection, id: Uuid) -> IpcResult<KitchenTicket> {
    rows::select(
        conn,
        &format!("SELECT {COLUMNS} FROM kitchen_tickets WHERE id = ?1 AND deleted_at IS NULL"),
        [id.to_string()],
    )
    .ipc()?
    .into_iter()
    .next()
    .ok_or_else(|| IpcError::new(IpcErrorCode::NotFound, "That ticket is gone."))
}

/// Mirrors `KitchenBoardSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct Board {
    pub open: Vec<KitchenTicket>,
    pub ready: Vec<KitchenTicket>,
    pub server_time: Timestamp,
}

pub fn board(conn: &Connection, recent_minutes: i64, now: Timestamp) -> IpcResult<Board> {
    let since = now
        .checked_add(Duration::hours(-BOARD_HOURS))
        .unwrap_or(now);
    let ready_since = now
        .checked_add(Duration::minutes(-recent_minutes.clamp(0, 240)))
        .unwrap_or(now);
    let open = rows::select(
        conn,
        &format!(
            "SELECT {COLUMNS} FROM kitchen_tickets
             WHERE status = 'open' AND deleted_at IS NULL AND fired_at >= ?1
             ORDER BY fired_at, ticket_number LIMIT 200"
        ),
        [since.to_string()],
    )
    .ipc()?;
    let ready = rows::select(
        conn,
        &format!(
            "SELECT {COLUMNS} FROM kitchen_tickets
             WHERE status = 'ready' AND deleted_at IS NULL AND ready_at >= ?1
             ORDER BY ready_at DESC LIMIT 50"
        ),
        [ready_since.to_string()],
    )
    .ipc()?;
    Ok(Board {
        open,
        ready,
        server_time: now,
    })
}

fn save(conn: &Connection, ticket: &KitchenTicket, now: Timestamp) -> IpcResult<()> {
    rows::upsert(conn, "kitchen_tickets", ticket, now).ipc()
}

/// Bumps a ticket off the board (`ready`) or recalls it.
pub fn bump(conn: &Connection, id: Uuid, ready: bool, now: Timestamp) -> IpcResult<KitchenTicket> {
    let mut ticket = get(conn, id)?;
    ticket.status = if ready {
        TicketStatus::Ready
    } else {
        TicketStatus::Open
    };
    ticket.ready_at = ready.then_some(now);
    ticket.meta.updated_at = now;
    save(conn, &ticket, now)?;
    Ok(ticket)
}

/// Strikes (or un-strikes) one item of a ticket.
pub fn set_done(
    conn: &Connection,
    id: Uuid,
    line_id: Uuid,
    done: bool,
    now: Timestamp,
) -> IpcResult<KitchenTicket> {
    let mut ticket = get(conn, id)?;
    let item = ticket
        .items
        .iter_mut()
        .find(|i| i.line_id == line_id)
        .ok_or_else(|| IpcError::validation("That item is not on the ticket."))?;
    item.done_at = done.then_some(now);
    ticket.meta.updated_at = now;
    save(conn, &ticket, now)?;
    Ok(ticket)
}

/// Mirrors `KitchenChangeSchema` (the `kitchen://changed` event).
#[derive(Debug, Clone, Serialize)]
pub struct Change {
    pub ticket_id: Uuid,
    pub ticket_number: i64,
    pub title: String,
    pub kind: TicketKind,
    pub status: TicketStatus,
}

impl From<&KitchenTicket> for Change {
    fn from(t: &KitchenTicket) -> Self {
        Self {
            ticket_id: t.meta.id,
            ticket_number: t.ticket_number,
            title: t.title.clone(),
            kind: t.kind,
            status: t.status,
        }
    }
}

fn order_type_label(order_type: OrderType) -> &'static str {
    match order_type {
        OrderType::Counter => "Counter",
        OrderType::DineIn => "Dine in",
        OrderType::Takeaway => "Takeaway",
        OrderType::Delivery => "Delivery",
    }
}

/// A ticket for a pay-now sale, from the stored transaction (see [`send`]).
pub fn for_sale(
    conn: &Connection,
    config: &ClientConfig,
    transaction_id: Uuid,
    now: Timestamp,
) -> IpcResult<Option<KitchenTicket>> {
    let id = transaction_id.to_string();
    let (device_id, receipt_number, order_type, server): (Uuid, String, OrderType, String) = conn
        .query_row(
            "SELECT t.device_id, t.receipt_number, t.order_type, COALESCE(u.display_name, '')
             FROM transactions t LEFT JOIN users u ON u.id = t.cashier_id WHERE t.id = ?1",
            [&id],
            |r| {
                Ok((
                    uuid_at(r, 0)?,
                    r.get(1)?,
                    crate::repo::enum_at(r, 2)?,
                    r.get(3)?,
                ))
            },
        )
        .ipc()?;
    let items: Vec<TicketItem> = conn
        .prepare(
            "SELECT id, quantity_milli, product_name, modifiers, note, course
             FROM transaction_items WHERE transaction_id = ?1 ORDER BY line_number",
        )
        .ipc()?
        .query_map(params![&id], |r| {
            let modifiers: Vec<pos_core::receipt::ModifierLine> =
                serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default();
            Ok(TicketItem::new(
                uuid_at(r, 0)?,
                &KitchenLine {
                    quantity_milli: r.get(1)?,
                    name: r.get(2)?,
                    modifiers: modifiers.into_iter().map(|m| m.name).collect(),
                    note: r.get(4)?,
                    course: r.get(5)?,
                },
            ))
        })
        .ipc()?
        .collect::<Result<_, _>>()
        .ipc()?;
    send(
        conn,
        config,
        device_id,
        Draft {
            kind: TicketKind::Order,
            order_id: None,
            transaction_id: Some(transaction_id),
            title: format!("{} {receipt_number}", order_type_label(order_type)),
            order_type,
            course: None,
            server_name: server,
            guests: 0,
            items,
        },
        now,
    )
}

/// A void ticket for every kitchen ticket of a sale that was voided (a
/// pay-now sale goes to the kitchen when it is paid). Only builds with the
/// display keep the rows this reads; the void ticket is also printed.
pub fn void_sale(
    conn: &Connection,
    transaction_id: Uuid,
    server_name: &str,
    now: Timestamp,
) -> IpcResult<Vec<KitchenTicket>> {
    let sent: Vec<KitchenTicket> = rows::select(
        conn,
        &format!(
            "SELECT {COLUMNS} FROM kitchen_tickets
             WHERE transaction_id = ?1 AND kind = 'order' AND deleted_at IS NULL"
        ),
        [transaction_id.to_string()],
    )
    .ipc()?;
    let device_id = crate::repo::device::id(conn).ipc()?;
    sent.into_iter()
        .map(|ticket| {
            create(
                conn,
                device_id,
                Draft {
                    kind: TicketKind::Void,
                    order_id: ticket.order_id,
                    transaction_id: Some(transaction_id),
                    title: ticket.title,
                    order_type: ticket.order_type,
                    course: ticket.course,
                    server_name: server_name.to_owned(),
                    guests: ticket.guests,
                    items: ticket
                        .items
                        .into_iter()
                        .map(|i| TicketItem { done_at: None, ..i })
                        .collect(),
                },
                now,
            )
            .and_then(|void| {
                let printed = Draft {
                    kind: void.kind,
                    order_id: void.order_id,
                    transaction_id: void.transaction_id,
                    title: void.title.clone(),
                    order_type: void.order_type,
                    course: void.course,
                    server_name: void.server_name.clone(),
                    guests: void.guests,
                    items: void.items.clone(),
                }
                .printed(now);
                PrintService::queue_kitchen(conn, &printed, now)?;
                Ok(void)
            })
        })
        .collect()
}

type Listener = Box<dyn Fn(&Change) + Send + Sync>;

/// Tells the windows that a ticket changed (`kitchen://changed`).
#[derive(Default)]
pub struct KitchenHub {
    listener: std::sync::OnceLock<Listener>,
}

impl KitchenHub {
    pub fn set_listener(&self, listener: impl Fn(&Change) + Send + Sync + 'static) {
        let _ = self.listener.set(Box::new(listener));
    }

    pub fn changed(&self, ticket: &KitchenTicket) {
        if let Some(listener) = self.listener.get() {
            listener(&Change::from(ticket));
        }
    }
}

#[cfg(test)]
mod tests;

//! Memberships: plans a shop sells (a monthly club, a yearly VIP card) and
//! the periods its customers hold. Both sync last-write-wins.
//!
//! A plan owns a product row (category "Memberships"), so it is sold like
//! anything else — paid, printed, reported, refunded. Selling it to a named
//! customer starts a period, or adds one after the period they already hold
//! (renewal); refunding or voiding that sale cancels the periods it bought.
//! Members get the plan's discount on every bill and earn points faster.

use chrono::Duration;
use pos_core::config::ClientConfig;
use pos_core::time::Timestamp;
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use rand::Rng;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::audit::Actor;
use super::catalog::{self, Product, Unit};
use super::{rows, Meta, SqlResultExt};

/// The category plan products are filed under.
pub const CATEGORY_NAME: &str = "Memberships";

/// Mirrors `MembershipPlanSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    #[serde(flatten)]
    pub meta: Meta,
    pub name: String,
    pub description: Option<String>,
    pub product_id: Uuid,
    pub price: i64,
    pub duration_days: i64,
    pub discount_bps: i64,
    pub points_multiplier_bps: i64,
    pub color: Option<String>,
    #[serde(deserialize_with = "rows::int_bool")]
    pub is_active: bool,
}

const PLAN_COLUMNS: &str = "id, created_at, updated_at, deleted_at, name, description, product_id,
    price, duration_days, discount_bps, points_multiplier_bps, color, is_active";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Active,
    Cancelled,
}

/// Mirrors `MembershipSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Membership {
    #[serde(flatten)]
    pub meta: Meta,
    pub customer_id: Uuid,
    pub plan_id: Uuid,
    pub card_number: String,
    pub starts_at: Timestamp,
    pub ends_at: Timestamp,
    pub status: Status,
    pub transaction_id: Option<Uuid>,
    pub price_paid: i64,
    pub device_id: Uuid,
    pub notes: Option<String>,
}

const COLUMNS: &str = "id, created_at, updated_at, deleted_at, customer_id, plan_id, card_number,
    starts_at, ends_at, status, transaction_id, price_paid, device_id, notes";

fn invalid(message: impl Into<String>) -> IpcError {
    IpcError::validation(message)
}

// ── Plans ──────────────────────────────────────────────────────────────────

pub fn plans(conn: &Connection) -> IpcResult<Vec<Plan>> {
    rows::select(
        conn,
        &format!(
            "SELECT {PLAN_COLUMNS} FROM membership_plans WHERE deleted_at IS NULL
             ORDER BY is_active DESC, price, name COLLATE NOCASE"
        ),
        [],
    )
    .ipc()
}

pub fn plan(conn: &Connection, id: Uuid) -> IpcResult<Option<Plan>> {
    Ok(rows::select(
        conn,
        &format!(
            "SELECT {PLAN_COLUMNS} FROM membership_plans WHERE id = ?1 AND deleted_at IS NULL"
        ),
        [id.to_string()],
    )
    .ipc()?
    .into_iter()
    .next())
}

/// The plan a product sells, if it is a plan's product.
pub fn plan_for_product(conn: &Connection, product_id: Uuid) -> IpcResult<Option<Plan>> {
    Ok(rows::select(
        conn,
        &format!(
            "SELECT {PLAN_COLUMNS} FROM membership_plans WHERE product_id = ?1 AND deleted_at IS NULL"
        ),
        [product_id.to_string()],
    )
    .ipc()?
    .into_iter()
    .next())
}

/// Mirrors `MembershipPlanInputSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct PlanInput {
    pub id: Option<Uuid>,
    pub name: String,
    pub description: Option<String>,
    pub price: i64,
    pub duration_days: i64,
    pub discount_bps: i64,
    pub points_multiplier_bps: i64,
    pub color: Option<String>,
    pub is_active: bool,
}

fn membership_category(conn: &Connection, now: Timestamp) -> IpcResult<Uuid> {
    let categories = catalog::categories(conn).ipc()?;
    if let Some(existing) = categories.iter().find(|c| c.name == CATEGORY_NAME) {
        return Ok(existing.meta.id);
    }
    let mut category = catalog::new_category(CATEGORY_NAME, 900, Some("#8b5cf6"), now);
    category.name_localized = serde_json::json!({ "ar": "العضويات" });
    catalog::save_category(conn, &category, now).ipc()?;
    Ok(category.meta.id)
}

/// Creates or changes a plan and keeps its product in step (name, price,
/// on sale or not).
pub fn save_plan(
    conn: &Connection,
    input: PlanInput,
    config: &ClientConfig,
    now: Timestamp,
) -> IpcResult<Plan> {
    let name = input.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(invalid("Give the plan a name (up to 80 characters)."));
    }
    if input.price < 0 {
        return Err(invalid("The price cannot be negative."));
    }
    if !(1..=3660).contains(&input.duration_days) {
        return Err(invalid("A membership lasts from 1 day to 10 years."));
    }
    if !(0..=10_000).contains(&input.discount_bps) {
        return Err(invalid("The member discount is between 0% and 100%."));
    }
    if !(0..=100_000).contains(&input.points_multiplier_bps) {
        return Err(invalid("Points can be multiplied up to 10 times."));
    }
    let description = input
        .description
        .map(|d| d.trim().to_owned())
        .filter(|d| !d.is_empty());
    if description
        .as_ref()
        .is_some_and(|d| d.chars().count() > 500)
    {
        return Err(invalid("The description is too long."));
    }
    let current = match input.id {
        Some(id) => Some(plan(conn, id)?.ok_or_else(|| invalid("That plan was removed."))?),
        None => None,
    };
    let product = match current
        .as_ref()
        .map(|p| catalog::get(conn, p.product_id))
        .transpose()
        .ipc()?
        .flatten()
    {
        Some(existing) => Product {
            meta: Meta {
                updated_at: now,
                ..existing.meta
            },
            name: name.clone(),
            price: input.price,
            is_active: input.is_active,
            ..existing
        },
        None => Product {
            meta: Meta::new(now),
            name: name.clone(),
            name_localized: serde_json::json!({}),
            category_id: Some(membership_category(conn, now)?),
            sku: None,
            barcode: None,
            price: input.price,
            cost: None,
            tax_rate_bps: i64::from(config.tax.default_rate_bps),
            unit: Unit::Each,
            sold_by_weight: false,
            track_stock: false,
            stock_on_hand_milli: 0,
            reorder_threshold_milli: None,
            reorder_quantity_milli: None,
            image_asset: None,
            quick_key_position: None,
            is_active: input.is_active,
        },
    };
    catalog::save(conn, &product, now).ipc()?;
    let plan = Plan {
        meta: match current {
            Some(p) => Meta {
                updated_at: now,
                ..p.meta
            },
            None => Meta::new(now),
        },
        name,
        description,
        product_id: product.meta.id,
        price: input.price,
        duration_days: input.duration_days,
        discount_bps: input.discount_bps,
        points_multiplier_bps: input.points_multiplier_bps,
        color: input.color.filter(|c| !c.trim().is_empty()),
        is_active: input.is_active,
    };
    rows::upsert(conn, "membership_plans", &plan, now).ipc()?;
    Ok(plan)
}

/// Removes a plan (and takes its product off sale). Members keep the
/// periods they paid for.
pub fn delete_plan(conn: &Connection, id: Uuid, now: Timestamp) -> IpcResult<Plan> {
    let current = plan(conn, id)?.ok_or_else(|| invalid("That plan was removed."))?;
    if let Some(product) = catalog::get(conn, current.product_id).ipc()? {
        catalog::save(
            conn,
            &Product {
                meta: Meta {
                    updated_at: now,
                    ..product.meta
                },
                is_active: false,
                ..product
            },
            now,
        )
        .ipc()?;
    }
    let removed = Plan {
        meta: Meta {
            updated_at: now,
            deleted_at: Some(now),
            ..current.meta
        },
        is_active: false,
        ..current
    };
    rows::upsert(conn, "membership_plans", &removed, now).ipc()?;
    Ok(removed)
}

// ── Memberships ────────────────────────────────────────────────────────────

pub fn get(conn: &Connection, id: Uuid) -> IpcResult<Option<Membership>> {
    Ok(rows::select(
        conn,
        &format!("SELECT {COLUMNS} FROM memberships WHERE id = ?1 AND deleted_at IS NULL"),
        [id.to_string()],
    )
    .ipc()?
    .into_iter()
    .next())
}

/// The period a customer holds at `at`, with its plan.
pub fn active(
    conn: &Connection,
    customer_id: Uuid,
    at: Timestamp,
) -> IpcResult<Option<(Membership, Plan)>> {
    let current: Option<Membership> = rows::select(
        conn,
        &format!(
            "SELECT {COLUMNS} FROM memberships
             WHERE customer_id = ?1 AND deleted_at IS NULL AND status = 'active'
               AND starts_at <= ?2 AND ends_at > ?2
             ORDER BY ends_at DESC LIMIT 1"
        ),
        params![customer_id.to_string(), at.to_string()],
    )
    .ipc()?
    .into_iter()
    .next();
    let Some(membership) = current else {
        return Ok(None);
    };
    // A removed plan still honours the periods it sold.
    let plan: Option<Plan> = rows::select(
        conn,
        &format!("SELECT {PLAN_COLUMNS} FROM membership_plans WHERE id = ?1"),
        [membership.plan_id.to_string()],
    )
    .ipc()?
    .into_iter()
    .next();
    Ok(plan.map(|p| (membership, p)))
}

/// EAN-13 in the in-store range (prefix 29), so cards can carry a barcode.
fn new_card_number(conn: &Connection) -> IpcResult<String> {
    let mut rng = rand::thread_rng();
    for _ in 0..20 {
        let body: String = std::iter::once("29".to_owned())
            .chain((0..10).map(|_| rng.gen_range(0..10).to_string()))
            .collect();
        let sum: u32 = body
            .bytes()
            .rev()
            .enumerate()
            .map(|(i, d)| u32::from(d - b'0') * if i % 2 == 0 { 3 } else { 1 })
            .sum();
        let card = format!("{body}{}", (10 - sum % 10) % 10);
        let taken: bool = conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM memberships WHERE card_number = ?1)",
                [&card],
                |r| r.get(0),
            )
            .ipc()?;
        if !taken {
            return Ok(card);
        }
    }
    Err(IpcError::internal("could not draw a free card number"))
}

/// Starts (or, after the period they hold, adds) `periods` periods of
/// `plan` for a customer. Keeps the customer's card number.
#[allow(clippy::too_many_arguments)]
pub fn grant(
    conn: &Connection,
    customer_id: Uuid,
    plan: &Plan,
    periods: i64,
    transaction_id: Option<Uuid>,
    price_paid: i64,
    notes: Option<String>,
    actor: &Actor,
    now: Timestamp,
) -> IpcResult<Membership> {
    let periods = periods.clamp(1, 120);
    let latest: Option<Membership> = rows::select(
        conn,
        &format!(
            "SELECT {COLUMNS} FROM memberships
             WHERE customer_id = ?1 AND deleted_at IS NULL AND status = 'active'
             ORDER BY ends_at DESC LIMIT 1"
        ),
        [customer_id.to_string()],
    )
    .ipc()?
    .into_iter()
    .next();
    let starts_at = latest
        .as_ref()
        .map(|m| m.ends_at)
        .filter(|end| *end > now)
        .unwrap_or(now);
    let ends_at = starts_at
        .checked_add(Duration::days(plan.duration_days * periods))
        .ok_or_else(|| invalid("That membership would end too far in the future."))?;
    let card_number = match conn
        .query_row(
            "SELECT card_number FROM memberships WHERE customer_id = ?1 AND deleted_at IS NULL
             ORDER BY created_at DESC LIMIT 1",
            [customer_id.to_string()],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .ipc()?
    {
        Some(card) => card,
        None => new_card_number(conn)?,
    };
    let membership = Membership {
        meta: Meta::new(now),
        customer_id,
        plan_id: plan.meta.id,
        card_number,
        starts_at,
        ends_at,
        status: Status::Active,
        transaction_id,
        price_paid,
        device_id: actor.device_id,
        notes: notes.filter(|n| !n.trim().is_empty()),
    };
    rows::upsert(conn, "memberships", &membership, now).ipc()?;
    Ok(membership)
}

pub fn cancel(conn: &Connection, id: Uuid, now: Timestamp) -> IpcResult<Membership> {
    let current = get(conn, id)?.ok_or_else(|| invalid("That membership was removed."))?;
    if current.status == Status::Cancelled {
        return Ok(current);
    }
    let cancelled = Membership {
        meta: Meta {
            updated_at: now,
            ..current.meta
        },
        status: Status::Cancelled,
        ..current
    };
    rows::upsert(conn, "memberships", &cancelled, now).ipc()?;
    Ok(cancelled)
}

/// A sold line that is a membership plan.
pub struct SoldPlan {
    pub product_id: Uuid,
    pub quantity_milli: i64,
    pub line_total: i64,
}

/// After a sale is written: every plan on it starts or extends the
/// customer's membership. A plan sold without a customer is refused (the
/// whole sale rolls back).
pub fn on_sale(
    conn: &Connection,
    transaction_id: Uuid,
    customer_id: Option<Uuid>,
    lines: &[SoldPlan],
    actor: &Actor,
    now: Timestamp,
) -> IpcResult<Vec<Membership>> {
    let mut granted = Vec::new();
    for line in lines {
        let Some(plan) = plan_for_product(conn, line.product_id)? else {
            continue;
        };
        let customer_id = customer_id.ok_or_else(|| {
            IpcError::new(
                IpcErrorCode::Validation,
                format!("Choose the customer who is buying “{}”.", plan.name),
            )
        })?;
        let periods = (line.quantity_milli / 1000).max(1);
        granted.push(grant(
            conn,
            customer_id,
            &plan,
            periods,
            Some(transaction_id),
            line.line_total,
            None,
            actor,
            now,
        )?);
    }
    Ok(granted)
}

/// A refund or void of a sale: the periods it bought for the reversed plan
/// products are cancelled.
pub fn on_reversal(
    conn: &Connection,
    sale_id: Uuid,
    reversed_products: &[Uuid],
    now: Timestamp,
) -> IpcResult<()> {
    let bought: Vec<Membership> = rows::select(
        conn,
        &format!(
            "SELECT {COLUMNS} FROM memberships
             WHERE transaction_id = ?1 AND deleted_at IS NULL AND status = 'active'"
        ),
        [sale_id.to_string()],
    )
    .ipc()?;
    for membership in bought {
        let product = plan_by_id_any(conn, membership.plan_id)?.map(|p| p.product_id);
        if product.is_some_and(|p| reversed_products.contains(&p)) {
            cancel(conn, membership.meta.id, now)?;
        }
    }
    Ok(())
}

fn plan_by_id_any(conn: &Connection, id: Uuid) -> IpcResult<Option<Plan>> {
    Ok(rows::select(
        conn,
        &format!("SELECT {PLAN_COLUMNS} FROM membership_plans WHERE id = ?1"),
        [id.to_string()],
    )
    .ipc()?
    .into_iter()
    .next())
}

/// Mirrors `MemberStateSchema`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberState {
    Active,
    Upcoming,
    Expired,
    Cancelled,
}

/// Mirrors `MemberRowSchema`: one period with who holds it.
#[derive(Debug, Clone, Serialize)]
pub struct MemberRow {
    pub membership: Membership,
    pub customer_name: String,
    pub customer_phone: Option<String>,
    pub plan_name: String,
    pub state: MemberState,
}

/// Mirrors `MemberFilterSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct MemberFilter {
    #[serde(default)]
    pub query: String,
    /// `None` = every state.
    pub state: Option<MemberState>,
    /// One customer's periods only.
    #[serde(default)]
    pub customer_id: Option<Uuid>,
    pub limit: i64,
}

fn state_of(m: &Membership, now: Timestamp) -> MemberState {
    match m.status {
        Status::Cancelled => MemberState::Cancelled,
        Status::Active if m.ends_at <= now => MemberState::Expired,
        Status::Active if m.starts_at > now => MemberState::Upcoming,
        Status::Active => MemberState::Active,
    }
}

/// Members for the Memberships screen: by name, phone or card number.
pub fn members(
    conn: &Connection,
    filter: &MemberFilter,
    now: Timestamp,
) -> IpcResult<Vec<MemberRow>> {
    let limit = filter.limit.clamp(1, 500);
    let query = filter.query.trim();
    let pattern = super::catalog::like_pattern(query);
    let found: Vec<(Membership, String, Option<String>, String)> = conn
        .prepare(&format!(
            "SELECT m.{cols}, c.display_name, c.phone, COALESCE(p.name, '?')
             FROM memberships m
             JOIN customers c ON c.id = m.customer_id
             LEFT JOIN membership_plans p ON p.id = m.plan_id
             WHERE m.deleted_at IS NULL
               AND (?1 = '' OR c.display_name LIKE ?2 ESCAPE '\\' OR c.phone LIKE ?2 ESCAPE '\\'
                    OR m.card_number LIKE ?2 ESCAPE '\\')
               AND (?3 IS NULL OR m.customer_id = ?3)
             ORDER BY m.ends_at DESC LIMIT 2000",
            cols = COLUMNS.replace(", ", ", m.").replace("\n    ", "\n    m.")
        ))
        .ipc()?
        .query_map(
            params![query, pattern, filter.customer_id.map(|id| id.to_string())],
            |r| {
                Ok((
                    Membership {
                        meta: Meta::read(r, 0)?,
                        customer_id: super::uuid_at(r, 4)?,
                        plan_id: super::uuid_at(r, 5)?,
                        card_number: r.get(6)?,
                        starts_at: super::ts_at(r, 7)?,
                        ends_at: super::ts_at(r, 8)?,
                        status: super::enum_at(r, 9)?,
                        transaction_id: super::opt_uuid_at(r, 10)?,
                        price_paid: r.get(11)?,
                        device_id: super::uuid_at(r, 12)?,
                        notes: r.get(13)?,
                    },
                    r.get(14)?,
                    r.get(15)?,
                    r.get(16)?,
                ))
            },
        )
        .ipc()?
        .collect::<Result<_, _>>()
        .ipc()?;
    Ok(found
        .into_iter()
        .map(
            |(membership, customer_name, customer_phone, plan_name)| MemberRow {
                state: state_of(&membership, now),
                membership,
                customer_name,
                customer_phone,
                plan_name,
            },
        )
        .filter(|row| filter.state.map_or(true, |s| s == row.state))
        .take(usize::try_from(limit).unwrap_or(500))
        .collect())
}

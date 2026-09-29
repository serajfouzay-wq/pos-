//! Discount rules (`discount_rules`, synced last-write-wins).
//!
//! A rule is a percentage or a fixed amount off the whole bill, a product or
//! a category, with an optional minimum spend, validity dates, days of the
//! week and a daily time window (local time). `automatic` rules are priced
//! into every sale while they run; `manual` ones are applied to a bill by a
//! manager or owner (`discount.apply`).

use pos_core::pricing::{Discount, DiscountScope, DiscountValue};
use pos_core::schedule::Schedule;
use pos_core::time::{Timestamp, Zone};
use pos_core::{IpcError, IpcResult};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{catalog, rows, Meta, SqlResultExt};

/// The wall clock schedules run in: the till's own; tests pin UTC+3 so they
/// do not depend on the machine (like the reports, D50).
#[cfg(not(test))]
pub const RULE_ZONE: Zone = Zone::System;
#[cfg(test)]
pub const RULE_ZONE: Zone = Zone::Fixed(180);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscountKind {
    Percentage,
    FixedAmount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleScope {
    Order,
    Product,
    Category,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyMode {
    Manual,
    Automatic,
}

/// Mirrors `DiscountRuleSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscountRule {
    #[serde(flatten)]
    pub meta: Meta,
    pub name: String,
    pub kind: DiscountKind,
    /// Basis points (percentage) or minor units (fixed amount).
    pub value: i64,
    pub scope: RuleScope,
    pub target_id: Option<Uuid>,
    pub min_subtotal: Option<i64>,
    pub starts_at: Option<Timestamp>,
    pub ends_at: Option<Timestamp>,
    #[serde(deserialize_with = "rows::int_bool")]
    pub is_active: bool,
    /// `None` on rows written before schedules existed: manual.
    pub apply_mode: Option<ApplyMode>,
    pub days_mask: Option<i64>,
    pub time_from: Option<i64>,
    pub time_to: Option<i64>,
}

const COLUMNS: &str = "id, created_at, updated_at, deleted_at, name, kind, value, scope, target_id,
    min_subtotal, starts_at, ends_at, is_active, apply_mode, days_mask, time_from, time_to";

impl DiscountRule {
    pub fn mode(&self) -> ApplyMode {
        self.apply_mode.unwrap_or(ApplyMode::Manual)
    }

    pub fn schedule(&self) -> Schedule {
        Schedule {
            days_mask: self.days_mask,
            time_from: self.time_from,
            time_to: self.time_to,
        }
    }

    /// Whether the rule runs at `now` (the schedule in `zone`'s local time).
    pub fn is_live(&self, now: Timestamp, zone: Zone) -> bool {
        self.is_active
            && self.meta.deleted_at.is_none()
            && self.starts_at.map_or(true, |s| now >= s)
            && self.ends_at.map_or(true, |e| now < e)
            && self.schedule().allows(zone.local(now))
    }

    /// The rule as the pricing engine applies it.
    pub fn to_pricing(&self) -> IpcResult<Discount> {
        let scope = match (self.scope, self.target_id) {
            (RuleScope::Order, _) => DiscountScope::Order,
            (RuleScope::Product, Some(t)) => DiscountScope::Product(t),
            (RuleScope::Category, Some(t)) => DiscountScope::Category(t),
            _ => return Err(IpcError::validation("That discount is misconfigured.")),
        };
        Ok(Discount {
            id: self.meta.id,
            value: match self.kind {
                DiscountKind::Percentage => DiscountValue::Percentage(self.value),
                DiscountKind::FixedAmount => DiscountValue::Fixed(self.value),
            },
            scope,
            min_subtotal: self.min_subtotal,
        })
    }
}

pub fn list(conn: &Connection) -> IpcResult<Vec<DiscountRule>> {
    rows::select(
        conn,
        &format!(
            "SELECT {COLUMNS} FROM discount_rules WHERE deleted_at IS NULL
             ORDER BY is_active DESC, name COLLATE NOCASE"
        ),
        [],
    )
    .ipc()
}

pub fn get(conn: &Connection, id: Uuid) -> IpcResult<Option<DiscountRule>> {
    Ok(rows::select(
        conn,
        &format!("SELECT {COLUMNS} FROM discount_rules WHERE id = ?1 AND deleted_at IS NULL"),
        [id.to_string()],
    )
    .ipc()?
    .into_iter()
    .next())
}

/// The automatic rules running at `now`.
pub fn automatic(conn: &Connection, now: Timestamp, zone: Zone) -> IpcResult<Vec<DiscountRule>> {
    Ok(list(conn)?
        .into_iter()
        .filter(|r| r.mode() == ApplyMode::Automatic && r.is_live(now, zone))
        .collect())
}

/// Manual rules a bill may use at `now`, for [`crate::repo::sales`].
pub fn manual(
    conn: &Connection,
    ids: &[Uuid],
    now: Timestamp,
    zone: Zone,
) -> IpcResult<Vec<DiscountRule>> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let rule =
            get(conn, *id)?.ok_or_else(|| IpcError::validation("That discount does not exist."))?;
        if !rule.is_live(now, zone) {
            return Err(IpcError::validation(format!(
                "“{}” is not running right now.",
                rule.name
            )));
        }
        out.push(rule);
    }
    Ok(out)
}

/// Mirrors `DiscountRuleInputSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct DiscountRuleInput {
    pub id: Option<Uuid>,
    pub name: String,
    pub kind: DiscountKind,
    pub value: i64,
    pub scope: RuleScope,
    pub target_id: Option<Uuid>,
    pub min_subtotal: Option<i64>,
    pub starts_at: Option<Timestamp>,
    pub ends_at: Option<Timestamp>,
    pub is_active: bool,
    pub apply_mode: ApplyMode,
    pub days_mask: Option<i64>,
    pub time_from: Option<i64>,
    pub time_to: Option<i64>,
}

fn invalid(message: impl Into<String>) -> IpcError {
    IpcError::validation(message)
}

/// Validates and writes a rule (new or edited). Returns it as stored.
pub fn save(
    conn: &Connection,
    input: DiscountRuleInput,
    now: Timestamp,
) -> IpcResult<DiscountRule> {
    let name = input.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(invalid("Give the discount a name (up to 80 characters)."));
    }
    match input.kind {
        DiscountKind::Percentage if !(1..=10_000).contains(&input.value) => {
            return Err(invalid("A percentage is between 0.01% and 100%."));
        }
        DiscountKind::FixedAmount if input.value <= 0 => {
            return Err(invalid("The amount off must be more than zero."));
        }
        _ => {}
    }
    let target_id = match input.scope {
        RuleScope::Order => None,
        RuleScope::Product => {
            let id = input
                .target_id
                .ok_or_else(|| invalid("Choose the product."))?;
            catalog::get(conn, id)
                .ipc()?
                .ok_or_else(|| invalid("That product no longer exists."))?;
            Some(id)
        }
        RuleScope::Category => {
            let id = input
                .target_id
                .ok_or_else(|| invalid("Choose the category."))?;
            if !catalog::categories(conn)
                .ipc()?
                .iter()
                .any(|c| c.meta.id == id)
            {
                return Err(invalid("That category no longer exists."));
            }
            Some(id)
        }
    };
    if input.min_subtotal.is_some_and(|m| m < 0) {
        return Err(invalid("The minimum spend cannot be negative."));
    }
    if let (Some(s), Some(e)) = (input.starts_at, input.ends_at) {
        if e <= s {
            return Err(invalid("The discount must end after it starts."));
        }
    }
    let schedule = Schedule {
        days_mask: input
            .days_mask
            .filter(|m| *m != pos_core::schedule::ALL_DAYS),
        time_from: input.time_from,
        time_to: input.time_to,
    };
    schedule
        .validate()
        .map_err(|e| invalid(format!("Schedule: {e}.")))?;

    let meta = match input.id {
        Some(id) => {
            let current = get(conn, id)?.ok_or_else(|| invalid("That discount was removed."))?;
            Meta {
                updated_at: now,
                ..current.meta
            }
        }
        None => Meta::new(now),
    };
    let rule = DiscountRule {
        meta,
        name,
        kind: input.kind,
        value: input.value,
        scope: input.scope,
        target_id,
        min_subtotal: input.min_subtotal.filter(|m| *m > 0),
        starts_at: input.starts_at,
        ends_at: input.ends_at,
        is_active: input.is_active,
        apply_mode: Some(input.apply_mode),
        days_mask: schedule.days_mask,
        time_from: schedule.time_from,
        time_to: schedule.time_to,
    };
    rows::upsert(conn, "discount_rules", &rule, now).ipc()?;
    Ok(rule)
}

/// Soft-deletes a rule; sales that used it keep their amounts.
pub fn delete(conn: &Connection, id: Uuid, now: Timestamp) -> IpcResult<DiscountRule> {
    let current = get(conn, id)?.ok_or_else(|| invalid("That discount was removed."))?;
    let rule = DiscountRule {
        meta: Meta {
            updated_at: now,
            deleted_at: Some(now),
            ..current.meta
        },
        ..current
    };
    rows::upsert(conn, "discount_rules", &rule, now).ipc()?;
    Ok(rule)
}

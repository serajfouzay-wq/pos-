//! Loyalty at the till: pricing a bill for a customer (points redeemed as
//! an order discount, points to earn), and registering / editing customers.
//!
//! The redemption is an ordinary order-scoped discount in the pricing
//! engine, applied after every other discount and spread across the lines,
//! so tax and later refunds treat it like any discount.

use pos_core::config::ClientConfig;
use pos_core::loyalty::LoyaltySettings;
use pos_core::pricing::{self, Discount, DiscountScope, DiscountValue};
use pos_core::time::Timestamp;
use pos_core::{IpcError, IpcErrorCode, IpcResult};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::repo::audit::{self, Actor};
use crate::repo::customers::{self, Customer, LedgerReason};
use crate::repo::sales::{self, PayloadItem, PricedCart};
use crate::repo::{shop, Meta, SqlResultExt};

fn invalid(message: impl Into<String>) -> IpcError {
    IpcError::validation(message)
}

/// Mirrors `LoyaltyRequestSchema`.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct LoyaltyRequest {
    pub customer_id: Uuid,
    #[serde(default)]
    pub redeem_points: i64,
}

/// Mirrors `LoyaltyQuoteSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoyaltyQuote {
    pub customer_id: Uuid,
    pub customer_name: String,
    pub enabled: bool,
    pub balance: i64,
    pub redeem_points: i64,
    pub redeem_value: i64,
    pub max_redeem_points: i64,
    pub points_earned: i64,
}

/// A priced bill and, when a customer was named, their points on it.
#[derive(Debug)]
pub struct CustomerCart {
    pub cart: PricedCart,
    pub loyalty: Option<LoyaltyQuote>,
}

pub fn live_customer(conn: &Connection, id: Uuid) -> IpcResult<Customer> {
    customers::get(conn, id)
        .ipc()?
        .ok_or_else(|| IpcError::new(IpcErrorCode::NotFound, "That customer does not exist."))
}

/// Prices the bill, then (for a customer) checks the redemption against the
/// rules and prices it again with the points taken off.
pub fn price(
    conn: &Connection,
    items: &[PayloadItem],
    discount_rule_ids: &[Uuid],
    request: Option<LoyaltyRequest>,
    config: &ClientConfig,
    now: Timestamp,
) -> IpcResult<CustomerCart> {
    let cart = sales::price_cart(conn, items, discount_rule_ids, config, now)?;
    let Some(request) = request else {
        return Ok(CustomerCart {
            cart,
            loyalty: None,
        });
    };
    let customer = live_customer(conn, request.customer_id)?;
    let rules: LoyaltySettings = shop::effective_loyalty(conn, config).ipc()?;
    let payable = cart.quote.subtotal - cart.quote.discount_total;
    let max = rules.max_redeemable(customer.loyalty_points, payable);
    let points = request.redeem_points;
    if points < 0 {
        return Err(invalid("Points to redeem cannot be negative."));
    }
    if points > 0 && !rules.enabled {
        return Err(invalid("The loyalty programme is off."));
    }
    if points > 0 && points < rules.min_redeem_points {
        return Err(invalid(format!(
            "Redeem at least {} points.",
            rules.min_redeem_points
        )));
    }
    if points > max {
        return Err(invalid(if max == 0 {
            format!(
                "{} cannot redeem points on this bill (balance {}, minimum {}).",
                customer.display_name, customer.loyalty_points, rules.min_redeem_points
            )
        } else {
            format!("This bill can take at most {max} points.")
        }));
    }
    let value = rules.value_of(points);
    let cart = if points > 0 {
        let mut discounts = cart.discounts.clone();
        discounts.push(Discount {
            id: shop::LOYALTY_ID,
            value: DiscountValue::Fixed(value),
            scope: DiscountScope::Order,
            min_subtotal: None,
        });
        let quote = pricing::price(&cart.lines, &discounts, config.tax.prices_include_tax)
            .map_err(|e| invalid(e.to_string()))?;
        if quote.discount_total - cart.quote.discount_total != value {
            return Err(invalid("The points do not fit this bill."));
        }
        PricedCart {
            quote,
            discounts,
            ..cart
        }
    } else {
        cart
    };
    let points_earned = rules.earned(cart.quote.total, config.currency.base);
    Ok(CustomerCart {
        loyalty: Some(LoyaltyQuote {
            customer_id: customer.meta.id,
            customer_name: customer.display_name,
            enabled: rules.enabled,
            balance: customer.loyalty_points,
            redeem_points: points,
            redeem_value: value,
            max_redeem_points: max,
            points_earned,
        }),
        cart,
    })
}

/// Records the points of a sale: the redemption, then what it earned.
pub fn record_sale_points(
    conn: &Connection,
    quote: &LoyaltyQuote,
    transaction_id: Uuid,
    actor: &Actor,
    now: Timestamp,
) -> IpcResult<()> {
    customers::add_points(
        conn,
        quote.customer_id,
        -quote.redeem_points,
        LedgerReason::Redeem,
        Some(transaction_id),
        actor,
        now,
    )
    .ipc()?;
    customers::add_points(
        conn,
        quote.customer_id,
        quote.points_earned,
        LedgerReason::Earn,
        Some(transaction_id),
        actor,
        now,
    )
    .ipc()
}

// ── Customers ──────────────────────────────────────────────────────────────

/// Mirrors `CustomerInputSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct CustomerInput {
    pub id: Option<Uuid>,
    pub display_name: String,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub notes: Option<String>,
}

fn clean(text: Option<String>, max: usize, what: &str) -> IpcResult<Option<String>> {
    let text = text.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
    if text.as_ref().is_some_and(|t| t.chars().count() > max) {
        return Err(invalid(format!("{what} is too long.")));
    }
    Ok(text)
}

/// Registers (`id: None`) or edits a customer. The caller checked the
/// permission for the kind of change.
pub fn save(
    conn: &Connection,
    actor: &Actor,
    input: CustomerInput,
    now: Timestamp,
) -> IpcResult<Customer> {
    let name = input.display_name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(invalid("Enter the customer's name (up to 120 characters)."));
    }
    let phone = match input.phone.as_deref().map(str::trim) {
        Some(p) if !p.is_empty() => {
            if !p.chars().all(|c| c.is_ascii_digit() || "+-() ".contains(c)) {
                return Err(invalid("A phone number has digits only."));
            }
            let normalized = customers::normalize_phone(p);
            if normalized
                .as_ref()
                .map_or(true, |n| n.len() < 5 || n.len() > 20)
            {
                return Err(invalid("That phone number looks incomplete."));
            }
            normalized
        }
        _ => None,
    };
    let email = clean(input.email, 200, "The email")?;
    if email
        .as_ref()
        .is_some_and(|e| !e.contains('@') || e.contains(' '))
    {
        return Err(invalid("That email address is not valid."));
    }
    if let Some(phone) = &phone {
        if let Some(other) = customers::by_phone(conn, phone, input.id).ipc()? {
            return Err(IpcError::new(
                IpcErrorCode::Conflict,
                format!("{phone} is already registered to {}.", other.display_name),
            ));
        }
    }
    let before = match input.id {
        Some(id) => Some(live_customer(conn, id)?),
        None => None,
    };
    let customer = Customer {
        meta: match &before {
            Some(b) => Meta {
                updated_at: now,
                ..b.meta.clone()
            },
            None => Meta::new(now),
        },
        display_name: name,
        phone,
        email,
        loyalty_points: before.as_ref().map_or(0, |b| b.loyalty_points),
        notes: clean(input.notes, 1000, "The note")?,
    };
    customers::save(conn, &customer, now).ipc()?;
    audit::record(
        conn,
        actor,
        if before.is_some() {
            "customer.update"
        } else {
            "customer.create"
        },
        "customers",
        Some(customer.meta.id),
        before.as_ref().and_then(|b| serde_json::to_value(b).ok()),
        serde_json::to_value(&customer).ok(),
        now,
    )
    .ipc()?;
    Ok(customer)
}

pub fn delete(conn: &Connection, actor: &Actor, id: Uuid, now: Timestamp) -> IpcResult<()> {
    let before = live_customer(conn, id)?;
    let deleted = Customer {
        meta: Meta {
            updated_at: now,
            deleted_at: Some(now),
            ..before.meta.clone()
        },
        ..before.clone()
    };
    customers::save(conn, &deleted, now).ipc()?;
    audit::record(
        conn,
        actor,
        "customer.delete",
        "customers",
        Some(id),
        serde_json::to_value(&before).ok(),
        None,
        now,
    )
    .ipc()
}

/// Mirrors `PointsAdjustmentSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct PointsAdjustment {
    pub customer_id: Uuid,
    pub points_delta: i64,
    pub note: String,
}

pub fn adjust(
    conn: &Connection,
    actor: &Actor,
    input: &PointsAdjustment,
    now: Timestamp,
) -> IpcResult<Customer> {
    let note = input.note.trim();
    if note.is_empty() || note.chars().count() > 200 {
        return Err(invalid("Say why the points change (up to 200 characters)."));
    }
    if input.points_delta == 0 || input.points_delta.abs() > 1_000_000 {
        return Err(invalid("Enter the points to add or remove."));
    }
    let before = live_customer(conn, input.customer_id)?;
    customers::add_points(
        conn,
        before.meta.id,
        input.points_delta,
        LedgerReason::Adjust,
        None,
        actor,
        now,
    )
    .ipc()?;
    let after = live_customer(conn, before.meta.id)?;
    audit::record(
        conn,
        actor,
        "loyalty.adjust",
        "customers",
        Some(before.meta.id),
        Some(serde_json::json!({ "loyalty_points": before.loyalty_points })),
        Some(serde_json::json!({
            "loyalty_points": after.loyalty_points,
            "points_delta": input.points_delta,
            "note": note,
        })),
        now,
    )
    .ipc()?;
    Ok(after)
}

#[cfg(test)]
mod tests;

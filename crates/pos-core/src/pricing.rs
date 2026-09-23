//! The pricing engine: catalogue prices × quantities → discounts → tax →
//! totals, in integer minor units only.
//!
//! This is the ONLY place totals are computed. The UI asks for a quote over
//! IPC (`quote_transaction`) instead of re-implementing it, and
//! `create_transaction` runs the same function on prices read from the
//! database — the UI never supplies a price.
//!
//! Order of operations
//! 1. `gross = unit_price × quantity` per line.
//! 2. Line-scoped discounts (product / category), capped at the line's gross.
//! 3. Order-scoped discounts on the remaining subtotal (if `min_subtotal` is
//!    met), spread across lines in proportion to their remaining value
//!    (largest remainder), so per-line tax stays exact.
//! 4. Tax per line on the discounted amount, extracted (tax-inclusive
//!    prices) or added (exclusive).

use serde::Serialize;
use uuid::Uuid;

use crate::money::{self, MinorUnits, MoneyError, RoundingMode};

const ROUNDING: RoundingMode = RoundingMode::HalfUp;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceLine {
    pub product_id: Uuid,
    pub category_id: Option<Uuid>,
    pub name: String,
    pub sku: Option<String>,
    pub unit_price: MinorUnits,
    pub quantity_milli: i64,
    pub tax_rate_bps: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscountValue {
    /// 0..=10 000 basis points.
    Percentage(i64),
    /// Minor units, once per matching line (line scope) or once per order.
    Fixed(MinorUnits),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscountScope {
    Order,
    Product(Uuid),
    Category(Uuid),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discount {
    pub id: Uuid,
    pub value: DiscountValue,
    pub scope: DiscountScope,
    pub min_subtotal: Option<MinorUnits>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PricedLine {
    pub product_id: Uuid,
    pub name: String,
    #[serde(skip)]
    pub sku: Option<String>,
    pub quantity_milli: i64,
    pub unit_price: MinorUnits,
    pub tax_rate_bps: i64,
    /// `unit_price × quantity`.
    pub gross: MinorUnits,
    pub discount_amount: MinorUnits,
    pub tax_amount: MinorUnits,
    /// What the customer pays for this line (net of discount, tax included).
    pub line_total: MinorUnits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaxLine {
    pub rate_bps: i64,
    pub taxable_amount: MinorUnits,
    pub tax_amount: MinorUnits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Quote {
    pub lines: Vec<PricedLine>,
    pub subtotal: MinorUnits,
    pub discount_total: MinorUnits,
    pub tax_total: MinorUnits,
    pub total: MinorUnits,
    pub tax_lines: Vec<TaxLine>,
    /// Discounts whose conditions were met.
    #[serde(skip)]
    pub applied_discounts: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PricingError {
    #[error("a sale needs at least one item")]
    Empty,
    #[error("invalid line for {0}: {1}")]
    InvalidLine(String, &'static str),
    #[error("invalid discount: {0}")]
    InvalidDiscount(&'static str),
    #[error(transparent)]
    Money(#[from] MoneyError),
}

fn line_matches(scope: DiscountScope, line: &PriceLine) -> bool {
    match scope {
        DiscountScope::Order => false,
        DiscountScope::Product(id) => line.product_id == id,
        DiscountScope::Category(id) => line.category_id == Some(id),
    }
}

fn discount_amount(value: DiscountValue, base: MinorUnits) -> Result<MinorUnits, PricingError> {
    let raw = match value {
        DiscountValue::Percentage(bps) => {
            if !(0..=10_000).contains(&bps) {
                return Err(PricingError::InvalidDiscount(
                    "percentage must be 0–10 000 bps",
                ));
            }
            money::apply_basis_points(base, bps, ROUNDING)?
        }
        DiscountValue::Fixed(amount) => {
            if amount < 0 {
                return Err(PricingError::InvalidDiscount(
                    "fixed amount must be positive",
                ));
            }
            amount
        }
    };
    Ok(raw.clamp(0, base.max(0)))
}

pub fn price(
    lines: &[PriceLine],
    discounts: &[Discount],
    prices_include_tax: bool,
) -> Result<Quote, PricingError> {
    if lines.is_empty() {
        return Err(PricingError::Empty);
    }
    for line in lines {
        if line.quantity_milli <= 0 {
            return Err(PricingError::InvalidLine(
                line.name.clone(),
                "quantity must be positive",
            ));
        }
        if line.unit_price < 0 {
            return Err(PricingError::InvalidLine(
                line.name.clone(),
                "price must not be negative",
            ));
        }
        if !(0..=10_000).contains(&line.tax_rate_bps) {
            return Err(PricingError::InvalidLine(
                line.name.clone(),
                "tax rate out of range",
            ));
        }
    }

    // 1. Gross.
    let gross: Vec<MinorUnits> = lines
        .iter()
        .map(|l| money::multiply_by_quantity(l.unit_price, l.quantity_milli, ROUNDING))
        .collect::<Result<_, _>>()?;
    let mut line_discount = vec![0; lines.len()];
    let mut applied = Vec::new();

    // 2. Line-scoped discounts.
    for discount in discounts.iter().filter(|d| d.scope != DiscountScope::Order) {
        let mut hit = false;
        for (i, line) in lines.iter().enumerate() {
            if line_matches(discount.scope, line) {
                let remaining = gross[i] - line_discount[i];
                line_discount[i] += discount_amount(discount.value, remaining)?;
                hit = true;
            }
        }
        if hit {
            applied.push(discount.id);
        }
    }

    // 3. Order-scoped discounts, allocated across lines.
    for discount in discounts.iter().filter(|d| d.scope == DiscountScope::Order) {
        let remaining: Vec<MinorUnits> = gross
            .iter()
            .zip(&line_discount)
            .map(|(g, d)| g - d)
            .collect();
        let remaining_total = money::add(&remaining)?;
        let pre_discount_subtotal = money::add(&gross)?;
        if discount
            .min_subtotal
            .is_some_and(|min| pre_discount_subtotal < min)
            || remaining_total <= 0
        {
            continue;
        }
        let amount = discount_amount(discount.value, remaining_total)?;
        if amount == 0 {
            continue;
        }
        let shares = money::allocate(amount, &remaining)?;
        for (i, share) in shares.into_iter().enumerate() {
            line_discount[i] += share;
        }
        applied.push(discount.id);
    }

    // 4. Tax.
    let mut priced = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let net = gross[i] - line_discount[i];
        let (tax, line_total) = if prices_include_tax {
            let split = money::extract_inclusive_tax(net, line.tax_rate_bps, ROUNDING)?;
            (split.tax, net)
        } else {
            let tax = money::apply_basis_points(net, line.tax_rate_bps, ROUNDING)?;
            (tax, money::add(&[net, tax])?)
        };
        priced.push(PricedLine {
            product_id: line.product_id,
            name: line.name.clone(),
            sku: line.sku.clone(),
            quantity_milli: line.quantity_milli,
            unit_price: line.unit_price,
            tax_rate_bps: line.tax_rate_bps,
            gross: gross[i],
            discount_amount: line_discount[i],
            tax_amount: tax,
            line_total,
        });
    }

    let mut tax_lines: Vec<TaxLine> = Vec::new();
    for line in &priced {
        if line.tax_rate_bps == 0 {
            continue;
        }
        let taxable = line.line_total - line.tax_amount;
        match tax_lines
            .iter_mut()
            .find(|t| t.rate_bps == line.tax_rate_bps)
        {
            Some(t) => {
                t.taxable_amount += taxable;
                t.tax_amount += line.tax_amount;
            }
            None => tax_lines.push(TaxLine {
                rate_bps: line.tax_rate_bps,
                taxable_amount: taxable,
                tax_amount: line.tax_amount,
            }),
        }
    }
    tax_lines.sort_by_key(|t| t.rate_bps);

    let sum =
        |f: fn(&PricedLine) -> MinorUnits| money::add(&priced.iter().map(f).collect::<Vec<_>>());
    Ok(Quote {
        subtotal: sum(|l| l.gross)?,
        discount_total: sum(|l| l.discount_amount)?,
        tax_total: sum(|l| l.tax_amount)?,
        total: sum(|l| l.line_total)?,
        lines: priced,
        tax_lines,
        applied_discounts: applied,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(n: u128, price: i64, qty: i64, tax: i64) -> PriceLine {
        PriceLine {
            product_id: Uuid::from_u128(n),
            category_id: Some(Uuid::from_u128(100)),
            name: format!("item {n}"),
            sku: None,
            unit_price: price,
            quantity_milli: qty,
            tax_rate_bps: tax,
        }
    }

    fn invariants(q: &Quote) {
        assert_eq!(q.total, q.lines.iter().map(|l| l.line_total).sum::<i64>());
        assert_eq!(
            q.discount_total,
            q.lines.iter().map(|l| l.discount_amount).sum::<i64>()
        );
        for l in &q.lines {
            assert!(l.discount_amount <= l.gross && l.discount_amount >= 0);
        }
    }

    #[test]
    fn inclusive_tax_is_extracted_not_added() {
        let q = price(&[line(1, 1150, 2000, 1500)], &[], true).expect("quote");
        assert_eq!((q.subtotal, q.tax_total, q.total), (2300, 300, 2300));
        assert_eq!(
            q.tax_lines,
            vec![TaxLine {
                rate_bps: 1500,
                taxable_amount: 2000,
                tax_amount: 300
            }]
        );
        invariants(&q);
    }

    #[test]
    fn exclusive_tax_is_added() {
        let q = price(
            &[line(1, 1000, 1000, 500), line(2, 250, 3000, 0)],
            &[],
            false,
        )
        .expect("quote");
        assert_eq!((q.subtotal, q.tax_total, q.total), (1750, 50, 1800));
        invariants(&q);
    }

    #[test]
    fn weighed_quantities_round_half_up() {
        // 1.999 KWD/kg × 0.250 kg = 0.49975 → 0.500
        let q = price(&[line(1, 1999, 250, 0)], &[], true).expect("quote");
        assert_eq!(q.total, 500);
    }

    #[test]
    fn order_discount_is_allocated_exactly() {
        let discount = Discount {
            id: Uuid::from_u128(9),
            value: DiscountValue::Fixed(100),
            scope: DiscountScope::Order,
            min_subtotal: None,
        };
        let q = price(
            &[
                line(1, 100, 1000, 0),
                line(2, 100, 1000, 0),
                line(3, 100, 1000, 0),
            ],
            &[discount],
            true,
        )
        .expect("quote");
        assert_eq!(q.discount_total, 100);
        assert_eq!(
            q.lines
                .iter()
                .map(|l| l.discount_amount)
                .collect::<Vec<_>>(),
            vec![34, 33, 33]
        );
        assert_eq!(q.total, 200);
        invariants(&q);
    }

    #[test]
    fn line_discounts_then_order_discounts_never_exceed_gross() {
        let q = price(
            &[line(1, 500, 1000, 1500), line(2, 300, 2000, 1500)],
            &[
                Discount {
                    id: Uuid::from_u128(1),
                    value: DiscountValue::Percentage(10_000),
                    scope: DiscountScope::Product(Uuid::from_u128(1)),
                    min_subtotal: None,
                },
                Discount {
                    id: Uuid::from_u128(2),
                    value: DiscountValue::Fixed(10_000),
                    scope: DiscountScope::Order,
                    min_subtotal: None,
                },
            ],
            true,
        )
        .expect("quote");
        assert_eq!(q.total, 0);
        assert_eq!(q.tax_total, 0);
        invariants(&q);
    }

    #[test]
    fn min_subtotal_gates_order_discounts() {
        let discount = Discount {
            id: Uuid::from_u128(9),
            value: DiscountValue::Percentage(1000),
            scope: DiscountScope::Order,
            min_subtotal: Some(5000),
        };
        let q = price(
            &[line(1, 1000, 1000, 0)],
            std::slice::from_ref(&discount),
            true,
        )
        .expect("quote");
        assert_eq!(q.discount_total, 0);
        assert!(q.applied_discounts.is_empty());
        let q = price(&[line(1, 1000, 5000, 0)], &[discount], true).expect("quote");
        assert_eq!(q.discount_total, 500);
    }

    #[test]
    fn category_discount_hits_matching_lines_only() {
        let mut other = line(2, 1000, 1000, 0);
        other.category_id = None;
        let q = price(
            &[line(1, 1000, 1000, 0), other],
            &[Discount {
                id: Uuid::from_u128(5),
                value: DiscountValue::Percentage(2500),
                scope: DiscountScope::Category(Uuid::from_u128(100)),
                min_subtotal: None,
            }],
            true,
        )
        .expect("quote");
        assert_eq!(q.lines[0].discount_amount, 250);
        assert_eq!(q.lines[1].discount_amount, 0);
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(price(&[], &[], true), Err(PricingError::Empty));
        assert!(price(&[line(1, 100, 0, 0)], &[], true).is_err());
        assert!(price(&[line(1, -1, 1000, 0)], &[], true).is_err());
    }
}

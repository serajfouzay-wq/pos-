//! Loyalty points: earning on what was paid, redeeming as an order
//! discount, and giving back the right share on refunds. Integer only.
//!
//! Mirrors `LoyaltySettingsSchema` in `@pos/shared`.

use serde::{Deserialize, Serialize};

use crate::currency::CurrencyCode;
use crate::money::{self, MinorUnits, RoundingMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoyaltySettings {
    pub enabled: bool,
    /// Points per whole unit of the base currency paid.
    pub points_per_unit: i64,
    /// Minor units one redeemed point takes off the bill.
    pub point_value: i64,
    /// Fewer points than this cannot be redeemed.
    pub min_redeem_points: i64,
    /// Share of the bill (before tax) points may pay, in basis points.
    pub max_redeem_bps: i64,
}

impl Default for LoyaltySettings {
    /// 1 point per unit spent; 100 points = 1 unit off (for a 3-decimal
    /// currency 1 point = 10 fils), from 100 points, up to the whole bill.
    fn default() -> Self {
        Self {
            enabled: true,
            points_per_unit: 1,
            point_value: 10,
            min_redeem_points: 100,
            max_redeem_bps: 10_000,
        }
    }
}

impl LoyaltySettings {
    /// Defaults scaled to the currency: 100 points are one unit off.
    pub fn default_for(currency: CurrencyCode) -> Self {
        let unit = 10_i64.pow(currency.exponent());
        Self {
            point_value: (unit / 100).max(1),
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if !(0..=1000).contains(&self.points_per_unit) {
            return Err("Points per unit must be between 0 and 1000.");
        }
        if !(1..=1_000_000).contains(&self.point_value) {
            return Err("A point must be worth between 1 and 1 000 000 minor units.");
        }
        if !(0..=1_000_000).contains(&self.min_redeem_points) {
            return Err("The minimum redemption must be between 0 and 1 000 000 points.");
        }
        if !(0..=10_000).contains(&self.max_redeem_bps) {
            return Err("The share points may pay must be between 0 and 100%.");
        }
        Ok(())
    }

    /// Points earned for paying `total` (floor: partial points are not given).
    pub fn earned(&self, total: MinorUnits, currency: CurrencyCode) -> i64 {
        if !self.enabled || total <= 0 {
            return 0;
        }
        let unit = 10_i128.pow(currency.exponent());
        let points = i128::from(total) * i128::from(self.points_per_unit) / unit;
        i64::try_from(points).unwrap_or(i64::MAX)
    }

    /// The most points a bill with `payable` left to pay (after the other
    /// discounts, before tax) can take for a customer holding `balance`.
    pub fn max_redeemable(&self, balance: i64, payable: MinorUnits) -> i64 {
        if !self.enabled || balance < self.min_redeem_points.max(1) || payable <= 0 {
            return 0;
        }
        let capped =
            money::apply_basis_points(payable, self.max_redeem_bps, RoundingMode::TowardZero)
                .unwrap_or(0);
        let by_bill = capped / self.point_value;
        let points = balance.min(by_bill);
        if points < self.min_redeem_points {
            0
        } else {
            points
        }
    }

    /// Minor units `points` take off the bill.
    pub fn value_of(&self, points: i64) -> MinorUnits {
        points.saturating_mul(self.point_value)
    }
}

/// `round_half_up(total × part / whole)` in i128; `whole > 0`.
fn share(total: i64, part: i64, whole: i64) -> i64 {
    if whole <= 0 {
        return 0;
    }
    let value = money::div_round(
        i128::from(total) * i128::from(part),
        i128::from(whole),
        RoundingMode::HalfUp,
    )
    .unwrap_or(0);
    i64::try_from(value).unwrap_or(0)
}

/// Points to reverse when `amount` more of a sale worth `sale_total` comes
/// back, after `before` already came back: cumulative shares, so several
/// partial refunds add up to exactly `points`. The till measures it on the
/// goods before discounts, which a redemption cannot bring to zero.
pub fn reversal(
    points: i64,
    sale_total: MinorUnits,
    before: MinorUnits,
    amount: MinorUnits,
) -> i64 {
    let after = (before + amount).min(sale_total);
    share(points, after, sale_total) - share(points, before.min(sale_total), sale_total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earns_whole_points_on_what_was_paid() {
        let rules = LoyaltySettings::default_for(CurrencyCode::KWD);
        assert_eq!(rules.point_value, 10);
        assert_eq!(rules.earned(12_750, CurrencyCode::KWD), 12);
        assert_eq!(rules.earned(999, CurrencyCode::KWD), 0);
        let usd = LoyaltySettings::default_for(CurrencyCode::USD);
        assert_eq!(usd.point_value, 1);
        assert_eq!(usd.earned(1_999, CurrencyCode::USD), 19);
        let off = LoyaltySettings {
            enabled: false,
            ..rules
        };
        assert_eq!(off.earned(12_750, CurrencyCode::KWD), 0);
    }

    #[test]
    fn redemption_respects_balance_bill_share_and_minimum() {
        let rules = LoyaltySettings::default_for(CurrencyCode::KWD);
        // 3.000 KWD bill: at most 300 points.
        assert_eq!(rules.max_redeemable(1_000, 3_000), 300);
        assert_eq!(rules.max_redeemable(250, 3_000), 250);
        assert_eq!(rules.max_redeemable(99, 3_000), 0, "below the minimum");
        let half = LoyaltySettings {
            max_redeem_bps: 5_000,
            ..rules
        };
        assert_eq!(half.max_redeemable(1_000, 3_000), 150);
        // A bill too small for the minimum.
        assert_eq!(rules.max_redeemable(1_000, 500), 0);
        assert_eq!(rules.value_of(300), 3_000);
    }

    #[test]
    fn partial_reversals_add_up_exactly() {
        // 7 points on a 3.000 sale, refunded in three goes of 1.000.
        let parts: Vec<i64> = (0..3)
            .map(|i| reversal(7, 3_000, i * 1_000, 1_000))
            .collect();
        assert_eq!(parts.iter().sum::<i64>(), 7);
        assert_eq!(reversal(7, 3_000, 0, 3_000), 7);
        assert_eq!(reversal(0, 3_000, 0, 1_000), 0);
    }

    #[test]
    fn rejects_out_of_range_settings() {
        let bad = LoyaltySettings {
            point_value: 0,
            ..LoyaltySettings::default()
        };
        assert!(bad.validate().is_err());
        assert!(LoyaltySettings::default().validate().is_ok());
    }
}

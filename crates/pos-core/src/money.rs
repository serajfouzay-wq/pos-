//! Integer money arithmetic — the authoritative implementation.
//!
//! Amounts are `i64` counts of a currency's minor unit. Intermediate products
//! use `i128`, and every result is checked to fit the JavaScript safe-integer
//! range so it survives the trip over IPC to the UI unchanged.
//!
//! Pinned to `packages/shared/contracts/money-vectors.json` together with the
//! TypeScript twin in `@pos/shared`.

use serde::{Deserialize, Serialize};

use crate::currency::{CurrencyCode, ExchangeRate};

/// An integer amount in the minor unit of some currency.
pub type MinorUnits = i64;

/// 2^53 − 1: the largest integer a JS `number` represents exactly.
pub const JS_MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
/// Quantities are thousandths of a unit: 1000 = 1 unit.
pub const QUANTITY_SCALE: i64 = 1_000;
/// 10 000 basis points = 100 %.
pub const BPS_SCALE: i64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoundingMode {
    /// Ties round away from zero.
    HalfUp,
    /// Ties round to the even neighbour (banker's rounding).
    HalfEven,
    /// Truncate.
    TowardZero,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MoneyError {
    #[error("division by zero")]
    DivisionByZero,
    #[error("{0} overflowed the safe integer range")]
    Overflow(&'static str),
    #[error("{0}")]
    Invalid(String),
}

pub type MoneyResult<T> = Result<T, MoneyError>;

fn to_safe(value: i128, label: &'static str) -> MoneyResult<MinorUnits> {
    if value > i128::from(JS_MAX_SAFE_INTEGER) || value < -i128::from(JS_MAX_SAFE_INTEGER) {
        return Err(MoneyError::Overflow(label));
    }
    i64::try_from(value).map_err(|_| MoneyError::Overflow(label))
}

fn checked(value: Option<i128>, label: &'static str) -> MoneyResult<i128> {
    value.ok_or(MoneyError::Overflow(label))
}

/// Integer division with an explicit rounding mode.
pub fn div_round(numerator: i128, denominator: i128, mode: RoundingMode) -> MoneyResult<i128> {
    if denominator == 0 {
        return Err(MoneyError::DivisionByZero);
    }
    let (n, d) = if denominator < 0 {
        (
            checked(numerator.checked_neg(), "numerator")?,
            checked(denominator.checked_neg(), "denominator")?,
        )
    } else {
        (numerator, denominator)
    };
    let quotient = n / d; // truncates toward zero
    let remainder = n % d;
    if remainder == 0 || mode == RoundingMode::TowardZero {
        return Ok(quotient);
    }
    let step = if n < 0 { -1 } else { 1 };
    let twice_remainder = checked(remainder.abs().checked_mul(2), "remainder")?;
    Ok(match twice_remainder.cmp(&d) {
        std::cmp::Ordering::Greater => quotient + step,
        std::cmp::Ordering::Less => quotient,
        std::cmp::Ordering::Equal => match mode {
            RoundingMode::HalfUp => quotient + step,
            RoundingMode::HalfEven if quotient % 2 == 0 => quotient,
            _ => quotient + step,
        },
    })
}

pub fn add(amounts: &[MinorUnits]) -> MoneyResult<MinorUnits> {
    let total = amounts.iter().map(|a| i128::from(*a)).sum::<i128>();
    to_safe(total, "sum")
}

/// `unit_price × quantity_milli / 1000`.
pub fn multiply_by_quantity(
    unit_price: MinorUnits,
    quantity_milli: i64,
    mode: RoundingMode,
) -> MoneyResult<MinorUnits> {
    let product = i128::from(unit_price) * i128::from(quantity_milli);
    to_safe(
        div_round(product, i128::from(QUANTITY_SCALE), mode)?,
        "line total",
    )
}

/// `amount × bps / 10 000` — percentage discounts, exclusive tax, loyalty earn.
pub fn apply_basis_points(
    amount: MinorUnits,
    bps: i64,
    mode: RoundingMode,
) -> MoneyResult<MinorUnits> {
    let product = i128::from(amount) * i128::from(bps);
    to_safe(
        div_round(product, i128::from(BPS_SCALE), mode)?,
        "percentage",
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TaxSplit {
    pub net: MinorUnits,
    pub tax: MinorUnits,
}

/// Splits a tax-inclusive gross amount; `net + tax == gross` always holds.
pub fn extract_inclusive_tax(
    gross: MinorUnits,
    rate_bps: i64,
    mode: RoundingMode,
) -> MoneyResult<TaxSplit> {
    if rate_bps < 0 {
        return Err(MoneyError::Invalid("tax rate must be non-negative".into()));
    }
    let g = i128::from(gross);
    let scale = i128::from(BPS_SCALE);
    let net = div_round(g * scale, scale + i128::from(rate_bps), mode)?;
    Ok(TaxSplit {
        net: to_safe(net, "net")?,
        tax: to_safe(g - net, "tax")?,
    })
}

/// Splits `amount` across `weights` so the parts sum exactly to `amount`
/// (largest remainder; ties go to the earliest index).
pub fn allocate(amount: MinorUnits, weights: &[i64]) -> MoneyResult<Vec<MinorUnits>> {
    if weights.is_empty() {
        return Err(MoneyError::Invalid(
            "allocate needs at least one weight".into(),
        ));
    }
    if weights.iter().any(|w| *w < 0) {
        return Err(MoneyError::Invalid("weights must be non-negative".into()));
    }
    let weight_sum: i128 = weights.iter().map(|w| i128::from(*w)).sum();
    if weight_sum == 0 {
        return Err(MoneyError::Invalid("weights must not all be zero".into()));
    }

    let sign: i128 = if amount < 0 { -1 } else { 1 };
    let magnitude = i128::from(amount).abs();
    let mut parts: Vec<i128> = weights
        .iter()
        .map(|w| magnitude * i128::from(*w) / weight_sum)
        .collect();
    let mut remainders: Vec<(usize, i128)> = weights
        .iter()
        .enumerate()
        .map(|(i, w)| (i, magnitude * i128::from(*w) % weight_sum))
        .collect();
    let mut leftover = magnitude - parts.iter().sum::<i128>();
    remainders.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for (index, _) in remainders {
        if leftover == 0 {
            break;
        }
        parts[index] += 1;
        leftover -= 1;
    }
    parts
        .into_iter()
        .map(|p| to_safe(p * sign, "allocation"))
        .collect()
}

/// Converts minor units of `rate.base` into minor units of `rate.quote`.
pub fn convert_currency(
    amount: MinorUnits,
    rate: &ExchangeRate,
    mode: RoundingMode,
) -> MoneyResult<MinorUnits> {
    if rate.numerator == 0 || rate.denominator == 0 {
        return Err(MoneyError::Invalid("exchange rate must be positive".into()));
    }
    let to_scale = 10_i128.pow(rate.quote.exponent());
    let from_scale = 10_i128.pow(rate.base.exponent());
    let numerator = checked(
        i128::from(amount)
            .checked_mul(i128::from(rate.numerator))
            .and_then(|v| v.checked_mul(to_scale)),
        "converted amount",
    )?;
    let denominator = checked(
        i128::from(rate.denominator).checked_mul(from_scale),
        "converted amount",
    )?;
    to_safe(div_round(numerator, denominator, mode)?, "converted amount")
}

/// `1500, KWD` → `"1.500"`. Exact, no floats.
pub fn to_decimal_string(amount: MinorUnits, currency: CurrencyCode) -> String {
    let exponent = currency.exponent() as usize;
    let digits = amount.unsigned_abs().to_string();
    let sign = if amount < 0 { "-" } else { "" };
    if exponent == 0 {
        return format!("{sign}{digits}");
    }
    let padded = format!("{digits:0>width$}", width = exponent + 1);
    let (major, minor) = padded.split_at(padded.len() - exponent);
    format!("{sign}{major}.{minor}")
}

/// Parses `"1.5"` into minor units (`1500` for KWD). Rejects excess precision.
pub fn parse_decimal_string(input: &str, currency: CurrencyCode) -> MoneyResult<MinorUnits> {
    let invalid = || MoneyError::Invalid(format!("not a decimal amount: \"{input}\""));
    let exponent = currency.exponent() as usize;
    let trimmed = input.trim();
    let (negative, unsigned) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed),
    };
    let (major, fraction) = match unsigned.split_once('.') {
        Some((major, fraction)) => (major, fraction),
        None => (unsigned, ""),
    };
    let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if major.is_empty() || !all_digits(major) || !all_digits(fraction) {
        return Err(invalid());
    }
    if fraction.len() > exponent {
        return Err(MoneyError::Invalid(format!(
            "{} allows at most {exponent} decimal places",
            currency.as_str()
        )));
    }
    let combined = format!("{major}{fraction:0<exponent$}");
    let magnitude: i128 = combined.parse().map_err(|_| invalid())?;
    to_safe(
        if negative { -magnitude } else { magnitude },
        "parsed amount",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_is_detected() {
        assert_eq!(
            multiply_by_quantity(JS_MAX_SAFE_INTEGER, 2_000, RoundingMode::HalfUp),
            Err(MoneyError::Overflow("line total"))
        );
    }

    #[test]
    fn division_by_zero_is_an_error() {
        assert_eq!(
            div_round(1, 0, RoundingMode::HalfUp),
            Err(MoneyError::DivisionByZero)
        );
    }
}

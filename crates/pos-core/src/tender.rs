//! Settling a total against the customer's tenders.
//!
//! Rules: card/wallet tenders can never exceed what is still owed (no change
//! is given on a card), cash covers the rest, and change comes only out of
//! cash. The applied amounts always sum exactly to the total.

use serde::Serialize;

use crate::money::MinorUnits;
use crate::sales::PaymentMethod;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tender {
    pub method: PaymentMethod,
    /// Base-currency minor units handed over.
    pub tendered: MinorUnits,
    pub reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppliedTender {
    pub method: PaymentMethod,
    /// Portion of the sale this tender paid.
    pub amount: MinorUnits,
    pub tendered: MinorUnits,
    pub change_given: MinorUnits,
    #[serde(skip)]
    pub reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settlement {
    pub tenders: Vec<AppliedTender>,
    pub change_due: MinorUnits,
}

impl Settlement {
    pub fn includes_cash(&self) -> bool {
        self.tenders.iter().any(|t| t.method == PaymentMethod::Cash)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TenderError {
    #[error("add a payment")]
    NoTenders,
    #[error("{0} payments are not supported yet")]
    Unsupported(&'static str),
    #[error("payment amounts must not be negative")]
    Negative,
    #[error("card and wallet payments cannot exceed the amount due (no change is given on them)")]
    NonCashOverpayment,
    #[error("payments are short by {0} (minor units)")]
    Short(MinorUnits),
}

pub fn settle(total: MinorUnits, tenders: &[Tender]) -> Result<Settlement, TenderError> {
    if tenders.is_empty() {
        return Err(TenderError::NoTenders);
    }
    for t in tenders {
        match t.method {
            PaymentMethod::Loyalty => return Err(TenderError::Unsupported("loyalty")),
            PaymentMethod::Voucher => return Err(TenderError::Unsupported("voucher")),
            PaymentMethod::Cash | PaymentMethod::Card | PaymentMethod::Wallet => {}
        }
        if t.tendered < 0 {
            return Err(TenderError::Negative);
        }
    }
    let due = total.max(0);
    let non_cash: i128 = tenders
        .iter()
        .filter(|t| t.method != PaymentMethod::Cash)
        .map(|t| i128::from(t.tendered))
        .sum();
    if non_cash > i128::from(due) {
        return Err(TenderError::NonCashOverpayment);
    }
    let all: i128 = tenders.iter().map(|t| i128::from(t.tendered)).sum();
    if all < i128::from(due) {
        let short = i64::try_from(i128::from(due) - all).unwrap_or(i64::MAX);
        return Err(TenderError::Short(short));
    }
    // Bounded by the sum of i64 tenders and ≥ 0 here.
    let mut change = i64::try_from(all - i128::from(due)).unwrap_or(i64::MAX);
    let change_due = change;

    // Change comes out of the cash tenders, last one first.
    let mut applied: Vec<AppliedTender> = tenders
        .iter()
        .map(|t| AppliedTender {
            method: t.method,
            amount: t.tendered,
            tendered: t.tendered,
            change_given: 0,
            reference: t.reference.clone(),
        })
        .collect();
    for t in applied
        .iter_mut()
        .rev()
        .filter(|t| t.method == PaymentMethod::Cash)
    {
        let from_this = change.min(t.amount);
        t.amount -= from_this;
        t.change_given = from_this;
        change -= from_this;
        if change == 0 {
            break;
        }
    }
    Ok(Settlement {
        tenders: applied,
        change_due,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cash(n: i64) -> Tender {
        Tender {
            method: PaymentMethod::Cash,
            tendered: n,
            reference: None,
        }
    }
    fn card(n: i64) -> Tender {
        Tender {
            method: PaymentMethod::Card,
            tendered: n,
            reference: None,
        }
    }

    #[test]
    fn cash_gives_change() {
        let s = settle(1_750, &[cash(5_000)]).expect("settles");
        assert_eq!(s.change_due, 3_250);
        assert_eq!(s.tenders[0].amount, 1_750);
        assert_eq!(s.tenders[0].change_given, 3_250);
    }

    #[test]
    fn split_tender_card_then_cash() {
        let s = settle(10_000, &[card(6_000), cash(5_000)]).expect("settles");
        assert_eq!(s.change_due, 1_000);
        assert_eq!(s.tenders.iter().map(|t| t.amount).sum::<i64>(), 10_000);
    }

    #[test]
    fn card_cannot_overpay_and_short_payments_fail() {
        assert_eq!(
            settle(1_000, &[card(1_001)]),
            Err(TenderError::NonCashOverpayment)
        );
        assert_eq!(settle(1_000, &[cash(999)]), Err(TenderError::Short(1)));
        assert_eq!(settle(1_000, &[]), Err(TenderError::NoTenders));
    }

    #[test]
    fn exact_card_payment() {
        let s = settle(1_000, &[card(1_000)]).expect("settles");
        assert_eq!((s.change_due, s.includes_cash()), (0, false));
    }
}

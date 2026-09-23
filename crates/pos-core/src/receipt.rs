//! Receipt data (mirrors `ReceiptSchema`). Built from the stored, immutable
//! transaction — never from anything the UI sent — then rendered to ESC/POS
//! by `pos-hardware`.

use serde::Serialize;
use uuid::Uuid;

use crate::currency::CurrencyCode;
use crate::money::MinorUnits;
use crate::pricing::TaxLine;
use crate::sales::{PaymentMethod, TransactionKind};
use crate::time::Timestamp;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModifierLine {
    pub modifier_id: Option<Uuid>,
    pub name: String,
    pub price_delta: MinorUnits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReceiptLine {
    pub name: String,
    pub quantity_milli: i64,
    pub unit_price: MinorUnits,
    pub modifiers: Vec<ModifierLine>,
    pub discount_amount: MinorUnits,
    pub line_total: MinorUnits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReceiptPayment {
    pub method: PaymentMethod,
    pub amount: MinorUnits,
    pub tendered_currency: CurrencyCode,
    pub tendered_amount: MinorUnits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoyaltySummary {
    pub earned: i64,
    pub redeemed: i64,
    pub balance: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Receipt {
    pub transaction_id: Uuid,
    pub kind: TransactionKind,
    pub receipt_number: String,
    pub issued_at: Timestamp,
    pub cashier_name: String,
    pub customer_name: Option<String>,
    pub currency: CurrencyCode,
    pub lines: Vec<ReceiptLine>,
    pub subtotal: MinorUnits,
    pub discount_total: MinorUnits,
    pub tax_lines: Vec<TaxLine>,
    pub total: MinorUnits,
    pub payments: Vec<ReceiptPayment>,
    pub change_due: MinorUnits,
    pub loyalty: Option<LoyaltySummary>,
    /// `false` when the printer was unreachable and the job is queued.
    pub printed: bool,
}

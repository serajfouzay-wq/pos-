//! Sales vocabulary shared by pricing, tendering, receipts and persistence.
//! Serialized names match `@pos/shared` (`PaymentMethodSchema`, …).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentMethod {
    Cash,
    Card,
    Wallet,
    Loyalty,
    Voucher,
}

impl PaymentMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            PaymentMethod::Cash => "cash",
            PaymentMethod::Card => "card",
            PaymentMethod::Wallet => "wallet",
            PaymentMethod::Loyalty => "loyalty",
            PaymentMethod::Voucher => "voucher",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionKind {
    Sale,
    Refund,
    Void,
}

impl TransactionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TransactionKind::Sale => "sale",
            TransactionKind::Refund => "refund",
            TransactionKind::Void => "void",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    Counter,
    DineIn,
    Takeaway,
    Delivery,
}

impl OrderType {
    pub fn as_str(self) -> &'static str {
        match self {
            OrderType::Counter => "counter",
            OrderType::DineIn => "dine_in",
            OrderType::Takeaway => "takeaway",
            OrderType::Delivery => "delivery",
        }
    }
}

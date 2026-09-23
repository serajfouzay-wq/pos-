//! Supported currencies (ISO 4217) and their minor-unit exponents.

use serde::{Deserialize, Serialize};

/// Variants are the ISO 4217 codes verbatim so serde round-trips them unchanged.
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CurrencyCode {
    AED,
    BHD,
    EGP,
    EUR,
    GBP,
    IQD,
    JOD,
    JPY,
    KWD,
    MYR,
    OMR,
    QAR,
    SAR,
    TND,
    USD,
}

impl CurrencyCode {
    pub const ALL: [CurrencyCode; 15] = [
        CurrencyCode::AED,
        CurrencyCode::BHD,
        CurrencyCode::EGP,
        CurrencyCode::EUR,
        CurrencyCode::GBP,
        CurrencyCode::IQD,
        CurrencyCode::JOD,
        CurrencyCode::JPY,
        CurrencyCode::KWD,
        CurrencyCode::MYR,
        CurrencyCode::OMR,
        CurrencyCode::QAR,
        CurrencyCode::SAR,
        CurrencyCode::TND,
        CurrencyCode::USD,
    ];

    /// Number of minor-unit digits: 1.500 KWD = 1500 fils → 3.
    pub const fn exponent(self) -> u32 {
        use CurrencyCode::*;
        match self {
            JPY => 0,
            AED | EGP | EUR | GBP | MYR | QAR | SAR | USD => 2,
            BHD | IQD | JOD | KWD | OMR | TND => 3,
        }
    }

    pub fn as_str(self) -> &'static str {
        use CurrencyCode::*;
        match self {
            AED => "AED",
            BHD => "BHD",
            EGP => "EGP",
            EUR => "EUR",
            GBP => "GBP",
            IQD => "IQD",
            JOD => "JOD",
            JPY => "JPY",
            KWD => "KWD",
            MYR => "MYR",
            OMR => "OMR",
            QAR => "QAR",
            SAR => "SAR",
            TND => "TND",
            USD => "USD",
        }
    }
}

/// Exact rate: 1 major unit of `base` = `numerator / denominator` major units of `quote`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeRate {
    pub base: CurrencyCode,
    pub quote: CurrencyCode,
    pub numerator: u64,
    pub denominator: u64,
}

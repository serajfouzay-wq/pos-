//! Supported currencies (ISO 4217) and their minor-unit exponents.

use serde::{Deserialize, Serialize};

/// Variants are the ISO 4217 codes verbatim so serde round-trips them unchanged.
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CurrencyCode {
    AED,
    BHD,
    DZD,
    EGP,
    EUR,
    GBP,
    IQD,
    JOD,
    JPY,
    KWD,
    LYD,
    MAD,
    MYR,
    OMR,
    QAR,
    SAR,
    TND,
    USD,
}

impl CurrencyCode {
    pub const ALL: [CurrencyCode; 18] = [
        CurrencyCode::AED,
        CurrencyCode::BHD,
        CurrencyCode::DZD,
        CurrencyCode::EGP,
        CurrencyCode::EUR,
        CurrencyCode::GBP,
        CurrencyCode::IQD,
        CurrencyCode::JOD,
        CurrencyCode::JPY,
        CurrencyCode::KWD,
        CurrencyCode::LYD,
        CurrencyCode::MAD,
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
            AED | DZD | EGP | EUR | GBP | MAD | MYR | QAR | SAR | USD => 2,
            BHD | IQD | JOD | KWD | LYD | OMR | TND => 3,
        }
    }

    pub fn as_str(self) -> &'static str {
        use CurrencyCode::*;
        match self {
            AED => "AED",
            BHD => "BHD",
            DZD => "DZD",
            EGP => "EGP",
            EUR => "EUR",
            GBP => "GBP",
            IQD => "IQD",
            JOD => "JOD",
            JPY => "JPY",
            KWD => "KWD",
            LYD => "LYD",
            MAD => "MAD",
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

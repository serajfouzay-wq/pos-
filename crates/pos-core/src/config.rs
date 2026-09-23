//! Per-client build configuration, embedded into the POS binary at compile time.
//!
//! Mirrors `ClientConfigSchema` in `@pos/shared`. [`ClientConfig::parse`] is
//! called from the POS client's `build.rs`, so an invalid config fails the
//! build rather than a shop's first boot.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::currency::CurrencyCode;

pub const CLIENT_CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusinessType {
    Retail,
    Cafe,
    Restaurant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Locale {
    En,
    Ar,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocaleConfig {
    pub default: Locale,
    pub supported: Vec<Locale>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurrencyConfig {
    pub base: CurrencyCode,
    pub accepted: Vec<CurrencyCode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxConfig {
    pub registration_number: Option<String>,
    pub prices_include_tax: bool,
    pub default_rate_bps: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptLayout {
    pub logo_asset: Option<String>,
    pub header_lines: Vec<String>,
    pub footer_text: String,
    pub show_tax_number: bool,
    pub paper_width_mm: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Branding {
    pub primary_color: String,
    pub accent_color: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Features {
    pub loyalty: bool,
    pub kitchen_display: bool,
    pub multi_currency: bool,
    pub purchase_orders: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientConfig {
    pub schema_version: u32,
    pub client_id: Uuid,
    pub client_slug: String,
    pub display_name: String,
    pub business_type: BusinessType,
    pub locale: LocaleConfig,
    pub currency: CurrencyConfig,
    pub tax: TaxConfig,
    pub receipt: ReceiptLayout,
    pub branding: Branding,
    pub features: Features,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("client config is not valid JSON for the schema: {0}")]
    Parse(String),
    #[error("client config field `{field}`: {message}")]
    Invalid {
        field: &'static str,
        message: String,
    },
}

fn invalid(field: &'static str, message: impl Into<String>) -> ConfigError {
    ConfigError::Invalid {
        field,
        message: message.into(),
    }
}

fn is_kebab_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 40
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s.bytes().skip(1).all(|b| b.is_ascii_hexdigit())
}

impl ClientConfig {
    pub fn parse(json: &str) -> Result<Self, ConfigError> {
        let config: Self =
            serde_json::from_str(json).map_err(|e| ConfigError::Parse(e.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    /// Same rules as the Zod schema (string lengths count UTF-16-agnostic chars).
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != CLIENT_CONFIG_SCHEMA_VERSION {
            return Err(invalid(
                "schema_version",
                format!("expected {CLIENT_CONFIG_SCHEMA_VERSION}"),
            ));
        }
        if !is_kebab_slug(&self.client_slug) {
            return Err(invalid("client_slug", "kebab-case, at most 40 chars"));
        }
        let name_len = self.display_name.chars().count();
        if name_len == 0 || name_len > 80 {
            return Err(invalid("display_name", "1–80 characters"));
        }
        if self.locale.supported.is_empty() {
            return Err(invalid("locale.supported", "at least one locale"));
        }
        if !self.locale.supported.contains(&self.locale.default) {
            return Err(invalid("locale.default", "must be one of locale.supported"));
        }
        if self.currency.accepted.contains(&self.currency.base) {
            return Err(invalid(
                "currency.accepted",
                "must not repeat the base currency",
            ));
        }
        if self.tax.default_rate_bps > 10_000 {
            return Err(invalid("tax.default_rate_bps", "at most 10 000 bps"));
        }
        if self
            .tax
            .registration_number
            .as_ref()
            .is_some_and(|n| n.chars().count() > 40)
        {
            return Err(invalid("tax.registration_number", "at most 40 characters"));
        }
        if self.receipt.header_lines.len() > 6
            || self
                .receipt
                .header_lines
                .iter()
                .any(|l| l.chars().count() > 48)
        {
            return Err(invalid(
                "receipt.header_lines",
                "at most 6 lines of 48 characters",
            ));
        }
        if self.receipt.footer_text.chars().count() > 240 {
            return Err(invalid("receipt.footer_text", "at most 240 characters"));
        }
        if self
            .receipt
            .logo_asset
            .as_ref()
            .is_some_and(String::is_empty)
        {
            return Err(invalid("receipt.logo_asset", "must be null or non-empty"));
        }
        if !matches!(self.receipt.paper_width_mm, 58 | 80) {
            return Err(invalid("receipt.paper_width_mm", "must be 58 or 80"));
        }
        if !is_hex_color(&self.branding.primary_color) {
            return Err(invalid("branding.primary_color", "expected #RRGGBB"));
        }
        if !is_hex_color(&self.branding.accent_color) {
            return Err(invalid("branding.accent_color", "expected #RRGGBB"));
        }
        Ok(())
    }
}

//! Client-level rules of the generator: defaults for a new client, what an
//! uploaded asset must look like, and the receipt preview.

use base64::Engine;
use pos_core::config::{
    Branding, BusinessType, ClientConfig, CloudConfig, CurrencyConfig, Features, Locale,
    LocaleConfig, ReceiptLayout, TaxConfig, CLIENT_CONFIG_SCHEMA_VERSION,
};
use pos_core::currency::CurrencyCode;
use pos_core::pricing::{price, PriceLine};
use pos_core::receipt::{Receipt, ReceiptLine, ReceiptPayment};
use pos_core::sales::{PaymentMethod, TransactionKind};
use pos_core::time::Timestamp;
use pos_core::{IpcError, IpcResult};
use pos_hardware::image::{dots_for_paper, logo_from_png, png_dimensions};
use pos_hardware::receipt::{columns_for_paper, render_text, ReceiptTemplate};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::store::AssetKind;

/// Mirrors `NewClientInputSchema`.
#[derive(Debug, Clone, Deserialize)]
pub struct NewClientInput {
    pub display_name: String,
    pub client_slug: String,
    pub business_type: BusinessType,
    pub base_currency: CurrencyCode,
}

/// A complete, valid config from the four things asked when creating a
/// client. Everything else is edited afterwards.
pub fn new_client_config(input: &NewClientInput) -> ClientConfig {
    let restaurant = input.business_type == BusinessType::Restaurant;
    ClientConfig {
        schema_version: CLIENT_CONFIG_SCHEMA_VERSION,
        client_id: Uuid::new_v4(),
        client_slug: input.client_slug.trim().to_owned(),
        display_name: input.display_name.trim().to_owned(),
        business_type: input.business_type,
        locale: LocaleConfig {
            default: Locale::Ar,
            supported: vec![Locale::Ar, Locale::En],
        },
        currency: CurrencyConfig {
            base: input.base_currency,
            accepted: vec![],
        },
        tax: TaxConfig {
            registration_number: None,
            prices_include_tax: true,
            default_rate_bps: 0,
        },
        receipt: ReceiptLayout {
            logo_asset: None,
            header_lines: vec![],
            footer_text: "Thank you!".into(),
            show_tax_number: false,
            paper_width_mm: 80,
        },
        branding: Branding {
            primary_color: "#1F6FEB".into(),
            accent_color: "#F59E0B".into(),
        },
        features: Features {
            loyalty: false,
            kitchen_display: restaurant,
            multi_currency: false,
            purchase_orders: false,
        },
        cloud: CloudConfig {
            supabase_url: None,
            supabase_anon_key: None,
        },
    }
}

const MAX_LOGO_BYTES: usize = 1024 * 1024;
const MAX_ICON_BYTES: usize = 4 * 1024 * 1024;
const MAX_DIMENSION: u32 = 4096;
const MIN_ICON_SIZE: u32 = 512;

/// Checks an upload and returns its `(width, height)`.
pub fn validate_asset(kind: AssetKind, bytes: &[u8]) -> IpcResult<(u32, u32)> {
    let limit = match kind {
        AssetKind::ReceiptLogo => MAX_LOGO_BYTES,
        AssetKind::AppIcon => MAX_ICON_BYTES,
    };
    if bytes.len() > limit {
        return Err(IpcError::validation(format!(
            "The image is too large (at most {} KB).",
            limit / 1024
        )));
    }
    let (width, height) =
        png_dimensions(bytes).map_err(|_| IpcError::validation("Upload a PNG image."))?;
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(IpcError::validation(format!(
            "Images must be between 1 and {MAX_DIMENSION} pixels on each side."
        )));
    }
    match kind {
        AssetKind::ReceiptLogo => {
            // Must survive the exact conversion the till performs.
            logo_from_png(bytes, dots_for_paper(80))
                .map_err(|e| IpcError::validation(e.to_string()))?;
        }
        AssetKind::AppIcon => {
            if width != height || width < MIN_ICON_SIZE {
                return Err(IpcError::validation(format!(
                    "The app icon must be square and at least {MIN_ICON_SIZE}×{MIN_ICON_SIZE} pixels (1024×1024 recommended)."
                )));
            }
        }
    }
    Ok((width, height))
}

pub fn decode_base64(data: &str) -> IpcResult<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|_| IpcError::validation("The upload is not valid base64."))
}

pub fn encode_base64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Mirrors `ReceiptPreviewSchema`.
#[derive(Debug, Clone, Serialize)]
pub struct ReceiptPreview {
    pub columns: usize,
    pub text: String,
    /// The logo exactly as printed (dithered, 1-bit), as a PNG.
    pub logo_png_base64: Option<String>,
    pub logo_width: Option<usize>,
    pub logo_height: Option<usize>,
}

fn scaled(amount_in_thousandths: i64, currency: CurrencyCode) -> i64 {
    // Sample prices are written for a 3-decimal currency; rescale to the
    // client's minor unit so the preview shows realistic amounts.
    match currency.exponent() {
        3 => amount_in_thousandths,
        2 => amount_in_thousandths / 10,
        _ => amount_in_thousandths,
    }
}

/// A sample sale priced by the real engine with the client's tax settings,
/// rendered by the real receipt layout.
pub fn preview_receipt(
    config: &ClientConfig,
    logo_png: Option<&[u8]>,
    now: Timestamp,
) -> IpcResult<ReceiptPreview> {
    let currency = config.currency.base;
    let rate = i64::from(config.tax.default_rate_bps);
    let sample = [
        ("Spanish Latte", 1_250, 2_000),
        ("Saffron cake", 2_750, 1_000),
    ];
    let lines: Vec<PriceLine> = sample
        .iter()
        .map(|(name, unit, qty)| PriceLine {
            product_id: Uuid::nil(),
            category_id: None,
            name: (*name).to_owned(),
            sku: None,
            unit_price: scaled(*unit, currency),
            quantity_milli: *qty,
            tax_rate_bps: rate,
            group: None,
        })
        .collect();
    let quote = price(&lines, &[], config.tax.prices_include_tax)
        .map_err(|e| IpcError::internal(e.to_string()))?;
    // Cash rounded up to the next whole unit (next 100 for zero-decimal currencies).
    let unit = 10_i64.pow(currency.exponent()).max(100);
    let tendered = (quote.total.div_euclid(unit) + 1).saturating_mul(unit);
    let receipt = Receipt {
        transaction_id: Uuid::nil(),
        kind: TransactionKind::Sale,
        receipt_number: "7F3A-000042".into(),
        issued_at: now,
        cashier_name: "Sara".into(),
        customer_name: None,
        currency,
        lines: quote
            .lines
            .iter()
            .map(|l| ReceiptLine {
                name: l.name.clone(),
                quantity_milli: l.quantity_milli,
                unit_price: l.unit_price,
                modifiers: vec![],
                discount_amount: l.discount_amount,
                line_total: l.line_total,
            })
            .collect(),
        subtotal: quote.subtotal,
        discount_total: quote.discount_total,
        tax_lines: quote.tax_lines.clone(),
        total: quote.total,
        payments: vec![ReceiptPayment {
            method: PaymentMethod::Cash,
            amount: quote.total,
            tendered_currency: currency,
            tendered_amount: tendered,
        }],
        change_due: tendered - quote.total,
        loyalty: None,
        printed: true,
    };

    let logo = logo_png
        .map(|bytes| logo_from_png(bytes, dots_for_paper(config.receipt.paper_width_mm)))
        .transpose()
        .map_err(|e| IpcError::validation(e.to_string()))?;
    let logo_png_base64 = logo
        .as_ref()
        .map(|l| l.to_png().map(|png| encode_base64(&png)))
        .transpose()
        .map_err(|e| IpcError::internal(e.to_string()))?;
    let (logo_width, logo_height) = logo
        .as_ref()
        .map_or((None, None), |l| (Some(l.width), Some(l.height)));
    let template = ReceiptTemplate::for_client(config, logo);
    Ok(ReceiptPreview {
        columns: columns_for_paper(config.receipt.paper_width_mm),
        text: render_text(&receipt, &template, false),
        logo_png_base64,
        logo_width,
        logo_height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> NewClientInput {
        NewClientInput {
            display_name: "  Acme Grocers ".into(),
            client_slug: "acme-grocers".into(),
            business_type: BusinessType::Restaurant,
            base_currency: CurrencyCode::SAR,
        }
    }

    pub(crate) fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("header");
            let pixels: Vec<u8> = (0..width * height)
                .map(|i| if i % 3 == 0 { 0 } else { 255 })
                .collect();
            writer.write_image_data(&pixels).expect("data");
        }
        out
    }

    #[test]
    fn new_clients_get_a_valid_config() {
        let config = new_client_config(&input());
        config.validate().expect("valid");
        assert_eq!(config.display_name, "Acme Grocers");
        assert!(config.features.kitchen_display, "restaurants get the KDS");
        assert_ne!(config.client_id, new_client_config(&input()).client_id);
    }

    #[test]
    fn assets_are_checked() {
        assert_eq!(
            validate_asset(AssetKind::ReceiptLogo, &png(300, 80)).expect("logo"),
            (300, 80)
        );
        assert!(validate_asset(AssetKind::ReceiptLogo, b"GIF89a").is_err());
        assert!(
            validate_asset(AssetKind::AppIcon, &png(300, 80)).is_err(),
            "not square"
        );
        assert!(
            validate_asset(AssetKind::AppIcon, &png(256, 256)).is_err(),
            "too small"
        );
        assert_eq!(
            validate_asset(AssetKind::AppIcon, &png(512, 512)).expect("icon"),
            (512, 512)
        );
    }

    #[test]
    fn preview_uses_the_clients_receipt_settings() {
        let mut config = new_client_config(&input());
        config.receipt.header_lines = vec!["Riyadh".into()];
        config.receipt.footer_text = "Come again".into();
        config.receipt.paper_width_mm = 58;
        config.tax.default_rate_bps = 1_500;
        config.tax.registration_number = Some("300000000000003".into());
        config.receipt.show_tax_number = true;
        let now: Timestamp = "2026-09-24T10:00:00.000Z".parse().expect("ts");
        let preview = preview_receipt(&config, Some(&png(900, 120)), now).expect("preview");
        assert_eq!(preview.columns, 32);
        assert!(preview.text.contains("Acme Grocers"));
        assert!(preview.text.contains("Riyadh"));
        assert!(preview.text.contains("Come again"));
        assert!(preview.text.contains("300000000000003"));
        assert!(preview.text.contains("SAR"), "{}", preview.text);
        assert_eq!(preview.logo_width, Some(384), "scaled to 58 mm paper");
        assert!(preview.logo_png_base64.is_some());
        for line in preview.text.lines() {
            assert!(line.chars().count() <= 32, "{line}");
        }
    }
}

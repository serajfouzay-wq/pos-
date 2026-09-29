//! Receipt layout. One layout, two outputs: ESC/POS bytes for the printer and
//! plain text (tests, on-screen preview) — so what is tested is what prints.

use pos_core::config::{ClientConfig, Locale};
use pos_core::money::{to_decimal_string, MinorUnits};
use pos_core::receipt::Receipt;
use pos_core::sales::{PaymentMethod, TransactionKind};
use pos_core::time::Zone;

use crate::doc::{Doc, Line};
use crate::escpos::Align;
use crate::image::MonoImage;
use crate::words::{is_rtl, words};

/// Per-client presentation, from the embedded `ClientConfig`.
#[derive(Debug, Clone)]
pub struct ReceiptTemplate {
    pub business_name: String,
    pub header_lines: Vec<String>,
    pub footer_text: String,
    /// Printed only when set and the client enabled `show_tax_number`.
    pub tax_number: Option<String>,
    pub paper_width_mm: u16,
    pub logo: Option<MonoImage>,
    /// How the date prints (the till: its own zone; previews: UTC).
    pub zone: Zone,
}

impl ReceiptTemplate {
    /// The client's receipt settings, exactly as the till prints them (also
    /// used by the generator's preview).
    pub fn for_client(client: &ClientConfig, logo: Option<MonoImage>) -> Self {
        let receipt = &client.receipt;
        Self {
            business_name: client.display_name.clone(),
            header_lines: receipt.header_lines.clone(),
            footer_text: receipt.footer_text.clone(),
            tax_number: receipt
                .show_tax_number
                .then(|| client.tax.registration_number.clone())
                .flatten(),
            paper_width_mm: receipt.paper_width_mm,
            logo,
            zone: Zone::Utc,
        }
    }
}

pub use crate::doc::{columns_for_paper, two_columns, wrap};

/// `2000` → `"2"`, `250` → `"0.25"`, `1500` → `"1.5"`.
pub fn format_quantity(quantity_milli: i64) -> String {
    let whole = quantity_milli / 1000;
    let frac = (quantity_milli % 1000).abs();
    if frac == 0 {
        return whole.to_string();
    }
    let digits = format!("{frac:03}");
    format!("{whole}.{}", digits.trim_end_matches('0'))
}

/// The receipt as a printable document, in `language`.
pub fn document(
    receipt: &Receipt,
    template: &ReceiptTemplate,
    copy: bool,
    language: Locale,
) -> Doc {
    let w = words(language);
    let money = |amount: MinorUnits| to_decimal_string(amount, receipt.currency);
    let code = receipt.currency.as_str();
    let mut doc = Doc::new(template.paper_width_mm, is_rtl(language));
    doc.logo = template.logo.clone();

    if template.logo.is_some() {
        doc.push(Line::Logo);
    }
    doc.push(Line::large(template.business_name.as_str(), Align::Center));
    for line in &template.header_lines {
        doc.push(Line::text(line.as_str(), Align::Center));
    }
    if let Some(tax_number) = &template.tax_number {
        doc.push(Line::text(
            format!("{}: {tax_number}", w.tax_no),
            Align::Center,
        ));
    }
    match receipt.kind {
        TransactionKind::Sale => {}
        TransactionKind::Refund => {
            doc.push(Line::bold(w.refund_banner, Align::Center));
        }
        TransactionKind::Void => {
            doc.push(Line::bold(w.void_banner, Align::Center));
        }
    }
    if copy {
        doc.push(Line::bold(w.copy_banner, Align::Center));
    }
    doc.push(Line::Rule);
    doc.push(Line::row(w.receipt, receipt.receipt_number.as_str()));
    // 2026-09-23T10:15:30.123Z → 2026-09-23 13:15 (till time) / … 10:15 UTC
    doc.push(Line::row(
        w.date,
        template.zone.format_minutes(receipt.issued_at),
    ));
    doc.push(Line::row(w.cashier, receipt.cashier_name.as_str()));
    if let Some(customer) = &receipt.customer_name {
        doc.push(Line::row(w.customer, customer.as_str()));
    }
    doc.push(Line::Rule);

    for line in &receipt.lines {
        doc.push(Line::text(line.name.as_str(), Align::Left));
        for modifier in &line.modifiers {
            let delta = if modifier.price_delta == 0 {
                String::new()
            } else {
                money(modifier.price_delta)
            };
            doc.push(Line::row(format!("  + {}", modifier.name), delta));
        }
        let qty = format!(
            "  {} x {}",
            format_quantity(line.quantity_milli),
            money(line.unit_price)
        );
        doc.push(Line::row(
            qty,
            money(line.line_total + line.discount_amount),
        ));
        if line.discount_amount > 0 {
            doc.push(Line::row(
                format!("  {}", w.discount),
                format!("-{}", money(line.discount_amount)),
            ));
        }
    }
    doc.push(Line::Rule);

    doc.push(Line::row(w.subtotal, money(receipt.subtotal)));
    if receipt.discount_total > 0 {
        doc.push(Line::row(
            w.discount,
            format!("-{}", money(receipt.discount_total)),
        ));
    }
    // A 0% line (no VAT, as in Libya) says nothing.
    for tax in receipt
        .tax_lines
        .iter()
        .filter(|t| t.rate_bps != 0 || t.tax_amount != 0)
    {
        let rate = format_quantity(tax.rate_bps * 10); // bps → percent with ≤2 decimals
        doc.push(Line::row(
            format!("{} {rate}%", w.tax),
            money(tax.tax_amount),
        ));
    }
    doc.push(Line::total(
        format!("{} {code}", w.total),
        money(receipt.total),
    ));
    doc.push(Line::Rule);

    for payment in &receipt.payments {
        let shown = if payment.method == PaymentMethod::Cash {
            payment.tendered_amount
        } else {
            payment.amount
        };
        let label = if payment.tendered_currency == receipt.currency {
            w.method(payment.method).to_owned()
        } else {
            format!(
                "{} ({})",
                w.method(payment.method),
                payment.tendered_currency.as_str()
            )
        };
        doc.push(Line::row(
            label,
            to_decimal_string(shown, payment.tendered_currency),
        ));
    }
    if receipt.change_due > 0 {
        doc.push(Line::total(w.change, money(receipt.change_due)));
    }
    if let Some(loyalty) = &receipt.loyalty {
        // On a refund or void the figures are what was reversed.
        let sale = receipt.kind == TransactionKind::Sale;
        if loyalty.redeemed > 0 {
            let label = if sale {
                w.points_redeemed
            } else {
                w.points_returned
            };
            doc.push(Line::row(label, loyalty.redeemed.to_string()));
        }
        if sale || loyalty.earned > 0 {
            let label = if sale {
                w.points_earned
            } else {
                w.points_taken_back
            };
            doc.push(Line::row(label, loyalty.earned.to_string()));
        }
        doc.push(Line::row(w.points_balance, loyalty.balance.to_string()));
    }
    if let Some(member) = &receipt.member {
        doc.push(Line::row(w.member, member.plan_name.as_str()));
        doc.push(Line::row(
            format!("  {}", member.card_number),
            format!(
                "{} {}",
                w.member_until,
                template.zone.format_date(member.ends_at)
            ),
        ));
    }
    if !template.footer_text.trim().is_empty() {
        doc.push(Line::Rule);
        doc.push(Line::text(template.footer_text.as_str(), Align::Center));
    }
    doc
}

/// The English receipt as plain text (tests, previews).
pub fn render_text(receipt: &Receipt, template: &ReceiptTemplate, copy: bool) -> String {
    document(receipt, template, copy, Locale::En).to_text()
}

#[cfg(test)]
mod tests {
    use pos_core::pricing::TaxLine;
    use pos_core::receipt::{ReceiptLine, ReceiptPayment};
    use uuid::Uuid;

    use super::*;
    use crate::doc::PrintMode;
    use pos_core::currency::CurrencyCode;

    fn sample() -> Receipt {
        Receipt {
            transaction_id: Uuid::from_u128(1),
            kind: TransactionKind::Sale,
            receipt_number: "7F3A-000042".into(),
            issued_at: "2026-09-23T10:15:30.123Z".parse().expect("ts"),
            cashier_name: "Sara".into(),
            customer_name: None,
            currency: CurrencyCode::KWD,
            lines: vec![
                ReceiptLine {
                    name: "Spanish Latte".into(),
                    quantity_milli: 2000,
                    unit_price: 1_250,
                    modifiers: vec![],
                    discount_amount: 0,
                    line_total: 2_500,
                },
                ReceiptLine {
                    name: "Saffron cake, extra large slice with pistachio".into(),
                    quantity_milli: 250,
                    unit_price: 4_000,
                    modifiers: vec![],
                    discount_amount: 100,
                    line_total: 900,
                },
            ],
            subtotal: 3_500,
            discount_total: 100,
            tax_lines: vec![TaxLine {
                rate_bps: 500,
                taxable_amount: 3_238,
                tax_amount: 162,
            }],
            total: 3_400,
            payments: vec![ReceiptPayment {
                method: PaymentMethod::Cash,
                amount: 3_400,
                tendered_currency: CurrencyCode::KWD,
                tendered_amount: 5_000,
            }],
            change_due: 1_600,
            loyalty: None,
            member: None,
            printed: false,
        }
    }

    fn template() -> ReceiptTemplate {
        ReceiptTemplate {
            business_name: "Demo Cafe".into(),
            header_lines: vec!["Kuwait City".into()],
            footer_text: "Thank you for visiting!".into(),
            tax_number: Some("KW-123".into()),
            paper_width_mm: 58,
            logo: None,
            zone: Zone::Utc,
        }
    }

    #[test]
    fn layout_58mm_golden() {
        let expected = [
            "       Demo Cafe",
            "          Kuwait City",
            "         Tax No: KW-123",
            "--------------------------------",
            "Receipt              7F3A-000042",
            "Date        2026-09-23 10:15 UTC",
            "Cashier                     Sara",
            "--------------------------------",
            "Spanish Latte",
            "  2 x 1.250                2.500",
            "Saffron cake, extra large slice",
            "with pistachio",
            "  0.25 x 4.000             1.000",
            "  Discount                -0.100",
            "--------------------------------",
            "Subtotal                   3.500",
            "Discount                  -0.100",
            "Tax 5%                     0.162",
            "TOTAL KWD                  3.400",
            "--------------------------------",
            "Cash                       5.000",
            "Change                     1.600",
            "--------------------------------",
            "    Thank you for visiting!",
        ]
        .map(|l| format!("{l}\n"))
        .concat();
        assert_eq!(render_text(&sample(), &template(), false), expected);
    }

    #[test]
    fn every_line_fits_the_paper() {
        for width_mm in [58, 80] {
            let t = ReceiptTemplate {
                paper_width_mm: width_mm,
                ..template()
            };
            for line in render_text(&sample(), &t, true).lines() {
                assert!(
                    line.chars().count() <= columns_for_paper(width_mm),
                    "{line:?}"
                );
            }
        }
    }

    #[test]
    fn copies_are_marked_and_bytes_end_with_a_cut() {
        assert!(render_text(&sample(), &template(), true).contains("*** COPY ***"));
        let bytes = document(&sample(), &template(), false, Locale::En).to_escpos(PrintMode::Auto);
        assert!(bytes.ends_with(&[0x1D, b'V', 66, 3]));
        assert!(
            !bytes.windows(5).any(|w| w == crate::escpos::DRAWER_KICK),
            "no drawer kick in receipts"
        );
    }

    #[test]
    fn quantities_format_compactly() {
        assert_eq!(format_quantity(2000), "2");
        assert_eq!(format_quantity(250), "0.25");
        assert_eq!(format_quantity(1500), "1.5");
        assert_eq!(format_quantity(1), "0.001");
    }
}

//! Receipt layout. One layout, two outputs: ESC/POS bytes for the printer and
//! plain text (tests, on-screen preview) — so what is tested is what prints.

use pos_core::currency::CurrencyCode;
use pos_core::money::{to_decimal_string, MinorUnits};
use pos_core::receipt::Receipt;
use pos_core::sales::{PaymentMethod, TransactionKind};

use crate::escpos::{Align, EscPos};
use crate::image::MonoImage;

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Block {
    Logo,
    Text {
        text: String,
        align: Align,
        bold: bool,
        double: bool,
    },
    Rule,
}

/// Characters per line in font A.
pub fn columns_for_paper(paper_width_mm: u16) -> usize {
    if paper_width_mm >= 80 {
        48
    } else {
        32
    }
}

fn text(text: impl Into<String>, align: Align) -> Block {
    Block::Text {
        text: text.into(),
        align,
        bold: false,
        double: false,
    }
}

fn bold(text: impl Into<String>, align: Align) -> Block {
    Block::Text {
        text: text.into(),
        align,
        bold: true,
        double: false,
    }
}

/// `left ....... right` in exactly `width` characters (left side truncated).
fn two_columns(left: &str, right: &str, width: usize) -> String {
    let right_len = right.chars().count();
    let room = width.saturating_sub(right_len + 1);
    let left: String = left.chars().take(room).collect();
    let pad = width.saturating_sub(left.chars().count() + right_len);
    format!("{left}{}{right}", " ".repeat(pad))
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let needed =
            current.chars().count() + usize::from(!current.is_empty()) + word.chars().count();
        if needed > width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
        while current.chars().count() > width {
            let head: String = current.chars().take(width).collect();
            current = current.chars().skip(width).collect();
            lines.push(head);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

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

fn method_label(method: PaymentMethod) -> &'static str {
    match method {
        PaymentMethod::Cash => "Cash",
        PaymentMethod::Card => "Card",
        PaymentMethod::Wallet => "Wallet",
        PaymentMethod::Loyalty => "Loyalty points",
        PaymentMethod::Voucher => "Voucher",
    }
}

fn layout(receipt: &Receipt, template: &ReceiptTemplate, copy: bool) -> Vec<Block> {
    let width = columns_for_paper(template.paper_width_mm);
    let money = |amount: MinorUnits| to_decimal_string(amount, receipt.currency);
    let code = receipt.currency.as_str();
    let mut out = Vec::new();

    if template.logo.is_some() {
        out.push(Block::Logo);
    }
    out.push(Block::Text {
        text: template.business_name.clone(),
        align: Align::Center,
        bold: true,
        double: true,
    });
    for line in &template.header_lines {
        out.push(text(line.as_str(), Align::Center));
    }
    if let Some(tax_number) = &template.tax_number {
        out.push(text(format!("Tax No: {tax_number}"), Align::Center));
    }
    match receipt.kind {
        TransactionKind::Sale => {}
        TransactionKind::Refund => out.push(bold("*** REFUND ***", Align::Center)),
        TransactionKind::Void => out.push(bold("*** VOID ***", Align::Center)),
    }
    if copy {
        out.push(bold("*** COPY ***", Align::Center));
    }
    out.push(Block::Rule);
    out.push(text(
        two_columns("Receipt", &receipt.receipt_number, width),
        Align::Left,
    ));
    // 2026-09-23T10:15:30.123Z → 2026-09-23 10:15 UTC
    let issued = receipt.issued_at.to_string();
    let issued = format!("{} {} UTC", &issued[..10], &issued[11..16]);
    out.push(text(two_columns("Date", &issued, width), Align::Left));
    out.push(text(
        two_columns("Cashier", &receipt.cashier_name, width),
        Align::Left,
    ));
    if let Some(customer) = &receipt.customer_name {
        out.push(text(two_columns("Customer", customer, width), Align::Left));
    }
    out.push(Block::Rule);

    for line in &receipt.lines {
        for name_line in wrap(&line.name, width) {
            out.push(text(name_line, Align::Left));
        }
        for modifier in &line.modifiers {
            let delta = if modifier.price_delta == 0 {
                String::new()
            } else {
                money(modifier.price_delta)
            };
            out.push(text(
                two_columns(&format!("  + {}", modifier.name), &delta, width),
                Align::Left,
            ));
        }
        let qty = format!(
            "  {} x {}",
            format_quantity(line.quantity_milli),
            money(line.unit_price)
        );
        out.push(text(
            two_columns(&qty, &money(line.line_total + line.discount_amount), width),
            Align::Left,
        ));
        if line.discount_amount > 0 {
            out.push(text(
                two_columns(
                    "  Discount",
                    &format!("-{}", money(line.discount_amount)),
                    width,
                ),
                Align::Left,
            ));
        }
    }
    out.push(Block::Rule);

    out.push(text(
        two_columns("Subtotal", &money(receipt.subtotal), width),
        Align::Left,
    ));
    if receipt.discount_total > 0 {
        out.push(text(
            two_columns(
                "Discount",
                &format!("-{}", money(receipt.discount_total)),
                width,
            ),
            Align::Left,
        ));
    }
    for tax in &receipt.tax_lines {
        let rate = format_quantity(tax.rate_bps * 10); // bps → percent with ≤2 decimals
        out.push(text(
            two_columns(&format!("Tax {rate}%"), &money(tax.tax_amount), width),
            Align::Left,
        ));
    }
    out.push(bold(
        two_columns(&format!("TOTAL {code}"), &money(receipt.total), width),
        Align::Left,
    ));
    out.push(Block::Rule);

    for payment in &receipt.payments {
        let shown = if payment.method == PaymentMethod::Cash {
            payment.tendered_amount
        } else {
            payment.amount
        };
        let label = if payment.tendered_currency == receipt.currency {
            method_label(payment.method).to_owned()
        } else {
            format!(
                "{} ({})",
                method_label(payment.method),
                payment.tendered_currency.as_str()
            )
        };
        out.push(text(
            two_columns(&label, &money_in(shown, payment.tendered_currency), width),
            Align::Left,
        ));
    }
    if receipt.change_due > 0 {
        out.push(bold(
            two_columns("Change", &money(receipt.change_due), width),
            Align::Left,
        ));
    }
    if let Some(loyalty) = &receipt.loyalty {
        out.push(text(
            two_columns("Points earned", &loyalty.earned.to_string(), width),
            Align::Left,
        ));
        out.push(text(
            two_columns("Points balance", &loyalty.balance.to_string(), width),
            Align::Left,
        ));
    }
    if !template.footer_text.trim().is_empty() {
        out.push(Block::Rule);
        for line in wrap(&template.footer_text, width) {
            out.push(text(line, Align::Center));
        }
    }
    out
}

fn money_in(amount: MinorUnits, currency: CurrencyCode) -> String {
    to_decimal_string(amount, currency)
}

/// ESC/POS bytes for the printer: initialise, content, feed, cut.
pub fn render_escpos(receipt: &Receipt, template: &ReceiptTemplate, copy: bool) -> Vec<u8> {
    let width = columns_for_paper(template.paper_width_mm);
    let mut p = EscPos::new();
    for block in layout(receipt, template, copy) {
        match block {
            Block::Logo => {
                if let Some(logo) = &template.logo {
                    p.align(Align::Center).raster(logo);
                }
            }
            Block::Text {
                text,
                align,
                bold,
                double,
            } => {
                p.align(align).bold(bold).double(double);
                if double {
                    for line in wrap(&text, width / 2) {
                        p.line(&line);
                    }
                } else {
                    p.line(&text);
                }
                p.bold(false).double(false);
            }
            Block::Rule => {
                p.align(Align::Left).line(&"-".repeat(width));
            }
        }
    }
    p.feed(3).cut();
    p.into_bytes()
}

/// Plain-text rendering of exactly the same layout.
pub fn render_text(receipt: &Receipt, template: &ReceiptTemplate, copy: bool) -> String {
    let width = columns_for_paper(template.paper_width_mm);
    let pad = |line: &str, align: Align, width_used: usize| {
        let len = line.chars().count();
        let free = width_used.saturating_sub(len);
        match align {
            Align::Left => line.to_owned(),
            Align::Center => format!("{}{line}", " ".repeat(free / 2)),
            Align::Right => format!("{}{line}", " ".repeat(free)),
        }
    };
    let mut out = String::new();
    for block in layout(receipt, template, copy) {
        match block {
            Block::Logo => out.push_str("[logo]\n"),
            Block::Text {
                text,
                align,
                double,
                ..
            } => {
                if double {
                    // Double-width glyphs occupy two columns each.
                    for line in wrap(&text, width / 2) {
                        let used = line.chars().count() * 2;
                        let free = width.saturating_sub(used);
                        let lead = match align {
                            Align::Left => 0,
                            Align::Center => free / 2,
                            Align::Right => free,
                        };
                        out.push_str(&" ".repeat(lead));
                        out.push_str(&line);
                        out.push('\n');
                    }
                } else {
                    out.push_str(pad(&text, align, width).trim_end());
                    out.push('\n');
                }
            }
            Block::Rule => {
                out.push_str(&"-".repeat(width));
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use pos_core::pricing::TaxLine;
    use pos_core::receipt::{ReceiptLine, ReceiptPayment};
    use uuid::Uuid;

    use super::*;

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
        let bytes = render_escpos(&sample(), &template(), false);
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

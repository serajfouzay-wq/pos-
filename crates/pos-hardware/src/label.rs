//! Shelf / product labels with a printed barcode, on the receipt printer.
//!
//! EAN-13, EAN-8 and UPC-A are used when the code is all digits with a valid
//! check digit (retail scanners read them fastest); anything else printable
//! goes out as Code 128 (set B).

use crate::escpos::{Align, EscPos};
use crate::receipt::columns_for_paper;

const GS: u8 = 0x1D;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symbology {
    Ean13,
    Ean8,
    UpcA,
    Code128,
}

fn gs1_check_digit_ok(digits: &[u8]) -> bool {
    let (body, check) = digits.split_at(digits.len() - 1);
    // Weights 3,1,3,… from the digit next to the check digit.
    let sum: u32 = body
        .iter()
        .rev()
        .enumerate()
        .map(|(i, d)| u32::from(d - b'0') * if i % 2 == 0 { 3 } else { 1 })
        .sum();
    (10 - sum % 10) % 10 == u32::from(check[0] - b'0')
}

/// The symbology a code prints as, or `None` if it cannot be printed.
pub fn symbology(code: &str) -> Option<Symbology> {
    let bytes = code.as_bytes();
    let digits = !bytes.is_empty() && bytes.iter().all(u8::is_ascii_digit);
    match bytes.len() {
        13 if digits && gs1_check_digit_ok(bytes) => Some(Symbology::Ean13),
        8 if digits && gs1_check_digit_ok(bytes) => Some(Symbology::Ean8),
        12 if digits && gs1_check_digit_ok(bytes) => Some(Symbology::UpcA),
        1..=40 if bytes.iter().all(|b| (0x20..0x7F).contains(b)) => Some(Symbology::Code128),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductLabel {
    pub name: String,
    /// Already formatted, e.g. `1.250 KWD`.
    pub price: String,
    pub barcode: Option<String>,
}

/// `GS k` (function B) for `code`, with human-readable digits below.
fn barcode_bytes(code: &str) -> Option<Vec<u8>> {
    let kind = symbology(code)?;
    let (m, data) = match kind {
        Symbology::Ean13 => (67, code.as_bytes().to_vec()),
        Symbology::Ean8 => (68, code.as_bytes().to_vec()),
        Symbology::UpcA => (65, code.as_bytes().to_vec()),
        Symbology::Code128 => (73, [b"{B".as_slice(), code.as_bytes()].concat()),
    };
    let mut out = vec![
        GS, b'h', 80, GS, b'w', 2, GS, b'H', 2, GS, b'f', 0, GS, b'k', m,
    ];
    out.push(u8::try_from(data.len()).ok()?);
    out.extend(data);
    Some(out)
}

fn fit(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

/// `copies` labels, each cut.
pub fn render_escpos(label: &ProductLabel, paper_width_mm: u16, copies: u8) -> Vec<u8> {
    let width = columns_for_paper(paper_width_mm);
    let mut p = EscPos::new();
    for _ in 0..copies.max(1) {
        p.align(Align::Center)
            .bold(true)
            .line(&fit(&label.name, width))
            .bold(false);
        p.double(true)
            .line(&fit(&label.price, width / 2))
            .double(false);
        if let Some(bytes) = label.barcode.as_deref().and_then(barcode_bytes) {
            p.raw(&bytes).feed(1);
        }
        p.feed(2).cut();
    }
    p.into_bytes()
}

/// Text rendering (tests and previews).
pub fn render_text(label: &ProductLabel, paper_width_mm: u16) -> String {
    let width = columns_for_paper(paper_width_mm);
    let center = |s: &str| {
        let s = fit(s, width);
        format!("{}{s}", " ".repeat((width - s.chars().count()) / 2))
    };
    let mut lines = vec![center(&label.name), center(&label.price)];
    if let Some(code) = &label.barcode {
        match symbology(code) {
            Some(kind) => lines.push(center(&format!("[{kind:?} {code}]"))),
            None => lines.push(center(&format!("({code}: not printable)"))),
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbologies_are_chosen_by_check_digit() {
        assert_eq!(
            symbology("6281000000014"),
            Some(Symbology::Ean13),
            "valid EAN-13"
        );
        assert_eq!(
            symbology("6281000000015"),
            Some(Symbology::Code128),
            "bad check digit"
        );
        assert_eq!(symbology("96385074"), Some(Symbology::Ean8));
        assert_eq!(symbology("036000291452"), Some(Symbology::UpcA));
        assert_eq!(symbology("SKU-42"), Some(Symbology::Code128));
        assert_eq!(symbology("قهوة"), None);
        assert_eq!(symbology(""), None);
    }

    #[test]
    fn label_bytes_carry_the_barcode_command() {
        let label = ProductLabel {
            name: "Dates 500 g".into(),
            price: "2.500 KWD".into(),
            barcode: Some("6281000000014".into()),
        };
        let bytes = render_escpos(&label, 58, 2);
        let ean = [GS, b'k', 67, 13];
        let hits = bytes.windows(ean.len()).filter(|w| *w == ean).count();
        assert_eq!(hits, 2, "one barcode per copy");
        assert!(bytes.windows(13).any(|w| w == b"6281000000014"));
        let code128 = render_escpos(
            &ProductLabel {
                barcode: Some("SKU-42".into()),
                ..label.clone()
            },
            80,
            1,
        );
        assert!(code128.windows(5).any(|w| w == [GS, b'k', 73, 8, b'{']));
        assert!(render_text(&label, 58).contains("[Ean13 6281000000014]"));
    }
}

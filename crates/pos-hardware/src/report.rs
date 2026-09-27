//! Printed reports (X / Z): a title, a few centred lines, then sections of
//! `label ...... value` rows. The till decides the content; this module only
//! lays it out for the paper width, as ESC/POS bytes or the same text.

use crate::escpos::{Align, EscPos};
use crate::receipt::{columns_for_paper, two_columns, wrap};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportRow {
    /// `label ...... value`; bold for totals.
    Pair {
        label: String,
        value: String,
        bold: bool,
    },
    /// A free line (wrapped).
    Text(String),
}

impl ReportRow {
    pub fn pair(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Pair {
            label: label.into(),
            value: value.into(),
            bold: false,
        }
    }

    pub fn total(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self::Pair {
            label: label.into(),
            value: value.into(),
            bold: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportSection {
    pub heading: Option<String>,
    pub rows: Vec<ReportRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportDoc {
    /// Printed double size, e.g. `Z REPORT #12`.
    pub title: String,
    /// Centred lines under the title (shop, till, period).
    pub lines: Vec<String>,
    pub sections: Vec<ReportSection>,
}

enum Line {
    Title(String),
    Centered(String),
    Plain(String, bool),
    Rule,
}

fn layout(doc: &ReportDoc, width: usize) -> Vec<Line> {
    let mut out = vec![Line::Title(doc.title.clone())];
    for line in &doc.lines {
        out.extend(wrap(line, width).into_iter().map(Line::Centered));
    }
    for section in &doc.sections {
        out.push(Line::Rule);
        if let Some(heading) = &section.heading {
            out.push(Line::Plain(heading.to_uppercase(), true));
        }
        for row in &section.rows {
            match row {
                ReportRow::Pair { label, value, bold } => {
                    out.push(Line::Plain(two_columns(label, value, width), *bold));
                }
                ReportRow::Text(text) => {
                    out.extend(wrap(text, width).into_iter().map(|l| Line::Plain(l, false)));
                }
            }
        }
    }
    out.push(Line::Rule);
    out
}

pub fn render_escpos(doc: &ReportDoc, paper_width_mm: u16) -> Vec<u8> {
    let width = columns_for_paper(paper_width_mm);
    let mut p = EscPos::new();
    for line in layout(doc, width) {
        match line {
            Line::Title(title) => {
                p.align(Align::Center).bold(true).double(true);
                for part in wrap(&title, width / 2) {
                    p.line(&part);
                }
                p.bold(false).double(false);
            }
            Line::Centered(text) => {
                p.align(Align::Center).line(&text);
            }
            Line::Plain(text, bold) => {
                p.align(Align::Left).bold(bold).line(&text).bold(false);
            }
            Line::Rule => {
                p.align(Align::Left).line(&"-".repeat(width));
            }
        }
    }
    p.feed(3).cut();
    p.into_bytes()
}

/// The same layout as plain text (on-screen preview, tests).
pub fn render_text(doc: &ReportDoc, paper_width_mm: u16) -> String {
    let width = columns_for_paper(paper_width_mm);
    let center = |text: &str, used: usize| {
        let free = width.saturating_sub(used);
        format!("{}{text}", " ".repeat(free / 2))
    };
    let mut out = String::new();
    for line in layout(doc, width) {
        let text = match line {
            Line::Title(title) => wrap(&title, width / 2)
                .iter()
                .map(|part| center(part, part.chars().count() * 2))
                .collect::<Vec<_>>()
                .join("\n"),
            Line::Centered(text) => center(&text, text.chars().count()),
            Line::Plain(text, _) => text.trim_end().to_owned(),
            Line::Rule => "-".repeat(width),
        };
        out.push_str(&text);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> ReportDoc {
        ReportDoc {
            title: "Z REPORT #3".into(),
            lines: vec!["Demo Cafe".into(), "Till 01A0".into()],
            sections: vec![
                ReportSection {
                    heading: Some("Sales".into()),
                    rows: vec![
                        ReportRow::pair("Sales (12)", "54.250"),
                        ReportRow::pair("Refunds (1)", "-1.250"),
                        ReportRow::total("Net sales", "53.000"),
                    ],
                },
                ReportSection {
                    heading: None,
                    rows: vec![ReportRow::Text("Printed 2026-09-24 23:05".into())],
                },
            ],
        }
    }

    #[test]
    fn layout_58mm_golden() {
        let expected = [
            "     Z REPORT #3",
            "           Demo Cafe",
            "           Till 01A0",
            "--------------------------------",
            "SALES",
            "Sales (12)                54.250",
            "Refunds (1)               -1.250",
            "Net sales                 53.000",
            "--------------------------------",
            "Printed 2026-09-24 23:05",
            "--------------------------------",
            "",
        ]
        .join("\n");
        assert_eq!(render_text(&doc(), 58), expected);
    }

    #[test]
    fn escpos_ends_with_a_cut_and_bolds_totals() {
        let bytes = render_escpos(&doc(), 80);
        assert!(bytes.windows(2).any(|w| w == [0x1d, 0x56]), "cut");
        // ESC E 1 … "Net sales" … ESC E 0
        let net = bytes
            .windows(9)
            .position(|w| w == b"Net sales")
            .expect("net line");
        assert_eq!(&bytes[net - 3..net], &[0x1b, 0x45, 0x01]);
    }
}

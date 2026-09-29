//! Printed reports (X / Z): a title, a few centred lines, then sections of
//! `label ...... value` rows. The till decides the content; this module only
//! lays it out for the paper width, as ESC/POS bytes or the same text.

use crate::doc::{Doc, Line};
use crate::escpos::Align;

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

/// The report as a printable document (`rtl` for right-to-left languages;
/// the words themselves come with the rows).
pub fn document(report: &ReportDoc, paper_width_mm: u16, rtl: bool) -> Doc {
    let mut doc = Doc::new(paper_width_mm, rtl);
    doc.push(Line::large(report.title.as_str(), Align::Center));
    for line in &report.lines {
        doc.push(Line::text(line.as_str(), Align::Center));
    }
    for section in &report.sections {
        doc.push(Line::Rule);
        if let Some(heading) = &section.heading {
            doc.push(Line::bold(heading.to_uppercase(), Align::Left));
        }
        for row in &section.rows {
            doc.push(match row {
                ReportRow::Pair {
                    label,
                    value,
                    bold: false,
                } => Line::row(label.as_str(), value.as_str()),
                ReportRow::Pair { label, value, .. } => Line::total(label.as_str(), value.as_str()),
                ReportRow::Text(text) => Line::text(text.as_str(), Align::Left),
            });
        }
    }
    doc.push(Line::Rule);
    doc
}

/// The same layout as plain text (on-screen preview, tests).
pub fn render_text(report: &ReportDoc, paper_width_mm: u16) -> String {
    document(report, paper_width_mm, false).to_text()
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
        let bytes = document(&doc(), 80, false).to_escpos(crate::doc::PrintMode::Text);
        assert!(bytes.windows(2).any(|w| w == [0x1d, 0x56]), "cut");
        // ESC E 1 … "Net sales" … ESC E 0
        let net = bytes
            .windows(9)
            .position(|w| w == b"Net sales")
            .expect("net line");
        assert_eq!(&bytes[net - 3..net], &[0x1b, 0x45, 0x01]);
    }
}

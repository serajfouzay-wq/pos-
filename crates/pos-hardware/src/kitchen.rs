//! Kitchen tickets: what to cook, for which table, which course. Printed on
//! the kitchen printer when a course is fired (the KDS screen is Phase 8).

use pos_core::time::Timestamp;

use crate::escpos::{Align, EscPos};
use crate::receipt::{columns_for_paper, format_quantity};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitchenLine {
    pub quantity_milli: i64,
    pub name: String,
    pub modifiers: Vec<String>,
    pub note: Option<String>,
    pub course: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitchenTicket {
    /// "Table T4" or "Tab Sara".
    pub title: String,
    /// The course being fired; `None` = everything not yet sent.
    pub course: Option<i64>,
    pub server: String,
    pub guests: i64,
    pub at: Timestamp,
    pub lines: Vec<KitchenLine>,
}

fn layout(ticket: &KitchenTicket, width: usize) -> Vec<(String, bool, bool)> {
    // (text, bold, double)
    let mut out = vec![(ticket.title.clone(), true, true)];
    if let Some(course) = ticket.course {
        out.push((format!("COURSE {course}"), true, false));
    }
    let time = ticket.at.to_string();
    let mut meta = format!("{} · {} UTC", ticket.server, &time[11..16]);
    if ticket.guests > 0 {
        meta.push_str(&format!(" · {} guests", ticket.guests));
    }
    out.push((meta, false, false));
    out.push(("-".repeat(width), false, false));
    let mut last_course = None;
    for line in &ticket.lines {
        if ticket.course.is_none() && line.course.is_some() && line.course != last_course {
            out.push((
                format!("-- course {} --", line.course.unwrap_or(0)),
                true,
                false,
            ));
            last_course = line.course;
        }
        out.push((
            format!("{} x {}", format_quantity(line.quantity_milli), line.name),
            true,
            false,
        ));
        for modifier in &line.modifiers {
            out.push((format!("   + {modifier}"), false, false));
        }
        if let Some(note) = line.note.as_deref().filter(|n| !n.trim().is_empty()) {
            out.push((format!("   ! {note}"), true, false));
        }
    }
    out.push(("-".repeat(width), false, false));
    out
}

pub fn render_escpos(ticket: &KitchenTicket, paper_width_mm: u16) -> Vec<u8> {
    let width = columns_for_paper(paper_width_mm);
    let mut p = EscPos::new();
    for (i, (text, bold, double)) in layout(ticket, width).into_iter().enumerate() {
        p.align(if i < 3 { Align::Center } else { Align::Left })
            .bold(bold)
            .double(double);
        p.line(&text).bold(false).double(false);
    }
    p.feed(3).cut();
    p.into_bytes()
}

pub fn render_text(ticket: &KitchenTicket, paper_width_mm: u16) -> String {
    let width = columns_for_paper(paper_width_mm);
    layout(ticket, width)
        .into_iter()
        .map(|(text, _, _)| text)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ticket_lists_what_to_cook() {
        let ticket = KitchenTicket {
            title: "Table T4".into(),
            course: Some(2),
            server: "Omar".into(),
            guests: 3,
            at: "2026-09-24T19:05:00.000Z".parse().expect("ts"),
            lines: vec![
                KitchenLine {
                    quantity_milli: 2000,
                    name: "Ribeye".into(),
                    modifiers: vec!["Medium rare".into(), "Fries".into()],
                    note: Some("sauce on the side".into()),
                    course: Some(2),
                },
                KitchenLine {
                    quantity_milli: 1000,
                    name: "Salad".into(),
                    modifiers: vec![],
                    note: None,
                    course: Some(2),
                },
            ],
        };
        let text = render_text(&ticket, 80);
        assert!(
            text.starts_with("Table T4\nCOURSE 2\nOmar · 19:05 UTC · 3 guests"),
            "{text}"
        );
        assert!(text.contains(
            "2 x Ribeye\n   + Medium rare\n   + Fries\n   ! sauce on the side\n1 x Salad"
        ));
        let bytes = render_escpos(&ticket, 80);
        assert!(
            bytes.ends_with(&[0x1D, b'V', 0x42, 0x00])
                || bytes.windows(2).any(|w| w == [0x1D, b'V'])
        );
    }
}

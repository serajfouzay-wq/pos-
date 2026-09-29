//! Kitchen tickets: what to cook, for which table, which course, and void
//! tickets for sent items that must not be made. Printed on the kitchen
//! printer whenever food is sent (the kitchen display shows the same).

use pos_core::config::Locale;
use pos_core::time::{Timestamp, Zone};
use serde::{Deserialize, Serialize};

use crate::doc::{Doc, Line};
use crate::escpos::Align;
use crate::receipt::format_quantity;
use crate::words::{is_rtl, words};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KitchenLine {
    pub quantity_milli: i64,
    pub name: String,
    pub modifiers: Vec<String>,
    pub note: Option<String>,
    pub course: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KitchenTicket {
    /// "Table T4" or "Tab Sara".
    pub title: String,
    /// The course being fired; `None` = everything not yet sent.
    pub course: Option<i64>,
    pub server: String,
    pub guests: i64,
    pub at: Timestamp,
    /// The wall clock the time prints in (not stored: the printing till's).
    #[serde(skip)]
    pub zone: Zone,
    pub lines: Vec<KitchenLine>,
    /// Items already sent that must NOT be made (changed or removed).
    #[serde(default)]
    pub void: bool,
}

/// The ticket as a printable document, in `language`.
pub fn document(ticket: &KitchenTicket, paper_width_mm: u16, language: Locale) -> Doc {
    let w = words(language);
    let mut doc = Doc::new(paper_width_mm, is_rtl(language));
    if ticket.void {
        doc.push(Line::large(
            format!("*** {} ***", w.void_ticket),
            Align::Center,
        ));
    }
    doc.push(Line::large(ticket.title.as_str(), Align::Center));
    if let Some(course) = ticket.course {
        doc.push(Line::bold(format!("{} {course}", w.course), Align::Center));
    }
    let mut meta = format!("{} · {}", ticket.server, ticket.zone.format_time(ticket.at));
    if ticket.guests > 0 {
        meta.push_str(&format!(" · {} {}", ticket.guests, w.guests));
    }
    doc.push(Line::text(meta, Align::Center));
    doc.push(Line::Rule);
    let mut last_course = None;
    for line in &ticket.lines {
        if ticket.course.is_none() && line.course.is_some() && line.course != last_course {
            doc.push(Line::bold(
                format!("-- {} {} --", w.course, line.course.unwrap_or(0)),
                Align::Left,
            ));
            last_course = line.course;
        }
        let item = format!("{} x {}", format_quantity(line.quantity_milli), line.name);
        doc.push(Line::large(item, Align::Left));
        for modifier in &line.modifiers {
            doc.push(Line::text(format!("   + {modifier}"), Align::Left));
        }
        if let Some(note) = line.note.as_deref().filter(|n| !n.trim().is_empty()) {
            doc.push(Line::bold(format!("   ! {note}"), Align::Left));
        }
    }
    doc.push(Line::Rule);
    doc
}

/// The English ticket as plain text (tests, the till's on-screen copy).
pub fn render_text(ticket: &KitchenTicket, paper_width_mm: u16) -> String {
    document(ticket, paper_width_mm, Locale::En).to_text()
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
            zone: Zone::Utc,
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
            void: false,
        };
        let text = render_text(&ticket, 80);
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        assert_eq!(
            lines[..3],
            ["Table T4", "COURSE 2", "Omar · 19:05 UTC · 3 guests"],
            "{text}"
        );
        assert!(text.contains(
            "2 x Ribeye\n   + Medium rare\n   + Fries\n   ! sauce on the side\n1 x Salad"
        ));
        let bytes = document(&ticket, 80, Locale::En).to_escpos(crate::doc::PrintMode::Auto);
        assert!(bytes.ends_with(&[0x1D, b'V', 66, 3]));

        let void = KitchenTicket {
            void: true,
            ..ticket
        };
        assert!(render_text(&void, 80)
            .trim_start()
            .starts_with("*** VOID ***"));
        let arabic = document(&void, 80, Locale::Ar);
        assert!(arabic.rtl && arabic.needs_image());
    }
}

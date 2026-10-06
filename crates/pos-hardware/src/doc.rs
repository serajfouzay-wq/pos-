//! A printable document. The layouts (receipt, kitchen ticket, report) build
//! one [`Doc`]; it then prints either as printer text (fast, but only the
//! Latin characters of code page 1252) or as an image drawn with the bundled
//! font ([`crate::raster`]), which handles Arabic, right-to-left lines and
//! anything else the font covers.
//!
//! Both outputs come from the same lines, and [`Doc::to_text`] (tests, the
//! on-screen preview) is exactly the text mode.

use serde::{Deserialize, Serialize};

use crate::escpos::{Align, EscPos};
use crate::image::MonoImage;
use crate::raster;

/// How documents reach the paper. `Auto` prints text when every character
/// fits the printer's code page and the document reads left to right, and
/// an image otherwise.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintMode {
    #[default]
    Auto,
    Text,
    Image,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// The document's logo, centred.
    Logo,
    /// `Align::Left` is the start of the line (the right edge when the
    /// document is right-to-left).
    Text {
        text: String,
        align: Align,
        bold: bool,
        large: bool,
    },
    /// `left ...... right`: a label at the start, a value at the end.
    Row {
        left: String,
        right: String,
        bold: bool,
    },
    Rule,
}

impl Line {
    pub fn text(text: impl Into<String>, align: Align) -> Self {
        Line::Text {
            text: text.into(),
            align,
            bold: false,
            large: false,
        }
    }

    pub fn bold(text: impl Into<String>, align: Align) -> Self {
        Line::Text {
            text: text.into(),
            align,
            bold: true,
            large: false,
        }
    }

    pub fn large(text: impl Into<String>, align: Align) -> Self {
        Line::Text {
            text: text.into(),
            align,
            bold: true,
            large: true,
        }
    }

    pub fn row(left: impl Into<String>, right: impl Into<String>) -> Self {
        Line::Row {
            left: left.into(),
            right: right.into(),
            bold: false,
        }
    }

    pub fn total(left: impl Into<String>, right: impl Into<String>) -> Self {
        Line::Row {
            left: left.into(),
            right: right.into(),
            bold: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    pub lines: Vec<Line>,
    pub paper_width_mm: u16,
    /// Right-to-left (Arabic): rows put the label on the right.
    pub rtl: bool,
    pub logo: Option<MonoImage>,
}

/// Characters per line in font A.
pub fn columns_for_paper(paper_width_mm: u16) -> usize {
    if paper_width_mm >= 80 {
        48
    } else {
        32
    }
}

/// `left ....... right` in exactly `width` characters (left side truncated).
pub fn two_columns(left: &str, right: &str, width: usize) -> String {
    let right_len = right.chars().count();
    let room = width.saturating_sub(right_len + 1);
    let left: String = left.chars().take(room).collect();
    let pad = width.saturating_sub(left.chars().count() + right_len);
    format!("{left}{}{right}", " ".repeat(pad))
}

/// Word wrap to `width` characters; words longer than a line are split.
/// Leading spaces (an indented option line) are kept on every line.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let body = text.trim_start();
    let indent = &text[..text.len() - body.len()];
    let indent_len = indent.chars().count();
    if indent_len == 0 || indent_len >= width {
        return wrap_words(body, width);
    }
    wrap_words(body, width - indent_len)
        .into_iter()
        .map(|line| format!("{indent}{line}"))
        .collect()
}

fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
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

/// Whether the printer's own font can print `c` (code page 1252).
fn printable_as_text(c: char) -> bool {
    matches!(c, ' '..='~' | '\u{A0}'..='\u{FF}' | '€' | '‘' | '’' | '“' | '”' | '–' | '—' | '…')
}

impl Doc {
    pub fn new(paper_width_mm: u16, rtl: bool) -> Self {
        Self {
            lines: Vec::new(),
            paper_width_mm,
            rtl,
            logo: None,
        }
    }

    pub fn push(&mut self, line: Line) -> &mut Self {
        self.lines.push(line);
        self
    }

    fn strings(&self) -> impl Iterator<Item = &str> {
        self.lines.iter().flat_map(|line| match line {
            Line::Text { text, .. } => vec![text.as_str()],
            Line::Row { left, right, .. } => vec![left.as_str(), right.as_str()],
            Line::Logo | Line::Rule => vec![],
        })
    }

    /// Whether text mode would lose something (a right-to-left layout, or a
    /// character outside the printer's code page).
    pub fn needs_image(&self) -> bool {
        self.rtl || self.strings().any(|s| !s.chars().all(printable_as_text))
    }

    pub fn prints_as_image(&self, mode: PrintMode) -> bool {
        match mode {
            PrintMode::Auto => self.needs_image(),
            PrintMode::Text => false,
            PrintMode::Image => true,
        }
    }

    /// ESC/POS bytes: initialise, content, feed, cut.
    pub fn to_escpos(&self, mode: PrintMode) -> Vec<u8> {
        let mut p = EscPos::new();
        // A failure in the image path (shaping, fonts) must never cost a
        // receipt: it falls back to text, which every printer can print.
        let image = self
            .prints_as_image(mode)
            .then(|| std::panic::catch_unwind(|| raster::render(self)).ok())
            .flatten();
        if let Some(bands) = image {
            p.raster_bands(&bands);
        } else {
            self.text_escpos(&mut p);
        }
        p.feed(3).cut();
        p.into_bytes()
    }

    fn text_escpos(&self, p: &mut EscPos) {
        let width = columns_for_paper(self.paper_width_mm);
        for line in &self.lines {
            match line {
                Line::Logo => {
                    if let Some(logo) = &self.logo {
                        p.align(Align::Center).raster(logo);
                    }
                }
                Line::Text {
                    text,
                    align,
                    bold,
                    large,
                } => {
                    p.align(*align).bold(*bold).double(*large);
                    let per_line = if *large { width / 2 } else { width };
                    for part in wrap(text, per_line) {
                        p.line(&part);
                    }
                    p.bold(false).double(false);
                }
                Line::Row { left, right, bold } => {
                    p.align(Align::Left)
                        .bold(*bold)
                        .line(&two_columns(left, right, width))
                        .bold(false);
                }
                Line::Rule => {
                    p.align(Align::Left).line(&"-".repeat(width));
                }
            }
        }
    }

    /// The text-mode layout as plain text (tests, on-screen previews).
    pub fn to_text(&self) -> String {
        let width = columns_for_paper(self.paper_width_mm);
        let lead = |used: usize, align: Align| {
            let free = width.saturating_sub(used);
            match align {
                Align::Left => 0,
                Align::Center => free / 2,
                Align::Right => free,
            }
        };
        let mut out = String::new();
        for line in &self.lines {
            match line {
                Line::Logo => {
                    if self.logo.is_some() {
                        out.push_str("[logo]\n");
                    }
                }
                Line::Text {
                    text, align, large, ..
                } => {
                    // Double-width glyphs occupy two columns each.
                    let (per_line, cell) = if *large { (width / 2, 2) } else { (width, 1) };
                    for part in wrap(text, per_line) {
                        let used = part.chars().count() * cell;
                        out.push_str(&" ".repeat(lead(used, *align)));
                        out.push_str(part.trim_end());
                        out.push('\n');
                    }
                }
                Line::Row { left, right, .. } => {
                    out.push_str(two_columns(left, right, width).trim_end());
                    out.push('\n');
                }
                Line::Rule => {
                    out.push_str(&"-".repeat(width));
                    out.push('\n');
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(rtl: bool) -> Doc {
        let mut doc = Doc::new(58, rtl);
        doc.push(Line::large("Demo", Align::Center))
            .push(Line::Rule)
            .push(Line::row("Total", "1.500"));
        doc
    }

    #[test]
    fn text_layout_pads_rows_and_centres() {
        assert_eq!(
            sample(false).to_text(),
            format!(
                "            Demo\n{}\n{}\n",
                "-".repeat(32),
                two_columns("Total", "1.500", 32)
            )
        );
    }

    #[test]
    fn auto_mode_prints_text_until_it_cannot() {
        let latin = sample(false);
        assert!(!latin.needs_image());
        assert!(!latin.prints_as_image(PrintMode::Auto));
        assert!(latin.prints_as_image(PrintMode::Image));

        let mut arabic = sample(false);
        arabic.push(Line::row("قهوة", "1.500"));
        assert!(arabic.needs_image());
        assert!(
            sample(true).needs_image(),
            "right-to-left layouts need an image"
        );
        assert!(!arabic.prints_as_image(PrintMode::Text));
    }

    #[test]
    fn image_mode_sends_a_raster_and_a_cut() {
        let bytes = sample(true).to_escpos(PrintMode::Image);
        assert!(
            bytes.windows(4).any(|w| w == [0x1D, b'v', b'0', 0]),
            "raster"
        );
        assert!(bytes.ends_with(&[0x1D, b'V', 66, 3]));
    }

    #[test]
    fn wrap_splits_long_words() {
        assert_eq!(wrap("abcdefgh ij", 4), vec!["abcd", "efgh", "ij"]);
        assert_eq!(wrap("", 4), Vec::<String>::new());
        assert_eq!(wrap("  + ab cd", 6), vec!["  + ab", "  cd"]);
    }
}

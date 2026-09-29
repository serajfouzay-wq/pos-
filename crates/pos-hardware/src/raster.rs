//! Draws a [`Doc`] as a 1-bit image for `GS v 0`, with the bundled Tajawal
//! font (SIL Open Font License, `fonts/OFL.txt`; Arabic and Latin).
//!
//! Each line goes through the Unicode bidirectional algorithm
//! (`unicode-bidi`), each run is shaped (`rustybuzz`: Arabic joining forms,
//! ligatures, marks) and the glyphs are rasterised (`ab_glyph`) and
//! thresholded to black and white. The printer's own fonts never see the
//! text, so any script the font covers prints.
//!
//! Floating point here is pixel geometry only; no amount of money is ever
//! computed in this module.
#![allow(clippy::float_arithmetic, clippy::cast_precision_loss)]

use std::ops::Range;
use std::sync::OnceLock;

use ab_glyph::{point, Font, FontRef, GlyphId, PxScale, ScaleFont};
use rustybuzz::{Direction, UnicodeBuffer};
use unicode_bidi::{Level, ParagraphBidiInfo};

use crate::doc::{Doc, Line};
use crate::escpos::Align;
use crate::image::{dots_for_paper, MonoImage};

const REGULAR: &[u8] = include_bytes!("../fonts/Tajawal-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../fonts/Tajawal-Bold.ttf");

/// Blank dots kept at each side of the paper.
const MARGIN: f32 = 4.0;
/// Space between a row's label and its value.
const GAP: f32 = 12.0;

struct Face {
    shaper: rustybuzz::Face<'static>,
    glyphs: FontRef<'static>,
}

struct Fonts {
    regular: Face,
    bold: Face,
}

fn face(bytes: &'static [u8]) -> Face {
    Face {
        shaper: rustybuzz::Face::from_slice(bytes, 0).expect("bundled font parses"),
        glyphs: FontRef::try_from_slice(bytes).expect("bundled font parses"),
    }
}

fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| Fonts {
        regular: face(REGULAR),
        bold: face(BOLD),
    })
}

/// Text sizes in dots (the font's ascent-to-descent height).
fn text_size(paper_width_mm: u16, large: bool) -> f32 {
    let normal = if paper_width_mm >= 80 { 34.0 } else { 30.0 };
    if large {
        normal * 1.6
    } else {
        normal
    }
}

struct Placed {
    id: u16,
    x: f32,
    y: f32,
}

/// One line of text, shaped and ordered for display left to right.
struct Shaped {
    glyphs: Vec<Placed>,
    width: f32,
}

/// Bidi runs of one line in visual order, each with its direction.
fn visual_runs(text: &str, rtl: bool) -> Vec<(Range<usize>, bool)> {
    let base = if rtl { Level::rtl() } else { Level::ltr() };
    let info = ParagraphBidiInfo::new(text, Some(base));
    let (levels, runs) = info.visual_runs(0..text.len());
    runs.into_iter()
        .filter(|run| !run.is_empty())
        .map(|run| {
            let rtl = levels[run.start].is_rtl();
            (run, rtl)
        })
        .collect()
}

/// Hebrew, Arabic and their presentation forms: letters that read right to
/// left.
fn is_rtl_letter(c: char) -> bool {
    matches!(c, '\u{0590}'..='\u{08FF}' | '\u{FB1D}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}')
}

/// A line reads in the direction of its first letter (like `dir="auto"` on
/// the web): an Arabic product name on an English receipt still reads right
/// to left, and a line with no letters at all (an amount, a date,
/// `2 x 1.250`) keeps left-to-right order on an Arabic receipt, so `-0.100`
/// never turns into `0.100-`. Where a line sits is the document's business.
fn reads_rtl(text: &str) -> bool {
    text.chars()
        .find(|c| c.is_alphabetic())
        .is_some_and(is_rtl_letter)
}

fn shape(text: &str, bold: bool, size: f32) -> Shaped {
    let rtl = reads_rtl(text);
    let face = if bold {
        &fonts().bold
    } else {
        &fonts().regular
    };
    let scale = face.glyphs.as_scaled(PxScale::from(size)).h_scale_factor();
    let mut glyphs = Vec::new();
    let mut pen = 0.0;
    for (run, run_rtl) in visual_runs(text, rtl) {
        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(&text[run]);
        buffer.set_direction(if run_rtl {
            Direction::RightToLeft
        } else {
            Direction::LeftToRight
        });
        buffer.guess_segment_properties();
        let output = rustybuzz::shape(&face.shaper, &[], buffer);
        for (info, pos) in output.glyph_infos().iter().zip(output.glyph_positions()) {
            glyphs.push(Placed {
                id: u16::try_from(info.glyph_id).unwrap_or(0),
                x: pen + pos.x_offset as f32 * scale,
                y: -(pos.y_offset as f32) * scale,
            });
            pen += pos.x_advance as f32 * scale;
        }
    }
    Shaped { glyphs, width: pen }
}

fn measure(text: &str, bold: bool, size: f32) -> f32 {
    shape(text, bold, size).width
}

/// Word wrap by measured width; a word wider than the line is split by
/// characters.
fn wrap_px(text: &str, bold: bool, size: f32, max: f32) -> Vec<String> {
    let body = text.trim_start();
    let indent = &text[..text.len() - body.len()];
    if !indent.is_empty() {
        let room = max - measure(indent, bold, size);
        if room > max / 2.0 {
            return wrap_px(body, bold, size, room)
                .into_iter()
                .map(|line| format!("{indent}{line}"))
                .collect();
        }
    }
    let fits = |s: &str| measure(s, bold, size) <= max;
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_owned()
        } else {
            format!("{current} {word}")
        };
        if fits(&candidate) {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        if fits(word) {
            current = word.to_owned();
            continue;
        }
        for c in word.chars() {
            let mut next = current.clone();
            next.push(c);
            if !current.is_empty() && !fits(&next) {
                lines.push(std::mem::replace(&mut current, c.to_string()));
            } else {
                current = next;
            }
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// A grayscale-free canvas: one bool per dot, grown line by line.
struct Canvas {
    width: usize,
    dots: Vec<bool>,
}

impl Canvas {
    fn height(&self) -> usize {
        self.dots.len() / self.width
    }

    fn grow(&mut self, rows: usize) -> usize {
        let top = self.height();
        self.dots.resize(self.dots.len() + rows * self.width, false);
        top
    }

    fn set(&mut self, x: i64, y: i64) {
        let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
            return;
        };
        if x < self.width && y < self.height() {
            self.dots[y * self.width + x] = true;
        }
    }

    /// Draws a shaped line with its left edge at `left` and its baseline at
    /// `baseline`.
    fn draw(&mut self, shaped: &Shaped, bold: bool, size: f32, left: f32, baseline: f32) {
        let face = if bold {
            &fonts().bold
        } else {
            &fonts().regular
        };
        for glyph in &shaped.glyphs {
            let positioned = GlyphId(glyph.id)
                .with_scale_and_position(size, point(left + glyph.x, baseline + glyph.y));
            if let Some(outline) = face.glyphs.outline_glyph(positioned) {
                let bounds = outline.px_bounds();
                let (x0, y0) = (bounds.min.x as i64, bounds.min.y as i64);
                outline.draw(|x, y, coverage| {
                    if coverage >= 0.5 {
                        self.set(x0 + i64::from(x), y0 + i64::from(y));
                    }
                });
            }
        }
    }

    fn blit(&mut self, image: &MonoImage, left: usize, top: usize) {
        let row = image.bytes_per_row();
        for y in 0..image.height {
            for x in 0..image.width {
                if image.data[y * row + x / 8] & (0x80 >> (x % 8)) != 0 {
                    self.set((left + x) as i64, (top + y) as i64);
                }
            }
        }
    }

    fn into_image(self) -> MonoImage {
        let height = self.height();
        let row = self.width.div_ceil(8);
        let mut data = vec![0u8; row * height];
        for (i, dot) in self.dots.iter().enumerate() {
            if *dot {
                let (x, y) = (i % self.width, i / self.width);
                data[y * row + x / 8] |= 0x80 >> (x % 8);
            }
        }
        MonoImage {
            width: self.width,
            height,
            data,
        }
    }
}

/// Where a line of `width` dots starts for `align` (start = right when the
/// document is right-to-left).
fn start_x(align: Align, rtl: bool, width: f32, paper: f32) -> f32 {
    let align = match (align, rtl) {
        (Align::Left, true) => Align::Right,
        (Align::Right, true) => Align::Left,
        (other, _) => other,
    };
    match align {
        Align::Left => MARGIN,
        Align::Center => ((paper - width) / 2.0).max(MARGIN),
        Align::Right => (paper - MARGIN - width).max(MARGIN),
    }
}

/// Line height and baseline offset for a text size.
fn metrics(bold: bool, size: f32) -> (usize, f32) {
    let face = if bold {
        &fonts().bold
    } else {
        &fonts().regular
    };
    let scaled = face.glyphs.as_scaled(PxScale::from(size));
    let height = (scaled.height() + scaled.line_gap()).ceil().max(1.0);
    (height as usize, scaled.ascent())
}

/// The whole document as one image, `dots_for_paper` wide.
pub fn render(doc: &Doc) -> MonoImage {
    let paper_dots = dots_for_paper(doc.paper_width_mm);
    let paper = paper_dots as f32;
    let usable = paper - 2.0 * MARGIN;
    let mut canvas = Canvas {
        width: paper_dots,
        dots: Vec::new(),
    };
    canvas.grow(6);
    for line in &doc.lines {
        match line {
            Line::Logo => {
                if let Some(logo) = &doc.logo {
                    let top = canvas.grow(logo.height + 8);
                    let left = paper_dots.saturating_sub(logo.width) / 2;
                    canvas.blit(logo, left, top);
                }
            }
            Line::Text {
                text,
                align,
                bold,
                large,
            } => {
                let size = text_size(doc.paper_width_mm, *large);
                let (height, ascent) = metrics(*bold, size);
                for part in wrap_px(text, *bold, size, usable) {
                    let shaped = shape(&part, *bold, size);
                    let top = canvas.grow(height) as f32;
                    let left = start_x(*align, doc.rtl, shaped.width, paper);
                    canvas.draw(&shaped, *bold, size, left, top + ascent);
                }
            }
            Line::Row { left, right, bold } => {
                let size = text_size(doc.paper_width_mm, false);
                let (height, ascent) = metrics(*bold, size);
                let value = shape(right, *bold, size);
                let room = (usable - value.width - GAP).max(usable / 3.0);
                let labels = wrap_px(left, *bold, size, room);
                let labels = if labels.is_empty() {
                    vec![String::new()]
                } else {
                    labels
                };
                for (i, part) in labels.iter().enumerate() {
                    let label = shape(part, *bold, size);
                    let top = canvas.grow(height) as f32;
                    let baseline = top + ascent;
                    let label_x = start_x(Align::Left, doc.rtl, label.width, paper);
                    canvas.draw(&label, *bold, size, label_x, baseline);
                    if i == 0 {
                        let value_x = start_x(Align::Right, doc.rtl, value.width, paper);
                        canvas.draw(&value, *bold, size, value_x, baseline);
                    }
                }
            }
            Line::Rule => {
                let top = canvas.grow(14);
                let mut x = MARGIN as usize;
                while x + 6 < paper_dots - MARGIN as usize {
                    for dx in 0..6 {
                        for dy in 6..8 {
                            canvas.set((x + dx) as i64, (top + dy) as i64);
                        }
                    }
                    x += 10;
                }
            }
        }
    }
    canvas.grow(8);
    canvas.into_image()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Line;

    fn ink(image: &MonoImage) -> usize {
        image.data.iter().map(|b| b.count_ones() as usize).sum()
    }

    /// The x range holding ink in rows `rows`.
    fn ink_span(image: &MonoImage, rows: Range<usize>) -> Option<(usize, usize)> {
        let row = image.bytes_per_row();
        let mut span: Option<(usize, usize)> = None;
        for y in rows {
            for x in 0..image.width {
                if image.data[y * row + x / 8] & (0x80 >> (x % 8)) != 0 {
                    span = Some(span.map_or((x, x), |(a, b)| (a.min(x), b.max(x))));
                }
            }
        }
        span
    }

    #[test]
    fn arabic_is_shaped_into_joined_forms() {
        // Joined (initial/medial/final) forms are different glyphs from the
        // isolated letters, so shaping the word changes the glyph ids.
        let word = shape("قهوة", false, 30.0);
        let isolated: Vec<u16> = "ق ه و ة"
            .split(' ')
            .map(|c| shape(c, false, 30.0).glyphs[0].id)
            .collect();
        let joined: Vec<u16> = word.glyphs.iter().map(|g| g.id).collect();
        assert_eq!(joined.len(), 4);
        assert_ne!(
            joined.iter().rev().copied().collect::<Vec<_>>(),
            isolated,
            "letters are joined"
        );
        assert!(word.width > 0.0);
    }

    #[test]
    fn right_to_left_rows_put_the_label_on_the_right() {
        let mut doc = Doc::new(80, true);
        doc.push(Line::row("الإجمالي", "12.500"));
        let image = render(&doc);
        assert_eq!(image.width, 576);
        let (left, right) = ink_span(&image, 0..image.height).expect("ink");
        assert!(left < 150, "value at the left edge: {left}");
        assert!(right > 450, "label at the right edge: {right}");

        let mut ltr = Doc::new(80, false);
        ltr.push(Line::row("Total", "12.500"));
        let image = render(&ltr);
        let (left, right) = ink_span(&image, 0..image.height).expect("ink");
        assert!(left < 20 && right > 500);
    }

    #[test]
    fn lines_read_in_the_direction_of_their_first_letter() {
        for text in ["-0.100", "2026-09-23 10:15", "  2 x 1.250", "TOTAL LYD"] {
            assert!(!reads_rtl(text), "{text}");
        }
        for text in ["  + حليب شوفان", "الإجمالي LYD", "خصم 5"] {
            assert!(reads_rtl(text), "{text}");
        }
    }

    #[test]
    fn long_text_wraps_inside_the_paper() {
        let mut doc = Doc::new(58, true);
        doc.push(Line::text(
            "شكراً لزيارتكم، نتمنى لكم يوماً سعيداً ونراكم قريباً في فرعنا الجديد",
            Align::Center,
        ));
        let image = render(&doc);
        let (one_line, _) = metrics(false, text_size(58, false));
        assert!(image.height > 2 * one_line, "wrapped onto several lines");
        let (left, right) = ink_span(&image, 0..image.height).expect("ink");
        assert!(left >= 2 && right < 384 - 2);
    }

    #[test]
    fn mixed_text_and_numbers_render_and_bold_is_heavier() {
        let mut regular = Doc::new(58, true);
        regular.push(Line::text("قهوة لاتيه x 2", Align::Left));
        let mut bold = Doc::new(58, true);
        bold.push(Line::bold("قهوة لاتيه x 2", Align::Left));
        assert!(ink(&render(&bold)) > ink(&render(&regular)));
    }
}

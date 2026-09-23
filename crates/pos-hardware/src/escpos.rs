//! ESC/POS command encoder (Epson-compatible subset understood by virtually
//! every thermal receipt printer).

use crate::image::MonoImage;

/// Pulse drawer pin 2 for 25 × 2 ms on, 25 × 2 ms off (per the spec).
pub const DRAWER_KICK: [u8; 5] = [0x1B, 0x70, 0x00, 0x19, 0x19];

const ESC: u8 = 0x1B;
const GS: u8 = 0x1D;
const LF: u8 = 0x0A;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Maps text to Windows-1252 (ESC t 16). Characters outside it become `?`.
///
/// Arabic and other non-Latin scripts need raster rendering; see
/// `docs/ARCHITECTURE.md` (D24).
pub fn encode_cp1252(text: &str) -> Vec<u8> {
    text.chars()
        .filter(|c| !c.is_control())
        .map(|c| match c {
            ' '..='~' => c as u8,
            '\u{A0}'..='\u{FF}' => u8::try_from(u32::from(c)).unwrap_or(b'?'),
            '€' => 0x80,
            '‘' | '’' => b'\'',
            '“' | '”' => b'"',
            '–' | '—' => b'-',
            '…' => 0x85,
            _ => b'?',
        })
        .collect()
}

#[derive(Debug, Default)]
pub struct EscPos {
    buf: Vec<u8>,
}

impl EscPos {
    /// Initialises the printer and selects code page WPC1252.
    pub fn new() -> Self {
        let mut p = Self {
            buf: Vec::with_capacity(2048),
        };
        p.buf.extend_from_slice(&[ESC, b'@', ESC, b't', 16]);
        p
    }

    pub fn align(&mut self, align: Align) -> &mut Self {
        let n = match align {
            Align::Left => 0,
            Align::Center => 1,
            Align::Right => 2,
        };
        self.buf.extend_from_slice(&[ESC, b'a', n]);
        self
    }

    pub fn bold(&mut self, on: bool) -> &mut Self {
        self.buf.extend_from_slice(&[ESC, b'E', u8::from(on)]);
        self
    }

    /// Double width + height.
    pub fn double(&mut self, on: bool) -> &mut Self {
        self.buf
            .extend_from_slice(&[GS, b'!', if on { 0x11 } else { 0x00 }]);
        self
    }

    pub fn line(&mut self, text: &str) -> &mut Self {
        self.buf.extend(encode_cp1252(text));
        self.buf.push(LF);
        self
    }

    pub fn feed(&mut self, lines: u8) -> &mut Self {
        self.buf.extend_from_slice(&[ESC, b'd', lines]);
        self
    }

    /// `GS v 0` raster bit image.
    pub fn raster(&mut self, image: &MonoImage) -> &mut Self {
        let bytes_per_row = image.bytes_per_row();
        let [xl, xh] = u16::try_from(bytes_per_row)
            .unwrap_or(u16::MAX)
            .to_le_bytes();
        let [yl, yh] = u16::try_from(image.height)
            .unwrap_or(u16::MAX)
            .to_le_bytes();
        self.buf
            .extend_from_slice(&[GS, b'v', b'0', 0, xl, xh, yl, yh]);
        self.buf.extend_from_slice(&image.data);
        self
    }

    /// Feed to the cutter and partial-cut.
    pub fn cut(&mut self) -> &mut Self {
        self.buf.extend_from_slice(&[GS, b'V', 66, 3]);
        self
    }

    pub fn raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(bytes);
        self
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_latin_and_replaces_the_rest() {
        assert_eq!(encode_cp1252("Café €5"), b"Caf\xe9 \x805".to_vec());
        assert_eq!(encode_cp1252("قهوة x"), b"???? x".to_vec());
        assert_eq!(
            encode_cp1252("a\nb\x1b"),
            b"ab".to_vec(),
            "no control injection"
        );
    }

    #[test]
    fn frames_a_document() {
        let mut p = EscPos::new();
        p.line("hi").cut();
        let bytes = p.into_bytes();
        assert!(bytes.starts_with(&[0x1B, b'@']));
        assert!(bytes.ends_with(&[0x1D, b'V', 66, 3]));
    }
}

//! Logo rasterisation for `GS v 0`: PNG → grayscale → scaled to the paper's
//! dot width → Floyd–Steinberg dithered 1-bit image. Integer arithmetic only.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonoImage {
    pub width: usize,
    pub height: usize,
    /// Row-major, MSB first, 1 = black, rows padded to whole bytes.
    pub data: Vec<u8>,
}

impl MonoImage {
    pub fn bytes_per_row(&self) -> usize {
        self.width.div_ceil(8)
    }

    /// 1-bit grayscale PNG of exactly what the printer receives (on-screen
    /// preview in the generator).
    pub fn to_png(&self) -> Result<Vec<u8>, ImageError> {
        let mut out = Vec::new();
        {
            let width = u32::try_from(self.width).map_err(|e| ImageError::Encode(e.to_string()))?;
            let height =
                u32::try_from(self.height).map_err(|e| ImageError::Encode(e.to_string()))?;
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::One);
            let mut writer = encoder
                .write_header()
                .map_err(|e| ImageError::Encode(e.to_string()))?;
            // PNG grayscale: 1 = white; ours: 1 = black.
            let inverted: Vec<u8> = self.data.iter().map(|b| !b).collect();
            writer
                .write_image_data(&inverted)
                .map_err(|e| ImageError::Encode(e.to_string()))?;
        }
        Ok(out)
    }
}

/// Width and height of a PNG without decoding the pixels.
pub fn png_dimensions(bytes: &[u8]) -> Result<(u32, u32), ImageError> {
    let reader = png::Decoder::new(bytes)
        .read_info()
        .map_err(|e| ImageError::Decode(e.to_string()))?;
    let info = reader.info();
    Ok((info.width, info.height))
}

#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("logo is not a readable PNG: {0}")]
    Decode(String),
    #[error("logo is empty")]
    Empty,
    #[error("cannot encode preview: {0}")]
    Encode(String),
}

/// Printable dot width: 384 dots on 58 mm paper, 576 on 80 mm (203 dpi heads).
pub fn dots_for_paper(paper_width_mm: u16) -> usize {
    if paper_width_mm >= 80 {
        576
    } else {
        384
    }
}

/// Decodes a PNG and converts it for printing at most `max_width` dots wide.
pub fn logo_from_png(bytes: &[u8], max_width: usize) -> Result<MonoImage, ImageError> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|e| ImageError::Decode(e.to_string()))?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| ImageError::Decode(e.to_string()))?;
    let (w, h) = (info.width as usize, info.height as usize);
    if w == 0 || h == 0 {
        return Err(ImageError::Empty);
    }
    let channels = info.color_type.samples();

    // Luma with alpha composited onto white paper; integer BT.601 weights.
    let luma: Vec<i32> = buf
        .chunks_exact(channels)
        .take(w * h)
        .map(|px| {
            let (r, g, b, a) = match channels {
                1 => (px[0], px[0], px[0], 255),
                2 => (px[0], px[0], px[0], px[1]),
                3 => (px[0], px[1], px[2], 255),
                _ => (px[0], px[1], px[2], px[3]),
            };
            let y = (299 * i32::from(r) + 587 * i32::from(g) + 114 * i32::from(b)) / 1000;
            let a = i32::from(a);
            (y * a + 255 * (255 - a)) / 255
        })
        .collect();

    // Nearest-neighbour downscale to fit the paper.
    let out_w = w.min(max_width);
    let out_h = (h * out_w).div_ceil(w).max(1);
    let mut pixels: Vec<i32> = (0..out_h)
        .flat_map(|y| {
            let luma = &luma;
            (0..out_w).map(move |x| luma[(y * h / out_h) * w + x * w / out_w])
        })
        .collect();

    // Floyd–Steinberg (error in 1/16ths).
    let stride = out_w.div_ceil(8);
    let mut data = vec![0u8; stride * out_h];
    for y in 0..out_h {
        for x in 0..out_w {
            let i = y * out_w + x;
            let old = pixels[i];
            let black = old < 128;
            let error = old - if black { 0 } else { 255 };
            if black {
                data[y * stride + x / 8] |= 0x80 >> (x % 8);
            }
            let mut spread = |dx: isize, dy: usize, weight: i32| {
                let nx = x as isize + dx;
                if nx >= 0 && (nx as usize) < out_w && y + dy < out_h {
                    pixels[(y + dy) * out_w + nx as usize] += error * weight / 16;
                }
            };
            spread(1, 0, 7);
            spread(-1, 1, 3);
            spread(0, 1, 5);
            spread(1, 1, 1);
        }
    }
    Ok(MonoImage {
        width: out_w,
        height: out_h,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("header");
        writer.write_image_data(rgba).expect("data");
        drop(writer);
        out
    }

    #[test]
    fn black_and_white_survive_and_transparency_is_paper() {
        // 2×1: opaque black, fully transparent.
        let img = logo_from_png(&png(2, 1, &[0, 0, 0, 255, 0, 0, 0, 0]), 576).expect("decodes");
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(img.data, vec![0b1000_0000]);
    }

    #[test]
    fn wide_images_are_scaled_to_the_paper() {
        let img = logo_from_png(&png(1000, 10, &vec![0; 1000 * 10 * 4]), 384).expect("decodes");
        assert_eq!(img.width, 384);
        assert_eq!(img.height, 4);
        assert_eq!(img.data.len(), 48 * 4);
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(logo_from_png(b"not a png", 384).is_err());
    }

    #[test]
    fn preview_png_round_trips_the_printed_dots() {
        let source = png(2, 1, &[0, 0, 0, 255, 0, 0, 0, 0]);
        assert_eq!(png_dimensions(&source).expect("dims"), (2, 1));
        let img = logo_from_png(&source, 576).expect("decodes");
        let preview = img.to_png().expect("encode");
        assert_eq!(png_dimensions(&preview).expect("dims"), (2, 1));
        // Decoding the preview gives the same dots back.
        assert_eq!(logo_from_png(&preview, 576).expect("decode preview"), img);
    }
}

//! Image *export* — the reverse of ingestion. Turns the decoded sample plane of
//! a PDF image XObject into a standalone PNG file (used by `pdf::extract_images`).
//!
//! JPEG (`DCTDecode`) data is already a complete JPEG file, so the caller writes
//! it out verbatim and it never passes through here. Everything else arrives as
//! the fully decoded, de-predicted, row-major sample plane — which is byte-for-
//! byte what the `png` crate wants (sub-byte depths packed MSB-first, 16-bit
//! samples big-endian), so we hand it straight to the encoder.

use crate::ImageError;

/// How to interpret the decoded `samples` passed to [`encode_png`].
#[derive(Debug, Clone)]
pub enum PngColor {
    /// One component per pixel.
    Gray,
    /// Three components per pixel (R, G, B).
    Rgb,
    /// Four components per pixel (C, M, Y, K); converted to 8-bit RGB on export.
    Cmyk,
    /// One palette index per pixel into an RGB lookup table (`palette.len()` is a
    /// multiple of 3).
    Indexed { palette: Vec<u8> },
}

/// A grayscale alpha plane (an image's `/SMask`), to be merged into the PNG.
#[derive(Debug, Clone)]
pub struct AlphaPlane {
    pub width: u32,
    pub height: u32,
    pub bits: u8,
    pub samples: Vec<u8>,
}

/// Encode a decoded image plane as a complete PNG file.
///
/// `alpha` is merged into the output only when it lines up cleanly (matching
/// 8-bit dimensions); otherwise it is dropped and an opaque image is produced.
pub fn encode_png(
    width: u32,
    height: u32,
    bits: u8,
    color: PngColor,
    samples: &[u8],
    alpha: Option<AlphaPlane>,
) -> Result<Vec<u8>, ImageError> {
    match color {
        PngColor::Gray => encode_plane(
            width,
            height,
            bits,
            png::ColorType::Grayscale,
            samples,
            alpha,
        ),
        PngColor::Rgb => encode_plane(width, height, bits, png::ColorType::Rgb, samples, alpha),
        PngColor::Cmyk => {
            if bits != 8 {
                return Err(ImageError::Format(
                    "only 8-bit CMYK images can be exported".into(),
                ));
            }
            let rgb = cmyk_to_rgb(samples);
            encode_plane(width, height, 8, png::ColorType::Rgb, &rgb, alpha)
        }
        // Alpha on palette images is uncommon and dropped here.
        PngColor::Indexed { palette } => {
            let mut out = Vec::new();
            {
                let mut enc = png::Encoder::new(&mut out, width, height);
                enc.set_color(png::ColorType::Indexed);
                enc.set_depth(depth(bits)?);
                enc.set_palette(palette);
                let mut writer = enc.write_header().map_err(fmt_err)?;
                writer.write_image_data(samples).map_err(fmt_err)?;
                writer.finish().map_err(fmt_err)?;
            }
            Ok(out)
        }
    }
}

/// Encode a gray/RGB plane, interleaving alpha into Gray+A / RGBA when possible.
fn encode_plane(
    width: u32,
    height: u32,
    bits: u8,
    base: png::ColorType,
    samples: &[u8],
    alpha: Option<AlphaPlane>,
) -> Result<Vec<u8>, ImageError> {
    let channels = match base {
        png::ColorType::Grayscale => 1,
        png::ColorType::Rgb => 3,
        _ => return Err(ImageError::Format("unsupported base color type".into())),
    };

    let (color_type, data) = match alpha {
        Some(a) if bits == 8 && a.bits == 8 && a.width == width && a.height == height => {
            let merged = interleave(samples, &a.samples, channels);
            let with_alpha = if channels == 1 {
                png::ColorType::GrayscaleAlpha
            } else {
                png::ColorType::Rgba
            };
            (with_alpha, merged)
        }
        _ => (base, samples.to_vec()),
    };

    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(color_type);
        enc.set_depth(depth(bits)?);
        let mut writer = enc.write_header().map_err(fmt_err)?;
        writer.write_image_data(&data).map_err(fmt_err)?;
        writer.finish().map_err(fmt_err)?;
    }
    Ok(out)
}

/// Append a trailing alpha byte to each `channels`-byte pixel.
fn interleave(color: &[u8], alpha: &[u8], channels: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(alpha.len() * (channels + 1));
    for (i, &a) in alpha.iter().enumerate() {
        let off = i * channels;
        if off + channels > color.len() {
            break;
        }
        out.extend_from_slice(&color[off..off + channels]);
        out.push(a);
    }
    out
}

/// Convert an 8-bit CMYK plane to an 8-bit RGB plane (naive, no ICC).
fn cmyk_to_rgb(samples: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() / 4 * 3);
    for px in samples.chunks_exact(4) {
        out.extend_from_slice(&cmyk_pixel(px));
    }
    out
}

/// Convert a single CMYK pixel (4 bytes) to RGB (3 bytes).
pub fn cmyk_pixel(px: &[u8]) -> [u8; 3] {
    let c = px[0] as f32 / 255.0;
    let m = px[1] as f32 / 255.0;
    let y = px[2] as f32 / 255.0;
    let k = px[3] as f32 / 255.0;
    [
        (255.0 * (1.0 - c) * (1.0 - k)).round() as u8,
        (255.0 * (1.0 - m) * (1.0 - k)).round() as u8,
        (255.0 * (1.0 - y) * (1.0 - k)).round() as u8,
    ]
}

fn depth(bits: u8) -> Result<png::BitDepth, ImageError> {
    Ok(match bits {
        1 => png::BitDepth::One,
        2 => png::BitDepth::Two,
        4 => png::BitDepth::Four,
        8 => png::BitDepth::Eight,
        16 => png::BitDepth::Sixteen,
        _ => return Err(ImageError::Format(format!("unsupported bit depth {bits}"))),
    })
}

fn fmt_err(e: png::EncodingError) -> ImageError {
    ImageError::Format(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode a PNG back to (width, height, color type, raw samples) for checks.
    fn decode(data: &[u8]) -> (u32, u32, png::ColorType, Vec<u8>) {
        let mut dec = png::Decoder::new(std::io::Cursor::new(data));
        dec.set_transformations(png::Transformations::IDENTITY);
        let mut reader = dec.read_info().unwrap();
        let mut buf = vec![0u8; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut buf).unwrap();
        buf.truncate(frame.buffer_size());
        (frame.width, frame.height, frame.color_type, buf)
    }

    #[test]
    fn rgb_roundtrip() {
        let samples = [255u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0]; // 2x2
        let png = encode_png(2, 2, 8, PngColor::Rgb, &samples, None).unwrap();
        let (w, h, ct, out) = decode(&png);
        assert_eq!((w, h), (2, 2));
        assert_eq!(ct, png::ColorType::Rgb);
        assert_eq!(out, samples);
    }

    #[test]
    fn gray_with_alpha_merges() {
        let gray = [10u8, 20, 30, 40]; // 2x2
        let alpha = AlphaPlane {
            width: 2,
            height: 2,
            bits: 8,
            samples: vec![255, 128, 64, 0],
        };
        let png = encode_png(2, 2, 8, PngColor::Gray, &gray, Some(alpha)).unwrap();
        let (_, _, ct, out) = decode(&png);
        assert_eq!(ct, png::ColorType::GrayscaleAlpha);
        assert_eq!(out, [10, 255, 20, 128, 30, 64, 40, 0]);
    }

    #[test]
    fn cmyk_converts_to_rgb() {
        // Pure cyan and pure black.
        let samples = [255u8, 0, 0, 0, 0, 0, 0, 255]; // 2x1
        let png = encode_png(2, 1, 8, PngColor::Cmyk, &samples, None).unwrap();
        let (_, _, ct, out) = decode(&png);
        assert_eq!(ct, png::ColorType::Rgb);
        assert_eq!(&out[0..3], &[0, 255, 255]); // cyan
        assert_eq!(&out[3..6], &[0, 0, 0]); // black
    }

    #[test]
    fn indexed_keeps_palette() {
        let palette = vec![255u8, 0, 0, 0, 255, 0]; // red, green
        let indices = [0u8, 1, 1, 0]; // 2x2
        let png = encode_png(2, 2, 8, PngColor::Indexed { palette }, &indices, None).unwrap();
        let (_, _, ct, out) = decode(&png);
        assert_eq!(ct, png::ColorType::Indexed);
        assert_eq!(out, indices);
    }
}

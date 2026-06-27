//! PNG ingestion (Fase 4.3/4.4): decode with the `png` crate and re-encode the
//! sample planes with `FlateDecode`. Palette → `Indexed`; alpha channels and
//! palette `tRNS` → a grayscale soft mask; 8- and 16-bit depths supported.

use crate::{flate_encode, ColorSpace, Filter, Image, ImageError, SoftMask};

pub(crate) fn from_png(data: &[u8]) -> Result<Image, ImageError> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    // IDENTITY: keep palette indices packed and 16-bit samples big-endian, which
    // is exactly what PDF image data wants — no expansion.
    decoder.set_transformations(png::Transformations::IDENTITY);

    let mut reader = decoder
        .read_info()
        .map_err(|e| ImageError::Format(e.to_string()))?;

    // Snapshot palette/tRNS (header data) before the mutable decode borrow.
    let (palette, trns) = {
        let info = reader.info();
        (
            info.palette.as_ref().map(|c| c.to_vec()),
            info.trns.as_ref().map(|c| c.to_vec()),
        )
    };

    let buf_size = reader
        .output_buffer_size()
        .ok_or_else(|| ImageError::Format("png output buffer size overflow".into()))?;
    let mut buf = vec![0u8; buf_size];
    let frame = reader
        .next_frame(&mut buf)
        .map_err(|e| ImageError::Format(e.to_string()))?;

    let width = frame.width;
    let height = frame.height;
    let bpc = bit_depth_to_u8(frame.bit_depth);
    let bps = if bpc == 16 { 2 } else { 1 }; // bytes per sample for split paths
    let raw = &buf[..frame.line_size * height as usize];

    let (color_space, data, soft_mask) = match frame.color_type {
        png::ColorType::Grayscale => (ColorSpace::DeviceGray, flate_encode(raw), None),
        png::ColorType::Rgb => (ColorSpace::DeviceRgb, flate_encode(raw), None),

        png::ColorType::GrayscaleAlpha => {
            let (gray, alpha) = split_alpha(raw, 1, bps);
            let mask = SoftMask {
                width,
                height,
                bits_per_component: bpc,
                data: flate_encode(&alpha),
            };
            (ColorSpace::DeviceGray, flate_encode(&gray), Some(mask))
        }

        png::ColorType::Rgba => {
            let (rgb, alpha) = split_alpha(raw, 3, bps);
            let mask = SoftMask {
                width,
                height,
                bits_per_component: bpc,
                data: flate_encode(&alpha),
            };
            (ColorSpace::DeviceRgb, flate_encode(&rgb), Some(mask))
        }

        png::ColorType::Indexed => {
            let palette = palette
                .ok_or_else(|| ImageError::Format("indexed PNG without a palette".into()))?;
            let hival = ((palette.len() / 3).saturating_sub(1)).min(255) as u8;
            let cs = ColorSpace::Indexed {
                base: Box::new(ColorSpace::DeviceRgb),
                hival,
                lookup: palette,
            };

            // Palette transparency (tRNS): build an 8-bit alpha plane. Only the
            // common 8-bit-index case is mapped; sub-byte indices stay opaque.
            let mask = match (&trns, bpc) {
                (Some(trns), 8) => {
                    let alpha: Vec<u8> = index_rows(raw, width, height)
                        .map(|idx| trns.get(idx as usize).copied().unwrap_or(255))
                        .collect();
                    Some(SoftMask {
                        width,
                        height,
                        bits_per_component: 8,
                        data: flate_encode(&alpha),
                    })
                }
                _ => None,
            };
            (cs, flate_encode(raw), mask)
        }
    };

    Ok(Image {
        width,
        height,
        bits_per_component: bpc,
        color_space,
        filter: Filter::FlateDecode,
        data,
        soft_mask,
        decode: None,
    })
}

fn bit_depth_to_u8(bd: png::BitDepth) -> u8 {
    match bd {
        png::BitDepth::One => 1,
        png::BitDepth::Two => 2,
        png::BitDepth::Four => 4,
        png::BitDepth::Eight => 8,
        png::BitDepth::Sixteen => 16,
    }
}

/// Split interleaved samples into a color plane and an alpha plane.
///
/// `color_channels` is the number of non-alpha channels (1 gray, 3 RGB); `bps`
/// is bytes per sample (1 for 8-bit, 2 for 16-bit). Alpha is the last channel.
fn split_alpha(data: &[u8], color_channels: usize, bps: usize) -> (Vec<u8>, Vec<u8>) {
    let stride = (color_channels + 1) * bps;
    let pixels = data.len() / stride;
    let mut color = Vec::with_capacity(pixels * color_channels * bps);
    let mut alpha = Vec::with_capacity(pixels * bps);
    for px in data.chunks_exact(stride) {
        let (c, a) = px.split_at(color_channels * bps);
        color.extend_from_slice(c);
        alpha.extend_from_slice(a);
    }
    (color, alpha)
}

/// Iterate 8-bit palette indices row by row (8-bit indices are byte-aligned).
fn index_rows(data: &[u8], width: u32, height: u32) -> impl Iterator<Item = u8> + '_ {
    let w = width as usize;
    let row_stride = data.len() / height.max(1) as usize;
    (0..height as usize).flat_map(move |y| {
        let start = y * row_stride;
        data[start..start + w].iter().copied()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode a tiny PNG with the `png` crate for round-trip testing.
    fn encode_png(
        width: u32,
        height: u32,
        color: png::ColorType,
        depth: png::BitDepth,
        data: &[u8],
        palette: Option<&[u8]>,
        trns: Option<&[u8]>,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, width, height);
            enc.set_color(color);
            enc.set_depth(depth);
            if let Some(p) = palette {
                enc.set_palette(p.to_vec());
            }
            if let Some(t) = trns {
                enc.set_trns(t.to_vec());
            }
            let mut writer = enc.write_header().unwrap();
            writer.write_image_data(data).unwrap();
        }
        out
    }

    #[test]
    fn opaque_rgb() {
        let pixels = [255u8, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0]; // 2x2 RGB
        let png = encode_png(
            2,
            2,
            png::ColorType::Rgb,
            png::BitDepth::Eight,
            &pixels,
            None,
            None,
        );
        let img = from_png(&png).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(img.color_space, ColorSpace::DeviceRgb);
        assert_eq!(img.bits_per_component, 8);
        assert!(img.soft_mask.is_none());
        assert_eq!(img.filter, Filter::FlateDecode);
    }

    #[test]
    fn rgba_splits_into_smask() {
        let pixels = [255u8, 0, 0, 128, 0, 255, 0, 64]; // 2x1 RGBA
        let png = encode_png(
            2,
            1,
            png::ColorType::Rgba,
            png::BitDepth::Eight,
            &pixels,
            None,
            None,
        );
        let img = from_png(&png).unwrap();
        assert_eq!(img.color_space, ColorSpace::DeviceRgb);
        let mask = img.soft_mask.expect("alpha -> smask");
        assert_eq!((mask.width, mask.height), (2, 1));
        assert_eq!(mask.bits_per_component, 8);
    }

    #[test]
    fn palette_becomes_indexed() {
        let palette = [255u8, 0, 0, 0, 255, 0]; // 2 entries
        let indices = [0u8, 1, 1, 0]; // 2x2
        let png = encode_png(
            2,
            2,
            png::ColorType::Indexed,
            png::BitDepth::Eight,
            &indices,
            Some(&palette),
            None,
        );
        let img = from_png(&png).unwrap();
        match img.color_space {
            ColorSpace::Indexed {
                hival, ref lookup, ..
            } => {
                assert_eq!(hival, 1);
                assert_eq!(lookup.len(), 6);
            }
            other => panic!("expected Indexed, got {other:?}"),
        }
    }

    #[test]
    fn palette_trns_builds_alpha() {
        let palette = [255u8, 0, 0, 0, 255, 0];
        let trns = [0u8, 255]; // index 0 transparent, index 1 opaque
        let indices = [0u8, 1, 1, 0];
        let png = encode_png(
            2,
            2,
            png::ColorType::Indexed,
            png::BitDepth::Eight,
            &indices,
            Some(&palette),
            Some(&trns),
        );
        let img = from_png(&png).unwrap();
        assert!(img.soft_mask.is_some(), "tRNS should produce a soft mask");
    }

    #[test]
    fn gray16_keeps_depth() {
        // 2x1 16-bit grayscale (big-endian samples).
        let pixels = [0x12u8, 0x34, 0xAB, 0xCD];
        let png = encode_png(
            2,
            1,
            png::ColorType::Grayscale,
            png::BitDepth::Sixteen,
            &pixels,
            None,
            None,
        );
        let img = from_png(&png).unwrap();
        assert_eq!(img.bits_per_component, 16);
        assert_eq!(img.color_space, ColorSpace::DeviceGray);
    }
}

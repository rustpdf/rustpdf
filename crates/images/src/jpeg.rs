//! JPEG ingestion (Fase 4.1): parse the SOF marker for geometry and embed the
//! original bytes verbatim via `DCTDecode` — never decoded, never re-encoded.

use crate::{ColorSpace, Filter, Image, ImageError};

pub(crate) fn from_jpeg(data: Vec<u8>) -> Result<Image, ImageError> {
    if !data.starts_with(&[0xFF, 0xD8]) {
        return Err(ImageError::Format("not a JPEG (no SOI marker)".into()));
    }

    let sof = parse_sof(&data)?;
    let color_space = match sof.components {
        1 => ColorSpace::DeviceGray,
        3 => ColorSpace::DeviceRgb,
        4 => ColorSpace::DeviceCmyk,
        n => {
            return Err(ImageError::Format(format!(
                "unsupported JPEG components: {n}"
            )))
        }
    };

    // Adobe CMYK JPEGs store inverted samples; PDF needs an inverting /Decode.
    let decode = if sof.components == 4 && has_adobe_marker(&data) {
        Some(vec![1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0])
    } else {
        None
    };

    Ok(Image {
        width: sof.width as u32,
        height: sof.height as u32,
        bits_per_component: sof.precision,
        color_space,
        filter: Filter::DctDecode,
        data,
        soft_mask: None,
        decode,
    })
}

struct Sof {
    precision: u8,
    width: u16,
    height: u16,
    components: u8,
}

/// Walk JPEG markers to the first Start-Of-Frame segment.
fn parse_sof(data: &[u8]) -> Result<Sof, ImageError> {
    let mut i = 2; // skip SOI
    while i + 1 < data.len() {
        if data[i] != 0xFF {
            return Err(ImageError::Format("expected marker prefix 0xFF".into()));
        }
        // Skip fill bytes (runs of 0xFF).
        while i < data.len() && data[i] == 0xFF {
            i += 1;
        }
        if i >= data.len() {
            break;
        }
        let marker = data[i];
        i += 1;

        // Standalone markers (no length): RSTn (D0–D7), SOI (D8), EOI (D9), TEM (01).
        if matches!(marker, 0xD0..=0xD9 | 0x01) {
            continue;
        }

        if i + 1 >= data.len() {
            break;
        }
        let len = u16::from_be_bytes([data[i], data[i + 1]]) as usize;
        if len < 2 || i + len > data.len() {
            return Err(ImageError::Format("bad segment length".into()));
        }
        let segment = &data[i + 2..i + len];

        // SOF0–SOF15 except DHT (C4), JPG (C8), DAC (CC).
        if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            if segment.len() < 6 {
                return Err(ImageError::Format("truncated SOF".into()));
            }
            return Ok(Sof {
                precision: segment[0],
                height: u16::from_be_bytes([segment[1], segment[2]]),
                width: u16::from_be_bytes([segment[3], segment[4]]),
                components: segment[5],
            });
        }

        i += len;
    }
    Err(ImageError::Format("no SOF marker found".into()))
}

/// Detect an APP14 "Adobe" marker (signals CMYK/YCCK color transform).
fn has_adobe_marker(data: &[u8]) -> bool {
    data.windows(5).any(|w| w == b"Adobe")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_jpeg() {
        assert!(from_jpeg(vec![0, 1, 2, 3]).is_err());
    }

    #[test]
    fn parses_a_minimal_baseline_header() {
        // SOI, then SOF0: len=17, precision=8, h=2, w=3, comps=3 (+ 3*3 comp bytes).
        let mut d = vec![
            0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x02, 0x00, 0x03, 0x03,
        ];
        d.extend_from_slice(&[1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0]); // component specs
        d.extend_from_slice(&[0xFF, 0xD9]); // EOI
        let img = from_jpeg(d).unwrap();
        assert_eq!((img.width, img.height), (3, 2));
        assert_eq!(img.bits_per_component, 8);
        assert_eq!(img.color_space, ColorSpace::DeviceRgb);
        assert_eq!(img.filter, Filter::DctDecode);
    }
}

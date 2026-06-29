//! Image XObjects and inline images: decode samples to an RGBA pixmap.

use crate::color::ColorSpace;
use cos::{Dict, Object};
use parser::{Lexer, PdfReader, Token};
use tiny_skia::{Pixmap, PremultipliedColorU8};

const MAX_IMAGE_PIXELS: u64 = 64_000_000; // ~64 MP guard

/// Decode an image XObject stream into a premultiplied RGBA pixmap at the
/// image's native resolution. `fill_rgb` is used for image masks.
pub fn decode_image(
    reader: &PdfReader,
    stream: &cos::Stream,
    resources: &Dict,
    fill_rgb: [f32; 3],
) -> Option<Pixmap> {
    let d = &stream.dict;
    let w = dim(d, "Width", "W")?;
    let h = dim(d, "Height", "H")?;
    if w == 0 || h == 0 || (w as u64 * h as u64) > MAX_IMAGE_PIXELS {
        return None;
    }
    let image_mask = bool_of(d, "ImageMask")
        .or_else(|| bool_of(d, "IM"))
        .unwrap_or(false);
    let bpc = if image_mask {
        1
    } else {
        int_of(d, "BitsPerComponent")
            .or_else(|| int_of(d, "BPC"))
            .unwrap_or(8) as u32
    };
    let filters = filter_names(reader, d);
    let terminal = filters.last().map(|s| s.as_str()).unwrap_or("");
    let decode_arr = decode_array(reader, d);

    if image_mask {
        let data = reader.stream_data(stream).ok()?;
        return build_image_mask(w, h, &data, &decode_arr, fill_rgb);
    }

    // JPEG (DCTDecode) is left encoded by the parser; decode pixels here.
    if matches!(terminal, "DCTDecode" | "DCT") {
        let jpeg = reader.stream_data(stream).ok()?;
        return decode_jpeg(&jpeg, w, h, smask(reader, d));
    }
    // Codecs we cannot cleanly rasterize.
    if matches!(
        terminal,
        "JPXDecode" | "CCITTFaxDecode" | "CCF" | "JBIG2Decode"
    ) {
        return None;
    }

    let cs = color_space(reader, d, resources);
    let samples = reader.stream_data(stream).ok()?;
    let alpha = smask(reader, d);
    build_from_samples(w, h, bpc, &cs, &samples, &decode_arr, alpha)
}

/// Decode an inline image (`BI … ID … EI`).
pub fn decode_inline(
    reader: &PdfReader,
    dict: &Dict,
    data: &[u8],
    resources: &Dict,
    fill_rgb: [f32; 3],
) -> Option<Pixmap> {
    let w = dim(dict, "Width", "W")?;
    let h = dim(dict, "Height", "H")?;
    if w == 0 || h == 0 || (w as u64 * h as u64) > MAX_IMAGE_PIXELS {
        return None;
    }
    let image_mask = bool_of(dict, "ImageMask")
        .or_else(|| bool_of(dict, "IM"))
        .unwrap_or(false);
    let bpc = if image_mask {
        1
    } else {
        int_of(dict, "BitsPerComponent")
            .or_else(|| int_of(dict, "BPC"))
            .unwrap_or(8) as u32
    };
    let decode_arr = decode_array(reader, dict);
    let filters = inline_filter_names(dict);
    let terminal = filters.last().map(|s| s.as_str()).unwrap_or("");

    // Apply non-DCT decode filters (Flate) to the raw inline payload.
    let decoded = match terminal {
        "DCTDecode" | "DCT" => {
            return decode_jpeg(data, w, h, None);
        }
        "FlateDecode" | "Fl" => images::flate_decode(data)?,
        "" => data.to_vec(),
        _ => return None, // LZW/ASCII filters on inline images: rare; skip.
    };

    if image_mask {
        return build_image_mask(w, h, &decoded, &decode_arr, fill_rgb);
    }
    let cs = inline_color_space(reader, dict, resources);
    build_from_samples(w, h, bpc, &cs, &decoded, &decode_arr, None)
}

/// Read an inline image's dict + raw data from the lexer (positioned right
/// after the `BI` keyword). Returns `(dict, raw_bytes_between_ID_and_EI)`.
pub fn read_inline_image(lex: &mut Lexer) -> Option<(Dict, Vec<u8>)> {
    let mut dict = Dict::new();
    loop {
        match lex.next_token()? {
            Token::Keyword(k) if k == b"ID" => break,
            Token::Name(key) => {
                let val = read_value(lex)?;
                dict.set(name_from(key), val);
            }
            Token::Keyword(_) => return None,
            _ => {}
        }
    }
    // After "ID" there is exactly one whitespace byte, then binary data.
    let data = lex.data();
    let mut p = lex.pos();
    if p < data.len() && is_ws(data[p]) {
        p += 1;
    }
    let start = p;
    // Scan for an `EI` delimited by whitespace (or EOF).
    let mut end = data.len();
    let mut i = start;
    while i + 1 < data.len() {
        if data[i] == b'E'
            && data[i + 1] == b'I'
            && (i == 0 || is_ws(data[i - 1]))
            && (i + 2 >= data.len() || is_ws(data[i + 2]))
        {
            end = i.saturating_sub(1).max(start);
            // Trim the single whitespace before EI.
            lex.seek(i + 2);
            let bytes = data[start..end].to_vec();
            return Some((dict, bytes));
        }
        i += 1;
    }
    lex.seek(data.len());
    Some((dict, data[start..end].to_vec()))
}

fn read_value(lex: &mut Lexer) -> Option<Object> {
    Some(match lex.next_token()? {
        Token::Integer(n) => Object::Integer(n),
        Token::Real(r) => Object::Real(r),
        Token::Name(n) => Object::Name(name_from(n)),
        Token::Str(s) => Object::String(cos::PdfString::literal(s)),
        Token::ArrayOpen => {
            let mut items = Vec::new();
            loop {
                match lex.next_token()? {
                    Token::ArrayClose => break,
                    Token::Integer(n) => items.push(Object::Integer(n)),
                    Token::Real(r) => items.push(Object::Real(r)),
                    Token::Name(n) => items.push(Object::Name(name_from(n))),
                    _ => {}
                }
            }
            Object::Array(items)
        }
        Token::Keyword(k) => match k.as_slice() {
            b"true" => Object::Bool(true),
            b"false" => Object::Bool(false),
            _ => Object::Null,
        },
        _ => Object::Null,
    })
}

// ---- sample → pixmap ----

fn build_from_samples(
    w: u32,
    h: u32,
    bpc: u32,
    cs: &ColorSpace,
    samples: &[u8],
    decode_arr: &Option<Vec<f32>>,
    alpha: Option<AlphaMap>,
) -> Option<Pixmap> {
    let ncomp = cs.components();
    let max_val = ((1u64 << bpc) - 1) as f32;
    let indexed = matches!(cs, ColorSpace::Indexed { .. });
    let row_bits = ncomp as u64 * bpc as u64 * w as u64;
    let row_bytes = row_bits.div_ceil(8) as usize;

    let mut pixmap = Pixmap::new(w, h)?;
    let pixels = pixmap.pixels_mut();

    let mut comps = vec![0f32; ncomp];
    for y in 0..h as usize {
        let row = samples.get(y * row_bytes..)?;
        for x in 0..w as usize {
            for (c, slot) in comps.iter_mut().enumerate() {
                let idx = (x * ncomp + c) as u64;
                let raw = read_bits(row, idx, bpc);
                if indexed {
                    *slot = raw as f32; // palette index, used as-is
                } else {
                    let mut v = raw as f32 / max_val;
                    if let Some(dec) = decode_arr {
                        if let (Some(&d0), Some(&d1)) = (dec.get(c * 2), dec.get(c * 2 + 1)) {
                            v = d0 + (raw as f32 / max_val) * (d1 - d0);
                        }
                    }
                    *slot = v;
                }
            }
            let rgb = cs.to_rgb(&comps);
            let a = match &alpha {
                Some(am) => am.at(x as u32, y as u32, w, h),
                None => 255,
            };
            set_px(pixels, y * w as usize + x, rgb, a);
        }
    }
    Some(pixmap)
}

fn build_image_mask(
    w: u32,
    h: u32,
    samples: &[u8],
    decode_arr: &Option<Vec<f32>>,
    fill_rgb: [f32; 3],
) -> Option<Pixmap> {
    // Default Decode [0 1]: sample 0 ⇒ paint. [1 0] inverts.
    let invert = decode_arr
        .as_ref()
        .map(|d| d.first().copied().unwrap_or(0.0) > 0.5)
        .unwrap_or(false);
    let row_bytes = (w as u64).div_ceil(8) as usize;
    let mut pixmap = Pixmap::new(w, h)?;
    let pixels = pixmap.pixels_mut();
    for y in 0..h as usize {
        let row = samples.get(y * row_bytes..)?;
        for x in 0..w as usize {
            let bit = read_bits(row, x as u64, 1);
            let paint = if invert { bit == 1 } else { bit == 0 };
            let a = if paint { 255 } else { 0 };
            set_px(pixels, y * w as usize + x, fill_rgb, a);
        }
    }
    Some(pixmap)
}

fn decode_jpeg(data: &[u8], w: u32, h: u32, alpha: Option<AlphaMap>) -> Option<Pixmap> {
    let mut dec = jpeg_decoder::Decoder::new(std::io::Cursor::new(data));
    let pixels = dec.decode().ok()?;
    let info = dec.info()?;
    let (jw, jh) = (info.width as u32, info.height as u32);
    let (jw, jh) = if jw == 0 || jh == 0 { (w, h) } else { (jw, jh) };
    let mut pixmap = Pixmap::new(jw, jh)?;
    let out = pixmap.pixels_mut();
    use jpeg_decoder::PixelFormat::*;
    let comps = match info.pixel_format {
        L8 => 1,
        L16 => 2,
        RGB24 => 3,
        CMYK32 => 4,
    };
    for i in 0..(jw as usize * jh as usize) {
        let off = i * comps;
        let rgb = match info.pixel_format {
            L8 => {
                let g = *pixels.get(off)? as f32 / 255.0;
                [g, g, g]
            }
            L16 => {
                let g = *pixels.get(off)? as f32 / 255.0;
                [g, g, g]
            }
            RGB24 => [
                *pixels.get(off)? as f32 / 255.0,
                *pixels.get(off + 1)? as f32 / 255.0,
                *pixels.get(off + 2)? as f32 / 255.0,
            ],
            CMYK32 => {
                // Adobe JPEGs store inverted CMYK; invert back before convert.
                let c = 1.0 - *pixels.get(off)? as f32 / 255.0;
                let m = 1.0 - *pixels.get(off + 1)? as f32 / 255.0;
                let y = 1.0 - *pixels.get(off + 2)? as f32 / 255.0;
                let k = 1.0 - *pixels.get(off + 3)? as f32 / 255.0;
                [
                    (1.0 - c) * (1.0 - k),
                    (1.0 - m) * (1.0 - k),
                    (1.0 - y) * (1.0 - k),
                ]
            }
        };
        let a = match &alpha {
            Some(am) => am.at((i as u32) % jw, (i as u32) / jw, jw, jh),
            None => 255,
        };
        set_px(out, i, rgb, a);
    }
    Some(pixmap)
}

#[inline]
fn set_px(pixels: &mut [PremultipliedColorU8], i: usize, rgb: [f32; 3], a: u8) {
    let af = a as f32 / 255.0;
    let r = (rgb[0].clamp(0.0, 1.0) * af * 255.0).round() as u8;
    let g = (rgb[1].clamp(0.0, 1.0) * af * 255.0).round() as u8;
    let b = (rgb[2].clamp(0.0, 1.0) * af * 255.0).round() as u8;
    if let Some(slot) = pixels.get_mut(i) {
        *slot = PremultipliedColorU8::from_rgba(r, g, b, a)
            .unwrap_or(PremultipliedColorU8::from_rgba(0, 0, 0, 0).unwrap());
    }
}

/// Read a `bits`-wide big-endian sample at sample-index `idx` within a row.
fn read_bits(row: &[u8], idx: u64, bits: u32) -> u64 {
    if bits == 8 {
        return row.get(idx as usize).copied().unwrap_or(0) as u64;
    }
    if bits == 16 {
        let o = idx as usize * 2;
        let hi = row.get(o).copied().unwrap_or(0) as u64;
        let lo = row.get(o + 1).copied().unwrap_or(0) as u64;
        return (hi << 8) | lo;
    }
    let bit_off = idx * bits as u64;
    let mut v = 0u64;
    for i in 0..bits as u64 {
        let b = bit_off + i;
        let byte = (b / 8) as usize;
        let bit = 7 - (b % 8) as u32;
        let set = row.get(byte).map(|&x| (x >> bit) & 1).unwrap_or(0);
        v = (v << 1) | set as u64;
    }
    v
}

// ---- /SMask handling ----

struct AlphaMap {
    w: u32,
    h: u32,
    data: Vec<u8>, // grayscale 8-bit
}

impl AlphaMap {
    fn at(&self, x: u32, y: u32, dst_w: u32, dst_h: u32) -> u8 {
        // Nearest-neighbour resample if soft-mask dims differ from the image.
        let sx = (x * self.w).checked_div(dst_w).unwrap_or(0);
        let sy = (y * self.h).checked_div(dst_h).unwrap_or(0);
        self.data
            .get(
                (sy.min(self.h.saturating_sub(1)) * self.w + sx.min(self.w.saturating_sub(1)))
                    as usize,
            )
            .copied()
            .unwrap_or(255)
    }
}

fn smask(reader: &PdfReader, d: &Dict) -> Option<AlphaMap> {
    let Object::Stream(s) = reader.resolve(d.get("SMask")?) else {
        return None;
    };
    let w = dim(&s.dict, "Width", "W")?;
    let h = dim(&s.dict, "Height", "H")?;
    if w == 0 || h == 0 || (w as u64 * h as u64) > MAX_IMAGE_PIXELS {
        return None;
    }
    let bpc = int_of(&s.dict, "BitsPerComponent").unwrap_or(8) as u32;
    let filters = filter_names(reader, &s.dict);
    if matches!(filters.last().map(|s| s.as_str()), Some("DCTDecode")) {
        // Soft mask stored as JPEG grayscale.
        let jpeg = reader.stream_data(s).ok()?;
        let mut dec = jpeg_decoder::Decoder::new(std::io::Cursor::new(jpeg));
        let px = dec.decode().ok()?;
        return Some(AlphaMap { w, h, data: px });
    }
    let raw = reader.stream_data(s).ok()?;
    let row_bytes = (w as u64 * bpc as u64).div_ceil(8) as usize;
    let mut data = Vec::with_capacity((w * h) as usize);
    let max_val = ((1u64 << bpc) - 1) as f32;
    for y in 0..h as usize {
        let row = raw.get(y * row_bytes..).unwrap_or(&[]);
        for x in 0..w as usize {
            let v = read_bits(row, x as u64, bpc);
            data.push((v as f32 / max_val * 255.0).round() as u8);
        }
    }
    Some(AlphaMap { w, h, data })
}

// ---- dict helpers ----

fn color_space(reader: &PdfReader, d: &Dict, resources: &Dict) -> ColorSpace {
    match d.get("ColorSpace").or_else(|| d.get("CS")) {
        Some(o) => ColorSpace::parse(reader, o, resources),
        None => ColorSpace::DeviceGray,
    }
}

fn inline_color_space(reader: &PdfReader, d: &Dict, resources: &Dict) -> ColorSpace {
    // Inline images allow abbreviated names.
    if let Some(Object::Name(n)) = d.get("ColorSpace").or_else(|| d.get("CS")) {
        let full = match n.as_str() {
            "G" => "DeviceGray",
            "RGB" => "DeviceRGB",
            "CMYK" => "DeviceCMYK",
            "I" => "Indexed",
            other => other,
        };
        return ColorSpace::parse(reader, &Object::Name(cos::Name::new(full)), resources);
    }
    color_space(reader, d, resources)
}

fn filter_names(reader: &PdfReader, d: &Dict) -> Vec<String> {
    match d.get("Filter").map(|o| reader.resolve(o)) {
        Some(Object::Name(n)) => vec![n.as_str().to_string()],
        Some(Object::Array(a)) => a
            .iter()
            .filter_map(|o| match reader.resolve(o) {
                Object::Name(n) => Some(n.as_str().to_string()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn abbr_filter(s: &str) -> &str {
    match s {
        "AHx" => "ASCIIHexDecode",
        "A85" => "ASCII85Decode",
        "LZW" => "LZWDecode",
        "Fl" => "FlateDecode",
        "RL" => "RunLengthDecode",
        "CCF" => "CCITTFaxDecode",
        "DCT" => "DCTDecode",
        other => other,
    }
}

fn inline_filter_names(d: &Dict) -> Vec<String> {
    match d.get("Filter").or_else(|| d.get("F")) {
        Some(Object::Name(n)) => vec![abbr_filter(n.as_str()).to_string()],
        Some(Object::Array(a)) => a
            .iter()
            .filter_map(|o| match o {
                Object::Name(n) => Some(abbr_filter(n.as_str()).to_string()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn name_from(bytes: Vec<u8>) -> cos::Name {
    cos::Name::new(String::from_utf8_lossy(&bytes).into_owned())
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c' | b'\0')
}

fn decode_array(reader: &PdfReader, d: &Dict) -> Option<Vec<f32>> {
    match d
        .get("Decode")
        .or_else(|| d.get("D"))
        .map(|o| reader.resolve(o))
    {
        Some(Object::Array(a)) => Some(a.iter().filter_map(num).collect()),
        _ => None,
    }
}

fn dim(d: &Dict, full: &str, abbr: &str) -> Option<u32> {
    int_of(d, full)
        .or_else(|| int_of(d, abbr))
        .filter(|v| *v >= 0)
        .map(|v| v as u32)
}

fn int_of(d: &Dict, key: &str) -> Option<i64> {
    match d.get(key) {
        Some(Object::Integer(n)) => Some(*n),
        Some(Object::Real(r)) => Some(*r as i64),
        _ => None,
    }
}

fn bool_of(d: &Dict, key: &str) -> Option<bool> {
    match d.get(key) {
        Some(Object::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn num(o: &Object) -> Option<f32> {
    match o {
        Object::Integer(n) => Some(*n as f32),
        Object::Real(r) => Some(*r as f32),
        _ => None,
    }
}

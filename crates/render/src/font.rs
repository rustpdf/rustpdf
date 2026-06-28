//! Font loading and glyph access for rendering.
//!
//! Two worlds, unified into [`LoadedFont`]:
//!
//! * **Type0 / CIDFontType2 with `Identity-H`** — what this library's own
//!   writer emits. Codes are 2 bytes, the CID equals the code, `CIDToGIDMap`
//!   is `Identity`, so the glyph id *is* the code. Widths come from `/W`.
//!   This path is exercised by the round-trip tests, so it is the most solid.
//! * **Simple fonts** (Type1 / TrueType, 1 byte per code) — width from
//!   `/Widths`, glyph id resolved through the embedded program's cmap via the
//!   code's Unicode value (WinAnsi/Standard + `/Differences`). Best-effort for
//!   arbitrary third-party PDFs.
//!
//! Non-embedded fonts fall back to a bundled Roboto program so standard-14
//! text still rasterizes (approximate metrics, real outlines).

use cos::{Dict, Object};
use parser::PdfReader;
use std::collections::HashMap;

/// Bundled fallback programs for non-embedded fonts (Apache-2.0).
static FALLBACK_REGULAR: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
));
static FALLBACK_BOLD: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Bold.ttf"
));

/// One shaped glyph ready to position and outline.
pub struct Glyph {
    /// Glyph id within the (embedded or fallback) program.
    pub gid: u16,
    /// Advance width in text-space units (already divided by 1000).
    pub width: f32,
    /// True when this code is the single-byte space (for word spacing).
    pub is_space: bool,
}

pub struct LoadedFont {
    program: fonts::Font,
    units_per_em: f32,
    two_byte: bool,
    /// Simple-font: glyph id per byte code.
    simple_gids: Option<Box<[u16; 256]>>,
    /// Simple-font widths (text-space units, /1000) per byte code.
    simple_widths: Option<Box<[f32; 256]>>,
    /// CID widths and default width (text-space units, /1000).
    cid_widths: HashMap<u32, f32>,
    cid_default_width: f32,
    /// Optional explicit CIDToGIDMap (cid → gid).
    cid_to_gid: Option<Vec<u16>>,
    /// Type 3 font data: glyphs are content streams, not outlines.
    type3: Option<Type3>,
}

/// A Type 3 font: each glyph is a content-stream procedure drawn in glyph
/// space and mapped to text space by `font_matrix`. Glyphs are executed by the
/// content interpreter (like a tiny Form XObject), not outlined.
pub struct Type3 {
    /// Glyph space → text space (e.g. `[0.001 0 0 0.001 0 0]`).
    pub font_matrix: [f32; 6],
    /// Byte code → decoded CharProc content stream.
    char_procs: HashMap<u8, Vec<u8>>,
    /// `/Resources` used by the CharProcs (fonts/images/colorspaces they draw).
    pub resources: Option<Dict>,
    /// Advance per byte code in text space (already × `font_matrix[0]`).
    widths_text: Box<[f32; 256]>,
}

impl Type3 {
    /// The CharProc content stream for a byte code, if this code has a glyph.
    pub fn char_proc(&self, code: u8) -> Option<&[u8]> {
        self.char_procs.get(&code).map(|v| v.as_slice())
    }
}

impl LoadedFont {
    pub fn units_per_em(&self) -> f32 {
        self.units_per_em
    }

    /// Access the underlying program for outline extraction.
    pub fn face(&self) -> ttf_parser::Face<'_> {
        self.program.face()
    }

    /// Type 3 data when this is a Type 3 font (glyphs are content streams).
    pub fn type3(&self) -> Option<&Type3> {
        self.type3.as_ref()
    }

    /// Decode a shown string into positioned glyphs.
    pub fn decode(&self, bytes: &[u8]) -> Vec<Glyph> {
        if let Some(t3) = &self.type3 {
            // Type 3 is single-byte; the byte code indexes CharProcs and widths.
            return bytes
                .iter()
                .map(|&c| Glyph {
                    gid: c as u16,
                    width: t3.widths_text[c as usize],
                    is_space: c == 32,
                })
                .collect();
        }
        if self.two_byte {
            let mut out = Vec::with_capacity(bytes.len() / 2);
            let mut i = 0;
            while i + 1 < bytes.len() {
                let code = ((bytes[i] as u32) << 8) | bytes[i + 1] as u32;
                i += 2;
                let gid = self.cid_gid(code);
                let width = self
                    .cid_widths
                    .get(&code)
                    .copied()
                    .unwrap_or(self.cid_default_width);
                out.push(Glyph {
                    gid,
                    width,
                    is_space: false,
                });
            }
            out
        } else {
            bytes
                .iter()
                .map(|&c| {
                    let gid = self
                        .simple_gids
                        .as_ref()
                        .map(|t| t[c as usize])
                        .unwrap_or(c as u16);
                    let width = self
                        .simple_widths
                        .as_ref()
                        .map(|t| t[c as usize])
                        .unwrap_or(0.5);
                    Glyph {
                        gid,
                        width,
                        is_space: c == 32,
                    }
                })
                .collect()
        }
    }

    fn cid_gid(&self, cid: u32) -> u16 {
        match &self.cid_to_gid {
            Some(map) => map.get(cid as usize).copied().unwrap_or(0),
            None => cid as u16,
        }
    }

    /// Build a `LoadedFont` from a `/Font` resource entry.
    pub fn load(reader: &PdfReader, font_dict: &Dict) -> Option<LoadedFont> {
        let subtype = name_of(font_dict.get("Subtype"));
        match subtype.as_deref() {
            Some("Type0") => Self::load_type0(reader, font_dict),
            Some("Type3") => Self::load_type3(reader, font_dict),
            _ => Self::load_simple(reader, font_dict),
        }
    }

    fn load_type3(reader: &PdfReader, font_dict: &Dict) -> Option<LoadedFont> {
        // FontMatrix maps glyph space → text space (default 1/1000).
        let fm: [f32; 6] = match font_dict.get("FontMatrix").map(|o| reader.resolve(o)) {
            Some(Object::Array(a)) => {
                let v: Vec<f32> = a.iter().filter_map(num).collect();
                if v.len() == 6 {
                    [v[0], v[1], v[2], v[3], v[4], v[5]]
                } else {
                    [0.001, 0.0, 0.0, 0.001, 0.0, 0.0]
                }
            }
            _ => [0.001, 0.0, 0.0, 0.001, 0.0, 0.0],
        };

        // /Encoding /Differences gives code → glyph name; CharProcs maps name →
        // the glyph's content stream.
        let names = build_encoding_names(reader, font_dict);
        let char_procs_dict = reader.resolve_dict(font_dict.get("CharProcs")?)?.clone();
        let mut char_procs: HashMap<u8, Vec<u8>> = HashMap::new();
        for (code, name) in names.iter().enumerate() {
            if let Some(name) = name {
                if let Some(Object::Stream(s)) = char_procs_dict.get(name).map(|o| reader.resolve(o))
                {
                    if let Ok(data) = reader.stream_data(s) {
                        char_procs.insert(code as u8, data);
                    }
                }
            }
        }

        // Widths are in glyph space; scale by font_matrix[0] to text space so the
        // advance path matches simple/Type0 fonts (advance = width × font_size).
        let first = font_dict.get("FirstChar").and_then(int).unwrap_or(0);
        let widths_arr = match font_dict.get("Widths").map(|o| reader.resolve(o)) {
            Some(Object::Array(a)) => a.iter().filter_map(num).collect::<Vec<f32>>(),
            _ => Vec::new(),
        };
        let mut widths_text = Box::new([0f32; 256]);
        for (i, w) in widths_arr.iter().enumerate() {
            let code = first as usize + i;
            if code < 256 {
                widths_text[code] = w * fm[0];
            }
        }

        let resources = reader
            .resolve_dict(font_dict.get("Resources").unwrap_or(&Object::Null))
            .cloned();

        // Placeholder program — never used for Type 3 (no outlines), but the
        // struct requires one; the bundled fallback is always valid.
        let (program, _) = load_program(reader, None, false);
        let units_per_em = program.units_per_em() as f32;
        Some(LoadedFont {
            program,
            units_per_em,
            two_byte: false,
            simple_gids: None,
            simple_widths: None,
            cid_widths: HashMap::new(),
            cid_default_width: 0.0,
            cid_to_gid: None,
            type3: Some(Type3 {
                font_matrix: fm,
                char_procs,
                resources,
                widths_text,
            }),
        })
    }

    fn load_type0(reader: &PdfReader, font_dict: &Dict) -> Option<LoadedFont> {
        let desc_fonts = reader.resolve(font_dict.get("DescendantFonts")?);
        let cid_font = match desc_fonts {
            Object::Array(a) => reader.resolve_dict(a.first()?)?.clone(),
            Object::Dict(d) => d.clone(),
            _ => return None,
        };
        let descriptor = reader
            .resolve_dict(cid_font.get("FontDescriptor")?)
            .cloned();
        let (program, embedded) = load_program(reader, descriptor.as_ref(), false);

        // CIDToGIDMap: Identity (None) or a 2-byte stream.
        let cid_to_gid = match cid_font.get("CIDToGIDMap").map(|o| reader.resolve(o)) {
            Some(Object::Stream(s)) => {
                let data = reader.stream_data(s).unwrap_or_default();
                Some(
                    data.chunks(2)
                        .map(|c| ((c[0] as u16) << 8) | *c.get(1).unwrap_or(&0) as u16)
                        .collect::<Vec<u16>>(),
                )
            }
            _ => None,
        };

        let dw = cid_font
            .get("DW")
            .and_then(num)
            .map(|w| w / 1000.0)
            .unwrap_or(1.0);
        let cid_widths = parse_w_array(reader, cid_font.get("W"));

        let units_per_em = program.units_per_em() as f32;
        let _ = embedded;
        Some(LoadedFont {
            program,
            units_per_em,
            two_byte: true,
            simple_gids: None,
            simple_widths: None,
            cid_widths,
            cid_default_width: dw,
            cid_to_gid,
            type3: None,
        })
    }

    fn load_simple(reader: &PdfReader, font_dict: &Dict) -> Option<LoadedFont> {
        let bold = name_of(font_dict.get("BaseFont"))
            .map(|n| n.to_ascii_lowercase().contains("bold"))
            .unwrap_or(false);
        let descriptor = reader
            .resolve_dict(font_dict.get("FontDescriptor").unwrap_or(&Object::Null))
            .cloned();
        let (program, _embedded) = load_program(reader, descriptor.as_ref(), bold);
        let units_per_em = program.units_per_em() as f32;

        // Build code → Unicode from the encoding, then code → gid via cmap.
        let code_to_unicode = build_encoding(reader, font_dict);
        let mut gids = Box::new([0u16; 256]);
        {
            let face = program.face();
            for (code, slot) in gids.iter_mut().enumerate() {
                let uni = code_to_unicode[code];
                let gid = uni
                    .and_then(|u| face.glyph_index(u))
                    .or_else(|| {
                        // symbolic: try the raw byte, then the 0xF000 PUA range.
                        char::from_u32(code as u32)
                            .and_then(|c| face.glyph_index(c))
                            .or_else(|| face.glyph_index(char::from_u32(0xF000 + code as u32)?))
                    })
                    .map(|g| g.0)
                    .unwrap_or(code as u16);
                *slot = gid;
            }
        }

        // Widths.
        let first = font_dict.get("FirstChar").and_then(int).unwrap_or(0);
        let widths_arr = match font_dict.get("Widths").map(|o| reader.resolve(o)) {
            Some(Object::Array(a)) => a.iter().filter_map(num).collect::<Vec<f32>>(),
            _ => Vec::new(),
        };
        let mut widths = Box::new([0.5f32; 256]);
        for (i, w) in widths_arr.iter().enumerate() {
            let code = first as usize + i;
            if code < 256 {
                widths[code] = w / 1000.0;
            }
        }

        Some(LoadedFont {
            program,
            units_per_em,
            two_byte: false,
            simple_gids: Some(gids),
            simple_widths: Some(widths),
            cid_widths: HashMap::new(),
            cid_default_width: 0.5,
            cid_to_gid: None,
            type3: None,
        })
    }
}

/// Load an embedded font program (FontFile2 TrueType / FontFile3 CFF-OpenType),
/// or the bundled fallback when none is usable. Returns `(font, was_embedded)`.
fn load_program(reader: &PdfReader, descriptor: Option<&Dict>, bold: bool) -> (fonts::Font, bool) {
    if let Some(desc) = descriptor {
        for key in ["FontFile2", "FontFile3", "FontFile"] {
            if let Some(Object::Stream(s)) = desc.get(key).map(|o| reader.resolve(o)) {
                if let Ok(data) = reader.stream_data(s) {
                    if let Ok(font) = fonts::Font::from_bytes(data, 0) {
                        return (font, true);
                    }
                }
            }
        }
    }
    let bytes = if bold {
        FALLBACK_BOLD
    } else {
        FALLBACK_REGULAR
    };
    let font = fonts::Font::from_bytes(bytes.to_vec(), 0).expect("bundled fallback font is valid");
    (font, false)
}

/// Parse a CIDFont `/W` array into `cid → width` (text-space units, /1000).
fn parse_w_array(reader: &PdfReader, w: Option<&Object>) -> HashMap<u32, f32> {
    let mut map = HashMap::new();
    let arr = match w.map(|o| reader.resolve(o)) {
        Some(Object::Array(a)) => a.clone(),
        _ => return map,
    };
    let mut i = 0;
    while i < arr.len() {
        let c = match int(reader.resolve(&arr[i])) {
            Some(v) => v as u32,
            None => break,
        };
        match arr.get(i + 1).map(|o| reader.resolve(o)) {
            // c [w1 w2 ...]: widths for c, c+1, ...
            Some(Object::Array(ws)) => {
                for (j, wo) in ws.iter().enumerate() {
                    if let Some(wv) = num(wo) {
                        map.insert(c + j as u32, wv / 1000.0);
                    }
                }
                i += 2;
            }
            // c_first c_last w: same width for the range.
            Some(_) => {
                let c_last = arr
                    .get(i + 1)
                    .and_then(|o| int(reader.resolve(o)))
                    .unwrap_or(c as i64) as u32;
                let wv = arr.get(i + 2).and_then(num).unwrap_or(0.0) / 1000.0;
                for cid in c..=c_last.min(c + 65_535) {
                    map.insert(cid, wv);
                }
                i += 3;
            }
            None => break,
        }
    }
    map
}

/// Build a code→glyph-name table from a Type 3 font's `/Encoding /Differences`
/// (the names index `/CharProcs`).
fn build_encoding_names(reader: &PdfReader, font_dict: &Dict) -> Vec<Option<String>> {
    let mut table: Vec<Option<String>> = vec![None; 256];
    if let Some(Object::Dict(enc)) = font_dict.get("Encoding").map(|o| reader.resolve(o)) {
        if let Some(Object::Array(diffs)) = enc.get("Differences").map(|o| reader.resolve(o)) {
            let mut code = 0usize;
            for item in diffs {
                match reader.resolve(item) {
                    Object::Integer(n) => code = *n as usize,
                    Object::Name(name) => {
                        if code < 256 {
                            table[code] = Some(name.as_str().to_string());
                        }
                        code += 1;
                    }
                    _ => {}
                }
            }
        }
    }
    table
}

/// Build a code→Unicode table from a simple font's `/Encoding`.
fn build_encoding(reader: &PdfReader, font_dict: &Dict) -> [Option<char>; 256] {
    let mut table = std_encoding(EncodingBase::Standard);
    let mut base = EncodingBase::Standard;
    match font_dict.get("Encoding").map(|o| reader.resolve(o)) {
        Some(Object::Name(n)) => {
            base = EncodingBase::from_name(n.as_str());
            table = std_encoding(base);
        }
        Some(Object::Dict(enc)) => {
            if let Some(Object::Name(n)) = enc.get("BaseEncoding") {
                base = EncodingBase::from_name(n.as_str());
            }
            table = std_encoding(base);
            // /Differences: [code /name /name code /name ...]
            if let Some(Object::Array(diffs)) = enc.get("Differences").map(|o| reader.resolve(o)) {
                let mut code = 0usize;
                for item in diffs {
                    match reader.resolve(item) {
                        Object::Integer(n) => code = *n as usize,
                        Object::Name(name) => {
                            if code < 256 {
                                table[code] = glyph_name_to_char(name.as_str());
                            }
                            code += 1;
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    let _ = base;
    table
}

#[derive(Clone, Copy)]
enum EncodingBase {
    Standard,
    WinAnsi,
    MacRoman,
}

impl EncodingBase {
    fn from_name(n: &str) -> EncodingBase {
        match n {
            "WinAnsiEncoding" => EncodingBase::WinAnsi,
            "MacRomanEncoding" => EncodingBase::MacRoman,
            _ => EncodingBase::Standard,
        }
    }
}

/// ASCII-range plus the WinAnsi high range (the common case); Standard/MacRoman
/// share the ASCII range and differ mostly above 127, approximated here.
fn std_encoding(base: EncodingBase) -> [Option<char>; 256] {
    let mut t: [Option<char>; 256] = [None; 256];
    for (code, slot) in t.iter_mut().enumerate().take(127).skip(32) {
        *slot = char::from_u32(code as u32);
    }
    // Latin-1 high range (0xA0..=0xFF maps 1:1 to Unicode) is shared.
    for (code, slot) in t.iter_mut().enumerate().skip(0xA0) {
        *slot = char::from_u32(code as u32);
    }
    if let EncodingBase::WinAnsi = base {
        // Overlay the CP1252 0x80..=0x9F specials.
        for &(code, ch) in WINANSI_HIGH {
            t[code as usize] = char::from_u32(ch);
        }
    }
    t
}

/// Resolve a glyph name to a Unicode scalar: `uniXXXX`, `uXXXXXX`, the AGL
/// subset below, or a single-character name.
fn glyph_name_to_char(name: &str) -> Option<char> {
    if let Some(hex) = name.strip_prefix("uni") {
        if hex.len() >= 4 {
            if let Ok(v) = u32::from_str_radix(&hex[..4], 16) {
                return char::from_u32(v);
            }
        }
    }
    if let Some(hex) = name.strip_prefix('u') {
        if (4..=6).contains(&hex.len()) {
            if let Ok(v) = u32::from_str_radix(hex, 16) {
                return char::from_u32(v);
            }
        }
    }
    if let Some(c) = agl_lookup(name) {
        return Some(c);
    }
    let mut chars = name.chars();
    let first = chars.next();
    if chars.next().is_none() {
        return first;
    }
    None
}

include!("agl.rs");

fn name_of(o: Option<&Object>) -> Option<String> {
    match o {
        Some(Object::Name(n)) => Some(n.as_str().to_string()),
        _ => None,
    }
}

fn int(o: &Object) -> Option<i64> {
    match o {
        Object::Integer(n) => Some(*n),
        Object::Real(r) => Some(*r as i64),
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

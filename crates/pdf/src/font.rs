//! PDF font-object construction for embedded, subsetted Unicode text
//! (Fase 3A.2 + 3B). Every text font is emitted as a Type0 composite font with
//! a CIDFontType2 descendant, `Identity-H` encoding, an identity `CIDToGIDMap`,
//! a `W` width array, a `ToUnicode` CMap and an embedded `FontFile2` program.
//!
//! Because `CIDToGIDMap` is `Identity`, the CIDs written in content streams are
//! exactly the *subset* glyph ids produced by [`fonts::Subset`]. All metrics
//! are converted from font design units to PDF glyph space (1000 units/em).

use std::collections::BTreeMap;

use cos::{Dict, Object, PdfString, Stream};
use fonts::{Font, Subset};
use writer::Document as WriterDoc;

/// Identifier for a font registered on a [`Document`](crate::Document).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontId(pub(crate) usize);

impl FontId {
    /// The content-stream resource name for this font (e.g. `F0`).
    pub(crate) fn resource_name(self) -> String {
        format!("F{}", self.0)
    }

    /// The zero-based registration index (stable handle for bindings/FFI).
    pub fn index(self) -> usize {
        self.0
    }

    /// Reconstruct a font id from its [`index`](FontId::index).
    pub fn from_index(index: usize) -> Self {
        FontId(index)
    }
}

/// A font registered on a document.
#[derive(Debug)]
pub(crate) struct RegisteredFont {
    pub font: Font,
}

impl RegisteredFont {
    pub fn new(font: Font) -> Self {
        RegisteredFont { font }
    }
}

/// Glyph usage accumulated for one font during a serialization pass.
#[derive(Default, Clone)]
pub(crate) struct FontUsage {
    /// Original glyph ids referenced anywhere in the document.
    pub used_gids: std::collections::BTreeSet<u16>,
    /// First-seen Unicode text for each original glyph id (for `ToUnicode`).
    pub gid_to_unicode: BTreeMap<u16, String>,
}

impl FontUsage {
    /// True if this font is referenced (any glyphs used).
    pub fn is_used(&self) -> bool {
        !self.used_gids.is_empty()
    }
}

/// Scale a design-unit value to PDF glyph space (1000 units/em).
fn to_glyph_space(value: f64, units_per_em: u16) -> i64 {
    (value * 1000.0 / units_per_em as f64).round() as i64
}

/// Deterministic 6-uppercase-letter subset tag from the font's index.
fn subset_tag(index: usize) -> String {
    let mut n = index;
    let mut letters = [b'A'; 6];
    for slot in letters.iter_mut().rev() {
        *slot = b'A' + (n % 26) as u8;
        n /= 26;
    }
    String::from_utf8(letters.to_vec()).unwrap()
}

/// Build all font objects for one used font and return the Type0 font reference
/// to be placed in page `Resources`.
pub(crate) fn build_font(
    doc: &mut WriterDoc,
    font: &Font,
    usage: &FontUsage,
    index: usize,
    need_cidset: bool,
) -> Result<(Subset, cos::Reference), fonts::FontError> {
    let upem = font.units_per_em();

    // Subset over the sorted set of used glyphs (glyph 0 is added implicitly).
    let used: Vec<u16> = usage.used_gids.iter().copied().collect();
    let subset = font.subset(&used)?;

    let tag = subset_tag(index);
    let base_name = format!("{}+{}", tag, sanitize_name(font.postscript_name()));

    // FontFile2: the embedded subset program.
    let mut ff_dict = Dict::new();
    ff_dict.set("Length1", subset.data.len() as i64);
    let font_file = doc.add(Stream::with_dict(ff_dict, subset.data.clone()));

    // FontDescriptor.
    let bbox = font.bbox();
    let mut descriptor = Dict::new()
        .with("Type", Object::name("FontDescriptor"))
        .with("FontName", Object::name(base_name.clone()))
        .with("Flags", font.descriptor_flags() as i64)
        .with(
            "FontBBox",
            Object::Array(vec![
                Object::Integer(to_glyph_space(bbox[0] as f64, upem)),
                Object::Integer(to_glyph_space(bbox[1] as f64, upem)),
                Object::Integer(to_glyph_space(bbox[2] as f64, upem)),
                Object::Integer(to_glyph_space(bbox[3] as f64, upem)),
            ]),
        )
        .with("ItalicAngle", Object::Real(font.italic_angle() as f64))
        .with("Ascent", to_glyph_space(font.ascender() as f64, upem))
        .with("Descent", to_glyph_space(font.descender() as f64, upem))
        .with("CapHeight", to_glyph_space(font.cap_height() as f64, upem))
        .with("StemV", Object::Real(font.stem_v()))
        .with("FontFile2", font_file);

    // PDF/A-1 requires a `/CIDSet` listing the CIDs present in the subset. With
    // an identity CID→GID map the CIDs are 0..N contiguous, so the bitmap is the
    // first N bits set (MSB first).
    if need_cidset {
        // Must cover every glyph in the embedded program (its `numGlyphs`), not
        // just the referenced ones (PDF/A 6.2.11.4.2). Read the count straight
        // from the produced subset program so it always matches exactly.
        let n = Font::from_bytes(subset.data.clone(), 0)
            .map(|f| f.num_glyphs() as usize)
            .unwrap_or_else(|_| subset.num_glyphs() as usize);
        let mut bits = vec![0u8; n.div_ceil(8)];
        for cid in 0..n {
            bits[cid / 8] |= 0x80 >> (cid % 8);
        }
        let cidset_ref = doc.add(Stream::new(bits));
        descriptor.set("CIDSet", cidset_ref);
    }
    let descriptor_ref = doc.add(descriptor);

    // W array: widths keyed by subset (== content-stream) glyph id, in order.
    let mut widths = Vec::new();
    for (_new, old) in subset.iter_new() {
        let w = to_glyph_space(font.advance(old) as f64, upem);
        widths.push(Object::Integer(w));
    }
    let w_array = Object::Array(vec![Object::Integer(0), Object::Array(widths)]);

    // CIDFontType2 descendant.
    let cid_system_info = Dict::new()
        .with("Registry", PdfString::literal("Adobe"))
        .with("Ordering", PdfString::literal("Identity"))
        .with("Supplement", 0);
    let cid_font = Dict::new()
        .with("Type", Object::name("Font"))
        .with("Subtype", Object::name("CIDFontType2"))
        .with("BaseFont", Object::name(base_name.clone()))
        .with("CIDSystemInfo", Object::Dict(cid_system_info))
        .with("FontDescriptor", descriptor_ref)
        .with("CIDToGIDMap", Object::name("Identity"))
        // Spec default width is 1000, not 0: a CID present in the embedded
        // program but absent from /W (e.g. a component glyph) must fall back to
        // 1000, not collapse to zero advance. /W densely covers the subset today
        // so this is defensive, but /DW 0 would be an incorrect default.
        .with("DW", 1000)
        .with("W", w_array);
    let cid_font_ref = doc.add(cid_font);

    // ToUnicode CMap for text extraction (Fase 3B.4).
    let to_unicode = build_to_unicode(&subset, &usage.gid_to_unicode);
    let to_unicode_ref = doc.add(Stream::new(to_unicode));

    // Type0 root font.
    let type0 = Dict::new()
        .with("Type", Object::name("Font"))
        .with("Subtype", Object::name("Type0"))
        .with("BaseFont", Object::name(base_name))
        .with("Encoding", Object::name("Identity-H"))
        .with(
            "DescendantFonts",
            Object::Array(vec![Object::Reference(cid_font_ref)]),
        )
        .with("ToUnicode", to_unicode_ref);
    let type0_ref = doc.add(type0);

    Ok((subset, type0_ref))
}

/// Build the `ToUnicode` CMap mapping each subset glyph id to its Unicode text.
fn build_to_unicode(subset: &Subset, gid_to_unicode: &BTreeMap<u16, String>) -> Vec<u8> {
    let mut entries: Vec<(u16, &String)> = Vec::new();
    for (new, old) in subset.iter_new() {
        if new == 0 {
            continue;
        }
        if let Some(s) = gid_to_unicode.get(&old) {
            if !s.is_empty() {
                entries.push((new, s));
            }
        }
    }

    let mut out = String::new();
    out.push_str(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );

    // bfchar blocks, max 100 entries each.
    for chunk in entries.chunks(100) {
        out.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (gid, text) in chunk {
            out.push_str(&format!("<{:04X}> <{}>\n", gid, utf16be_hex(text)));
        }
        out.push_str("endbfchar\n");
    }

    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out.into_bytes()
}

/// UTF-16BE hex encoding of a string (surrogate pairs for astral chars).
fn utf16be_hex(s: &str) -> String {
    let mut out = String::new();
    for unit in s.encode_utf16() {
        out.push_str(&format!("{unit:04X}"));
    }
    out
}

/// Strip characters not allowed in a PDF name from a font base name.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '+')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subset_tags_are_six_letters_and_deterministic() {
        assert_eq!(subset_tag(0), "AAAAAA");
        assert_eq!(subset_tag(1), "AAAAAB");
        assert_eq!(subset_tag(26), "AAAABA");
        assert_eq!(subset_tag(0).len(), 6);
    }

    #[test]
    fn glyph_space_scaling() {
        // 2048 upem: half-em advance (1024) -> 500 in glyph space.
        assert_eq!(to_glyph_space(1024.0, 2048), 500);
        assert_eq!(to_glyph_space(2048.0, 2048), 1000);
    }

    #[test]
    fn utf16_encoding() {
        assert_eq!(utf16be_hex("A"), "0041");
        assert_eq!(utf16be_hex("é"), "00E9");
        assert_eq!(utf16be_hex("\u{1F600}"), "D83DDE00"); // surrogate pair
    }
}

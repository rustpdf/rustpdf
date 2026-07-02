//! Parsing, metrics, descriptor data and subsetting (Fase 3A.1/3A.2/3C).

use std::collections::BTreeMap;

use subsetter::GlyphRemapper;
use ttf_parser::{Face, GlyphId};

/// Errors from font loading or subsetting.
#[derive(Debug)]
pub enum FontError {
    /// The bytes could not be parsed as a TrueType/OpenType face.
    Parse(String),
    /// Subsetting failed.
    Subset(String),
    /// I/O error while reading a font file.
    Io(String),
}

impl std::fmt::Display for FontError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FontError::Parse(s) => write!(f, "font parse error: {s}"),
            FontError::Subset(s) => write!(f, "font subset error: {s}"),
            FontError::Io(s) => write!(f, "font io error: {s}"),
        }
    }
}

impl std::error::Error for FontError {}

/// A loaded font face: owns its bytes so it is `Send` and self-contained.
///
/// `ttf-parser` borrows from the byte slice, so a fresh [`Face`] is parsed on
/// demand from `data` (cheap — it is a zero-allocation header scan). Frequently
/// used scalar metrics are cached at load time.
#[derive(Clone)]
pub struct Font {
    data: Vec<u8>,
    index: u32,
    units_per_em: u16,
    ascender: i16,
    descender: i16,
    line_gap: i16,
    cap_height: i16,
    x_height: i16,
    italic_angle: f32,
    weight: u16,
    is_italic: bool,
    is_monospaced: bool,
    is_serif: bool,
    num_glyphs: u16,
    bbox: [i16; 4],
    postscript_name: String,
    typo_ascender: Option<i16>,
    typo_descender: Option<i16>,
    typo_line_gap: Option<i16>,
    win_ascent: Option<i16>,
    win_descent: Option<i16>,
}

impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Font")
            .field("postscript_name", &self.postscript_name)
            .field("units_per_em", &self.units_per_em)
            .field("num_glyphs", &self.num_glyphs)
            .field("bytes", &self.data.len())
            .finish()
    }
}

impl Font {
    /// Parse a font from raw bytes (`index` selects a face in a collection).
    pub fn from_bytes(data: impl Into<Vec<u8>>, index: u32) -> Result<Font, FontError> {
        let data = data.into();
        let face = Face::parse(&data, index).map_err(|e| FontError::Parse(e.to_string()))?;

        // Prefer a decodable PostScript name (id 6); Macintosh-platform records
        // often fail to decode, so skip those that yield `None`.
        let postscript_name = face
            .names()
            .into_iter()
            .filter(|n| n.name_id == ttf_parser::name_id::POST_SCRIPT_NAME)
            .find_map(|n| n.to_string())
            .unwrap_or_else(|| "Subset".to_string());

        let bbox = face.global_bounding_box();
        let units_per_em = face.units_per_em();
        // Fallbacks keep the descriptor valid even on minimal faces.
        let ascender = face.ascender();
        let descender = face.descender();
        let cap_height = face
            .capital_height()
            .unwrap_or((ascender as f32 * 0.7) as i16);
        let x_height = face.x_height().unwrap_or((ascender as f32 * 0.5) as i16);

        // Raw OS/2 typographic + Windows metrics (independent of the
        // USE_TYPO_METRICS-aware `ascender()`/`descender()` above), used by
        // consumers that reproduce legacy line-box models (e.g. iText).
        let os2 = face.tables().os2;
        let typo_ascender = os2.map(|t| t.typographic_ascender());
        let typo_descender = os2.map(|t| t.typographic_descender());
        let typo_line_gap = os2.map(|t| t.typographic_line_gap());
        let win_ascent = os2.map(|t| t.windows_ascender());
        let win_descent = os2.map(|t| t.windows_descender());

        let font = Font {
            index,
            units_per_em,
            ascender,
            descender,
            line_gap: face.line_gap(),
            cap_height,
            x_height,
            italic_angle: face.italic_angle(),
            weight: face.weight().to_number(),
            is_italic: face.is_italic(),
            is_monospaced: face.is_monospaced(),
            is_serif: is_serif_by_name(&postscript_name),
            num_glyphs: face.number_of_glyphs(),
            bbox: [bbox.x_min, bbox.y_min, bbox.x_max, bbox.y_max],
            postscript_name,
            typo_ascender,
            typo_descender,
            typo_line_gap,
            win_ascent,
            win_descent,
            data,
        };
        Ok(font)
    }

    /// Read a font from a file path.
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Font, FontError> {
        let data = std::fs::read(path).map_err(|e| FontError::Io(e.to_string()))?;
        Font::from_bytes(data, 0)
    }

    /// Parse a fresh `ttf-parser` face borrowing this font's bytes.
    pub fn face(&self) -> Face<'_> {
        Face::parse(&self.data, self.index).expect("already validated at load")
    }

    /// The raw font bytes (the full, un-subsetted program).
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Face index within a font collection (0 for single-face files).
    pub fn index(&self) -> u32 {
        self.index
    }

    /// Design units per em (the coordinate space for all metrics below).
    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    /// Ascender in font units.
    pub fn ascender(&self) -> i16 {
        self.ascender
    }

    /// Descender in font units (typically negative).
    pub fn descender(&self) -> i16 {
        self.descender
    }

    /// Recommended extra line spacing in font units.
    pub fn line_gap(&self) -> i16 {
        self.line_gap
    }

    /// Capital height in font units.
    pub fn cap_height(&self) -> i16 {
        self.cap_height
    }

    /// OS/2 `sTypoAscender` in font units (`None` when the face has no OS/2
    /// table). Raw table value — NOT the USE_TYPO_METRICS-aware [`Self::ascender`].
    pub fn typo_ascender(&self) -> Option<i16> {
        self.typo_ascender
    }

    /// OS/2 `sTypoDescender` in font units (typically negative).
    pub fn typo_descender(&self) -> Option<i16> {
        self.typo_descender
    }

    /// OS/2 `sTypoLineGap` in font units.
    pub fn typo_line_gap(&self) -> Option<i16> {
        self.typo_line_gap
    }

    /// OS/2 `usWinAscent` in font units (positive).
    pub fn win_ascent(&self) -> Option<i16> {
        self.win_ascent
    }

    /// OS/2 `usWinDescent` in font units, **negated** (≤ 0, like
    /// [`Self::descender`]) — the table stores a positive magnitude.
    pub fn win_descent(&self) -> Option<i16> {
        self.win_descent
    }

    /// x-height in font units.
    pub fn x_height(&self) -> i16 {
        self.x_height
    }

    /// Italic angle in degrees (0 for upright fonts).
    pub fn italic_angle(&self) -> f32 {
        self.italic_angle
    }

    /// usWeightClass (100–900).
    pub fn weight(&self) -> u16 {
        self.weight
    }

    /// Total glyph count in the (un-subsetted) font.
    pub fn num_glyphs(&self) -> u16 {
        self.num_glyphs
    }

    /// PostScript name (used as the base of the subset font name).
    pub fn postscript_name(&self) -> &str {
        &self.postscript_name
    }

    /// Global glyph bounding box `[x_min, y_min, x_max, y_max]` in font units.
    pub fn bbox(&self) -> [i16; 4] {
        self.bbox
    }

    /// Map a Unicode scalar to a glyph id, if the font covers it.
    pub fn glyph_index(&self, c: char) -> Option<u16> {
        self.face().glyph_index(c).map(|g| g.0)
    }

    /// Horizontal advance of a glyph in font units (0 if unknown).
    pub fn advance(&self, gid: u16) -> u16 {
        self.face().glyph_hor_advance(GlyphId(gid)).unwrap_or(0)
    }

    /// PDF `FontDescriptor` flags (spec table 121).
    ///
    /// Bit 1 FixedPitch, bit 2 Serif, bit 3 Symbolic, bit 6 Nonsymbolic,
    /// bit 7 Italic. We treat text fonts as Nonsymbolic.
    pub fn descriptor_flags(&self) -> u32 {
        let mut flags = 0u32;
        if self.is_monospaced {
            flags |= 1 << 0; // FixedPitch
        }
        if self.is_serif {
            flags |= 1 << 1; // Serif
        }
        flags |= 1 << 5; // Nonsymbolic (bit 6)
        if self.is_italic || self.italic_angle != 0.0 {
            flags |= 1 << 6; // Italic (bit 7)
        }
        flags
    }

    /// A rough StemV estimate from weight (PDF requires the field; readers
    /// rarely use it precisely).
    pub fn stem_v(&self) -> f64 {
        // Linear-ish mapping: 400 -> ~80, 700 -> ~140.
        50.0 + (self.weight as f64) * 0.12
    }

    /// Build a subset containing exactly `old_gids` (glyph 0 / `.notdef` is
    /// always included by the subsetter). Returns the new font program plus the
    /// old↔new glyph-id remapping (Fase 3C).
    pub fn subset(&self, old_gids: &[u16]) -> Result<Subset, FontError> {
        let mut remapper = GlyphRemapper::new();
        for &g in old_gids {
            remapper.remap(g);
        }
        let data = subsetter::subset(&self.data, self.index, &remapper)
            .map_err(|e| FontError::Subset(e.to_string()))?;

        let mut old_to_new = BTreeMap::new();
        let mut new_to_old = BTreeMap::new();
        // The subsetter always keeps glyph 0.
        old_to_new.insert(0u16, 0u16);
        new_to_old.insert(0u16, 0u16);
        for &old in old_gids {
            if let Some(new) = remapper.get(old) {
                old_to_new.insert(old, new);
                new_to_old.insert(new, old);
            }
        }
        let num_glyphs = remapper.num_gids();
        Ok(Subset {
            data,
            old_to_new,
            new_to_old,
            num_glyphs,
        })
    }
}

/// The product of subsetting: the new font program and the glyph-id remapping.
///
/// In the PDF, the new glyph ids double as CIDs (`CIDToGIDMap` is `Identity`),
/// so `new_gid` is what gets written into content streams and keyed in the `W`
/// width array and `ToUnicode` map.
#[derive(Debug, Clone)]
pub struct Subset {
    /// The subset font program (goes into `FontFile2`).
    pub data: Vec<u8>,
    old_to_new: BTreeMap<u16, u16>,
    new_to_old: BTreeMap<u16, u16>,
    num_glyphs: u16,
}

impl Subset {
    /// New glyph id for an original glyph id, if it was included.
    pub fn new_gid(&self, old_gid: u16) -> Option<u16> {
        self.old_to_new.get(&old_gid).copied()
    }

    /// Original glyph id for a new glyph id.
    pub fn old_gid(&self, new_gid: u16) -> Option<u16> {
        self.new_to_old.get(&new_gid).copied()
    }

    /// Number of glyphs in the subset (== highest new gid + 1).
    pub fn num_glyphs(&self) -> u16 {
        self.num_glyphs
    }

    /// Iterate `(new_gid, old_gid)` pairs in ascending new-gid order.
    pub fn iter_new(&self) -> impl Iterator<Item = (u16, u16)> + '_ {
        self.new_to_old.iter().map(|(&n, &o)| (n, o))
    }
}

/// Heuristic serif detection from the font name. The PDF Serif flag is not
/// render-critical, so a name match is sufficient; default is sans (false).
fn is_serif_by_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        "serif",
        "times",
        "georgia",
        "garamond",
        "roman",
        "minion",
        "merriweather",
    ]
    .iter()
    .any(|kw| n.contains(kw))
        && !n.contains("sans")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roboto() -> Font {
        Font::from_file(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/fonts/Roboto-Regular.ttf"
        ))
        .expect("load Roboto")
    }

    #[test]
    fn parses_metrics() {
        let f = roboto();
        // Known Roboto design values.
        assert_eq!(f.units_per_em(), 2048);
        assert!(f.ascender() > 0 && f.descender() < 0);
        assert!(f.num_glyphs() > 1000);
        assert!(f.postscript_name().contains("Roboto"));
        assert!(f.descriptor_flags() & (1 << 5) != 0, "Nonsymbolic flag");
    }

    #[test]
    fn glyph_index_and_advance() {
        let f = roboto();
        let a = f.glyph_index('A').expect("A present");
        assert!(a > 0);
        assert!(f.advance(a) > 0);
        assert_eq!(f.glyph_index('\u{1F600}'), None); // emoji not in Roboto
    }

    #[test]
    fn subset_is_smaller_and_remaps() {
        let f = roboto();
        let gids: Vec<u16> = "Hello".chars().map(|c| f.glyph_index(c).unwrap()).collect();
        let subset = f.subset(&gids).expect("subset");

        // Fase 3C milestone: subset is dramatically smaller (>70%).
        let ratio = subset.data.len() as f64 / f.data().len() as f64;
        assert!(
            ratio < 0.30,
            "subset only {:.0}% smaller",
            (1.0 - ratio) * 100.0
        );

        // Glyph 0 is preserved; each used glyph gets a new id; new ids are dense.
        assert_eq!(subset.new_gid(0), Some(0));
        for &old in &gids {
            assert!(subset.new_gid(old).is_some());
        }
        // The subset is itself a valid, parseable font.
        let reparsed = Font::from_bytes(subset.data.clone(), 0).expect("subset parses");
        assert!(reparsed.num_glyphs() as usize <= gids.len() + 2);
    }
}

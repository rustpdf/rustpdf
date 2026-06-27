//! Text shaping via `rustybuzz` (Fase 3D.1, and the engine behind complex
//! scripts in 3E). Shaping turns a Unicode string into positioned glyphs,
//! applying kerning (GPOS/`kern`), ligatures (GSUB) and, for complex scripts,
//! contextual forms and reordering.

use crate::font::Font;

/// Writing direction handed to the shaper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    LeftToRight,
    RightToLeft,
}

/// One positioned glyph from shaping. All metrics are in font design units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapedGlyph {
    /// Glyph id in the (original, un-subsetted) font.
    pub gid: u16,
    /// Byte offset of the source cluster in the input string.
    pub cluster: u32,
    /// Horizontal advance applied after this glyph.
    pub x_advance: i32,
    /// Vertical advance (0 for horizontal text).
    pub y_advance: i32,
    /// Horizontal positioning offset for this glyph.
    pub x_offset: i32,
    /// Vertical positioning offset for this glyph.
    pub y_offset: i32,
}

/// Shape `text` with `font` in the given direction.
///
/// Script and language are auto-detected. The returned glyphs are in *visual*
/// order for the run (the shaper reverses RTL runs).
pub fn shape(font: &Font, text: &str, dir: Direction) -> Vec<ShapedGlyph> {
    let face = match rustybuzz::Face::from_slice(font.data(), font.index()) {
        Some(f) => f,
        None => return Vec::new(),
    };

    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.set_direction(match dir {
        Direction::LeftToRight => rustybuzz::Direction::LeftToRight,
        Direction::RightToLeft => rustybuzz::Direction::RightToLeft,
    });
    // Fill in script/language from the buffer contents.
    buffer.guess_segment_properties();

    let glyphs = rustybuzz::shape(&face, &[], buffer);
    let infos = glyphs.glyph_infos();
    let positions = glyphs.glyph_positions();

    infos
        .iter()
        .zip(positions.iter())
        .map(|(info, pos)| ShapedGlyph {
            gid: info.glyph_id as u16,
            cluster: info.cluster,
            x_advance: pos.x_advance,
            y_advance: pos.y_advance,
            x_offset: pos.x_offset,
            y_offset: pos.y_offset,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roboto() -> Font {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/fonts/Roboto-Regular.ttf"
        );
        Font::from_file(path).expect("load Roboto")
    }

    #[test]
    fn shapes_simple_word() {
        let font = roboto();
        let glyphs = shape(&font, "Hello", Direction::LeftToRight);
        assert_eq!(glyphs.len(), 5);
        // Clusters are the byte offsets 0..5 for ASCII.
        assert_eq!(glyphs[0].cluster, 0);
        assert!(glyphs.iter().all(|g| g.x_advance > 0));
    }

    #[test]
    fn kerning_tightens_av() {
        let font = roboto();
        // "AV" kerns negative: the shaped advance of 'A' is less than its raw
        // horizontal advance.
        let glyphs = shape(&font, "AV", Direction::LeftToRight);
        let a_gid = font.glyph_index('A').unwrap();
        let raw = font.advance(a_gid) as i32;
        assert_eq!(glyphs[0].gid, a_gid);
        assert!(
            glyphs[0].x_advance < raw,
            "expected kerning: shaped {} < raw {}",
            glyphs[0].x_advance,
            raw
        );
    }
}

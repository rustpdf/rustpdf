//! Adobe Core-14 **Helvetica** glyph metrics (AFM), used to measure text drawn
//! with the standard (non-embedded) Helvetica font — e.g. aligned/centered
//! [`EditableDoc::place_text`](crate::EditableDoc::place_text) and
//! [`masked_text`](crate::EditableDoc::masked_text). Widths are in 1000-unit
//! glyph space, indexed by **WinAnsi** code (the encoding `place_text` writes).
//! For ASCII and Latin-1 (U+0020–U+00FF) the Unicode scalar equals the WinAnsi
//! code, so the table is indexed directly; the handful of WinAnsi typographic
//! glyphs that live at higher Unicode points (smart quotes, dashes, …) are
//! mapped explicitly. Anything else falls back to the lowercase-`n` average.

/// Helvetica cap height in 1000-unit glyph space (top of an uppercase letter
/// above the baseline) — the visual height used to vertically center a line.
pub const CAP_HEIGHT: f64 = 718.0;

/// WinAnsi-indexed advance widths (1000 units/em). Control codes are 0.
#[rustfmt::skip]
const WIDTHS: [u16; 256] = [
    // 0x00–0x1F control
    0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
    0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,
    // 0x20–0x2F  space ! " # $ % & ' ( ) * + , - . /
    278,278,355,556,556,889,667,191,333,333,389,584,278,333,278,278,
    // 0x30–0x39  0-9   0x3A–0x3F : ; < = > ?
    556,556,556,556,556,556,556,556,556,556,278,278,584,584,584,556,
    // 0x40 @  0x41–0x4F A-O
    1015,667,667,722,722,667,611,778,722,278,500,667,556,833,722,778,
    // 0x50–0x5A P-Z  0x5B–0x60 [ \ ] ^ _ `
    667,778,722,667,611,722,667,944,667,667,611,278,278,278,469,556,333,
    // 0x61–0x6F a-o
    556,556,500,556,556,278,556,556,222,222,500,222,833,556,556,
    // 0x70–0x7A p-z  0x7B–0x7E { | } ~  0x7F
    556,556,333,500,278,556,500,722,500,500,500,334,260,334,584,0,
    // 0x80–0x8F  WinAnsi typographic (mostly unused via Unicode; kept for direct bytes)
    556,0,222,556,333,1000,556,556,333,1000,667,333,1000,0,611,0,
    // 0x90–0x9F
    0,222,222,333,333,350,556,1000,333,1000,500,333,944,0,500,667,
    // 0xA0–0xAF  nbsp ¡ ¢ £ ¤ ¥ ¦ § ¨ © ª « ¬ ­ ® ¯
    278,333,556,556,556,556,260,556,333,737,370,556,584,333,737,333,
    // 0xB0–0xBF  ° ± ² ³ ´ µ ¶ · ¸ ¹ º » ¼ ½ ¾ ¿
    400,584,333,333,333,556,537,278,333,333,365,556,834,834,834,611,
    // 0xC0–0xCF  À-Ï
    667,667,667,667,667,667,1000,722,667,667,667,667,278,278,278,278,
    // 0xD0–0xDF  Ð-ß
    722,722,778,778,778,778,778,584,778,722,722,722,722,667,667,611,
    // 0xE0–0xEF  à-ï
    556,556,556,556,556,556,889,500,556,556,556,556,278,278,278,278,
    // 0xF0–0xFF  ð-ÿ
    556,556,556,556,556,556,556,584,611,556,556,556,556,500,556,500,
];

/// Advance width of `c` in 1000-unit glyph space.
fn char_width(c: char) -> u16 {
    let code = c as u32;
    if code < 256 {
        let w = WIDTHS[code as usize];
        if w != 0 {
            return w;
        }
    }
    match c {
        '\u{2018}' | '\u{2019}' => 222, // ‘ ’
        '\u{201A}' => 222,              // ‚
        '\u{201C}' | '\u{201D}' => 333, // “ ”
        '\u{201E}' => 333,              // „
        '\u{2013}' => 556,              // – en dash
        '\u{2014}' => 1000,             // — em dash
        '\u{2022}' => 350,              // •
        '\u{2026}' => 1000,             // …
        '\u{20AC}' => 556,              // €
        '\u{2122}' => 1000,             // ™
        _ => 556,                       // average (lowercase n / digit)
    }
}

/// Width of `text` rendered in Helvetica at `size` points.
pub fn text_width(text: &str, size: f64) -> f64 {
    let units: u32 = text.chars().map(|c| char_width(c) as u32).sum();
    units as f64 / 1000.0 * size
}

/// Map a Unicode scalar to its **WinAnsi (CP1252)** byte, or `None` when the
/// character is not representable. ASCII (`0x20..=0x7E`) and Latin-1
/// (`0xA0..=0xFF`) map by identity; the CP1252 `0x80..=0x9F` block holds
/// typographic glyphs at scattered Unicode points (smart quotes, dashes, €, …).
/// Used to transcode text drawn with a WinAnsi-encoded standard font, instead of
/// passing raw UTF-8 bytes (which would render accented chars as mojibake).
pub(crate) fn unicode_to_winansi(c: char) -> Option<u8> {
    let u = c as u32;
    match u {
        0x20..=0x7E | 0xA0..=0xFF => Some(u as u8),
        _ => Some(match c {
            '\u{20AC}' => 0x80, // €
            '\u{201A}' => 0x82, // ‚
            '\u{0192}' => 0x83, // ƒ
            '\u{201E}' => 0x84, // „
            '\u{2026}' => 0x85, // …
            '\u{2020}' => 0x86, // †
            '\u{2021}' => 0x87, // ‡
            '\u{02C6}' => 0x88, // ˆ
            '\u{2030}' => 0x89, // ‰
            '\u{0160}' => 0x8A, // Š
            '\u{2039}' => 0x8B, // ‹
            '\u{0152}' => 0x8C, // Œ
            '\u{017D}' => 0x8E, // Ž
            '\u{2018}' => 0x91, // ‘
            '\u{2019}' => 0x92, // ’
            '\u{201C}' => 0x93, // “
            '\u{201D}' => 0x94, // ”
            '\u{2022}' => 0x95, // •
            '\u{2013}' => 0x96, // – en dash
            '\u{2014}' => 0x97, // — em dash
            '\u{02DC}' => 0x98, // ˜
            '\u{2122}' => 0x99, // ™
            '\u{0161}' => 0x9A, // š
            '\u{203A}' => 0x9B, // ›
            '\u{0153}' => 0x9C, // œ
            '\u{017E}' => 0x9E, // ž
            '\u{0178}' => 0x9F, // Ÿ
            _ => return None,
        }),
    }
}

/// Decode a **WinAnsi (CP1252)** byte to its Unicode scalar — the inverse of
/// [`unicode_to_winansi`]. The `0x80..=0x9F` block maps to its typographic
/// glyphs; everything else is identity (ASCII + Latin-1). Used by text
/// extraction to recover text from a WinAnsi-encoded font that carries no
/// `ToUnicode` map (the standard-14 fonts), so e.g. byte `0x97` reads back as an
/// em dash instead of a U+0097 control character.
pub(crate) fn winansi_to_unicode(b: u8) -> char {
    match b {
        0x80 => '\u{20AC}',
        0x82 => '\u{201A}',
        0x83 => '\u{0192}',
        0x84 => '\u{201E}',
        0x85 => '\u{2026}',
        0x86 => '\u{2020}',
        0x87 => '\u{2021}',
        0x88 => '\u{02C6}',
        0x89 => '\u{2030}',
        0x8A => '\u{0160}',
        0x8B => '\u{2039}',
        0x8C => '\u{0152}',
        0x8E => '\u{017D}',
        0x91 => '\u{2018}',
        0x92 => '\u{2019}',
        0x93 => '\u{201C}',
        0x94 => '\u{201D}',
        0x95 => '\u{2022}',
        0x96 => '\u{2013}',
        0x97 => '\u{2014}',
        0x98 => '\u{02DC}',
        0x99 => '\u{2122}',
        0x9A => '\u{0161}',
        0x9B => '\u{203A}',
        0x9C => '\u{0153}',
        0x9E => '\u{017E}',
        0x9F => '\u{0178}',
        // 0x00–0x7F and 0xA0–0xFF are identity; 0x81/0x8D/0x8F/0x90/0x9D unused.
        _ => b as char,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_widths() {
        // "AV" = 667 + 667 = 1334 units → 13.34 pt at size 10.
        assert!((text_width("AV", 10.0) - 13.34).abs() < 1e-6);
        // Accented Portuguese letters reuse their base width (á = a = 556).
        assert_eq!(text_width("á", 1000.0).round() as i64, 556);
        assert_eq!(text_width("ç", 1000.0).round() as i64, 500);
        // Empty string has zero width.
        assert_eq!(text_width("", 12.0), 0.0);
    }
}

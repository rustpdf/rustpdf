//! FINDING-003: `EditableDoc` needs width-bounded, word-wrapping text
//! stamping (`place_paragraph`/`place_paragraph_with_font`) — the stamping
//! counterpart of `Document`'s `Paragraph`, matching iText
//! `Paragraph.SetFixedPosition(x, y, width)` (+ `SetMaxHeight`): `(x, y)` is
//! the **top-left** of the box, lines break greedily by word inside `width`,
//! and `max_height` truncates the overflow.
//!
//! Assertions parse the emitted content stream (`Tm` translations, `Tj`/`TJ`
//! runs, `Tw` word spacing), pinning break points and baselines in points.

use pdf::{Align, Document, EditableDoc};

const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
);

/// Helvetica AFM widths used by the assertions (em fractions): lowercase 'a'
/// (and 'b', 'c' — 556) and space (278); ascent 718.
const W_A: f64 = 0.556;
const W_SPACE: f64 = 0.278;
const HELV_ASCENT: f64 = 0.718;
const HELV_DESCENT: f64 = -0.207;

const BLACK: (f64, f64, f64) = (0.0, 0.0, 0.0);

fn base() -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_page_sized(400.0, 500.0);
    doc.to_bytes().unwrap()
}

/// All `Tm` translations `(sx, sy)` in the serialized bytes, in emission order
/// (stamp content streams are uncompressed; the blank base page has no text).
fn tms(pdf: &[u8]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = pdf[from..].windows(3).position(|w| w == b" Tm") {
        let pos = from + rel;
        let line_start = pdf[..pos].iter().rposition(|&b| b == b'\n').unwrap() + 1;
        if let Ok(line) = std::str::from_utf8(&pdf[line_start..pos]) {
            let nums: Vec<f64> = line
                .split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect();
            if nums.len() == 6 {
                out.push((nums[4], nums[5]));
            }
        }
        from = pos + 3;
    }
    out
}

fn text_of(pdf: &[u8]) -> String {
    String::from_utf8_lossy(pdf).into_owned()
}

fn assert_close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 0.011,
        "{what}: expected {expected:.3}, got {actual:.3}"
    );
}

// ---- built-in Helvetica ------------------------------------------------------

#[test]
fn wraps_words_within_width_and_stacks_lines() {
    let size = 12.0;
    // Three 4-char words of 'a' (4×0.556 em = 26.688 pt each). A 30 pt box
    // fits exactly one word per line → 3 lines.
    let mut ed = EditableDoc::load(base()).unwrap();
    let drawn = ed.place_paragraph(
        0,
        50.0,
        400.0,
        30.0,
        "aaaa aaaa aaaa",
        size,
        BLACK,
        Align::Left,
        None,
        1.0,
    );
    assert_eq!(drawn, Some(3));
    let bytes = ed.to_bytes().unwrap();
    let tms = tms(&bytes);
    assert_eq!(tms.len(), 3, "one Tm per wrapped line");
    // Top-anchored first baseline (FINDING-002 semantics), then 1.2em leading.
    let first = 400.0 - HELV_ASCENT * size;
    for (i, &(sx, sy)) in tms.iter().enumerate() {
        assert_close(sx, 50.0, "left-aligned line x");
        assert_close(sy, first - i as f64 * size * 1.2, "stacked baseline");
    }
}

#[test]
fn fits_multiple_words_per_line_like_document_paragraph() {
    let size = 10.0;
    // Word "aa" = 11.12 pt, space = 2.78 pt. Two words = 25.02 pt; three would
    // be 38.92 pt. Box of 26 pt → two words per line: "aa aa" ×2 → 2 lines.
    let mut ed = EditableDoc::load(base()).unwrap();
    let drawn = ed.place_paragraph(
        0,
        50.0,
        400.0,
        26.0,
        "aa aa aa aa",
        size,
        BLACK,
        Align::Left,
        None,
        1.0,
    );
    assert_eq!(drawn, Some(2));
    let bytes = ed.to_bytes().unwrap();
    assert_eq!(text_of(&bytes).matches("(aa aa) Tj").count(), 2);
}

#[test]
fn align_right_lines_end_at_box_right_edge() {
    let size = 12.0;
    let word_w = 4.0 * W_A * size; // 26.688
    let mut ed = EditableDoc::load(base()).unwrap();
    ed.place_paragraph(
        0,
        50.0,
        400.0,
        30.0,
        "aaaa aaaa",
        size,
        BLACK,
        Align::Right,
        None,
        1.0,
    )
    .unwrap();
    let bytes = ed.to_bytes().unwrap();
    for &(sx, _) in &tms(&bytes) {
        assert_close(sx, 50.0 + 30.0 - word_w, "right-aligned line start");
    }
}

#[test]
fn justify_stretches_gaps_except_last_line() {
    let size = 10.0;
    // Box 26 pt, words "aa" (11.12) → two per line, natural 25.02, 1 gap.
    let mut ed = EditableDoc::load(base()).unwrap();
    ed.place_paragraph(
        0,
        50.0,
        400.0,
        26.0,
        "aa aa aa aa",
        size,
        BLACK,
        Align::Justify,
        None,
        1.0,
    )
    .unwrap();
    let content = text_of(&ed.to_bytes().unwrap());
    let natural = 2.0 * (2.0 * W_A * size) + W_SPACE * size; // 25.02
    let extra = 26.0 - natural; // 0.98 stretched into the single gap
    assert!(
        content.contains(&format!("{extra:.3} Tw")),
        "interior line must stretch its gap by {extra:.3}: {content}"
    );
    assert!(
        content.contains("0.000 Tw"),
        "last line must reset word spacing"
    );
}

#[test]
fn max_height_truncates_overflow_lines() {
    let size = 12.0;
    // 3 wrapped lines; line i bottom depth = (asc − desc)·size + i·leading
    // = 11.1 + i·14.4. max_height 30 keeps lines 0 and 1, cuts line 2.
    let mut ed = EditableDoc::load(base()).unwrap();
    let drawn = ed.place_paragraph(
        0,
        50.0,
        400.0,
        30.0,
        "aaaa aaaa aaaa",
        size,
        BLACK,
        Align::Left,
        Some(30.0),
        1.0,
    );
    assert_eq!(drawn, Some(2), "third line must be cut by max_height");
    assert_eq!(tms(&ed.to_bytes().unwrap()).len(), 2);
    let _ = HELV_DESCENT;
}

#[test]
fn newline_forces_break_and_line_height_scales_leading() {
    let size = 12.0;
    let mut ed = EditableDoc::load(base()).unwrap();
    let drawn = ed.place_paragraph(
        0,
        50.0,
        400.0,
        200.0,
        "aaaa\nbbbb",
        size,
        BLACK,
        Align::Left,
        None,
        1.5,
    );
    assert_eq!(drawn, Some(2));
    let tms = tms(&ed.to_bytes().unwrap());
    assert_eq!(tms.len(), 2);
    let step = tms[0].1 - tms[1].1;
    assert_close(step, size * 1.2 * 1.5, "line_height multiplies the leading");
}

#[test]
fn invalid_page_or_width_returns_none() {
    let mut ed = EditableDoc::load(base()).unwrap();
    assert_eq!(
        ed.place_paragraph(9, 0.0, 0.0, 100.0, "x", 12.0, BLACK, Align::Left, None, 1.0),
        None
    );
    assert_eq!(
        ed.place_paragraph(0, 0.0, 0.0, 0.0, "x", 12.0, BLACK, Align::Left, None, 1.0),
        None
    );
}

// ---- embedded font -----------------------------------------------------------

#[test]
fn embedded_font_wraps_with_real_shaped_widths() {
    let size = 12.0;
    let font = fonts::Font::from_file(FONT).unwrap();
    let upem = font.units_per_em() as f64;
    let asc = font.ascender() as f64 / upem;
    // Measure "aaaa" with the same shaper the wrapper uses.
    let word_w: f64 = {
        let glyphs = fonts::shape(&font, "aaaa", fonts::Direction::LeftToRight);
        size * glyphs.iter().map(|g| g.x_advance as i64).sum::<i64>() as f64 / upem
    };

    let mut ed = EditableDoc::load(base()).unwrap();
    let id = ed.add_font_file(FONT).unwrap();
    // Box slightly wider than one word but narrower than two → one word/line.
    let drawn = ed.place_paragraph_with_font(
        0,
        50.0,
        400.0,
        word_w * 1.2,
        "aaaa aaaa aaaa",
        size,
        BLACK,
        Align::Left,
        id,
        None,
        1.0,
    );
    assert_eq!(drawn, Some(3));
    let bytes = ed.to_bytes().unwrap();
    let tms = tms(&bytes);
    assert_eq!(tms.len(), 3);
    assert_close(tms[0].1, 400.0 - asc * size, "font-metric top anchor");
    assert_close(tms[1].1, tms[0].1 - size * 1.2, "leading step");
}

#[test]
fn embedded_font_justify_emits_tj_gap_adjustments() {
    let size = 12.0;
    let mut ed = EditableDoc::load(base()).unwrap();
    let id = ed.add_font_file(FONT).unwrap();
    let word_w = {
        let font = fonts::Font::from_file(FONT).unwrap();
        let glyphs = fonts::shape(&font, "aaaa", fonts::Direction::LeftToRight);
        size * glyphs.iter().map(|g| g.x_advance as i64).sum::<i64>() as f64
            / font.units_per_em() as f64
    };
    // Two words per line, then a last line — interior line justifies via TJ.
    let drawn = ed.place_paragraph_with_font(
        0,
        50.0,
        400.0,
        word_w * 2.4,
        "aaaa aaaa aaaa",
        size,
        BLACK,
        Align::Justify,
        id,
        None,
        1.0,
    );
    assert_eq!(drawn, Some(2));
    let content = text_of(&ed.to_bytes().unwrap());
    assert!(
        content.contains("] TJ"),
        "justified CID line must use a TJ array (Tw is a no-op for 2-byte CIDs)"
    );
}

#[test]
fn embedded_font_paragraph_text_roundtrips_via_extraction() {
    // The per-line shaped runs must keep the ToUnicode pipeline intact.
    let mut ed = EditableDoc::load(base()).unwrap();
    let id = ed.add_font_file(FONT).unwrap();
    ed.place_paragraph_with_font(
        0,
        50.0,
        400.0,
        120.0,
        "primeira linha longa que quebra aqui",
        12.0,
        BLACK,
        Align::Left,
        id,
        None,
        1.0,
    )
    .unwrap();
    let bytes = ed.to_bytes().unwrap();
    let text = pdf::extract_text(&bytes).unwrap();
    for word in ["primeira", "linha", "quebra", "aqui"] {
        assert!(text.contains(word), "extraction lost {word:?}: {text}");
    }
}

// ---- block anchors (bottom-pin + ceiling; line-box leading behind Line*) ----

use pdf::VerticalAnchor;

/// Helvetica iText line box (typo × 1.2 + 0.21 half-leading) — mirrors
/// `stamp_line_metrics`.
const HELV_LINE_ASC: f64 = 1.2 * 0.718 + 0.21 * 0.925;
const HELV_LINE_DESC: f64 = 1.2 * 0.207 + 0.21 * 0.925;
/// LineTop/LineBottom leading basis = the iText multiplied-leading advance:
/// selected metrics (typo × 1.2 for Helvetica) + (1.35 − 1) em.
const HELV_LINE_LEADING: f64 = 1.2 * (0.718 + 0.207) + 0.35;

#[test]
fn line_bottom_anchor_bottom_pins_block_on_y() {
    let size = 12.0;
    // 3 wrapped lines, no max_height: the block's bottom (last line's box
    // bottom) rests on y; lines stack upward by the LINE-BOX leading.
    let mut ed = EditableDoc::load(base()).unwrap();
    let leading = size * HELV_LINE_LEADING;
    let (drawn, height) = ed
        .place_paragraph_anchored(
            0,
            50.0,
            100.0,
            30.0,
            "aaaa aaaa aaaa",
            size,
            BLACK,
            Align::Left,
            None,
            1.0,
            VerticalAnchor::LineBottom,
            0.0,
        )
        .unwrap();
    assert_eq!(drawn, 3);
    let tms = tms(&ed.to_bytes().unwrap());
    assert_eq!(tms.len(), 3);
    assert_close(tms[2].1, 100.0 + HELV_LINE_DESC * size, "last line on y");
    assert_close(
        tms[0].1,
        100.0 + 2.0 * leading + HELV_LINE_DESC * size,
        "first baseline of bottom-pinned block",
    );
    assert_close(
        height,
        (HELV_LINE_ASC + HELV_LINE_DESC) * size + 2.0 * leading,
        "consumed height",
    );
}

#[test]
fn bottom_pin_ignores_roomy_max_height() {
    // Item A: a ceiling LARGER than the content must not inflate the position
    // (the old behavior filled the box from its top → dY +114 in the bench).
    let size = 12.0;
    let mut ed_no_mh = EditableDoc::load(base()).unwrap();
    ed_no_mh
        .place_paragraph_anchored(
            0,
            50.0,
            100.0,
            30.0,
            "aaaa aaaa aaaa",
            size,
            BLACK,
            Align::Left,
            None,
            1.0,
            VerticalAnchor::LineBottom,
            0.0,
        )
        .unwrap();
    let mut ed_mh = EditableDoc::load(base()).unwrap();
    ed_mh
        .place_paragraph_anchored(
            0,
            50.0,
            100.0,
            30.0,
            "aaaa aaaa aaaa",
            size,
            BLACK,
            Align::Left,
            Some(168.0),
            1.0,
            VerticalAnchor::LineBottom,
            0.0,
        )
        .unwrap();
    assert_eq!(
        tms(&ed_no_mh.to_bytes().unwrap()),
        tms(&ed_mh.to_bytes().unwrap()),
        "a roomy ceiling must not move a bottom-pinned block"
    );
}

#[test]
fn bottom_pin_ceiling_cuts_overflow_from_the_top() {
    let size = 12.0;
    // 4 wrapped lines; single-line box = 17.98pt, leading = 17.52pt →
    // 2 lines = 35.50, 3 lines = 53.02. Ceiling 40 keeps the LAST 2 lines
    // pinned to y; the first 2 are cut from the top.
    let mut ed = EditableDoc::load(base()).unwrap();
    let (drawn, height) = ed
        .place_paragraph_anchored(
            0,
            50.0,
            100.0,
            30.0,
            "aaaa bbbb cccc aaaa",
            size,
            BLACK,
            Align::Left,
            Some(40.0),
            1.0,
            VerticalAnchor::LineBottom,
            0.0,
        )
        .unwrap();
    assert_eq!(drawn, 2, "ceiling must keep only the last 2 lines");
    assert!(height <= 40.0 + 0.01);
    let bytes = ed.to_bytes().unwrap();
    let content = text_of(&bytes);
    // The kept lines are the LAST wrapped ones ("cccc", "aaaa"), still pinned.
    assert!(content.contains("(cccc) Tj") && !content.contains("(bbbb) Tj"));
    let tms = tms(&bytes);
    assert_close(
        tms.last().unwrap().1,
        100.0 + HELV_LINE_DESC * size,
        "last line stays pinned to y under the ceiling",
    );
}

#[test]
fn geometric_bottom_anchor_uses_hhea_metrics_and_plain_leading() {
    let size = 12.0;
    // `Bottom` (geometric) bottom-pins with hhea descent + the plain 1.2 em
    // leading — distinct from `LineBottom` (line box).
    let mut ed = EditableDoc::load(base()).unwrap();
    ed.place_paragraph_anchored(
        0,
        50.0,
        100.0,
        30.0,
        "aaaa aaaa",
        size,
        BLACK,
        Align::Left,
        None,
        1.0,
        VerticalAnchor::Bottom,
        0.0,
    )
    .unwrap();
    let tms = tms(&ed.to_bytes().unwrap());
    assert_close(tms[1].1, 100.0 - HELV_DESCENT * size, "hhea descender on y");
    assert_close(tms[0].1 - tms[1].1, size * 1.2, "plain 1.2 em leading");
}

#[test]
fn paragraph_baseline_anchor_puts_first_baseline_at_y() {
    let mut ed = EditableDoc::load(base()).unwrap();
    ed.place_paragraph_anchored(
        0,
        50.0,
        400.0,
        30.0,
        "aaaa aaaa",
        12.0,
        BLACK,
        Align::Left,
        None,
        1.0,
        VerticalAnchor::Baseline,
        0.0,
    )
    .unwrap();
    let tms = tms(&ed.to_bytes().unwrap());
    assert_close(tms[0].1, 400.0, "baseline anchor");
}

#[test]
fn paragraph_rotation_pivots_about_the_anchor() {
    // Item C: rotating the block must leave the anchor point invariant. With
    // Baseline anchor and Left align, the first line starts AT the anchor for
    // any rotation; the second line's offset rotates with the block.
    let size = 12.0;
    let mut ed = EditableDoc::load(base()).unwrap();
    ed.place_paragraph_anchored(
        0,
        50.0,
        200.0,
        30.0,
        "aaaa aaaa",
        size,
        BLACK,
        Align::Left,
        None,
        1.0,
        VerticalAnchor::Baseline,
        90.0,
    )
    .unwrap();
    let tms = tms(&ed.to_bytes().unwrap());
    assert_close(tms[0].0, 50.0, "anchor x invariant under rotation");
    assert_close(tms[0].1, 200.0, "anchor y invariant under rotation");
    // Second line: local (0, −leading) rotated 90° CCW → global (+leading, 0).
    assert_close(tms[1].0, 50.0 + size * 1.2, "rotated leading direction x");
    assert_close(tms[1].1, 200.0, "rotated leading direction y");
}

// ---- masked_text pad (FINDING-004 follow-up: dX = 1.8) -------------------------

#[test]
fn masked_text_zero_pad_starts_flush_with_box_edge() {
    use pdf::VerticalAlign;
    let mut ed = EditableDoc::load(base()).unwrap();
    // Default inset = 0.15 × 12 = 1.8 pt (the measured dX); pad 0 = flush.
    assert!(ed.masked_text_padded(
        0,
        40.0,
        300.0,
        200.0,
        42.0,
        "Hello",
        12.0,
        BLACK,
        (1.0, 1.0, 1.0),
        Align::Left,
        VerticalAlign::Top,
        Some(0.0),
    ));
    let tms_flush = tms(&ed.to_bytes().unwrap());
    assert_close(tms_flush.last().unwrap().0, 40.0, "flush left edge");

    let mut ed = EditableDoc::load(base()).unwrap();
    assert!(ed.masked_text(
        0,
        40.0,
        300.0,
        200.0,
        42.0,
        "Hello",
        12.0,
        BLACK,
        (1.0, 1.0, 1.0),
        Align::Left
    ));
    let tms_default = tms(&ed.to_bytes().unwrap());
    assert_close(
        tms_default.last().unwrap().0,
        41.8,
        "historical 1.8pt inset",
    );
}

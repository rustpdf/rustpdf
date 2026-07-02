//! FINDING-002: the vertical anchor of stamped text must be explicit and
//! controllable. `place_text*` historically anchors `y` at the **baseline**
//! (kept as the default), but legacy stacks anchor differently: iText
//! `SetFixedPosition` hangs text from the **top** of its box, and Syncfusion
//! `DrawString` with `LineAlignment = Top` hangs the line from the top of the
//! rect — while our `masked_text` centered it. The new `VerticalAnchor`
//! (place_text) and `VerticalAlign` (masked_text) parameters resolve
//! `Top`/`Bottom` with the **selected font's** ascent/descent (embedded font
//! metrics, or Helvetica AFM for the built-in stamps).
//!
//! The assertions read the `Tm` translation actually written into the content
//! stream, so they pin the baseline math in points.

use pdf::{Align, Document, EditableDoc, VerticalAlign, VerticalAnchor};

const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
);

/// Helvetica AFM vertical metrics in em fractions (mirrors `helvetica.rs`).
const HELV_ASCENT: f64 = 0.718;
const HELV_DESCENT: f64 = -0.207;

const BLACK: (f64, f64, f64) = (0.0, 0.0, 0.0);
const WHITE: (f64, f64, f64) = (1.0, 1.0, 1.0);

fn base() -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_page_sized(400.0, 500.0);
    doc.to_bytes().unwrap()
}

/// Parse the translation of the **last** `Tm` in the serialized bytes (stamp
/// content streams are appended uncompressed), returning `(sx, sy)`.
fn last_tm(pdf: &[u8]) -> (f64, f64) {
    let pos = pdf
        .windows(3)
        .rposition(|w| w == b" Tm")
        .expect("no Tm operator in output");
    let line_start = pdf[..pos].iter().rposition(|&b| b == b'\n').unwrap() + 1;
    let line = std::str::from_utf8(&pdf[line_start..pos]).unwrap();
    let nums: Vec<f64> = line
        .split_whitespace()
        .map(|t| t.parse().expect("Tm operand"))
        .collect();
    assert_eq!(nums.len(), 6, "Tm must have 6 operands: {line}");
    (nums[4], nums[5])
}

fn assert_close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 0.011, // content stream rounds to 2 decimals
        "{what}: expected {expected:.3}, got {actual:.3}"
    );
}

// ---- place_text (built-in Helvetica) ----------------------------------------

#[test]
fn place_text_default_keeps_baseline_anchor() {
    let mut ed = EditableDoc::load(base()).unwrap();
    assert!(ed.place_text(0, 40.0, 400.0, "Hello", 20.0, BLACK, 0.0));
    let (sx, sy) = last_tm(&ed.to_bytes().unwrap());
    assert_close(sx, 40.0, "baseline sx");
    assert_close(sy, 400.0, "baseline sy (retrocompat)");
}

#[test]
fn place_text_top_anchor_drops_baseline_by_helvetica_ascent() {
    let mut ed = EditableDoc::load(base()).unwrap();
    assert!(ed.place_text_anchored(
        0,
        40.0,
        400.0,
        "Hello",
        20.0,
        BLACK,
        0.0,
        Align::Left,
        VerticalAnchor::Top,
    ));
    let (_, sy) = last_tm(&ed.to_bytes().unwrap());
    assert_close(sy, 400.0 - HELV_ASCENT * 20.0, "top-anchored baseline");
}

#[test]
fn place_text_bottom_anchor_raises_baseline_by_helvetica_descent() {
    let mut ed = EditableDoc::load(base()).unwrap();
    assert!(ed.place_text_anchored(
        0,
        40.0,
        400.0,
        "Hello",
        20.0,
        BLACK,
        0.0,
        Align::Left,
        VerticalAnchor::Bottom,
    ));
    let (_, sy) = last_tm(&ed.to_bytes().unwrap());
    assert_close(sy, 400.0 - HELV_DESCENT * 20.0, "bottom-anchored baseline");
}

#[test]
fn place_text_top_anchor_shift_follows_rotation() {
    // With 90° CCW rotation the anchor shift must be perpendicular to the
    // (now vertical) baseline: it moves the start point in +x, not -y.
    let mut ed = EditableDoc::load(base()).unwrap();
    assert!(ed.place_text_anchored(
        0,
        40.0,
        100.0,
        "Hello",
        20.0,
        BLACK,
        90.0,
        Align::Left,
        VerticalAnchor::Top,
    ));
    let (sx, sy) = last_tm(&ed.to_bytes().unwrap());
    assert_close(sx, 40.0 + HELV_ASCENT * 20.0, "rotated top-anchor sx");
    assert_close(sy, 100.0, "rotated top-anchor sy");
}

#[test]
fn place_text_with_font_top_anchor_uses_embedded_font_metrics() {
    let font = fonts::Font::from_file(FONT).unwrap();
    let asc = font.ascender() as f64 / font.units_per_em() as f64;

    let mut ed = EditableDoc::load(base()).unwrap();
    let id = ed.add_font_file(FONT).unwrap();
    assert!(ed.place_text_with_font_anchored(
        0,
        40.0,
        400.0,
        "Hello",
        20.0,
        BLACK,
        0.0,
        Align::Left,
        id,
        VerticalAnchor::Top,
    ));
    let (_, sy) = last_tm(&ed.to_bytes().unwrap());
    assert_close(sy, 400.0 - asc * 20.0, "embedded-font top anchor");
    // The embedded font's ascent differs from Helvetica's — the metrics must
    // come from the *selected* font, not the built-in table.
    assert!(
        (asc - HELV_ASCENT).abs() > 0.01,
        "fixture font should discriminate metric sources"
    );
}

// ---- masked_text -------------------------------------------------------------

#[test]
fn masked_text_default_still_centers_cap_height() {
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
        WHITE,
        Align::Left
    ));
    let (_, sy) = last_tm(&ed.to_bytes().unwrap());
    let centered = 300.0 + (42.0 - 12.0 * HELV_ASCENT) / 2.0; // cap == ascent for Helvetica
    assert_close(sy, centered, "masked_text default centering (retrocompat)");
}

#[test]
fn masked_text_valign_top_hangs_line_from_box_top() {
    let mut ed = EditableDoc::load(base()).unwrap();
    assert!(ed.masked_text_valign(
        0,
        40.0,
        300.0,
        200.0,
        42.0,
        "Hello",
        12.0,
        BLACK,
        WHITE,
        Align::Left,
        VerticalAlign::Top,
    ));
    let (_, sy) = last_tm(&ed.to_bytes().unwrap());
    let expected = 300.0 + 42.0 - HELV_ASCENT * 12.0;
    assert_close(sy, expected, "masked_text Top baseline");
    // Discriminates against the historical centering (FINDING-002 dY): in a
    // 42pt box at 12pt the two anchors differ by ~17pt.
    let centered = 300.0 + (42.0 - 12.0 * HELV_ASCENT) / 2.0;
    assert!((expected - centered).abs() > 1.0);
}

#[test]
fn masked_text_valign_bottom_rests_descender_on_box_bottom() {
    let mut ed = EditableDoc::load(base()).unwrap();
    assert!(ed.masked_text_valign(
        0,
        40.0,
        300.0,
        200.0,
        42.0,
        "Hello",
        12.0,
        BLACK,
        WHITE,
        Align::Left,
        VerticalAlign::Bottom,
    ));
    let (_, sy) = last_tm(&ed.to_bytes().unwrap());
    assert_close(
        sy,
        300.0 - HELV_DESCENT * 12.0,
        "masked_text Bottom baseline",
    );
}

#[test]
fn masked_text_with_font_valign_top_uses_embedded_font_metrics() {
    let font = fonts::Font::from_file(FONT).unwrap();
    let asc = font.ascender() as f64 / font.units_per_em() as f64;

    let mut ed = EditableDoc::load(base()).unwrap();
    let id = ed.add_font_file(FONT).unwrap();
    assert!(ed.masked_text_with_font_valign(
        0,
        40.0,
        300.0,
        200.0,
        42.0,
        "Olá",
        12.0,
        BLACK,
        WHITE,
        Align::Left,
        id,
        VerticalAlign::Top,
    ));
    let (_, sy) = last_tm(&ed.to_bytes().unwrap());
    assert_close(sy, 300.0 + 42.0 - asc * 12.0, "embedded-font masked Top");
}

// ---- iText line-box anchors (FINDING-004 follow-up: residual dY) -------------

/// Helvetica iText line box (typo × 1.2 branch + 0.21 half-leading):
/// ascent = 1.2·0.718 + 0.21·0.925, descent = 1.2·0.207 + 0.21·0.925.
const HELV_LINE_ASC: f64 = 1.2 * 0.718 + 0.21 * 0.925;
const HELV_LINE_DESC: f64 = 1.2 * 0.207 + 0.21 * 0.925;

#[test]
fn place_text_line_bottom_uses_itext_line_box() {
    let mut ed = EditableDoc::load(base()).unwrap();
    assert!(ed.place_text_anchored(
        0,
        40.0,
        100.0,
        "Hello",
        12.0,
        BLACK,
        0.0,
        Align::Left,
        VerticalAnchor::LineBottom,
    ));
    let (_, sy) = last_tm(&ed.to_bytes().unwrap());
    assert_close(sy, 100.0 + HELV_LINE_DESC * 12.0, "LineBottom baseline");
    // Discriminates from the plain descender Bottom anchor (~2.8pt at 12pt —
    // the FINDING residual).
    const _: () = assert!((HELV_LINE_DESC - (-HELV_DESCENT)) * 12.0 > 1.0);
}

#[test]
fn place_text_line_top_uses_itext_line_box() {
    let mut ed = EditableDoc::load(base()).unwrap();
    assert!(ed.place_text_anchored(
        0,
        40.0,
        400.0,
        "Hello",
        12.0,
        BLACK,
        0.0,
        Align::Left,
        VerticalAnchor::LineTop,
    ));
    let (_, sy) = last_tm(&ed.to_bytes().unwrap());
    assert_close(sy, 400.0 - HELV_LINE_ASC * 12.0, "LineTop baseline");
}

#[test]
fn line_anchors_use_win_metrics_of_the_embedded_font() {
    // The fixture font's win metrics drive the line box when they differ from
    // the typo values (iText's selection rule); otherwise typo × 1.2. Either
    // way the offset must come from the *embedded* font, not Helvetica.
    let font = fonts::Font::from_file(FONT).unwrap();
    let upem = font.units_per_em() as f64;
    let (wa, wd) = (font.win_ascent(), font.win_descent());
    let (ta, td) = (font.typo_ascender(), font.typo_descender());
    let expected_desc = match (wa, wd) {
        (Some(a), Some(d)) if a > 0 && d < 0 && !(ta == Some(a) && td == Some(d)) => {
            let (a, d) = (a as f64 / upem, -d as f64 / upem);
            d + 0.21 * (a + d)
        }
        _ => {
            let a = ta.unwrap_or_else(|| font.ascender()) as f64 / upem;
            let d = -(td.unwrap_or_else(|| font.descender()) as f64) / upem;
            d * 1.2 + 0.21 * (a + d)
        }
    };

    let mut ed = EditableDoc::load(base()).unwrap();
    let id = ed.add_font_file(FONT).unwrap();
    assert!(ed.place_text_with_font_anchored(
        0,
        40.0,
        100.0,
        "Hello",
        12.0,
        BLACK,
        0.0,
        Align::Left,
        id,
        VerticalAnchor::LineBottom,
    ));
    let (_, sy) = last_tm(&ed.to_bytes().unwrap());
    assert_close(sy, 100.0 + expected_desc * 12.0, "embedded LineBottom");
    assert!(
        (expected_desc - HELV_LINE_DESC).abs() * 12.0 > 0.2,
        "fixture font must discriminate the metric source"
    );
}

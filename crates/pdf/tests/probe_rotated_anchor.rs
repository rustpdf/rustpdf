//! Empirical probe of the exact bench configuration for `text-rotated-*`:
//! page with `/Rotate`, `StampSpace::Media`, embedded font, `LineBottom`
//! anchor, `rotation_deg` = the page rotation. Asserts the anchor offset
//! ROTATES with the text (`baseline = (x, y) + R(θ)·(0, dy)`), i.e. the
//! pivot is the effective anchor on this path too.

use pdf::{Align, Document, EditableDoc, StampSpace, VerticalAnchor};

const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
);

fn last_tm(pdf: &[u8]) -> (f64, f64) {
    let pos = pdf.windows(3).rposition(|w| w == b" Tm").unwrap();
    let line_start = pdf[..pos].iter().rposition(|&b| b == b'\n').unwrap() + 1;
    let line = std::str::from_utf8(&pdf[line_start..pos]).unwrap();
    let n: Vec<f64> = line
        .split_whitespace()
        .map(|t| t.parse().unwrap())
        .collect();
    (n[4], n[5])
}

/// The line-box descent our code should use for the fixture font, computed
/// independently from the raw metrics (iText selection rule).
fn line_desc_em() -> f64 {
    let f = fonts::Font::from_file(FONT).unwrap();
    let upem = f.units_per_em() as f64;
    match (f.win_ascent(), f.win_descent()) {
        (Some(wa), Some(wd))
            if wa > 0
                && wd < 0
                && !(f.typo_ascender() == Some(wa) && f.typo_descender() == Some(wd)) =>
        {
            let (a, d) = (wa as f64 / upem, -wd as f64 / upem);
            d + 0.21 * (a + d)
        }
        _ => {
            let a = f.typo_ascender().unwrap_or_else(|| f.ascender()) as f64 / upem;
            let d = -(f.typo_descender().unwrap_or_else(|| f.descender()) as f64) / upem;
            d * 1.2 + 0.21 * (a + d)
        }
    }
}

#[test]
fn bench_configuration_rotates_the_anchor_offset() {
    let (x, y, size) = (120.0, 300.0, 12.0);
    let dy = line_desc_em() * size;
    for page_rot in [90, 180, 270] {
        let mut doc = Document::new();
        doc.add_page_sized(400.0, 500.0);
        let mut ed = EditableDoc::load(doc.to_bytes().unwrap()).unwrap();
        ed.rotate_page(0, page_rot);
        ed.set_stamp_space(StampSpace::Media);
        let id = ed.add_font_file(FONT).unwrap();
        assert!(ed.place_text_with_font_anchored(
            0,
            x,
            y,
            "Rotacionado",
            size,
            (0.0, 0.0, 0.0),
            page_rot as f64,
            Align::Left,
            id,
            VerticalAnchor::LineBottom,
        ));
        let (sx, sy) = last_tm(&ed.to_bytes().unwrap());
        let (s, c) = (page_rot as f64).to_radians().sin_cos();
        let (ex, ey) = (x - dy * s, y + dy * c);
        assert!(
            (sx - ex).abs() < 0.02 && (sy - ey).abs() < 0.02,
            "page_rot {page_rot}: got ({sx:.2}, {sy:.2}); rotated-offset expects \
             ({ex:.2}, {ey:.2}); UNROTATED offset would be ({x:.2}, {:.2})",
            y + dy,
        );
    }
}

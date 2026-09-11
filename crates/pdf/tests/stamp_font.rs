//! FINDING-001: `EditableDoc` text stamping must be able to use an arbitrary
//! embedded TrueType font (e.g. Times), not only the built-in Helvetica.
//!
//! The mechanism is verified with the bundled Roboto (deterministic in CI):
//! stamping with the embedded font must render the *same glyphs* as
//! `Document::show_text` with the same TTF, and must differ clearly from the
//! built-in-Helvetica stamp of the same string.

use pdf::{Align, Document, EditableDoc};

const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
);

const TEXT: &str = "Assinado por Fulano";
const SIZE: f64 = 28.0;
const X: f64 = 40.0;
const Y: f64 = 400.0;

fn blank_base() -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_page_sized(400.0, 500.0);
    doc.to_bytes().unwrap()
}

/// Mean absolute per-channel RGB difference over the *inked* region — pixels
/// where either image has non-white content. Averaging over the whole page
/// would drown the glyphs in identical white background; restricting to ink
/// makes glyph-shape differences (font family) dominate.
fn mean_diff(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let inked = |p: &[u8]| p[0] < 240 || p[1] < 240 || p[2] < 240;
    let mut sum = 0u64;
    let mut n = 0u64;
    for (pa, pb) in a.chunks(4).zip(b.chunks(4)) {
        if inked(pa) || inked(pb) {
            for c in 0..3 {
                sum += (pa[c] as i32 - pb[c] as i32).unsigned_abs() as u64;
                n += 1;
            }
        }
    }
    if n == 0 {
        return 0.0;
    }
    sum as f64 / n as f64 / 255.0
}

fn render(bytes: &[u8]) -> Vec<u8> {
    pdf::render_page_rgba(bytes, 0, 150.0).unwrap().rgba
}

/// Total inked coverage (fraction of dark pixels), a proxy for "glyphs drew".
fn ink_coverage(px: &[u8]) -> f64 {
    let mut inked = 0u64;
    let total = (px.len() / 4) as u64;
    for p in px.chunks(4) {
        if p[0] < 240 || p[1] < 240 || p[2] < 240 {
            inked += 1;
        }
    }
    inked as f64 / total as f64
}

#[test]
fn stamped_embedded_font_honors_font_selection() {
    // Stamp the same string at the same place with (a) the embedded real font
    // and (b) the built-in Helvetica. Both use the plain-Tj stamp path, so the
    // glyph *positions* are identical and any pixel difference is pure glyph
    // shape — a clean check that the font_id is actually honored (not ignored
    // and silently drawn in Helvetica, which was the bug).
    let mut ed = EditableDoc::load(blank_base()).unwrap();
    let sfid = ed.add_font_file(FONT).unwrap();
    assert!(ed.place_text_with_font(0, X, Y, TEXT, SIZE, (0.0, 0.0, 0.0), 0.0, Align::Left, sfid));
    let embedded_px = render(&ed.to_bytes().unwrap());

    let mut ed2 = EditableDoc::load(blank_base()).unwrap();
    assert!(ed2.place_text(0, X, Y, TEXT, SIZE, (0.0, 0.0, 0.0), 0.0));
    let helv_px = render(&ed2.to_bytes().unwrap());

    // The embedded stamp must actually draw glyphs (not blank / not tofu boxes).
    let coverage = ink_coverage(&embedded_px);
    assert!(
        coverage > 0.005,
        "embedded-font stamp drew almost nothing (coverage {coverage:.4})"
    );

    // And it must differ from the Helvetica rendering — proof the real font's
    // glyph outlines were used, not the built-in fallback.
    let shape_diff = mean_diff(&embedded_px, &helv_px);
    eprintln!("ink coverage={coverage:.4}  shape diff vs helvetica={shape_diff:.4}");
    // A silent Helvetica fallback would make these renders byte-identical
    // (diff exactly 0); Roboto vs Helvetica differ by ~0.01 in the ink region.
    assert!(
        shape_diff > 0.004,
        "embedded-font stamp is indistinguishable from Helvetica (diff {shape_diff:.4}) — \
         font selection may be ignored"
    );

    // Parity of glyph *shapes* with the generation path: compare total ink area
    // against Document::show_text with the same TTF. Same font ⇒ near-equal
    // coverage (positions differ by kerning, but the glyph set is identical).
    let reference = {
        let mut d = Document::new();
        let fid = d.add_font_file(FONT).unwrap();
        d.add_page_sized(400.0, 500.0)
            .text(fid, SIZE)
            .at(X, Y)
            .show(TEXT);
        d.to_bytes().unwrap()
    };
    let ref_cov = ink_coverage(&render(&reference));
    let ratio = coverage / ref_cov;
    assert!(
        (0.85..=1.15).contains(&ratio),
        "embedded stamp ink area ({coverage:.4}) should match Document::show_text \
         ({ref_cov:.4}) with the same font; ratio {ratio:.3}"
    );
}

#[test]
fn stamp_output_embeds_the_font_program() {
    let mut ed = EditableDoc::load(blank_base()).unwrap();
    let sfid = ed.add_font_file(FONT).unwrap();
    ed.place_text_with_font(0, X, Y, TEXT, SIZE, (0.0, 0.0, 0.0), 0.0, Align::Left, sfid);
    let out = ed.to_bytes().unwrap();
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("/Type0"), "expected a Type0 composite font");
    assert!(
        text.contains("/CIDFontType2"),
        "expected a CIDFontType2 descendant"
    );
    assert!(text.contains("/FontFile2"), "expected the embedded program");
    assert!(
        text.contains("/CIDToGIDMap"),
        "expected a CIDToGIDMap remap"
    );
    // A registered-but-unused font must NOT be embedded.
    let mut ed2 = EditableDoc::load(blank_base()).unwrap();
    let _ = ed2.add_font_file(FONT).unwrap();
    let out2 = ed2.to_bytes().unwrap();
    assert!(
        !String::from_utf8_lossy(&out2).contains("/FontFile2"),
        "an unused stamp font must not be embedded"
    );
}

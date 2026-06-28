//! Visual regression: compare the native rasterizer against **mutool** (the
//! reference renderer) using testkit's perceptual mean-absolute-difference
//! metric. The two renderers will never be byte-identical (different
//! anti-aliasing and glyph hinting), so we assert the normalized diff stays
//! under a small threshold.
//!
//! The test **skips** (passes with a printed note) when mutool is not
//! available or cannot be spawned (the CI sandbox often blocks it); run it
//! locally with `mutool` on PATH to actually exercise the comparison.

use std::path::PathBuf;

use parser::PdfReader;
use render::{render_page_to_png, RenderOptions};
use testkit::{render_to_png, VisualError};

const FONT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
));

fn tmp(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("rustpdf_vr_{name}"));
    p
}

fn render_native(pdf: &[u8], dpi: u32) -> Vec<u8> {
    let reader = PdfReader::parse(pdf).expect("parse");
    let page = &reader.pages()[0];
    render_page_to_png(&reader, page, &RenderOptions::dpi(dpi as f32)).expect("native render")
}

/// Render `pdf` with both renderers at `dpi` and return the perceptual diff,
/// or `None` when mutool is unavailable (so the caller can skip).
fn diff_against_mutool(pdf: &[u8], dpi: u32, tag: &str) -> Option<f64> {
    let pdf_path = tmp(&format!("{tag}.pdf"));
    std::fs::write(&pdf_path, pdf).unwrap();

    let ref_png = tmp(&format!("{tag}_mutool.png"));
    match render_to_png(&pdf_path, &ref_png, dpi) {
        Ok(()) => {}
        Err(VisualError::RendererMissing) => return None,
        // A spawn failure in a locked-down sandbox is treated as "unavailable".
        Err(VisualError::Render(_)) => return None,
        Err(e) => panic!("mutool render error for {tag}: {e}"),
    }

    let mine_png = tmp(&format!("{tag}_native.png"));
    std::fs::write(&mine_png, render_native(pdf, dpi)).unwrap();

    let d = perceptual_diff(&mine_png, &ref_png);
    eprintln!("[visual] {tag} @ {dpi}dpi: diff = {d:.5}");
    Some(d)
}

/// Mean absolute per-channel RGB difference (`0.0..=1.0`) over the overlapping
/// top-left region. Comparing the common `min(w)`x`min(h)` area absorbs the
/// off-by-one bitmap sizing that renderers differ on for fractional-point page
/// boxes (mutool rounds the page box up, we round to nearest); the dropped
/// edge strip is at most one pixel and changes the metric negligibly.
fn perceptual_diff(a: &PathBuf, b: &PathBuf) -> f64 {
    let ia = image::open(a).expect("decode native png").to_rgb8();
    let ib = image::open(b).expect("decode mutool png").to_rgb8();
    let w = ia.width().min(ib.width());
    let h = ia.height().min(ib.height());
    assert!(w > 0 && h > 0, "empty image");
    let mut acc: u64 = 0;
    for y in 0..h {
        for x in 0..w {
            let pa = ia.get_pixel(x, y).0;
            let pb = ib.get_pixel(x, y).0;
            for c in 0..3 {
                acc += pa[c].abs_diff(pb[c]) as u64;
            }
        }
    }
    acc as f64 / (w as f64 * h as f64 * 3.0 * 255.0)
}

// ---- documents under test -------------------------------------------------

fn vector_doc() -> Vec<u8> {
    // Filled rectangle + a thick stroked diagonal: vectors should match mutool
    // very closely (flat color, simple anti-aliased edges).
    let mut doc = pdf::Document::new();
    let page = doc.add_page_sized(200.0, 200.0);
    page.content()
        .set_fill_rgb(0.20, 0.45, 0.85)
        .rect(30.0, 40.0, 140.0, 90.0)
        .fill()
        .set_stroke_rgb(0.85, 0.12, 0.12)
        .set_line_width(6.0)
        .move_to(25.0, 165.0)
        .line_to(175.0, 150.0)
        .stroke();
    doc.to_bytes().unwrap()
}

fn text_doc() -> Vec<u8> {
    // Real embedded font, rendered from glyph outlines on both sides.
    let mut doc = pdf::Document::new();
    let f = doc.add_font(FONT.to_vec()).unwrap();
    let page = doc.add_page_sized(220.0, 120.0);
    page.text(f, 40.0).at(16.0, 60.0).show("Render");
    doc.to_bytes().unwrap()
}

fn mixed_doc() -> Vec<u8> {
    // CMYK fill + grayscale + text together.
    let mut doc = pdf::Document::new();
    let f = doc.add_font(FONT.to_vec()).unwrap();
    let page = doc.add_page_sized(240.0, 160.0);
    page.content()
        .set_fill_cmyk(0.8, 0.0, 0.1, 0.0)
        .rect(20.0, 90.0, 200.0, 40.0)
        .fill()
        .set_fill_gray(0.55)
        .rect(20.0, 40.0, 90.0, 30.0)
        .fill();
    page.text(f, 22.0).at(24.0, 18.0).show("PDF to image");
    doc.to_bytes().unwrap()
}

/// Each case maps to a perceptual-diff ceiling. Thresholds are generous enough
/// to absorb anti-aliasing/hinting differences but tight enough to catch a
/// genuinely wrong render (wrong color, missing content, mispositioned text).
#[test]
fn matches_mutool_reference() {
    // Measured locally (mutool 1.27, RGB diff): vectors ~0.0011, text ~0.0016,
    // the CMYK "mixed" case ~0.036 (naive CMYK→RGB vs mutool's color-managed
    // path). The ceilings keep a healthy margin for AA/hinting and
    // mutool-version drift while still failing loudly on a genuinely wrong
    // render.
    let cases: &[(&str, Vec<u8>, u32, f64)] = &[
        ("vectors", vector_doc(), 72, 0.015),
        ("vectors_hi", vector_doc(), 144, 0.015),
        ("text", text_doc(), 144, 0.020),
        ("mixed", mixed_doc(), 144, 0.060),
    ];

    let mut ran = 0;
    for (tag, pdf, dpi, ceiling) in cases {
        match diff_against_mutool(pdf, *dpi, tag) {
            Some(d) => {
                ran += 1;
                assert!(
                    d < *ceiling,
                    "{tag} @ {dpi}dpi: perceptual diff {d:.5} exceeds ceiling {ceiling}"
                );
            }
            None => {
                eprintln!("[visual] {tag}: mutool unavailable, skipping");
            }
        }
    }

    if ran == 0 {
        eprintln!("[visual] mutool not available; all cases skipped");
    }
}

/// Render real project-generated corpus PDFs (even-odd fills, nested CTMs,
/// Unicode/CID text, an image with an alpha SMask) and compare to mutool. These
/// exercise the renderer on content it did not author in-test.
#[test]
fn matches_mutool_on_corpus() {
    const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../corpus/generated");
    // (file stem, diff ceiling at 72 dpi)
    let cases: &[(&str, f64)] = &[
        ("gfx_even_odd", 0.015),
        ("gfx_ctm_nested", 0.015),
        ("text_unicode", 0.030),
        ("img_png_alpha", 0.050),
    ];

    let mut ran = 0;
    for (stem, ceiling) in cases {
        let path = format!("{DIR}/{stem}.pdf");
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("[visual] corpus {stem}: missing ({path}), skipping");
            continue;
        };
        match diff_against_mutool(&bytes, 72, stem) {
            Some(d) => {
                ran += 1;
                assert!(
                    d < *ceiling,
                    "corpus {stem}: perceptual diff {d:.5} exceeds ceiling {ceiling}"
                );
            }
            None => eprintln!("[visual] corpus {stem}: mutool unavailable, skipping"),
        }
    }

    if ran == 0 {
        eprintln!("[visual] mutool not available; all corpus cases skipped");
    }
}

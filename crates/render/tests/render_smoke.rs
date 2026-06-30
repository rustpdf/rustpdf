//! End-to-end: build a PDF with the `pdf` crate, render it, inspect pixels.

use parser::PdfReader;
use render::{render_page, RenderOptions};

const FONT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
));

/// Count pixels that are (roughly) a given color in straight RGBA.
fn count_near(rgba: &[u8], target: [u8; 3], tol: i32) -> usize {
    rgba.chunks_exact(4)
        .filter(|p| {
            p[3] > 200
                && (p[0] as i32 - target[0] as i32).abs() <= tol
                && (p[1] as i32 - target[1] as i32).abs() <= tol
                && (p[2] as i32 - target[2] as i32).abs() <= tol
        })
        .count()
}

fn build_pdf() -> Vec<u8> {
    let mut doc = pdf::Document::new();
    let font = doc.add_font(FONT.to_vec()).unwrap();
    let page = doc.add_page_sized(200.0, 200.0);
    // A red filled rectangle in the lower-left quadrant.
    page.content()
        .set_fill_rgb(1.0, 0.0, 0.0)
        .rect(20.0, 20.0, 80.0, 60.0)
        .fill();
    // Black text near the top.
    page.text(font, 24.0).at(20.0, 150.0).show("Hello");
    doc.to_bytes().unwrap()
}

#[test]
fn renders_rectangle_and_text() {
    let bytes = build_pdf();
    let reader = PdfReader::parse(&bytes).expect("parse");
    let page = &reader.pages()[0];

    let pixmap = render_page(&reader, page, &RenderOptions::dpi(144.0)).expect("render");
    // 200pt @ 144dpi = 400px square.
    assert_eq!(pixmap.width(), 400);
    assert_eq!(pixmap.height(), 400);

    let (rgba, _, _) = render::to_rgba8(&pixmap);

    // The red rectangle: 80x60 pt → 160x120 px = 19_200 px, expect most of it.
    let red = count_near(&rgba, [255, 0, 0], 40);
    assert!(
        red > 12_000,
        "expected a big red rectangle, got {red} red px"
    );

    // Some near-black text pixels.
    let black = count_near(&rgba, [0, 0, 0], 40);
    assert!(black > 200, "expected black glyph pixels, got {black}");

    // Background should be white and dominate.
    let white = count_near(&rgba, [255, 255, 255], 5);
    assert!(white > 100_000, "expected white background, got {white}");
}

#[test]
fn red_rectangle_lands_in_the_lower_left() {
    let bytes = build_pdf();
    let reader = PdfReader::parse(&bytes).unwrap();
    let page = &reader.pages()[0];
    let pixmap = render_page(&reader, page, &RenderOptions::dpi(72.0)).unwrap();
    let (rgba, w, h) = render::to_rgba8(&pixmap);
    let at = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2]]
    };
    // Rectangle center (60, 50)pt in PDF → device y flips: (60, 200-50)=(60,150).
    let c = at(60, 150);
    assert!(
        c[0] > 200 && c[1] < 60 && c[2] < 60,
        "center not red: {c:?}"
    );
    // Top-left corner should be white background.
    let corner = at(2, 2);
    assert!(
        corner[0] > 240 && corner[1] > 240 && corner[2] > 240,
        "corner not white: {corner:?}"
    );
    let _ = h;
}

/// A page whose `/MediaBox` lives on the `/Pages` tree node (inherited), not on
/// the leaf `/Page` — produced by e.g. PyFPDF. The renderer must walk `/Parent`
/// to find it, otherwise it errors with `BadPageGeometry` ("no usable
/// MediaBox"). Regression for the C# `render_page_to_png` report.
#[test]
fn renders_page_with_inherited_mediabox() {
    let bytes = include_bytes!("fixtures/inherited_mediabox.pdf");
    let reader = PdfReader::parse(bytes.as_slice()).expect("parse");
    let page = &reader.pages()[0];
    let pixmap =
        render_page(&reader, page, &RenderOptions::dpi(72.0)).expect("inherited MediaBox renders");
    // A4 at 72 dpi ≈ 595×842.
    assert!(
        (590..=600).contains(&pixmap.width()) && (838..=846).contains(&pixmap.height()),
        "unexpected size {}x{}",
        pixmap.width(),
        pixmap.height()
    );
}

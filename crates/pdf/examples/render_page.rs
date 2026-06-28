//! Fase 7.8: rasterize a PDF page to a PNG image.
//!
//! Run with:
//!   `cargo run -p pdf --example render_page -- in.pdf out.png [page] [dpi]`
//!
//! With no input PDF it builds a small demo document first, renders its first
//! page at 150 DPI and writes the PNG.

use pdf::{Document, RenderOptions};

fn main() {
    // Page rendering is a licensed Pro feature. This demo activates the
    // committed dev license; in production set the `RUSTPDF_LICENSE` env var to
    // your token (auto-activated) instead.
    let _ = pdf::activate_license(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../license/fixtures/dev_license.txt"
    )));

    let mut args = std::env::args().skip(1);
    let input = args.next();
    let output = args.next().unwrap_or_else(|| "page.png".to_string());
    let page: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let dpi: f32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(150.0);

    let pdf_bytes = match input.as_deref() {
        Some("") | Some("-") | None => demo_pdf(),
        Some(path) => std::fs::read(path).expect("read input PDF"),
    };

    let png = pdf::render_page_to_png_with(&pdf_bytes, page, &RenderOptions::dpi(dpi))
        .expect("render page");
    std::fs::write(&output, &png).expect("write PNG");
    println!(
        "wrote {output} ({} bytes) — page {page} @ {dpi} DPI",
        png.len()
    );
}

/// A tiny self-contained document so the example runs with no arguments.
fn demo_pdf() -> Vec<u8> {
    const FONT: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/Roboto-Regular.ttf"
    ));
    let mut doc = Document::new();
    let font = doc.add_font(FONT.to_vec()).unwrap();
    let page = doc.add_page_sized(300.0, 200.0);
    page.content()
        .set_fill_rgb(0.18, 0.45, 0.86)
        .rect(20.0, 20.0, 260.0, 90.0)
        .fill();
    page.text(font, 28.0).at(28.0, 150.0).show("Rendered!");
    doc.to_bytes().unwrap()
}

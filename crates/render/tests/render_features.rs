//! Feature coverage: image XObjects, CMYK color, and clipping.

use parser::PdfReader;
use render::{render_page, RenderOptions};

fn rgba_at(rgba: &[u8], w: u32, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * w + x) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2]]
}

/// A 2×2 RGBA PNG: red, green / blue, white.
fn quad_png() -> Vec<u8> {
    let data: [u8; 16] = [
        255, 0, 0, 255, // top-left red
        0, 255, 0, 255, // top-right green
        0, 0, 255, 255, // bottom-left blue
        255, 255, 255, 255, // bottom-right white
    ];
    let mut buf = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut buf, 2, 2);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&data).unwrap();
    }
    buf
}

#[test]
fn renders_image_xobject() {
    let mut doc = pdf::Document::new();
    let img = doc.add_image_png(quad_png()).unwrap();
    let page = doc.add_page_sized(100.0, 100.0);
    // Fill the whole page with the 2x2 image.
    page.draw_image(img, 0.0, 0.0, 100.0, 100.0);
    let bytes = doc.to_bytes().unwrap();

    let reader = PdfReader::parse(&bytes).unwrap();
    let pixmap = render_page(&reader, &reader.pages()[0], &RenderOptions::dpi(72.0)).unwrap();
    let (rgba, w, _h) = render::to_rgba8(&pixmap);

    // Each pure color from the 2x2 grid should appear somewhere with strength.
    let has = |t: [u8; 3]| {
        rgba.chunks_exact(4).any(|p| {
            (p[0] as i32 - t[0] as i32).abs() < 50
                && (p[1] as i32 - t[1] as i32).abs() < 50
                && (p[2] as i32 - t[2] as i32).abs() < 50
        })
    };
    assert!(has([255, 0, 0]), "no red from image");
    assert!(has([0, 255, 0]), "no green from image");
    assert!(has([0, 0, 255]), "no blue from image");

    // Sanity: the page is not blank (some non-white pixel exists).
    let nonwhite = rgba
        .chunks_exact(4)
        .filter(|p| p[0] < 240 || p[1] < 240 || p[2] < 240)
        .count();
    assert!(
        nonwhite > 1000,
        "image did not paint, only {nonwhite} non-white"
    );
    let _ = (w, rgba_at(&rgba, w, 0, 0));
}

#[test]
fn cmyk_fill_converts_to_rgb() {
    let mut doc = pdf::Document::new();
    let page = doc.add_page_sized(80.0, 80.0);
    // Pure cyan in CMYK ⇒ RGB (0, 255, 255).
    page.content()
        .set_fill_cmyk(1.0, 0.0, 0.0, 0.0)
        .rect(10.0, 10.0, 60.0, 60.0)
        .fill();
    let bytes = doc.to_bytes().unwrap();
    let reader = PdfReader::parse(&bytes).unwrap();
    let pixmap = render_page(&reader, &reader.pages()[0], &RenderOptions::dpi(72.0)).unwrap();
    let (rgba, w, _) = render::to_rgba8(&pixmap);
    let c = rgba_at(&rgba, w, 40, 40);
    assert!(
        c[0] < 60 && c[1] > 200 && c[2] > 200,
        "cyan center should be ~(0,255,255), got {c:?}"
    );
}

// ---- Type 3 fonts ---------------------------------------------------------

fn t3_obj(pdf: &mut Vec<u8>, offs: &mut [usize], n: usize, body: &str) {
    offs[n] = pdf.len();
    pdf.extend_from_slice(format!("{n} 0 obj\n{body}\nendobj\n").as_bytes());
}

fn t3_stream(pdf: &mut Vec<u8>, offs: &mut [usize], n: usize, data: &[u8]) {
    offs[n] = pdf.len();
    pdf.extend_from_slice(format!("{n} 0 obj\n<< /Length {} >>\nstream\n", data.len()).as_bytes());
    pdf.extend_from_slice(data);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
}

/// A Type 3 font's glyph is a content stream, not an outline. This hand-built
/// PDF defines one glyph (code 65 'A' → name `sq`) that fills a 100×100 square
/// in glyph space; FontMatrix scales by 0.01 and the text is shown at size 10,
/// so the result is a 10pt black square at pdf (50,700). Regression guard for
/// the Type 3 interpreter path (CharProcs executed via the content runner).
#[test]
fn renders_type3_glyph() {
    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.7\n");
    let mut offs = [0usize; 9];
    t3_obj(&mut pdf, &mut offs, 1, "<< /Type /Catalog /Pages 2 0 R >>");
    t3_obj(&mut pdf, &mut offs, 2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    t3_obj(&mut pdf, &mut offs, 3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 800] /Resources << /Font << /F0 4 0 R >> >> /Contents 5 0 R >>");
    t3_obj(&mut pdf, &mut offs, 4, "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 100 100] /FontMatrix [0.01 0 0 0.01 0 0] /CharProcs 6 0 R /Encoding 7 0 R /FirstChar 65 /LastChar 65 /Widths [100] >>");
    t3_stream(&mut pdf, &mut offs, 5, b"BT /F0 10 Tf 50 700 Td (A) Tj ET\n");
    t3_obj(&mut pdf, &mut offs, 6, "<< /sq 8 0 R >>");
    t3_obj(&mut pdf, &mut offs, 7, "<< /Type /Encoding /Differences [65 /sq] >>");
    t3_stream(&mut pdf, &mut offs, 8, b"100 0 0 0 100 100 d1\n0 0 100 100 re f\n");

    let xref_off = pdf.len();
    pdf.extend_from_slice(b"xref\n0 9\n0000000000 65535 f \n");
    for &off in &offs[1..=8] {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size 9 /Root 1 0 R >>\nstartxref\n{xref_off}\n%%EOF").as_bytes(),
    );

    let reader = PdfReader::parse(&pdf).unwrap();
    let pixmap = render_page(&reader, &reader.pages()[0], &RenderOptions::dpi(72.0)).unwrap();
    let (rgba, w, h) = render::to_rgba8(&pixmap);

    // 10pt square at pdf (50,700)-(60,710); device y-flips on an 800pt page →
    // device (50..60, 90..100). The center must be black (glyph was drawn).
    let center = rgba_at(&rgba, w, 55, 95);
    assert!(
        center[0] < 80 && center[1] < 80 && center[2] < 80,
        "Type 3 glyph not rendered; center pixel = {center:?}"
    );
    // Elsewhere stays white (no stray ink): a far corner is light.
    let corner = rgba_at(&rgba, w, w - 2, h - 2);
    assert!(
        corner[0] > 200 && corner[1] > 200 && corner[2] > 200,
        "unexpected ink away from the glyph; corner = {corner:?}"
    );
}

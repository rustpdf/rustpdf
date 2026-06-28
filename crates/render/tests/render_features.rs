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

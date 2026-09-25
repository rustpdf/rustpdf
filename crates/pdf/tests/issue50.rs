//! Issue #50: `EditableDoc::draw_image` — stamp an image onto an existing page
//! at a given position/size/rotation. Outputs are re-parsed with our own
//! parser/extractor (the sandbox can't spawn qpdf); structural assertions
//! confirm the image landed on the right page.

use image::{ImageEncoder, Rgb, RgbImage};
use pdf::{extract_images, Document, EditableDoc, Image, ImageFormat};

const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
);

fn png_bytes(w: u32, h: u32) -> Vec<u8> {
    let img = RgbImage::from_fn(w, h, |x, y| Rgb([x as u8 * 6, y as u8 * 8, 90]));
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&img, w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    png
}

fn two_page_doc() -> Vec<u8> {
    let mut doc = Document::new();
    let f = doc.add_font_file(FONT).unwrap();
    doc.add_page_sized(595.276, 841.89)
        .text(f, 18.0)
        .at(72.0, 700.0)
        .show("One");
    doc.add_page_sized(612.0, 792.0)
        .text(f, 18.0)
        .at(72.0, 700.0)
        .show("Two");
    doc.to_bytes().unwrap()
}

#[test]
fn draw_image_lands_on_the_target_page() {
    let png = Image::from_png(png_bytes(40, 30)).unwrap();
    let mut ed = EditableDoc::load(two_page_doc()).unwrap();
    assert!(ed.draw_image(1, &png, 72.0, 600.0, 160.0, 120.0, 0.0));
    // Out-of-range page index is a no-op.
    assert!(!ed.draw_image(9, &png, 0.0, 0.0, 1.0, 1.0, 0.0));
    let bytes = ed.to_bytes().unwrap();

    let body = String::from_utf8_lossy(&bytes);
    assert!(body.contains("160.00 0 0 120.00 0 0 cm"), "image scale cm");
    assert!(body.contains(" Do\n"), "image painted");

    let images = extract_images(&bytes).unwrap();
    assert_eq!(images.len(), 1, "exactly one image embedded");
    let img = &images[0];
    assert_eq!(img.page, 1, "stamped on page index 1");
    assert_eq!((img.width, img.height), (40, 30));
    assert_eq!(img.format, ImageFormat::Png);
}

#[test]
fn draw_image_rotation_emits_rotated_matrix() {
    let png = Image::from_png(png_bytes(8, 8)).unwrap();
    let mut ed = EditableDoc::load(two_page_doc()).unwrap();
    ed.draw_image(0, &png, 50.0, 50.0, 100.0, 100.0, 90.0);
    let bytes = ed.to_bytes().unwrap();
    let body = String::from_utf8_lossy(&bytes);
    // 90°: cos=0, sin=1 → rotation cm "0.00000 1.00000 -1.00000 0.00000 50.00 50.00 cm".
    assert!(
        body.contains("0.00000 1.00000 -1.00000 0.00000 50.00 50.00 cm"),
        "rotated image matrix present"
    );
}

#[test]
fn draw_image_twice_uses_distinct_resource_names() {
    let png = Image::from_png(png_bytes(8, 8)).unwrap();
    let mut ed = EditableDoc::load(two_page_doc()).unwrap();
    assert!(ed.draw_image(0, &png, 10.0, 10.0, 50.0, 50.0, 0.0));
    assert!(ed.draw_image(0, &png, 80.0, 80.0, 50.0, 50.0, 0.0));
    let bytes = ed.to_bytes().unwrap();

    // Two independent Image XObjects, both extractable from the same page.
    let images = extract_images(&bytes).unwrap();
    assert_eq!(images.len(), 2);
    assert!(images.iter().all(|i| i.page == 0));
}

// ---- Integrator gaps (#3 extract-page-text, #4 align, #5 mask) ----

use pdf::{extract_page_text, extract_text, Align};

#[test]
fn extract_page_text_isolates_one_page() {
    let bytes = two_page_doc();
    assert_eq!(
        extract_page_text(&bytes, 0).unwrap().as_deref(),
        Some("One")
    );
    assert_eq!(
        extract_page_text(&bytes, 1).unwrap().as_deref(),
        Some("Two")
    );
    // Out of range → None (not an error).
    assert_eq!(extract_page_text(&bytes, 9).unwrap(), None);
    // The whole-doc extraction still contains both.
    let all = extract_text(&bytes).unwrap();
    assert!(all.contains("One") && all.contains("Two"));
}

#[test]
fn place_text_aligned_shifts_anchor_by_width() {
    let mut ed = EditableDoc::load(two_page_doc()).unwrap();
    // Same anchor x=300, three alignments → three different Tm start x.
    assert!(ed.place_text_aligned(
        0,
        300.0,
        700.0,
        "ALIGN",
        12.0,
        (0.0, 0.0, 0.0),
        0.0,
        Align::Left
    ));
    assert!(ed.place_text_aligned(
        0,
        300.0,
        680.0,
        "ALIGN",
        12.0,
        (0.0, 0.0, 0.0),
        0.0,
        Align::Center
    ));
    assert!(ed.place_text_aligned(
        0,
        300.0,
        660.0,
        "ALIGN",
        12.0,
        (0.0, 0.0, 0.0),
        0.0,
        Align::Right
    ));
    let bytes = ed.to_bytes().unwrap();
    let body = String::from_utf8_lossy(&bytes);
    // Left starts exactly at the anchor.
    assert!(body.contains("300.00 700.00 Tm"), "left at anchor");
    // Right ends at the anchor: start x = 300 - width("ALIGN"@12). A<V widths
    // sum to >0, so the right-aligned start must be strictly left of 300.
    assert!(
        !body.contains("300.00 660.00 Tm"),
        "right-aligned must shift the start x left of the anchor"
    );
    // Text remains extractable.
    assert!(extract_text(&bytes).unwrap().matches("ALIGN").count() >= 3);
}

#[test]
fn masked_text_paints_box_and_centers_text() {
    let mut ed = EditableDoc::load(two_page_doc()).unwrap();
    assert!(ed.masked_text(
        0,
        100.0,
        500.0,
        200.0,
        24.0,
        "R$ 1.234,56",
        12.0,
        (0.0, 0.0, 0.0),
        (1.0, 1.0, 1.0),
        Align::Center,
    ));
    assert!(!ed.masked_text(
        9,
        0.0,
        0.0,
        10.0,
        10.0,
        "x",
        12.0,
        (0.0, 0.0, 0.0),
        (1.0, 1.0, 1.0),
        Align::Left,
    ));
    let bytes = ed.to_bytes().unwrap();
    let body = String::from_utf8_lossy(&bytes);
    assert!(
        body.contains("1.000 1.000 1.000 rg"),
        "white background box"
    );
    assert!(body.contains("200.00 24.00 re"), "box with given size");
    assert!(
        extract_text(&bytes).unwrap().contains("R$ 1.234,56"),
        "masked text extractable"
    );
}

#[test]
fn placed_text_roundtrips_accents_as_winansi() {
    // A typical e-signature footer: accented Latin-1 + an em dash. Must come back exactly
    // through extraction (the bytes are written as WinAnsi, not raw UTF-8).
    let footer = "Operação 123456 — assinado eletronicamente pela Example Corp";
    let mut ed = EditableDoc::load(two_page_doc()).unwrap();
    assert!(ed.place_text(0, 40.0, 60.0, footer, 9.0, (0.0, 0.0, 0.0), 0.0));
    assert!(ed.masked_text(
        0,
        40.0,
        100.0,
        400.0,
        16.0,
        "Assinado: ção, ã, õ, é",
        9.0,
        (0.0, 0.0, 0.0),
        (1.0, 1.0, 1.0),
        Align::Left,
    ));
    let bytes = ed.to_bytes().unwrap();
    // No mojibake: the literal UTF-8 of "Operação" (Ã§) must NOT appear raw.
    let body = String::from_utf8_lossy(&bytes);
    assert!(
        !body.contains("OperaÃ"),
        "accents must be WinAnsi, not raw UTF-8"
    );
    // Extraction recovers the exact text (WinAnsi → Unicode).
    let text = extract_text(&bytes).unwrap();
    assert!(
        text.contains(footer),
        "footer must round-trip exactly: {text:?}"
    );
    assert!(
        text.contains("ção, ã, õ, é"),
        "masked accents round-trip: {text:?}"
    );
}

#[test]
fn rendered_text_advance_is_proportional_not_flat() {
    // Render a placed line of standard Helvetica (no /Widths) and return the
    // inked width in pixels.
    fn ink_width(s: &str) -> u32 {
        let mut doc = Document::new();
        doc.add_page_sized(600.0, 60.0);
        let plain = doc.to_bytes().unwrap();
        let mut ed = EditableDoc::load(&plain).unwrap();
        ed.place_text(0, 10.0, 25.0, s, 20.0, (0.0, 0.0, 0.0), 0.0);
        let png = pdf::render_page_to_png(ed.to_bytes().unwrap(), 0, 150.0).unwrap();
        let img = image::load_from_memory(&png).unwrap().to_luma8();
        let (mut min_x, mut max_x) = (u32::MAX, 0u32);
        for (x, _y, p) in img.enumerate_pixels() {
            if p.0[0] < 128 {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
            }
        }
        max_x.saturating_sub(min_x)
    }
    let narrow = ink_width("iiiiiiiiii"); // 10 × Helvetica 'i' (222)
    let wide = ink_width("mmmmmmmmmm"); // 10 × Helvetica 'm' (833)
                                        // With the old flat-0.5em advance both lines spanned the same block width;
                                        // proportional metrics make 'm' (~3.75× 'i') visibly wider.
    assert!(
        wide as f64 > narrow as f64 * 1.8,
        "advance not proportional: wide={wide}px narrow={narrow}px"
    );
}

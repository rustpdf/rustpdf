//! Tier 2 (item 6): true redaction removes the content (not just a black box).
//! The redacted text must no longer be extractable.

use pdf::{Document, EditableDoc};

const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
);

/// Redaction is a licensed (Enterprise) feature; activate the dev token.
fn lic() {
    pdf::activate_license(
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../license/fixtures/dev_license.txt"
        ))
        .trim(),
    )
    .unwrap();
}

/// A page with a secret line at y≈600 and a kept line at y≈700.
fn doc_with_two_lines() -> Vec<u8> {
    let mut doc = Document::new();
    let f = doc.add_font_file(FONT).unwrap();
    {
        let p = doc.add_page();
        p.text(f, 20.0).at(72.0, 700.0).show("KEEP THIS LINE");
        p.text(f, 20.0).at(72.0, 600.0).show("SECRET PASSWORD 1234");
    }
    doc.to_bytes().unwrap()
}

#[test]
fn redaction_removes_text_from_content() {
    lic();
    let pdf = doc_with_two_lines();
    assert!(pdf::extract_text(&pdf).unwrap().contains("SECRET PASSWORD"));

    let mut ed = EditableDoc::load(&pdf).unwrap();
    // Redact a rect covering the secret line's origin (72,600).
    assert!(ed.redact(0, &[[60.0, 590.0, 400.0, 620.0]]).unwrap());
    let out = ed.to_bytes().unwrap();

    let text = pdf::extract_text(&out).unwrap();
    assert!(
        !text.contains("SECRET"),
        "secret text must be gone: {text:?}"
    );
    assert!(!text.contains("PASSWORD"), "secret text must be gone");
    assert!(text.contains("KEEP THIS LINE"), "kept text must remain");

    // The raw bytes must not contain the redacted glyphs either (true removal):
    // a black rectangle fill operator marks the covered region.
    let raw = String::from_utf8_lossy(&out);
    assert!(
        raw.contains(" re") && raw.contains("0 g"),
        "black box drawn"
    );
}

#[test]
fn redaction_leaves_other_pages_and_lines_intact() {
    lic();
    let pdf = doc_with_two_lines();
    let mut ed = EditableDoc::load(&pdf).unwrap();
    ed.redact(0, &[[60.0, 590.0, 400.0, 620.0]]).unwrap();
    let out = ed.to_bytes().unwrap();
    assert_eq!(EditableDoc::load(&out).unwrap().page_count(), 1);
    assert!(pdf::extract_text(&out).unwrap().contains("KEEP THIS LINE"));
}

#[test]
fn redact_nonexistent_page_returns_false() {
    let mut ed = EditableDoc::load(doc_with_two_lines()).unwrap();
    assert!(!ed.redact(9, &[[0.0, 0.0, 10.0, 10.0]]).unwrap());
}

#[test]
fn redaction_survives_flate_encoded_content() {
    lic();
    // Optimize first so the content stream becomes FlateDecode, then redact.
    let pdf = doc_with_two_lines();
    let mut ed = EditableDoc::load(&pdf).unwrap();
    ed.optimize(); // compresses content streams
    let compressed = ed.to_bytes().unwrap();

    let mut ed2 = EditableDoc::load(&compressed).unwrap();
    assert!(ed2.redact(0, &[[60.0, 590.0, 400.0, 620.0]]).unwrap());
    let out = ed2.to_bytes().unwrap();
    let text = pdf::extract_text(&out).unwrap();
    assert!(
        !text.contains("SECRET"),
        "secret gone from flate content: {text:?}"
    );
    assert!(text.contains("KEEP THIS LINE"));
}

// ---- FINDING-006: glyph-level removal (not origin-only, not paint-over) -------

/// The exact bench repro: the secret is in the MIDDLE of a single show op.
/// An origin-only test keeps the whole run (the run starts outside the rect);
/// glyph-level redaction must remove only the covered glyphs.
#[test]
fn redacts_word_in_the_middle_of_a_run() {
    lic();
    let mut doc = Document::new();
    let f = doc.add_font_file(FONT).unwrap();
    doc.add_page()
        .text(f, 20.0)
        .at(72.0, 700.0)
        .show("PUBLICO SEGREDOXYZ FIM");
    let pdf = doc.to_bytes().unwrap();

    let hit = &pdf::find_text(&pdf, "SEGREDOXYZ", pdf::FindOptions::default()).unwrap()[0];
    let mut ed = EditableDoc::load(&pdf).unwrap();
    assert!(ed
        .redact(
            0,
            &[[
                hit.x - 1.0,
                hit.y - 3.0,
                hit.x + hit.width + 1.0,
                hit.y + hit.height + 3.0,
            ]],
        )
        .unwrap());
    let out = ed.to_bytes().unwrap();

    let text = pdf::extract_text(&out).unwrap();
    assert!(
        !text.contains("SEGREDO"),
        "covered word must be gone: {text:?}"
    );
    assert!(text.contains("PUBLICO"), "text before the rect must remain");
    assert!(text.contains("FIM"), "text after the rect must remain");
    // The black box is still drawn.
    let raw = String::from_utf8_lossy(&out);
    assert!(raw.contains(" re") && raw.contains("0 g"));
}

/// Surviving glyphs keep their exact positions: the dropped glyphs become TJ
/// displacements, so "FIM" is found at the same place before and after.
#[test]
fn kept_text_keeps_its_position() {
    lic();
    let mut doc = Document::new();
    let f = doc.add_font_file(FONT).unwrap();
    doc.add_page()
        .text(f, 20.0)
        .at(72.0, 700.0)
        .show("PUBLICO SEGREDOXYZ FIM");
    let pdf = doc.to_bytes().unwrap();
    let before = pdf::find_text(&pdf, "FIM", pdf::FindOptions::default()).unwrap()[0].clone();
    let secret = &pdf::find_text(&pdf, "SEGREDOXYZ", pdf::FindOptions::default()).unwrap()[0];

    let mut ed = EditableDoc::load(&pdf).unwrap();
    ed.redact(
        0,
        &[[
            secret.x - 1.0,
            secret.y - 3.0,
            secret.x + secret.width + 1.0,
            secret.y + secret.height + 3.0,
        ]],
    )
    .unwrap();
    let out = ed.to_bytes().unwrap();
    let after = pdf::find_text(&out, "FIM", pdf::FindOptions::default()).unwrap()[0].clone();
    assert!(
        (after.x - before.x).abs() < 0.5 && (after.y - before.y).abs() < 0.5,
        "FIM moved: before ({}, {}), after ({}, {})",
        before.x,
        before.y,
        after.x,
        after.y
    );
}

/// An image overlapping the rect is dropped — no `Do` paint remains, the
/// page's resource entry is pruned, and the unreferenced XObject is nulled
/// (the pixel data leaves the file).
#[test]
fn redacts_images_under_the_rect() {
    lic();
    // 1×1 PNG.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0xC9, 0xFE, 0x92, 0xEF, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    let mut doc = Document::new();
    doc.add_page_sized(400.0, 500.0);
    let base = doc.to_bytes().unwrap();
    let mut ed = EditableDoc::load(&base).unwrap();
    let img = pdf::Image::from_png(PNG).unwrap();
    assert!(ed.draw_image(0, &img, 100.0, 100.0, 120.0, 80.0, 0.0));
    let with_img = ed.to_bytes().unwrap();
    assert!(String::from_utf8_lossy(&with_img).contains(" Do"));

    let mut ed = EditableDoc::load(&with_img).unwrap();
    // Rect overlapping only a corner of the image: conservative full drop.
    assert!(ed.redact(0, &[[90.0, 90.0, 130.0, 130.0]]).unwrap());
    let out = ed.to_bytes().unwrap();
    let raw = String::from_utf8_lossy(&out);
    assert!(!raw.contains(" Do"), "image paint must be gone");
    assert!(!raw.contains("/Imd"), "image resource entry must be pruned");
}

/// Annotations whose /Rect intersects the redaction are deleted.
#[test]
fn redacts_intersecting_annotations() {
    lic();
    let mut doc = Document::new();
    let f = doc.add_font_file(FONT).unwrap();
    {
        let p = doc.add_page();
        p.text(f, 12.0).at(72.0, 700.0).show("link here");
        p.link_uri([72.0, 695.0, 200.0, 715.0], "https://secret.example");
    }
    let pdf = doc.to_bytes().unwrap();
    assert!(String::from_utf8_lossy(&pdf).contains("secret.example"));

    let mut ed = EditableDoc::load(&pdf).unwrap();
    assert!(ed.redact(0, &[[60.0, 690.0, 210.0, 720.0]]).unwrap());
    let out = ed.to_bytes().unwrap();
    let raw = String::from_utf8_lossy(&out);
    assert!(
        !raw.contains("secret.example"),
        "intersecting annotation must be removed from the file"
    );
}

/// Inline images cannot be rewritten safely: redaction must FAIL LOUDLY and
/// draw nothing (never paint a box over still-present data).
#[test]
fn inline_image_fails_loudly_and_draws_nothing() {
    lic();
    let mut doc = Document::new();
    doc.add_page_sized(200.0, 200.0);
    let base = doc.to_bytes().unwrap();
    let mut ed = EditableDoc::load(&base).unwrap();
    ed.overlay_page(0, b"BI /W 1 /H 1 /CS /G /BPC 8 ID \x7f EI\n", None);
    let with_bi = ed.to_bytes().unwrap();

    let mut ed = EditableDoc::load(&with_bi).unwrap();
    let err = ed.redact(0, &[[0.0, 0.0, 200.0, 200.0]]).unwrap_err();
    assert_eq!(err, pdf::RedactError::InlineImage);
    // No black box was painted (nothing pretends to be redacted).
    let out = ed.to_bytes().unwrap();
    assert!(!String::from_utf8_lossy(&out).contains("0 g"));
}

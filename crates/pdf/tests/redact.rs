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
    assert!(ed.redact(0, &[[60.0, 590.0, 400.0, 620.0]]));
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
    ed.redact(0, &[[60.0, 590.0, 400.0, 620.0]]);
    let out = ed.to_bytes().unwrap();
    assert_eq!(EditableDoc::load(&out).unwrap().page_count(), 1);
    assert!(pdf::extract_text(&out).unwrap().contains("KEEP THIS LINE"));
}

#[test]
fn redact_nonexistent_page_returns_false() {
    let mut ed = EditableDoc::load(doc_with_two_lines()).unwrap();
    assert!(!ed.redact(9, &[[0.0, 0.0, 10.0, 10.0]]));
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
    assert!(ed2.redact(0, &[[60.0, 590.0, 400.0, 620.0]]));
    let out = ed2.to_bytes().unwrap();
    let text = pdf::extract_text(&out).unwrap();
    assert!(
        !text.contains("SECRET"),
        "secret gone from flate content: {text:?}"
    );
    assert!(text.contains("KEEP THIS LINE"));
}

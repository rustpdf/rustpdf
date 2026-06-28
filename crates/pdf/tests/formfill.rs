//! Tier 1 tests: filling existing AcroForm fields (with generated `/AP`
//! appearances), flattening forms into static content, and stamping watermarks.
//! Built docs are round-tripped through `EditableDoc` + re-parsed.

use pdf::{Document, EditableDoc, WatermarkOptions};

const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
);

/// A small form to load and manipulate.
fn sample_form() -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_page();
    doc.text_field("fullname", 0, [120.0, 700.0, 360.0, 720.0], "", 12.0);
    doc.checkbox("agree", 0, [120.0, 660.0, 138.0, 678.0], false);
    doc.radio_group(
        "plan",
        0,
        vec![
            ([120.0, 620.0, 138.0, 638.0], "basic".to_string()),
            ([200.0, 620.0, 218.0, 638.0], "pro".to_string()),
        ],
        None,
    );
    doc.dropdown(
        "country",
        0,
        [120.0, 580.0, 300.0, 600.0],
        vec!["Brasil".into(), "Portugal".into()],
        None,
        12.0,
    );
    doc.to_bytes().unwrap()
}

#[test]
fn field_names_lists_all_terminal_fields() {
    let mut form = EditableDoc::load(sample_form()).unwrap();
    let names = form.field_names();
    for expected in ["fullname", "agree", "plan", "country"] {
        assert!(names.iter().any(|n| n == expected), "missing {expected}");
    }
    // Filling a non-existent field returns false.
    assert!(!form.fill_text_field("does-not-exist", "x"));
}

#[test]
fn fill_text_field_generates_appearance() {
    let mut form = EditableDoc::load(sample_form()).unwrap();
    assert!(form.fill_text_field("fullname", "Ada Lovelace"));
    let out = form.to_bytes().unwrap();
    let text = String::from_utf8_lossy(&out);
    // Value set + a generated appearance form (no NeedAppearances needed).
    assert!(text.contains("(Ada Lovelace)"));
    assert!(text.contains("/Subtype /Form"));
    assert!(!text.contains("/NeedAppearances true"));
    assert_eq!(EditableDoc::load(&out).unwrap().page_count(), 1);
}

#[test]
fn checkbox_and_radio_set_state() {
    let mut form = EditableDoc::load(sample_form()).unwrap();
    assert!(form.set_checkbox("agree", true));
    assert!(form.set_radio("plan", "pro"));
    let out = form.to_bytes().unwrap();
    let text = String::from_utf8_lossy(&out);
    // Checkbox on-state is "On" (as authored); radio selects export "pro".
    assert!(text.contains("/AS /On"));
    assert!(text.contains("/V /pro") || text.contains("/AS /pro"));
}

#[test]
fn flatten_removes_acroform_and_bakes_content() {
    let mut form = EditableDoc::load(sample_form()).unwrap();
    form.fill_text_field("fullname", "Grace Hopper");
    form.set_checkbox("agree", true);
    form.flatten_forms();
    let out = form.to_bytes().unwrap();
    let text = String::from_utf8_lossy(&out);

    // No interactive form survives.
    assert!(!text.contains("/AcroForm"));
    assert!(!text.contains("/Subtype /Widget"));
    // The field appearance is now painted into page content via a Form XObject.
    assert!(text.contains(" Do"));
    assert!(text.contains("(Grace Hopper)"));
    // Still parses as a 1-page document.
    assert_eq!(EditableDoc::load(&out).unwrap().page_count(), 1);
}

#[test]
fn flatten_is_safe_on_a_formless_pdf() {
    let mut doc = Document::new();
    doc.add_page();
    let mut ed = EditableDoc::load(doc.to_bytes().unwrap()).unwrap();
    ed.flatten_forms(); // no-op, must not panic
    assert_eq!(
        EditableDoc::load(ed.to_bytes().unwrap())
            .unwrap()
            .page_count(),
        1
    );
}

#[test]
fn text_watermark_stamps_every_page() {
    let mut doc = Document::new();
    doc.add_page();
    doc.add_page();
    let mut ed = EditableDoc::load(doc.to_bytes().unwrap()).unwrap();
    ed.watermark_text("CONFIDENTIAL", WatermarkOptions::default());
    let out = ed.to_bytes().unwrap();
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("(CONFIDENTIAL)"));
    assert!(text.contains("/Helvetica"));
    assert!(text.contains("/ExtGState"));
    assert!(text.contains("/ca 0.3"));
    assert_eq!(EditableDoc::load(&out).unwrap().page_count(), 2);
}

#[test]
fn watermark_preserves_existing_page_fonts() {
    // A page that already draws text (has its own font resources) must keep them
    // after watermarking — the resource merge is a deep merge, not a clobber.
    let mut doc = Document::new();
    let fid = doc.add_font_file(FONT).unwrap();
    doc.add_page().text(fid, 24.0).at(72.0, 700.0).show("Hello");
    let original = doc.to_bytes().unwrap();
    // The embedded subset font is named /F0 in the page resources.
    assert!(String::from_utf8_lossy(&original).contains("/F0"));

    let mut ed = EditableDoc::load(&original).unwrap();
    ed.watermark_text("DRAFT", WatermarkOptions::default());
    let out = ed.to_bytes().unwrap();
    let text = String::from_utf8_lossy(&out);
    // Both the original font and the watermark font survive on the page.
    assert!(text.contains("/F0"));
    assert!(text.contains("/Helvwm"));
    assert!(text.contains("(DRAFT)"));
    assert_eq!(EditableDoc::load(&out).unwrap().page_count(), 1);
}

//! Fase 6.7 tests: interactive AcroForm fields with generated appearance
//! streams and hierarchical names. Confirmed against qpdf manually
//! (`qpdf --json --json-key=acroform` lists every field with its fullname/value).

use pdf::{Document, EditableDoc};

fn form() -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_page();
    doc.text_field(
        "address.city",
        0,
        [120.0, 700.0, 360.0, 720.0],
        "São Paulo",
        12.0,
    );
    doc.text_field(
        "address.zip",
        0,
        [120.0, 670.0, 240.0, 690.0],
        "01000-000",
        12.0,
    );
    doc.checkbox("subscribe", 0, [120.0, 630.0, 138.0, 648.0], true);
    doc.radio_group(
        "plan",
        0,
        vec![
            ([120.0, 600.0, 138.0, 618.0], "basic".to_string()),
            ([200.0, 600.0, 218.0, 618.0], "pro".to_string()),
        ],
        Some(1),
    );
    doc.dropdown(
        "country",
        0,
        [120.0, 560.0, 300.0, 580.0],
        vec!["Brasil".into(), "Portugal".into(), "Angola".into()],
        Some(0),
        12.0,
    );
    doc.to_bytes().unwrap()
}

#[test]
fn acroform_has_all_field_types_with_appearances() {
    let bytes = form();
    let text = String::from_utf8_lossy(&bytes);

    assert!(text.contains("/AcroForm"));
    assert!(text.contains("/FT /Tx")); // text
    assert!(text.contains("/FT /Btn")); // checkbox + radio
    assert!(text.contains("/FT /Ch")); // dropdown
                                       // Every field has a generated appearance and standard DR fonts.
    assert!(text.contains("/AP"));
    assert!(text.contains("/Subtype /Form"));
    assert!(text.contains("/Helv") && text.contains("/ZaDb"));
    // Radio + checkbox states.
    assert!(text.contains("/AS /On") || text.contains("/AS /Off"));
    // Dropdown options.
    assert!(text.contains("/Opt"));
}

#[test]
fn appearance_stream_uses_winansi_not_utf8() {
    // Regression: the `/AP` appearance draws with a WinAnsi Helvetica, so a
    // value like "São Paulo" must be transcoded to the single WinAnsi byte
    // 0xE3 (emitted as octal \343), NOT the two UTF-8 bytes 0xC3 0xA3 which
    // render as "SÃ£o". `/V` stores UTF-16BE and is checked separately.
    let bytes = form();
    // The octal escape for 'ã' (0xE3) must appear in a content-stream literal.
    let needle = b"S\\343o Paulo"; // "S" \343 "o Paulo"
    assert!(
        bytes.windows(needle.len()).any(|w| w == needle),
        "expected WinAnsi octal escape for the appearance value"
    );
    // The raw UTF-8 encoding of 'ã' must NOT appear inside an appearance literal
    // preceding " Paulo" (guards against a UTF-8 regression).
    let utf8_bad = b"S\xc3\xa3o Paulo";
    assert!(
        !bytes.windows(utf8_bad.len()).any(|w| w == utf8_bad),
        "appearance stream must not contain raw UTF-8 for the field value"
    );
}

#[test]
fn hierarchical_names_nest_under_a_parent() {
    let bytes = form();
    let text = String::from_utf8_lossy(&bytes);
    // The dotted name "address.city" must produce a parent field "address"
    // with /Kids and leaf fields named just "city"/"zip" carrying /Parent.
    assert!(text.contains("/T (address)"));
    assert!(text.contains("/T (city)"));
    assert!(text.contains("/Kids"));
    assert!(text.contains("/Parent"));
}

#[test]
fn form_still_parses() {
    let bytes = form();
    assert_eq!(EditableDoc::load(&bytes).unwrap().page_count(), 1);
}

#[test]
fn no_form_means_no_acroform() {
    let mut doc = Document::new();
    doc.add_page();
    let text = String::from_utf8_lossy(&doc.to_bytes().unwrap()).into_owned();
    assert!(!text.contains("/AcroForm"));
}

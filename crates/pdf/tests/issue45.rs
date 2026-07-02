//! Issue #45 P1: read-only page geometry (#1), positioned rect/text stamping
//! (#2) and non-mutating document inspection (#3). Outputs are re-parsed with
//! our own parser/extractor (the sandbox can't spawn qpdf); structural
//! assertions confirm each feature took effect.

use pdf::{
    extract_text, inspect, measure_page, measure_pages, Document, EditableDoc, Encryption,
    Permissions, Version,
};

const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
);

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

fn two_page_doc() -> Vec<u8> {
    let mut doc = Document::new();
    let f = doc.add_font_file(FONT).unwrap();
    // Page 0: A4 portrait. Page 1: US Letter.
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

// ---- P1 #1: page geometry --------------------------------------------------

#[test]
fn measure_pages_reports_each_box() {
    let pages = measure_pages(two_page_doc()).unwrap();
    assert_eq!(pages.len(), 2);

    assert!((pages[0].width - 595.276).abs() < 0.01);
    assert!((pages[0].height - 841.89).abs() < 0.01);
    assert_eq!(pages[0].rotation, 0);
    assert!((pages[0].media_box.width() - 595.276).abs() < 0.01);
    // Unrotated page: rotated size equals plain size.
    assert!((pages[0].rotated_width - pages[0].width).abs() < 0.01);

    assert!((pages[1].width - 612.0).abs() < 0.01);
    assert!((pages[1].height - 792.0).abs() < 0.01);
}

#[test]
fn measure_page_swaps_dimensions_when_rotated() {
    let mut ed = EditableDoc::load(two_page_doc()).unwrap();
    ed.rotate_page(0, 90);
    let bytes = ed.to_bytes().unwrap();

    let g = measure_page(&bytes, 0).unwrap();
    assert_eq!(g.rotation, 90);
    // Unrotated box unchanged, but the rotation-adjusted size is swapped.
    assert!((g.width - 595.276).abs() < 0.01);
    assert!((g.height - 841.89).abs() < 0.01);
    assert!((g.rotated_width - 841.89).abs() < 0.01);
    assert!((g.rotated_height - 595.276).abs() < 0.01);
}

#[test]
fn measure_page_out_of_range_errors() {
    assert!(measure_page(two_page_doc(), 9).is_err());
}

// ---- P1 #2: positioned rect + text -----------------------------------------

#[test]
fn fill_rect_paints_a_white_box() {
    let mut ed = EditableDoc::load(two_page_doc()).unwrap();
    assert!(ed.fill_rect(0, 100.0, 100.0, 200.0, 50.0, (1.0, 1.0, 1.0), 1.0));
    assert!(!ed.fill_rect(5, 0.0, 0.0, 1.0, 1.0, (0.0, 0.0, 0.0), 1.0));
    let bytes = ed.to_bytes().unwrap();
    let body = String::from_utf8_lossy(&bytes);
    assert!(body.contains("1.000 1.000 1.000 rg"), "white fill emitted");
    assert!(body.contains("200.00 50.00 re"), "rect with given size");
}

#[test]
fn place_text_is_extractable() {
    let mut ed = EditableDoc::load(two_page_doc()).unwrap();
    assert!(ed.place_text(1, 120.0, 400.0, "Stamped", 14.0, (0.0, 0.0, 1.0), 0.0));
    let bytes = ed.to_bytes().unwrap();
    let text = extract_text(&bytes).unwrap();
    assert!(
        text.contains("Stamped"),
        "placed text extractable: {text:?}"
    );
}

#[test]
fn place_text_rotation_emits_rotated_matrix() {
    let mut ed = EditableDoc::load(two_page_doc()).unwrap();
    ed.place_text(0, 50.0, 50.0, "Angle", 12.0, (0.0, 0.0, 0.0), 90.0);
    let bytes = ed.to_bytes().unwrap();
    let body = String::from_utf8_lossy(&bytes);
    // 90°: cos=0, sin=1 → "0.00000 1.00000 -1.00000 0.00000 ... Tm".
    assert!(
        body.contains("0.00000 1.00000 -1.00000 0.00000"),
        "rotated text matrix present"
    );
}

// ---- P1 #3: document inspection --------------------------------------------

#[test]
fn inspect_plain_document() {
    let o = inspect(two_page_doc());
    assert_eq!(o.version, "1.7");
    assert!(!o.encrypted);
    assert_eq!(o.encryption, "None");
    assert!(!o.requires_password);
    assert_eq!(o.page_count, 2);
    assert_eq!(o.pdfa_level, None);
}

#[test]
fn inspect_detects_pdfa_level() {
    lic();
    let mut doc = Document::new();
    let f = doc.add_font_file(FONT).unwrap();
    doc.add_page().text(f, 18.0).at(72.0, 700.0).show("A");
    let pdfa = doc.pdfa().to_bytes().unwrap(); // PDF/A-2b

    let o = inspect(&pdfa);
    assert_eq!(o.pdfa_level.as_deref(), Some("2b"), "detect A-2b");
}

#[test]
fn inspect_detects_pdfa4() {
    lic();
    let mut doc = Document::new();
    let f = doc.add_font_file(FONT).unwrap();
    doc.add_page().text(f, 18.0).at(72.0, 700.0).show("A");
    let pdfa = doc.pdfa4().to_bytes().unwrap(); // PDF/A-4 (PDF 2.0)

    let o = inspect(&pdfa);
    assert_eq!(o.version, "2.0", "PDF/A-4 is PDF 2.0");
    assert_eq!(o.pdfa_level.as_deref(), Some("4"));
}

#[test]
fn inspect_encrypted_no_user_password() {
    lic();
    let mut ed = EditableDoc::load(two_page_doc()).unwrap();
    // Empty user password, owner-only restriction.
    ed.encrypt_with(Encryption::Aes256, "", "owner", Permissions::read_only());
    let bytes = ed.to_bytes().unwrap();

    let o = inspect(&bytes);
    assert!(o.encrypted);
    assert_eq!(o.encryption, "AES-256");
    assert!(
        !o.requires_password,
        "empty user password opens it (owner restricts perms)"
    );
}

#[test]
fn inspect_encrypted_requires_user_password() {
    lic();
    let mut ed = EditableDoc::load(two_page_doc()).unwrap();
    ed.encrypt_with(Encryption::Rc4, "secret", "owner", Permissions::read_only());
    let bytes = ed.to_bytes().unwrap();

    let o = inspect(&bytes);
    assert!(o.encrypted);
    assert_eq!(o.encryption, "RC4");
    assert!(o.requires_password, "non-empty user password is required");
}

#[test]
fn inspect_reports_catalog_version_override() {
    let mut ed = EditableDoc::load(two_page_doc()).unwrap();
    ed.set_version(Version::V2_0);
    let bytes = ed.to_bytes().unwrap();
    let o = inspect(&bytes);
    assert_eq!(o.version, "2.0");
}

/// Regression: stamping text/images onto a page whose `/Resources` is an
/// **indirect reference** (not an inline dict) must MERGE into the existing
/// resources, not replace them with a dict holding only the stamp's font.
/// A real ForSign-signed file lost all original text because `add_page_resource`
/// only matched an inline `/Resources` dict, dropping the page's `/TT*` fonts so
/// its untouched content stream rendered blank.
#[test]
fn place_text_preserves_indirect_page_resources() {
    let original = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/indirect_resources.pdf"
    ));
    // Sanity: the original page draws this text.
    let before = extract_text(original).unwrap();
    assert!(
        before.contains("SIGN_ID_1"),
        "fixture should contain original text"
    );

    let mut ed = EditableDoc::load(original).unwrap();
    ed.place_text(
        0,
        236.0,
        10.0,
        "Assinado eletronicamente",
        8.0,
        (0.0, 0.0, 0.0),
        0.0,
    );
    let out = ed.to_bytes().unwrap();

    // Original text survives, and the stamp is added.
    let after = extract_text(&out).unwrap();
    assert!(
        after.contains("SIGN_ID_1") && after.contains("Assinante 1"),
        "original page text must survive stamping, got: {after:?}"
    );
    assert!(
        after.contains("Assinado eletronicamente"),
        "stamp must be present"
    );
}

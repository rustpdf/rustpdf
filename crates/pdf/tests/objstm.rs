//! Fase 6.8 tests: object streams + cross-reference stream on write. Outputs are
//! re-parsed with our own parser (the sandbox can't spawn qpdf); `qpdf --check`
//! is confirmed manually in the shell.

use pdf::{Document, EditableDoc};

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

fn sample(n: usize) -> Vec<u8> {
    lic();
    let mut doc = Document::new();
    let font = doc.add_font_file(FONT).unwrap();
    for i in 0..n {
        let page = doc.add_page();
        page.content()
            .set_fill_rgb(0.2, 0.4, 0.8)
            .rect(72.0, 700.0, 200.0, 60.0)
            .fill();
        page.text(font, 18.0)
            .at(80.0, 720.0)
            .show(format!("Página número {i}"));
    }
    doc.to_bytes().unwrap()
}

#[test]
fn compact_uses_objstm_and_xref_stream_and_is_smaller() {
    let classic = {
        let mut d = EditableDoc::load(sample(6)).unwrap();
        d.compact(false);
        d.to_bytes().unwrap()
    };
    let compact = {
        let mut d = EditableDoc::load(sample(6)).unwrap();
        d.compact(true);
        d.to_bytes().unwrap()
    };

    let text = String::from_utf8_lossy(&compact);
    assert!(text.contains("/Type /ObjStm"), "no object stream emitted");
    assert!(text.contains("/Type /XRef"), "no cross-reference stream");
    // The classic table keyword must be gone from the compact form.
    assert!(
        !text.contains("\nxref\n"),
        "classic xref table still present"
    );
    assert!(
        compact.len() < classic.len(),
        "compact ({}) should be smaller than classic ({})",
        compact.len(),
        classic.len()
    );
}

#[test]
fn compact_output_round_trips_through_our_parser() {
    let mut d = EditableDoc::load(sample(4)).unwrap();
    d.compact(true);
    let bytes = d.to_bytes().unwrap();

    // Re-parse: object-stream-packed objects must resolve, pages intact.
    let reloaded = EditableDoc::load(&bytes).unwrap();
    assert_eq!(reloaded.page_count(), 4);

    let extracted = pdf::extract_text(&bytes).unwrap();
    assert!(extracted.contains("Página número 0"));
    assert!(extracted.contains("Página número 3"));
}

#[test]
fn optimize_implies_compact() {
    let mut d = EditableDoc::load(sample(3)).unwrap();
    d.optimize();
    let bytes = d.to_bytes().unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("/Type /XRef"));
    assert_eq!(EditableDoc::load(&bytes).unwrap().page_count(), 3);
}

#[test]
fn encrypted_output_stays_classic_even_if_compact_requested() {
    use pdf::Permissions;
    let mut d = EditableDoc::load(sample(2)).unwrap();
    d.compact(true);
    d.encrypt("", "owner", Permissions::default());
    let bytes = d.to_bytes().unwrap();
    let text = String::from_utf8_lossy(&bytes);
    // Encryption forces the classic table (object streams would break per-object
    // encryption); confirm no XRef stream and the file still decrypts.
    assert!(!text.contains("/Type /XRef"));
    assert!(text.contains("\nxref\n"));
    assert_eq!(EditableDoc::load(&bytes).unwrap().page_count(), 2);
}

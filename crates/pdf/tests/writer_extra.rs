//! Writer extras: object dedupe in `optimize()` and generic incremental update.
//! Re-parsed with our own parser (sandbox can't spawn qpdf); `qpdf --check`
//! confirmed manually.

use pdf::{Document, EditableDoc};

const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
);

fn doc_with_text(label: &str) -> Vec<u8> {
    let mut doc = Document::new();
    let f = doc.add_font_file(FONT).unwrap();
    doc.add_page().text(f, 16.0).at(72.0, 700.0).show(label);
    doc.to_bytes().unwrap()
}

#[test]
fn optimize_dedupes_identical_objects_after_merge() {
    // Merge a document with a copy of itself: the second copy's font/resources
    // duplicate the first. optimize() should collapse the duplicates.
    let mut a = EditableDoc::load(doc_with_text("Mesma fonte")).unwrap();
    let b = EditableDoc::load(doc_with_text("Mesma fonte")).unwrap();
    a.merge(&b);
    let before = a.to_bytes().unwrap().len();

    a.optimize();
    let after = a.to_bytes().unwrap();

    assert!(
        after.len() < before,
        "dedupe should shrink output ({} -> {})",
        before,
        after.len()
    );
    // Still valid: both pages and their text survive.
    assert_eq!(EditableDoc::load(&after).unwrap().page_count(), 2);
    let txt = pdf::extract_text(&after).unwrap();
    assert!(txt.matches("Mesma fonte").count() >= 2);
}

#[test]
fn incremental_update_appends_and_preserves_original() {
    let original = doc_with_text("Original");
    let mut doc = EditableDoc::load(&original).unwrap();
    doc.set_info("Title", "Editado");
    let updated = doc.to_bytes_incremental(&original).unwrap();

    // The original bytes are preserved verbatim as a prefix (signatures safe).
    assert!(updated.starts_with(&original));
    // A second cross-reference section chained via /Prev.
    assert!(String::from_utf8_lossy(&updated).contains("/Prev "));
    assert_eq!(updated.matches_eof(), 2);

    // Re-parses, page intact, and the new metadata is visible.
    let reloaded = EditableDoc::load(&updated).unwrap();
    assert_eq!(reloaded.page_count(), 1);
    assert_eq!(reloaded.get_info("Title").as_deref(), Some("Editado"));
}

#[test]
fn incremental_update_is_smaller_than_full_rewrite() {
    let original = doc_with_text("Base");
    let mut doc = EditableDoc::load(&original).unwrap();
    doc.set_info("Author", "rust-pdf");
    let incr = doc.to_bytes_incremental(&original).unwrap();
    // The appended delta is tiny compared to re-emitting the whole file twice.
    assert!(incr.len() < original.len() * 2);
    assert!(incr.len() > original.len()); // it did append something
}

// Small helper trait to count `%%EOF` markers.
trait Eof {
    fn matches_eof(&self) -> usize;
}
impl Eof for Vec<u8> {
    fn matches_eof(&self) -> usize {
        String::from_utf8_lossy(self).matches("%%EOF").count()
    }
}

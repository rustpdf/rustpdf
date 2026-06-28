//! Fase 6 (manipulation) end-to-end tests. Self-contained: outputs are
//! re-parsed with our own parser/EditableDoc (the test sandbox can't spawn
//! qpdf). `qpdf --check` acceptance is confirmed manually in the shell.

use cos::{Dict, Object, Reference};
use pdf::{Align, Document, EditableDoc, Paragraph};

const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
);

/// A document with `n` pages, each labeled with extractable text.
fn sample(n: usize) -> Vec<u8> {
    let mut doc = Document::new();
    let font = doc.add_font_file(FONT).unwrap();
    for i in 0..n {
        let page = doc.add_page();
        page.content()
            .set_fill_rgb(0.2, 0.4, 0.8)
            .rect(72.0, 700.0, 100.0, 60.0)
            .fill();
        page.text(font, 18.0)
            .at(72.0, 670.0)
            .show(format!("Página {}", i + 1));
    }
    doc.to_bytes().unwrap()
}

#[test]
fn merge_concatenates_pages() {
    let a = EditableDoc::load(sample(2)).unwrap();
    let b = EditableDoc::load(sample(3)).unwrap();
    let mut merged = a;
    merged.merge(&b);
    assert_eq!(merged.page_count(), 5);

    // Round-trips back to a parseable 5-page document.
    let bytes = merged.to_bytes().unwrap();
    let reparsed = EditableDoc::load(&bytes).unwrap();
    assert_eq!(reparsed.page_count(), 5);
}

#[test]
fn extract_pages_keeps_only_selected() {
    let doc = EditableDoc::load(sample(4)).unwrap();
    let sub = doc.extract_pages(&[1, 3]); // pages 2 and 4
    assert_eq!(sub.page_count(), 2);

    let bytes = sub.to_bytes().unwrap();
    let text = pdf::extract_text(&bytes).unwrap();
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.contains("Página 2"), "got: {flat:?}");
    assert!(flat.contains("Página 4"), "got: {flat:?}");
    assert!(!flat.contains("Página 1"), "leaked page 1: {flat:?}");
}

#[test]
fn rotate_reorder_delete() {
    let mut doc = EditableDoc::load(sample(3)).unwrap();

    doc.rotate_page(0, 90);
    doc.delete_page(1); // remove middle page
    assert_eq!(doc.page_count(), 2);

    let bytes = doc.to_bytes().unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("/Rotate 90"));
    let text = pdf::extract_text(&bytes).unwrap();
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.contains("Página 1") && flat.contains("Página 3"));
    assert!(!flat.contains("Página 2"), "deleted page still present");

    // Reorder the two remaining pages.
    let mut doc2 = EditableDoc::load(&bytes).unwrap();
    doc2.reorder_pages(&[1, 0]);
    assert_eq!(doc2.page_count(), 2);
}

#[test]
fn text_extraction_matches_content() {
    let pdf = sample(2);
    let text = pdf::extract_text(&pdf).unwrap();
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat.contains("Página 1"), "got: {flat:?}");
    assert!(flat.contains("Página 2"), "got: {flat:?}");
}

#[test]
fn text_extraction_handles_justified_paragraph() {
    let mut doc = Document::new();
    let font = doc.add_font_file(FONT).unwrap();
    let body = "The quick brown fox jumps over the lazy dog and then keeps on \
        running across the whole column so the paragraph wraps and justifies.";
    doc.add_page().paragraph(
        Paragraph::new(font, 12.0)
            .box_at(72.0, 740.0, 300.0)
            .align(Align::Justify)
            .text(body),
    );
    let text = pdf::extract_text(doc.to_bytes().unwrap()).unwrap();
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let want: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(flat, want, "justified text not recovered");
}

#[test]
fn metadata_info_and_xmp_roundtrip() {
    let mut doc = EditableDoc::load(sample(1)).unwrap();
    doc.set_info("Title", "Relatório Anual");
    doc.set_info("Author", "rust-pdf");
    let xmp = b"<?xpacket?><x:xmpmeta xmlns:x='adobe:ns:meta/'></x:xmpmeta>";
    doc.set_xmp(xmp.to_vec());

    let bytes = doc.to_bytes().unwrap();
    let reloaded = EditableDoc::load(&bytes).unwrap();
    assert_eq!(
        reloaded.get_info("Title").as_deref(),
        Some("Relatório Anual")
    );
    assert_eq!(reloaded.get_info("Author").as_deref(), Some("rust-pdf"));
    assert_eq!(reloaded.get_xmp().as_deref(), Some(xmp.as_slice()));
}

#[test]
fn overlay_adds_content_without_corruption() {
    let mut doc = EditableDoc::load(sample(1)).unwrap();
    // A red diagonal "watermark" stroke.
    doc.overlay_page(0, b"1 0 0 RG 3 w 72 72 m 500 770 l S", None);
    let bytes = doc.to_bytes().unwrap();
    // Still parses, still one page, original text survives.
    let reloaded = EditableDoc::load(&bytes).unwrap();
    assert_eq!(reloaded.page_count(), 1);
    let text = pdf::extract_text(&bytes).unwrap();
    assert!(text.contains("Página 1"));
}

#[test]
fn optimize_drops_unused_and_shrinks() {
    let mut doc = EditableDoc::load(sample(5)).unwrap();
    doc.delete_page(0);
    doc.delete_page(0); // delete two pages -> their objects become unused
    let before = doc.to_bytes().unwrap().len();

    doc.optimize();
    let after = doc.to_bytes().unwrap();
    assert!(after.len() <= before, "optimize grew the file");

    let reloaded = EditableDoc::load(&after).unwrap();
    assert_eq!(reloaded.page_count(), 3);
    assert!(pdf::extract_text(&after).unwrap().contains("Página 3"));
}

#[test]
fn fill_text_field_sets_value_and_generates_appearance() {
    // Hand-build a minimal AcroForm with one text field via the writer.
    let form = build_form_pdf();
    let mut doc = EditableDoc::load(&form).unwrap();
    assert!(doc.fill_text_field("name", "Ada Lovelace"));

    let bytes = doc.to_bytes().unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("(Ada Lovelace)"), "field value not set");
    // The fill now generates a `/AP` appearance stream, so viewers need not
    // regenerate one (`NeedAppearances` stays off).
    assert!(text.contains("/Subtype /Form"));
    assert!(!text.contains("/NeedAppearances true"));
}

/// A tiny one-page PDF with a single AcroForm text field named "name".
fn build_form_pdf() -> Vec<u8> {
    let mut w = writer::Document::new(writer::PdfVersion::V1_7);
    let catalog = w.reserve();
    let pages = w.reserve();
    let page = w.reserve();
    let field = w.reserve();
    let acro = w.reserve();

    w.assign(
        field,
        Dict::new()
            .with("Type", Object::name("Annot"))
            .with("Subtype", Object::name("Widget"))
            .with("FT", Object::name("Tx"))
            .with("T", cos::PdfString::literal("name".as_bytes().to_vec()))
            .with(
                "Rect",
                Object::Array(vec![72.into(), 700.into(), 300.into(), 720.into()]),
            )
            .with("P", page),
    );
    w.assign(
        acro,
        Dict::new().with("Fields", Object::Array(vec![Object::Reference(field)])),
    );
    w.assign(
        page,
        Dict::new()
            .with("Type", Object::name("Page"))
            .with("Parent", pages)
            .with(
                "MediaBox",
                Object::Array(vec![0.into(), 0.into(), 612.into(), 792.into()]),
            )
            .with("Annots", Object::Array(vec![Object::Reference(field)])),
    );
    w.assign(
        pages,
        Dict::new()
            .with("Type", Object::name("Pages"))
            .with("Kids", Object::Array(vec![Object::Reference(page)]))
            .with("Count", 1),
    );
    w.assign(
        catalog,
        Dict::new()
            .with("Type", Object::name("Catalog"))
            .with("Pages", pages)
            .with("AcroForm", acro),
    );
    w.set_root(catalog);
    let _ = Reference::new(1);
    w.write().unwrap()
}

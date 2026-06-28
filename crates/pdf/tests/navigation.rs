//! Tier 1 tests: hyperlinks (`/Link` annotations) and bookmarks (`/Outlines`).
//! Outputs are re-parsed with our own parser to confirm they stay well-formed.

use pdf::{Bookmark, Document, EditableDoc};

#[test]
fn web_link_emits_uri_action() {
    let mut doc = Document::new();
    doc.add_page()
        .link_uri([72.0, 700.0, 300.0, 720.0], "https://example.com/");
    let bytes = doc.to_bytes().unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Subtype /Link"));
    assert!(text.contains("/S /URI"));
    assert!(text.contains("(https://example.com/)"));
    assert!(text.contains("/Annots"));
    // Still a valid PDF for our parser.
    assert_eq!(EditableDoc::load(&bytes).unwrap().page_count(), 1);
}

#[test]
fn internal_link_points_at_target_page() {
    let mut doc = Document::new();
    doc.add_page()
        .link_to_page([72.0, 700.0, 200.0, 720.0], 1, Some(800.0));
    doc.add_page(); // destination
    let bytes = doc.to_bytes().unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Subtype /Link"));
    assert!(text.contains("/Dest"));
    assert!(text.contains("/XYZ"));
    assert_eq!(EditableDoc::load(&bytes).unwrap().page_count(), 2);
}

#[test]
fn links_coexist_with_form_widgets_in_annots() {
    let mut doc = Document::new();
    doc.add_page()
        .link_uri([72.0, 700.0, 200.0, 720.0], "https://rust-pdf.dev/");
    doc.text_field("name", 0, [72.0, 600.0, 300.0, 620.0], "", 12.0);
    let bytes = doc.to_bytes().unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Subtype /Link"));
    assert!(text.contains("/Subtype /Widget"));
    assert!(text.contains("/AcroForm"));
    assert_eq!(EditableDoc::load(&bytes).unwrap().page_count(), 1);
}

#[test]
fn no_links_means_no_link_annotations() {
    let mut doc = Document::new();
    doc.add_page();
    let text = String::from_utf8_lossy(&doc.to_bytes().unwrap()).into_owned();
    assert!(!text.contains("/Subtype /Link"));
}

#[test]
fn nested_bookmarks_build_outline_tree() {
    let mut doc = Document::new();
    doc.add_page();
    doc.add_page();
    doc.add_page();
    doc.add_bookmark(
        Bookmark::new("Chapter 1", 0)
            .at_top(800.0)
            .child(Bookmark::new("Section 1.1", 1))
            .child(Bookmark::new("Section 1.2", 2)),
    );
    doc.add_bookmark(Bookmark::new("Chapter 2", 2));
    let bytes = doc.to_bytes().unwrap();
    let text = String::from_utf8_lossy(&bytes);

    assert!(text.contains("/Type /Outlines"));
    assert!(text.contains("/Outlines"));
    assert!(text.contains("/PageMode /UseOutlines"));
    assert!(text.contains("(Chapter 1)"));
    assert!(text.contains("(Section 1.1)"));
    assert!(text.contains("(Chapter 2)"));
    // Tree wiring + destinations.
    assert!(text.contains("/First"));
    assert!(text.contains("/Last"));
    assert!(text.contains("/Next"));
    assert!(text.contains("/Prev"));
    assert!(text.contains("/Dest"));
    // The two top-level chapters plus two children = 4 visible entries.
    assert!(text.contains("/Count 4"));

    assert_eq!(EditableDoc::load(&bytes).unwrap().page_count(), 3);
}

#[test]
fn no_bookmarks_means_no_outline() {
    let mut doc = Document::new();
    doc.add_page();
    let text = String::from_utf8_lossy(&doc.to_bytes().unwrap()).into_owned();
    assert!(!text.contains("/Outlines"));
    assert!(!text.contains("/UseOutlines"));
}

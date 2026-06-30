//! Issue #41 P1 #6: positional text search returns bounding boxes in PDF user
//! space, anchored where text was drawn. No license required (like extraction).

use pdf::{find_text, Document, FindOptions};

const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
);

fn two_line_doc() -> Vec<u8> {
    let mut doc = Document::new();
    let f = doc.add_font_file(FONT).unwrap();
    let p = doc.add_page();
    p.text(f, 20.0).at(72.0, 700.0).show("Hello World");
    p.text(f, 20.0).at(72.0, 600.0).show("Goodbye World");
    doc.to_bytes().unwrap()
}

#[test]
fn finds_unique_term_with_box_near_baseline() {
    let bytes = two_line_doc();
    let hits = find_text(&bytes, "Hello", FindOptions::default()).unwrap();
    assert_eq!(hits.len(), 1, "exactly one 'Hello'");
    let h = &hits[0];
    assert_eq!(h.page, 0);
    assert_eq!(h.text, "Hello");
    // Drawn at (72, 700) baseline: left edge near 72, box straddles the baseline.
    assert!((h.x - 72.0).abs() < 6.0, "x≈72, got {}", h.x);
    assert!(
        h.y < 700.0 && h.y + h.height > 700.0,
        "box straddles baseline 700: y={} h={}",
        h.y,
        h.height
    );
    // Height ≈ font size (ascent+descent ratios), width positive & sane.
    assert!(
        h.height > 15.0 && h.height < 25.0,
        "height≈20, got {}",
        h.height
    );
    assert!(
        h.width > 30.0,
        "width should cover 5 glyphs, got {}",
        h.width
    );
}

#[test]
fn case_insensitive_by_default() {
    let bytes = two_line_doc();
    let hits = find_text(&bytes, "hello", FindOptions::default()).unwrap();
    assert_eq!(hits.len(), 1);
    let none = find_text(
        &bytes,
        "hello",
        FindOptions {
            case_sensitive: true,
        },
    )
    .unwrap();
    assert_eq!(
        none.len(),
        0,
        "case-sensitive 'hello' must not match 'Hello'"
    );
}

#[test]
fn repeated_term_returns_one_box_per_occurrence() {
    let bytes = two_line_doc();
    let hits = find_text(&bytes, "World", FindOptions::default()).unwrap();
    assert_eq!(hits.len(), 2, "'World' appears on both lines");
    // Different baselines → different y.
    assert!((hits[0].y - hits[1].y).abs() > 50.0);
}

#[test]
fn missing_term_and_empty_query_yield_nothing() {
    let bytes = two_line_doc();
    assert!(find_text(&bytes, "Nonexistent", FindOptions::default())
        .unwrap()
        .is_empty());
    assert!(find_text(&bytes, "", FindOptions::default())
        .unwrap()
        .is_empty());
}

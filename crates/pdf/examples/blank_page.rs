//! Fase 1.6 milestone: generate a single blank A4 page.
//!
//! Run with: `cargo run -p pdf --example blank_page -- out.pdf`

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "blank.pdf".to_string());

    let mut doc = pdf::Document::new();
    doc.add_page();
    doc.save(&path).expect("failed to write PDF");

    println!("wrote blank A4 page to {path}");
}

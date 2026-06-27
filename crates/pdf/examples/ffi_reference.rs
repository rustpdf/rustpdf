//! Reference drawing for the FFI dogfood (Fase 1.7). The Python binding builds
//! the identical drawing through the C ABI; the two outputs must be
//! byte-for-byte equal.
//!
//! Run with: `cargo run -p pdf --example ffi_reference -- out.pdf`

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "rust_reference.pdf".to_string());

    let mut doc = pdf::Document::new();
    doc.add_page()
        .content()
        .set_fill_rgb(1.0, 0.0, 0.0)
        .rect(0.0, 0.0, 100.0, 100.0)
        .fill();
    doc.save(&path).expect("failed to write PDF");

    println!("wrote reference drawing to {path}");
}

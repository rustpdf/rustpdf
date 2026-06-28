//! Minimal runnable demo:
//!   RUSTPDF_LIB=/path/to/libpdf_ffi.dylib cargo run --example hello -- out.pdf
//!
//! From the repo it also works without RUSTPDF_LIB if the engine cdylib is in
//! `target/debug` or `target/release` (the loader walks up to find it).

use rustpdf::{Align, Document};

fn main() -> rustpdf::Result<()> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "out.pdf".into());

    println!("rust-pdf engine version: {}", rustpdf::version());

    let mut doc = Document::new()?;
    doc.set_info(Some("Hello from Rust"), Some("rustpdf"), None, None, None)?;
    doc.add_page()?;

    doc.set_fill_rgb(0.10, 0.20, 0.80)?
        .rect(72.0, 700.0, 200.0, 60.0)?
        .fill()?;

    let font = doc.add_font_file("../../assets/fonts/Roboto-Regular.ttf")?;
    doc.show_text(font, 28.0, 72.0, 640.0, "Hello from Rust", 1)?;
    doc.paragraph(
        font,
        12.0,
        72.0,
        600.0,
        420.0,
        Align::Justify,
        "This PDF was produced by the rust-pdf engine through its C ABI, \
         loaded at run time by the rustpdf binding — no engine source required.",
    )?;

    doc.save(&out)?;
    println!("wrote {out}");
    Ok(())
}

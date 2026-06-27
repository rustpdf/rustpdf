//! Fase 3F milestone: the L2 DX layer — a heading plus auto-wrapped,
//! aligned, inline-styled paragraphs. Mirrors the spirit of the §1.2.1 target
//! snippet (`doc.page().heading().paragraph()...`).
//!
//! Run: `cargo run -p pdf --example report -- out.pdf`

use pdf::{Align, Document, Paragraph};

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "report.pdf".to_string());

    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts");
    let mut doc = Document::new();
    let regular = doc
        .add_font_file(format!("{dir}/Roboto-Regular.ttf"))
        .unwrap();
    let bold = doc.add_font_file(format!("{dir}/Roboto-Bold.ttf")).unwrap();

    let body = "Rust-pdf builds PDF files from a single Rust core with thin \
        foreign-language bindings. This paragraph is laid out automatically: the \
        engine shapes each word with HarfBuzz-quality kerning, measures it, and \
        greedily breaks lines to fit the column width. Justified alignment then \
        distributes the leftover space evenly between words so both margins stay \
        flush, exactly as a typesetting engine should.";

    let page = doc.add_page();

    // Heading (large bold text).
    page.text(bold, 28.0)
        .fill(0.1, 0.1, 0.1)
        .at(72.0, 770.0)
        .show("Fatura #42");

    // Justified body paragraph inside a 451pt-wide column.
    page.paragraph(
        Paragraph::new(regular, 12.0)
            .box_at(72.0, 730.0, 451.0)
            .leading(18.0)
            .align(Align::Justify)
            .fill(0.15, 0.15, 0.15)
            .text(body),
    );

    // Inline-styled paragraph: mixed weight, size and color in one flow.
    page.paragraph(
        Paragraph::new(regular, 13.0)
            .box_at(72.0, 560.0, 451.0)
            .leading(20.0)
            .align(Align::Left)
            .text("Total devido: ")
            .span("R$ 1.234,56", bold, 13.0, Some((0.78, 0.12, 0.12)))
            .text("  — vencimento em ")
            .span("30 dias", bold, 13.0, Some((0.0, 0.0, 0.0)))
            .text("."),
    );

    // Centered and right-aligned lines to show alignment modes.
    page.paragraph(
        Paragraph::new(regular, 11.0)
            .box_at(72.0, 500.0, 451.0)
            .align(Align::Center)
            .text("— centralizado —"),
    );
    page.paragraph(
        Paragraph::new(regular, 11.0)
            .box_at(72.0, 480.0, 451.0)
            .align(Align::Right)
            .text("alinhado à direita"),
    );

    doc.save(&path).expect("save");
    println!("wrote {path}");
}

//! Fase 7.6 milestone: an auto-paginated report with a running header, page
//! numbers, body paragraphs and a long table that breaks across pages with a
//! repeated header row.
//!
//! Run: `cargo run -p pdf --example flow_report -- out.pdf`

use pdf::{Document, Report, Table};

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "report.pdf".to_string());

    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts");
    let mut doc = Document::new();
    let font = doc
        .add_font_file(format!("{dir}/Roboto-Regular.ttf"))
        .unwrap();

    let mut table = Table::new(font, 10.0, vec![60.0, 240.0, 90.0])
        .header_row(true)
        .row(["#", "Item", "Valor"]);
    for i in 1..=40 {
        table = table.row([
            i.to_string(),
            format!(
                "Produto {i} com uma descrição que pode quebrar em mais de uma linha quando longa"
            ),
            format!("R$ {},00", i * 7),
        ]);
    }

    let report = Report::new(font)
        .header("rust-pdf — Relatório de Demonstração (Fase 7.6)")
        .page_numbers(true)
        .body_size(11.0)
        .heading("Relatório Anual", 24.0)
        .paragraph_justified(
            "Este relatório é gerado pelo motor de layout de alto nível: o texto \
             flui automaticamente, quebra em linhas, justifica e paginа sozinho. \
             A tabela abaixo tem quarenta linhas e quebra entre páginas repetindo \
             o cabeçalho.",
        )
        .spacer(12.0)
        .heading("Itens", 16.0)
        .table(table);

    report.render(&mut doc);
    doc.save(&path).expect("save");
    println!("wrote {path} with {} pages", doc.page_count());
}

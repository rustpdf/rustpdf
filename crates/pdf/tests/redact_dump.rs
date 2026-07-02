//! Helper (ignored): dump a redacted PDF for external validation (qpdf/pdftotext).
use pdf::{Document, EditableDoc};

#[test]
#[ignore]
fn dump_redacted_for_external_validation() {
    pdf::activate_license(
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../license/fixtures/dev_license.txt"
        ))
        .trim(),
    )
    .unwrap();
    let font = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/Roboto-Regular.ttf"
    );
    let mut doc = Document::new();
    let f = doc.add_font_file(font).unwrap();
    doc.add_page()
        .text(f, 20.0)
        .at(72.0, 700.0)
        .show("PUBLICO SEGREDOXYZ FIM");
    let pdf_bytes = doc.to_bytes().unwrap();
    let hit = &pdf::find_text(&pdf_bytes, "SEGREDOXYZ", pdf::FindOptions::default()).unwrap()[0];
    let mut ed = EditableDoc::load(&pdf_bytes).unwrap();
    ed.redact(
        0,
        &[[
            hit.x - 1.0,
            hit.y - 3.0,
            hit.x + hit.width + 1.0,
            hit.y + hit.height + 3.0,
        ]],
    )
    .unwrap();
    let out = std::env::var("REDACT_DUMP").unwrap();
    std::fs::write(format!("{out}/before.pdf"), &pdf_bytes).unwrap();
    std::fs::write(format!("{out}/after.pdf"), ed.to_bytes().unwrap()).unwrap();
}

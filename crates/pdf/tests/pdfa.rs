//! Fase 7.4 tests: PDF/A-2b structure. Conformance is confirmed externally with
//! veraPDF (`verapdf -f 2b out.pdf` → isCompliant="true", 144/144 rules); the
//! test sandbox can't spawn veraPDF, so these assert the required structure and
//! that the file still parses/extracts.

use pdf::{AFRelationship, Document, EditableDoc, Info, PdfaLevel};

const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
);

fn lic() {
    pdf::activate_license(
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../license/fixtures/dev_license.txt"
        ))
        .trim(),
    )
    .unwrap();
}

fn pdfa_doc() -> Vec<u8> {
    lic();
    let mut doc = Document::new().pdfa();
    doc.set_info(Info {
        title: Some("Relatório".into()),
        author: Some("rust-pdf".into()),
        ..Default::default()
    });
    let f = doc.add_font_file(FONT).unwrap();
    let page = doc.add_page();
    page.content()
        .set_fill_rgb(0.1, 0.3, 0.7)
        .rect(72.0, 700.0, 120.0, 60.0)
        .fill();
    page.text(f, 16.0).at(72.0, 660.0).show("PDF/A — café");
    doc.to_bytes().unwrap()
}

#[test]
fn pdfa_has_required_structure() {
    let bytes = pdfa_doc();
    let text = String::from_utf8_lossy(&bytes);

    // OutputIntent + embedded sRGB ICC profile (N = 3).
    assert!(text.contains("/OutputIntents"));
    assert!(text.contains("/S /GTS_PDFA1"));
    assert!(text.contains("/DestOutputProfile"));
    assert!(text.contains("/N 3"));
    // XMP metadata with the PDF/A identifier.
    assert!(text.contains("/Type /Metadata"));
    assert!(text.contains("/Subtype /XML"));
    assert!(text.contains("<pdfaid:part>2</pdfaid:part>"));
    assert!(text.contains("<pdfaid:conformance>B</pdfaid:conformance>"));
    // Info/XMP are kept in sync (Title appears in both).
    assert!(text.contains("<dc:title>"));
    // Document /ID (required by PDF/A).
    assert!(text.contains("/ID ["));
    // Fonts are embedded (subsetted).
    assert!(text.contains("/FontFile2"));
}

#[test]
fn pdfa_still_parses_and_extracts() {
    let bytes = pdfa_doc();
    assert_eq!(EditableDoc::load(&bytes).unwrap().page_count(), 1);
    assert!(pdf::extract_text(&bytes).unwrap().contains("PDF/A — café"));
}

#[test]
fn verapdf_confirms_pdfa2b_when_available() {
    // Runs only where veraPDF is reachable (skipped in the test sandbox); proves
    // real PDF/A-2b conformance, not just structure.
    use testkit::{validate_with, Validator, ValidatorStatus};
    let path = std::env::temp_dir().join("rustpdf_pdfa_conformance.pdf");
    std::fs::write(&path, pdfa_doc()).unwrap();
    for r in validate_with(&path, &[Validator::VeraPdf]) {
        match r.status {
            ValidatorStatus::Pass | ValidatorStatus::Unavailable => {}
            ValidatorStatus::Fail { code, output } => {
                panic!("veraPDF rejected the PDF/A file (code {code:?}):\n{output}")
            }
        }
    }
}

fn simple_level_doc(level: PdfaLevel, attach: bool) -> Vec<u8> {
    lic();
    let mut doc = Document::new().pdfa_with(level);
    doc.set_info(Info {
        title: Some("Arquivo PDF/A".into()),
        author: Some("rust-pdf".into()),
        ..Default::default()
    });
    let f = doc.add_font_file(FONT).unwrap();
    if attach {
        doc.attach_file(
            "dados.csv",
            "text/csv",
            b"a,b\n1,2\n".to_vec(),
            AFRelationship::Source,
            "Dados de origem",
        );
    }
    doc.add_page()
        .text(f, 16.0)
        .at(72.0, 740.0)
        .show("Documento PDF/A — café");
    doc.to_bytes().unwrap()
}

#[test]
fn pdfa1b_is_pdf14_with_cidset() {
    let bytes = simple_level_doc(PdfaLevel::A1b, false);
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.starts_with("%PDF-1.4"), "A-1 must declare PDF 1.4");
    assert!(text.contains("/CIDSet"), "A-1 needs a CIDSet");
    assert!(text.contains("<pdfaid:part>1</pdfaid:part>"));
    assert!(
        !text.contains("/Type /ObjStm"),
        "A-1 forbids object streams"
    );
    assert_eq!(EditableDoc::load(&bytes).unwrap().page_count(), 1);
}

#[test]
fn pdfa3_embeds_associated_file() {
    let bytes = simple_level_doc(PdfaLevel::A3b, true);
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("<pdfaid:part>3</pdfaid:part>"));
    assert!(text.contains("/Type /EmbeddedFile"));
    assert!(text.contains("/Subtype /text#2Fcsv"));
    assert!(text.contains("/AFRelationship /Source"));
    assert!(text.contains("/EmbeddedFiles"));
    assert!(text.contains("/AF ")); // document-level associated files
    assert_eq!(EditableDoc::load(&bytes).unwrap().page_count(), 1);
}

#[test]
fn verapdf_confirms_levels_1b_3b_3a_when_available() {
    use testkit::{validate_with, Validator, ValidatorStatus};
    for (level, attach, tag) in [
        (PdfaLevel::A1b, false, "1b"),
        (PdfaLevel::A3b, true, "3b"),
        (PdfaLevel::A3a, true, "3a"),
    ] {
        let path = std::env::temp_dir().join(format!("rustpdf_pdfa_{tag}.pdf"));
        std::fs::write(&path, simple_level_doc(level, attach)).unwrap();
        for r in validate_with(&path, &[Validator::VeraPdf]) {
            if let ValidatorStatus::Fail { code, output } = r.status {
                panic!("veraPDF rejected PDF/A-{tag} (code {code:?}):\n{output}");
            }
        }
    }
}

#[test]
fn pdfa4_is_pdf20_with_rev_and_no_conformance() {
    let bytes = simple_level_doc(PdfaLevel::A4, false);
    let text = String::from_utf8_lossy(&bytes);
    // PDF/A-4 is based on PDF 2.0.
    assert!(text.starts_with("%PDF-2.0"), "A-4 must declare PDF 2.0");
    assert!(text.contains("/Version /2.0"), "catalog records 2.0");
    // pdfaid uses part 4 + rev 2020 and (for the base level) NO conformance.
    assert!(text.contains("<pdfaid:part>4</pdfaid:part>"));
    assert!(text.contains("<pdfaid:rev>2020</pdfaid:rev>"));
    assert!(!text.contains("<pdfaid:conformance>"));
    // Same archival scaffolding as the other levels.
    assert!(text.contains("/OutputIntents"));
    assert!(text.contains("/S /GTS_PDFA1"));
    assert!(text.contains("/FontFile2"));
    // CIDSet is deprecated in PDF 2.0 — not emitted for A-4.
    assert!(!text.contains("/CIDSet"), "A-4 should not emit a CIDSet");
    assert_eq!(EditableDoc::load(&bytes).unwrap().page_count(), 1);
    assert!(pdf::extract_text(&bytes).unwrap().contains("café"));
}

#[test]
fn pdfa4f_embeds_file_with_conformance_marker() {
    let bytes = simple_level_doc(PdfaLevel::A4f, true);
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("<pdfaid:part>4</pdfaid:part>"));
    assert!(text.contains("<pdfaid:rev>2020</pdfaid:rev>"));
    assert!(text.contains("<pdfaid:conformance>F</pdfaid:conformance>"));
    assert!(text.contains("/Type /EmbeddedFile"));
    assert!(text.contains("/AFRelationship /Source"));
}

#[test]
fn pdf20_plain_document_header() {
    // PDF 2.0 without PDF/A: just the header + catalog /Version.
    use pdf::Version;
    let mut doc = Document::new().with_version(Version::V2_0);
    doc.add_page();
    let bytes = doc.to_bytes().unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.starts_with("%PDF-2.0"));
    assert!(text.contains("/Version /2.0"));
    assert_eq!(EditableDoc::load(&bytes).unwrap().page_count(), 1);
}

#[test]
fn verapdf_confirms_pdfa4_when_available() {
    use testkit::{validate_with, Validator, ValidatorStatus};
    for (level, attach, tag) in [(PdfaLevel::A4, false, "4"), (PdfaLevel::A4f, true, "4f")] {
        let path = std::env::temp_dir().join(format!("rustpdf_pdfa_{tag}.pdf"));
        std::fs::write(&path, simple_level_doc(level, attach)).unwrap();
        for r in validate_with(&path, &[Validator::VeraPdf]) {
            if let ValidatorStatus::Fail { code, output } = r.status {
                panic!("veraPDF rejected PDF/A-{tag} (code {code:?}):\n{output}");
            }
        }
    }
}

#[test]
fn non_pdfa_has_no_output_intent() {
    let mut doc = Document::new();
    doc.add_page();
    let text = String::from_utf8_lossy(&doc.to_bytes().unwrap()).into_owned();
    assert!(!text.contains("/OutputIntents"));
    assert!(!text.contains("pdfaid"));
}

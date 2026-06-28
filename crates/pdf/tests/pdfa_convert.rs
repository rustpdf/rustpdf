//! Tier 2 (items 7 + 8): ZUGFeRD / Factur-X e-invoices and converting an
//! existing PDF to PDF/A. veraPDF conformance is asserted in the shell (the
//! sandbox can't spawn it); here we assert structure + round-trip parsing.

use pdf::{Document, EditableDoc, FacturxProfile, PdfaLevel};

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

const INVOICE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rsm:CrossIndustryInvoice xmlns:rsm="urn:un:unece:uncefact:data:standard:CrossIndustryInvoice:100">
  <rsm:ExchangedDocument><ram:ID>INV-2026-001</ram:ID></rsm:ExchangedDocument>
</rsm:CrossIndustryInvoice>"#;

#[test]
fn facturx_embeds_xml_and_marks_pdfa3() {
    lic();
    let mut doc = Document::new();
    let f = doc.add_font_file(FONT).unwrap();
    doc.add_page()
        .text(f, 18.0)
        .at(72.0, 760.0)
        .show("Invoice INV-2026-001");
    doc.facturx(INVOICE_XML.as_bytes().to_vec(), FacturxProfile::En16931);
    let bytes = doc.to_bytes().unwrap();
    let text = String::from_utf8_lossy(&bytes);

    // PDF/A-3 identifier + Factur-X XMP schema.
    assert!(text.contains("<pdfaid:part>3</pdfaid:part>"));
    assert!(text.contains("urn:factur-x:pdfa:CrossIndustryDocument:invoice:1p0#"));
    assert!(text.contains("<fx:DocumentFileName>factur-x.xml</fx:DocumentFileName>"));
    assert!(text.contains("<fx:ConformanceLevel>EN 16931</fx:ConformanceLevel>"));
    assert!(text.contains("Factur-X PDFA Extension Schema"));
    // Embedded file + association.
    assert!(text.contains("factur-x.xml"));
    assert!(text.contains("/AFRelationship /Alternative"));
    assert!(text.contains("/EmbeddedFile"));

    assert_eq!(EditableDoc::load(&bytes).unwrap().page_count(), 1);
}

#[test]
fn convert_existing_pdf_to_pdfa() {
    lic();
    // A plain document with an embedded subset font.
    let mut doc = Document::new();
    let f = doc.add_font_file(FONT).unwrap();
    doc.add_page()
        .text(f, 24.0)
        .at(72.0, 700.0)
        .show("Convert me to PDF/A");
    let plain = doc.to_bytes().unwrap();
    assert!(!String::from_utf8_lossy(&plain).contains("pdfaid:part"));

    let mut ed = EditableDoc::load(&plain).unwrap();
    ed.convert_to_pdfa(PdfaLevel::A2b).unwrap();
    let out = ed.to_bytes().unwrap();
    let text = String::from_utf8_lossy(&out);

    assert!(text.contains("<pdfaid:part>2</pdfaid:part>"));
    assert!(text.contains("<pdfaid:conformance>B</pdfaid:conformance>"));
    assert!(text.contains("/OutputIntent"));
    assert!(text.contains("GTS_PDFA1"));
    assert!(text.contains("/Metadata"));
    assert!(text.contains("/ID ["));
    // Original text preserved.
    assert!(pdf::extract_text(&out)
        .unwrap()
        .contains("Convert me to PDF/A"));
    assert_eq!(EditableDoc::load(&out).unwrap().page_count(), 1);
}

#[test]
fn convert_rejects_non_embedded_fonts() {
    lic();
    // Build a PDF whose only font is the standard-14 Helvetica (NOT embedded),
    // via the form path (AcroForm DR uses Helvetica), then strip pages of fonts.
    // Simpler: hand a doc that references Helvetica through a form field.
    let mut doc = Document::new();
    doc.add_page();
    doc.text_field("name", 0, [72.0, 700.0, 300.0, 720.0], "x", 12.0);
    let pdf = doc.to_bytes().unwrap();

    let mut ed = EditableDoc::load(&pdf).unwrap();
    match ed.convert_to_pdfa(PdfaLevel::A2b) {
        Err(pdf::ConvertError::FontsNotEmbedded(fonts)) => {
            assert!(fonts.iter().any(|f| f.contains("Helvetica")), "{fonts:?}");
        }
        other => panic!("expected FontsNotEmbedded, got {other:?}"),
    }
}

#[test]
fn convert_rejects_level_a() {
    lic();
    let mut doc = Document::new();
    let f = doc.add_font_file(FONT).unwrap();
    doc.add_page().text(f, 12.0).at(72.0, 700.0).show("hi");
    let mut ed = EditableDoc::load(doc.to_bytes().unwrap()).unwrap();
    assert!(matches!(
        ed.convert_to_pdfa(PdfaLevel::A2a),
        Err(pdf::ConvertError::TaggingRequired)
    ));
}

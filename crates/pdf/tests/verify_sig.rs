//! Tier 2 (item 5): signature validation. Sign a doc, then verify it; confirm a
//! tampered byte invalidates the digest. Cross-checked externally with `pdfsig`.

use pdf::{verify_signatures, Document, SignOptions, Signer};

const FX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
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

fn signer() -> Signer {
    let key = std::fs::read(format!("{FX}/signer_key.pk8")).unwrap();
    let cert = std::fs::read(format!("{FX}/signer_cert.der")).unwrap();
    Signer::from_pkcs8_der(&key, &cert).unwrap()
}

fn signed_pdf() -> Vec<u8> {
    lic();
    let mut doc = Document::new();
    let f = doc.add_font_file(FONT).unwrap();
    doc.add_page()
        .text(f, 20.0)
        .at(72.0, 700.0)
        .show("Documento assinado");
    let pdf = doc.to_bytes().unwrap();
    pdf::sign(&pdf, &signer(), &SignOptions::default()).unwrap()
}

#[test]
fn unsigned_pdf_has_no_reports() {
    lic(); // validation is a licensed (Enterprise) feature
    let mut doc = Document::new();
    doc.add_page();
    let reports = verify_signatures(doc.to_bytes().unwrap()).unwrap();
    assert!(reports.is_empty());
}

#[test]
fn valid_signature_verifies() {
    let pdf = signed_pdf();
    let reports = verify_signatures(&pdf).unwrap();
    assert_eq!(reports.len(), 1);
    let r = &reports[0];
    assert!(r.digest_valid, "digest should match: {r:?}");
    assert!(r.signature_valid, "CMS signature should verify: {r:?}");
    assert!(r.covers_whole_document, "single sig covers whole file");
    assert!(r.is_valid());
    assert!(r.signer.is_some());
    assert!(r.sub_filter.contains("pkcs7") || r.sub_filter.contains("CAdES"));
}

#[test]
fn tampering_breaks_the_digest() {
    let mut pdf = signed_pdf();
    // Flip a byte inside the first signed segment (the page content area).
    let r = &verify_signatures(&pdf).unwrap()[0];
    let pos = r.byte_range[1] / 2; // within segment 1
    pdf[pos] ^= 0xFF;
    let reports = verify_signatures(&pdf).unwrap();
    assert!(!reports[0].digest_valid, "tamper must break the digest");
    assert!(!reports[0].is_valid());
}

#[test]
fn pades_signature_verifies() {
    lic();
    let mut doc = Document::new();
    let f = doc.add_font_file(FONT).unwrap();
    doc.add_page().text(f, 18.0).at(72.0, 700.0).show("PAdES");
    let pdf = doc.to_bytes().unwrap();
    let opts = SignOptions {
        pades: true,
        ..Default::default()
    };
    let signed = pdf::sign(&pdf, &signer(), &opts).unwrap();
    let reports = verify_signatures(&signed).unwrap();
    assert_eq!(reports.len(), 1);
    assert!(
        reports[0].is_valid(),
        "PAdES sig should verify: {:?}",
        reports[0]
    );
    assert_eq!(reports[0].sub_filter, "ETSI.CAdES.detached");
}

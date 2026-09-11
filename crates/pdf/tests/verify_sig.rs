//! Tier 2 (item 5): signature validation. Sign a doc, then verify it; confirm a
//! tampered byte invalidates the digest. Cross-checked externally with `pdfsig`.

use pdf::{verify_signatures, Document, SignOptions, Signer};

const FX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
);

fn signer() -> Signer {
    let key = std::fs::read(format!("{FX}/signer_key.pk8")).unwrap();
    let cert = std::fs::read(format!("{FX}/signer_cert.der")).unwrap();
    Signer::from_pkcs8_der(&key, &cert).unwrap()
}

fn tsa() -> Signer {
    let key = std::fs::read(format!("{FX}/tsa_key.pk8")).unwrap();
    let cert = std::fs::read(format!("{FX}/tsa_cert.der")).unwrap();
    Signer::from_pkcs8_der(&key, &cert).unwrap()
}

fn signed_pdf() -> Vec<u8> {
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
fn report_exposes_rich_certificate_details() {
    // Issue #41 P1 #5: inspection beyond subject DN + integrity.
    let pdf = signed_pdf();
    let r = &verify_signatures(&pdf).unwrap()[0];
    assert!(r.signer.is_some(), "subject DN");
    assert!(r.issuer.is_some(), "issuer DN must be exposed");
    let serial = r.serial_number.as_deref().expect("serial number");
    assert!(
        serial.chars().all(|c| c.is_ascii_hexdigit()) && !serial.is_empty(),
        "serial is uppercase hex: {serial}"
    );
    let from = r.valid_from.as_deref().expect("validity start");
    let to = r.valid_to.as_deref().expect("validity end");
    assert!(from.len() == 20 && from.ends_with('Z'), "ISO-8601: {from}");
    assert!(to.len() == 20 && to.ends_with('Z'), "ISO-8601: {to}");
    assert_eq!(
        r.algorithm.as_deref(),
        Some("SHA256withRSA"),
        "this library signs RSA + SHA-256"
    );
    assert!(r.cert_count >= 1, "at least the signer cert is embedded");
}

#[test]
fn timestamp_flag_set_for_doctimestamp() {
    let signed = signed_pdf();
    let stamped = pdf::timestamp(&signed, &tsa(), None).unwrap();
    let ts = verify_signatures(&stamped)
        .unwrap()
        .into_iter()
        .find(|r| r.sub_filter == "ETSI.RFC3161")
        .unwrap();
    assert!(ts.has_timestamp, "a DocTimeStamp reports has_timestamp");
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

#[test]
fn document_timestamp_verifies() {
    // Regression: a DocTimeStamp (RFC 3161) commits to the covered bytes via the
    // TSTInfo messageImprint, not the CMS messageDigest. verify_signatures must
    // check the imprint, otherwise it falsely reports the timestamp as invalid.
    let signed = signed_pdf();
    let stamped = pdf::timestamp(&signed, &tsa(), None).unwrap();
    let reports = verify_signatures(&stamped).unwrap();

    let ts = reports
        .iter()
        .find(|r| r.sub_filter == "ETSI.RFC3161")
        .expect("a DocTimeStamp report");
    assert!(
        ts.signature_valid,
        "TSA CMS signature should verify: {ts:?}"
    );
    assert!(
        ts.digest_valid,
        "timestamp messageImprint must match the ByteRange digest: {ts:?}"
    );
    assert!(
        ts.covers_whole_document,
        "the timestamp covers the whole file"
    );
    assert!(
        ts.is_valid(),
        "the document timestamp should be valid: {ts:?}"
    );

    // The original signature is still present and intact under the timestamp.
    let sig = reports
        .iter()
        .find(|r| r.sub_filter != "ETSI.RFC3161")
        .expect("the original signature report");
    assert!(sig.digest_valid && sig.signature_valid, "{sig:?}");
}

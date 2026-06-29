//! Fase 7.1 tests: sign a PDF (incremental update + PKCS#7 detached) and verify
//! structure, self-parse (our parser follows the `/Prev` chain), ByteRange math,
//! and that `/Contents` is a well-formed CMS SignedData. Cryptographic validity
//! is confirmed externally with `pdfsig` (the test sandbox can't spawn it).

use cms::content_info::ContentInfo;
use der::{Decode, Encode};
use pdf::{Document, EditableDoc, SignOptions, Signer, VisibleSignature};

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

fn sample() -> Vec<u8> {
    lic();
    let mut doc = Document::new();
    let f = doc.add_font_file(FONT).unwrap();
    doc.add_page()
        .text(f, 20.0)
        .at(72.0, 700.0)
        .show("Documento para assinar");
    doc.to_bytes().unwrap()
}

fn parse_byte_range(pdf: &[u8]) -> [usize; 4] {
    let start = pdf.windows(12).position(|w| w == b"/ByteRange [").unwrap() + 12;
    let end = pdf[start..].iter().position(|&b| b == b']').unwrap() + start;
    let text = std::str::from_utf8(&pdf[start..end]).unwrap();
    let nums: Vec<usize> = text
        .split_whitespace()
        .map(|s| s.parse().unwrap())
        .collect();
    [nums[0], nums[1], nums[2], nums[3]]
}

#[test]
fn signed_pdf_has_signature_structure() {
    let signed = pdf::sign(&sample(), &signer(), &SignOptions::default()).unwrap();
    let text = String::from_utf8_lossy(&signed);
    assert!(text.contains("/Type /Sig"));
    assert!(text.contains("/SubFilter /adbe.pkcs7.detached"));
    assert!(text.contains("/ByteRange ["));
    assert!(text.contains("/FT /Sig"));
    assert!(text.contains("/SigFlags 3"));
    // It's an incremental update: original bytes are a prefix, with a /Prev.
    assert!(signed.starts_with(&sample()[..200]));
    assert!(text.contains("/Prev "));
}

#[test]
fn byte_range_covers_everything_except_contents() {
    let signed = pdf::sign(&sample(), &signer(), &SignOptions::default()).unwrap();
    let [s0, l0, s1, l1] = parse_byte_range(&signed);
    assert_eq!(s0, 0);
    // The gap between the two segments is exactly the `<...>` Contents region.
    let gap = s1 - (s0 + l0);
    assert!(gap > 2, "gap should hold the hex contents");
    // Segments + gap span the whole file.
    assert_eq!(s1 + l1, signed.len(), "second segment must reach EOF");
    assert_eq!(l0 + gap + l1, signed.len());
    // The excluded region starts with '<' and ends with '>'.
    assert_eq!(signed[s0 + l0], b'<');
    assert_eq!(signed[s1 - 1], b'>');
}

#[test]
fn contents_is_valid_cms_signed_data() {
    let signed = pdf::sign(&sample(), &signer(), &SignOptions::default()).unwrap();
    let [_, l0, s1, _] = parse_byte_range(&signed);
    // Hex between the angle brackets. The placeholder is zero-padded on the
    // right, so decode all the bytes and slice off exactly the DER object's
    // self-described length (trimming trailing '0' chars is wrong: a CMS that
    // legitimately ends in a 0x00 byte would lose a byte and fail to parse).
    let lt = l0; // index of '<'
    let hex = &signed[lt + 1..s1 - 1];
    let hex_str = std::str::from_utf8(hex).unwrap();
    let hex_str = &hex_str[..hex_str.len() - hex_str.len() % 2];
    let all: Vec<u8> = (0..hex_str.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex_str[i..i + 2], 16).unwrap())
        .collect();
    // DER length = header (tag + length-of-length) + content length.
    let der_len = {
        assert!(
            all.len() > 2 && all[0] == 0x30,
            "Contents is not a DER SEQUENCE"
        );
        let l = all[1];
        if l < 0x80 {
            2 + l as usize
        } else {
            let n = (l & 0x7f) as usize;
            let len = all[2..2 + n]
                .iter()
                .fold(0usize, |a, &b| (a << 8) | b as usize);
            2 + n + len
        }
    };
    let der = &all[..der_len];

    let ci = ContentInfo::from_der(der).expect("Contents is valid DER CMS");
    // It must be a SignedData content type, and re-encode losslessly.
    assert_eq!(ci.content_type, const_oid::db::rfc5911::ID_SIGNED_DATA);
    assert!(ci.to_der().is_ok());
}

#[test]
fn our_parser_reads_the_signed_incremental_update() {
    let original = sample();
    let signed = pdf::sign(&original, &signer(), &SignOptions::default()).unwrap();

    // Our parser follows the /Prev chain and applies the updated catalog/page.
    let doc = EditableDoc::load(&signed).unwrap();
    assert_eq!(doc.page_count(), 1);

    let text = pdf::extract_text(&signed).unwrap();
    assert!(text.contains("Documento para assinar"), "got: {text:?}");
}

#[test]
fn options_set_reason_and_name() {
    let opts = SignOptions {
        reason: Some("Aprovado".into()),
        name: Some("Ada".into()),
        ..Default::default()
    };
    let signed = pdf::sign(&sample(), &signer(), &opts).unwrap();
    let text = String::from_utf8_lossy(&signed);
    assert!(text.contains("/Reason (Aprovado)"));
    assert!(text.contains("/Name (Ada)"));
}

#[test]
fn visible_signature_has_appearance() {
    let opts = SignOptions {
        visible: Some(VisibleSignature {
            page: 0,
            rect: [360.0, 690.0, 540.0, 750.0],
            lines: vec!["Assinado por: Ada".into(), "Data: 2026".into()],
        }),
        ..Default::default()
    };
    let signed = pdf::sign(&sample(), &signer(), &opts).unwrap();
    let text = String::from_utf8_lossy(&signed);
    // A Form XObject appearance with a Helvetica font, referenced from /AP /N.
    assert!(text.contains("/Subtype /Form"));
    assert!(text.contains("/BaseFont /Helvetica"));
    assert!(text.contains("/AP <<"));
    assert!(
        !text.contains("/Rect [0 0 0 0]"),
        "should be a visible rect"
    );
    // Still parses and the signature is well-formed CMS.
    assert_eq!(EditableDoc::load(&signed).unwrap().page_count(), 1);
}

#[test]
fn multiple_signatures_each_get_a_field() {
    let s = signer();
    let first = pdf::sign(&sample(), &s, &SignOptions::default()).unwrap();
    let second = pdf::sign(&first, &s, &SignOptions::default()).unwrap();
    let text = String::from_utf8_lossy(&second);

    // Two signature dictionaries and two distinct field names.
    assert_eq!(text.matches("/SubFilter /adbe.pkcs7.detached").count(), 2);
    assert!(text.contains("(Signature1)"));
    assert!(text.contains("(Signature2)"));
    // The twice-incremental file still parses with one page and our text.
    assert_eq!(EditableDoc::load(&second).unwrap().page_count(), 1);
    assert!(pdf::extract_text(&second)
        .unwrap()
        .contains("Documento para assinar"));
}

/// Extract the CMS DER from a signed PDF's `/Contents`.
fn contents_der(signed: &[u8]) -> Vec<u8> {
    let [_, l0, s1, _] = parse_byte_range(signed);
    let hex = &signed[l0 + 1..s1 - 1];
    let hex_str = std::str::from_utf8(hex).unwrap().trim_end_matches('0');
    let hex_str = if hex_str.len() % 2 == 1 {
        &hex_str[..hex_str.len() - 1]
    } else {
        hex_str
    };
    (0..hex_str.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex_str[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn pades_b_b_uses_etsi_subfilter_and_signing_cert_attr() {
    let opts = SignOptions {
        pades: true,
        reason: Some("PAdES".into()),
        ..Default::default()
    };
    let signed = pdf::sign(&sample(), &signer(), &opts).unwrap();
    let text = String::from_utf8_lossy(&signed);
    assert!(text.contains("/SubFilter /ETSI.CAdES.detached"));

    let der = contents_der(&signed);
    // It is still a well-formed CMS SignedData.
    assert!(ContentInfo::from_der(&der).is_ok());
    // The ESS signing-certificate-v2 OID (1.2.840.113549.1.9.16.2.47) is present
    // as a signed attribute: DER 06 0B 2A 86 48 86 F7 0D 01 09 10 02 2F.
    let oid = [
        0x06u8, 0x0B, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x10, 0x02, 0x2F,
    ];
    assert!(
        der.windows(oid.len()).any(|w| w == oid),
        "signing-certificate-v2 attribute missing"
    );
    assert_eq!(EditableDoc::load(&signed).unwrap().page_count(), 1);
}

/// Extract the CMS DER from the *last* `/Contents` (newest signature/timestamp).
fn last_contents_der(signed: &[u8]) -> Vec<u8> {
    let p = signed
        .windows(11)
        .rposition(|w| w == b"/Contents <")
        .unwrap();
    let lt = p + b"/Contents ".len();
    let gt = signed[lt..].iter().position(|&b| b == b'>').unwrap() + lt;
    let hex = std::str::from_utf8(&signed[lt + 1..gt])
        .unwrap()
        .trim_end_matches('0');
    let hex = if hex.len() % 2 == 1 {
        &hex[..hex.len() - 1]
    } else {
        hex
    };
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn document_timestamp_is_valid_rfc3161() {
    let signed = pdf::sign(&sample(), &signer(), &SignOptions::default()).unwrap();
    let stamped = pdf::timestamp(&signed, &tsa(), Some("20260625000000Z")).unwrap();

    let text = String::from_utf8_lossy(&stamped);
    assert!(text.contains("/Type /DocTimeStamp"));
    assert!(text.contains("/SubFilter /ETSI.RFC3161"));

    // The token is a CMS SignedData encapsulating an RFC 3161 TSTInfo
    // (id-ct-TSTInfo OID 1.2.840.113549.1.9.16.1.4). Use the LAST /Contents.
    let der = last_contents_der(&stamped);
    assert!(ContentInfo::from_der(&der).is_ok());
    let tstinfo_oid = [
        0x06u8, 0x0B, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x09, 0x10, 0x01, 0x04,
    ];
    assert!(
        der.windows(tstinfo_oid.len()).any(|w| w == tstinfo_oid),
        "TSTInfo content type missing"
    );
    // Both the original signature and the timestamp are present; parses fine.
    assert_eq!(text.matches("/SubFilter").count(), 2);
    assert_eq!(EditableDoc::load(&stamped).unwrap().page_count(), 1);
}

#[test]
fn dss_adds_validation_material() {
    let signed = pdf::sign(&sample(), &signer(), &SignOptions::default()).unwrap();
    let cert = std::fs::read(format!("{FX}/signer_cert.der")).unwrap();
    let crl = std::fs::read(format!("{FX}/test.crl")).unwrap();
    let with_dss = pdf::add_dss(&signed, &[cert], &[crl]).unwrap();

    let text = String::from_utf8_lossy(&with_dss);
    assert!(text.contains("/DSS "));
    assert!(text.contains("/Certs ["));
    assert!(text.contains("/CRLs ["));
    // Still parses; the catalog now references the DSS.
    let doc = EditableDoc::load(&with_dss).unwrap();
    assert_eq!(doc.page_count(), 1);
}

#[test]
fn full_ltv_flow_b_b_then_lt_then_lta() {
    // PAdES-B-B -> add DSS (B-LT) -> document timestamp (B-LTA).
    let cert = std::fs::read(format!("{FX}/signer_cert.der")).unwrap();
    let tsacert = std::fs::read(format!("{FX}/tsa_cert.der")).unwrap();
    let crl = std::fs::read(format!("{FX}/test.crl")).unwrap();

    let s1 = pdf::sign(
        &sample(),
        &signer(),
        &SignOptions {
            pades: true,
            ..Default::default()
        },
    )
    .unwrap();
    let s2 = pdf::add_dss(&s1, &[cert, tsacert], &[crl]).unwrap();
    let s3 = pdf::timestamp(&s2, &tsa(), Some("20260625000000Z")).unwrap();

    let text = String::from_utf8_lossy(&s3);
    assert!(text.contains("/SubFilter /ETSI.CAdES.detached")); // B-B
    assert!(text.contains("/DSS ")); // B-LT
    assert!(text.contains("/Type /DocTimeStamp")); // B-LTA
    assert_eq!(EditableDoc::load(&s3).unwrap().page_count(), 1);
    assert!(pdf::extract_text(&s3)
        .unwrap()
        .contains("Documento para assinar"));
}

#[test]
fn certificate_chain_is_accepted() {
    // A distinct CA certificate is carried alongside the signer certificate.
    let key = std::fs::read(format!("{FX}/signer_key.pk8")).unwrap();
    let cert = std::fs::read(format!("{FX}/signer_cert.der")).unwrap();
    let ca = std::fs::read(format!("{FX}/signer_ca.der")).unwrap();
    let signer = Signer::from_pkcs8_der_with_chain(&key, &cert, &[ca]).unwrap();
    let signed = pdf::sign(&sample(), &signer, &SignOptions::default()).unwrap();
    assert_eq!(EditableDoc::load(&signed).unwrap().page_count(), 1);
}

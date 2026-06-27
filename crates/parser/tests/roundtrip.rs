//! Fase 5 end-to-end tests, using committed fixture PDFs (generated once with
//! qpdf from our own writer's output — see tests/fixtures/). The tests are
//! self-contained: round-trip validity is checked by re-parsing with our own
//! parser, so they run in any sandbox without external tools.

use cos::{Dict, Object, Reference};
use parser::PdfReader;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Decode the first page's content stream to text (proves filters + decryption).
fn first_page_content(reader: &PdfReader) -> String {
    let page = reader.pages().into_iter().next().expect("a page");
    let contents = page.get("Contents").expect("/Contents");
    match reader.resolve(contents) {
        Object::Stream(s) => String::from_utf8_lossy(&reader.stream_data(s).unwrap()).into_owned(),
        _ => String::new(),
    }
}

/// Rewrite a parsed document into a fresh PDF, preserving object numbers.
fn rewrite(reader: &PdfReader) -> Vec<u8> {
    let max = reader.object_numbers().max().unwrap_or(0);
    let mut w = writer::Document::new(writer::PdfVersion::V1_7);
    for _ in 0..max {
        w.reserve();
    }
    for n in 1..=max {
        w.assign(Reference::new(n), Object::Null);
    }
    for n in reader.object_numbers() {
        if let Some(obj) = reader.get(n) {
            w.assign(Reference::new(n), normalize(obj));
        }
    }
    if let Some(Object::Reference(r)) = reader.trailer().get("Root") {
        w.set_root(*r);
    }
    if let Some(Object::Reference(r)) = reader.trailer().get("Info") {
        w.set_info(*r);
    }
    w.write().expect("rewrite")
}

/// Drop `/Length` from stream dicts so the writer recomputes it (decryption can
/// change a stream's length, e.g. AES strips the IV and padding).
fn normalize(obj: &Object) -> Object {
    match obj {
        Object::Stream(s) => {
            let mut dict = Dict::new();
            for (k, v) in s.dict.iter() {
                if k.as_str() != "Length" {
                    dict.set(k.clone(), v.clone());
                }
            }
            Object::Stream(cos::Stream {
                dict,
                data: s.data.clone(),
            })
        }
        other => other.clone(),
    }
}

#[test]
fn parses_classic_xref() {
    let reader = PdfReader::parse(fixture("base.pdf")).unwrap();
    assert_eq!(reader.pages().len(), 2);
    assert!(reader.root().unwrap().contains_key("Pages"));
    assert!(first_page_content(&reader).contains("BT")); // has text
}

#[test]
fn parses_xref_streams_and_object_streams() {
    // modern.pdf uses a cross-reference stream (5.5) and object streams (5.6).
    let reader = PdfReader::parse(fixture("modern.pdf")).unwrap();
    assert_eq!(reader.pages().len(), 2, "object-stream pages not found");
    assert!(reader.root().is_ok());
    assert!(first_page_content(&reader).contains("BT"));
}

#[test]
fn roundtrip_reparses_consistently() {
    // Round-trip (5.10): parse → rewrite → parse again yields the same shape.
    for name in ["base.pdf", "modern.pdf"] {
        let r1 = PdfReader::parse(fixture(name)).unwrap();
        let rewritten = rewrite(&r1);
        assert!(rewritten.starts_with(b"%PDF-"));
        let r2 =
            PdfReader::parse(&rewritten).unwrap_or_else(|e| panic!("{name}: reparse failed: {e}"));
        assert_eq!(r1.pages().len(), r2.pages().len(), "{name}: page count");
        assert!(r2.root().is_ok(), "{name}: lost root");
    }
}

#[test]
fn recovers_from_corruption_without_panic() {
    let pdf = fixture("base.pdf");

    // Truncate trailer/xref off the end; the recovery scan should still work.
    let truncated = &pdf[..pdf.len() * 3 / 4];
    // A severely cut file may fail; if it parses, it should find something.
    if let Ok(reader) = PdfReader::parse(truncated) {
        assert!(reader.root().is_ok() || !reader.is_empty());
    }

    // Corrupt the startxref offset → recovery must kick in.
    let mut bad = pdf.clone();
    if let Some(pos) = bad.windows(9).rposition(|w| w == b"startxref") {
        if let Some(d) = bad.get_mut(pos + 11) {
            *d = b'9';
        }
    }
    let reader = PdfReader::parse(&bad).expect("recovery should parse");
    assert!(reader.root().is_ok());
    assert_eq!(reader.pages().len(), 2);
}

#[test]
fn reads_rc4_and_aes_encrypted() {
    // Empty user password; decryption must yield readable content streams.
    for name in [
        "enc_rc4_40.pdf",
        "enc_rc4_128.pdf",
        "enc_aes128.pdf",
        "enc_aes256.pdf",
    ] {
        let reader =
            PdfReader::parse(fixture(name)).unwrap_or_else(|e| panic!("{name}: parse failed: {e}"));
        assert_eq!(reader.pages().len(), 2, "{name}: pages");
        assert!(
            first_page_content(&reader).contains("BT"),
            "{name}: content not decrypted"
        );

        // Decrypted content round-trips to a parseable file.
        let reader2 = PdfReader::parse(rewrite(&reader)).unwrap();
        assert_eq!(reader2.pages().len(), 2, "{name}: round-trip pages");
    }
}

#[test]
fn aes256_r6_decrypts_with_empty_password() {
    // R6/AES-256 (V5): file key via Algorithm 2.A/2.B, streams via AESV3.
    let reader = PdfReader::parse(fixture("enc_aes256.pdf")).expect("R6 parse");
    assert_eq!(reader.pages().len(), 2);
    assert!(
        first_page_content(&reader).contains("BT"),
        "R6 content not decrypted"
    );
}

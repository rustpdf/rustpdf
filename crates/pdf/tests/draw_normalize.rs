//! Issue #41 P1: visible-signature image (#4), draw-over refinements (#7) and
//! normalization (#8). Outputs are re-parsed with our own parser (the sandbox
//! can't spawn qpdf); structural assertions confirm the features took effect.

use pdf::{
    extract_text, sign, Document, EditableDoc, PdfReader, SignOptions, Signer, Version,
    VisibleSignature, WatermarkOptions,
};

const FX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
const FONT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/Roboto-Regular.ttf"
);

fn base_doc() -> Vec<u8> {
    let mut doc = Document::new();
    let f = doc.add_font_file(FONT).unwrap();
    doc.add_page().text(f, 18.0).at(72.0, 700.0).show("Base");
    doc.to_bytes().unwrap()
}

/// A small RGB PNG (signature-like) generated with the `image` crate.
fn tiny_png() -> Vec<u8> {
    use image::{ImageEncoder, RgbImage};
    let img = RgbImage::from_fn(60, 30, |x, y| {
        image::Rgb([(x * 4) as u8, (y * 8) as u8, 120])
    });
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&img, 60, 30, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

fn signer() -> Signer {
    let key = std::fs::read(format!("{FX}/signer_key.pk8")).unwrap();
    let cert = std::fs::read(format!("{FX}/signer_cert.der")).unwrap();
    Signer::from_pkcs8_der(&key, &cert).unwrap()
}

#[test]
fn visible_signature_embeds_image() {
    let opts = SignOptions {
        visible: Some(VisibleSignature {
            page: 0,
            rect: [72.0, 600.0, 272.0, 680.0],
            lines: vec!["Assinado".into()],
            image: Some(tiny_png()),
        }),
        ..Default::default()
    };
    let signed = sign(&base_doc(), &signer(), &opts).unwrap();
    let reader = PdfReader::parse(&signed).unwrap();
    // An Image XObject (the signature image) must now exist in the file.
    let has_image = reader.object_numbers().any(|n| {
        matches!(reader.get(n), Some(cos::Object::Stream(s))
            if s.dict.get("Subtype").map(|o| matches!(o, cos::Object::Name(name) if name.as_str() == "Image")).unwrap_or(false))
    });
    assert!(has_image, "visible signature must embed an Image XObject");
}

#[test]
fn page_dimensions_account_for_rotation() {
    let mut ed = EditableDoc::load(base_doc()).unwrap();
    let (w0, h0) = ed.page_dimensions(0);
    assert!(h0 > w0, "portrait base page");
    ed.rotate_page(0, 90);
    let (w1, h1) = ed.page_dimensions(0);
    assert!(
        (w1 - h0).abs() < 0.01 && (h1 - w0).abs() < 0.01,
        "90° swaps w/h"
    );
}

#[test]
fn opaque_watermark_emits_white_fill() {
    let mut ed = EditableDoc::load(base_doc()).unwrap();
    ed.watermark_text(
        "PAID",
        WatermarkOptions {
            opaque_background: true,
            ..Default::default()
        },
    );
    let bytes = ed.to_bytes().unwrap();
    // The opaque white-out box ("1 1 1 rg ... re f") must be present.
    let body = String::from_utf8_lossy(&bytes);
    assert!(
        body.contains("1 1 1 rg") || bytes.windows(8).any(|w| w == b"1 1 1 rg"),
        "opaque watermark draws a white fill"
    );
    // The watermark text is still extractable.
    assert!(extract_text(&bytes).unwrap().contains("PAID"));
}

#[test]
fn normalize_downgrades_and_strips_pdfa() {
    // Build a PDF/A-2b document (carries OutputIntents + XMP pdfaid + 2.0? no, A2b is 1.7-era).
    let mut doc = Document::new();
    let f = doc.add_font_file(FONT).unwrap();
    doc.add_page().text(f, 18.0).at(72.0, 700.0).show("A");
    let pdfa = doc.pdfa().to_bytes().unwrap();

    let mut ed = EditableDoc::load(&pdfa).unwrap();
    ed.normalize(Version::V1_7);
    let out = ed.to_bytes().unwrap();

    // Header is 1.7.
    assert!(out.starts_with(b"%PDF-1.7"), "downgraded header");
    // Catalog no longer advertises OutputIntents / Metadata.
    let reader = PdfReader::parse(&out).unwrap();
    let root = reader.trailer().get("Root").unwrap();
    if let cos::Object::Dict(cat) = reader.resolve(root) {
        assert!(
            !cat.contains_key("OutputIntents"),
            "PDF/A OutputIntents stripped"
        );
        assert!(!cat.contains_key("Metadata"), "PDF/A XMP metadata stripped");
    } else {
        panic!("no catalog");
    }
}

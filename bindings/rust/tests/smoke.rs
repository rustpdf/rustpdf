//! Full-surface smoke test against the precompiled `libpdf_ffi`.
//!
//! Run with the engine cdylib resolvable. `make rust-test` builds it and points
//! `RUSTPDF_LIB` at `target/debug/libpdf_ffi.dylib`; this test also sets it from
//! `CARGO_MANIFEST_DIR` as a fallback so `cargo test` works from the repo.

use std::path::PathBuf;
use std::sync::Once;

use rustpdf::{
    Align, Bookmark, Document, EditableDoc, Encryption, FacturxProfile, ImageAnchor, PdfVersion,
    PdfaLevel, SigningOptions, StampSpace, VerticalAlign, VerticalAnchor,
};

static INIT: Once = Once::new();

/// Point RUSTPDF_LIB at the workspace's debug cdylib if not already set.
fn setup() {
    INIT.call_once(|| {
        if std::env::var_os("RUSTPDF_LIB").is_some() {
            return;
        }
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest.parent().unwrap().parent().unwrap();
        for name in ["libpdf_ffi.dylib", "libpdf_ffi.so", "pdf_ffi.dll"] {
            let p = repo_root.join("target").join("debug").join(name);
            if p.is_file() {
                std::env::set_var("RUSTPDF_LIB", &p);
                break;
            }
        }
    });
}

#[test]
fn full_surface() {
    setup();

    // Library loads and reports a version.
    let version = rustpdf::ensure_loaded().expect("cdylib should load");
    assert!(!version.is_empty(), "version string should be non-empty");

    // --- author a document exercising graphics, text and a paragraph ---
    let mut doc = Document::new().expect("new document");
    doc.set_info(
        Some("Smoke Test"),
        Some("rustpdf"),
        None,
        None,
        Some("binding"),
    )
    .unwrap();
    doc.add_page().unwrap();
    doc.set_fill_rgb(0.1, 0.2, 0.8)
        .unwrap()
        .rect(72.0, 700.0, 200.0, 60.0)
        .unwrap()
        .fill()
        .unwrap();

    let font = doc
        .add_font_file("../../assets/fonts/Roboto-Regular.ttf")
        .expect("embed Roboto");
    doc.show_text(font, 24.0, 72.0, 650.0, "Hello from Rust", 1)
        .unwrap();
    doc.paragraph(
        font,
        12.0,
        72.0,
        600.0,
        400.0,
        Align::Justify,
        "A wrapping paragraph laid out by the engine through the C ABI.",
    )
    .unwrap();

    assert_eq!(doc.page_count(), 1);

    let bytes = doc.write().expect("serialize document");
    assert!(bytes.starts_with(b"%PDF-"), "output should be a PDF");

    // --- round-trip through the editable / extraction surface ---
    let mut ed = EditableDoc::load(&bytes).expect("load editable");
    assert_eq!(ed.page_count(), 1);
    ed.set_info("Producer", "rustpdf-smoke").unwrap();
    let producer = ed.get_info("Producer").unwrap();
    assert_eq!(producer, "rustpdf-smoke");

    // Merge with a copy → two pages.
    let other = EditableDoc::load(&bytes).expect("load second editable");
    ed.merge(&other).unwrap();
    assert_eq!(ed.page_count(), 2);

    // Extract just the first page into a new doc.
    let extracted = ed.extract_pages(&[0]).expect("extract pages");
    assert_eq!(extracted.page_count(), 1);

    let out = ed.to_bytes().expect("serialize editable");
    assert!(out.starts_with(b"%PDF-"));

    // Text extraction returns a string (content may be empty without a font).
    let _text = rustpdf::extract_text(&bytes).expect("extract text");

    // Image extraction writes any raster images into a temp dir.
    let img_dir = std::env::temp_dir().join("rustpdf_smoke_images");
    std::fs::create_dir_all(&img_dir).expect("create image dir");
    let _count =
        rustpdf::extract_images_to_dir(&bytes, img_dir.to_str().unwrap()).expect("extract images");

    // Page rendering.
    assert_eq!(rustpdf::page_count(&bytes).expect("page count"), 1);
    let png = rustpdf::render_page_to_png(&bytes, 0, 72.0).expect("render page");
    assert!(
        png.len() > 8 && &png[1..4] == b"PNG",
        "expected a PNG header"
    );

    // --- encryption path ---
    let mut enc = EditableDoc::load(&bytes).expect("load for encrypt");
    enc.encrypt(Encryption::Aes256, "user", "owner", false)
        .unwrap();
    let encrypted = enc.to_bytes().expect("encrypt to bytes");
    assert!(encrypted.starts_with(b"%PDF-"));

    // --- PDF/A tagging path on a fresh doc ---
    let mut pdfa = Document::new().unwrap();
    pdfa.pdfa_level(PdfaLevel::A2b).unwrap();
    pdfa.add_page().unwrap();
    let pdfa_bytes = pdfa.write().expect("write PDF/A");
    assert!(pdfa_bytes.starts_with(b"%PDF-"));

    // --- Tier 1/2 authoring: links + bookmarks (Document) ---
    let mut nav = Document::new().unwrap();
    nav.add_page().unwrap();
    nav.add_page().unwrap();
    nav.link_uri([72.0, 700.0, 272.0, 720.0], "https://example.com")
        .unwrap()
        .link_to_page([72.0, 660.0, 272.0, 680.0], 1, Some(700.0))
        .unwrap()
        .link_to_page([72.0, 620.0, 272.0, 640.0], 0, None)
        .unwrap();
    let outline = Bookmark::new("Chapter 1", 0)
        .with_top(740.0)
        .child(Bookmark::new("Section 1.1", 0).with_top(600.0))
        .child(Bookmark::new("Section 1.2", 1));
    nav.add_bookmark(&outline).unwrap();
    nav.add_bookmark(&Bookmark::new("Chapter 2", 1)).unwrap();
    let nav_bytes = nav.write().expect("write nav doc");
    assert!(nav_bytes.starts_with(b"%PDF-"));

    // --- Factur-X / ZUGFeRD embedding (PDF/A-3) ---
    let mut invoice = Document::new().unwrap();
    invoice.add_page().unwrap();
    let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<rsm:CrossIndustryInvoice xmlns:rsm="urn:invoice"><Doc>1</Doc></rsm:CrossIndustryInvoice>"#;
    invoice.facturx(xml, FacturxProfile::En16931).unwrap();
    let invoice_bytes = invoice.write().expect("write facturx");
    assert!(invoice_bytes.starts_with(b"%PDF-"));

    // --- forms: build a doc with fields, then fill / flatten via EditableDoc ---
    let mut form = Document::new().unwrap();
    form.add_page().unwrap();
    form.text_field("name", 0, [72.0, 700.0, 272.0, 720.0], "", 12.0)
        .unwrap();
    form.checkbox("agree", 0, [72.0, 660.0, 90.0, 678.0], false)
        .unwrap();
    form.radio_group(
        "color",
        0,
        &[
            ([72.0, 620.0, 90.0, 638.0], "red"),
            ([100.0, 620.0, 118.0, 638.0], "green"),
        ],
        None,
    )
    .unwrap();
    form.dropdown(
        "size",
        0,
        [72.0, 580.0, 200.0, 598.0],
        &["S", "M", "L"],
        Some(0),
        12.0,
    )
    .unwrap();
    let form_bytes = form.write().expect("write form");

    let mut ed_form = EditableDoc::load(&form_bytes).expect("load form");
    let names = ed_form.field_names().expect("field names");
    assert!(
        names.iter().any(|n| n == "name"),
        "expected 'name' field in {names:?}"
    );
    assert!(ed_form.fill_text_field("name", "Ada").unwrap());
    assert!(ed_form.set_checkbox("agree", true).unwrap());
    assert!(ed_form.set_radio("color", "green").unwrap());
    assert!(ed_form.set_choice("size", "L").unwrap());
    // Unknown fields report not-found rather than erroring.
    assert!(!ed_form.set_checkbox("nope", true).unwrap());
    ed_form.flatten_forms().unwrap();
    let flattened = ed_form.to_bytes().expect("flatten to bytes");
    assert!(flattened.starts_with(b"%PDF-"));

    // --- watermarks + redaction + PDF/A conversion (EditableDoc) ---
    let mut wm = EditableDoc::load(&bytes).expect("load for watermark");
    wm.watermark_text("CONFIDENTIAL", 64.0, (0.5, 0.5, 0.5), 0.30, 45.0, false)
        .unwrap();

    let png: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8,
        0xcf, 0xc0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let png_path = std::env::temp_dir().join("rustpdf_smoke_wm.png");
    std::fs::write(&png_path, png).expect("write png");
    wm.watermark_image_file(png_path.to_str().unwrap(), 100.0, 100.0, 0.30, 0.0)
        .unwrap();

    // Redact a region on the first page; non-existent pages report false.
    assert!(wm.redact(0, &[[72.0, 700.0, 200.0, 720.0]]).unwrap());
    assert!(!wm.redact(999, &[[0.0, 0.0, 10.0, 10.0]]).unwrap());
    let wm_bytes = wm.to_bytes().expect("watermark to bytes");
    assert!(wm_bytes.starts_with(b"%PDF-"));

    let mut conv = EditableDoc::load(&bytes).expect("load for pdfa convert");
    conv.convert_to_pdfa(PdfaLevel::A2b).unwrap();
    let conv_bytes = conv.to_bytes().expect("convert_to_pdfa to bytes");
    assert!(conv_bytes.starts_with(b"%PDF-"));

    // --- signature verification on a freshly-signed doc ---
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("crates/pdf/tests/fixtures");
    let key = std::fs::read(fixtures.join("signer_key.pk8")).expect("read signer key");
    let cert = std::fs::read(fixtures.join("signer_cert.der")).expect("read signer cert");

    // Unsigned document → no reports.
    assert!(rustpdf::verify_signatures(&bytes).unwrap().is_empty());

    let signed = rustpdf::sign(
        &bytes,
        &key,
        &cert,
        Some("smoke test"),
        Some("here"),
        Some("Tester"),
        false,
    )
    .expect("sign document");
    let reports = rustpdf::verify_signatures(&signed).expect("verify signatures");
    assert_eq!(reports.len(), 1, "expected exactly one signature");
    let rep = &reports[0];
    assert!(!rep.sub_filter.is_empty(), "sub_filter should be set");
    assert_ne!(rep.byte_range, [0, 0, 0, 0], "byte_range should be filled");

    // --- rich signature fields (issue #41 P1) are accessible ---
    // A plain PKCS#7 signature carries the signer cert but no embedded timestamp.
    assert!(
        rep.cert_count >= 1,
        "expected at least the signer certificate"
    );
    assert!(
        !rep.has_timestamp,
        "plain signature has no embedded timestamp"
    );
    let _rich = (
        &rep.issuer,
        &rep.serial_number,
        &rep.valid_from,
        &rep.valid_to,
        &rep.algorithm,
        &rep.signing_time,
    );

    // --- pre-signing inventory (list_signatures) ---
    assert!(
        rustpdf::list_signatures(&bytes).unwrap().is_empty(),
        "unsigned document has no signature fields"
    );
    let fields = rustpdf::list_signatures(&signed).expect("list signatures");
    assert_eq!(fields.len(), 1, "signed document has one signature field");
    assert!(fields[0].signed, "the field should be reported as signed");

    // --- positional text search (find_text, issue #41 P1) ---
    let hits = rustpdf::find_text(&bytes, "Hello", false).expect("find_text");
    assert!(!hits.is_empty(), "expected at least one match for 'Hello'");
    let hit = &hits[0];
    assert!(
        hit.width > 0.0 && hit.height > 0.0,
        "match should carry a non-empty bounding box: {hit:?}"
    );

    // --- normalization (set_version / strip_pdfa / normalize, issue #41 P1) ---
    let mut norm = EditableDoc::load(&pdfa_bytes).expect("load for normalize");
    norm.set_version(PdfVersion::V1_7).unwrap();
    let v17 = norm.to_bytes().expect("set_version to bytes");
    assert!(v17.starts_with(b"%PDF-"));
    let mut norm2 = EditableDoc::load(&pdfa_bytes).expect("load for normalize 2");
    norm2.normalize(PdfVersion::V2_0).unwrap();
    let normalized = norm2.to_bytes().expect("normalize to bytes");
    assert!(
        normalized.starts_with(b"%PDF-2.0"),
        "PDF 2.0 header expected"
    );

    // --- page geometry (measure_pages / measure_page, issue #45 P1) ---
    let geos = rustpdf::measure_pages(&bytes).expect("measure_pages");
    assert_eq!(geos.len(), 1, "one page in the authored doc");
    let g0 = &geos[0];
    assert!(
        g0.width > 0.0 && g0.height > 0.0,
        "page should have a non-empty size: {g0:?}"
    );
    assert_eq!(g0.rotation, 0, "unrotated page");
    assert!(
        (g0.media_box.width() - g0.width).abs() < 0.01,
        "media box width should match page width: {g0:?}"
    );
    // measure_page targets a single index.
    let single = rustpdf::measure_page(&bytes, 0).expect("measure_page");
    assert_eq!(single, *g0);
    assert!(
        rustpdf::measure_page(&bytes, 99).is_err(),
        "out-of-range page should error"
    );

    // Rotating a page swaps the rotated dimensions while size stays unrotated.
    let mut rot = EditableDoc::load(&bytes).expect("load for rotate");
    rot.rotate_page(0, 90).unwrap();
    let rot_bytes = rot.to_bytes().expect("rotate to bytes");
    let rg = rustpdf::measure_page(&rot_bytes, 0).expect("measure rotated page");
    assert_eq!(rg.rotation, 90, "page should report 90° rotation");
    assert!(
        (rg.rotated_width - rg.height).abs() < 0.01 && (rg.rotated_height - rg.width).abs() < 0.01,
        "rotated dimensions should be swapped: {rg:?}"
    );

    // --- inspect (issue #45 P1) ---
    let overview = rustpdf::inspect(&bytes).expect("inspect");
    assert_eq!(overview.page_count, 1, "one page reported");
    assert!(!overview.encrypted, "plain doc is not encrypted");
    assert!(!overview.requires_password, "plain doc needs no password");
    assert!(!overview.version.is_empty(), "version string set");
    let enc_overview = rustpdf::inspect(&encrypted).expect("inspect encrypted");
    assert!(enc_overview.encrypted, "encrypted doc reports encrypted");

    // --- fill_rect + place_text (issue #45 P1) ---
    let mut paint = EditableDoc::load(&bytes).expect("load for paint");
    assert!(
        paint
            .fill_rect(0, 72.0, 500.0, 200.0, 40.0, (0.9, 0.9, 0.2), 0.5)
            .unwrap(),
        "fill_rect should report the page existed"
    );
    assert!(
        paint
            .place_text(0, 80.0, 510.0, "PLACED_MARKER", 18.0, (0.0, 0.0, 0.0), 0.0)
            .unwrap(),
        "place_text should report the page existed"
    );
    // Out-of-range pages report not-found rather than erroring.
    assert!(!paint
        .fill_rect(99, 0.0, 0.0, 10.0, 10.0, (0.0, 0.0, 0.0), 1.0)
        .unwrap());
    assert!(!paint
        .place_text(99, 0.0, 0.0, "x", 12.0, (0.0, 0.0, 0.0), 0.0)
        .unwrap());
    // --- draw_image onto an existing page (issue #50) ---
    assert!(
        paint
            .draw_image(0, png, 72.0, 200.0, 64.0, 64.0, 0.0)
            .unwrap(),
        "draw_image should report the page existed"
    );
    assert!(!paint
        .draw_image(99, png, 0.0, 0.0, 10.0, 10.0, 0.0)
        .unwrap());
    // --- place_text_aligned + masked_text (ForSign integration) ---
    assert!(
        paint
            .place_text_aligned(
                0,
                300.0,
                480.0,
                "ALIGNED_MARKER",
                14.0,
                (0.0, 0.0, 0.0),
                0.0,
                Align::Center,
            )
            .unwrap(),
        "place_text_aligned should report the page existed"
    );
    assert!(
        paint
            .masked_text(
                0,
                72.0,
                440.0,
                200.0,
                24.0,
                "MASKED_MARKER",
                12.0,
                (0.0, 0.0, 0.0),
                (1.0, 1.0, 1.0),
                Align::Left,
            )
            .unwrap(),
        "masked_text should report the page existed"
    );
    assert!(!paint
        .place_text_aligned(99, 0.0, 0.0, "x", 12.0, (0.0, 0.0, 0.0), 0.0, Align::Right)
        .unwrap());
    assert!(!paint
        .masked_text(
            99,
            0.0,
            0.0,
            10.0,
            10.0,
            "x",
            12.0,
            (0.0, 0.0, 0.0),
            (1.0, 1.0, 1.0),
            Align::Left,
        )
        .unwrap());

    // --- anchored stamping + embedded fonts (issue #54) ---
    let roboto = paint
        .add_font_file("../../assets/fonts/Roboto-Regular.ttf")
        .expect("register stamping font");
    assert!(
        paint
            .place_text_anchored(
                0,
                72.0,
                400.0,
                "ANCHORED_MARKER",
                14.0,
                (0.0, 0.0, 0.0),
                0.0,
                Align::Left,
                VerticalAnchor::LineBottom,
                Some(roboto),
            )
            .unwrap(),
        "place_text_anchored should report the page existed"
    );
    assert!(
        paint
            .masked_text_padded(
                0,
                72.0,
                360.0,
                200.0,
                24.0,
                "PADDED_MARKER",
                12.0,
                (0.0, 0.0, 0.0),
                (1.0, 1.0, 1.0),
                Align::Left,
                VerticalAlign::Top,
                0.0,
                None,
            )
            .unwrap(),
        "masked_text_padded should report the page existed"
    );
    // A narrow column forces the paragraph to wrap onto several lines.
    let (lines, height, found) = paint
        .place_paragraph(
            0,
            72.0,
            330.0,
            120.0,
            "WRAPPED words flowing across a deliberately narrow column",
            12.0,
            (0.0, 0.0, 0.0),
            Align::Left,
            VerticalAnchor::Top,
            None,
            0.0,
            0.0,
            0.0,
        )
        .expect("place_paragraph");
    assert!(found, "place_paragraph should report the page existed");
    assert!(
        lines > 1,
        "narrow paragraph should wrap, got {lines} line(s)"
    );
    assert!(height > 0.0, "paragraph should report its height: {height}");
    // Media-space stamping: switch the coordinate space, then place.
    paint.set_stamp_space(StampSpace::Media).unwrap();
    assert!(
        paint
            .place_text(0, 72.0, 160.0, "MEDIA_MARKER", 12.0, (0.0, 0.0, 0.0), 0.0)
            .unwrap(),
        "media-space place_text should report the page existed"
    );
    paint.set_stamp_space(StampSpace::Visible).unwrap();
    // Rotated image anchored by its bounding box.
    assert!(
        paint
            .draw_image_anchored(
                0,
                png,
                72.0,
                100.0,
                64.0,
                64.0,
                30.0,
                ImageAnchor::BoundingBox
            )
            .unwrap(),
        "draw_image_anchored should report the page existed"
    );
    assert!(!paint
        .draw_image_anchored(99, png, 0.0, 0.0, 10.0, 10.0, 0.0, ImageAnchor::Corner)
        .unwrap());

    let painted = paint.to_bytes().expect("paint to bytes");
    let painted_text = rustpdf::extract_text(&painted).expect("extract painted text");
    assert!(
        painted_text.contains("PLACED_MARKER"),
        "placed text should be extractable, got: {painted_text:?}"
    );
    assert!(
        painted_text.contains("ALIGNED_MARKER"),
        "aligned text should be extractable, got: {painted_text:?}"
    );
    assert!(
        painted_text.contains("MASKED_MARKER"),
        "masked text should be extractable, got: {painted_text:?}"
    );
    assert!(
        painted_text.contains("ANCHORED_MARKER"),
        "anchored (embedded-font) text should be extractable, got: {painted_text:?}"
    );
    assert!(
        painted_text.contains("PADDED_MARKER"),
        "padded masked text should be extractable, got: {painted_text:?}"
    );
    assert!(
        painted_text.contains("WRAPPED"),
        "wrapped paragraph text should be extractable, got: {painted_text:?}"
    );
    assert!(
        painted_text.contains("MEDIA_MARKER"),
        "media-space text should be extractable, got: {painted_text:?}"
    );

    // --- extract_page_text (single page) ---
    let page0 = rustpdf::extract_page_text(&painted, 0).expect("extract page 0 text");
    assert!(
        page0.contains("PLACED_MARKER"),
        "page-0 text should contain the placed marker, got: {page0:?}"
    );
    // Out-of-range page is an error, not a panic.
    assert!(
        rustpdf::extract_page_text(&painted, 999).is_err(),
        "out-of-range page should error"
    );

    // --- deferred signing: Model A callback wiring + visible appearance ---
    // Model A: the closure receives the to-be-signed bytes (the key never
    // crosses the FFI line). We return a placeholder container; we only assert
    // the callback fired, proving the trampoline plumbing works.
    let mut called = false;
    let _ = rustpdf::sign_with(&bytes, &cert, &[], None, |tbs| {
        called = true;
        assert!(!tbs.is_empty(), "callback should receive bytes to sign");
        Ok(vec![0u8; 256])
    });
    assert!(called, "sign_with must invoke the hash callback");

    // Model B with a visible-signature appearance (exercises the extended
    // PdfSigningOptions struct, incl. the appended visible_* fields).
    let opts = SigningOptions {
        reason: Some("smoke".into()),
        visible: true,
        visible_page: 0,
        visible_rect: [72.0, 72.0, 272.0, 144.0],
        visible_text: Some("Signed by\nrustpdf".into()),
        ..Default::default()
    };
    let session = rustpdf::begin_signing(&bytes, Some(&opts)).expect("begin_signing");
    assert!(
        session.document.starts_with(b"%PDF-"),
        "prepared document should be a PDF"
    );
    assert!(
        !session.to_be_signed.is_empty(),
        "to-be-signed bytes should be present"
    );

    // --- network TSA (AD-RT) helpers (issue #41 P1) ---
    let (ts_doc, ts_tbs) = rustpdf::begin_timestamp(&signed).expect("begin_timestamp");
    assert!(ts_doc.starts_with(b"%PDF-"));
    assert!(!ts_tbs.is_empty());
    // A 32-byte imprint stands in for SHA-256(ts_tbs); the request is DER.
    let request = rustpdf::timestamp_request(&[0u8; 32], None, true).expect("timestamp_request");
    assert_eq!(request.first(), Some(&0x30), "TimeStampReq should be DER");
}

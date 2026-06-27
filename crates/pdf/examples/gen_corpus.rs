//! Generate the synthetic golden corpus and its catalog (Fase 0.3).
//!
//! Run with: `cargo run -p pdf --example gen_corpus`
//!
//! This seeds `corpus/` with PDFs the writer can produce *today* (blank pages,
//! multi-page, every vector-graphics feature, varied sizes, deliberately
//! corrupted files) and writes `corpus/catalog.json`. Categories that depend on
//! later phases (embedded fonts, AcroForms, encrypted, CJK) are catalogued as
//! `pending` entries with `file: null`, to be filled in when those phases land.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use image::{ImageEncoder, Rgb, RgbImage, Rgba, RgbaImage};
use pdf::{sizes, Align, Document, Matrix, Paragraph};

struct Entry {
    id: String,
    category: String,
    description: String,
    file: Option<String>,
    features: Vec<&'static str>,
    expect_valid: bool,
    status: &'static str,
}

/// A named vector-graphics case: (id, description, builder).
type GfxCase = (&'static str, &'static str, fn(&mut Document));

fn main() {
    // The corpus includes PDF/A and encrypted samples (corporate features), so
    // activate the committed dev license first.
    let dev_license =
        std::fs::read_to_string(workspace_root().join("crates/license/fixtures/dev_license.txt"))
            .expect("dev license fixture");
    pdf::activate_license(dev_license.trim()).expect("activate dev license");

    let root = workspace_root().join("corpus");
    std::fs::create_dir_all(root.join("generated")).unwrap();
    std::fs::create_dir_all(root.join("corrupted")).unwrap();

    let mut entries: Vec<Entry> = Vec::new();

    // --- blank pages of various sizes ------------------------------------
    for (name, size) in [
        ("a4", sizes::A4),
        ("a3", sizes::A3),
        ("letter", sizes::LETTER),
        ("legal", sizes::LEGAL),
        ("tiny", (72.0, 72.0)),
        ("wide", (1684.0, 595.0)),
        ("a5", (419.528, 595.276)),
        ("square", (500.0, 500.0)),
    ] {
        let mut doc = Document::new().with_default_size(size);
        doc.add_page();
        let rel = format!("generated/blank_{name}.pdf");
        doc.save(root.join(&rel)).unwrap();
        entries.push(Entry {
            id: format!("blank-{name}"),
            category: "simple".into(),
            description: format!("blank {name} page"),
            file: Some(rel),
            features: vec!["page-tree", "media-box"],
            expect_valid: true,
            status: "generated",
        });
    }

    // --- multipage --------------------------------------------------------
    for n in [2usize, 3, 5, 10] {
        let mut doc = Document::new();
        for _ in 0..n {
            doc.add_page();
        }
        let rel = format!("generated/multipage_{n}.pdf");
        doc.save(root.join(&rel)).unwrap();
        entries.push(Entry {
            id: format!("multipage-{n}"),
            category: "simple".into(),
            description: format!("{n}-page document"),
            file: Some(rel),
            features: vec!["page-tree", "kids", "count"],
            expect_valid: true,
            status: "generated",
        });
    }

    // --- vector graphics features ----------------------------------------
    let graphics: Vec<GfxCase> = vec![
        ("rgb_fill", "RGB filled rectangle", |d| {
            d.add_page()
                .content()
                .set_fill_rgb(0.86, 0.2, 0.18)
                .rect(72.0, 600.0, 200.0, 120.0)
                .fill();
        }),
        ("rgb_stroke", "RGB stroked rectangle, thick line", |d| {
            d.add_page()
                .content()
                .set_stroke_rgb(0.1, 0.3, 0.8)
                .set_line_width(4.0)
                .rect(72.0, 600.0, 200.0, 120.0)
                .stroke();
        }),
        ("gray", "DeviceGray fill", |d| {
            d.add_page()
                .content()
                .set_fill_gray(0.4)
                .rect(72.0, 600.0, 200.0, 120.0)
                .fill();
        }),
        ("cmyk_curve", "CMYK Bézier curve", |d| {
            d.add_page()
                .content()
                .set_stroke_cmyk(0.6, 0.0, 0.8, 0.0)
                .set_line_width(3.0)
                .move_to(72.0, 500.0)
                .curve_to(180.0, 600.0, 360.0, 400.0, 500.0, 500.0)
                .stroke();
        }),
        ("even_odd", "even-odd filled donut", |d| {
            d.add_page()
                .content()
                .set_fill_rgb(0.95, 0.6, 0.1)
                .rect(72.0, 400.0, 160.0, 160.0)
                .rect(112.0, 440.0, 80.0, 80.0)
                .fill_even_odd();
        }),
        ("clip", "clipped fill region", |d| {
            let c = d.add_page().content();
            c.rect(100.0, 500.0, 120.0, 120.0)
                .clip()
                .end_path()
                .set_fill_rgb(0.2, 0.7, 0.3)
                .rect(0.0, 400.0, 400.0, 400.0)
                .fill();
        }),
        ("ctm_nested", "nested CTM transforms", |d| {
            let c = d.add_page().content();
            c.save_state()
                .concat_matrix(Matrix::translate(100.0, 400.0))
                .concat_matrix(Matrix::scale(2.0, 1.5))
                .set_fill_rgb(0.3, 0.3, 0.9)
                .rect(0.0, 0.0, 80.0, 80.0)
                .fill()
                .restore_state();
        }),
        ("fill_and_stroke", "fill then stroke", |d| {
            d.add_page()
                .content()
                .set_fill_rgb(1.0, 0.9, 0.2)
                .set_stroke_rgb(0.0, 0.0, 0.0)
                .set_line_width(2.0)
                .rect(120.0, 500.0, 150.0, 100.0)
                .fill_and_stroke();
        }),
    ];
    for (id, desc, build) in graphics {
        let mut doc = Document::new();
        build(&mut doc);
        let rel = format!("generated/gfx_{id}.pdf");
        doc.save(root.join(&rel)).unwrap();
        entries.push(Entry {
            id: format!("gfx-{id}"),
            category: "graphics".into(),
            description: desc.into(),
            file: Some(rel),
            features: vec!["content-stream", "graphics-state", "color", "path"],
            expect_valid: true,
            status: "generated",
        });
    }

    // --- corrupted variants (recovery targets, Fase 5.8) ------------------
    let mut base = Document::new();
    base.add_page()
        .content()
        .set_fill_rgb(0.0, 0.0, 0.0)
        .rect(72.0, 600.0, 100.0, 100.0)
        .fill();
    let good = base.to_bytes().unwrap();

    // Truncated file (missing trailer/EOF).
    write_bytes(
        &root.join("corrupted/truncated.pdf"),
        &good[..good.len() * 2 / 3],
    );
    entries.push(Entry {
        id: "corrupt-truncated".into(),
        category: "corrupted".into(),
        description: "file truncated mid-body (no xref/trailer)".into(),
        file: Some("corrupted/truncated.pdf".into()),
        features: vec!["recovery"],
        expect_valid: false,
        status: "generated",
    });

    // Broken startxref offset.
    let mut bad_xref = good.clone();
    if let Some(pos) = find(&bad_xref, b"startxref\n") {
        let num_start = pos + b"startxref\n".len();
        // Overwrite the first digit so the offset points nowhere sensible.
        if num_start < bad_xref.len() {
            bad_xref[num_start] = b'9';
        }
    }
    write_bytes(&root.join("corrupted/bad_startxref.pdf"), &bad_xref);
    entries.push(Entry {
        id: "corrupt-bad-startxref".into(),
        category: "corrupted".into(),
        description: "valid body but startxref offset is wrong".into(),
        file: Some("corrupted/bad_startxref.pdf".into()),
        features: vec!["recovery", "xref-scan"],
        expect_valid: false,
        status: "generated",
    });

    // Missing %%EOF marker.
    let no_eof: Vec<u8> = String::from_utf8_lossy(&good)
        .replace("%%EOF\n", "")
        .into_bytes();
    write_bytes(&root.join("corrupted/no_eof.pdf"), &no_eof);
    entries.push(Entry {
        id: "corrupt-no-eof".into(),
        category: "corrupted".into(),
        description: "trailer present but %%EOF marker removed".into(),
        file: Some("corrupted/no_eof.pdf".into()),
        features: vec!["recovery"],
        expect_valid: false,
        status: "generated",
    });

    // --- embedded text (Fase 3): generated with the bundled Roboto fonts --
    let font_dir = workspace_root().join("assets/fonts");
    if font_dir.join("Roboto-Regular.ttf").exists() {
        // Latin "Hello World" (3A.5).
        let mut doc = Document::new();
        let f = doc
            .add_font_file(font_dir.join("Roboto-Regular.ttf"))
            .unwrap();
        doc.add_page()
            .text(f, 24.0)
            .at(72.0, 740.0)
            .show("Hello, World!");
        let rel = "generated/text_latin.pdf".to_string();
        doc.save(root.join(&rel)).unwrap();
        entries.push(Entry {
            id: "text-latin".into(),
            category: "fonts".into(),
            description: "Latin embedded subset text (Type0/CIDFontType2)".into(),
            file: Some(rel),
            features: vec!["FontFile2", "Type0", "Identity-H", "ToUnicode", "subset"],
            expect_valid: true,
            status: "generated",
        });

        // Accented Unicode (3B.4).
        let mut doc = Document::new();
        let f = doc
            .add_font_file(font_dir.join("Roboto-Regular.ttf"))
            .unwrap();
        doc.add_page()
            .text(f, 20.0)
            .at(72.0, 740.0)
            .show("Olá, açúcar — café, ñandú");
        let rel = "generated/text_unicode.pdf".to_string();
        doc.save(root.join(&rel)).unwrap();
        entries.push(Entry {
            id: "text-unicode".into(),
            category: "fonts".into(),
            description: "accented Unicode text, extracts via ToUnicode".into(),
            file: Some(rel),
            features: vec!["ToUnicode", "cid", "subset"],
            expect_valid: true,
            status: "generated",
        });

        // Justified paragraph with inline bold (3D/3F).
        let mut doc = Document::new();
        let reg = doc
            .add_font_file(font_dir.join("Roboto-Regular.ttf"))
            .unwrap();
        let bold = doc.add_font_file(font_dir.join("Roboto-Bold.ttf")).unwrap();
        doc.add_page().paragraph(
            Paragraph::new(reg, 12.0)
                .box_at(72.0, 740.0, 400.0)
                .leading(17.0)
                .align(Align::Justify)
                .text("Justified, auto-wrapped paragraph with an inline ")
                .span("bold", bold, 12.0, Some((0.8, 0.1, 0.1)))
                .text(" run that exercises shaping, kerning and line breaking."),
        );
        let rel = "generated/text_paragraph.pdf".to_string();
        doc.save(root.join(&rel)).unwrap();
        entries.push(Entry {
            id: "text-paragraph".into(),
            category: "fonts".into(),
            description: "justified paragraph, inline styles, two subset fonts".into(),
            file: Some(rel),
            features: vec!["paragraph", "kerning", "justify", "inline-style"],
            expect_valid: true,
            status: "generated",
        });
    }

    // --- images (Fase 4) --------------------------------------------------
    // JPEG embedded verbatim (DCTDecode).
    let jpeg = {
        let img = RgbImage::from_fn(160, 100, |x, y| {
            Rgb([(x * 255 / 160) as u8, (y * 255 / 100) as u8, 128])
        });
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
            .write_image(&img, 160, 100, image::ExtendedColorType::Rgb8)
            .unwrap();
        out
    };
    let mut doc = Document::new();
    let id = doc.add_image_jpeg(jpeg).unwrap();
    doc.add_page().draw_image(id, 72.0, 560.0, 320.0, 200.0);
    let rel = "generated/img_jpeg.pdf".to_string();
    doc.save(root.join(&rel)).unwrap();
    entries.push(Entry {
        id: "img-jpeg".into(),
        category: "images".into(),
        description: "JPEG embedded verbatim (DCTDecode)".into(),
        file: Some(rel),
        features: vec!["DCTDecode", "XObject", "Do"],
        expect_valid: true,
        status: "generated",
    });

    // Transparent PNG (FlateDecode + SMask) over a colored background.
    let png = {
        let img = RgbaImage::from_fn(120, 120, |x, y| {
            let (dx, dy) = (x as f32 - 60.0, y as f32 - 60.0);
            if (dx * dx + dy * dy).sqrt() < 55.0 {
                Rgba([220, 40, 40, 255])
            } else {
                Rgba([220, 40, 40, 0])
            }
        });
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(&img, 120, 120, image::ExtendedColorType::Rgba8)
            .unwrap();
        out
    };
    let mut doc = Document::new();
    let id = doc.add_image_png(&png).unwrap();
    let page = doc.add_page();
    page.content()
        .set_fill_rgb(0.1, 0.5, 0.2)
        .rect(72.0, 560.0, 160.0, 160.0)
        .fill();
    page.draw_image(id, 112.0, 560.0, 160.0, 160.0);
    let rel = "generated/img_png_alpha.pdf".to_string();
    doc.save(root.join(&rel)).unwrap();
    entries.push(Entry {
        id: "img-png-alpha".into(),
        category: "images".into(),
        description: "transparent PNG (FlateDecode + SMask)".into(),
        file: Some(rel),
        features: vec!["FlateDecode", "SMask", "alpha"],
        expect_valid: true,
        status: "generated",
    });

    // --- pending categories (need later phases) ---------------------------
    let pending: &[(&str, &str, &str, &[&str])] = &[
        ("font-cff", "fonts", "CFF/OTF embedded text", &["FontFile3"]),
        (
            "cjk",
            "cjk",
            "CJK sample (large font, subset)",
            &["cid", "cjk"],
        ),
        ("form-acroform", "forms", "fillable AcroForm", &["AcroForm"]),
        (
            "encrypted-rc4",
            "encrypted",
            "RC4 password-protected",
            &["encrypt"],
        ),
        (
            "encrypted-aes",
            "encrypted",
            "AES-256 password-protected",
            &["encrypt"],
        ),
        (
            "pdfa-1b",
            "conformance",
            "PDF/A-1b conformant",
            &["pdfa", "outputintent"],
        ),
        (
            "tagged",
            "conformance",
            "Tagged PDF / PDF-UA structure tree",
            &["tagged"],
        ),
        (
            "signed",
            "signature",
            "digitally signed (PKCS#7)",
            &["signature"],
        ),
    ];
    for (id, cat, desc, feats) in pending {
        entries.push(Entry {
            id: (*id).into(),
            category: (*cat).into(),
            description: (*desc).into(),
            file: None,
            features: feats.to_vec(),
            expect_valid: true,
            status: "pending",
        });
    }

    write_catalog(&root.join("catalog.json"), &entries);

    let generated = entries.iter().filter(|e| e.file.is_some()).count();
    println!(
        "corpus: {} catalogued ({} generated, {} pending) -> {}",
        entries.len(),
        generated,
        entries.len() - generated,
        root.display()
    );
}

fn write_catalog(path: &Path, entries: &[Entry]) {
    let mut s = String::new();
    s.push_str("{\n  \"entries\": [\n");
    for (i, e) in entries.iter().enumerate() {
        let features = e
            .features
            .iter()
            .map(|f| format!("{f:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let file = match &e.file {
            Some(f) => format!("{:?}", f),
            None => "null".to_string(),
        };
        let _ = writeln!(
            s,
            "    {{ \"id\": {:?}, \"category\": {:?}, \"description\": {:?}, \
             \"file\": {}, \"features\": [{}], \"expect_valid\": {}, \"status\": {:?} }}{}",
            e.id,
            e.category,
            e.description,
            file,
            features,
            e.expect_valid,
            e.status,
            if i + 1 == entries.len() { "" } else { "," }
        );
    }
    s.push_str("  ]\n}\n");
    write_bytes(path, s.as_bytes());
}

fn write_bytes(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn workspace_root() -> PathBuf {
    // examples run with CWD = workspace root under cargo, but be robust.
    let dir = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap());
    // crates/pdf -> repo root
    dir.parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or(dir)
}

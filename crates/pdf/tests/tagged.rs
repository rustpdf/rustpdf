//! Fase 7.5 tests: Tagged PDF / accessibility. Conformance confirmed externally
//! with veraPDF (`verapdf -f 2a` → PDF/A-2a compliant; `verapdf -f ua1` →
//! PDF/UA-1 compliant, both 0 failures). The sandbox may not spawn veraPDF, so
//! these assert the required structure; the `verapdf_*` test runs it when present.

use pdf::{Document, EditableDoc, Info, Report, StructTag, Table};

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

fn png_bytes() -> Vec<u8> {
    use image::{ImageFormat, RgbImage};
    let img = RgbImage::from_fn(32, 24, |x, y| image::Rgb([x as u8 * 8, y as u8 * 10, 120]));
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Png).unwrap();
    buf.into_inner()
}

/// A tagged PDF/A-2a doc exercising the semantic roles: H1/H2 headings, a
/// `/Figure` with `/Alt`, and a `Report` table (`Table`/`TR`/`TH`/`TD`).
fn rich_tagged_doc() -> Vec<u8> {
    lic();
    let mut doc = Document::new().pdfa_a();
    doc.set_info(Info {
        title: Some("Relatório Acessível".into()),
        ..Default::default()
    });
    let f = doc.add_font_file(FONT).unwrap();
    let img = doc.add_image_png(png_bytes()).unwrap();

    let page = doc.add_page();
    page.text(f, 22.0)
        .tag(StructTag::heading(1))
        .at(72.0, 780.0)
        .show("Título principal");
    page.text(f, 16.0)
        .tag(StructTag::heading(2))
        .at(72.0, 740.0)
        .show("Seção com figura");
    page.figure(img, 72.0, 640.0, 96.0, 72.0, "Gráfico azul de exemplo");

    let mut report = Report::new(f).page_size((595.0, 842.0));
    report = report.heading_level("Tabela de dados", 16.0, 2).table(
        Table::new(f, 11.0, vec![140.0, 160.0, 120.0])
            .header_row(true)
            .row(["Item", "Descrição", "Valor"])
            .row(["Café", "Torra média", "R$ 25"])
            .row(["Açúcar", "Refinado", "R$ 8"]),
    );
    report.render(&mut doc);

    doc.to_bytes().unwrap()
}

fn tagged_doc() -> Vec<u8> {
    lic();
    let mut doc = Document::new().pdfa_a(); // tagged + PDF/A-2a
    doc.set_info(Info {
        title: Some("Documento Acessível".into()),
        ..Default::default()
    });
    let f = doc.add_font_file(FONT).unwrap();
    let page = doc.add_page();
    page.content()
        .set_fill_rgb(0.85, 0.9, 0.95)
        .rect(60.0, 760.0, 475.0, 40.0)
        .fill();
    page.text(f, 20.0).at(72.0, 770.0).show("Relatório");
    page.text(f, 12.0)
        .at(72.0, 720.0)
        .show("Parágrafo marcado.");
    doc.to_bytes().unwrap()
}

#[test]
fn tagged_has_structure_tree_and_marked_content() {
    let bytes = tagged_doc();
    let text = String::from_utf8_lossy(&bytes);

    // Catalog tagging entries.
    assert!(text.contains("/MarkInfo"));
    assert!(text.contains("/Marked true"));
    assert!(text.contains("/StructTreeRoot"));
    assert!(text.contains("/Lang (en-US)"));
    assert!(text.contains("/DisplayDocTitle true"));
    // Structure tree.
    assert!(text.contains("/Type /StructTreeRoot"));
    assert!(text.contains("/S /Document"));
    assert!(text.contains("/S /P"));
    assert!(text.contains("/ParentTree"));
    assert!(text.contains("/StructParents 0"));
    // Marked content in the stream: artifacts via BMC, text via /P + MCID.
    assert!(text.contains("/Artifact BMC"));
    assert!(text.contains("/P <</MCID 0>> BDC"));
    assert!(text.contains("EMC"));
    // XMP carries PDF/UA + PDF/A level A identifiers.
    assert!(text.contains("<pdfuaid:part>1</pdfuaid:part>"));
    assert!(text.contains("<pdfaid:conformance>A</pdfaid:conformance>"));
}

#[test]
fn tagged_still_parses_and_extracts() {
    let bytes = tagged_doc();
    assert_eq!(EditableDoc::load(&bytes).unwrap().page_count(), 1);
    let t = pdf::extract_text(&bytes).unwrap();
    assert!(t.contains("Relatório") && t.contains("Parágrafo marcado."));
}

#[test]
fn rich_doc_has_semantic_roles() {
    let bytes = rich_tagged_doc();
    let text = String::from_utf8_lossy(&bytes);
    // Heading roles and a figure with alternate text.
    assert!(text.contains("/S /H1"));
    assert!(text.contains("/S /H2"));
    assert!(text.contains("/S /Figure"));
    assert!(text.contains("/Alt ")); // alternate text present (UTF-16BE encoded)
                                     // Nested table structure: Table → TR → TH/TD.
    assert!(text.contains("/S /Table"));
    assert!(text.contains("/S /TR"));
    assert!(text.contains("/S /TH"));
    assert!(text.contains("/S /TD"));
    // Figure marked content carries an MCID in the stream.
    assert!(text.contains("/Figure <</MCID"));
}

fn verapdf(bytes: &[u8], label: &str) {
    use std::process::Command;
    let bins = ["/opt/homebrew/bin/verapdf", "/usr/local/bin/verapdf"];
    let Some(bin) = bins.iter().find(|p| std::path::Path::new(p).is_file()) else {
        eprintln!("skipping: veraPDF not found");
        return;
    };
    let path = std::env::temp_dir().join(format!("rustpdf_{label}.pdf"));
    std::fs::write(&path, bytes).unwrap();
    for flavour in ["2a", "ua1"] {
        let out = Command::new(bin)
            .arg("-f")
            .arg(flavour)
            .arg(&path)
            .output()
            .unwrap();
        let report = String::from_utf8_lossy(&out.stdout);
        assert!(
            report.contains("isCompliant=\"true\""),
            "{label}/{flavour}: not compliant:\n{report}"
        );
    }
}

/// Finer accessibility: a list (L/LI/LBody), a caption, and a table whose
/// headers are associated by both `/Scope` and `/Headers`-`/ID`.
fn finer_tags_doc() -> Vec<u8> {
    lic();
    let mut doc = Document::new().pdfa_a();
    doc.set_info(Info {
        title: Some("Acessível fino".into()),
        ..Default::default()
    });
    let f = doc.add_font_file(FONT).unwrap();
    doc.add_page()
        .text(f, 10.0)
        .tag(StructTag::Caption)
        .at(72.0, 720.0)
        .show("Figura 1: legenda.");
    let r = Report::new(f)
        .page_size((595.0, 842.0))
        .heading("Relatório", 20.0)
        .list(vec!["Primeiro item", "Segundo item", "Terceiro item"])
        .table(
            Table::new(f, 11.0, vec![140.0, 140.0, 120.0])
                .header_row(true)
                .row(["Produto", "Categoria", "Preço"])
                .row(["Café", "Bebida", "25"]),
        );
    r.render(&mut doc);
    doc.to_bytes().unwrap()
}

#[test]
fn finer_tags_emit_list_caption_and_header_ids() {
    let text = String::from_utf8_lossy(&finer_tags_doc()).into_owned();
    assert!(text.contains("/S /L")); // list
    assert!(text.contains("/S /LI"));
    assert!(text.contains("/S /LBody"));
    assert!(text.contains("/S /Caption"));
    // Header association: TH carries an /ID, TD carries /Headers.
    assert!(text.contains("/ID (th_"));
    assert!(text.contains("/Headers [(th_"));
}

/// Inline `/Span`: a tagged run nested inside a paragraph's marked content.
fn inline_span_doc() -> Vec<u8> {
    lic();
    let mut doc = Document::new().pdfa_a();
    doc.set_info(Info {
        title: Some("Span inline".into()),
        ..Default::default()
    });
    let f = doc.add_font_file(FONT).unwrap();
    let page = doc.add_page();
    page.text(f, 14.0)
        .at(72.0, 740.0)
        .show("Texto com um ")
        .show_span("trecho destacado", StructTag::Span)
        .show(" no meio.");
    doc.to_bytes().unwrap()
}

#[test]
fn inline_span_nests_under_its_paragraph() {
    let text = String::from_utf8_lossy(&inline_span_doc()).into_owned();
    // Nested marked content in the stream.
    assert!(text.contains("/P <</MCID 0>> BDC"));
    assert!(text.contains("/Span <</MCID 1>> BDC"));
    // The P element's K mixes its own MCID with the child Span element.
    assert!(text.contains("/S /Span"));
    assert!(text.contains("/K [0 ")); // P → [MCID 0, span ref]
}

#[test]
fn verapdf_confirms_pdfa2a_and_ua1_when_available() {
    verapdf(&tagged_doc(), "tagged");
    verapdf(&finer_tags_doc(), "finer");
    verapdf(&inline_span_doc(), "span");
}

#[test]
fn verapdf_confirms_rich_semantic_structure_when_available() {
    verapdf(&rich_tagged_doc(), "rich");
}

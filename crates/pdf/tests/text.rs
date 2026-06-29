//! Fase 3 end-to-end tests: embedded subsetted text must validate and extract.
//! Uses the vendored Apache-2.0 Roboto fonts; CJK uses a system font when one
//! is present (skipped otherwise).

use std::path::{Path, PathBuf};
use std::process::Command;

use pdf::{Align, Document, Paragraph};
use testkit::{validate_with, Validator, ValidatorStatus};

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts")
}

fn qpdf_ok(path: &Path) {
    for r in validate_with(path, &[Validator::Qpdf, Validator::MutoolClean]) {
        if let ValidatorStatus::Fail { code, output } = r.status {
            panic!("{:?} failed (code {code:?}):\n{output}", r.validator);
        }
    }
}

/// Run `pdftotext path -` and return its output, or `None` if not installed.
fn pdftotext(path: &Path) -> Option<String> {
    let out = Command::new("pdftotext").arg(path).arg("-").output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn unicode_text_embeds_subsets_and_extracts() {
    let path = std::env::temp_dir().join("rustpdf_text_unicode.pdf");
    let mut doc = Document::new();
    let font = doc
        .add_font_file(assets().join("Roboto-Regular.ttf"))
        .unwrap();
    doc.add_page()
        .text(font, 20.0)
        .at(72.0, 740.0)
        .show("Hello, World!")
        .at(72.0, 710.0)
        .show("café ñandú açúcar");
    let bytes = doc.to_bytes().unwrap();
    std::fs::write(&path, &bytes).unwrap();

    // Embedded (FontFile2), Type0, subsetted (far smaller than the 515KB font).
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Subtype /Type0"));
    assert!(text.contains("/FontFile2"));
    assert!(text.contains("/Encoding /Identity-H"));
    assert!(
        bytes.len() < 50_000,
        "subset too large: {} bytes",
        bytes.len()
    );

    qpdf_ok(&path);

    if let Some(extracted) = pdftotext(&path) {
        assert!(extracted.contains("Hello, World!"), "got: {extracted:?}");
        assert!(
            extracted.contains("café ñandú açúcar"),
            "accents lost in extraction: {extracted:?}"
        );
    }
}

#[test]
fn separate_text_blocks_get_a_line_break_in_extraction() {
    // Each `page.text(...)` starts a new page item, emitted as its own
    // `BT … Tm … ET`. Two such blocks at different baselines must extract as two
    // lines — not glued together — matching pdftotext's "line inference".
    // Regression: the `BT` handler used to clear the vertical-position state,
    // so the downward step between blocks was never detected (lines concatenated).
    let mut doc = Document::new();
    let font = doc
        .add_font_file(assets().join("Roboto-Regular.ttf"))
        .unwrap();
    let page = doc.add_page();
    page.text(font, 14.0).at(72.0, 740.0).show("FIRST LINE");
    page.text(font, 14.0).at(72.0, 700.0).show("SECOND LINE");
    let bytes = doc.to_bytes().unwrap();

    let extracted = pdf::extract_text(&bytes).unwrap();
    assert!(
        extracted.contains("FIRST LINE") && extracted.contains("SECOND LINE"),
        "both lines must survive: {extracted:?}"
    );
    assert!(
        !extracted.contains("FIRST LINESECOND LINE"),
        "separate baselines must not be glued together: {extracted:?}"
    );
    // Same baseline twice must NOT introduce a spurious break.
    let mut doc2 = Document::new();
    let font2 = doc2
        .add_font_file(assets().join("Roboto-Regular.ttf"))
        .unwrap();
    let page2 = doc2.add_page();
    page2.text(font2, 14.0).at(72.0, 700.0).show("AAA");
    page2.text(font2, 14.0).at(200.0, 700.0).show("BBB");
    let same_line = pdf::extract_text(doc2.to_bytes().unwrap()).unwrap();
    assert!(
        !same_line.contains('\n') || !same_line.trim().contains('\n'),
        "same baseline should not force a line break: {same_line:?}"
    );
}

#[test]
fn paragraph_wraps_and_extracts_full_text() {
    let path = std::env::temp_dir().join("rustpdf_text_paragraph.pdf");
    let mut doc = Document::new();
    let font = doc
        .add_font_file(assets().join("Roboto-Regular.ttf"))
        .unwrap();

    let body = "The quick brown fox jumps over the lazy dog again and again \
        so that this sentence is forced to wrap across several lines inside a \
        narrow column and exercise the greedy line breaker thoroughly.";
    doc.add_page().paragraph(
        Paragraph::new(font, 12.0)
            .box_at(72.0, 740.0, 300.0)
            .leading(16.0)
            .align(Align::Justify)
            .text(body),
    );
    let bytes = doc.to_bytes().unwrap();
    std::fs::write(&path, &bytes).unwrap();
    qpdf_ok(&path);

    if let Some(extracted) = pdftotext(&path) {
        // All words survive wrapping (compare whitespace-insensitively).
        let got: String = extracted.split_whitespace().collect::<Vec<_>>().join(" ");
        let want: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(got, want, "wrapped text lost content");
    }
}

#[test]
fn inline_styles_use_multiple_fonts() {
    let path = std::env::temp_dir().join("rustpdf_text_inline.pdf");
    let mut doc = Document::new();
    let regular = doc
        .add_font_file(assets().join("Roboto-Regular.ttf"))
        .unwrap();
    let bold = doc.add_font_file(assets().join("Roboto-Bold.ttf")).unwrap();

    doc.add_page().paragraph(
        Paragraph::new(regular, 14.0)
            .box_at(72.0, 740.0, 400.0)
            .text("normal ")
            .span("BOLD", bold, 14.0, Some((0.8, 0.0, 0.0)))
            .text(" normal again"),
    );
    let bytes = doc.to_bytes().unwrap();
    std::fs::write(&path, &bytes).unwrap();

    // Two distinct subset fonts embedded.
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.matches("/FontFile2").count() == 2,
        "expected 2 embedded fonts"
    );
    qpdf_ok(&path);

    if let Some(extracted) = pdftotext(&path) {
        let joined: String = extracted.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            joined.contains("normal BOLD normal again"),
            "got: {joined:?}"
        );
    }
}

/// Fase 3E.4 — CJK shapes, embeds and extracts when a system CJK font exists.
#[test]
fn cjk_text_when_system_font_available() {
    let candidates = [
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    ];
    let Some(font_path) = candidates.iter().find(|p| Path::new(p).exists()) else {
        eprintln!("skipping CJK test: no system CJK font found");
        return;
    };

    let mut doc = Document::new();
    // .ttc collections: face 0.
    let data = std::fs::read(font_path).unwrap();
    let Ok(font) = doc.add_font(data) else {
        eprintln!("skipping CJK test: font not parseable as face 0");
        return;
    };

    let sample = "日本語のテキスト";
    doc.add_page().text(font, 24.0).at(72.0, 700.0).show(sample);
    let path = std::env::temp_dir().join("rustpdf_text_cjk.pdf");
    let bytes = doc.to_bytes().unwrap();
    std::fs::write(&path, &bytes).unwrap();

    qpdf_ok(&path);
    if let Some(extracted) = pdftotext(&path) {
        assert!(
            extracted.contains(sample),
            "CJK text did not extract: {extracted:?}"
        );
    }
}

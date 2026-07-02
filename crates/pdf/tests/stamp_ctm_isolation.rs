//! Regression: stamps (watermark / fill_rect / redaction box) must land at the
//! page's initial CTM even when the page's *original* content stream leaves the
//! graphics state modified (e.g. a top-level `cm` with no surrounding `q`/`Q`).
//! Content streams in a `/Contents` array concatenate, so without isolating the
//! original in a balanced `q … Q` the stamp would inherit that CTM. Misplacing a
//! redaction box is a confidentiality bug, hence the render-level assertion.

use pdf::EditableDoc;

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

/// A minimal one-page PDF (100×100) whose content scales everything to 10%
/// with a bare top-level `cm` — no `q`/`Q`. Anything drawn *after* this in the
/// same combined stream is shrunk to a tenth unless the state is reset.
fn polluted_ctm_pdf() -> Vec<u8> {
    let content = b"0.1 0 0 0.1 0 0 cm\n".to_vec();
    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.7\n");
    let mut offsets = [0usize; 5];

    offsets[1] = pdf.len();
    pdf.extend_from_slice(b"1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n");
    offsets[2] = pdf.len();
    pdf.extend_from_slice(b"2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n");
    offsets[3] = pdf.len();
    pdf.extend_from_slice(
        b"3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 100 100]/Contents 4 0 R/Resources<<>>>>endobj\n",
    );
    offsets[4] = pdf.len();
    pdf.extend_from_slice(format!("4 0 obj<</Length {}>>stream\n", content.len()).as_bytes());
    pdf.extend_from_slice(&content);
    pdf.extend_from_slice(b"endstream endobj\n");

    let xref = pdf.len();
    pdf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for off in offsets.iter().skip(1) {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer<</Size 5/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF").as_bytes(),
    );
    pdf
}

#[test]
fn fill_rect_ignores_polluted_page_ctm() {
    lic();
    let mut ed = EditableDoc::load(polluted_ctm_pdf()).unwrap();
    // Fill the whole page bright red. In the page's *visible* space this covers
    // the entire 100×100 box. If the original 0.1× CTM leaked, the fill would be
    // confined to a 10×10 corner and most of the page would stay white.
    assert!(ed.fill_rect(0, 0.0, 0.0, 100.0, 100.0, (1.0, 0.0, 0.0), 1.0));
    let out = ed.to_bytes().unwrap();

    let page = pdf::render_page_rgba(&out, 0, 72.0).unwrap();
    // Sample the middle of the page — it must be red, not the original white.
    let (w, h) = (page.width as usize, page.height as usize);
    let idx = ((h / 2) * w + w / 2) * 4;
    let (r, g, b) = (page.rgba[idx], page.rgba[idx + 1], page.rgba[idx + 2]);
    assert!(
        r > 200 && g < 80 && b < 80,
        "page centre should be red (stamp at initial CTM), got ({r},{g},{b})"
    );
}

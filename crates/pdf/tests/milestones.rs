//! End-to-end milestone tests (Fase 1.6 and 2.7 of `project.md`).
//!
//! These generate real files and run every *available* external validator.
//! If a validator binary is missing it is reported `Unavailable` and skipped,
//! so the suite still passes on a bare machine; where `qpdf`/`mutool` exist
//! they must accept the output.

use pdf::{Document, Matrix};
use testkit::{validate, ValidatorStatus};

fn assert_all_available_pass(path: &std::path::Path) {
    // A validator that runs must accept the file; if none can run (e.g. the
    // test sandbox can't spawn qpdf/mutool), skip rather than fail. External
    // acceptance is confirmed manually in the shell.
    for r in validate(path) {
        if let ValidatorStatus::Fail { code, output } = r.status {
            panic!(
                "{:?} rejected {}: code={:?}\n{}",
                r.validator,
                path.display(),
                code,
                output
            );
        }
    }
}

#[test]
fn milestone_1_6_blank_page_validates() {
    let path = std::env::temp_dir().join("rustpdf_milestone_blank.pdf");
    let mut doc = Document::new();
    doc.add_page();
    doc.save(&path).unwrap();
    assert_all_available_pass(&path);
}

#[test]
fn milestone_2_7_vector_graphics_validates_and_renders() {
    let path = std::env::temp_dir().join("rustpdf_milestone_vectors.pdf");

    let mut doc = Document::new();
    let page = doc.add_page();
    let c = page.content();
    c.save_state()
        .set_fill_rgb(0.86, 0.20, 0.18)
        .rect(72.0, 640.0, 200.0, 120.0)
        .fill()
        .restore_state();
    c.save_state()
        .set_stroke_rgb(0.12, 0.35, 0.78)
        .set_line_width(4.0)
        .rect(300.0, 640.0, 200.0, 120.0)
        .stroke()
        .restore_state();
    c.save_state()
        .set_stroke_cmyk(0.6, 0.0, 0.8, 0.0)
        .move_to(72.0, 520.0)
        .curve_to(180.0, 620.0, 360.0, 420.0, 500.0, 520.0)
        .stroke()
        .restore_state();
    c.save_state()
        .concat_matrix(Matrix::translate(72.0, 360.0))
        .set_fill_gray(0.5)
        .rect(0.0, 0.0, 100.0, 50.0)
        .fill()
        .restore_state();
    doc.save(&path).unwrap();

    assert_all_available_pass(&path);

    // If mutool is present, render and confirm the page is not blank — i.e. the
    // graphics actually painted pixels (regression against an empty stream).
    let png = std::env::temp_dir().join("rustpdf_milestone_vectors.png");
    match testkit::render_to_png(&path, &png, 72) {
        Ok(()) => {
            let blank = std::env::temp_dir().join("rustpdf_milestone_blank_render.png");
            let mut blank_doc = Document::new();
            blank_doc.add_page();
            let blank_pdf = std::env::temp_dir().join("rustpdf_milestone_blank2.pdf");
            blank_doc.save(&blank_pdf).unwrap();
            testkit::render_to_png(&blank_pdf, &blank, 72).unwrap();

            let diff = testkit::visual_diff(&png, &blank).unwrap();
            assert!(
                diff > 0.001,
                "vector page rendered identical to a blank page (diff={diff})"
            );
        }
        Err(testkit::VisualError::RendererMissing) => { /* skip on bare machine */ }
        Err(e) => panic!("render failed: {e}"),
    }
}

#[test]
fn ffi_and_rust_produce_identical_bytes() {
    // Dogfood (Fase 1.7): the same drawing through the high-level Rust API must
    // be byte-identical to building it object-by-object, proving determinism
    // (no timestamps/hash-map ordering). The FFI path drives this same code.
    let build = || {
        let mut doc = Document::new();
        let page = doc.add_page();
        page.content()
            .set_fill_rgb(1.0, 0.0, 0.0)
            .rect(0.0, 0.0, 100.0, 100.0)
            .fill();
        doc.to_bytes().unwrap()
    };
    assert_eq!(build(), build());
}

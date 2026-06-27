//! Fase 2.7 milestone: a page with colored lines, rectangles and curves.
//!
//! Run with: `cargo run -p pdf --example vector_graphics -- out.pdf`

use pdf::{Document, Matrix};

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "vectors.pdf".to_string());

    let mut doc = Document::new();
    let page = doc.add_page();
    let c = page.content();

    // Filled red rectangle.
    c.save_state()
        .set_fill_rgb(0.86, 0.20, 0.18)
        .rect(72.0, 640.0, 200.0, 120.0)
        .fill()
        .restore_state();

    // Blue stroked rectangle with a thicker line.
    c.save_state()
        .set_stroke_rgb(0.12, 0.35, 0.78)
        .set_line_width(4.0)
        .rect(300.0, 640.0, 200.0, 120.0)
        .stroke()
        .restore_state();

    // A CMYK-green Bézier curve.
    c.save_state()
        .set_stroke_cmyk(0.6, 0.0, 0.8, 0.0)
        .set_line_width(3.0)
        .move_to(72.0, 520.0)
        .curve_to(180.0, 620.0, 360.0, 420.0, 500.0, 520.0)
        .stroke()
        .restore_state();

    // A gray diagonal line drawn through a translated/scaled CTM.
    c.save_state()
        .concat_matrix(Matrix::translate(72.0, 360.0))
        .concat_matrix(Matrix::scale(2.0, 1.0))
        .set_stroke_gray(0.3)
        .set_line_width(1.0)
        .move_to(0.0, 0.0)
        .line_to(200.0, 80.0)
        .stroke()
        .restore_state();

    // An even-odd filled "donut" (outer rect minus inner rect).
    c.save_state()
        .set_fill_rgb(0.95, 0.6, 0.1)
        .rect(72.0, 180.0, 160.0, 160.0)
        .rect(112.0, 220.0, 80.0, 80.0)
        .fill_even_odd()
        .restore_state();

    doc.save(&path).expect("failed to write PDF");
    println!("wrote vector graphics to {path}");
}

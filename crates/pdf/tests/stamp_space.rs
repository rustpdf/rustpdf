//! FINDING-004: on a page with `/Rotate`, the positioned stamps historically
//! compose the up-righting transform (`upright_cm`) with the caller's
//! `rotation_deg` — so a stamp made with iText-style **media-space**
//! coordinates comes out rotated by an extra `/Rotate` (measured `dA` = the
//! page rotation) and displaced (the anchor swings around with it).
//!
//! `EditableDoc::set_stamp_space(StampSpace::Media)` disables that
//! composition for the positioned primitives: coordinates and `rotation_deg`
//! are taken in the raw PDF user space, like iText
//! `SetFixedPosition`/`SetRotationAngle`. The default (`Visible`) keeps the
//! historical behavior.
//!
//! Assertions inspect the emitted content stream: the presence/absence of the
//! up-righting `cm` and the actual `Tm` matrix (angle + translation).

use pdf::{Align, Document, EditableDoc, StampSpace};

const BLACK: (f64, f64, f64) = (0.0, 0.0, 0.0);

/// A blank 400×500 page, then `/Rotate 90` applied via the editing API.
fn rotated_base(deg: i32) -> EditableDoc {
    let mut doc = Document::new();
    doc.add_page_sized(400.0, 500.0);
    let mut ed = EditableDoc::load(doc.to_bytes().unwrap()).unwrap();
    if deg != 0 {
        ed.rotate_page(0, deg);
    }
    ed
}

/// The full 6-operand matrix of the last `Tm` in the bytes.
fn last_tm_matrix(pdf: &[u8]) -> [f64; 6] {
    let pos = pdf
        .windows(3)
        .rposition(|w| w == b" Tm")
        .expect("no Tm operator");
    let line_start = pdf[..pos].iter().rposition(|&b| b == b'\n').unwrap() + 1;
    let line = std::str::from_utf8(&pdf[line_start..pos]).unwrap();
    let nums: Vec<f64> = line
        .split_whitespace()
        .map(|t| t.parse().expect("Tm operand"))
        .collect();
    assert_eq!(nums.len(), 6);
    [nums[0], nums[1], nums[2], nums[3], nums[4], nums[5]]
}

/// Whether the last stamp segment (`q` … `Q`) carries a `cm` operator.
fn last_stamp_has_cm(pdf: &[u8]) -> bool {
    let text = String::from_utf8_lossy(pdf);
    let bt = text.rfind("BT\n").expect("no BT");
    let q = text[..bt].rfind("q\n").expect("no q before BT");
    text[q..bt].contains(" cm\n")
}

fn assert_close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 0.011,
        "{what}: expected {expected:.3}, got {actual:.3}"
    );
}

#[test]
fn visible_space_still_uprights_rotated_pages_by_default() {
    // Retrocompat: the default mode must keep composing the /Rotate
    // compensation (a `cm` before BT on a rotated page).
    let mut ed = rotated_base(90);
    assert!(ed.place_text(0, 40.0, 100.0, "Hi", 12.0, BLACK, 0.0));
    let bytes = ed.to_bytes().unwrap();
    assert!(
        last_stamp_has_cm(&bytes),
        "visible mode must emit the up-righting cm on a /Rotate 90 page"
    );
}

#[test]
fn media_space_composes_no_rotation_on_rotated_page() {
    // /Rotate 90 + rotation_deg 0 in media space → identity rotation in Tm,
    // no cm prefix, translation = the raw coordinates. This is what iText
    // produces for SetFixedPosition on the same page (dA = 0 vs iText).
    let mut ed = rotated_base(90);
    ed.set_stamp_space(StampSpace::Media);
    assert!(ed.place_text(0, 40.0, 100.0, "Hi", 12.0, BLACK, 0.0));
    let bytes = ed.to_bytes().unwrap();
    assert!(
        !last_stamp_has_cm(&bytes),
        "media mode must not emit any up-righting cm"
    );
    let m = last_tm_matrix(&bytes);
    assert_close(m[0], 1.0, "cos 0");
    assert_close(m[1], 0.0, "sin 0");
    assert_close(m[4], 40.0, "raw media x");
    assert_close(m[5], 100.0, "raw media y");
}

#[test]
fn media_space_rotation_deg_is_the_media_baseline_angle() {
    // The app's legacy path: /Rotate 90 page, swapped coordinates and
    // rotation_deg = page rotation. In media mode the Tm angle must be exactly
    // 90° — NOT 90° + /Rotate.
    for page_rot in [90, 180, 270] {
        let mut ed = rotated_base(page_rot);
        ed.set_stamp_space(StampSpace::Media);
        assert!(ed.place_text(0, 40.0, 100.0, "Hi", 12.0, BLACK, 90.0));
        let m = last_tm_matrix(&ed.to_bytes().unwrap());
        let angle = m[1].atan2(m[0]).to_degrees();
        assert_close(
            angle,
            90.0,
            &format!("media baseline angle on /Rotate {page_rot}"),
        );
    }
}

#[test]
fn media_space_applies_to_the_whole_stamp_family() {
    // masked_text = fill_rect + place_text; both legs must follow the mode
    // (a media-space text over a visible-space box would tear apart).
    let mut ed = rotated_base(90);
    ed.set_stamp_space(StampSpace::Media);
    assert!(ed.masked_text(
        0,
        40.0,
        100.0,
        120.0,
        30.0,
        "MASKED",
        12.0,
        BLACK,
        (1.0, 1.0, 1.0),
        Align::Left,
    ));
    let bytes = ed.to_bytes().unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        !text.contains(" cm\n"),
        "no stamp leg may emit an up-righting cm in media mode"
    );
    // place_paragraph honors it too.
    let mut ed = rotated_base(90);
    ed.set_stamp_space(StampSpace::Media);
    assert_eq!(
        ed.place_paragraph(
            0,
            40.0,
            400.0,
            30.0,
            "aaaa aaaa",
            12.0,
            BLACK,
            Align::Left,
            None,
            1.0
        ),
        Some(2)
    );
    assert!(!last_stamp_has_cm(&ed.to_bytes().unwrap()));
}

#[test]
fn media_space_ignores_crop_box_offset() {
    // Visible space translates by the crop origin even on unrotated pages;
    // media space must be the raw user space (iText coordinates), untranslated.
    let mut ed = rotated_base(0);
    ed.set_stamp_space(StampSpace::Media);
    assert!(ed.place_text(0, 40.0, 100.0, "Hi", 12.0, BLACK, 0.0));
    let bytes = ed.to_bytes().unwrap();
    assert!(!last_stamp_has_cm(&bytes));
    let m = last_tm_matrix(&bytes);
    assert_close(m[4], 40.0, "raw x untouched");
    assert_close(m[5], 100.0, "raw y untouched");
}

#[test]
fn mode_is_settable_back_to_visible() {
    let mut ed = rotated_base(90);
    ed.set_stamp_space(StampSpace::Media);
    assert_eq!(ed.stamp_space(), StampSpace::Media);
    ed.set_stamp_space(StampSpace::Visible);
    assert!(ed.place_text(0, 40.0, 100.0, "Hi", 12.0, BLACK, 0.0));
    assert!(last_stamp_has_cm(&ed.to_bytes().unwrap()));
}

// ---- rotation pivot contract (spec item C) ------------------------------------

use pdf::VerticalAnchor;

/// The pivot of `place_text` rotation is the anchor `(x, y)`: with the
/// default `Baseline` anchor and `Align::Left` the text matrix translation IS
/// the anchor, for every angle.
#[test]
fn place_text_rotation_pivots_at_the_anchor() {
    for deg in [0.0, 45.0, 90.0, 180.0, 270.0] {
        let mut ed = rotated_base(0);
        ed.set_stamp_space(StampSpace::Media);
        assert!(ed.place_text(0, 120.0, 300.0, "Pivot", 12.0, BLACK, deg));
        let m = last_tm_matrix(&ed.to_bytes().unwrap());
        assert_close(m[4], 120.0, &format!("pivot x at {deg}°"));
        assert_close(m[5], 300.0, &format!("pivot y at {deg}°"));
    }
}

/// Vertical-anchor offsets rotate WITH the text (the anchor stays the same
/// physical point of the text box): at 90° a `LineBottom` offset moves the
/// baseline start along −x, keeping (x, y) on the box-bottom line.
#[test]
fn place_text_anchor_offset_rotates_with_the_text() {
    let size = 12.0;
    let line_desc = (1.2 * 0.207 + 0.21 * 0.925) * size; // Helvetica line box
    let mut ed = rotated_base(0);
    ed.set_stamp_space(StampSpace::Media);
    assert!(ed.place_text_anchored(
        0,
        120.0,
        300.0,
        "Pivot",
        size,
        BLACK,
        90.0,
        pdf::Align::Left,
        VerticalAnchor::LineBottom,
    ));
    let m = last_tm_matrix(&ed.to_bytes().unwrap());
    assert_close(m[4], 120.0 - line_desc, "rotated LineBottom offset x");
    assert_close(m[5], 300.0, "rotated LineBottom offset y");
}

/// The pivot of `draw_image` rotation is the image's lower-left corner
/// `(x, y)`: the rotation `cm` translation is the anchor for every angle.
#[test]
fn draw_image_rotation_pivots_at_the_corner() {
    // 1×1 px opaque PNG.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0xC9, 0xFE, 0x92, 0xEF, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    for deg in [0.0, 90.0, 210.0] {
        let img = pdf::Image::from_png(PNG).unwrap();
        let mut ed = rotated_base(0);
        ed.set_stamp_space(StampSpace::Media);
        assert!(ed.draw_image(0, &img, 80.0, 150.0, 100.0, 40.0, deg));
        let bytes = ed.to_bytes().unwrap();
        // First cm of the stamp = rotation about the corner: e,f must be the
        // anchor regardless of the angle.
        let text = String::from_utf8_lossy(&bytes);
        let do_pos = text.rfind(" Do").expect("no Do");
        let seg = &text[..do_pos];
        let q = seg.rfind("q\n").expect("no q");
        let cm_line = seg[q..].lines().nth(1).expect("no cm line");
        let nums: Vec<f64> = cm_line
            .split_whitespace()
            .filter_map(|t| t.parse().ok())
            .collect();
        assert_eq!(nums.len(), 6, "rotation cm: {cm_line}");
        assert_close(nums[4], 80.0, &format!("image pivot x at {deg}°"));
        assert_close(nums[5], 150.0, &format!("image pivot y at {deg}°"));
    }
}

/// `ImageAnchor::BoundingBox` lands the rotated image's bbox lower-left on
/// the anchor (iText layout): at 90° the rotation `cm` translate becomes
/// `(x + height, y)` so the image occupies `[x, x+h] × [y, y+w]`.
#[test]
fn draw_image_bounding_box_anchor_matches_itext_layout() {
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0xC9, 0xFE, 0x92, 0xEF, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    // (deg, expected translate) for a 100×40 image anchored at (80, 150).
    let cases = [
        (0.0, 80.0, 150.0),
        (90.0, 80.0 + 40.0, 150.0),          // bbox [x, x+h] × [y, y+w]
        (180.0, 80.0 + 100.0, 150.0 + 40.0), // bbox [x, x+w] × [y, y+h]
        (270.0, 80.0, 150.0 + 100.0),
    ];
    for (deg, ex, ey) in cases {
        let img = pdf::Image::from_png(PNG).unwrap();
        let mut ed = rotated_base(0);
        ed.set_stamp_space(StampSpace::Media);
        assert!(ed.draw_image_anchored(
            0,
            &img,
            80.0,
            150.0,
            100.0,
            40.0,
            deg,
            pdf::ImageAnchor::BoundingBox,
        ));
        let bytes = ed.to_bytes().unwrap();
        let text = String::from_utf8_lossy(&bytes);
        let do_pos = text.rfind(" Do").expect("no Do");
        let seg = &text[..do_pos];
        let q = seg.rfind("q\n").expect("no q");
        let cm_line = seg[q..].lines().nth(1).expect("no cm line");
        let nums: Vec<f64> = cm_line
            .split_whitespace()
            .filter_map(|t| t.parse().ok())
            .collect();
        assert_eq!(nums.len(), 6, "rotation cm: {cm_line}");
        assert_close(nums[4], ex, &format!("bbox anchor x at {deg}°"));
        assert_close(nums[5], ey, &format!("bbox anchor y at {deg}°"));
    }
}

/// Spec item 2 (rotated text): the LineBottom anchor already realizes the
/// "effective anchor" pivot — the baseline start equals
/// `(x, y) + R(θ)·(0, line_desc·size)` at EVERY angle, i.e. the pivot is the
/// line-box bottom point (iText). Passing a pre-offset baseline `y` with
/// `Baseline` anchor instead reproduces the bench's `+off·(sinθ, 1−cosθ)`
/// error — the offset must be applied by the anchor, not by the caller.
#[test]
fn line_bottom_rotated_pivots_at_the_box_bottom_at_every_angle() {
    let size = 12.0;
    let dy = (1.2 * 0.207 + 0.21 * 0.925) * size; // Helvetica line descent
    for deg in [0.0_f64, 45.0, 90.0, 180.0, 270.0] {
        let (s, c) = deg.to_radians().sin_cos();
        let mut ed = rotated_base(0);
        ed.set_stamp_space(StampSpace::Media);
        assert!(ed.place_text_anchored(
            0,
            120.0,
            300.0,
            "Pivot",
            size,
            BLACK,
            deg,
            pdf::Align::Left,
            VerticalAnchor::LineBottom,
        ));
        let m = last_tm_matrix(&ed.to_bytes().unwrap());
        assert_close(m[4], 120.0 - dy * s, &format!("baseline x at {deg}°"));
        assert_close(m[5], 300.0 + dy * c, &format!("baseline y at {deg}°"));
    }
}

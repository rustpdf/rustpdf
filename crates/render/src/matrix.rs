//! Coordinate transforms.
//!
//! PDF user space has its origin at the lower-left with **y pointing up** and
//! is measured in points. The device pixmap has its origin at the top-left
//! with **y pointing down** and is measured in pixels. The `base` transform
//! bridges the two (and folds in page rotation); the graphics-state CTM is
//! kept in PDF user space and composed onto `base` only when we actually
//! paint.

use crate::PageBox;
use tiny_skia::Transform;

/// Build the device transform mapping PDF user-space points (already inside
/// the crop box) to device pixels, honoring `scale` and page `/Rotate`.
///
/// Derived directly: with `X = (x-x0)·scale`, `Y = (y-y0)·scale`, `W`/`H` the
/// unrotated pixel extents, each rotation maps `(x, y)` to a device pixel via
/// the affine below. `Transform::from_row(sx, ky, kx, sy, tx, ty)` evaluates
/// `px = sx·x + kx·y + tx`, `py = ky·x + sy·y + ty`.
pub fn device_transform(pbox: &PageBox, scale: f32) -> Transform {
    let (x0, y0) = (pbox.x0, pbox.y0);
    let w = (pbox.x1 - pbox.x0) * scale;
    let h = (pbox.y1 - pbox.y0) * scale;
    let s = scale;
    match pbox.rotate {
        // px = Y, py = X
        90 => Transform::from_row(0.0, s, s, 0.0, -s * y0, -s * x0),
        // px = W - X, py = Y
        180 => Transform::from_row(-s, 0.0, 0.0, s, w + s * x0, -s * y0),
        // px = H - Y, py = W - X
        270 => Transform::from_row(0.0, -s, -s, 0.0, h + s * y0, w + s * x0),
        // px = X, py = H - Y
        _ => Transform::from_row(s, 0.0, 0.0, -s, -s * x0, h + s * y0),
    }
}

/// Convert a PDF transformation matrix `[a b c d e f]` to a tiny-skia
/// [`Transform`].
pub fn pdf_matrix(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> Transform {
    Transform::from_row(a, b, c, d, e, f)
}

/// Approximate the uniform scale factor a transform applies (geometric mean of
/// the x/y axis lengths) — used to give zero-width strokes a ~1px footprint.
pub fn mean_scale(t: &Transform) -> f32 {
    let sx = (t.sx * t.sx + t.kx * t.kx).sqrt();
    let sy = (t.ky * t.ky + t.sy * t.sy).sqrt();
    (sx * sy).sqrt().max(1e-6)
}

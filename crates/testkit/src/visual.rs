//! Perceptual visual-regression helpers (Fase 0.5).
//!
//! A PDF is rendered to PNG with `mutool draw`, then compared against a
//! baseline image with a normalized mean-absolute-difference metric.
//! [`visual_diff`] returns a value in `0.0..=1.0`; callers assert it stays
//! under a threshold.

use std::path::Path;
use std::process::Command;

/// Errors from the visual-regression pipeline.
#[derive(Debug)]
pub enum VisualError {
    /// The `mutool` renderer is not installed.
    RendererMissing,
    /// The renderer process failed.
    Render(String),
    /// An image could not be read or decoded.
    Image(String),
    /// The two images differ in dimensions and cannot be compared.
    SizeMismatch { a: (u32, u32), b: (u32, u32) },
}

impl std::fmt::Display for VisualError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VisualError::RendererMissing => write!(f, "mutool renderer not found on PATH"),
            VisualError::Render(s) => write!(f, "render failed: {s}"),
            VisualError::Image(s) => write!(f, "image error: {s}"),
            VisualError::SizeMismatch { a, b } => {
                write!(f, "image size mismatch: {a:?} vs {b:?}")
            }
        }
    }
}

impl std::error::Error for VisualError {}

/// Render the first page (or all pages) of `pdf` to a PNG at `dpi`.
pub fn render_to_png(
    pdf: impl AsRef<Path>,
    out_png: impl AsRef<Path>,
    dpi: u32,
) -> Result<(), VisualError> {
    if which("mutool").is_none() {
        return Err(VisualError::RendererMissing);
    }
    let output = Command::new("mutool")
        .arg("draw")
        .arg("-r")
        .arg(dpi.to_string())
        .arg("-o")
        .arg(out_png.as_ref())
        .arg(pdf.as_ref())
        .output()
        .map_err(|e| VisualError::Render(e.to_string()))?;
    if !output.status.success() {
        return Err(VisualError::Render(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    Ok(())
}

/// Compute the normalized perceptual difference between two PNGs.
///
/// Returns the mean absolute per-channel difference scaled to `0.0..=1.0`,
/// where `0.0` is identical. Use as `assert!(visual_diff(a, b)? < threshold)`.
pub fn visual_diff(a: impl AsRef<Path>, b: impl AsRef<Path>) -> Result<f64, VisualError> {
    let img_a = image::open(a.as_ref())
        .map_err(|e| VisualError::Image(e.to_string()))?
        .to_rgba8();
    let img_b = image::open(b.as_ref())
        .map_err(|e| VisualError::Image(e.to_string()))?
        .to_rgba8();

    if img_a.dimensions() != img_b.dimensions() {
        return Err(VisualError::SizeMismatch {
            a: img_a.dimensions(),
            b: img_b.dimensions(),
        });
    }

    let pa = img_a.as_raw();
    let pb = img_b.as_raw();
    let mut acc: u64 = 0;
    for (x, y) in pa.iter().zip(pb.iter()) {
        acc += x.abs_diff(*y) as u64;
    }
    let max = (pa.len() as u64) * 255;
    Ok(if max == 0 {
        0.0
    } else {
        acc as f64 / max as f64
    })
}

fn which(bin: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(bin);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    fn write_solid(path: &Path, w: u32, h: u32, color: [u8; 4]) {
        let img = ImageBuffer::from_fn(w, h, |_, _| Rgba(color));
        img.save(path).unwrap();
    }

    #[test]
    fn identical_images_diff_zero() {
        let dir = std::env::temp_dir();
        let a = dir.join("tk_a.png");
        let b = dir.join("tk_b.png");
        write_solid(&a, 8, 8, [10, 20, 30, 255]);
        write_solid(&b, 8, 8, [10, 20, 30, 255]);
        assert_eq!(visual_diff(&a, &b).unwrap(), 0.0);
    }

    #[test]
    fn black_vs_white_is_high() {
        let dir = std::env::temp_dir();
        let a = dir.join("tk_black.png");
        let b = dir.join("tk_white.png");
        write_solid(&a, 8, 8, [0, 0, 0, 255]);
        write_solid(&b, 8, 8, [255, 255, 255, 255]);
        // RGB channels differ fully; alpha matches → 3/4 of max.
        let d = visual_diff(&a, &b).unwrap();
        assert!(d > 0.7 && d <= 0.76, "diff was {d}");
    }

    #[test]
    fn size_mismatch_errors() {
        let dir = std::env::temp_dir();
        let a = dir.join("tk_s1.png");
        let b = dir.join("tk_s2.png");
        write_solid(&a, 8, 8, [0, 0, 0, 255]);
        write_solid(&b, 4, 4, [0, 0, 0, 255]);
        assert!(matches!(
            visual_diff(&a, &b),
            Err(VisualError::SizeMismatch { .. })
        ));
    }
}

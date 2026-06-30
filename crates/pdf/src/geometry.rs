//! Read-only per-page geometry (issue #45 P1 #1).
//!
//! Maps percentage-based layout back to absolute PDF points by reading each
//! page's box geometry and rotation — without mutating the document. This is the
//! read companion to [`crate::EditableDoc::rotate_page`]: where that writes
//! `/Rotate`, [`measure_pages`] reports it back along with the page boxes.
//!
//! Coordinates are in PDF user space (points, origin lower-left). [`PageGeometry`]
//! reports the *unrotated* page size plus a rotation-adjusted size, so a 90°/270°
//! page reports swapped [`PageGeometry::rotated_width`]/[`rotated_height`](PageGeometry::rotated_height).

use cos::{Dict, Object};
use parser::{PdfError, PdfReader};

/// A rectangle in PDF user space (points), e.g. a page's `/MediaBox`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PdfRect {
    /// Lower-left X.
    pub x0: f64,
    /// Lower-left Y.
    pub y0: f64,
    /// Upper-right X.
    pub x1: f64,
    /// Upper-right Y.
    pub y1: f64,
}

impl PdfRect {
    /// Width of the rectangle (always non-negative).
    pub fn width(&self) -> f64 {
        (self.x1 - self.x0).abs()
    }
    /// Height of the rectangle (always non-negative).
    pub fn height(&self) -> f64 {
        (self.y1 - self.y0).abs()
    }
    /// Normalize so `x0 <= x1` and `y0 <= y1`.
    fn normalized(self) -> PdfRect {
        PdfRect {
            x0: self.x0.min(self.x1),
            y0: self.y0.min(self.y1),
            x1: self.x0.max(self.x1),
            y1: self.y0.max(self.y1),
        }
    }
    /// Intersection with `other`, or `None` if they do not overlap.
    fn intersect(self, other: PdfRect) -> Option<PdfRect> {
        let a = self.normalized();
        let b = other.normalized();
        let r = PdfRect {
            x0: a.x0.max(b.x0),
            y0: a.y0.max(b.y0),
            x1: a.x1.min(b.x1),
            y1: a.y1.min(b.y1),
        };
        if r.x1 > r.x0 && r.y1 > r.y0 {
            Some(r)
        } else {
            None
        }
    }
}

/// The read-only geometry of a single page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGeometry {
    /// Zero-based page index.
    pub page: usize,
    /// Visible (crop) width in points, ignoring rotation.
    pub width: f64,
    /// Visible (crop) height in points, ignoring rotation.
    pub height: f64,
    /// Page rotation in degrees, normalized to 0, 90, 180 or 270.
    pub rotation: i32,
    /// Width as the page appears once rotation is applied (swapped for 90/270).
    pub rotated_width: f64,
    /// Height as the page appears once rotation is applied (swapped for 90/270).
    pub rotated_height: f64,
    /// The page `/MediaBox` (inherited from the page tree if not set directly).
    pub media_box: PdfRect,
    /// The page `/CropBox` (defaults to the MediaBox when absent).
    pub crop_box: PdfRect,
}

/// A4 in points, the fallback when a page declares no `/MediaBox`.
const A4: PdfRect = PdfRect {
    x0: 0.0,
    y0: 0.0,
    x1: 595.276,
    y1: 841.89,
};

/// Read the geometry of every page in `bytes`, in page order.
pub fn measure_pages(bytes: impl AsRef<[u8]>) -> Result<Vec<PageGeometry>, PdfError> {
    let reader = PdfReader::parse(bytes)?;
    let pages = reader.pages();
    Ok(pages
        .iter()
        .enumerate()
        .map(|(i, page)| measure(&reader, page, i))
        .collect())
}

/// Read the geometry of a single page (0-based). Errors if `index` is out of
/// range.
pub fn measure_page(bytes: impl AsRef<[u8]>, index: usize) -> Result<PageGeometry, PdfError> {
    let reader = PdfReader::parse(bytes)?;
    let pages = reader.pages();
    let page = pages
        .get(index)
        .ok_or_else(|| PdfError::Syntax(format!("page index {index} out of range")))?;
    Ok(measure(&reader, page, index))
}

fn measure(reader: &PdfReader, page: &Dict, index: usize) -> PageGeometry {
    let media = inherited_rect(reader, page, "MediaBox").unwrap_or(A4);
    // CropBox defaults to MediaBox; clamp it to the media box.
    let crop = inherited_rect(reader, page, "CropBox")
        .and_then(|c| c.intersect(media))
        .unwrap_or(media)
        .normalized();
    let media = media.normalized();
    let rotation = normalize_rotation(inherited_int(reader, page, "Rotate").unwrap_or(0));
    let (w, h) = (crop.width(), crop.height());
    let (rw, rh) = if rotation == 90 || rotation == 270 {
        (h, w)
    } else {
        (w, h)
    };
    PageGeometry {
        page: index,
        width: w,
        height: h,
        rotation,
        rotated_width: rw,
        rotated_height: rh,
        media_box: media,
        crop_box: crop,
    }
}

fn normalize_rotation(r: i64) -> i32 {
    let r = r.rem_euclid(360);
    // Snap to the nearest legal quarter turn.
    match r {
        90 => 90,
        180 => 180,
        270 => 270,
        _ => 0,
    }
}

/// Walk `key` up the page tree via `/Parent` (max 32 hops), returning the first
/// dict that carries it. Used for inheritable attributes (MediaBox/CropBox/Rotate).
fn inherited<'a>(reader: &'a PdfReader, page: &'a Dict, key: &str) -> Option<Object> {
    let mut current = page.clone();
    for _ in 0..32 {
        if let Some(v) = current.get(key) {
            return Some(reader.resolve(v).clone());
        }
        let parent = current.get("Parent")?;
        current = reader.resolve_dict(parent)?.clone();
    }
    None
}

fn inherited_rect(reader: &PdfReader, page: &Dict, key: &str) -> Option<PdfRect> {
    match inherited(reader, page, key) {
        Some(Object::Array(a)) => {
            let v: Vec<f64> = a.iter().filter_map(|o| num(reader.resolve(o))).collect();
            if v.len() == 4 {
                Some(PdfRect {
                    x0: v[0],
                    y0: v[1],
                    x1: v[2],
                    y1: v[3],
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

fn inherited_int(reader: &PdfReader, page: &Dict, key: &str) -> Option<i64> {
    match inherited(reader, page, key) {
        Some(Object::Integer(n)) => Some(n),
        Some(Object::Real(r)) => Some(r as i64),
        _ => None,
    }
}

fn num(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(n) => Some(*n as f64),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

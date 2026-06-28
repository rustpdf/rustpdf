//! Native PDF page rasterizer.
//!
//! This crate interprets a page's content stream and paints it onto an RGBA
//! pixmap — text (real glyph outlines), vector graphics, raster images,
//! clipping, color spaces, transparency and shadings. It is the *reverse* of
//! the `writer`/`graphics` path: where those emit content-stream operators,
//! this one executes them.
//!
//! The 2D backend is [`tiny_skia`] (pure-Rust, deterministic anti-aliased
//! scan conversion); glyph outlines come from `ttf-parser` via the `fonts`
//! crate, and image samples from the `images` crate. No C dependencies, so it
//! stays portable and matches the project's single-Rust-core design.
//!
//! ```no_run
//! use parser::PdfReader;
//! let reader = PdfReader::open("in.pdf").unwrap();
//! let page = &reader.pages()[0];
//! let png = render::render_page_to_png(&reader, page, &render::RenderOptions::dpi(150.0)).unwrap();
//! std::fs::write("page1.png", png).unwrap();
//! ```

mod color;
mod content;
mod font;
mod func;
mod gstate;
mod matrix;
mod shading;
mod xobject;

use cos::Dict;
use parser::PdfReader;

pub use tiny_skia::Pixmap;

/// How a page should be rasterized.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Device pixels per PDF point (1 point = 1/72 inch). `1.0` ⇒ 72 DPI.
    pub scale: f32,
    /// Background fill. `None` leaves the canvas fully transparent; most
    /// callers want opaque white (the default).
    pub background: Option<[u8; 4]>,
    /// Clamp on the produced bitmap's largest dimension (guards against
    /// hostile `/MediaBox`es). `0` disables the cap.
    pub max_pixels: u32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            scale: 1.0,
            background: Some([255, 255, 255, 255]),
            max_pixels: 20_000,
        }
    }
}

impl RenderOptions {
    /// Options targeting a given output resolution in dots-per-inch.
    pub fn dpi(dpi: f32) -> Self {
        RenderOptions {
            scale: dpi / 72.0,
            ..Default::default()
        }
    }

    /// Transparent background instead of white.
    pub fn transparent(mut self) -> Self {
        self.background = None;
        self
    }
}

/// Anything that can go wrong while rasterizing a page.
#[derive(Debug)]
pub enum RenderError {
    /// The page has no usable `/MediaBox` (or it is degenerate).
    BadPageGeometry,
    /// The requested (or implied) bitmap size is zero or exceeds `max_pixels`.
    Size(u32, u32),
    /// Allocating the backing pixmap failed.
    Alloc,
    /// A lower-level parser error surfaced while pulling object data.
    Parser(parser::PdfError),
    /// Encoding the final PNG failed.
    Encode(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::BadPageGeometry => write!(f, "page has no usable MediaBox"),
            RenderError::Size(w, h) => write!(f, "refusing to render a {w}x{h} bitmap"),
            RenderError::Alloc => write!(f, "could not allocate the page pixmap"),
            RenderError::Parser(e) => write!(f, "parser error: {e}"),
            RenderError::Encode(e) => write!(f, "PNG encode failed: {e}"),
        }
    }
}

impl std::error::Error for RenderError {}

impl From<parser::PdfError> for RenderError {
    fn from(e: parser::PdfError) -> Self {
        RenderError::Parser(e)
    }
}

/// The geometry of a page in PDF user space, after applying `/MediaBox`,
/// `/CropBox` clamping and `/Rotate`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PageBox {
    /// Lower-left / upper-right of the visible box, in points.
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    /// Page rotation, normalized to one of 0/90/180/270.
    pub rotate: i32,
}

impl PageBox {
    fn width(&self) -> f32 {
        self.x1 - self.x0
    }
    fn height(&self) -> f32 {
        self.y1 - self.y0
    }
    /// Bitmap size (in pixels) for a given scale, accounting for rotation.
    fn pixel_size(&self, scale: f32) -> (u32, u32) {
        let w = (self.width() * scale).round().max(1.0);
        let h = (self.height() * scale).round().max(1.0);
        if self.rotate % 180 == 0 {
            (w as u32, h as u32)
        } else {
            (h as u32, w as u32)
        }
    }
}

/// Render a page to an RGBA [`Pixmap`] (straight, non-premultiplied alpha is
/// produced by [`to_rgba8`]/[`render_page_to_png`]; the pixmap itself stores
/// premultiplied pixels per tiny-skia convention).
pub fn render_page(
    reader: &PdfReader,
    page: &Dict,
    opts: &RenderOptions,
) -> Result<Pixmap, RenderError> {
    let pbox = page_box(reader, page).ok_or(RenderError::BadPageGeometry)?;
    let (pw, ph) = pbox.pixel_size(opts.scale);
    if pw == 0 || ph == 0 || (opts.max_pixels != 0 && pw.max(ph) > opts.max_pixels) {
        return Err(RenderError::Size(pw, ph));
    }
    let mut pixmap = Pixmap::new(pw, ph).ok_or(RenderError::Alloc)?;
    if let Some(bg) = opts.background {
        pixmap.fill(tiny_skia::Color::from_rgba8(bg[0], bg[1], bg[2], bg[3]));
    }

    content::render(reader, page, pbox, opts, &mut pixmap);
    Ok(pixmap)
}

/// Render a page and return its pixels as straight (non-premultiplied) RGBA8,
/// row-major, 4 bytes per pixel, alongside `(width, height)`.
pub fn to_rgba8(pixmap: &Pixmap) -> (Vec<u8>, u32, u32) {
    let (w, h) = (pixmap.width(), pixmap.height());
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for px in pixmap.pixels() {
        // tiny-skia stores premultiplied; demultiply back to straight alpha.
        let c = px.demultiply();
        out.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    (out, w, h)
}

/// Render a page straight to PNG bytes.
pub fn render_page_to_png(
    reader: &PdfReader,
    page: &Dict,
    opts: &RenderOptions,
) -> Result<Vec<u8>, RenderError> {
    let pixmap = render_page(reader, page, opts)?;
    encode_png(&pixmap)
}

fn encode_png(pixmap: &Pixmap) -> Result<Vec<u8>, RenderError> {
    let (rgba, w, h) = to_rgba8(pixmap);
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc
            .write_header()
            .map_err(|e| RenderError::Encode(e.to_string()))?;
        writer
            .write_image_data(&rgba)
            .map_err(|e| RenderError::Encode(e.to_string()))?;
        writer
            .finish()
            .map_err(|e| RenderError::Encode(e.to_string()))?;
    }
    Ok(out)
}

/// Resolve a page's visible box and rotation. `/CropBox` (when present and
/// sane) wins over `/MediaBox`; both are inherited up the page tree by the
/// parser's `pages()` flattening, but we also defensively walk `/Parent`.
fn page_box(reader: &PdfReader, page: &Dict) -> Option<PageBox> {
    let media = inherited_rect(reader, page, "MediaBox")?;
    let crop = inherited_rect(reader, page, "CropBox");
    let (mut x0, mut y0, mut x1, mut y1) = crop.unwrap_or(media);
    // Intersect crop with media, normalize ordering.
    let (mx0, my0, mx1, my1) = media;
    x0 = x0.max(mx0.min(mx1));
    y0 = y0.max(my0.min(my1));
    x1 = x1.min(mx1.max(mx0));
    y1 = y1.min(my1.max(my0));
    if !(x1 > x0 && y1 > y0) {
        // Fall back to the raw media box if the intersection collapsed.
        let (a, b, c, d) = media;
        x0 = a.min(c);
        y0 = b.min(d);
        x1 = a.max(c);
        y1 = b.max(d);
    }
    if !(x1 > x0 && y1 > y0) {
        return None;
    }
    let rotate = inherited_int(reader, page, "Rotate").unwrap_or(0);
    let rotate = ((rotate % 360) + 360) % 360;
    Some(PageBox {
        x0,
        y0,
        x1,
        y1,
        rotate,
    })
}

fn inherited_rect(reader: &PdfReader, page: &Dict, key: &str) -> Option<(f32, f32, f32, f32)> {
    let obj = inherited(reader, page, key)?;
    let arr = match reader.resolve(&obj) {
        cos::Object::Array(a) => a,
        _ => return None,
    };
    if arr.len() < 4 {
        return None;
    }
    let n = |i: usize| num(reader.resolve(&arr[i]));
    Some((n(0)?, n(1)?, n(2)?, n(3)?))
}

fn inherited_int(reader: &PdfReader, page: &Dict, key: &str) -> Option<i32> {
    let obj = inherited(reader, page, key)?;
    match reader.resolve(&obj) {
        cos::Object::Integer(n) => Some(*n as i32),
        _ => None,
    }
}

/// Look up an attribute on the page, walking `/Parent` for inheritable keys.
fn inherited(reader: &PdfReader, page: &Dict, key: &str) -> Option<cos::Object> {
    let mut cur = page.clone();
    for _ in 0..32 {
        if let Some(v) = cur.get(key) {
            return Some(v.clone());
        }
        match cur.get("Parent") {
            Some(p) => match reader.resolve_dict(p) {
                Some(d) => cur = d.clone(),
                None => return None,
            },
            None => return None,
        }
    }
    None
}

pub(crate) fn num(o: &cos::Object) -> Option<f32> {
    match o {
        cos::Object::Integer(n) => Some(*n as f32),
        cos::Object::Real(r) => Some(*r as f32),
        _ => None,
    }
}

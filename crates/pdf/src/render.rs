//! Page rasterization — render a PDF page to a raster image (the reverse of
//! the writer path).
//!
//! This is a thin, high-level wrapper over the [`render`] crate: it parses the
//! input PDF and renders the requested page to PNG or raw RGBA. The heavy
//! lifting (content-stream interpretation, glyph outlines, images, shadings)
//! lives in the `render` crate so the Rust core stays layered.

use parser::PdfReader;
pub use render::{RenderError, RenderOptions};

/// A rendered page as straight (non-premultiplied) RGBA8 pixels.
#[derive(Debug, Clone)]
pub struct RenderedPage {
    pub width: u32,
    pub height: u32,
    /// Row-major RGBA, 4 bytes per pixel, `width * height * 4` long.
    pub rgba: Vec<u8>,
}

/// Errors from the page-rendering helpers.
#[derive(Debug)]
pub enum PageRenderError {
    /// The page index is out of range for the document.
    PageOutOfRange(usize),
    /// The underlying renderer failed.
    Render(RenderError),
}

impl std::fmt::Display for PageRenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PageRenderError::PageOutOfRange(i) => write!(f, "page index {i} out of range"),
            PageRenderError::Render(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for PageRenderError {}

impl From<RenderError> for PageRenderError {
    fn from(e: RenderError) -> Self {
        PageRenderError::Render(e)
    }
}

impl From<parser::PdfError> for PageRenderError {
    fn from(e: parser::PdfError) -> Self {
        PageRenderError::Render(RenderError::Parser(e))
    }
}

/// Render page `index` (0-based) of `bytes` to a PNG image at `dpi`.
pub fn render_page_to_png(
    bytes: impl AsRef<[u8]>,
    index: usize,
    dpi: f32,
) -> Result<Vec<u8>, PageRenderError> {
    render_page_to_png_with(bytes, index, &RenderOptions::dpi(dpi))
}

/// Render a page to PNG with full control over [`RenderOptions`].
pub fn render_page_to_png_with(
    bytes: impl AsRef<[u8]>,
    index: usize,
    opts: &RenderOptions,
) -> Result<Vec<u8>, PageRenderError> {
    let reader = PdfReader::parse(bytes)?;
    let pages = reader.pages();
    let page = pages
        .get(index)
        .ok_or(PageRenderError::PageOutOfRange(index))?;
    Ok(render::render_page_to_png(&reader, page, opts)?)
}

/// Render a page to raw RGBA8 pixels at `dpi`.
pub fn render_page_rgba(
    bytes: impl AsRef<[u8]>,
    index: usize,
    dpi: f32,
) -> Result<RenderedPage, PageRenderError> {
    render_page_rgba_with(bytes, index, &RenderOptions::dpi(dpi))
}

/// Render a page to raw RGBA8 with full control over [`RenderOptions`].
pub fn render_page_rgba_with(
    bytes: impl AsRef<[u8]>,
    index: usize,
    opts: &RenderOptions,
) -> Result<RenderedPage, PageRenderError> {
    let reader = PdfReader::parse(bytes)?;
    let pages = reader.pages();
    let page = pages
        .get(index)
        .ok_or(PageRenderError::PageOutOfRange(index))?;
    let pixmap = render::render_page(&reader, page, opts)?;
    let (rgba, width, height) = render::to_rgba8(&pixmap);
    Ok(RenderedPage {
        width,
        height,
        rgba,
    })
}

/// Number of pages in `bytes` (convenience for callers iterating renders).
pub fn page_count(bytes: impl AsRef<[u8]>) -> Result<usize, PageRenderError> {
    Ok(PdfReader::parse(bytes)?.pages().len())
}

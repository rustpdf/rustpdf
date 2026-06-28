//! Safe wrapper over the document-authoring surface.

use std::os::raw::c_char;
use std::ptr;

use crate::enums::{AFRelationship, Align, FacturxProfile, PdfVersion, PdfaLevel};
use crate::error::{PdfError, PdfStatus, Result};
use crate::ffi::{self, RawDoc};
use crate::util::{check, cstr, opt_cstr, take_buffer};
use crate::{FontId, ImageId};

/// A document outline (bookmark) entry. Nest children with [`Bookmark::child`]
/// to build a tree; pass a root to [`Document::add_bookmark`].
#[derive(Clone, Debug, PartialEq)]
pub struct Bookmark {
    /// Visible outline title.
    pub title: String,
    /// 0-based page index the bookmark jumps to.
    pub page: usize,
    /// Optional vertical position (PDF user-space `top`) within the page.
    pub top: Option<f64>,
    /// Nested child bookmarks.
    pub children: Vec<Bookmark>,
}

impl Bookmark {
    /// A leaf bookmark pointing at `page` (0-based), with no explicit `top`.
    pub fn new(title: impl Into<String>, page: usize) -> Self {
        Bookmark {
            title: title.into(),
            page,
            top: None,
            children: Vec::new(),
        }
    }

    /// Set the vertical destination (`top`) within the target page.
    pub fn with_top(mut self, top: f64) -> Self {
        self.top = Some(top);
        self
    }

    /// Append a child bookmark, returning `self` so calls chain.
    pub fn child(mut self, bookmark: Bookmark) -> Self {
        self.children.push(bookmark);
        self
    }

    /// Pre-order flatten into the parallel arrays the C ABI expects.
    fn flatten(
        &self,
        level: i32,
        levels: &mut Vec<i32>,
        titles: &mut Vec<std::ffi::CString>,
        pages: &mut Vec<usize>,
        tops: &mut Vec<f64>,
        has_tops: &mut Vec<i32>,
    ) -> Result<()> {
        levels.push(level);
        titles.push(cstr(&self.title)?);
        pages.push(self.page);
        match self.top {
            Some(t) => {
                tops.push(t);
                has_tops.push(1);
            }
            None => {
                tops.push(0.0);
                has_tops.push(0);
            }
        }
        for c in &self.children {
            c.flatten(level + 1, levels, titles, pages, tops, has_tops)?;
        }
        Ok(())
    }
}

/// A PDF being authored from scratch.
///
/// Mutators return `&mut Self` (wrapped in [`Result`]) so calls chain:
/// `doc.add_page()?.rect(...)?.fill()?;`. The handle is freed on drop.
pub struct Document {
    handle: *mut RawDoc,
}

// The engine core is `Send` but not `Sync` (ADR 0002); the handle follows.
unsafe impl Send for Document {}

impl Document {
    /// Create a new, empty A4 document.
    pub fn new() -> Result<Self> {
        let a = ffi::api()?;
        // SAFETY: no arguments; returns an owned handle or null.
        let handle = unsafe { (a.pdf_document_new)() };
        if handle.is_null() {
            return Err(PdfError::new(
                PdfStatus::NullPointer,
                "pdf_document_new returned null",
            ));
        }
        Ok(Document { handle })
    }

    /// Number of pages (0 if the library could not be reached).
    pub fn page_count(&self) -> usize {
        let Ok(a) = ffi::api() else { return 0 };
        let n = unsafe { (a.pdf_document_page_count)(self.handle) };
        if n < 0 {
            0
        } else {
            n as usize
        }
    }

    /// Append a page using the default size (A4).
    pub fn add_page(&mut self) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe { (a.pdf_document_add_page)(self.handle) })?;
        Ok(self)
    }

    /// Append a page with an explicit size (points).
    pub fn add_page_sized(&mut self, width: f64, height: f64) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_document_add_page_sized)(self.handle, width, height)
        })?;
        Ok(self)
    }

    /// Set the default page size (points) for subsequently added pages.
    pub fn set_default_size(&mut self, width: f64, height: f64) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_document_set_default_size)(self.handle, width, height)
        })?;
        Ok(self)
    }

    /// Set the PDF header version.
    pub fn set_version(&mut self, version: PdfVersion) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_document_set_version)(self.handle, version.code())
        })?;
        Ok(self)
    }

    /// Set `/Info` metadata; any field may be `None`.
    pub fn set_info(
        &mut self,
        title: Option<&str>,
        author: Option<&str>,
        subject: Option<&str>,
        keywords: Option<&str>,
        creator: Option<&str>,
    ) -> Result<&mut Self> {
        let a = ffi::api()?;
        let (t, au, su, kw, cr) = (
            opt_cstr(title)?,
            opt_cstr(author)?,
            opt_cstr(subject)?,
            opt_cstr(keywords)?,
            opt_cstr(creator)?,
        );
        let p = |c: &Option<std::ffi::CString>| c.as_ref().map_or(ptr::null(), |s| s.as_ptr());
        check(a, unsafe {
            (a.pdf_document_set_info)(self.handle, p(&t), p(&au), p(&su), p(&kw), p(&cr))
        })?;
        Ok(self)
    }

    // --- vector graphics ---

    /// Set the fill color (DeviceRGB, components 0..=1) on the current page.
    pub fn set_fill_rgb(&mut self, r: f64, g: f64, b: f64) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_page_set_fill_rgb)(self.handle, r, g, b)
        })?;
        Ok(self)
    }

    /// Set the stroke color (DeviceRGB) on the current page.
    pub fn set_stroke_rgb(&mut self, r: f64, g: f64, b: f64) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_page_set_stroke_rgb)(self.handle, r, g, b)
        })?;
        Ok(self)
    }

    /// Set the line width on the current page.
    pub fn set_line_width(&mut self, width: f64) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_page_set_line_width)(self.handle, width)
        })?;
        Ok(self)
    }

    /// Append a rectangle subpath on the current page.
    pub fn rect(&mut self, x: f64, y: f64, width: f64, height: f64) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_page_rect)(self.handle, x, y, width, height)
        })?;
        Ok(self)
    }

    /// Fill the current path (nonzero winding) on the current page.
    pub fn fill(&mut self) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe { (a.pdf_page_fill)(self.handle) })?;
        Ok(self)
    }

    /// Stroke the current path on the current page.
    pub fn stroke(&mut self) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe { (a.pdf_page_stroke)(self.handle) })?;
        Ok(self)
    }

    // --- fonts & text ---

    /// Register a font from a file path.
    pub fn add_font_file(&mut self, path: &str) -> Result<FontId> {
        let a = ffi::api()?;
        let path = cstr(path)?;
        let mut id = 0;
        check(a, unsafe {
            (a.pdf_document_add_font_file)(self.handle, path.as_ptr(), &mut id)
        })?;
        Ok(FontId(id))
    }

    /// Register a font from raw TrueType/OpenType bytes.
    pub fn add_font(&mut self, data: &[u8]) -> Result<FontId> {
        let a = ffi::api()?;
        let mut id = 0;
        check(a, unsafe {
            (a.pdf_document_add_font)(self.handle, data.as_ptr(), data.len(), &mut id)
        })?;
        Ok(FontId(id))
    }

    /// Show a line of text at the baseline `(x, y)`. `heading_level` 1..=6 tags
    /// it as `H1`..`H6` when the document is tagged; 0 leaves it a paragraph.
    pub fn show_text(
        &mut self,
        font: FontId,
        size: f64,
        x: f64,
        y: f64,
        text: &str,
        heading_level: u8,
    ) -> Result<&mut Self> {
        let a = ffi::api()?;
        let text = cstr(text)?;
        check(a, unsafe {
            (a.pdf_page_show_text)(
                self.handle,
                font.0,
                size,
                x,
                y,
                text.as_ptr(),
                heading_level as i32,
            )
        })?;
        Ok(self)
    }

    /// Lay out a wrapping paragraph in the box `(x, y, width)` (y = first
    /// baseline) with `align`.
    #[allow(clippy::too_many_arguments)]
    pub fn paragraph(
        &mut self,
        font: FontId,
        size: f64,
        x: f64,
        y: f64,
        width: f64,
        align: Align,
        text: &str,
    ) -> Result<&mut Self> {
        let a = ffi::api()?;
        let text = cstr(text)?;
        check(a, unsafe {
            (a.pdf_page_paragraph)(
                self.handle,
                font.0,
                size,
                x,
                y,
                width,
                align.code(),
                text.as_ptr(),
            )
        })?;
        Ok(self)
    }

    // --- images ---

    /// Register an image from a file (JPEG or PNG by signature).
    pub fn add_image_file(&mut self, path: &str) -> Result<ImageId> {
        let a = ffi::api()?;
        let path = cstr(path)?;
        let mut id = 0;
        check(a, unsafe {
            (a.pdf_document_add_image_file)(self.handle, path.as_ptr(), &mut id)
        })?;
        Ok(ImageId(id))
    }

    /// Register a PNG from bytes.
    pub fn add_image_png(&mut self, data: &[u8]) -> Result<ImageId> {
        let a = ffi::api()?;
        let mut id = 0;
        check(a, unsafe {
            (a.pdf_document_add_image_png)(self.handle, data.as_ptr(), data.len(), &mut id)
        })?;
        Ok(ImageId(id))
    }

    /// Register a JPEG from bytes.
    pub fn add_image_jpeg(&mut self, data: &[u8]) -> Result<ImageId> {
        let a = ffi::api()?;
        let mut id = 0;
        check(a, unsafe {
            (a.pdf_document_add_image_jpeg)(self.handle, data.as_ptr(), data.len(), &mut id)
        })?;
        Ok(ImageId(id))
    }

    /// Draw a (decorative) image in `(x, y, w, h)` on the current page.
    pub fn draw_image(
        &mut self,
        image: ImageId,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    ) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_page_draw_image)(self.handle, image.0, x, y, w, h)
        })?;
        Ok(self)
    }

    /// Draw a meaningful image (tagged `/Figure` with alternate text `alt`).
    pub fn figure(
        &mut self,
        image: ImageId,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        alt: &str,
    ) -> Result<&mut Self> {
        let a = ffi::api()?;
        let alt = cstr(alt)?;
        check(a, unsafe {
            (a.pdf_page_figure)(self.handle, image.0, x, y, w, h, alt.as_ptr())
        })?;
        Ok(self)
    }

    // --- standards: PDF/A, tagging ---

    /// Mark the document as PDF/A-2b.
    pub fn pdfa(&mut self) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe { (a.pdf_document_pdfa)(self.handle) })?;
        Ok(self)
    }

    /// Mark the document as PDF/A at the given conformance level.
    pub fn pdfa_level(&mut self, level: PdfaLevel) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_document_pdfa_level)(self.handle, level.code())
        })?;
        Ok(self)
    }

    /// Enable the tagged structure tree (accessibility).
    pub fn tagged(&mut self) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe { (a.pdf_document_tagged)(self.handle) })?;
        Ok(self)
    }

    /// Attach an embedded file (PDF/A-3).
    pub fn attach_file(
        &mut self,
        name: &str,
        mime: &str,
        data: &[u8],
        relationship: AFRelationship,
        desc: &str,
    ) -> Result<&mut Self> {
        let a = ffi::api()?;
        let (name, mime, desc) = (cstr(name)?, cstr(mime)?, cstr(desc)?);
        check(a, unsafe {
            (a.pdf_document_attach_file)(
                self.handle,
                name.as_ptr(),
                mime.as_ptr(),
                data.as_ptr(),
                data.len(),
                relationship.code(),
                desc.as_ptr(),
            )
        })?;
        Ok(self)
    }

    // --- AcroForm fields ---

    /// Add a text field. `rect` is `[x0, y0, x1, y1]`; `size` 0 = auto.
    pub fn text_field(
        &mut self,
        name: &str,
        page: usize,
        rect: [f64; 4],
        value: &str,
        size: f64,
    ) -> Result<&mut Self> {
        let a = ffi::api()?;
        let (name, value) = (cstr(name)?, cstr(value)?);
        check(a, unsafe {
            (a.pdf_document_text_field)(
                self.handle,
                name.as_ptr(),
                page,
                rect[0],
                rect[1],
                rect[2],
                rect[3],
                value.as_ptr(),
                size,
            )
        })?;
        Ok(self)
    }

    /// Add a checkbox.
    pub fn checkbox(
        &mut self,
        name: &str,
        page: usize,
        rect: [f64; 4],
        checked: bool,
    ) -> Result<&mut Self> {
        let a = ffi::api()?;
        let name = cstr(name)?;
        check(a, unsafe {
            (a.pdf_document_checkbox)(
                self.handle,
                name.as_ptr(),
                page,
                rect[0],
                rect[1],
                rect[2],
                rect[3],
                checked as i32,
            )
        })?;
        Ok(self)
    }

    /// Add a dropdown (choice) field. `selected` is the 0-based option index.
    pub fn dropdown(
        &mut self,
        name: &str,
        page: usize,
        rect: [f64; 4],
        options: &[&str],
        selected: Option<usize>,
        size: f64,
    ) -> Result<&mut Self> {
        let a = ffi::api()?;
        let name = cstr(name)?;
        let joined = cstr(&options.join("\n"))?;
        let sel = selected.map_or(-1, |i| i as i32);
        check(a, unsafe {
            (a.pdf_document_dropdown)(
                self.handle,
                name.as_ptr(),
                page,
                rect[0],
                rect[1],
                rect[2],
                rect[3],
                joined.as_ptr(),
                sel,
                size,
            )
        })?;
        Ok(self)
    }

    /// Add a radio-button group. Each button is `(rect, export_value)`.
    pub fn radio_group(
        &mut self,
        name: &str,
        page: usize,
        buttons: &[([f64; 4], &str)],
        selected: Option<usize>,
    ) -> Result<&mut Self> {
        let a = ffi::api()?;
        let name = cstr(name)?;
        let mut rects: Vec<f64> = Vec::with_capacity(buttons.len() * 4);
        let mut exports: Vec<std::ffi::CString> = Vec::with_capacity(buttons.len());
        for (rect, export) in buttons {
            rects.extend_from_slice(rect);
            exports.push(cstr(export)?);
        }
        let export_ptrs: Vec<*const c_char> = exports.iter().map(|c| c.as_ptr()).collect();
        let sel = selected.map_or(-1, |i| i as i32);
        check(a, unsafe {
            (a.pdf_document_radio_group)(
                self.handle,
                name.as_ptr(),
                page,
                buttons.len(),
                rects.as_ptr(),
                export_ptrs.as_ptr(),
                sel,
            )
        })?;
        Ok(self)
    }

    // --- hyperlinks / bookmarks / Factur-X ---

    /// Add a hyperlink annotation over `rect` (`[x0, y0, x1, y1]`) that opens
    /// `uri` on the current page.
    pub fn link_uri(&mut self, rect: [f64; 4], uri: &str) -> Result<&mut Self> {
        let a = ffi::api()?;
        let uri = cstr(uri)?;
        check(a, unsafe {
            (a.pdf_page_link_uri)(
                self.handle,
                rect[0],
                rect[1],
                rect[2],
                rect[3],
                uri.as_ptr(),
            )
        })?;
        Ok(self)
    }

    /// Add an internal link over `rect` jumping to `page_index` (0-based); `top`
    /// optionally sets the vertical destination within that page.
    pub fn link_to_page(
        &mut self,
        rect: [f64; 4],
        page_index: usize,
        top: Option<f64>,
    ) -> Result<&mut Self> {
        let a = ffi::api()?;
        let (top_val, has_top) = match top {
            Some(t) => (t, 1),
            None => (0.0, 0),
        };
        check(a, unsafe {
            (a.pdf_page_link_to_page)(
                self.handle,
                rect[0],
                rect[1],
                rect[2],
                rect[3],
                page_index,
                top_val,
                has_top,
            )
        })?;
        Ok(self)
    }

    /// Add one outline tree (pre-order flattened) to the document. Call once per
    /// root bookmark; nested children become nested outline entries.
    pub fn add_bookmark(&mut self, bookmark: &Bookmark) -> Result<&mut Self> {
        let a = ffi::api()?;
        let mut levels = Vec::new();
        let mut titles = Vec::new();
        let mut pages = Vec::new();
        let mut tops = Vec::new();
        let mut has_tops = Vec::new();
        bookmark.flatten(
            0,
            &mut levels,
            &mut titles,
            &mut pages,
            &mut tops,
            &mut has_tops,
        )?;
        let title_ptrs: Vec<*const c_char> = titles.iter().map(|c| c.as_ptr()).collect();
        check(a, unsafe {
            (a.pdf_document_add_bookmarks)(
                self.handle,
                levels.len(),
                levels.as_ptr(),
                title_ptrs.as_ptr(),
                pages.as_ptr(),
                tops.as_ptr(),
                has_tops.as_ptr(),
            )
        })?;
        Ok(self)
    }

    /// Embed a Factur-X / ZUGFeRD invoice XML, turning the document into a
    /// PDF/A-3 hybrid e-invoice.
    pub fn facturx(&mut self, xml: &[u8], profile: FacturxProfile) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_document_facturx)(self.handle, xml.as_ptr(), xml.len(), profile.code())
        })?;
        Ok(self)
    }

    // --- output ---

    /// Serialize and write the document to `path`.
    pub fn save(&self, path: &str) -> Result<()> {
        let a = ffi::api()?;
        let path = cstr(path)?;
        check(a, unsafe {
            (a.pdf_document_save)(self.handle, path.as_ptr())
        })
    }

    /// Serialize the document into a byte buffer.
    pub fn write(&self) -> Result<Vec<u8>> {
        let a = ffi::api()?;
        let mut ptr: *mut u8 = ptr::null_mut();
        let mut len: usize = 0;
        check(a, unsafe {
            (a.pdf_document_write)(self.handle, &mut ptr, &mut len)
        })?;
        Ok(take_buffer(a, ptr, len))
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        if let Ok(a) = ffi::api() {
            // SAFETY: handle came from pdf_document_new and is freed once.
            unsafe { (a.pdf_document_free)(self.handle) };
        }
    }
}

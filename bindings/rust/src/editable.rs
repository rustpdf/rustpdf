//! Safe wrapper over the manipulation / extraction surface.

use std::ptr;

use crate::enums::{
    Align, Encryption, ImageAnchor, PdfVersion, PdfaLevel, StampSpace, VerticalAlign,
    VerticalAnchor,
};
use crate::error::{PdfError, PdfStatus, Result};
use crate::ffi::{self, RawEditable};
use crate::util::{check, cstr, last_error, take_buffer};
use crate::FontId;

/// The `font_id` the C ABI understands for "no embedded font" (built-in
/// Helvetica).
fn font_code(font: Option<FontId>) -> i32 {
    font.map_or(-1, |f| f.0)
}

/// An existing PDF loaded for manipulation (merge/split/encrypt/…).
pub struct EditableDoc {
    handle: *mut RawEditable,
}

// Mirrors the core: `Send`, not `Sync`.
unsafe impl Send for EditableDoc {}

impl EditableDoc {
    /// Load and parse an existing PDF from bytes.
    pub fn load(data: &[u8]) -> Result<Self> {
        let a = ffi::api()?;
        // SAFETY: `data` is a readable region for `len` bytes.
        let handle = unsafe { (a.pdf_editable_load)(data.as_ptr(), data.len()) };
        if handle.is_null() {
            return Err(PdfError::new(PdfStatus::Parse, last_error(a)));
        }
        Ok(EditableDoc { handle })
    }

    /// Load an encrypted PDF using `password`.
    pub fn load_password(data: &[u8], password: &str) -> Result<Self> {
        let a = ffi::api()?;
        let pw = cstr(password)?;
        let handle =
            unsafe { (a.pdf_editable_load_password)(data.as_ptr(), data.len(), pw.as_ptr()) };
        if handle.is_null() {
            return Err(PdfError::new(PdfStatus::Parse, last_error(a)));
        }
        Ok(EditableDoc { handle })
    }

    /// Number of pages (0 if the library could not be reached).
    pub fn page_count(&self) -> usize {
        let Ok(a) = ffi::api() else { return 0 };
        let n = unsafe { (a.pdf_editable_page_count)(self.handle) };
        if n < 0 {
            0
        } else {
            n as usize
        }
    }

    /// Append all pages of `other` to this document.
    pub fn merge(&mut self, other: &EditableDoc) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_editable_merge)(self.handle, other.handle)
        })?;
        Ok(self)
    }

    /// Rotate page `index` by `degrees` (a multiple of 90).
    pub fn rotate_page(&mut self, index: usize, degrees: i32) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_editable_rotate_page)(self.handle, index, degrees)
        })?;
        Ok(self)
    }

    /// Delete page `index`.
    pub fn delete_page(&mut self, index: usize) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_editable_delete_page)(self.handle, index)
        })?;
        Ok(self)
    }

    /// Reorder pages to the given 0-based order.
    pub fn reorder_pages(&mut self, order: &[usize]) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_editable_reorder_pages)(self.handle, order.as_ptr(), order.len())
        })?;
        Ok(self)
    }

    /// Extract `indices` into a brand-new [`EditableDoc`].
    pub fn extract_pages(&self, indices: &[usize]) -> Result<EditableDoc> {
        let a = ffi::api()?;
        let mut out: *mut RawEditable = ptr::null_mut();
        check(a, unsafe {
            (a.pdf_editable_extract_pages)(self.handle, indices.as_ptr(), indices.len(), &mut out)
        })?;
        if out.is_null() {
            return Err(PdfError::new(PdfStatus::InvalidArgument, last_error(a)));
        }
        Ok(EditableDoc { handle: out })
    }

    /// Set an `/Info` entry (`key` → `value`).
    pub fn set_info(&mut self, key: &str, value: &str) -> Result<&mut Self> {
        let a = ffi::api()?;
        let (key, value) = (cstr(key)?, cstr(value)?);
        check(a, unsafe {
            (a.pdf_editable_set_info)(self.handle, key.as_ptr(), value.as_ptr())
        })?;
        Ok(self)
    }

    /// Read an `/Info` entry (empty string if absent).
    pub fn get_info(&self, key: &str) -> Result<String> {
        let a = ffi::api()?;
        let key = cstr(key)?;
        let mut ptr: *mut u8 = ptr::null_mut();
        let mut len: usize = 0;
        check(a, unsafe {
            (a.pdf_editable_get_info)(self.handle, key.as_ptr(), &mut ptr, &mut len)
        })?;
        let bytes = take_buffer(a, ptr, len);
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Replace the XMP `/Metadata` stream.
    pub fn set_xmp(&mut self, xml: &[u8]) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_editable_set_xmp)(self.handle, xml.as_ptr(), xml.len())
        })?;
        Ok(self)
    }

    /// Overlay raw content-stream bytes on page `index` (e.g. a watermark).
    pub fn overlay_page(&mut self, index: usize, content: &[u8]) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_editable_overlay_page)(self.handle, index, content.as_ptr(), content.len())
        })?;
        Ok(self)
    }

    /// Fill an AcroForm text field by name. Returns whether the field existed.
    pub fn fill_text_field(&mut self, name: &str, value: &str) -> Result<bool> {
        let a = ffi::api()?;
        let (name, value) = (cstr(name)?, cstr(value)?);
        let mut found = 0;
        check(a, unsafe {
            (a.pdf_editable_fill_text_field)(self.handle, name.as_ptr(), value.as_ptr(), &mut found)
        })?;
        Ok(found != 0)
    }

    /// Set a checkbox field on/off by name. Returns whether the field existed.
    pub fn set_checkbox(&mut self, name: &str, checked: bool) -> Result<bool> {
        let a = ffi::api()?;
        let name = cstr(name)?;
        let mut found = 0;
        check(a, unsafe {
            (a.pdf_editable_set_checkbox)(self.handle, name.as_ptr(), checked as i32, &mut found)
        })?;
        Ok(found != 0)
    }

    /// Select a radio button in group `name` by its export value. Returns whether
    /// the field existed.
    pub fn set_radio(&mut self, name: &str, export_value: &str) -> Result<bool> {
        let a = ffi::api()?;
        let (name, export_value) = (cstr(name)?, cstr(export_value)?);
        let mut found = 0;
        check(a, unsafe {
            (a.pdf_editable_set_radio)(
                self.handle,
                name.as_ptr(),
                export_value.as_ptr(),
                &mut found,
            )
        })?;
        Ok(found != 0)
    }

    /// Set a choice (dropdown / list) field's value by name. Returns whether the
    /// field existed.
    pub fn set_choice(&mut self, name: &str, value: &str) -> Result<bool> {
        let a = ffi::api()?;
        let (name, value) = (cstr(name)?, cstr(value)?);
        let mut found = 0;
        check(a, unsafe {
            (a.pdf_editable_set_choice)(self.handle, name.as_ptr(), value.as_ptr(), &mut found)
        })?;
        Ok(found != 0)
    }

    /// Flatten all AcroForm fields into static page content (removes the form).
    pub fn flatten_forms(&mut self) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe { (a.pdf_editable_flatten_forms)(self.handle) })?;
        Ok(self)
    }

    /// List the document's terminal AcroForm field names.
    pub fn field_names(&self) -> Result<Vec<String>> {
        let a = ffi::api()?;
        let mut ptr: *mut u8 = ptr::null_mut();
        let mut len: usize = 0;
        check(a, unsafe {
            (a.pdf_editable_field_names)(self.handle, &mut ptr, &mut len)
        })?;
        let bytes = take_buffer(a, ptr, len);
        let joined = String::from_utf8_lossy(&bytes);
        Ok(joined
            .split('\n')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_owned())
            .collect())
    }

    /// Stamp a diagonal text watermark across every page. `opaque_background`
    /// draws a filled (non-transparent) plate behind the text.
    #[allow(clippy::too_many_arguments)]
    pub fn watermark_text(
        &mut self,
        text: &str,
        size: f64,
        color: (f64, f64, f64),
        opacity: f64,
        rotation_deg: f64,
        opaque_background: bool,
    ) -> Result<&mut Self> {
        let a = ffi::api()?;
        let text = cstr(text)?;
        let (r, g, b) = color;
        check(a, unsafe {
            (a.pdf_editable_watermark_text)(
                self.handle,
                text.as_ptr(),
                size,
                r,
                g,
                b,
                opacity,
                rotation_deg,
                opaque_background as i32,
            )
        })?;
        Ok(self)
    }

    /// Stamp an image watermark (from a file path) across every page, rotated
    /// `rotation_deg` degrees.
    pub fn watermark_image_file(
        &mut self,
        path: &str,
        width: f64,
        height: f64,
        opacity: f64,
        rotation_deg: f64,
    ) -> Result<&mut Self> {
        let a = ffi::api()?;
        let path = cstr(path)?;
        check(a, unsafe {
            (a.pdf_editable_watermark_image_file)(
                self.handle,
                path.as_ptr(),
                width,
                height,
                opacity,
                rotation_deg,
            )
        })?;
        Ok(self)
    }

    /// Set the output PDF version (downgrade / normalize). Clears any catalog
    /// `/Version` override.
    pub fn set_version(&mut self, version: PdfVersion) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_editable_set_version)(self.handle, version.code())
        })?;
        Ok(self)
    }

    /// Strip PDF/A conformance (`/OutputIntents`, XMP `pdfaid`, `/Version`) so
    /// the file is a plain PDF.
    pub fn strip_pdfa(&mut self) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe { (a.pdf_editable_strip_pdfa)(self.handle) })?;
        Ok(self)
    }

    /// Normalize to a plain PDF at `version` (strip PDF/A + set the version).
    pub fn normalize(&mut self, version: PdfVersion) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_editable_normalize)(self.handle, version.code())
        })?;
        Ok(self)
    }

    /// Redact rectangular regions on page `index`. Each rect is `[x0,y0,x1,y1]`.
    /// Returns whether the page existed.
    pub fn redact(&mut self, index: usize, rects: &[[f64; 4]]) -> Result<bool> {
        let a = ffi::api()?;
        let mut flat: Vec<f64> = Vec::with_capacity(rects.len() * 4);
        for r in rects {
            flat.extend_from_slice(r);
        }
        let mut found = 0;
        check(a, unsafe {
            (a.pdf_editable_redact)(self.handle, index, flat.as_ptr(), rects.len(), &mut found)
        })?;
        Ok(found != 0)
    }

    /// Paint a filled rectangle at `(x, y)` sized `width`×`height` on page
    /// `page_index` (0-based), in RGB `color` (each `0..=1`) at `opacity`
    /// (`0..=1`). Coordinates are in the page's **visible** space (origin
    /// lower-left, y up), regardless of the page `/Rotate`. Returns whether the
    /// page existed.
    #[allow(clippy::too_many_arguments)]
    pub fn fill_rect(
        &mut self,
        page_index: usize,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        color: (f64, f64, f64),
        opacity: f64,
    ) -> Result<bool> {
        let a = ffi::api()?;
        let (r, g, b) = color;
        let mut found = 0;
        check(a, unsafe {
            (a.pdf_editable_fill_rect)(
                self.handle,
                page_index as i32,
                x,
                y,
                width,
                height,
                r,
                g,
                b,
                opacity,
                &mut found,
            )
        })?;
        Ok(found != 0)
    }

    /// Draw a line of positioned text with its baseline at `(x, y)` on page
    /// `page_index` (0-based), in standard Helvetica at `size` points and RGB
    /// `color` (each `0..=1`). `rotation_deg` rotates the text
    /// counter-clockwise about the `(x, y)` anchor. Coordinates are in the
    /// page's **visible** space (origin lower-left, y up), regardless of the
    /// page `/Rotate`. Returns whether the page existed.
    #[allow(clippy::too_many_arguments)]
    pub fn place_text(
        &mut self,
        page_index: usize,
        x: f64,
        y: f64,
        text: &str,
        size: f64,
        color: (f64, f64, f64),
        rotation_deg: f64,
    ) -> Result<bool> {
        let a = ffi::api()?;
        let text = cstr(text)?;
        let (r, g, b) = color;
        let mut found = 0;
        check(a, unsafe {
            (a.pdf_editable_place_text)(
                self.handle,
                page_index as i32,
                x,
                y,
                text.as_ptr(),
                size,
                r,
                g,
                b,
                rotation_deg,
                &mut found,
            )
        })?;
        Ok(found != 0)
    }

    /// Like [`place_text`](Self::place_text) but shifts the anchor along the
    /// baseline by the text width per `align`: `Align::Left` starts at `(x, y)`,
    /// `Align::Right` ends there, and `Align::Center` centers on it
    /// (`Align::Justify` behaves like `Align::Left`). `rotation_deg` rotates the
    /// text counter-clockwise about the (shifted) anchor. Coordinates are in the
    /// page's **visible** space (origin lower-left, y up), regardless of the page
    /// `/Rotate`. Returns whether the page existed.
    #[allow(clippy::too_many_arguments)]
    pub fn place_text_aligned(
        &mut self,
        page_index: usize,
        x: f64,
        y: f64,
        text: &str,
        size: f64,
        color: (f64, f64, f64),
        rotation_deg: f64,
        align: Align,
    ) -> Result<bool> {
        let a = ffi::api()?;
        let text = cstr(text)?;
        let (r, g, b) = color;
        let mut found = 0;
        check(a, unsafe {
            (a.pdf_editable_place_text_aligned)(
                self.handle,
                page_index as i32,
                x,
                y,
                text.as_ptr(),
                size,
                r,
                g,
                b,
                rotation_deg,
                align.code(),
                &mut found,
            )
        })?;
        Ok(found != 0)
    }

    /// Draw `text` over an opaque background box `[x, y, x+width, y+height]` on
    /// page `page_index` (0-based): fills the box in `bg_color`, then writes the
    /// text (standard Helvetica at `size` points in `text_color`) horizontally
    /// aligned per `align` and vertically centered within the box. The classic
    /// use is masking a placeholder and stamping the real value over it without
    /// hand-computing the baseline. Coordinates are in the page's **visible**
    /// space (origin lower-left, y up). Returns whether the page existed.
    #[allow(clippy::too_many_arguments)]
    pub fn masked_text(
        &mut self,
        page_index: usize,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        text: &str,
        size: f64,
        text_color: (f64, f64, f64),
        bg_color: (f64, f64, f64),
        align: Align,
    ) -> Result<bool> {
        let a = ffi::api()?;
        let text = cstr(text)?;
        let (tr, tg, tb) = text_color;
        let (br, bg, bb) = bg_color;
        let mut found = 0;
        check(a, unsafe {
            (a.pdf_editable_masked_text)(
                self.handle,
                page_index as i32,
                x,
                y,
                width,
                height,
                text.as_ptr(),
                size,
                tr,
                tg,
                tb,
                br,
                bg,
                bb,
                align.code(),
                &mut found,
            )
        })?;
        Ok(found != 0)
    }

    /// Stamp an image onto page `page_index` (0-based) with its lower-left
    /// corner at `(x, y)`, scaled to `width`×`height` points. `image` is the
    /// raw PNG or JPEG file bytes (the format is detected from the signature).
    /// `rotation_deg` rotates the image counter-clockwise about the `(x, y)`
    /// corner. Coordinates are in the page's **visible** space (origin
    /// lower-left, y up), regardless of the page `/Rotate`. Returns whether the
    /// page existed.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image(
        &mut self,
        page_index: usize,
        image: &[u8],
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        rotation_deg: f64,
    ) -> Result<bool> {
        let a = ffi::api()?;
        let mut found = 0;
        check(a, unsafe {
            (a.pdf_editable_draw_image)(
                self.handle,
                page_index as i32,
                image.as_ptr(),
                image.len(),
                x,
                y,
                width,
                height,
                rotation_deg,
                &mut found,
            )
        })?;
        Ok(found != 0)
    }

    /// Register a stamping font from a TrueType/OpenType file for the
    /// `place_*`/`masked_*` primitives (embedded and subset on save), exactly
    /// like [`crate::Document::add_font_file`] + `show_text`. Pass the returned
    /// id as `Some(font)`; `None` keeps the built-in Helvetica.
    pub fn add_font_file(&mut self, path: &str) -> Result<FontId> {
        let a = ffi::api()?;
        let path = cstr(path)?;
        let mut id = 0;
        check(a, unsafe {
            (a.pdf_editable_add_font_file)(self.handle, path.as_ptr(), &mut id)
        })?;
        Ok(FontId(id))
    }

    /// Register a stamping font from raw TrueType/OpenType bytes. See
    /// [`add_font_file`](Self::add_font_file).
    pub fn add_font(&mut self, data: &[u8]) -> Result<FontId> {
        let a = ffi::api()?;
        let mut id = 0;
        check(a, unsafe {
            (a.pdf_editable_add_font)(self.handle, data.as_ptr(), data.len(), &mut id)
        })?;
        Ok(FontId(id))
    }

    /// Like [`place_text_aligned`](Self::place_text_aligned) but with an
    /// explicit **vertical anchor** for `y` and an optional embedded font.
    /// [`VerticalAnchor::Baseline`] keeps the historical behavior;
    /// [`VerticalAnchor::Top`] hangs the text from `y` (baseline at
    /// `y − ascent × size`, legacy fixed-position layout semantics);
    /// [`VerticalAnchor::Bottom`] rests the descender line on `y`
    /// (`LineTop`/`LineBottom` use the layout line box). Ascent/descent come
    /// from the selected font's metrics: `font` is an id from
    /// [`add_font_file`](Self::add_font_file)/[`add_font`](Self::add_font), or
    /// `None` for the built-in Helvetica. Returns whether the page (and font)
    /// existed.
    #[allow(clippy::too_many_arguments)]
    pub fn place_text_anchored(
        &mut self,
        page_index: usize,
        x: f64,
        y: f64,
        text: &str,
        size: f64,
        color: (f64, f64, f64),
        rotation_deg: f64,
        align: Align,
        anchor: VerticalAnchor,
        font: Option<FontId>,
    ) -> Result<bool> {
        let a = ffi::api()?;
        let text = cstr(text)?;
        let (r, g, b) = color;
        let mut found = 0;
        check(a, unsafe {
            (a.pdf_editable_place_text_anchored)(
                self.handle,
                page_index as i32,
                x,
                y,
                text.as_ptr(),
                size,
                r,
                g,
                b,
                rotation_deg,
                align.code(),
                anchor.code(),
                font_code(font),
                &mut found,
            )
        })?;
        Ok(found != 0)
    }

    /// Like [`masked_text`](Self::masked_text) but with an explicit **vertical
    /// alignment** of the line inside the box, an explicit horizontal edge
    /// inset `pad` (points) for `Align::Left`/`Align::Right` (the text starts
    /// at `x + pad` or ends at `x + width − pad`; `pad < 0` keeps the
    /// historical default `min(0.15 × size, width / 4)`; `0.0` starts flush
    /// with the box edge, rectangle-based DrawString semantics), and an optional
    /// embedded font (`None` = built-in Helvetica). Returns whether the page
    /// (and font) existed.
    #[allow(clippy::too_many_arguments)]
    pub fn masked_text_padded(
        &mut self,
        page_index: usize,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        text: &str,
        size: f64,
        text_color: (f64, f64, f64),
        bg_color: (f64, f64, f64),
        align: Align,
        valign: VerticalAlign,
        pad: f64,
        font: Option<FontId>,
    ) -> Result<bool> {
        let a = ffi::api()?;
        let text = cstr(text)?;
        let (tr, tg, tb) = text_color;
        let (br, bg, bb) = bg_color;
        let mut found = 0;
        check(a, unsafe {
            (a.pdf_editable_masked_text_pad)(
                self.handle,
                page_index as i32,
                x,
                y,
                width,
                height,
                text.as_ptr(),
                size,
                tr,
                tg,
                tb,
                br,
                bg,
                bb,
                align.code(),
                valign.code(),
                pad,
                font_code(font),
                &mut found,
            )
        })?;
        Ok(found != 0)
    }

    /// Stamp a **paragraph with automatic word wrapping** on page
    /// `page_index`: break `text` into lines that fit `width` points and draw
    /// them from `(x, y)` per `anchor` (`'\n'` forces a break).
    /// [`VerticalAnchor::Top`] draws downward from the top-left corner (first
    /// baseline at `y − ascent × size`, legacy fixed-position layout semantics);
    /// [`VerticalAnchor::Baseline`] makes `y` the first line's baseline;
    /// [`VerticalAnchor::Bottom`]/[`VerticalAnchor::LineBottom`] are
    /// **bottom-pinned**: the block's bottom rests on `y` and grows upward by
    /// its real content height. `align` gaps of every line but the last of
    /// each paragraph are stretched for [`Align::Justify`]. `font` is an id
    /// from [`add_font_file`](Self::add_font_file)/[`add_font`](Self::add_font)
    /// (`None` = built-in Helvetica). `max_height > 0.0` truncates lines that
    /// would overflow it (`<= 0.0` = unlimited; for bottom anchors it is a
    /// ceiling cutting lines from the top). `line_height` scales the default
    /// `1.2 × size` baseline-to-baseline leading (`<= 0.0` = `1.0`).
    /// `rotation_deg` rotates the block counter-clockwise about the anchor.
    ///
    /// Returns `(lines, height, found)`: the number of lines drawn, the
    /// block's laid-out height in points, and whether the page (and font)
    /// existed and `width`/`size` were valid.
    #[allow(clippy::too_many_arguments)]
    pub fn place_paragraph(
        &mut self,
        page_index: usize,
        x: f64,
        y: f64,
        width: f64,
        text: &str,
        size: f64,
        color: (f64, f64, f64),
        align: Align,
        anchor: VerticalAnchor,
        font: Option<FontId>,
        max_height: f64,
        line_height: f64,
        rotation_deg: f64,
    ) -> Result<(i32, f64, bool)> {
        let a = ffi::api()?;
        let text = cstr(text)?;
        let (r, g, b) = color;
        let mut height = 0.0f64;
        let mut lines = 0;
        let mut found = 0;
        check(a, unsafe {
            (a.pdf_editable_place_paragraph_anchored)(
                self.handle,
                page_index as i32,
                x,
                y,
                width,
                text.as_ptr(),
                size,
                r,
                g,
                b,
                align.code(),
                anchor.code(),
                font_code(font),
                max_height,
                line_height,
                rotation_deg,
                &mut height,
                &mut lines,
                &mut found,
            )
        })?;
        Ok((lines, height, found != 0))
    }

    /// Choose the **coordinate space** of the positioned stamping primitives
    /// ([`fill_rect`](Self::fill_rect), `place_text*`, `masked_text*`,
    /// [`place_paragraph`](Self::place_paragraph), `draw_image*`) for
    /// subsequent calls. [`StampSpace::Visible`] (default) keeps the
    /// historical displayed-space coordinates, compensating `/Rotate`;
    /// [`StampSpace::Media`] uses raw PDF user space (legacy layout engines
    /// `fixed-position layout`/rotation semantics). Watermarks and
    /// redaction are unaffected.
    pub fn set_stamp_space(&mut self, space: StampSpace) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_editable_set_stamp_space)(self.handle, space.code())
        })?;
        Ok(self)
    }

    /// Like [`draw_image`](Self::draw_image) but with an explicit **rotation
    /// anchor**: [`ImageAnchor::Corner`] (the historical behavior) rotates the
    /// image about its lower-left corner at `(x, y)`;
    /// [`ImageAnchor::BoundingBox`] lands the rotated image's axis-aligned
    /// bounding box's lower-left corner at `(x, y)`. Returns whether the page
    /// existed.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image_anchored(
        &mut self,
        page_index: usize,
        image: &[u8],
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        rotation_deg: f64,
        anchor: ImageAnchor,
    ) -> Result<bool> {
        let a = ffi::api()?;
        let mut found = 0;
        check(a, unsafe {
            (a.pdf_editable_draw_image_anchored)(
                self.handle,
                page_index as i32,
                image.as_ptr(),
                image.len(),
                x,
                y,
                width,
                height,
                rotation_deg,
                anchor.code(),
                &mut found,
            )
        })?;
        Ok(found != 0)
    }

    /// Convert the document to PDF/A on save. Only B-levels (A1b/A2b/A3b) are
    /// valid for conversion.
    pub fn convert_to_pdfa(&mut self, level: PdfaLevel) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_editable_convert_to_pdfa)(self.handle, level.code())
        })?;
        Ok(self)
    }

    /// Drop unreferenced objects, recompress, dedupe and emit object streams.
    pub fn optimize(&mut self) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe { (a.pdf_editable_optimize)(self.handle) })?;
        Ok(self)
    }

    /// Toggle object streams + cross-reference stream output on save.
    pub fn compact(&mut self, on: bool) -> Result<&mut Self> {
        let a = ffi::api()?;
        check(a, unsafe {
            (a.pdf_editable_compact)(self.handle, on as i32)
        })?;
        Ok(self)
    }

    /// Encrypt on save with the given method and passwords.
    pub fn encrypt(
        &mut self,
        method: Encryption,
        user: &str,
        owner: &str,
        read_only: bool,
    ) -> Result<&mut Self> {
        let a = ffi::api()?;
        let (user, owner) = (cstr(user)?, cstr(owner)?);
        check(a, unsafe {
            (a.pdf_editable_encrypt)(
                self.handle,
                method.code(),
                user.as_ptr(),
                owner.as_ptr(),
                read_only as i32,
            )
        })?;
        Ok(self)
    }

    /// Serialize to a byte buffer.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let a = ffi::api()?;
        let mut ptr: *mut u8 = ptr::null_mut();
        let mut len: usize = 0;
        check(a, unsafe {
            (a.pdf_editable_to_bytes)(self.handle, &mut ptr, &mut len)
        })?;
        Ok(take_buffer(a, ptr, len))
    }

    /// Serialize as an incremental update over `original` (preserves it verbatim).
    pub fn to_bytes_incremental(&self, original: &[u8]) -> Result<Vec<u8>> {
        let a = ffi::api()?;
        let mut ptr: *mut u8 = ptr::null_mut();
        let mut len: usize = 0;
        check(a, unsafe {
            (a.pdf_editable_to_bytes_incremental)(
                self.handle,
                original.as_ptr(),
                original.len(),
                &mut ptr,
                &mut len,
            )
        })?;
        Ok(take_buffer(a, ptr, len))
    }

    /// Save to `path`.
    pub fn save(&self, path: &str) -> Result<()> {
        let a = ffi::api()?;
        let path = cstr(path)?;
        check(a, unsafe {
            (a.pdf_editable_save)(self.handle, path.as_ptr())
        })
    }
}

impl Drop for EditableDoc {
    fn drop(&mut self) {
        if let Ok(a) = ffi::api() {
            // SAFETY: handle came from a load/extract call and is freed once.
            unsafe { (a.pdf_editable_free)(self.handle) };
        }
    }
}

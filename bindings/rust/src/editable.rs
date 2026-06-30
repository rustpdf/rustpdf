//! Safe wrapper over the manipulation / extraction surface.

use std::ptr;

use crate::enums::{Encryption, PdfVersion, PdfaLevel};
use crate::error::{PdfError, PdfStatus, Result};
use crate::ffi::{self, RawEditable};
use crate::util::{check, cstr, last_error, take_buffer};

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

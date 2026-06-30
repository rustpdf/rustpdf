//! C ABI: PDF manipulation (`pdf::EditableDoc`) and text extraction.
//!
//! `PdfEditable` is its own opaque handle, freed with [`pdf_editable_free`].

use std::ffi::{c_char, c_double, c_int, c_uchar};
use std::panic::{catch_unwind, AssertUnwindSafe};

use pdf::{
    Align, ConvertError, EditableDoc, Encryption, Image, PdfaLevel, Permissions, WatermarkOptions,
};

/// Map a C-ABI alignment int to [`Align`] (0=Left, 1=Right, 2=Center, 3=Justify;
/// anything else = Left).
fn align_from_int(a: c_int) -> Align {
    match a {
        1 => Align::Right,
        2 => Align::Center,
        3 => Align::Justify,
        _ => Align::Left,
    }
}

use crate::{bytes, clear_last_error, cstr, emit_buffer, guard, set_last_error, PdfStatus};

fn pdfa_level(v: c_int) -> PdfaLevel {
    match v {
        0 => PdfaLevel::A1b,
        3 => PdfaLevel::A3b,
        _ => PdfaLevel::A2b,
    }
}

/// Opaque editable-document handle.
pub struct PdfEditable {
    inner: EditableDoc,
}

fn encryption(method: c_int) -> Encryption {
    match method {
        0 => Encryption::Rc4,
        2 => Encryption::Aes256,
        _ => Encryption::Aes128,
    }
}

/// Load and parse an existing PDF from bytes. Returns NULL on failure (see
/// [`pdf_last_error_message`](crate::pdf_last_error_message)).
///
/// # Safety
/// `data`/`len` must describe a readable region.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_load(data: *const u8, len: usize) -> *mut PdfEditable {
    load_impl(unsafe { bytes(data, len) }, b"")
}

/// Load an encrypted PDF using `password` (a NUL-terminated C string).
///
/// # Safety
/// `data`/`len` readable; `password` a valid C string.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_load_password(
    data: *const u8,
    len: usize,
    password: *const c_char,
) -> *mut PdfEditable {
    let pw = match unsafe { cstr(password, "pdf_editable_load_password") } {
        Ok(s) => s.to_string(),
        Err(_) => return std::ptr::null_mut(),
    };
    load_impl(unsafe { bytes(data, len) }, pw.as_bytes())
}

fn load_impl(data: &[u8], password: &[u8]) -> *mut PdfEditable {
    let result = catch_unwind(AssertUnwindSafe(|| {
        EditableDoc::load_with_password(data, password)
    }));
    match result {
        Ok(Ok(inner)) => {
            clear_last_error();
            Box::into_raw(Box::new(PdfEditable { inner }))
        }
        Ok(Err(e)) => {
            set_last_error(format!("load failed: {e}"));
            std::ptr::null_mut()
        }
        Err(_) => {
            set_last_error("panic while loading document");
            std::ptr::null_mut()
        }
    }
}

/// Free an editable handle. NULL is a no-op.
///
/// # Safety
/// `ed` must come from a `pdf_editable_load*` call and not be reused.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_free(ed: *mut PdfEditable) {
    if ed.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { Box::from_raw(ed) })));
}

/// Number of pages, or -1 on a null handle.
///
/// # Safety
/// `ed` must be a valid handle or NULL.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_page_count(ed: *const PdfEditable) -> c_int {
    match catch_unwind(AssertUnwindSafe(|| unsafe { ed.as_ref() })) {
        Ok(Some(ed)) => ed.inner.page_count() as c_int,
        _ => -1,
    }
}

fn with_editable<F>(ed: *mut PdfEditable, who: &str, f: F) -> PdfStatus
where
    F: FnOnce(&mut EditableDoc) -> PdfStatus,
{
    guard(|| {
        let Some(ed) = (unsafe { ed.as_mut() }) else {
            set_last_error(format!("{who}: null handle"));
            return PdfStatus::NullPointer;
        };
        f(&mut ed.inner)
    })
}

/// Append all pages of `other` to `ed`.
///
/// # Safety
/// Both handles must be valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_merge(
    ed: *mut PdfEditable,
    other: *const PdfEditable,
) -> PdfStatus {
    guard(|| {
        let (Some(ed), Some(other)) = (unsafe { ed.as_mut() }, unsafe { other.as_ref() }) else {
            set_last_error("pdf_editable_merge: null handle");
            return PdfStatus::NullPointer;
        };
        ed.inner.merge(&other.inner);
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Rotate page `index` by `degrees` (a multiple of 90).
///
/// # Safety
/// `ed` must be valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_rotate_page(
    ed: *mut PdfEditable,
    index: usize,
    degrees: c_int,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_rotate_page", |d| {
        d.rotate_page(index, degrees);
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Delete page `index`.
///
/// # Safety
/// `ed` must be valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_delete_page(ed: *mut PdfEditable, index: usize) -> PdfStatus {
    with_editable(ed, "pdf_editable_delete_page", |d| {
        d.delete_page(index);
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Reorder pages to the 0-based `order` (an array of `count` indices).
///
/// # Safety
/// `ed` valid; `order` points to `count` `usize` values.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_reorder_pages(
    ed: *mut PdfEditable,
    order: *const usize,
    count: usize,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_reorder_pages", |d| {
        if order.is_null() && count > 0 {
            set_last_error("reorder: null order");
            return PdfStatus::NullPointer;
        }
        let order = unsafe { std::slice::from_raw_parts(order, count) };
        d.reorder_pages(order);
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Extract pages `indices` (array of `count`) into a **new** editable handle,
/// returned via `out`. Returns NULL `*out` on failure.
///
/// # Safety
/// `ed` valid; `indices` points to `count` `usize`; `out` writable.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_extract_pages(
    ed: *const PdfEditable,
    indices: *const usize,
    count: usize,
    out: *mut *mut PdfEditable,
) -> PdfStatus {
    guard(|| {
        let Some(ed) = (unsafe { ed.as_ref() }) else {
            set_last_error("extract_pages: null handle");
            return PdfStatus::NullPointer;
        };
        if out.is_null() || (indices.is_null() && count > 0) {
            set_last_error("extract_pages: null argument");
            return PdfStatus::NullPointer;
        }
        let indices = unsafe { std::slice::from_raw_parts(indices, count) };
        let new = ed.inner.extract_pages(indices);
        unsafe { *out = Box::into_raw(Box::new(PdfEditable { inner: new })) };
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Set an `/Info` entry (`key` → `value`).
///
/// # Safety
/// `ed`, `key`, `value` must be valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_set_info(
    ed: *mut PdfEditable,
    key: *const c_char,
    value: *const c_char,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_set_info", |d| {
        let key = match unsafe { cstr(key, "set_info:key") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let value = match unsafe { cstr(value, "set_info:value") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        d.set_info(&key, &value);
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Read an `/Info` entry into a buffer (`out_ptr`/`out_len`); empty if absent.
///
/// # Safety
/// `ed`, `key`, `out_ptr`, `out_len` must be valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_get_info(
    ed: *const PdfEditable,
    key: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(|| {
        let Some(ed) = (unsafe { ed.as_ref() }) else {
            set_last_error("get_info: null handle");
            return PdfStatus::NullPointer;
        };
        let key = match unsafe { cstr(key, "get_info:key") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let value = ed.inner.get_info(&key).unwrap_or_default();
        unsafe { emit_buffer(value.into_bytes(), out_ptr, out_len) }
    })
}

/// Replace the XMP `/Metadata` stream with `xml`.
///
/// # Safety
/// `ed` valid; `xml`/`len` readable.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_set_xmp(
    ed: *mut PdfEditable,
    xml: *const u8,
    len: usize,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_set_xmp", |d| {
        d.set_xmp(unsafe { bytes(xml, len) }.to_vec());
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Overlay raw content-stream bytes on page `index` (e.g. a watermark).
///
/// # Safety
/// `ed` valid; `content`/`len` readable.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_overlay_page(
    ed: *mut PdfEditable,
    index: usize,
    content: *const u8,
    len: usize,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_overlay_page", |d| {
        d.overlay_page(index, unsafe { bytes(content, len) }, None);
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Fill an AcroForm text field by name. `out_found` (if non-NULL) gets 1 if the
/// field existed, else 0.
///
/// # Safety
/// `ed`, `name`, `value` valid; `out_found` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_fill_text_field(
    ed: *mut PdfEditable,
    name: *const c_char,
    value: *const c_char,
    out_found: *mut c_int,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_fill_text_field", |d| {
        let name = match unsafe { cstr(name, "fill_text_field:name") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let value = match unsafe { cstr(value, "fill_text_field:value") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let found = d.fill_text_field(&name, &value);
        if !out_found.is_null() {
            unsafe { *out_found = found as c_int };
        }
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Check/uncheck a checkbox field by name. `out_found` (if non-NULL) gets 1/0.
///
/// # Safety
/// `ed`, `name` valid; `out_found` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_set_checkbox(
    ed: *mut PdfEditable,
    name: *const c_char,
    checked: c_int,
    out_found: *mut c_int,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_set_checkbox", |d| {
        let name = match unsafe { cstr(name, "set_checkbox:name") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let found = d.set_checkbox(&name, checked != 0);
        if !out_found.is_null() {
            unsafe { *out_found = found as c_int };
        }
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Select a radio button by its export value. `out_found` (if non-NULL) gets 1/0.
///
/// # Safety
/// `ed`, `name`, `export_value` valid; `out_found` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_set_radio(
    ed: *mut PdfEditable,
    name: *const c_char,
    export_value: *const c_char,
    out_found: *mut c_int,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_set_radio", |d| {
        let name = match unsafe { cstr(name, "set_radio:name") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let export = match unsafe { cstr(export_value, "set_radio:export") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let found = d.set_radio(&name, &export);
        if !out_found.is_null() {
            unsafe { *out_found = found as c_int };
        }
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Set a choice (dropdown/list) field value. `out_found` (if non-NULL) gets 1/0.
///
/// # Safety
/// `ed`, `name`, `value` valid; `out_found` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_set_choice(
    ed: *mut PdfEditable,
    name: *const c_char,
    value: *const c_char,
    out_found: *mut c_int,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_set_choice", |d| {
        let name = match unsafe { cstr(name, "set_choice:name") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let value = match unsafe { cstr(value, "set_choice:value") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let found = d.set_choice(&name, &value);
        if !out_found.is_null() {
            unsafe { *out_found = found as c_int };
        }
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Flatten all interactive form fields into static page content (removes the
/// `/AcroForm` and widgets).
///
/// # Safety
/// `ed` must be valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_flatten_forms(ed: *mut PdfEditable) -> PdfStatus {
    with_editable(ed, "pdf_editable_flatten_forms", |d| {
        d.flatten_forms();
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Write the document's terminal field names (newline-separated) into a buffer.
///
/// # Safety
/// `ed`, `out_ptr`, `out_len` valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_field_names(
    ed: *const PdfEditable,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(|| {
        let Some(ed) = (unsafe { ed.as_ref() }) else {
            set_last_error("field_names: null handle");
            return PdfStatus::NullPointer;
        };
        let joined = ed.inner.field_names().join("\n");
        unsafe { emit_buffer(joined.into_bytes(), out_ptr, out_len) }
    })
}

/// Stamp a diagonal text watermark across every page (standard Helvetica).
/// `rotation_deg` is counter-clockwise; `opacity` in 0..=1.
///
/// # Safety
/// `ed`, `text` valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_editable_watermark_text(
    ed: *mut PdfEditable,
    text: *const c_char,
    size: f64,
    r: f64,
    g: f64,
    b: f64,
    opacity: f64,
    rotation_deg: f64,
    opaque_background: c_int,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_watermark_text", |d| {
        let text = match unsafe { cstr(text, "watermark_text:text") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        d.watermark_text(
            &text,
            WatermarkOptions {
                size,
                color: (r, g, b),
                opacity,
                rotation_deg,
                opaque_background: opaque_background != 0,
            },
        );
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Stamp an image (from a JPEG/PNG file `path`) centered on every page at
/// `width`×`height` points, rotated `rotation_deg` degrees, at `opacity`.
/// Respects page `/Rotate` and `/CropBox`.
///
/// # Safety
/// `ed`, `path` valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_editable_watermark_image_file(
    ed: *mut PdfEditable,
    path: *const c_char,
    width: f64,
    height: f64,
    opacity: f64,
    rotation_deg: f64,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_watermark_image_file", |d| {
        let path = match unsafe { cstr(path, "watermark_image_file:path") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let img = match Image::from_file(&path) {
            Ok(i) => i,
            Err(e) => {
                set_last_error(format!("watermark image load failed: {e}"));
                return PdfStatus::Image;
            }
        };
        d.watermark_image(&img, width, height, opacity, rotation_deg);
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Paint a filled rectangle at `(x, y)` sized `width`×`height` on page `index`
/// (0-based), in RGB `color` (`r`/`g`/`b`, each 0..=1) at `opacity` (0..=1).
/// Coordinates are in the page's visible space (origin lower-left, y up).
/// `out_found` receives `1` if the page existed, else `0`.
///
/// # Safety
/// `ed` valid; `out_found` writable or NULL.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_editable_fill_rect(
    ed: *mut PdfEditable,
    index: c_int,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    r: f64,
    g: f64,
    b: f64,
    opacity: f64,
    out_found: *mut c_int,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_fill_rect", |d| {
        let found = d.fill_rect(
            index.max(0) as usize,
            x,
            y,
            width,
            height,
            (r, g, b),
            opacity,
        );
        if !out_found.is_null() {
            unsafe { *out_found = found as c_int };
        }
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Draw a line of positioned text with baseline at `(x, y)` on page `index`
/// (0-based), standard Helvetica at `size` points, RGB `color` (each 0..=1).
/// `rotation_deg` rotates the text counter-clockwise about `(x, y)`.
/// Coordinates are in the page's visible space (origin lower-left, y up).
/// `out_found` receives `1` if the page existed, else `0`.
///
/// # Safety
/// `ed`, `text` valid; `out_found` writable or NULL.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_editable_place_text(
    ed: *mut PdfEditable,
    index: c_int,
    x: f64,
    y: f64,
    text: *const c_char,
    size: f64,
    r: f64,
    g: f64,
    b: f64,
    rotation_deg: f64,
    out_found: *mut c_int,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_place_text", |d| {
        let text = match unsafe { cstr(text, "place_text:text") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let found = d.place_text(
            index.max(0) as usize,
            x,
            y,
            &text,
            size,
            (r, g, b),
            rotation_deg,
        );
        if !out_found.is_null() {
            unsafe { *out_found = found as c_int };
        }
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Like [`pdf_editable_place_text`] but with horizontal `align` (0=Left, 1=Right,
/// 2=Center, 3=Justify) relative to the anchor `(x, y)`. `out_found` receives `1`
/// if the page existed, else `0`.
///
/// # Safety
/// `ed`, `text` valid; `out_found` writable or NULL.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_editable_place_text_aligned(
    ed: *mut PdfEditable,
    index: c_int,
    x: f64,
    y: f64,
    text: *const c_char,
    size: f64,
    r: f64,
    g: f64,
    b: f64,
    rotation_deg: f64,
    align: c_int,
    out_found: *mut c_int,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_place_text_aligned", |d| {
        let text = match unsafe { cstr(text, "place_text_aligned:text") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let found = d.place_text_aligned(
            index.max(0) as usize,
            x,
            y,
            &text,
            size,
            (r, g, b),
            rotation_deg,
            align_from_int(align),
        );
        if !out_found.is_null() {
            unsafe { *out_found = found as c_int };
        }
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Draw `text` over an opaque background box `[x, y, x+width, y+height]`: fills
/// the box in `bg_*` color, then writes the text (standard Helvetica, `size`
/// points, `text_*` color) horizontally aligned per `align` (0=Left, 1=Right,
/// 2=Center, 3=Justify) and vertically centered. Coordinates are in the page's
/// visible space (origin lower-left, y up). `out_found` receives `1` if the page
/// existed, else `0`.
///
/// # Safety
/// `ed`, `text` valid; `out_found` writable or NULL.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_editable_masked_text(
    ed: *mut PdfEditable,
    index: c_int,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    text: *const c_char,
    size: f64,
    text_r: f64,
    text_g: f64,
    text_b: f64,
    bg_r: f64,
    bg_g: f64,
    bg_b: f64,
    align: c_int,
    out_found: *mut c_int,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_masked_text", |d| {
        let text = match unsafe { cstr(text, "masked_text:text") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let found = d.masked_text(
            index.max(0) as usize,
            x,
            y,
            width,
            height,
            &text,
            size,
            (text_r, text_g, text_b),
            (bg_r, bg_g, bg_b),
            align_from_int(align),
        );
        if !out_found.is_null() {
            unsafe { *out_found = found as c_int };
        }
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Draw an image (from in-memory JPEG/PNG bytes `data`/`len`, dispatched on the
/// file signature) on page `index` (0-based) with its lower-left corner at
/// `(x, y)`, scaled to `width`×`height` points, rotated `rotation_deg` degrees
/// counter-clockwise about that corner. Coordinates are in the page's visible
/// space (origin lower-left, y up), honoring `/Rotate`. `out_found` receives
/// `1` if the page existed, else `0`.
///
/// # Safety
/// `ed` valid; `data` points to `len` readable bytes; `out_found` writable or NULL.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_editable_draw_image(
    ed: *mut PdfEditable,
    index: c_int,
    data: *const u8,
    len: usize,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    rotation_deg: f64,
    out_found: *mut c_int,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_draw_image", |d| {
        let buf = unsafe { bytes(data, len) };
        let img = if buf.starts_with(&[0xFF, 0xD8]) {
            Image::from_jpeg(buf.to_vec())
        } else {
            Image::from_png(buf)
        };
        let img = match img {
            Ok(i) => i,
            Err(e) => {
                set_last_error(format!("draw_image load failed: {e}"));
                return PdfStatus::Image;
            }
        };
        let found = d.draw_image(
            index.max(0) as usize,
            &img,
            x,
            y,
            width,
            height,
            rotation_deg,
        );
        if !out_found.is_null() {
            unsafe { *out_found = found as c_int };
        }
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Set the output PDF version (downgrade/normalize): `version` is `0`=1.4,
/// `1`=1.5, `2`=1.7, `3`=2.0. Clears any catalog `/Version` override.
///
/// # Safety
/// `ed` valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_set_version(
    ed: *mut PdfEditable,
    version: c_int,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_set_version", |d| {
        d.set_version(crate::build::version(version));
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Strip PDF/A conformance (catalog `/OutputIntents`, XMP `/Metadata` `pdfaid`,
/// `/Version`) so the file is a plain PDF.
///
/// # Safety
/// `ed` valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_strip_pdfa(ed: *mut PdfEditable) -> PdfStatus {
    with_editable(ed, "pdf_editable_strip_pdfa", |d| {
        d.strip_pdfa();
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Normalize to a plain PDF at `version` (strip PDF/A + set version). `version`
/// codes as in [`pdf_editable_set_version`].
///
/// # Safety
/// `ed` valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_normalize(ed: *mut PdfEditable, version: c_int) -> PdfStatus {
    with_editable(ed, "pdf_editable_normalize", |d| {
        d.normalize(crate::build::version(version));
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Redact rectangular regions on page `index`: `rects` holds `count*4` doubles
/// (x0,y0,x1,y1 per region). The covered content is removed and a black box is
/// drawn. `out_found` (if non-NULL) gets 1 if the page existed.
///
/// # Safety
/// `ed` valid; `rects` points to `count*4` doubles; `out_found` NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_redact(
    ed: *mut PdfEditable,
    index: usize,
    rects: *const f64,
    count: usize,
    out_found: *mut c_int,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_redact", |d| {
        if rects.is_null() && count > 0 {
            set_last_error("redact: null rects");
            return PdfStatus::NullPointer;
        }
        let flat = unsafe { std::slice::from_raw_parts(rects, count * 4) };
        let mut boxes = Vec::with_capacity(count);
        for i in 0..count {
            boxes.push([
                flat[i * 4],
                flat[i * 4 + 1],
                flat[i * 4 + 2],
                flat[i * 4 + 3],
            ]);
        }
        let found = d.redact(index, &boxes);
        if !out_found.is_null() {
            unsafe { *out_found = found as c_int };
        }
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Convert the loaded document to PDF/A at `level` (0=A-1b, 1=A-2b, 3=A-3b).
/// Fails if fonts are not embedded or a level-A profile is requested.
///
/// # Safety
/// `ed` must be valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_convert_to_pdfa(
    ed: *mut PdfEditable,
    level: c_int,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_convert_to_pdfa", |d| {
        match d.convert_to_pdfa(pdfa_level(level)) {
            Ok(()) => {
                clear_last_error();
                PdfStatus::Ok
            }
            Err(ConvertError::License(e)) => {
                set_last_error(format!("convert_to_pdfa: {e}"));
                PdfStatus::License
            }
            Err(e) => {
                set_last_error(format!("convert_to_pdfa: {e}"));
                PdfStatus::InvalidArgument
            }
        }
    })
}

/// Drop unreferenced objects, recompress, dedupe and emit object streams on save.
///
/// # Safety
/// `ed` must be valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_optimize(ed: *mut PdfEditable) -> PdfStatus {
    with_editable(ed, "pdf_editable_optimize", |d| {
        d.optimize();
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Toggle object streams + cross-reference stream output on save.
///
/// # Safety
/// `ed` must be valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_compact(ed: *mut PdfEditable, on: c_int) -> PdfStatus {
    with_editable(ed, "pdf_editable_compact", |d| {
        d.compact(on != 0);
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Encrypt on save. `method`: 0=RC4-128, 1=AES-128, 2=AES-256/R6.
/// `read_only` != 0 applies the read-only permission set.
///
/// # Safety
/// `ed`, `user`, `owner` valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_encrypt(
    ed: *mut PdfEditable,
    method: c_int,
    user: *const c_char,
    owner: *const c_char,
    read_only: c_int,
) -> PdfStatus {
    with_editable(ed, "pdf_editable_encrypt", |d| {
        let user = match unsafe { cstr(user, "encrypt:user") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let owner = match unsafe { cstr(owner, "encrypt:owner") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let perms = if read_only != 0 {
            Permissions::read_only()
        } else {
            Permissions::default()
        };
        d.encrypt_with(encryption(method), &user, &owner, perms);
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Serialize to a buffer (`out_ptr`/`out_len`), freed by `pdf_buffer_free`.
///
/// # Safety
/// `ed`, `out_ptr`, `out_len` valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_to_bytes(
    ed: *const PdfEditable,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(|| {
        let Some(ed) = (unsafe { ed.as_ref() }) else {
            set_last_error("to_bytes: null handle");
            return PdfStatus::NullPointer;
        };
        match ed.inner.to_bytes() {
            Ok(b) => unsafe { emit_buffer(b, out_ptr, out_len) },
            Err(e) => {
                set_last_error(format!("serialize failed: {e}"));
                build_status(&e)
            }
        }
    })
}

/// Map a `BuildError` to a status code (license errors are distinguished so the
/// caller can tell "needs a license" from a generic serialization failure).
fn build_status(e: &pdf::BuildError) -> PdfStatus {
    match e {
        pdf::BuildError::License(_) => PdfStatus::License,
        _ => PdfStatus::Serialize,
    }
}

/// Serialize as an incremental update over `original` (preserves it verbatim).
///
/// # Safety
/// `ed`, `original`, `out_ptr`, `out_len` valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_to_bytes_incremental(
    ed: *const PdfEditable,
    original: *const u8,
    original_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(|| {
        let Some(ed) = (unsafe { ed.as_ref() }) else {
            set_last_error("to_bytes_incremental: null handle");
            return PdfStatus::NullPointer;
        };
        match ed
            .inner
            .to_bytes_incremental(unsafe { bytes(original, original_len) })
        {
            Ok(b) => unsafe { emit_buffer(b, out_ptr, out_len) },
            Err(e) => {
                set_last_error(format!("incremental update failed: {e}"));
                build_status(&e)
            }
        }
    })
}

/// Save to `path`.
///
/// # Safety
/// `ed`, `path` valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_editable_save(
    ed: *const PdfEditable,
    path: *const c_char,
) -> PdfStatus {
    guard(|| {
        let Some(ed) = (unsafe { ed.as_ref() }) else {
            set_last_error("save: null handle");
            return PdfStatus::NullPointer;
        };
        let path = match unsafe { cstr(path, "pdf_editable_save") } {
            Ok(p) => p,
            Err(st) => return st,
        };
        match ed.inner.save(path) {
            Ok(()) => {
                clear_last_error();
                PdfStatus::Ok
            }
            Err(e) => {
                set_last_error(format!("save failed: {e}"));
                PdfStatus::Io
            }
        }
    })
}

/// Extract the document's text into a UTF-8 buffer (`out_ptr`/`out_len`).
///
/// # Safety
/// `data`/`len` readable; `out_ptr`/`out_len` writable.
#[no_mangle]
pub unsafe extern "C" fn pdf_extract_text(
    data: *const u8,
    len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(|| match pdf::extract_text(unsafe { bytes(data, len) }) {
        Ok(text) => unsafe { emit_buffer(text.into_bytes(), out_ptr, out_len) },
        Err(e) => {
            set_last_error(format!("extract_text failed: {e}"));
            PdfStatus::Parse
        }
    })
}

/// Extract the text of a single page (0-based `page_index`) into a UTF-8 buffer
/// (`out_ptr`/`out_len`). Returns `PdfStatus::InvalidArgument` if the page is
/// out of range (no buffer is written).
///
/// # Safety
/// `data`/`len` readable; `out_ptr`/`out_len` writable.
#[no_mangle]
pub unsafe extern "C" fn pdf_extract_page_text(
    data: *const u8,
    len: usize,
    page_index: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(
        || match pdf::extract_page_text(unsafe { bytes(data, len) }, page_index) {
            Ok(Some(text)) => unsafe { emit_buffer(text.into_bytes(), out_ptr, out_len) },
            Ok(None) => {
                set_last_error(format!("extract_page_text: page {page_index} out of range"));
                PdfStatus::InvalidArgument
            }
            Err(e) => {
                set_last_error(format!("extract_page_text failed: {e}"));
                PdfStatus::Parse
            }
        },
    )
}

/// Find every occurrence of `query` in `data`/`len` and write a JSON array of
/// bounding boxes into `out_ptr`/`out_len` (freed with [`pdf_buffer_free`]).
/// Each element is `{"page":int,"text":str,"x":num,"y":num,"width":num,
/// "height":num}` with coordinates in PDF user space (points, origin
/// lower-left). `case_sensitive` is `0` for case-insensitive matching, non-zero
/// for exact. An empty array `[]` means no match.
///
/// # Safety
/// `data`/`len` readable; `query` a valid NUL-terminated UTF-8 string;
/// `out_ptr`/`out_len` writable.
#[no_mangle]
pub unsafe extern "C" fn pdf_find_text_json(
    data: *const u8,
    len: usize,
    query: *const c_char,
    case_sensitive: c_int,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(|| {
        let query = match unsafe { cstr(query, "pdf_find_text_json") } {
            Ok(q) => q,
            Err(s) => return s,
        };
        let opts = pdf::FindOptions {
            case_sensitive: case_sensitive != 0,
        };
        match pdf::find_text(unsafe { bytes(data, len) }, query, opts) {
            Ok(hits) => unsafe { emit_buffer(hits_to_json(&hits).into_bytes(), out_ptr, out_len) },
            Err(e) => {
                set_last_error(format!("find_text failed: {e}"));
                PdfStatus::Parse
            }
        }
    })
}

fn hits_to_json(hits: &[pdf::TextHit]) -> String {
    let mut s = String::from("[");
    for (i, h) in hits.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"page\":{p},\"text\":\"{t}\",\"x\":{x},\"y\":{y},\"width\":{w},\"height\":{h}}}",
            p = h.page,
            t = crate::verify::json_escape(&h.text),
            x = h.x,
            y = h.y,
            w = h.width,
            h = h.height,
        ));
    }
    s.push(']');
    s
}

/// Read per-page geometry from `data`/`len` and write a JSON array into
/// `out_ptr`/`out_len` (freed with [`pdf_buffer_free`]). Each element is
/// `{"page":int,"width":num,"height":num,"rotation":int,"rotatedWidth":num,
/// "rotatedHeight":num,"mediaBox":[x0,y0,x1,y1],"cropBox":[x0,y0,x1,y1]}` with
/// coordinates in PDF points. Sizes are unrotated; `rotatedWidth`/`Height` are
/// swapped for 90/270 pages.
///
/// # Safety
/// `data`/`len` readable; `out_ptr`/`out_len` writable.
#[no_mangle]
pub unsafe extern "C" fn pdf_measure_pages_json(
    data: *const u8,
    len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(|| match pdf::measure_pages(unsafe { bytes(data, len) }) {
        Ok(pages) => unsafe {
            emit_buffer(geometry_to_json(&pages).into_bytes(), out_ptr, out_len)
        },
        Err(e) => {
            set_last_error(format!("measure_pages failed: {e}"));
            PdfStatus::Parse
        }
    })
}

fn geometry_to_json(pages: &[pdf::PageGeometry]) -> String {
    let rect = |r: &pdf::PdfRect| format!("[{},{},{},{}]", r.x0, r.y0, r.x1, r.y1);
    let mut s = String::from("[");
    for (i, p) in pages.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"page\":{page},\"width\":{w},\"height\":{h},\"rotation\":{rot},\
             \"rotatedWidth\":{rw},\"rotatedHeight\":{rh},\"mediaBox\":{mb},\"cropBox\":{cb}}}",
            page = p.page,
            w = p.width,
            h = p.height,
            rot = p.rotation,
            rw = p.rotated_width,
            rh = p.rotated_height,
            mb = rect(&p.media_box),
            cb = rect(&p.crop_box),
        ));
    }
    s.push(']');
    s
}

/// Inspect `data`/`len` without mutating it and write a JSON object into
/// `out_ptr`/`out_len` (freed with [`pdf_buffer_free`]):
/// `{"version":str,"pdfaLevel":str|null,"encrypted":bool,"encryption":str,
/// "requiresPassword":bool,"pageCount":int}`. Never fails on a password-locked
/// file.
///
/// # Safety
/// `data`/`len` readable; `out_ptr`/`out_len` writable.
#[no_mangle]
pub unsafe extern "C" fn pdf_inspect_json(
    data: *const u8,
    len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(|| {
        let o = pdf::inspect(unsafe { bytes(data, len) });
        let pdfa = match &o.pdfa_level {
            Some(l) => format!("\"{}\"", crate::verify::json_escape(l)),
            None => "null".to_string(),
        };
        let json = format!(
            "{{\"version\":\"{v}\",\"pdfaLevel\":{pdfa},\"encrypted\":{enc},\
             \"encryption\":\"{cipher}\",\"requiresPassword\":{rp},\"pageCount\":{pc}}}",
            v = crate::verify::json_escape(&o.version),
            enc = o.encrypted,
            cipher = crate::verify::json_escape(&o.encryption),
            rp = o.requires_password,
            pc = o.page_count,
        );
        unsafe { emit_buffer(json.into_bytes(), out_ptr, out_len) }
    })
}

/// Extract every raster image from `data`/`len` and write each one as a file
/// into the directory `dir`. JPEG (`DCTDecode`) images are written verbatim as
/// `.jpg`; everything else is re-encoded as `.png`. Files are named
/// `page{N}_{name}.{ext}`. The number written is stored in `out_count`.
///
/// # Safety
/// `data`/`len` readable; `dir` a valid NUL-terminated UTF-8 path to an existing
/// directory; `out_count` writable (or NULL to ignore the count).
#[no_mangle]
pub unsafe extern "C" fn pdf_extract_images_to_dir(
    data: *const u8,
    len: usize,
    dir: *const c_char,
    out_count: *mut usize,
) -> PdfStatus {
    guard(|| {
        let dir = match unsafe { cstr(dir, "pdf_extract_images_to_dir") } {
            Ok(d) => d,
            Err(s) => return s,
        };
        let images = match pdf::extract_images(unsafe { bytes(data, len) }) {
            Ok(v) => v,
            Err(e) => {
                set_last_error(format!("extract_images failed: {e}"));
                return PdfStatus::Parse;
            }
        };
        for img in &images {
            if let Err(e) = img.save_in(dir) {
                set_last_error(format!("extract_images write failed: {e}"));
                return PdfStatus::Io;
            }
        }
        if !out_count.is_null() {
            unsafe { *out_count = images.len() };
        }
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Render page `page_index` (0-based) of the PDF in `data`/`len` to a PNG image
/// at `dpi` dots-per-inch. The PNG bytes are returned in `out_ptr`/`out_len`,
/// to be released with [`pdf_buffer_free`].
///
/// # Safety
/// `data`/`len` readable; `out_ptr`/`out_len` writable, non-aliasing.
#[no_mangle]
pub unsafe extern "C" fn pdf_render_page_to_png(
    data: *const u8,
    len: usize,
    page_index: usize,
    dpi: c_double,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(
        || match pdf::render_page_to_png(unsafe { bytes(data, len) }, page_index, dpi as f32) {
            Ok(png) => unsafe { emit_buffer(png, out_ptr, out_len) },
            Err(pdf::PageRenderError::License(_)) => {
                set_last_error("render_page_to_png requires a license (Pro feature)");
                PdfStatus::License
            }
            Err(e) => {
                set_last_error(format!("render_page_to_png failed: {e}"));
                PdfStatus::Parse
            }
        },
    )
}

/// Number of pages in the PDF in `data`/`len`, written to `out_count`.
///
/// # Safety
/// `data`/`len` readable; `out_count` writable.
#[no_mangle]
pub unsafe extern "C" fn pdf_page_count(
    data: *const u8,
    len: usize,
    out_count: *mut usize,
) -> PdfStatus {
    guard(
        || match pdf::render_page_count(unsafe { bytes(data, len) }) {
            Ok(count) => {
                if !out_count.is_null() {
                    unsafe { *out_count = count };
                }
                clear_last_error();
                PdfStatus::Ok
            }
            Err(e) => {
                set_last_error(format!("page_count failed: {e}"));
                PdfStatus::Parse
            }
        },
    )
}

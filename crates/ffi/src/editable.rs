//! C ABI: PDF manipulation (`pdf::EditableDoc`) and text extraction.
//!
//! `PdfEditable` is its own opaque handle, freed with [`pdf_editable_free`].

use std::ffi::{c_char, c_int, c_uchar};
use std::panic::{catch_unwind, AssertUnwindSafe};

use pdf::{EditableDoc, Encryption, Permissions};

use crate::{bytes, clear_last_error, cstr, emit_buffer, guard, set_last_error, PdfStatus};

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
                PdfStatus::Serialize
            }
        }
    })
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
                PdfStatus::Serialize
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

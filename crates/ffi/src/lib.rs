//! C ABI boundary for the PDF core — the **only** layer that crosses the FFI
//! frontier (`project.md` §1.4). Everything here obeys the boundary rules:
//!
//! * Opaque handles (`*mut PdfDocument`), never Rust structs.
//! * No generics/lifetimes/traits/`Result`/enums-with-data cross the line.
//! * Errors via [`PdfStatus`] return codes plus a thread-local last-error
//!   string retrievable with [`pdf_last_error_message`].
//! * Every export is wrapped in [`catch_unwind`]: a panic never escapes.
//! * Whoever creates a handle frees it ([`pdf_document_free`]); buffers from
//!   [`pdf_document_write`] are freed with [`pdf_buffer_free`].
//!
//! The high-level Rust API stays rich and unrestricted; this crate merely
//! translates a deliberately small, stable surface to C.

use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_uchar, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

use pdf::Document;

mod build;
mod editable;
mod signing;
mod verify;

/// Status code returned by every fallible export.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfStatus {
    Ok = 0,
    NullPointer = 1,
    InvalidUtf8 = 2,
    Io = 3,
    Serialize = 4,
    Panic = 5,
    /// A document/argument could not be parsed.
    Parse = 6,
    /// A font could not be loaded/embedded.
    Font = 7,
    /// An image could not be decoded/embedded.
    Image = 8,
    /// Encryption setup failed.
    Encrypt = 9,
    /// Digital signing failed.
    Sign = 10,
    /// An out-of-range index or other invalid argument.
    InvalidArgument = 11,
    /// License activation failed (bad signature, expired, or malformed).
    License = 12,
    /// The operation cannot be performed safely on this input (e.g. redaction
    /// of a page whose content cannot be rewritten) — see the last error.
    Unsupported = 13,
}

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

pub(crate) fn set_last_error(msg: impl Into<String>) {
    let cstring = CString::new(msg.into()).unwrap_or_else(|_| CString::new("error").unwrap());
    LAST_ERROR.with(|slot| *slot.borrow_mut() = Some(cstring));
}

pub(crate) fn clear_last_error() {
    LAST_ERROR.with(|slot| *slot.borrow_mut() = None);
}

/// Run `f` under `catch_unwind`, mapping a panic to [`PdfStatus::Panic`].
pub(crate) fn guard<F: FnOnce() -> PdfStatus>(f: F) -> PdfStatus {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(status) => status,
        Err(_) => {
            set_last_error("panic caught at FFI boundary");
            PdfStatus::Panic
        }
    }
}

/// Borrow a NUL-terminated C string as `&str`, or fail with a clear error.
///
/// # Safety
/// `ptr` must be NULL or a valid NUL-terminated C string.
pub(crate) unsafe fn cstr<'a>(ptr: *const c_char, who: &str) -> Result<&'a str, PdfStatus> {
    if ptr.is_null() {
        set_last_error(format!("{who}: null string"));
        return Err(PdfStatus::NullPointer);
    }
    unsafe { CStr::from_ptr(ptr) }.to_str().map_err(|_| {
        set_last_error(format!("{who}: not valid UTF-8"));
        PdfStatus::InvalidUtf8
    })
}

/// Borrow a `(ptr, len)` byte buffer as a slice (empty if `len == 0`).
///
/// # Safety
/// `ptr`/`len` must describe a valid readable region, or `ptr` may be NULL with
/// `len == 0`.
pub(crate) unsafe fn bytes<'a>(ptr: *const c_uchar, len: usize) -> &'a [u8] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }
}

/// Hand a `Vec<u8>` to C as a freshly-allocated buffer (`out_ptr`/`out_len`),
/// to be released by [`pdf_buffer_free`].
///
/// # Safety
/// `out_ptr`/`out_len` must be valid, non-aliasing writable pointers.
pub(crate) unsafe fn emit_buffer(
    data: Vec<u8>,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    if out_ptr.is_null() || out_len.is_null() {
        set_last_error("null out parameter");
        return PdfStatus::NullPointer;
    }
    let mut boxed = data.into_boxed_slice();
    let len = boxed.len();
    let ptr = boxed.as_mut_ptr();
    std::mem::forget(boxed);
    unsafe {
        *out_ptr = ptr;
        *out_len = len;
    }
    clear_last_error();
    PdfStatus::Ok
}

/// Opaque document handle. The C side only ever sees `*mut PdfDocument`.
pub struct PdfDocument {
    pub(crate) inner: Document,
}

/// Apply a *consuming* builder (`pdfa`, `tagged`, …) to the wrapped document by
/// swapping it out, transforming it, and putting it back.
pub(crate) fn transform_doc<F>(doc: *mut PdfDocument, who: &str, f: F) -> PdfStatus
where
    F: FnOnce(Document) -> Document,
{
    guard(|| {
        let Some(d) = (unsafe { doc.as_mut() }) else {
            set_last_error(format!("{who}: null document"));
            return PdfStatus::NullPointer;
        };
        let taken = std::mem::replace(&mut d.inner, Document::new());
        d.inner = f(taken);
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Borrow the wrapped `Document` mutably for a non-consuming operation.
pub(crate) fn with_doc<F>(doc: *mut PdfDocument, who: &str, f: F) -> PdfStatus
where
    F: FnOnce(&mut Document) -> PdfStatus,
{
    guard(|| {
        let Some(d) = (unsafe { doc.as_mut() }) else {
            set_last_error(format!("{who}: null document"));
            return PdfStatus::NullPointer;
        };
        f(&mut d.inner)
    })
}

/// Library version as a static, NUL-terminated C string. Never freed by the
/// caller.
///
/// # Safety
/// The returned pointer is valid for the lifetime of the program.
#[no_mangle]
pub extern "C" fn pdf_version() -> *const c_char {
    // A 'static CStr backed by a compile-time NUL-terminated literal.
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}

/// Return the last error message for the current thread, or NULL if none.
/// The pointer is owned by the library and valid until the next FFI call on
/// this thread.
///
/// # Safety
/// Do not free the returned pointer or use it across other FFI calls.
#[no_mangle]
pub extern "C" fn pdf_last_error_message() -> *const c_char {
    LAST_ERROR.with(|slot| match &*slot.borrow() {
        Some(s) => s.as_ptr(),
        None => ptr::null(),
    })
}

/// Activate a license token for this process, unlocking the corporate features
/// it grants (PDF/A, signatures, encryption, accessibility) until it expires.
/// Returns [`PdfStatus::License`] if the token is forged, expired or malformed.
///
/// # Safety
/// `token` must be a valid NUL-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn pdf_activate_license(token: *const c_char) -> PdfStatus {
    guard(|| {
        let token = match unsafe { cstr(token, "pdf_activate_license") } {
            Ok(t) => t,
            Err(s) => return s,
        };
        match pdf::activate_license(token) {
            Ok(_) => {
                clear_last_error();
                PdfStatus::Ok
            }
            Err(e) => {
                set_last_error(format!("license: {e}"));
                PdfStatus::License
            }
        }
    })
}

/// Create a new, empty A4 document. Returns NULL on allocation failure.
///
/// # Safety
/// The returned handle must eventually be passed to [`pdf_document_free`].
#[no_mangle]
pub extern "C" fn pdf_document_new() -> *mut PdfDocument {
    let result = catch_unwind(|| {
        Box::into_raw(Box::new(PdfDocument {
            inner: Document::new(),
        }))
    });
    match result {
        Ok(ptr) => {
            clear_last_error();
            ptr
        }
        Err(_) => {
            set_last_error("panic while creating document");
            ptr::null_mut()
        }
    }
}

/// Free a document handle. Passing NULL is a no-op.
///
/// # Safety
/// `doc` must have come from [`pdf_document_new`] and not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn pdf_document_free(doc: *mut PdfDocument) {
    if doc.is_null() {
        return;
    }
    // Reconstitute the Box and drop it; ignore any panic in Drop.
    let _ = catch_unwind(AssertUnwindSafe(|| {
        drop(unsafe { Box::from_raw(doc) });
    }));
}

/// Append a page using the document's default size (A4).
///
/// # Safety
/// `doc` must be a valid handle from [`pdf_document_new`].
#[no_mangle]
pub unsafe extern "C" fn pdf_document_add_page(doc: *mut PdfDocument) -> PdfStatus {
    guard(|| {
        let Some(doc) = (unsafe { doc.as_mut() }) else {
            set_last_error("pdf_document_add_page: null document");
            return PdfStatus::NullPointer;
        };
        doc.inner.add_page();
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Set the fill color (DeviceRGB, components in 0..=1) on the current page.
///
/// # Safety
/// `doc` must be valid and have at least one page.
#[no_mangle]
pub unsafe extern "C" fn pdf_page_set_fill_rgb(
    doc: *mut PdfDocument,
    r: f64,
    g: f64,
    b: f64,
) -> PdfStatus {
    with_current_page(doc, "pdf_page_set_fill_rgb", |page| {
        page.content().set_fill_rgb(r, g, b);
    })
}

/// Set the stroke color (DeviceRGB) on the current page.
///
/// # Safety
/// `doc` must be valid and have at least one page.
#[no_mangle]
pub unsafe extern "C" fn pdf_page_set_stroke_rgb(
    doc: *mut PdfDocument,
    r: f64,
    g: f64,
    b: f64,
) -> PdfStatus {
    with_current_page(doc, "pdf_page_set_stroke_rgb", |page| {
        page.content().set_stroke_rgb(r, g, b);
    })
}

/// Set the line width on the current page.
///
/// # Safety
/// `doc` must be valid and have at least one page.
#[no_mangle]
pub unsafe extern "C" fn pdf_page_set_line_width(doc: *mut PdfDocument, width: f64) -> PdfStatus {
    with_current_page(doc, "pdf_page_set_line_width", |page| {
        page.content().set_line_width(width);
    })
}

/// Append a rectangle subpath on the current page.
///
/// # Safety
/// `doc` must be valid and have at least one page.
#[no_mangle]
pub unsafe extern "C" fn pdf_page_rect(
    doc: *mut PdfDocument,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> PdfStatus {
    with_current_page(doc, "pdf_page_rect", |page| {
        page.content().rect(x, y, width, height);
    })
}

/// Fill the current path (nonzero winding) on the current page.
///
/// # Safety
/// `doc` must be valid and have at least one page.
#[no_mangle]
pub unsafe extern "C" fn pdf_page_fill(doc: *mut PdfDocument) -> PdfStatus {
    with_current_page(doc, "pdf_page_fill", |page| {
        page.content().fill();
    })
}

/// Stroke the current path on the current page.
///
/// # Safety
/// `doc` must be valid and have at least one page.
#[no_mangle]
pub unsafe extern "C" fn pdf_page_stroke(doc: *mut PdfDocument) -> PdfStatus {
    with_current_page(doc, "pdf_page_stroke", |page| {
        page.content().stroke();
    })
}

/// Serialize the document and write it to `path` (a UTF-8, NUL-terminated C
/// string).
///
/// # Safety
/// `doc` must be valid; `path` must be a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn pdf_document_save(
    doc: *mut PdfDocument,
    path: *const c_char,
) -> PdfStatus {
    guard(|| {
        let Some(doc) = (unsafe { doc.as_ref() }) else {
            set_last_error("pdf_document_save: null document");
            return PdfStatus::NullPointer;
        };
        if path.is_null() {
            set_last_error("pdf_document_save: null path");
            return PdfStatus::NullPointer;
        }
        let path = match unsafe { CStr::from_ptr(path) }.to_str() {
            Ok(p) => p,
            Err(_) => {
                set_last_error("pdf_document_save: path is not valid UTF-8");
                return PdfStatus::InvalidUtf8;
            }
        };
        match doc.inner.save(path) {
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

/// Serialize the document into a freshly-allocated buffer. On success writes
/// the pointer to `*out_ptr` and length to `*out_len`; the caller must release
/// it with [`pdf_buffer_free`].
///
/// # Safety
/// `doc`, `out_ptr` and `out_len` must be valid, non-aliasing pointers.
#[no_mangle]
pub unsafe extern "C" fn pdf_document_write(
    doc: *mut PdfDocument,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(|| {
        let Some(doc) = (unsafe { doc.as_ref() }) else {
            set_last_error("pdf_document_write: null document");
            return PdfStatus::NullPointer;
        };
        if out_ptr.is_null() || out_len.is_null() {
            set_last_error("pdf_document_write: null out parameter");
            return PdfStatus::NullPointer;
        }
        match doc.inner.to_bytes() {
            Ok(bytes) => {
                let mut boxed = bytes.into_boxed_slice();
                let len = boxed.len();
                let ptr = boxed.as_mut_ptr();
                std::mem::forget(boxed);
                unsafe {
                    *out_ptr = ptr;
                    *out_len = len;
                }
                clear_last_error();
                PdfStatus::Ok
            }
            Err(e) => {
                set_last_error(format!("serialize failed: {e}"));
                PdfStatus::Serialize
            }
        }
    })
}

/// Free a buffer returned by [`pdf_document_write`]. NULL is a no-op.
///
/// # Safety
/// `ptr`/`len` must be exactly the pair produced by [`pdf_document_write`].
#[no_mangle]
pub unsafe extern "C" fn pdf_buffer_free(ptr: *mut c_uchar, len: usize) {
    if ptr.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        drop(Vec::from_raw_parts(ptr, len, len));
    }));
}

/// Number of pages in the document, or -1 on a null handle.
///
/// # Safety
/// `doc` must be a valid handle or NULL.
#[no_mangle]
pub unsafe extern "C" fn pdf_document_page_count(doc: *const PdfDocument) -> c_int {
    match catch_unwind(AssertUnwindSafe(|| unsafe { doc.as_ref() })) {
        Ok(Some(doc)) => doc.inner.page_count() as c_int,
        _ => -1,
    }
}

/// Shared helper: run `f` against the document's last (current) page.
pub(crate) fn with_current_page<F>(doc: *mut PdfDocument, who: &str, f: F) -> PdfStatus
where
    F: FnOnce(&mut pdf::Page),
{
    guard(|| {
        let Some(doc) = (unsafe { doc.as_mut() }) else {
            set_last_error(format!("{who}: null document"));
            return PdfStatus::NullPointer;
        };
        let count = doc.inner.page_count();
        if count == 0 {
            set_last_error(format!("{who}: document has no pages"));
            return PdfStatus::NullPointer;
        }
        // Re-borrow the last page mutably.
        let page = last_page_mut(&mut doc.inner);
        f(page);
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Borrow the last page mutably. Adds a page only if there are none (callers
/// guarantee at least one before this is reached).
fn last_page_mut(doc: &mut Document) -> &mut pdf::Page {
    // `add_page` returns the new page; to get the existing last page we rely
    // on the high-level API exposing it. We re-add nothing here: the public
    // API returns the just-added page, so we expose a dedicated accessor.
    doc.last_page_mut().expect("checked non-empty by caller")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn version_is_nul_terminated() {
        let p = pdf_version();
        let s = unsafe { CStr::from_ptr(p) }.to_str().unwrap();
        assert_eq!(s, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn null_handles_report_errors_not_panics() {
        assert_eq!(
            unsafe { pdf_document_add_page(ptr::null_mut()) },
            PdfStatus::NullPointer
        );
        let msg = unsafe { CStr::from_ptr(pdf_last_error_message()) }
            .to_str()
            .unwrap();
        assert!(msg.contains("null document"));
    }

    #[test]
    fn full_round_trip_through_ffi() {
        let doc = pdf_document_new();
        assert!(!doc.is_null());
        assert_eq!(unsafe { pdf_document_add_page(doc) }, PdfStatus::Ok);
        assert_eq!(
            unsafe { pdf_page_set_fill_rgb(doc, 1.0, 0.0, 0.0) },
            PdfStatus::Ok
        );
        assert_eq!(
            unsafe { pdf_page_rect(doc, 0.0, 0.0, 100.0, 100.0) },
            PdfStatus::Ok
        );
        assert_eq!(unsafe { pdf_page_fill(doc) }, PdfStatus::Ok);
        assert_eq!(unsafe { pdf_document_page_count(doc) }, 1);

        let mut ptr_out: *mut c_uchar = ptr::null_mut();
        let mut len_out: usize = 0;
        assert_eq!(
            unsafe { pdf_document_write(doc, &mut ptr_out, &mut len_out) },
            PdfStatus::Ok
        );
        assert!(!ptr_out.is_null() && len_out > 0);
        let bytes = unsafe { std::slice::from_raw_parts(ptr_out, len_out) };
        assert!(bytes.starts_with(b"%PDF-1.7"));
        unsafe { pdf_buffer_free(ptr_out, len_out) };

        // Save path exercise.
        let tmp = std::env::temp_dir().join("ffi_roundtrip.pdf");
        let cpath = CString::new(tmp.to_str().unwrap()).unwrap();
        assert_eq!(
            unsafe { pdf_document_save(doc, cpath.as_ptr()) },
            PdfStatus::Ok
        );

        unsafe { pdf_document_free(doc) };
    }

    #[test]
    fn editable_and_extract_round_trip() {
        use crate::editable::*;

        // Build a small doc through the building surface.
        let doc = pdf_document_new();
        assert_eq!(unsafe { pdf_document_add_page(doc) }, PdfStatus::Ok);
        // A font + a line of text so extraction has something to find.
        let font_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/fonts/Roboto-Regular.ttf\0"
        );
        let mut fid: c_int = -1;
        assert_eq!(
            unsafe {
                crate::build::pdf_document_add_font_file(
                    doc,
                    font_path.as_ptr() as *const c_char,
                    &mut fid,
                )
            },
            PdfStatus::Ok
        );
        let text = CString::new("Olá FFI").unwrap();
        assert_eq!(
            unsafe {
                crate::build::pdf_page_show_text(doc, fid, 16.0, 72.0, 700.0, text.as_ptr(), 0)
            },
            PdfStatus::Ok
        );
        let mut p: *mut c_uchar = ptr::null_mut();
        let mut n: usize = 0;
        assert_eq!(
            unsafe { pdf_document_write(doc, &mut p, &mut n) },
            PdfStatus::Ok
        );
        let pdf_bytes = unsafe { std::slice::from_raw_parts(p, n) }.to_vec();
        unsafe { pdf_buffer_free(p, n) };
        unsafe { pdf_document_free(doc) };

        // Load it as editable, set info, re-serialize.
        let ed = unsafe { pdf_editable_load(pdf_bytes.as_ptr(), pdf_bytes.len()) };
        assert!(!ed.is_null());
        assert_eq!(unsafe { pdf_editable_page_count(ed) }, 1);
        let key = CString::new("Title").unwrap();
        let val = CString::new("Editado FFI").unwrap();
        assert_eq!(
            unsafe { pdf_editable_set_info(ed, key.as_ptr(), val.as_ptr()) },
            PdfStatus::Ok
        );
        let mut p2: *mut c_uchar = ptr::null_mut();
        let mut n2: usize = 0;
        assert_eq!(
            unsafe { pdf_editable_to_bytes(ed, &mut p2, &mut n2) },
            PdfStatus::Ok
        );
        let edited = unsafe { std::slice::from_raw_parts(p2, n2) }.to_vec();
        unsafe { pdf_buffer_free(p2, n2) };
        unsafe { pdf_editable_free(ed) };

        // Extract text from the edited bytes via the free function.
        let mut tp: *mut c_uchar = ptr::null_mut();
        let mut tn: usize = 0;
        assert_eq!(
            unsafe { pdf_extract_text(edited.as_ptr(), edited.len(), &mut tp, &mut tn) },
            PdfStatus::Ok
        );
        let extracted = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(tp, tn) });
        assert!(extracted.contains("Olá FFI"), "got: {extracted:?}");
        unsafe { pdf_buffer_free(tp, tn) };
    }
}

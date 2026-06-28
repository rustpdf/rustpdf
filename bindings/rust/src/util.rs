//! Internal helpers shared by the safe wrappers.

use std::ffi::{CStr, CString};
use std::os::raw::c_int;

use crate::error::{PdfError, PdfStatus, Result};
use crate::ffi::Api;

/// The engine's thread-local last-error message (empty if none).
pub(crate) fn last_error(a: &Api) -> String {
    // SAFETY: `pdf_last_error_message` returns a NUL-terminated string owned by
    // the library, valid until the next FFI call on this thread.
    unsafe {
        let p = (a.pdf_last_error_message)();
        if p.is_null() {
            String::new()
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

/// Turn a `PdfStatus` code into `Ok(())`/`Err`, attaching the last-error text.
pub(crate) fn check(a: &Api, code: c_int) -> Result<()> {
    let status = PdfStatus::from_code(code);
    if status == PdfStatus::Ok {
        Ok(())
    } else {
        Err(PdfError::new(status, last_error(a)))
    }
}

/// Copy an out-buffer `(ptr, len)` produced by the engine into a `Vec`, then
/// release it with `pdf_buffer_free`.
pub(crate) fn take_buffer(a: &Api, ptr: *mut u8, len: usize) -> Vec<u8> {
    if ptr.is_null() || len == 0 {
        if !ptr.is_null() {
            // SAFETY: still our buffer to free even when empty.
            unsafe { (a.pdf_buffer_free)(ptr, len) };
        }
        return Vec::new();
    }
    // SAFETY: `(ptr, len)` is exactly the pair the engine just handed back.
    unsafe {
        let v = std::slice::from_raw_parts(ptr, len).to_vec();
        (a.pdf_buffer_free)(ptr, len);
        v
    }
}

/// Build a `CString`, mapping an interior NUL to `InvalidArgument`.
pub(crate) fn cstr(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| {
        PdfError::new(
            PdfStatus::InvalidArgument,
            "string argument contains an interior NUL byte",
        )
    })
}

/// Build an optional `CString` (for arguments that accept NULL).
pub(crate) fn opt_cstr(s: Option<&str>) -> Result<Option<CString>> {
    match s {
        Some(s) => Ok(Some(cstr(s)?)),
        None => Ok(None),
    }
}

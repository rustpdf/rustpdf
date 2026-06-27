//! C ABI: digital signatures and PAdES LTV (`pdf::sign`/`timestamp`/`add_dss`).

use std::ffi::{c_char, c_int, c_uchar};

use pdf::{sign, timestamp, SignOptions, Signer};

use crate::{bytes, emit_buffer, guard, set_last_error, PdfStatus};

/// # Safety
/// `p` must be NULL or a valid C string.
unsafe fn opt(p: *const c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    unsafe { std::ffi::CStr::from_ptr(p) }
        .to_str()
        .ok()
        .map(str::to_string)
}

/// Sign `pdf` with a PKCS#8 DER private key + DER certificate, producing a new
/// PDF (incremental update) in `out_ptr`/`out_len`. `reason`/`location`/`name`
/// may be NULL; `pades` != 0 selects PAdES-B-B.
///
/// # Safety
/// All `(ptr, len)` pairs readable; string args NULL or valid C strings;
/// `out_ptr`/`out_len` writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_sign(
    pdf: *const u8,
    pdf_len: usize,
    key_der: *const u8,
    key_len: usize,
    cert_der: *const u8,
    cert_len: usize,
    reason: *const c_char,
    location: *const c_char,
    name: *const c_char,
    pades: c_int,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(|| {
        let signer = match Signer::from_pkcs8_der(unsafe { bytes(key_der, key_len) }, unsafe {
            bytes(cert_der, cert_len)
        }) {
            Ok(s) => s,
            Err(e) => {
                set_last_error(format!("signer setup failed: {e}"));
                return PdfStatus::Sign;
            }
        };
        let opts = SignOptions {
            reason: unsafe { opt(reason) },
            location: unsafe { opt(location) },
            name: unsafe { opt(name) },
            date: None,
            visible: None,
            pades: pades != 0,
        };
        match sign(unsafe { bytes(pdf, pdf_len) }, &signer, &opts) {
            Ok(b) => unsafe { emit_buffer(b, out_ptr, out_len) },
            Err(e) => {
                set_last_error(format!("sign failed: {e}"));
                PdfStatus::Sign
            }
        }
    })
}

/// Append a document timestamp (`/DocTimeStamp`, PAdES-B-LTA) using a TSA key +
/// cert. `date` may be NULL (a fixed reproducible value is used).
///
/// # Safety
/// All `(ptr, len)` pairs readable; `out_ptr`/`out_len` writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_timestamp(
    pdf: *const u8,
    pdf_len: usize,
    key_der: *const u8,
    key_len: usize,
    cert_der: *const u8,
    cert_len: usize,
    date: *const c_char,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(|| {
        let tsa = match Signer::from_pkcs8_der(unsafe { bytes(key_der, key_len) }, unsafe {
            bytes(cert_der, cert_len)
        }) {
            Ok(s) => s,
            Err(e) => {
                set_last_error(format!("TSA setup failed: {e}"));
                return PdfStatus::Sign;
            }
        };
        let date = unsafe { opt(date) };
        match timestamp(unsafe { bytes(pdf, pdf_len) }, &tsa, date.as_deref()) {
            Ok(b) => unsafe { emit_buffer(b, out_ptr, out_len) },
            Err(e) => {
                set_last_error(format!("timestamp failed: {e}"));
                PdfStatus::Sign
            }
        }
    })
}

/// Append a Document Security Store (`/DSS`, PAdES-B-LT) with the given DER
/// certificates and CRLs. Each is passed as parallel `ptr`/`len` arrays.
///
/// # Safety
/// `cert_ptrs`/`cert_lens` have `cert_count` entries; `crl_ptrs`/`crl_lens`
/// have `crl_count`; every entry readable; `out_ptr`/`out_len` writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_add_dss(
    pdf: *const u8,
    pdf_len: usize,
    cert_ptrs: *const *const u8,
    cert_lens: *const usize,
    cert_count: usize,
    crl_ptrs: *const *const u8,
    crl_lens: *const usize,
    crl_count: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(|| {
        let collect = |ptrs: *const *const u8, lens: *const usize, n: usize| -> Vec<Vec<u8>> {
            if n == 0 || ptrs.is_null() || lens.is_null() {
                return Vec::new();
            }
            let ptrs = unsafe { std::slice::from_raw_parts(ptrs, n) };
            let lens = unsafe { std::slice::from_raw_parts(lens, n) };
            (0..n)
                .map(|i| unsafe { bytes(ptrs[i], lens[i]) }.to_vec())
                .collect()
        };
        let certs = collect(cert_ptrs, cert_lens, cert_count);
        let crls = collect(crl_ptrs, crl_lens, crl_count);
        match pdf::add_dss(unsafe { bytes(pdf, pdf_len) }, &certs, &crls) {
            Ok(b) => unsafe { emit_buffer(b, out_ptr, out_len) },
            Err(e) => {
                set_last_error(format!("add_dss failed: {e}"));
                PdfStatus::Sign
            }
        }
    })
}

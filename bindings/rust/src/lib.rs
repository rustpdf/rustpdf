//! Safe Rust bindings for the rust-pdf engine.
//!
//! This crate is a thin, idiomatic wrapper over the precompiled `libpdf_ffi`
//! cdylib (the same C ABI used by the Python, C#, Go, … bindings). The engine
//! source is **not** part of this crate: the cdylib is located and loaded at
//! run time (see [`loader`]), so consumers `cargo add rustpdf` and ship the
//! library beside their binary (or point `RUSTPDF_LIB` at it).
//!
//! ```no_run
//! use rustpdf::{Document, Align};
//!
//! # fn main() -> rustpdf::Result<()> {
//! let mut doc = Document::new()?;
//! doc.add_page()?
//!    .set_fill_rgb(0.1, 0.2, 0.8)?
//!    .rect(72.0, 700.0, 200.0, 60.0)?
//!    .fill()?;
//! doc.save("out.pdf")?;
//! # Ok(())
//! # }
//! ```

#![deny(rust_2018_idioms)]
#![warn(missing_debug_implementations)]

mod document;
mod editable;
mod enums;
mod error;
mod ffi;
mod json;
mod loader;
mod signing;
mod util;

use std::ffi::CStr;
use std::ptr;

pub use document::{Bookmark, Document};
pub use editable::EditableDoc;
pub use enums::{
    AFRelationship, Align, Certify, Encryption, FacturxProfile, PdfVersion, PdfaLevel,
};
pub use error::{PdfError, PdfStatus, Result};
pub use signing::{
    begin_signing, begin_timestamp, complete_signature, list_signatures, sign_with,
    timestamp_request, timestamp_token_from_response, SignatureField, SignaturePolicy,
    SigningOptions, SigningSession,
};

use util::{check, cstr, opt_cstr, take_buffer};

/// A registered font handle, returned by [`Document::add_font`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontId(pub i32);

/// A registered image handle, returned by [`Document::add_image_png`] etc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageId(pub i32);

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("pages", &self.page_count())
            .finish()
    }
}

impl std::fmt::Debug for EditableDoc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditableDoc")
            .field("pages", &self.page_count())
            .finish()
    }
}

/// The engine version string (empty if the library could not be loaded).
pub fn version() -> String {
    let Ok(a) = ffi::api() else {
        return String::new();
    };
    // SAFETY: returns a static NUL-terminated string owned by the library.
    unsafe {
        let p = (a.pdf_version)();
        if p.is_null() {
            String::new()
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

/// Force the cdylib to load now (useful to surface a missing-library error
/// eagerly). Returns the engine [`version`] on success.
pub fn ensure_loaded() -> Result<String> {
    ffi::api()?;
    Ok(version())
}

/// Activate a license token, unlocking corporate features (PDF/A, signatures,
/// encryption, accessibility) for this process.
pub fn activate_license(token: &str) -> Result<()> {
    let a = ffi::api()?;
    let token = cstr(token)?;
    check(a, unsafe { (a.pdf_activate_license)(token.as_ptr()) })
}

/// Extract the document's text into a UTF-8 string.
pub fn extract_text(data: &[u8]) -> Result<String> {
    let a = ffi::api()?;
    let mut ptr: *mut u8 = ptr::null_mut();
    let mut len: usize = 0;
    check(a, unsafe {
        (a.pdf_extract_text)(data.as_ptr(), data.len(), &mut ptr, &mut len)
    })?;
    let bytes = take_buffer(a, ptr, len);
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Extract every raster image from `data` into directory `dir` (JPEGs are
/// written verbatim as `.jpg`, everything else as `.png`; files are named
/// `page{N}_{name}.{ext}`). Returns the number of images written.
pub fn extract_images_to_dir(data: &[u8], dir: &str) -> Result<usize> {
    let a = ffi::api()?;
    let dir = cstr(dir)?;
    let mut count: usize = 0;
    check(a, unsafe {
        (a.pdf_extract_images_to_dir)(data.as_ptr(), data.len(), dir.as_ptr(), &mut count)
    })?;
    Ok(count)
}

/// Render page `page` (0-based) of `data` to a PNG image at `dpi`
/// dots-per-inch. Page rendering is a licensed **Pro** feature: returns an
/// error (license status) unless a license granting it is active.
pub fn render_page_to_png(data: &[u8], page: usize, dpi: f64) -> Result<Vec<u8>> {
    let a = ffi::api()?;
    let mut ptr: *mut u8 = ptr::null_mut();
    let mut len: usize = 0;
    check(a, unsafe {
        (a.pdf_render_page_to_png)(data.as_ptr(), data.len(), page, dpi, &mut ptr, &mut len)
    })?;
    Ok(take_buffer(a, ptr, len))
}

/// Number of pages in `data` (free — no license required).
pub fn page_count(data: &[u8]) -> Result<usize> {
    let a = ffi::api()?;
    let mut count: usize = 0;
    check(a, unsafe {
        (a.pdf_page_count)(data.as_ptr(), data.len(), &mut count)
    })?;
    Ok(count)
}

/// The validation result for a single signature in a document, as returned by
/// [`verify_signatures`].
#[derive(Clone, Debug, PartialEq)]
pub struct SignatureReport {
    /// The signature field's name (`None` if unnamed).
    pub field_name: Option<String>,
    /// The signature sub-filter (e.g. `adbe.pkcs7.detached`, `ETSI.CAdES.detached`).
    pub sub_filter: String,
    /// The signer's subject/common name (`None` if it could not be read).
    pub signer: Option<String>,
    /// Whether the `/ByteRange` covers the whole document (no later edits).
    pub covers_whole_document: bool,
    /// Whether the document digest matches the signed digest.
    pub digest_valid: bool,
    /// Whether the cryptographic signature itself verifies.
    pub signature_valid: bool,
    /// Whether the signature is valid overall (digest + signature + coverage).
    pub is_valid: bool,
    /// The signature's `/ByteRange` (`[start, len, start, len]`).
    pub byte_range: [i64; 4],
    /// The signer certificate's issuer DN (`None` if unavailable).
    pub issuer: Option<String>,
    /// The signer certificate's serial number, hex (`None` if unavailable).
    pub serial_number: Option<String>,
    /// The certificate's "not before" validity bound, ISO-8601 (`None` if unavailable).
    pub valid_from: Option<String>,
    /// The certificate's "not after" validity bound, ISO-8601 (`None` if unavailable).
    pub valid_to: Option<String>,
    /// The signature algorithm (e.g. `SHA256withRSA`; `None` if unavailable).
    pub algorithm: Option<String>,
    /// The claimed signing time, ISO-8601 (`None` if absent).
    pub signing_time: Option<String>,
    /// Number of certificates embedded in the CMS.
    pub cert_count: usize,
    /// Whether the signature carries an embedded (PAdES) timestamp.
    pub has_timestamp: bool,
}

/// Validate every signature in `data`. Returns one [`SignatureReport`] per
/// signature; an empty vector means the document is unsigned.
pub fn verify_signatures(data: &[u8]) -> Result<Vec<SignatureReport>> {
    let a = ffi::api()?;
    let mut ptr: *mut u8 = ptr::null_mut();
    let mut len: usize = 0;
    check(a, unsafe {
        (a.pdf_verify_signatures_json)(data.as_ptr(), data.len(), &mut ptr, &mut len)
    })?;
    let bytes = take_buffer(a, ptr, len);
    let text = String::from_utf8_lossy(&bytes);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let value = json::parse(trimmed).ok_or_else(|| {
        PdfError::new(
            PdfStatus::Parse,
            format!("could not parse signature report JSON: {trimmed}"),
        )
    })?;
    let array = value
        .as_array()
        .ok_or_else(|| PdfError::new(PdfStatus::Parse, "signature report JSON was not an array"))?;
    let mut out = Vec::with_capacity(array.len());
    for item in array {
        let mut byte_range = [0i64; 4];
        if let Some(br) = item.get("byte_range").and_then(json::Json::as_array) {
            for (i, slot) in byte_range.iter_mut().enumerate() {
                if let Some(n) = br.get(i).and_then(json::Json::as_i64) {
                    *slot = n;
                }
            }
        }
        let opt_str = |key: &str| {
            item.get(key).and_then(|v| {
                if v.is_null() {
                    None
                } else {
                    v.as_str().map(str::to_owned)
                }
            })
        };
        out.push(SignatureReport {
            field_name: opt_str("field_name"),
            sub_filter: item
                .get("sub_filter")
                .and_then(json::Json::as_str)
                .unwrap_or_default()
                .to_owned(),
            signer: opt_str("signer"),
            covers_whole_document: item
                .get("covers_whole_document")
                .and_then(json::Json::as_bool)
                .unwrap_or(false),
            digest_valid: item
                .get("digest_valid")
                .and_then(json::Json::as_bool)
                .unwrap_or(false),
            signature_valid: item
                .get("signature_valid")
                .and_then(json::Json::as_bool)
                .unwrap_or(false),
            is_valid: item
                .get("is_valid")
                .and_then(json::Json::as_bool)
                .unwrap_or(false),
            byte_range,
            issuer: opt_str("issuer"),
            serial_number: opt_str("serial_number"),
            valid_from: opt_str("valid_from"),
            valid_to: opt_str("valid_to"),
            algorithm: opt_str("algorithm"),
            signing_time: opt_str("signing_time"),
            cert_count: item
                .get("cert_count")
                .and_then(json::Json::as_i64)
                .unwrap_or(0)
                .max(0) as usize,
            has_timestamp: item
                .get("has_timestamp")
                .and_then(json::Json::as_bool)
                .unwrap_or(false),
        });
    }
    Ok(out)
}

/// One positional match from [`find_text`]. Coordinates are in PDF user space
/// (points, origin lower-left).
#[derive(Clone, Debug, PartialEq)]
pub struct TextHit {
    /// 0-based page index.
    pub page: usize,
    /// The matched text.
    pub text: String,
    /// Lower-left x of the bounding box.
    pub x: f64,
    /// Lower-left y of the bounding box.
    pub y: f64,
    /// Box width.
    pub width: f64,
    /// Box height.
    pub height: f64,
}

/// Find every occurrence of `query` in `data`, returning a [`TextHit`] (with a
/// bounding box) per match. An empty vector means no match.
pub fn find_text(data: &[u8], query: &str, case_sensitive: bool) -> Result<Vec<TextHit>> {
    let a = ffi::api()?;
    let query = cstr(query)?;
    let mut ptr: *mut u8 = ptr::null_mut();
    let mut len: usize = 0;
    check(a, unsafe {
        (a.pdf_find_text_json)(
            data.as_ptr(),
            data.len(),
            query.as_ptr(),
            case_sensitive as i32,
            &mut ptr,
            &mut len,
        )
    })?;
    let bytes = take_buffer(a, ptr, len);
    let text = String::from_utf8_lossy(&bytes);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let value = json::parse(trimmed).ok_or_else(|| {
        PdfError::new(
            PdfStatus::Parse,
            format!("could not parse find_text JSON: {trimmed}"),
        )
    })?;
    let array = value
        .as_array()
        .ok_or_else(|| PdfError::new(PdfStatus::Parse, "find_text JSON was not an array"))?;
    let num =
        |item: &json::Json, key: &str| item.get(key).and_then(json::Json::as_f64).unwrap_or(0.0);
    let mut out = Vec::with_capacity(array.len());
    for item in array {
        out.push(TextHit {
            page: item
                .get("page")
                .and_then(json::Json::as_i64)
                .unwrap_or(0)
                .max(0) as usize,
            text: item
                .get("text")
                .and_then(json::Json::as_str)
                .unwrap_or_default()
                .to_owned(),
            x: num(item, "x"),
            y: num(item, "y"),
            width: num(item, "width"),
            height: num(item, "height"),
        });
    }
    Ok(out)
}

/// Sign `pdf` with a PKCS#8 DER private key + DER certificate, returning a new
/// PDF (incremental update). `pades` selects PAdES-B-B.
#[allow(clippy::too_many_arguments)]
pub fn sign(
    pdf: &[u8],
    key_der: &[u8],
    cert_der: &[u8],
    reason: Option<&str>,
    location: Option<&str>,
    name: Option<&str>,
    pades: bool,
) -> Result<Vec<u8>> {
    let a = ffi::api()?;
    let (reason, location, name) = (opt_cstr(reason)?, opt_cstr(location)?, opt_cstr(name)?);
    let p = |c: &Option<std::ffi::CString>| c.as_ref().map_or(ptr::null(), |s| s.as_ptr());
    let mut out: *mut u8 = ptr::null_mut();
    let mut out_len: usize = 0;
    check(a, unsafe {
        (a.pdf_sign)(
            pdf.as_ptr(),
            pdf.len(),
            key_der.as_ptr(),
            key_der.len(),
            cert_der.as_ptr(),
            cert_der.len(),
            p(&reason),
            p(&location),
            p(&name),
            pades as i32,
            &mut out,
            &mut out_len,
        )
    })?;
    Ok(take_buffer(a, out, out_len))
}

/// Append a document timestamp (`/DocTimeStamp`, PAdES-B-LTA) using a TSA key +
/// cert. `date` may be `None` (a fixed reproducible value is used).
pub fn timestamp(
    pdf: &[u8],
    key_der: &[u8],
    cert_der: &[u8],
    date: Option<&str>,
) -> Result<Vec<u8>> {
    let a = ffi::api()?;
    let date = opt_cstr(date)?;
    let date_ptr = date.as_ref().map_or(ptr::null(), |s| s.as_ptr());
    let mut out: *mut u8 = ptr::null_mut();
    let mut out_len: usize = 0;
    check(a, unsafe {
        (a.pdf_timestamp)(
            pdf.as_ptr(),
            pdf.len(),
            key_der.as_ptr(),
            key_der.len(),
            cert_der.as_ptr(),
            cert_der.len(),
            date_ptr,
            &mut out,
            &mut out_len,
        )
    })?;
    Ok(take_buffer(a, out, out_len))
}

/// Append a Document Security Store (`/DSS`, PAdES-B-LT) with the given DER
/// certificates and CRLs.
pub fn add_dss(pdf: &[u8], certs: &[&[u8]], crls: &[&[u8]]) -> Result<Vec<u8>> {
    let a = ffi::api()?;

    let cert_ptrs: Vec<*const u8> = certs.iter().map(|c| c.as_ptr()).collect();
    let cert_lens: Vec<usize> = certs.iter().map(|c| c.len()).collect();
    let crl_ptrs: Vec<*const u8> = crls.iter().map(|c| c.as_ptr()).collect();
    let crl_lens: Vec<usize> = crls.iter().map(|c| c.len()).collect();

    let mut out: *mut u8 = ptr::null_mut();
    let mut out_len: usize = 0;
    check(a, unsafe {
        (a.pdf_add_dss)(
            pdf.as_ptr(),
            pdf.len(),
            cert_ptrs.as_ptr(),
            cert_lens.as_ptr(),
            certs.len(),
            crl_ptrs.as_ptr(),
            crl_lens.as_ptr(),
            crls.len(),
            &mut out,
            &mut out_len,
        )
    })?;
    Ok(take_buffer(a, out, out_len))
}

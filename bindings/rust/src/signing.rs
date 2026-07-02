//! Deferred / external (HSM) signing and network-TSA helpers (issue #41).
//!
//! The private key never reaches this library:
//! * **Model A** ([`sign_with`]) builds the CMS signed attributes and calls a
//!   caller-supplied closure for the raw RSA signature.
//! * **Model B** ([`begin_signing`] → [`SigningSession::complete`]) is the
//!   two-phase prepare / hash / embed flow for async HSM / HTTP boundaries.
//! * **Network TSA (AD-RT)** ([`begin_timestamp`] / [`timestamp_request`] /
//!   [`timestamp_token_from_response`] + [`complete_signature`]) drives an
//!   RFC 3161 timestamp authority over the network.

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use crate::enums::Certify;
use crate::error::{PdfError, PdfStatus, Result};
use crate::ffi::{self, PdfSignHashFn, PdfSigningOptions};
use crate::util::{check, cstr, take_buffer};

/// A signature-policy identifier (PAdES-EPES / ICP-Brasil AD-RB).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SignaturePolicy {
    /// The policy OID (dotted-decimal), e.g. the ICP-Brasil AD-RB OID.
    pub oid: String,
    /// The policy document hash (under [`Self::hash_algorithm_oid`]).
    pub hash: Vec<u8>,
    /// Hash algorithm OID; `None` = SHA-256.
    pub hash_algorithm_oid: Option<String>,
    /// Optional SPURI qualifier — where the policy can be retrieved.
    pub uri: Option<String>,
}

/// Options for deferred / external signing, shared by [`sign_with`] and
/// [`begin_signing`]. All fields default to "absent".
#[derive(Clone, Debug, Default)]
pub struct SigningOptions {
    /// Free-text reason for signing.
    pub reason: Option<String>,
    /// Free-text location.
    pub location: Option<String>,
    /// Signer name shown in the signature.
    pub name: Option<String>,
    /// Produce a PAdES-B-B signature (`ETSI.CAdES.detached`).
    pub pades: bool,
    /// Certify the document (DocMDP) — use only on the first signature.
    pub certify: Certify,
    /// Reserved `/Contents` bytes; 0 = default (8192). Raise for large
    /// cloud-HSM CMS containers.
    pub container_size: usize,
    /// Signature-policy identifier (PAdES-EPES); `None` = none.
    pub policy: Option<SignaturePolicy>,
    /// Draw a visible signature appearance using the `visible_*` fields.
    pub visible: bool,
    /// 0-based page index for the visible appearance.
    pub visible_page: usize,
    /// Appearance rectangle `[x0, y0, x1, y1]` in page points.
    pub visible_rect: [f64; 4],
    /// Appearance text lines, separated by `\n`; `None` = none.
    pub visible_text: Option<String>,
    /// PNG/JPEG bytes of a handwritten-signature image; `None` = none.
    pub visible_image: Option<Vec<u8>>,
}

/// A signature field discovered in a PDF (pre-signing inventory).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureField {
    /// The field's fully-qualified name.
    pub name: String,
    /// Whether the field is already signed.
    pub signed: bool,
}

/// An in-progress two-phase (Model B) signature.
///
/// [`Self::document`] holds the prepared PDF (with a zero-filled `/Contents`
/// placeholder); [`Self::to_be_signed`] holds the exact bytes the signature
/// covers. SHA-256 `to_be_signed`, build the DER CMS / PKCS#7 container with a
/// remote signer, then call [`Self::complete`]. The key never reaches this
/// library.
#[derive(Clone, Debug)]
pub struct SigningSession {
    /// The prepared PDF with a zero-filled `/Contents` placeholder.
    pub document: Vec<u8>,
    /// The exact bytes the signature covers (hash these for the signer).
    pub to_be_signed: Vec<u8>,
}

impl SigningSession {
    /// Phase 2: embed a finished DER CMS / PKCS#7 `container`, returning the
    /// final signed PDF.
    pub fn complete(&self, container: &[u8]) -> Result<Vec<u8>> {
        complete_signature(&self.document, container)
    }
}

/// Build an optional `CString` argument, recording it in `keep` so its backing
/// storage outlives the FFI call.
fn push_cstr(value: Option<&str>, keep: &mut Vec<CString>) -> Result<*const c_char> {
    match value {
        Some(v) => {
            let c = cstr(v)?;
            let p = c.as_ptr();
            keep.push(c);
            Ok(p)
        }
        None => Ok(ptr::null()),
    }
}

/// Marshal [`SigningOptions`] into the C-ABI struct plus a keep-alive vector of
/// `CString`s that must outlive the native call. Byte slices (`policy.hash`,
/// `visible_image`) are borrowed from `options`, which the caller keeps alive
/// for the duration of the FFI call.
fn build_options(options: Option<&SigningOptions>) -> Result<(PdfSigningOptions, Vec<CString>)> {
    let mut native = PdfSigningOptions::empty();
    let mut keep: Vec<CString> = Vec::new();
    let Some(o) = options else {
        return Ok((native, keep));
    };
    native.reason = push_cstr(o.reason.as_deref(), &mut keep)?;
    native.location = push_cstr(o.location.as_deref(), &mut keep)?;
    native.name = push_cstr(o.name.as_deref(), &mut keep)?;
    native.pades = o.pades as c_int;
    native.certification = o.certify.code();
    native.estimated_size = o.container_size;
    if let Some(pol) = &o.policy {
        native.policy_oid = push_cstr(Some(&pol.oid), &mut keep)?;
        if !pol.hash.is_empty() {
            native.policy_hash = pol.hash.as_ptr();
            native.policy_hash_len = pol.hash.len();
        }
        native.policy_hash_alg_oid = push_cstr(pol.hash_algorithm_oid.as_deref(), &mut keep)?;
        native.policy_uri = push_cstr(pol.uri.as_deref(), &mut keep)?;
    }
    native.visible = o.visible as c_int;
    native.vis_page = o.visible_page;
    native.vis_rect = o.visible_rect;
    native.vis_text = push_cstr(o.visible_text.as_deref(), &mut keep)?;
    if let Some(img) = &o.visible_image {
        if !img.is_empty() {
            native.vis_image = img.as_ptr();
            native.vis_image_len = img.len();
        }
    }
    Ok((native, keep))
}

/// Context passed through the C `void *ctx` to [`trampoline`]: the user closure
/// plus a slot to stash an error so it can be re-surfaced past the opaque FFI
/// status code.
struct CbCtx<'a> {
    f: &'a mut dyn FnMut(&[u8]) -> Result<Vec<u8>>,
    err: Option<PdfError>,
}

/// The `PdfSignHashFn` the engine calls for the raw signature. Never lets a
/// panic unwind across the C boundary.
unsafe extern "C" fn trampoline(
    ctx: *mut c_void,
    data: *const u8,
    data_len: usize,
    sig_buf: *mut u8,
    sig_cap: usize,
    sig_len: *mut usize,
) -> c_int {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: `ctx` is the `&mut CbCtx` we passed to `pdf_sign_with`.
        let cx = &mut *(ctx as *mut CbCtx<'_>);
        // SAFETY: the engine hands us a readable region of `data_len` bytes.
        let input = std::slice::from_raw_parts(data, data_len);
        match (cx.f)(input) {
            Ok(sig) => {
                if sig.len() > sig_cap {
                    cx.err = Some(PdfError::new(
                        PdfStatus::InvalidArgument,
                        "signature exceeds the callback buffer capacity",
                    ));
                    return 2;
                }
                // SAFETY: `sig_buf` has `sig_cap >= sig.len()` capacity.
                std::ptr::copy_nonoverlapping(sig.as_ptr(), sig_buf, sig.len());
                *sig_len = sig.len();
                0
            }
            Err(e) => {
                cx.err = Some(e);
                1
            }
        }
    }));
    result.unwrap_or(1)
}

/// **Model A — remote signer.** Sign `pdf` without handing this library a key:
/// it builds the CMS signed attributes and calls `sign_hash` for the raw RSA
/// PKCS#1 v1.5 signature over SHA-256 of its argument, then assembles and embeds
/// the CMS. `cert_der` is the signer certificate; `chain` are intermediate
/// certificates (DER), supplied independently of the key.
pub fn sign_with<F>(
    pdf: &[u8],
    cert_der: &[u8],
    chain: &[&[u8]],
    options: Option<&SigningOptions>,
    mut sign_hash: F,
) -> Result<Vec<u8>>
where
    F: FnMut(&[u8]) -> Result<Vec<u8>>,
{
    let a = ffi::api()?;
    let (native, _keep) = build_options(options)?;

    let chain_ptrs: Vec<*const u8> = chain.iter().map(|c| c.as_ptr()).collect();
    let chain_lens: Vec<usize> = chain.iter().map(|c| c.len()).collect();
    let (chain_ptr, chain_len_ptr) = if chain.is_empty() {
        (ptr::null(), ptr::null())
    } else {
        (chain_ptrs.as_ptr(), chain_lens.as_ptr())
    };

    let mut cx = CbCtx {
        f: &mut sign_hash,
        err: None,
    };
    let callback: PdfSignHashFn = trampoline;

    let mut out: *mut u8 = ptr::null_mut();
    let mut out_len: usize = 0;
    let code = unsafe {
        (a.pdf_sign_with)(
            pdf.as_ptr(),
            pdf.len(),
            cert_der.as_ptr(),
            cert_der.len(),
            chain_ptr,
            chain_len_ptr,
            chain.len(),
            &native,
            callback,
            &mut cx as *mut CbCtx<'_> as *mut c_void,
            &mut out,
            &mut out_len,
        )
    };
    // Surface a callback error in preference to the opaque FFI status.
    if let Some(e) = cx.err.take() {
        return Err(e);
    }
    check(a, code)?;
    Ok(take_buffer(a, out, out_len))
}

/// **Model B — two-phase signing, phase 1.** Prepare `pdf` for deferred signing
/// and return a [`SigningSession`]. Hash its `to_be_signed`, build the CMS
/// container remotely, then call [`SigningSession::complete`].
pub fn begin_signing(pdf: &[u8], options: Option<&SigningOptions>) -> Result<SigningSession> {
    let a = ffi::api()?;
    let (native, _keep) = build_options(options)?;
    let mut doc: *mut u8 = ptr::null_mut();
    let mut doc_len: usize = 0;
    let mut tbs: *mut u8 = ptr::null_mut();
    let mut tbs_len: usize = 0;
    check(a, unsafe {
        (a.pdf_sign_begin)(
            pdf.as_ptr(),
            pdf.len(),
            &native,
            &mut doc,
            &mut doc_len,
            &mut tbs,
            &mut tbs_len,
        )
    })?;
    Ok(SigningSession {
        document: take_buffer(a, doc, doc_len),
        to_be_signed: take_buffer(a, tbs, tbs_len),
    })
}

/// **Model B — two-phase signing, phase 2.** Embed a complete DER CMS / PKCS#7
/// `container` into a prepared `document` (from [`begin_signing`] or
/// [`begin_timestamp`]), producing the final signed PDF.
pub fn complete_signature(document: &[u8], container: &[u8]) -> Result<Vec<u8>> {
    let a = ffi::api()?;
    let mut out: *mut u8 = ptr::null_mut();
    let mut out_len: usize = 0;
    check(a, unsafe {
        (a.pdf_sign_complete)(
            document.as_ptr(),
            document.len(),
            container.as_ptr(),
            container.len(),
            &mut out,
            &mut out_len,
        )
    })?;
    Ok(take_buffer(a, out, out_len))
}

/// List the signature fields in `pdf` (detect existing signatures before
/// signing — the classic pre-sign signature-field inventory). An empty
/// vector means there are no signature fields.
pub fn list_signatures(pdf: &[u8]) -> Result<Vec<SignatureField>> {
    let a = ffi::api()?;
    let mut ptr_out: *mut u8 = ptr::null_mut();
    let mut len: usize = 0;
    check(a, unsafe {
        (a.pdf_list_signatures)(pdf.as_ptr(), pdf.len(), &mut ptr_out, &mut len)
    })?;
    let bytes = take_buffer(a, ptr_out, len);
    let text = String::from_utf8_lossy(&bytes);
    let mut out = Vec::new();
    for line in text.split('\n') {
        if line.is_empty() {
            continue;
        }
        let Some(tab) = line.find('\t') else {
            continue;
        };
        out.push(SignatureField {
            signed: &line[..tab] == "1",
            name: line[tab + 1..].to_owned(),
        });
    }
    Ok(out)
}

/// **Network-TSA, phase 1.** Prepare `pdf` for a `/DocTimeStamp` from a network
/// RFC 3161 TSA. Returns `(document, to_be_signed)`: the prepared PDF (with a
/// zero-filled `/Contents` placeholder) and the exact bytes covered by the
/// timestamp. SHA-256 `to_be_signed` to build the request with
/// [`timestamp_request`].
pub fn begin_timestamp(pdf: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let a = ffi::api()?;
    let mut doc: *mut u8 = ptr::null_mut();
    let mut doc_len: usize = 0;
    let mut tbs: *mut u8 = ptr::null_mut();
    let mut tbs_len: usize = 0;
    check(a, unsafe {
        (a.pdf_timestamp_begin)(
            pdf.as_ptr(),
            pdf.len(),
            &mut doc,
            &mut doc_len,
            &mut tbs,
            &mut tbs_len,
        )
    })?;
    Ok((take_buffer(a, doc, doc_len), take_buffer(a, tbs, tbs_len)))
}

/// Build an RFC 3161 `TimeStampReq` (DER) for `imprint` (the SHA-256 of the
/// bytes to timestamp). POST the result to the TSA. `nonce` is optional;
/// `cert_req` asks the TSA to embed its certificate.
pub fn timestamp_request(imprint: &[u8], nonce: Option<&[u8]>, cert_req: bool) -> Result<Vec<u8>> {
    let a = ffi::api()?;
    let (nonce_ptr, nonce_len) = match nonce {
        Some(n) if !n.is_empty() => (n.as_ptr(), n.len()),
        _ => (ptr::null(), 0),
    };
    let mut out: *mut u8 = ptr::null_mut();
    let mut out_len: usize = 0;
    check(a, unsafe {
        (a.pdf_timestamp_request)(
            imprint.as_ptr(),
            imprint.len(),
            nonce_ptr,
            nonce_len,
            cert_req as i32,
            &mut out,
            &mut out_len,
        )
    })?;
    Ok(take_buffer(a, out, out_len))
}

/// Extract the `TimeStampToken` (a CMS `ContentInfo`) from a TSA's RFC 3161
/// `TimeStampResp`. Embed the result via [`complete_signature`].
pub fn timestamp_token_from_response(response: &[u8]) -> Result<Vec<u8>> {
    let a = ffi::api()?;
    let mut out: *mut u8 = ptr::null_mut();
    let mut out_len: usize = 0;
    check(a, unsafe {
        (a.pdf_timestamp_token_from_response)(
            response.as_ptr(),
            response.len(),
            &mut out,
            &mut out_len,
        )
    })?;
    Ok(take_buffer(a, out, out_len))
}

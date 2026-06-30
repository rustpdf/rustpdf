//! C ABI: digital signatures and PAdES LTV (`pdf::sign`/`timestamp`/`add_dss`),
//! plus **deferred / HSM signing** (issue #41 P0): two-phase
//! prepare/embed (`pdf_sign_begin`/`pdf_sign_complete`), an external-signer
//! callback (`pdf_sign_with`), and signature-field listing
//! (`pdf_list_signatures`).

use std::ffi::{c_char, c_double, c_int, c_uchar, c_void};

use pdf::{
    begin_signing, begin_timestamp, complete_signing, list_signatures, sign, sign_with, timestamp,
    timestamp_request, timestamp_token_from_response, Certify, SignError, SignOptions,
    SignaturePolicy, Signer, VisibleSignature,
};

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

/// Options for deferred/external signing, carried across the C ABI. NULL string
/// fields and a zero `policy_hash_len` mean "absent". `certification` is the
/// DocMDP `/P` value (0 = none, 1/2/3); `estimated_size` of 0 uses the default.
#[repr(C)]
pub struct PdfSigningOptions {
    pub reason: *const c_char,
    pub location: *const c_char,
    pub name: *const c_char,
    /// Non-zero selects PAdES-B-B (`ETSI.CAdES.detached`).
    pub pades: c_int,
    /// DocMDP certification level: 0 = none, 1/2/3 = `/P` value.
    pub certification: c_int,
    /// Reserved `/Contents` bytes; 0 = library default (8192).
    pub estimated_size: usize,
    /// Signature-policy OID (PAdES-EPES / ICP-Brasil); NULL = no policy.
    pub policy_oid: *const c_char,
    /// Policy hash bytes (with `policy_hash_len`); ignored if `policy_oid` NULL.
    pub policy_hash: *const u8,
    pub policy_hash_len: usize,
    /// Policy hash algorithm OID; NULL = SHA-256.
    pub policy_hash_alg_oid: *const c_char,
    /// SPURI qualifier; NULL = none.
    pub policy_uri: *const c_char,
    /// Non-zero draws a **visible** signature using the fields below.
    pub visible: c_int,
    /// 0-based page index for the visible appearance.
    pub vis_page: usize,
    /// Appearance rectangle `[x0, y0, x1, y1]` in page points.
    pub vis_rect: [c_double; 4],
    /// Text lines for the appearance, separated by `\n`; NULL = none.
    pub vis_text: *const c_char,
    /// PNG/JPEG bytes of a handwritten-signature image (with `vis_image_len`);
    /// NULL/0 = no image.
    pub vis_image: *const u8,
    pub vis_image_len: usize,
}

/// Build a [`SignOptions`] from C-ABI [`PdfSigningOptions`].
///
/// # Safety
/// `params` must be NULL or point at a valid `PdfSigningOptions` whose string
/// pointers are NULL or valid C strings and whose `policy_hash`/`policy_hash_len`
/// describe a readable region.
unsafe fn sign_options(params: *const PdfSigningOptions) -> SignOptions {
    let Some(p) = (unsafe { params.as_ref() }) else {
        return SignOptions::default();
    };
    let certification = match p.certification {
        1 => Some(Certify::Locked),
        2 => Some(Certify::Forms),
        3 => Some(Certify::FormsAndAnnotations),
        _ => None,
    };
    let policy = unsafe { opt(p.policy_oid) }.map(|oid| SignaturePolicy {
        oid,
        hash: if p.policy_hash.is_null() || p.policy_hash_len == 0 {
            Vec::new()
        } else {
            unsafe { bytes(p.policy_hash, p.policy_hash_len) }.to_vec()
        },
        hash_algorithm_oid: unsafe { opt(p.policy_hash_alg_oid) },
        uri: unsafe { opt(p.policy_uri) },
    });
    let visible = if p.visible != 0 {
        let lines = unsafe { opt(p.vis_text) }
            .map(|t| t.lines().map(str::to_string).collect())
            .unwrap_or_default();
        let image = if p.vis_image.is_null() || p.vis_image_len == 0 {
            None
        } else {
            Some(unsafe { bytes(p.vis_image, p.vis_image_len) }.to_vec())
        };
        Some(VisibleSignature {
            page: p.vis_page,
            rect: p.vis_rect,
            lines,
            image,
        })
    } else {
        None
    };
    SignOptions {
        reason: unsafe { opt(p.reason) },
        location: unsafe { opt(p.location) },
        name: unsafe { opt(p.name) },
        date: None,
        visible,
        pades: p.pades != 0,
        certification,
        policy,
        estimated_size: (p.estimated_size != 0).then_some(p.estimated_size),
    }
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
            pades: pades != 0,
            ..Default::default()
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

/// **Two-phase signing, phase 1.** Prepare `pdf` for deferred signing: returns
/// the prepared PDF (`out_doc`/`out_doc_len`, with a zero-filled `/Contents`
/// placeholder) and the exact bytes to be signed (`out_tbs`/`out_tbs_len`).
/// Hash `out_tbs` (SHA-256), sign remotely / build the CMS container, then call
/// [`pdf_sign_complete`]. The private key never reaches this library.
///
/// # Safety
/// `pdf` readable for `pdf_len`; `params` NULL or a valid [`PdfSigningOptions`]; the
/// four out pointers writable. Both emitted buffers are freed with
/// `pdf_buffer_free`.
#[no_mangle]
pub unsafe extern "C" fn pdf_sign_begin(
    pdf: *const u8,
    pdf_len: usize,
    params: *const PdfSigningOptions,
    out_doc: *mut *mut c_uchar,
    out_doc_len: *mut usize,
    out_tbs: *mut *mut c_uchar,
    out_tbs_len: *mut usize,
) -> PdfStatus {
    guard(|| {
        let opts = unsafe { sign_options(params) };
        match begin_signing(unsafe { bytes(pdf, pdf_len) }, &opts) {
            Ok(prepared) => {
                let tbs = prepared.signed_bytes().to_vec();
                let status = unsafe { emit_buffer(tbs, out_tbs, out_tbs_len) };
                if status != PdfStatus::Ok {
                    return status;
                }
                unsafe { emit_buffer(prepared.document().to_vec(), out_doc, out_doc_len) }
            }
            Err(e) => {
                set_last_error(format!("sign prepare failed: {e}"));
                PdfStatus::Sign
            }
        }
    })
}

/// **Two-phase signing, phase 2.** Embed a complete DER CMS / PKCS#7 `container`
/// into the prepared `document` (from [`pdf_sign_begin`]), producing the final
/// signed PDF in `out_ptr`/`out_len`.
///
/// # Safety
/// `document`/`container` readable for their lengths; `out_ptr`/`out_len`
/// writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_sign_complete(
    document: *const u8,
    document_len: usize,
    container: *const u8,
    container_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(|| {
        match complete_signing(unsafe { bytes(document, document_len) }, unsafe {
            bytes(container, container_len)
        }) {
            Ok(b) => unsafe { emit_buffer(b, out_ptr, out_len) },
            Err(e) => {
                set_last_error(format!("sign embed failed: {e}"));
                PdfStatus::Sign
            }
        }
    })
}

/// **Network timestamp (AD-RT), phase 1.** Prepare `pdf` for a `/DocTimeStamp`
/// from a network RFC 3161 TSA: returns the prepared PDF (`out_doc`) and the
/// bytes to timestamp (`out_tbs`). SHA-256 `out_tbs`, build a request with
/// [`pdf_timestamp_request`], POST it to the TSA, extract the token with
/// [`pdf_timestamp_token_from_response`], then embed it via [`pdf_sign_complete`].
///
/// # Safety
/// `pdf` readable for `pdf_len`; the four out pointers writable (buffers freed
/// with `pdf_buffer_free`).
#[no_mangle]
pub unsafe extern "C" fn pdf_timestamp_begin(
    pdf: *const u8,
    pdf_len: usize,
    out_doc: *mut *mut c_uchar,
    out_doc_len: *mut usize,
    out_tbs: *mut *mut c_uchar,
    out_tbs_len: *mut usize,
) -> PdfStatus {
    guard(|| match begin_timestamp(unsafe { bytes(pdf, pdf_len) }) {
        Ok(prepared) => {
            let tbs = prepared.signed_bytes().to_vec();
            let status = unsafe { emit_buffer(tbs, out_tbs, out_tbs_len) };
            if status != PdfStatus::Ok {
                return status;
            }
            unsafe { emit_buffer(prepared.document().to_vec(), out_doc, out_doc_len) }
        }
        Err(e) => {
            set_last_error(format!("timestamp prepare failed: {e}"));
            PdfStatus::Sign
        }
    })
}

/// Build an RFC 3161 `TimeStampReq` (DER) for `imprint` (the SHA-256 of the
/// bytes to timestamp). `nonce`/`nonce_len` is optional (NULL/0 = none);
/// `cert_req` non-zero asks the TSA to embed its certificate. Result in
/// `out_ptr`/`out_len` (freed with `pdf_buffer_free`).
///
/// # Safety
/// `imprint` readable for `imprint_len`; `nonce` NULL or readable for
/// `nonce_len`; out pointers writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_timestamp_request(
    imprint: *const u8,
    imprint_len: usize,
    nonce: *const u8,
    nonce_len: usize,
    cert_req: c_int,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(|| {
        let imprint = unsafe { bytes(imprint, imprint_len) };
        let nonce_vec = if nonce.is_null() || nonce_len == 0 {
            None
        } else {
            Some(unsafe { bytes(nonce, nonce_len) }.to_vec())
        };
        let req = timestamp_request(imprint, nonce_vec.as_deref(), cert_req != 0);
        unsafe { emit_buffer(req, out_ptr, out_len) }
    })
}

/// Extract the `TimeStampToken` (a CMS `ContentInfo`) from a TSA's RFC 3161
/// `TimeStampResp` in `response`/`response_len`. The token bytes (for
/// [`pdf_sign_complete`]) are returned in `out_ptr`/`out_len`.
///
/// # Safety
/// `response` readable for `response_len`; out pointers writable.
#[no_mangle]
pub unsafe extern "C" fn pdf_timestamp_token_from_response(
    response: *const u8,
    response_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(
        || match timestamp_token_from_response(unsafe { bytes(response, response_len) }) {
            Ok(token) => unsafe { emit_buffer(token, out_ptr, out_len) },
            Err(e) => {
                set_last_error(format!("timestamp response parse failed: {e}"));
                PdfStatus::Sign
            }
        },
    )
}

/// Callback invoked to produce the raw RSA PKCS#1 v1.5 signature (over SHA-256
/// of `data`) from a remote HSM. Write the signature into `sig_buf` (capacity
/// `sig_cap`), set `*sig_len`, and return 0 on success (non-zero = failure).
pub type PdfSignHashFn = extern "C" fn(
    ctx: *mut c_void,
    data: *const u8,
    data_len: usize,
    sig_buf: *mut u8,
    sig_cap: usize,
    sig_len: *mut usize,
) -> c_int;

/// **Model A — external signer callback.** Sign `pdf` without handing this
/// library a key: it builds the CMS signed attributes and calls `callback`
/// (with `ctx`) for the raw RSA signature, then assembles and embeds the CMS.
/// `cert_der` is the signer certificate; `chain_ptrs`/`chain_lens`/`chain_count`
/// are intermediate certificates (DER), supplied independently of the key.
///
/// # Safety
/// `pdf`/`cert_der` readable for their lengths; each `chain_ptrs[i]` readable
/// for `chain_lens[i]`; `params` NULL or valid; `callback` a valid function
/// pointer; `out_ptr`/`out_len` writable.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_sign_with(
    pdf: *const u8,
    pdf_len: usize,
    cert_der: *const u8,
    cert_len: usize,
    chain_ptrs: *const *const u8,
    chain_lens: *const usize,
    chain_count: usize,
    params: *const PdfSigningOptions,
    callback: PdfSignHashFn,
    ctx: *mut c_void,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(|| {
        let chain: Vec<Vec<u8>> =
            if chain_count == 0 || chain_ptrs.is_null() || chain_lens.is_null() {
                Vec::new()
            } else {
                let ptrs = unsafe { std::slice::from_raw_parts(chain_ptrs, chain_count) };
                let lens = unsafe { std::slice::from_raw_parts(chain_lens, chain_count) };
                (0..chain_count)
                    .map(|i| unsafe { bytes(ptrs[i], lens[i]) }.to_vec())
                    .collect()
            };
        let opts = unsafe { sign_options(params) };
        // `ctx` is an opaque pointer owned by the caller; threading it through the
        // closure is the documented contract.
        let ctx_addr = ctx as usize;
        let sign_raw = move |data: &[u8]| -> Result<Vec<u8>, SignError> {
            let mut buf = vec![0u8; 2048];
            let mut sig_len = 0usize;
            let rc = callback(
                ctx_addr as *mut c_void,
                data.as_ptr(),
                data.len(),
                buf.as_mut_ptr(),
                buf.len(),
                &mut sig_len,
            );
            if rc != 0 {
                return Err(SignError::Key(format!("external signer returned {rc}")));
            }
            if sig_len > buf.len() {
                return Err(SignError::Key("external signature exceeds buffer".into()));
            }
            buf.truncate(sig_len);
            Ok(buf)
        };
        match sign_with(
            unsafe { bytes(pdf, pdf_len) },
            unsafe { bytes(cert_der, cert_len) },
            &chain,
            &opts,
            sign_raw,
        ) {
            Ok(b) => unsafe { emit_buffer(b, out_ptr, out_len) },
            Err(e) => {
                set_last_error(format!("external sign failed: {e}"));
                PdfStatus::Sign
            }
        }
    })
}

/// List the signature fields in `pdf` (detect existing signatures before
/// signing). Emits a newline-separated text buffer in `out_ptr`/`out_len`; each
/// line is `<0|1>\t<field-name>` where the first column is 1 when the field is
/// already signed. An empty buffer means no signature fields.
///
/// # Safety
/// `pdf` readable for `pdf_len`; `out_ptr`/`out_len` writable. The buffer is
/// freed with `pdf_buffer_free`.
#[no_mangle]
pub unsafe extern "C" fn pdf_list_signatures(
    pdf: *const u8,
    pdf_len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(|| match list_signatures(unsafe { bytes(pdf, pdf_len) }) {
        Ok(fields) => {
            let mut text = String::new();
            for f in &fields {
                text.push_str(if f.signed { "1\t" } else { "0\t" });
                text.push_str(&f.name);
                text.push('\n');
            }
            unsafe { emit_buffer(text.into_bytes(), out_ptr, out_len) }
        }
        Err(e) => {
            set_last_error(format!("signature field listing failed: {e}"));
            PdfStatus::Sign
        }
    })
}

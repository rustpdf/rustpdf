//! Digital signatures and long-term validation (Fase 7.1 / 7.2).
//!
//! * [`sign`] — incremental-update signing with a `ByteRange` placeholder and a
//!   PKCS#7 (CMS) detached signature. Supports invisible/visible signatures,
//!   multiple signatures, certificate chains, **PAdES-B-B**
//!   (`ETSI.CAdES.detached` + `signing-certificate-v2`), **DocMDP
//!   certification** ([`Certify`]) and a **signature-policy
//!   identifier** ([`SignaturePolicy`], PAdES-EPES / ICP-Brasil AD-RB).
//! * **Deferred / HSM signing — "bring your own signer"** (the private key
//!   never reaches this library):
//!   * [`sign_with`] — *Model A*: the library builds the CMS signed
//!     attributes and calls back for the raw RSA signature (from a remote HSM),
//!     then assembles and embeds the CMS.
//!   * [`begin_signing`] + [`SigningSession::complete`] (or the stateless
//!     [`complete_signing`]) — *Model B*: a two-phase signing session, so the
//!     hash can cross an async/HTTP boundary and the caller supplies a complete
//!     CMS container.
//! * [`timestamp`] — append an RFC 3161 **document timestamp** (`/DocTimeStamp`,
//!   `ETSI.RFC3161`) signed by a TSA — the PAdES-B-LTA building block. (Works
//!   fully offline with a self-issued TSA; no network needed.)
//! * [`add_dss`] — add a **Document Security Store** (`/DSS`) carrying the
//!   validation certificates and CRLs — the PAdES-B-LT building block.
//!
//! Each operation appends an incremental update (new objects + `xref` chained
//! via `/Prev`); earlier signatures remain valid. Pre-flight with
//! [`list_signatures`](crate::list_signatures) to detect existing
//! signatures.

use std::cell::RefCell;

use cms::builder::{SignedDataBuilder, SignerInfoBuilder};
use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
use cms::signed_data::{EncapsulatedContentInfo, SignerIdentifier};
use const_oid::db::{rfc5911::ID_DATA, rfc5912::ID_SHA_256};
use const_oid::ObjectIdentifier;
use der::asn1::{Null, OctetString, SetOfVec};
use der::{Any, Decode, Encode};
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::RsaPrivateKey;
use sha2::{Digest, Sha256};
use signature::Keypair;
use x509_cert::attr::Attribute;
use x509_cert::spki::{self, AlgorithmIdentifierOwned, DynSignatureAlgorithmIdentifier};
use x509_cert::Certificate;

/// `sha256WithRSAEncryption` (PKCS#1): `1.2.840.113549.1.1.11`.
const SHA256_WITH_RSA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.11");
/// `id-aa-ets-sigPolicyId` (RFC 5126, PAdES-EPES): `1.2.840.113549.1.9.16.2.15`.
const ID_AA_ETS_SIG_POLICY_ID: ObjectIdentifier =
    ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.2.15");
/// `id-spq-ets-uri` (SPURI qualifier): `1.2.840.113549.1.9.16.5.1`.
const ID_SPQ_ETS_URI: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.5.1");
/// SHA-256 (the default policy hash algorithm): `2.16.840.1.101.3.4.2.1`.
const SHA256_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.16.840.1.101.3.4.2.1");

use cos::{Dict, Object, Reference, Stream};
use parser::PdfReader;

use crate::image::image_xobject_dict;

/// Errors from signing/timestamping.
#[derive(Debug)]
pub enum SignError {
    Parse(String),
    Key(String),
    Cms(String),
    SignatureTooLarge {
        got: usize,
        reserved: usize,
    },
    Structure(String),
    /// Signing/timestamping/LTV used without a valid license.
    License(license::LicenseError),
}

impl std::fmt::Display for SignError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignError::Parse(s) => write!(f, "sign parse error: {s}"),
            SignError::Key(s) => write!(f, "sign key error: {s}"),
            SignError::Cms(s) => write!(f, "sign CMS error: {s}"),
            SignError::SignatureTooLarge { got, reserved } => {
                write!(f, "signature {got} bytes exceeds reserved {reserved}")
            }
            SignError::Structure(s) => write!(f, "sign structure error: {s}"),
            SignError::License(e) => write!(f, "{e}"),
        }
    }
}

impl From<license::LicenseError> for SignError {
    fn from(e: license::LicenseError) -> Self {
        SignError::License(e)
    }
}

impl std::error::Error for SignError {}

/// An RSA signer (also used as a TSA): private key + certificate + chain.
pub struct Signer {
    signing_key: SigningKey<Sha256>,
    cert: Certificate,
    chain: Vec<Certificate>,
}

impl Signer {
    /// Load a PKCS#8 DER private key and a DER X.509 certificate.
    pub fn from_pkcs8_der(key_der: &[u8], cert_der: &[u8]) -> Result<Signer, SignError> {
        Self::from_pkcs8_der_with_chain(key_der, cert_der, &[])
    }

    /// Like [`Signer::from_pkcs8_der`] but also embeds a certificate `chain`.
    pub fn from_pkcs8_der_with_chain(
        key_der: &[u8],
        cert_der: &[u8],
        chain: &[Vec<u8>],
    ) -> Result<Signer, SignError> {
        let key = RsaPrivateKey::from_pkcs8_der(key_der)
            .map_err(|e| SignError::Key(format!("private key: {e}")))?;
        let cert = Certificate::from_der(cert_der)
            .map_err(|e| SignError::Key(format!("certificate: {e}")))?;
        let chain = chain
            .iter()
            .map(|c| Certificate::from_der(c).map_err(|e| SignError::Key(format!("chain: {e}"))))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Signer {
            signing_key: SigningKey::<Sha256>::new(key),
            cert,
            chain,
        })
    }
}

/// A visible signature appearance.
#[derive(Debug, Clone, Default)]
pub struct VisibleSignature {
    pub page: usize,
    pub rect: [f64; 4],
    pub lines: Vec<String>,
    /// Optional raw PNG or JPEG bytes of a handwritten-signature / logo image,
    /// drawn (aspect-fit, centered) inside the appearance rectangle. Any
    /// [`lines`](VisibleSignature::lines) are drawn on top.
    pub image: Option<Vec<u8>>,
}

/// Decode raw image bytes (PNG or JPEG, sniffed by magic number) for embedding
/// in a visible-signature appearance.
fn decode_signature_image(data: &[u8]) -> Option<images::Image> {
    if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        images::Image::from_png(data).ok()
    } else if data.starts_with(&[0xFF, 0xD8]) {
        images::Image::from_jpeg(data.to_vec()).ok()
    } else {
        None
    }
}

/// What a **certifying** (DocMDP) signature still allows. Set this on the
/// *first* signature of a document to lock down later changes — it writes
/// `/Perms /DocMDP` with the matching `/TransformParams /P`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Certify {
    /// `/P 1` — the document is sealed; no changes are permitted after signing.
    Locked,
    /// `/P 2` — only form-filling and further signing are permitted.
    Forms,
    /// `/P 3` — form-filling, signing **and** annotation changes are permitted.
    FormsAndAnnotations,
}

impl Certify {
    fn p(self) -> u8 {
        match self {
            Certify::Locked => 1,
            Certify::Forms => 2,
            Certify::FormsAndAnnotations => 3,
        }
    }
}

/// A signature-policy identifier (PAdES-EPES, RFC 5126 — the building block for
/// ICP-Brasil's *Política de Assinatura*, e.g. AD-RB). Embedded as the
/// `id-aa-ets-sigPolicyId` signed attribute.
#[derive(Debug, Clone)]
pub struct SignaturePolicy {
    /// The policy OID (dotted-decimal string), e.g. the ICP-Brasil AD-RB OID.
    pub oid: String,
    /// The policy document's hash (its digest under `hash_algorithm_oid`).
    pub hash: Vec<u8>,
    /// The hash algorithm OID for `hash`; `None` means SHA-256.
    pub hash_algorithm_oid: Option<String>,
    /// Optional SPURI qualifier — the URI where the policy can be retrieved.
    pub uri: Option<String>,
}

/// Signature metadata + optional visible appearance.
#[derive(Debug, Clone, Default)]
pub struct SignOptions {
    pub reason: Option<String>,
    pub location: Option<String>,
    pub name: Option<String>,
    /// PDF date string for `/M`. Defaults to a fixed value for reproducibility.
    pub date: Option<String>,
    pub visible: Option<VisibleSignature>,
    /// Produce a PAdES-B-B signature (ETSI subfilter + signing-certificate-v2).
    pub pades: bool,
    /// Certify the document (DocMDP). Use only on the **first** signature.
    pub certification: Option<Certify>,
    /// Embed a signature-policy identifier (PAdES-EPES / ICP-Brasil AD-RB).
    pub policy: Option<SignaturePolicy>,
    /// Override the bytes reserved for the `/Contents` CMS container. Cloud-HSM
    /// containers vary in size; raise this if signing fails with
    /// [`SignError::SignatureTooLarge`]. Defaults to 8192.
    pub estimated_size: Option<usize>,
}

const RESERVED: usize = 8192;
const BYTERANGE_FIELD: usize = 48;

/// Sign `pdf` with a local [`Signer`] (key + cert held in process). Signing an
/// already-signed PDF adds a further signature as a new incremental revision.
///
/// For HSM / "bring-your-own-signer" flows where the private key must never
/// reach this library, use [`sign_with`] (Model A — signature callback) or
/// [`begin_signing`] + [`SigningSession::complete`] (Model B — two-phase).
pub fn sign(pdf: &[u8], signer: &Signer, opts: &SignOptions) -> Result<Vec<u8>, SignError> {
    crate::require(license::Feature::Signatures)?;
    let prepared = begin_signing(pdf, opts)?;
    let der = build_pkcs7(
        signer,
        prepared.signed_bytes(),
        opts.pades,
        opts.policy.as_ref(),
    )?;
    prepared.complete(&der)
}

/// **Model A — external signer callback.** Sign `pdf` without ever handing this
/// library a private key: it computes the `ByteRange`, builds the CMS signed
/// attributes, then calls `sign_raw` with the exact bytes to be signed. The
/// callback returns the raw RSA PKCS#1 v1.5 signature (over SHA-256 of those
/// bytes) produced by a remote HSM (Azure Key Vault, VIDaaS, BirdID, …); the
/// library assembles the CMS `SignedData` and embeds it.
///
/// `cert_der` is the signer certificate; `chain` are intermediate certificates
/// (each DER-encoded), supplied independently of the key. For an async HSM call
/// drive [`begin_signing`]/[`SigningSession::complete`] instead.
pub fn sign_with<F>(
    pdf: &[u8],
    cert_der: &[u8],
    chain: &[Vec<u8>],
    opts: &SignOptions,
    sign_raw: F,
) -> Result<Vec<u8>, SignError>
where
    F: Fn(&[u8]) -> Result<Vec<u8>, SignError>,
{
    crate::require(license::Feature::Signatures)?;
    let cert =
        Certificate::from_der(cert_der).map_err(|e| SignError::Key(format!("certificate: {e}")))?;
    let chain_certs = chain
        .iter()
        .map(|c| Certificate::from_der(c).map_err(|e| SignError::Key(format!("chain: {e}"))))
        .collect::<Result<Vec<_>, _>>()?;
    let prepared = begin_signing(pdf, opts)?;
    let der = build_pkcs7_with(
        &cert,
        &chain_certs,
        prepared.signed_bytes(),
        opts.pades,
        opts.policy.as_ref(),
        &sign_raw,
    )?;
    prepared.complete(&der)
}

/// **Model B — two-phase / detached container.** Phase 1: prepare `pdf` for
/// signing, returning a [`SigningSession`] whose [`hash`](SigningSession::hash)
/// / [`signed_bytes`](SigningSession::signed_bytes) you send to the HSM
/// (possibly across an async or HTTP boundary). Once you have a complete CMS /
/// PKCS#7 container, call [`SigningSession::complete`] to inject it into the
/// reserved placeholder (phase 2). The key never touches this library.
pub fn begin_signing(pdf: &[u8], opts: &SignOptions) -> Result<SigningSession, SignError> {
    crate::require(license::Feature::Signatures)?;
    let subfilter = if opts.pades {
        "ETSI.CAdES.detached"
    } else {
        "adbe.pkcs7.detached"
    };
    let date = opts
        .date
        .clone()
        .unwrap_or_else(|| "D:20260625000000Z".to_string());
    let mut extra = format!(" /M ({date})");
    if let Some(r) = &opts.reason {
        extra.push_str(&format!(" /Reason ({})", escape(r)));
    }
    if let Some(l) = &opts.location {
        extra.push_str(&format!(" /Location ({})", escape(l)));
    }
    if let Some(n) = &opts.name {
        extra.push_str(&format!(" /Name ({})", escape(n)));
    }
    prepare_internal(
        pdf,
        PrepareParams {
            sig_type: "Sig",
            subfilter,
            field_prefix: "Signature",
            extra: extra.into_bytes(),
            visible: opts.visible.as_ref(),
            certification: opts.certification,
            reserved: opts.estimated_size.unwrap_or(RESERVED),
        },
    )
}

/// Stateless **phase 2** for two-phase signing: embed a complete DER CMS /
/// PKCS#7 `container` into the **last** zero-filled `/Contents` placeholder of a
/// [`begin_signing`]-prepared `document`. Use this when phase 1 (prepare)
/// and phase 2 (embed) run in different processes and only the prepared bytes
/// crossed the boundary; otherwise [`SigningSession::complete`] is more direct.
pub fn complete_signing(document: &[u8], container: &[u8]) -> Result<Vec<u8>, SignError> {
    let a = rfind_sub(document, b"/Contents <")
        .map(|p| p + b"/Contents ".len())
        .ok_or_else(|| SignError::Structure("no /Contents placeholder".into()))?;
    // `a` indexes the '<'; find the closing '>'.
    let close = document[a..]
        .iter()
        .position(|&b| b == b'>')
        .map(|i| a + i)
        .ok_or_else(|| SignError::Structure("unterminated /Contents".into()))?;
    let reserved = (close - a - 1) / 2;
    let hex = hex_encode(container);
    if hex.len() > reserved * 2 {
        return Err(SignError::SignatureTooLarge {
            got: container.len(),
            reserved,
        });
    }
    let mut out = document.to_vec();
    out[a + 1..a + 1 + hex.len()].copy_from_slice(hex.as_bytes());
    Ok(out)
}

/// Append an RFC 3161 document timestamp signed by `tsa` (PAdES-B-LTA block).
pub fn timestamp(pdf: &[u8], tsa: &Signer, date: Option<&str>) -> Result<Vec<u8>, SignError> {
    crate::require(license::Feature::Signatures)?;
    let gen_time = date.unwrap_or("20260625000000Z").to_string();
    let prepared = prepare_internal(
        pdf,
        PrepareParams {
            sig_type: "DocTimeStamp",
            subfilter: "ETSI.RFC3161",
            field_prefix: "Timestamp",
            extra: Vec::new(),
            visible: None,
            certification: None,
            reserved: RESERVED,
        },
    )?;
    let der = build_timestamp_token(tsa, prepared.signed_bytes(), &gen_time)?;
    prepared.complete(&der)
}

/// Begin a **document timestamp** against a *network* RFC 3161 TSA (PAdES-B-T /
/// AD-RT). Prepares the `/DocTimeStamp` incremental update with a placeholder
/// `/Contents` and returns a [`SigningSession`]; the timestamp authority — not
/// this library — performs the network round-trip, keeping the core offline and
/// dependency-free:
///
/// 1. `let s = begin_timestamp(pdf)?;`
/// 2. `let req = timestamp_request(&s.hash(), None, true);`
/// 3. POST `req` to the TSA (`Content-Type: application/timestamp-query`) and
///    read the `application/timestamp-reply` body.
/// 4. `let token = timestamp_token_from_response(&reply)?;`
/// 5. `let out = s.complete(&token)?;`
pub fn begin_timestamp(pdf: &[u8]) -> Result<SigningSession, SignError> {
    crate::require(license::Feature::Signatures)?;
    prepare_internal(
        pdf,
        PrepareParams {
            sig_type: "DocTimeStamp",
            subfilter: "ETSI.RFC3161",
            field_prefix: "Timestamp",
            extra: Vec::new(),
            visible: None,
            certification: None,
            reserved: RESERVED,
        },
    )
}

/// Build an RFC 3161 `TimeStampReq` (DER) for `imprint` (the SHA-256 of the
/// bytes to timestamp — e.g. [`SigningSession::hash`]). `nonce` is optional
/// anti-replay; `cert_req` asks the TSA to embed its certificate in the token
/// (set it `true` so the token is self-contained for later verification/LTV).
pub fn timestamp_request(imprint: &[u8], nonce: Option<&[u8]>, cert_req: bool) -> Vec<u8> {
    // hashAlgorithm: SEQUENCE { OID sha256, NULL }
    let mut alg = tlv(0x06, ID_SHA_256.as_bytes());
    alg.extend(tlv(0x05, &[])); // NULL parameters
    let alg = tlv(0x30, &alg);
    // MessageImprint: SEQUENCE { hashAlgorithm, hashedMessage OCTET STRING }
    let mut mi = alg;
    mi.extend(tlv(0x04, imprint));
    let mi = tlv(0x30, &mi);
    // TimeStampReq: SEQUENCE { version 1, messageImprint, [nonce], certReq }
    let mut body = tlv(0x02, &[1]); // version v1
    body.extend(mi);
    if let Some(n) = nonce {
        body.extend(tlv(0x02, n));
    }
    if cert_req {
        body.extend(tlv(0x01, &[0xFF])); // certReq TRUE (DEFAULT FALSE)
    }
    tlv(0x30, &body)
}

/// Extract the `TimeStampToken` (a CMS `ContentInfo`, ready for `/Contents`)
/// from a TSA's RFC 3161 `TimeStampResp`. Errors if the response status is not
/// *granted* / *grantedWithMods* or no token is present.
///
/// `TimeStampResp ::= SEQUENCE { status PKIStatusInfo, timeStampToken ContentInfo OPTIONAL }`
pub fn timestamp_token_from_response(response: &[u8]) -> Result<Vec<u8>, SignError> {
    let err = || SignError::Structure("malformed RFC 3161 TimeStampResp".into());
    // Outer SEQUENCE.
    let (tag, hdr, len) = der_header(response).ok_or_else(err)?;
    if tag != 0x30 {
        return Err(err());
    }
    let body = response.get(hdr..hdr + len).ok_or_else(err)?;
    // First element: PKIStatusInfo (SEQUENCE) — its first field is the status INTEGER.
    let (st_tag, st_hdr, st_len) = der_header(body).ok_or_else(err)?;
    if st_tag != 0x30 {
        return Err(err());
    }
    let status_info = body.get(st_hdr..st_hdr + st_len).ok_or_else(err)?;
    let (status_tag, status_hdr, status_len) = der_header(status_info).ok_or_else(err)?;
    if status_tag == 0x02 {
        let status = status_info
            .get(status_hdr..status_hdr + status_len)
            .and_then(|b| b.last())
            .copied()
            .unwrap_or(0xFF);
        // 0 = granted, 1 = grantedWithMods; anything else is a rejection.
        if status != 0 && status != 1 {
            return Err(SignError::Structure(format!(
                "TSA rejected the request (PKIStatus {status})"
            )));
        }
    }
    // The remaining bytes after PKIStatusInfo are the timeStampToken ContentInfo.
    let token = body.get(st_hdr + st_len..).ok_or_else(err)?;
    if token.is_empty() || der_header(token).map(|(t, _, _)| t) != Some(0x30) {
        return Err(SignError::Structure(
            "TimeStampResp carried no token".into(),
        ));
    }
    Ok(token.to_vec())
}

/// Read one DER TLV header: returns `(tag, header_len, content_len)`.
fn der_header(b: &[u8]) -> Option<(u8, usize, usize)> {
    let tag = *b.first()?;
    let l0 = *b.get(1)?;
    if l0 < 0x80 {
        Some((tag, 2, l0 as usize))
    } else {
        let n = (l0 & 0x7f) as usize;
        if n == 0 || n > 4 {
            return None;
        }
        let mut len = 0usize;
        for i in 0..n {
            len = (len << 8) | *b.get(2 + i)? as usize;
        }
        Some((tag, 2 + n, len))
    }
}

/// Add a Document Security Store (`/DSS`) with validation `certs` and `crls`
/// (each DER-encoded) — the PAdES-B-LT material for long-term validation.
pub fn add_dss(pdf: &[u8], certs: &[Vec<u8>], crls: &[Vec<u8>]) -> Result<Vec<u8>, SignError> {
    crate::require(license::Feature::Signatures)?;
    let reader = PdfReader::parse(pdf).map_err(|e| SignError::Parse(e.to_string()))?;
    let catalog_num = ref_num(reader.trailer().get("Root"))
        .ok_or_else(|| SignError::Structure("no /Root".into()))?;
    let catalog = dict_of(reader.get(catalog_num))
        .ok_or_else(|| SignError::Structure("catalog not a dict".into()))?
        .clone();
    let prev = find_startxref(pdf).ok_or_else(|| SignError::Structure("no startxref".into()))?;

    let mut next = reader.object_numbers().max().unwrap_or(catalog_num) + 1;
    let mut alloc = || {
        let n = next;
        next += 1;
        n
    };

    let mut objects: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut cert_refs = Vec::new();
    for der in certs {
        let n = alloc();
        objects.push((n, Object::Stream(Stream::new(der.clone())).to_bytes()));
        cert_refs.push(Object::Reference(Reference::new(n)));
    }
    let mut crl_refs = Vec::new();
    for der in crls {
        let n = alloc();
        objects.push((n, Object::Stream(Stream::new(der.clone())).to_bytes()));
        crl_refs.push(Object::Reference(Reference::new(n)));
    }

    let mut dss = Dict::new();
    if !cert_refs.is_empty() {
        dss.set("Certs", Object::Array(cert_refs));
    }
    if !crl_refs.is_empty() {
        dss.set("CRLs", Object::Array(crl_refs));
    }
    let dss_num = alloc();
    objects.push((dss_num, Object::Dict(dss).to_bytes()));

    let mut new_catalog = catalog.clone();
    new_catalog.set("DSS", Reference::new(dss_num));
    objects.push((catalog_num, Object::Dict(new_catalog).to_bytes()));

    let mut blob = Vec::new();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in &objects {
        offsets.push((*num, pdf.len() + blob.len()));
        blob.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        blob.extend_from_slice(body);
        blob.extend_from_slice(b"\nendobj\n");
    }
    let xref_offset = pdf.len() + blob.len();
    write_incremental_xref(
        &mut blob,
        &mut offsets,
        next,
        catalog_num,
        prev,
        &reader,
        xref_offset,
    );

    let mut out = pdf.to_vec();
    out.extend_from_slice(&blob);
    Ok(out)
}

/// A PDF prepared for **deferred (two-phase) signing**: its `/ByteRange` is
/// fixed and `/Contents` holds a zero-filled placeholder. Send the
/// [`hash`](Self::hash) (or [`signed_bytes`](Self::signed_bytes)) to a
/// remote signer/HSM, build the CMS/PKCS#7 container, then call
/// [`complete`](Self::complete) to inject it. The private key never reaches this
/// library. Produced by [`begin_signing`].
pub struct SigningSession {
    /// The prepared PDF (incremental update with a placeholder `/Contents`).
    document: Vec<u8>,
    /// Byte offset of the `<` opening the `/Contents` hex string.
    contents_offset: usize,
    /// Bytes reserved for the DER container (placeholder is `reserved * 2` hex).
    reserved: usize,
    /// Exactly the bytes the signature covers (the two `/ByteRange` segments).
    signed_bytes: Vec<u8>,
}

impl SigningSession {
    /// The prepared PDF bytes (with a zero-filled `/Contents` placeholder).
    /// Embedding the container yields the final signed PDF.
    pub fn document(&self) -> &[u8] {
        &self.document
    }

    /// The exact bytes the signature covers — the two `/ByteRange` segments
    /// concatenated. Hash and sign these (or use [`hash`](Self::hash)).
    pub fn signed_bytes(&self) -> &[u8] {
        &self.signed_bytes
    }

    /// SHA-256 of [`signed_bytes`](Self::signed_bytes) — the value an HSM signs.
    pub fn hash(&self) -> [u8; 32] {
        Sha256::digest(&self.signed_bytes).into()
    }

    /// The number of bytes reserved for the CMS container.
    pub fn reserved_size(&self) -> usize {
        self.reserved
    }

    /// Phase 2: complete the signature by embedding a finished DER CMS / PKCS#7
    /// `container` into the reserved `/Contents` placeholder, returning the final
    /// signed PDF. Fails with [`SignError::SignatureTooLarge`] if the container
    /// exceeds the reserved space (raise `SignOptions::estimated_size` and
    /// re-open the session).
    pub fn complete(mut self, container: &[u8]) -> Result<Vec<u8>, SignError> {
        let hex = hex_encode(container);
        if hex.len() > self.reserved * 2 {
            return Err(SignError::SignatureTooLarge {
                got: container.len(),
                reserved: self.reserved,
            });
        }
        let a = self.contents_offset;
        self.document[a + 1..a + 1 + hex.len()].copy_from_slice(hex.as_bytes());
        Ok(self.document)
    }
}

/// Internal parameters for [`prepare_internal`].
struct PrepareParams<'a> {
    sig_type: &'a str,
    subfilter: &'a str,
    field_prefix: &'a str,
    extra: Vec<u8>,
    visible: Option<&'a VisibleSignature>,
    certification: Option<Certify>,
    reserved: usize,
}

/// Core: append a signature field + signature dictionary as an incremental
/// update and fix the `/ByteRange`, returning a [`SigningSession`] whose
/// `/Contents` placeholder is ready to receive the CMS container.
fn prepare_internal(pdf: &[u8], p: PrepareParams) -> Result<SigningSession, SignError> {
    let PrepareParams {
        sig_type,
        subfilter,
        field_prefix,
        extra,
        visible,
        certification,
        reserved,
    } = p;
    let reader = PdfReader::parse(pdf).map_err(|e| SignError::Parse(e.to_string()))?;
    let catalog_num = ref_num(reader.trailer().get("Root"))
        .ok_or_else(|| SignError::Structure("no /Root".into()))?;
    let catalog = dict_of(reader.get(catalog_num))
        .ok_or_else(|| SignError::Structure("catalog not a dict".into()))?
        .clone();
    let pages_root =
        ref_num(catalog.get("Pages")).ok_or_else(|| SignError::Structure("no /Pages".into()))?;
    let page_numbers = collect_page_numbers(&reader, pages_root);
    if page_numbers.is_empty() {
        return Err(SignError::Structure("no pages".into()));
    }
    let target_idx = visible
        .map(|v| v.page)
        .unwrap_or(0)
        .min(page_numbers.len() - 1);
    let page_num = page_numbers[target_idx];
    let page = dict_of(reader.get(page_num))
        .ok_or_else(|| SignError::Structure("page not a dict".into()))?
        .clone();
    let prev = find_startxref(pdf).ok_or_else(|| SignError::Structure("no startxref".into()))?;

    let mut next = reader.object_numbers().max().unwrap_or(catalog_num) + 1;
    let mut alloc = || {
        let n = next;
        next += 1;
        n
    };
    let sig_num = alloc();
    let widget_num = alloc();
    let appearance = visible.map(|v| (alloc(), alloc(), v.clone()));

    // AcroForm: reuse and extend an existing one (multi-signature).
    let (acro_num, acro_dict, update_catalog) = match ref_num(catalog.get("AcroForm")) {
        Some(num) => {
            let mut d = dict_of(reader.get(num)).cloned().unwrap_or_default();
            let mut fields = match d.get("Fields").map(|o| reader.resolve(o)) {
                Some(Object::Array(a)) => a.clone(),
                _ => Vec::new(),
            };
            fields.push(Object::Reference(Reference::new(widget_num)));
            d.set("Fields", Object::Array(fields));
            d.set("SigFlags", 3);
            (num, d, false)
        }
        None => {
            let num = alloc();
            let d = Dict::new()
                .with(
                    "Fields",
                    Object::Array(vec![Object::Reference(Reference::new(widget_num))]),
                )
                .with("SigFlags", 3);
            (num, d, true)
        }
    };
    let field_name = format!(
        "{field_prefix}{}",
        existing_field_count(&reader, acro_num) + 1
    );

    let mut objects: Vec<(u32, Vec<u8>)> = Vec::new();

    let mut new_page = page.clone();
    let mut annots = match page.get("Annots").map(|o| reader.resolve(o)) {
        Some(Object::Array(a)) => a.clone(),
        _ => Vec::new(),
    };
    annots.push(Object::Reference(Reference::new(widget_num)));
    new_page.set("Annots", Object::Array(annots));
    objects.push((page_num, Object::Dict(new_page).to_bytes()));

    // Re-emit the catalog if we introduced an AcroForm or are certifying
    // (DocMDP needs `/Perms /DocMDP` pointing at this signature).
    if update_catalog || certification.is_some() {
        let mut c = catalog.clone();
        if update_catalog {
            c.set("AcroForm", Reference::new(acro_num));
        }
        if certification.is_some() {
            c.set(
                "Perms",
                Object::Dict(Dict::new().with("DocMDP", Reference::new(sig_num))),
            );
        }
        objects.push((catalog_num, Object::Dict(c).to_bytes()));
    }
    objects.push((acro_num, Object::Dict(acro_dict).to_bytes()));

    let mut widget = Dict::new()
        .with("Type", Object::name("Annot"))
        .with("Subtype", Object::name("Widget"))
        .with("FT", Object::name("Sig"))
        .with("T", cos::PdfString::literal(field_name.into_bytes()))
        .with("F", 132)
        .with("P", Reference::new(page_num))
        .with("V", Reference::new(sig_num));
    if let Some((form_num, font_num, vis)) = &appearance {
        let [x0, y0, x1, y1] = vis.rect;
        widget.set(
            "Rect",
            Object::Array(vec![x0.into(), y0.into(), x1.into(), y1.into()]),
        );
        widget.set(
            "AP",
            Object::Dict(Dict::new().with("N", Reference::new(*form_num))),
        );
        let (w, h) = (x1 - x0, y1 - y0);

        // Optional embedded image (handwritten signature / logo).
        let mut xobject_res = Dict::new();
        let mut img_dims: Option<(f64, f64)> = None;
        if let Some(img) = vis.image.as_ref().and_then(|b| decode_signature_image(b)) {
            let img_num = alloc();
            let smask_ref = img.soft_mask.as_ref().map(|m| {
                let n = alloc();
                let d = Dict::new()
                    .with("Type", Object::name("XObject"))
                    .with("Subtype", Object::name("Image"))
                    .with("Width", m.width as i64)
                    .with("Height", m.height as i64)
                    .with("ColorSpace", Object::name("DeviceGray"))
                    .with("BitsPerComponent", m.bits_per_component as i64)
                    .with("Filter", Object::name("FlateDecode"));
                objects.push((
                    n,
                    Object::Stream(Stream::with_dict(d, m.data.clone())).to_bytes(),
                ));
                n
            });
            let mut idict = image_xobject_dict(&img);
            if let Some(n) = smask_ref {
                idict.set("SMask", Reference::new(n));
            }
            objects.push((
                img_num,
                Object::Stream(Stream::with_dict(idict, img.data.clone())).to_bytes(),
            ));
            xobject_res.set("SigImg", Reference::new(img_num));
            img_dims = Some((img.width as f64, img.height as f64));
        }

        let mut form_dict = Dict::new()
            .with("Type", Object::name("XObject"))
            .with("Subtype", Object::name("Form"))
            .with(
                "BBox",
                Object::Array(vec![0.into(), 0.into(), w.into(), h.into()]),
            );
        let mut resources = Dict::new().with(
            "Font",
            Object::Dict(Dict::new().with("Helv", Reference::new(*font_num))),
        );
        if !xobject_res.is_empty() {
            resources.set("XObject", Object::Dict(xobject_res));
        }
        form_dict.set("Resources", Object::Dict(resources));
        let font = Dict::new()
            .with("Type", Object::name("Font"))
            .with("Subtype", Object::name("Type1"))
            .with("BaseFont", Object::name("Helvetica"))
            .with("Encoding", Object::name("WinAnsiEncoding"));
        objects.push((
            *form_num,
            Object::Stream(Stream::with_dict(
                form_dict,
                appearance_content(w, h, &vis.lines, img_dims),
            ))
            .to_bytes(),
        ));
        objects.push((*font_num, Object::Dict(font).to_bytes()));
    } else {
        widget.set(
            "Rect",
            Object::Array(vec![0.into(), 0.into(), 0.into(), 0.into()]),
        );
    }
    objects.push((widget_num, Object::Dict(widget).to_bytes()));

    // A certifying signature carries a DocMDP transform reference.
    let mut extra = extra;
    if let Some(level) = certification {
        extra.extend_from_slice(
            format!(
                " /Reference [ << /Type /SigRef /TransformMethod /DocMDP \
                 /TransformParams << /Type /TransformParams /P {} /V /1.2 >> >> ]",
                level.p()
            )
            .as_bytes(),
        );
    }
    objects.push((
        sig_num,
        sig_object_body(sig_type, subfilter, &extra, reserved),
    ));

    // Assemble incremental blob.
    let mut blob = Vec::new();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in &objects {
        offsets.push((*num, pdf.len() + blob.len()));
        blob.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        blob.extend_from_slice(body);
        blob.extend_from_slice(b"\nendobj\n");
    }
    let xref_offset = pdf.len() + blob.len();
    write_incremental_xref(
        &mut blob,
        &mut offsets,
        next,
        catalog_num,
        prev,
        &reader,
        xref_offset,
    );

    let mut file = pdf.to_vec();
    file.extend_from_slice(&blob);

    // The newest /Contents is the one we just wrote.
    let a = rfind_sub(&file, b"/Contents <")
        .map(|p| p + b"/Contents ".len())
        .ok_or_else(|| SignError::Structure("contents placeholder lost".into()))?;
    let gap = reserved * 2 + 2;
    let seg2_start = a + gap;
    let seg2_len = file.len() - seg2_start;
    overwrite_last_byterange(&mut file, &format!("0 {a} {seg2_start} {seg2_len}"))?;

    let mut signed = Vec::with_capacity(a + seg2_len);
    signed.extend_from_slice(&file[..a]);
    signed.extend_from_slice(&file[seg2_start..]);

    Ok(SigningSession {
        document: file,
        contents_offset: a,
        reserved,
        signed_bytes: signed,
    })
}

fn appearance_content(w: f64, h: f64, lines: &[String], image: Option<(f64, f64)>) -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"q\n");
    // Embedded image (aspect-fit, centered) drawn behind any text.
    if let Some((iw, ih)) = image {
        if iw > 0.0 && ih > 0.0 {
            let scale = (w / iw).min(h / ih);
            let (dw, dh) = (iw * scale, ih * scale);
            let (dx, dy) = ((w - dw) / 2.0, (h - dh) / 2.0);
            s.extend_from_slice(
                format!("q {dw:.2} 0 0 {dh:.2} {dx:.2} {dy:.2} cm /SigImg Do Q\n").as_bytes(),
            );
        }
    }
    s.extend_from_slice(
        format!(
            "0.4 0.4 0.4 RG 0.8 w 0.4 0.4 {:.2} {:.2} re S\n",
            w - 0.8,
            h - 0.8
        )
        .as_bytes(),
    );
    let size = 9.0;
    s.extend_from_slice(b"BT\n");
    s.extend_from_slice(format!("/Helv {size} Tf {} TL 0 0 0 rg\n", size * 1.3).as_bytes());
    s.extend_from_slice(format!("4 {:.2} Td\n", h - size - 4.0).as_bytes());
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            s.extend_from_slice(b"T*\n");
        }
        s.push(b'(');
        s.extend_from_slice(&win_ansi(line));
        s.extend_from_slice(b") Tj\n");
    }
    s.extend_from_slice(b"ET\nQ\n");
    s
}

fn win_ansi(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for ch in text.chars() {
        // Transcode to WinAnsi (CP1252), covering the 0x80–0x9F typographic
        // block (em dash, smart quotes, €, …) — not just `ch as u8`, which would
        // turn an em dash into '?' in the visible-signature appearance.
        let b = crate::helvetica::unicode_to_winansi(ch).unwrap_or(b'?');
        match b {
            b'(' | b')' | b'\\' => {
                out.push(b'\\');
                out.push(b);
            }
            _ => out.push(b),
        }
    }
    out
}

fn sig_object_body(sig_type: &str, subfilter: &str, extra: &[u8], reserved: usize) -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(
        format!("<< /Type /{sig_type} /Filter /Adobe.PPKLite /SubFilter /{subfilter}").as_bytes(),
    );
    s.extend_from_slice(b" /ByteRange [");
    s.extend_from_slice(&[b' '; BYTERANGE_FIELD]);
    s.extend_from_slice(b"]");
    s.extend_from_slice(b" /Contents <");
    s.extend(std::iter::repeat_n(b'0', reserved * 2));
    s.extend_from_slice(b">");
    s.extend_from_slice(extra);
    s.extend_from_slice(b" >>");
    s
}

fn overwrite_last_byterange(file: &mut [u8], value: &str) -> Result<(), SignError> {
    let pos = rfind_sub(file, b"/ByteRange [")
        .map(|p| p + b"/ByteRange [".len())
        .ok_or_else(|| SignError::Structure("ByteRange placeholder lost".into()))?;
    let bytes = value.as_bytes();
    if bytes.len() > BYTERANGE_FIELD {
        return Err(SignError::Structure("ByteRange too long".into()));
    }
    for i in 0..BYTERANGE_FIELD {
        file[pos + i] = if i < bytes.len() { bytes[i] } else { b' ' };
    }
    Ok(())
}

fn build_pkcs7(
    signer: &Signer,
    message: &[u8],
    pades: bool,
    policy: Option<&SignaturePolicy>,
) -> Result<Vec<u8>, SignError> {
    build_signed_data(
        &signer.signing_key,
        &signer.cert,
        &signer.chain,
        message,
        pades,
        policy,
    )
}

/// Build a detached CMS `SignedData` over `message`, signing with `key`. The
/// signer `S` is either an in-process [`SigningKey`] ([`build_pkcs7`]) or an
/// [`DelegatedSigner`] delegating to a remote HSM ([`build_pkcs7_with`]) —
/// both paths produce identical structure for the same key.
fn build_signed_data<S>(
    key: &S,
    cert: &Certificate,
    chain: &[Certificate],
    message: &[u8],
    pades: bool,
    policy: Option<&SignaturePolicy>,
) -> Result<Vec<u8>, SignError>
where
    S: Keypair + DynSignatureAlgorithmIdentifier + signature::Signer<rsa::pkcs1v15::Signature>,
{
    let digest = Sha256::digest(message);
    let content = EncapsulatedContentInfo {
        econtent_type: ID_DATA,
        econtent: None,
    };
    let digest_alg = AlgorithmIdentifierOwned {
        oid: ID_SHA_256,
        parameters: None,
    };
    let sid = signer_id(cert);
    let mut signer_info = SignerInfoBuilder::new(
        key,
        sid,
        digest_alg.clone(),
        &content,
        Some(digest.as_slice()),
    )
    .map_err(|e| SignError::Cms(e.to_string()))?;
    if pades {
        signer_info
            .add_signed_attribute(signing_certificate_v2(cert)?)
            .map_err(|e| SignError::Cms(e.to_string()))?;
    }
    if let Some(pol) = policy {
        signer_info
            .add_signed_attribute(signature_policy_identifier(pol)?)
            .map_err(|e| SignError::Cms(e.to_string()))?;
    }

    let mut builder = SignedDataBuilder::new(&content);
    builder
        .add_digest_algorithm(digest_alg)
        .map_err(|e| SignError::Cms(e.to_string()))?
        .add_certificate(CertificateChoices::Certificate(cert.clone()))
        .map_err(|e| SignError::Cms(e.to_string()))?;
    for c in chain {
        builder
            .add_certificate(CertificateChoices::Certificate(c.clone()))
            .map_err(|e| SignError::Cms(e.to_string()))?;
    }
    builder
        .add_signer_info::<S, rsa::pkcs1v15::Signature>(signer_info)
        .map_err(|e| SignError::Cms(e.to_string()))?
        .build()
        .map_err(|e| SignError::Cms(e.to_string()))?
        .to_der()
        .map_err(|e| SignError::Cms(e.to_string()))
}

/// A callback yielding the raw RSA PKCS#1 v1.5 signature (over SHA-256 of the
/// supplied bytes) — the "bring-your-own-signer" hook backed by a remote HSM.
type SignHashFn<'a> = dyn Fn(&[u8]) -> Result<Vec<u8>, SignError> + 'a;

/// Build a detached CMS `SignedData` where the RSA signature value comes from
/// an external callback (Model A — see [`sign_with`]).
fn build_pkcs7_with(
    cert: &Certificate,
    chain: &[Certificate],
    message: &[u8],
    pades: bool,
    policy: Option<&SignaturePolicy>,
    sign_raw: &SignHashFn,
) -> Result<Vec<u8>, SignError> {
    let signer = DelegatedSigner::new(sign_raw);
    let result = build_signed_data(&signer, cert, chain, message, pades, policy);
    // Surface the callback's own error rather than the opaque CMS error.
    if let Some(err) = signer.take_error() {
        return Err(err);
    }
    result
}

/// A CMS signer whose signature value is produced by a caller-supplied callback
/// (a remote HSM). It contributes only the `sha256WithRSAEncryption` algorithm
/// identifier and the raw signature bytes — no private key is held.
struct DelegatedSigner<'a> {
    sign_raw: &'a SignHashFn<'a>,
    /// Captures a callback failure so the opaque [`signature::Error`] the CMS
    /// builder surfaces can be replaced with the real cause.
    error: RefCell<Option<SignError>>,
}

impl<'a> DelegatedSigner<'a> {
    fn new(sign_raw: &'a SignHashFn<'a>) -> Self {
        DelegatedSigner {
            sign_raw,
            error: RefCell::new(None),
        }
    }

    fn take_error(&self) -> Option<SignError> {
        self.error.borrow_mut().take()
    }
}

impl Keypair for DelegatedSigner<'_> {
    // The CMS build path never inspects the verifying key, so a unit suffices.
    type VerifyingKey = ();
    fn verifying_key(&self) {}
}

impl DynSignatureAlgorithmIdentifier for DelegatedSigner<'_> {
    fn signature_algorithm_identifier(&self) -> Result<AlgorithmIdentifierOwned, spki::Error> {
        Ok(AlgorithmIdentifierOwned {
            oid: SHA256_WITH_RSA,
            parameters: Some(Any::from(Null)),
        })
    }
}

impl signature::Signer<rsa::pkcs1v15::Signature> for DelegatedSigner<'_> {
    fn try_sign(&self, msg: &[u8]) -> Result<rsa::pkcs1v15::Signature, signature::Error> {
        match (self.sign_raw)(msg) {
            Ok(bytes) => rsa::pkcs1v15::Signature::try_from(bytes.as_slice()).map_err(|e| {
                *self.error.borrow_mut() =
                    Some(SignError::Cms(format!("invalid external signature: {e}")));
                signature::Error::new()
            }),
            Err(e) => {
                *self.error.borrow_mut() = Some(e);
                Err(signature::Error::new())
            }
        }
    }
}

/// ESS signature-policy-identifier signed attribute (PAdES-EPES, RFC 5126).
fn signature_policy_identifier(pol: &SignaturePolicy) -> Result<Attribute, SignError> {
    let policy_oid: ObjectIdentifier = pol
        .oid
        .parse()
        .map_err(|e| SignError::Cms(format!("policy OID: {e}")))?;
    let hash_oid: ObjectIdentifier = match &pol.hash_algorithm_oid {
        Some(s) => s
            .parse()
            .map_err(|e| SignError::Cms(format!("policy hash OID: {e}")))?,
        None => SHA256_OID,
    };

    // SigPolicyId ::= OBJECT IDENTIFIER
    let sig_policy_id = tlv(0x06, policy_oid.as_bytes());
    // OtherHashAlgAndValue ::= SEQUENCE { hashAlgorithm AlgorithmIdentifier, hashValue OCTET STRING }
    let hash_alg = tlv(0x30, &tlv(0x06, hash_oid.as_bytes()));
    let hash_value = tlv(0x04, &pol.hash);
    let sig_policy_hash = tlv(0x30, &[hash_alg, hash_value].concat());

    let mut body = [sig_policy_id, sig_policy_hash].concat();
    if let Some(uri) = &pol.uri {
        // SigPolicyQualifierInfo { id-spq-ets-uri, SPuri (IA5String) }
        let qualifier = tlv(0x16, uri.as_bytes());
        let qinfo = tlv(
            0x30,
            &[tlv(0x06, ID_SPQ_ETS_URI.as_bytes()), qualifier].concat(),
        );
        body.extend_from_slice(&tlv(0x30, &qinfo)); // SEQUENCE OF SigPolicyQualifierInfo
    }
    // SignaturePolicy CHOICE -> signaturePolicyId (SignaturePolicyId SEQUENCE)
    let sig_policy = tlv(0x30, &body);

    let any = Any::from_der(&sig_policy).map_err(|e| SignError::Cms(e.to_string()))?;
    let values = SetOfVec::try_from(vec![any]).map_err(|e| SignError::Cms(e.to_string()))?;
    Ok(Attribute {
        oid: ID_AA_ETS_SIG_POLICY_ID,
        values,
    })
}

/// Build an RFC 3161 TimeStampToken (a CMS SignedData encapsulating TSTInfo)
/// over the SHA-256 of `data`, signed by the TSA `signer`.
fn build_timestamp_token(
    signer: &Signer,
    data: &[u8],
    gen_time: &str,
) -> Result<Vec<u8>, SignError> {
    let imprint = Sha256::digest(data);

    // MessageImprint ::= SEQUENCE { hashAlgorithm AlgorithmIdentifier, hashedMessage OCTET STRING }
    let hash_alg = tlv(0x30, &tlv(0x06, ID_SHA_256.as_bytes()));
    let hashed = tlv(0x04, imprint.as_slice());
    let message_imprint = tlv(0x30, &[hash_alg, hashed].concat());

    // TSTInfo ::= SEQUENCE { version INTEGER(1), policy OID, messageImprint,
    //   serialNumber INTEGER, genTime GeneralizedTime }
    let version = tlv(0x02, &[0x01]);
    let policy = tlv(
        0x06,
        ObjectIdentifier::new_unwrap("1.3.6.1.4.1.99999.1.1").as_bytes(),
    );
    let serial = tlv(0x02, &[0x01]);
    let gentime = tlv(0x18, gen_time.as_bytes());
    let tstinfo = tlv(
        0x30,
        &[version, policy, message_imprint, serial, gentime].concat(),
    );

    // Encapsulate TSTInfo as the eContent OCTET STRING.
    let os = OctetString::new(tstinfo).map_err(|e| SignError::Cms(e.to_string()))?;
    let econtent = Any::from_der(&os.to_der().map_err(|e| SignError::Cms(e.to_string()))?)
        .map_err(|e| SignError::Cms(e.to_string()))?;
    let content = EncapsulatedContentInfo {
        econtent_type: ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.1.4"), // id-ct-TSTInfo
        econtent: Some(econtent),
    };
    let digest_alg = AlgorithmIdentifierOwned {
        oid: ID_SHA_256,
        parameters: None,
    };
    let signer_info = SignerInfoBuilder::new(
        &signer.signing_key,
        signer_id(&signer.cert),
        digest_alg.clone(),
        &content,
        None, // encapsulated content -> digest computed by the builder
    )
    .map_err(|e| SignError::Cms(e.to_string()))?;

    SignedDataBuilder::new(&content)
        .add_digest_algorithm(digest_alg)
        .map_err(|e| SignError::Cms(e.to_string()))?
        .add_certificate(CertificateChoices::Certificate(signer.cert.clone()))
        .map_err(|e| SignError::Cms(e.to_string()))?
        .add_signer_info::<SigningKey<Sha256>, rsa::pkcs1v15::Signature>(signer_info)
        .map_err(|e| SignError::Cms(e.to_string()))?
        .build()
        .map_err(|e| SignError::Cms(e.to_string()))?
        .to_der()
        .map_err(|e| SignError::Cms(e.to_string()))
}

fn signer_id(cert: &Certificate) -> SignerIdentifier {
    SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
        issuer: cert.tbs_certificate.issuer.clone(),
        serial_number: cert.tbs_certificate.serial_number.clone(),
    })
}

/// ESS signing-certificate-v2 signed attribute (RFC 5035), PAdES-B-B.
fn signing_certificate_v2(cert: &Certificate) -> Result<Attribute, SignError> {
    let cert_der = cert.to_der().map_err(|e| SignError::Cms(e.to_string()))?;
    let hash = Sha256::digest(&cert_der);
    let cert_hash = tlv(0x04, hash.as_slice());
    let esscertidv2 = tlv(0x30, &cert_hash);
    let certs = tlv(0x30, &esscertidv2);
    let scv2 = tlv(0x30, &certs);
    let any = Any::from_der(&scv2).map_err(|e| SignError::Cms(e.to_string()))?;
    let oid = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.2.47");
    let values = SetOfVec::try_from(vec![any]).map_err(|e| SignError::Cms(e.to_string()))?;
    Ok(Attribute { oid, values })
}

fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let len = content.len();
    if len < 0x80 {
        out.push(len as u8);
    } else {
        let mut bytes = len.to_be_bytes().to_vec();
        while bytes.first() == Some(&0) {
            bytes.remove(0);
        }
        out.push(0x80 | bytes.len() as u8);
        out.extend_from_slice(&bytes);
    }
    out.extend_from_slice(content);
    out
}

#[allow(clippy::too_many_arguments)]
fn write_incremental_xref(
    blob: &mut Vec<u8>,
    offsets: &mut [(u32, usize)],
    size: u32,
    root: u32,
    prev: usize,
    reader: &PdfReader,
    xref_offset: usize,
) {
    offsets.sort_by_key(|(n, _)| *n);
    blob.extend_from_slice(b"xref\n");
    let mut i = 0;
    while i < offsets.len() {
        let start = offsets[i].0;
        let mut j = i;
        while j + 1 < offsets.len() && offsets[j + 1].0 == offsets[j].0 + 1 {
            j += 1;
        }
        blob.extend_from_slice(format!("{start} {}\n", j - i + 1).as_bytes());
        for entry in &offsets[i..=j] {
            blob.extend_from_slice(format!("{:010} 00000 n\r\n", entry.1).as_bytes());
        }
        i = j + 1;
    }

    let mut trailer = Dict::new()
        .with("Size", size as i64)
        .with("Root", Reference::new(root))
        .with("Prev", prev as i64);
    if let Some(Object::Reference(r)) = reader.trailer().get("Info") {
        trailer.set("Info", *r);
    }
    if let Some(id) = reader.trailer().get("ID") {
        trailer.set("ID", id.clone());
    }
    blob.extend_from_slice(b"trailer\n");
    blob.extend_from_slice(&Object::Dict(trailer).to_bytes());
    blob.extend_from_slice(b"\nstartxref\n");
    blob.extend_from_slice(format!("{xref_offset}\n").as_bytes());
    blob.extend_from_slice(b"%%EOF\n");
}

// ---- helpers ---------------------------------------------------------------

fn ref_num(obj: Option<&Object>) -> Option<u32> {
    match obj? {
        Object::Reference(r) => Some(r.number),
        _ => None,
    }
}

fn dict_of(obj: Option<&Object>) -> Option<&Dict> {
    match obj? {
        Object::Dict(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

fn existing_field_count(reader: &PdfReader, acro_num: u32) -> usize {
    match dict_of(reader.get(acro_num)).and_then(|d| d.get("Fields").map(|o| reader.resolve(o))) {
        Some(Object::Array(a)) => a.len(),
        _ => 0,
    }
}

fn collect_page_numbers(reader: &PdfReader, pages_root: u32) -> Vec<u32> {
    fn walk(reader: &PdfReader, num: u32, out: &mut Vec<u32>, depth: u32) {
        if depth > 64 {
            return;
        }
        let Some(dict) = dict_of(reader.get(num)) else {
            return;
        };
        let is_pages = matches!(dict.get("Type"), Some(Object::Name(n)) if n.as_str() == "Pages");
        if is_pages {
            if let Some(Object::Array(kids)) = dict.get("Kids") {
                for kid in kids {
                    if let Object::Reference(r) = kid {
                        walk(reader, r.number, out, depth + 1);
                    }
                }
            }
        } else {
            out.push(num);
        }
    }
    let mut out = Vec::new();
    walk(reader, pages_root, &mut out, 0);
    out
}

fn find_startxref(pdf: &[u8]) -> Option<usize> {
    let kw = b"startxref";
    let pos = pdf.windows(kw.len()).rposition(|w| w == kw)?;
    pdf[pos + kw.len()..]
        .iter()
        .skip_while(|b| b.is_ascii_whitespace())
        .take_while(|b| b.is_ascii_digit())
        .map(|&b| b as char)
        .collect::<String>()
        .parse()
        .ok()
}

fn rfind_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).rposition(|w| w == needle)
}

fn hex_encode(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len() * 2);
    for &b in data {
        s.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        s.push(char::from_digit((b & 0xF) as u32, 16).unwrap());
    }
    s
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_request_is_well_formed_der() {
        let imprint = [0x11u8; 32];
        let req = timestamp_request(&imprint, Some(&[0x42]), true);
        // Outer SEQUENCE.
        assert_eq!(req[0], 0x30);
        let (_, hdr, len) = der_header(&req).unwrap();
        assert_eq!(hdr + len, req.len(), "length covers the whole request");
        // Contains the SHA-256 OID and the imprint bytes.
        assert!(req.windows(imprint.len()).any(|w| w == imprint));
        assert!(req
            .windows(ID_SHA_256.as_bytes().len())
            .any(|w| w == ID_SHA_256.as_bytes()));
        // certReq TRUE (BOOLEAN 0xFF) present.
        assert!(req.windows(3).any(|w| w == [0x01, 0x01, 0xFF]));
    }

    #[test]
    fn extracts_token_from_granted_response() {
        // A minimal token ContentInfo (any SEQUENCE will do for the codec test).
        let token = tlv(0x30, &tlv(0x06, &[0x2A]));
        // PKIStatusInfo { status INTEGER 0 }.
        let status_info = tlv(0x30, &tlv(0x02, &[0]));
        let mut body = status_info;
        body.extend_from_slice(&token);
        let resp = tlv(0x30, &body);
        let got = timestamp_token_from_response(&resp).unwrap();
        assert_eq!(got, token, "the ContentInfo token is returned verbatim");
    }

    #[test]
    fn rejected_response_is_an_error() {
        let status_info = tlv(0x30, &tlv(0x02, &[2])); // rejection
        let resp = tlv(0x30, &status_info);
        assert!(timestamp_token_from_response(&resp).is_err());
    }
}

//! Digital signatures and long-term validation (Fase 7.1 / 7.2).
//!
//! * [`sign`] — incremental-update signing with a `ByteRange` placeholder and a
//!   PKCS#7 (CMS) detached signature. Supports invisible/visible signatures,
//!   multiple signatures, certificate chains, and **PAdES-B-B**
//!   (`ETSI.CAdES.detached` + `signing-certificate-v2`).
//! * [`timestamp`] — append an RFC 3161 **document timestamp** (`/DocTimeStamp`,
//!   `ETSI.RFC3161`) signed by a TSA — the PAdES-B-LTA building block. (Works
//!   fully offline with a self-issued TSA; no network needed.)
//! * [`add_dss`] — add a **Document Security Store** (`/DSS`) carrying the
//!   validation certificates and CRLs — the PAdES-B-LT building block.
//!
//! Each operation appends an incremental update (new objects + `xref` chained
//! via `/Prev`); earlier signatures remain valid.

use cms::builder::{SignedDataBuilder, SignerInfoBuilder};
use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
use cms::signed_data::{EncapsulatedContentInfo, SignerIdentifier};
use const_oid::db::{rfc5911::ID_DATA, rfc5912::ID_SHA_256};
use const_oid::ObjectIdentifier;
use der::asn1::{OctetString, SetOfVec};
use der::{Any, Decode, Encode};
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::RsaPrivateKey;
use sha2::{Digest, Sha256};
use x509_cert::attr::Attribute;
use x509_cert::spki::AlgorithmIdentifierOwned;
use x509_cert::Certificate;

use cos::{Dict, Object, Reference, Stream};
use parser::PdfReader;

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
#[derive(Debug, Clone)]
pub struct VisibleSignature {
    pub page: usize,
    pub rect: [f64; 4],
    pub lines: Vec<String>,
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
}

const RESERVED: usize = 8192;
const BYTERANGE_FIELD: usize = 48;

/// Sign `pdf`. Signing an already-signed PDF adds a further signature.
pub fn sign(pdf: &[u8], signer: &Signer, opts: &SignOptions) -> Result<Vec<u8>, SignError> {
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
    let pades = opts.pades;
    append_signature(
        pdf,
        "Sig",
        subfilter,
        "Signature",
        extra.as_bytes(),
        opts.visible.as_ref(),
        |bytes| build_pkcs7(signer, bytes, pades),
    )
}

/// Append an RFC 3161 document timestamp signed by `tsa` (PAdES-B-LTA block).
pub fn timestamp(pdf: &[u8], tsa: &Signer, date: Option<&str>) -> Result<Vec<u8>, SignError> {
    crate::require(license::Feature::Signatures)?;
    let gen_time = date.unwrap_or("20260625000000Z").to_string();
    append_signature(
        pdf,
        "DocTimeStamp",
        "ETSI.RFC3161",
        "Timestamp",
        b"",
        None,
        |bytes| build_timestamp_token(tsa, bytes, &gen_time),
    )
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

/// Core: append a signature field + signature dictionary as an incremental
/// update, set the `ByteRange`, then fill `/Contents` with `make_contents`.
fn append_signature(
    pdf: &[u8],
    sig_type: &str,
    subfilter: &str,
    field_prefix: &str,
    extra: &[u8],
    visible: Option<&VisibleSignature>,
    make_contents: impl Fn(&[u8]) -> Result<Vec<u8>, SignError>,
) -> Result<Vec<u8>, SignError> {
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

    if update_catalog {
        let mut c = catalog.clone();
        c.set("AcroForm", Reference::new(acro_num));
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
        let mut form_dict = Dict::new()
            .with("Type", Object::name("XObject"))
            .with("Subtype", Object::name("Form"))
            .with(
                "BBox",
                Object::Array(vec![0.into(), 0.into(), w.into(), h.into()]),
            );
        form_dict.set(
            "Resources",
            Object::Dict(Dict::new().with(
                "Font",
                Object::Dict(Dict::new().with("Helv", Reference::new(*font_num))),
            )),
        );
        let font = Dict::new()
            .with("Type", Object::name("Font"))
            .with("Subtype", Object::name("Type1"))
            .with("BaseFont", Object::name("Helvetica"))
            .with("Encoding", Object::name("WinAnsiEncoding"));
        objects.push((
            *form_num,
            Object::Stream(Stream::with_dict(
                form_dict,
                appearance_content(w, h, &vis.lines),
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

    objects.push((sig_num, sig_object_body(sig_type, subfilter, extra)));

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
    let gap = RESERVED * 2 + 2;
    let seg2_start = a + gap;
    let seg2_len = file.len() - seg2_start;
    overwrite_last_byterange(&mut file, &format!("0 {a} {seg2_start} {seg2_len}"))?;

    let mut signed = Vec::with_capacity(a + seg2_len);
    signed.extend_from_slice(&file[..a]);
    signed.extend_from_slice(&file[seg2_start..]);

    let der = make_contents(&signed)?;
    let hex = hex_encode(&der);
    if hex.len() > RESERVED * 2 {
        return Err(SignError::SignatureTooLarge {
            got: der.len(),
            reserved: RESERVED,
        });
    }
    file[a + 1..a + 1 + hex.len()].copy_from_slice(hex.as_bytes());
    Ok(file)
}

fn appearance_content(w: f64, h: f64, lines: &[String]) -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(b"q\n");
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
        let b = if (ch as u32) <= 0xFF { ch as u8 } else { b'?' };
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

fn sig_object_body(sig_type: &str, subfilter: &str, extra: &[u8]) -> Vec<u8> {
    let mut s = Vec::new();
    s.extend_from_slice(
        format!("<< /Type /{sig_type} /Filter /Adobe.PPKLite /SubFilter /{subfilter}").as_bytes(),
    );
    s.extend_from_slice(b" /ByteRange [");
    s.extend_from_slice(&[b' '; BYTERANGE_FIELD]);
    s.extend_from_slice(b"]");
    s.extend_from_slice(b" /Contents <");
    s.extend(std::iter::repeat_n(b'0', RESERVED * 2));
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

fn build_pkcs7(signer: &Signer, message: &[u8], pades: bool) -> Result<Vec<u8>, SignError> {
    let digest = Sha256::digest(message);
    let content = EncapsulatedContentInfo {
        econtent_type: ID_DATA,
        econtent: None,
    };
    let digest_alg = AlgorithmIdentifierOwned {
        oid: ID_SHA_256,
        parameters: None,
    };
    let sid = signer_id(&signer.cert);
    let mut signer_info = SignerInfoBuilder::new(
        &signer.signing_key,
        sid,
        digest_alg.clone(),
        &content,
        Some(digest.as_slice()),
    )
    .map_err(|e| SignError::Cms(e.to_string()))?;
    if pades {
        signer_info
            .add_signed_attribute(signing_certificate_v2(&signer.cert)?)
            .map_err(|e| SignError::Cms(e.to_string()))?;
    }

    let mut builder = SignedDataBuilder::new(&content);
    builder
        .add_digest_algorithm(digest_alg)
        .map_err(|e| SignError::Cms(e.to_string()))?
        .add_certificate(CertificateChoices::Certificate(signer.cert.clone()))
        .map_err(|e| SignError::Cms(e.to_string()))?;
    for c in &signer.chain {
        builder
            .add_certificate(CertificateChoices::Certificate(c.clone()))
            .map_err(|e| SignError::Cms(e.to_string()))?;
    }
    builder
        .add_signer_info::<SigningKey<Sha256>, rsa::pkcs1v15::Signature>(signer_info)
        .map_err(|e| SignError::Cms(e.to_string()))?
        .build()
        .map_err(|e| SignError::Cms(e.to_string()))?
        .to_der()
        .map_err(|e| SignError::Cms(e.to_string()))
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

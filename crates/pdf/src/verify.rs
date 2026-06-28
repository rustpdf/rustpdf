//! Signature **validation** (Tier 2, item 5). The companion to [`sign`](crate::sign):
//! given a signed PDF, locate each signature dictionary, recompute the
//! `/ByteRange` digest, parse the `/Contents` CMS (PKCS#7) SignedData, and check
//!
//! * the **CMS signature** is cryptographically valid (RSA PKCS#1 v1.5 over the
//!   signed attributes, using the signer certificate's public key), and
//! * the **`messageDigest`** signed attribute equals the digest of the bytes the
//!   signature actually covers, and
//! * whether the signature **covers the whole document** (no bytes were appended
//!   after it — i.e. it was not modified post-signing).
//!
//! Only RSA + SHA-256 (the algorithm [`Signer`](crate::Signer) produces) is
//! verified cryptographically; other algorithms are reported with
//! `signature_valid = false` rather than erroring. Trust-chain / revocation
//! checking is out of scope (that needs external infrastructure); this answers
//! "is the signature intact and does it cover the document".

use cms::content_info::ContentInfo;
use cms::signed_data::{SignedData, SignerInfo};
use const_oid::ObjectIdentifier;
use der::{Decode, Encode};
use rsa::pkcs1::DecodeRsaPublicKey;
use rsa::pkcs1v15::{Signature as RsaSignature, VerifyingKey};
use rsa::RsaPublicKey;
use sha2::{Digest, Sha256};
use signature::Verifier;
use x509_cert::Certificate;

use cos::Object;
use parser::PdfReader;

use crate::{require, BuildError};
use license::Feature;

/// id-messageDigest (PKCS#9): `1.2.840.113549.1.9.4`.
const ID_MESSAGE_DIGEST: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.4");
/// rsaEncryption: `1.2.840.113549.1.1.1`.
const RSA_ENCRYPTION: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");

/// The result of validating one signature in a PDF.
#[derive(Debug, Clone)]
pub struct SignatureReport {
    /// The signature field's `/T` name, if it could be located.
    pub field_name: Option<String>,
    /// The signature `/SubFilter` (e.g. `adbe.pkcs7.detached`, `ETSI.CAdES.detached`).
    pub sub_filter: String,
    /// The signer certificate subject (RFC 4514 DN), if available.
    pub signer: Option<String>,
    /// The signature covers the entire file — no bytes were appended after it.
    /// A later incremental update (or tampering past the signed range) makes this
    /// `false`.
    pub covers_whole_document: bool,
    /// The `messageDigest` signed attribute matches the digest of the covered
    /// bytes (the document content under the signature is intact).
    pub digest_valid: bool,
    /// The CMS signature is cryptographically valid for the signer certificate.
    pub signature_valid: bool,
    /// The `/ByteRange` the signature covers: `[start1, len1, start2, len2]`.
    pub byte_range: [usize; 4],
}

impl SignatureReport {
    /// Overall verdict: the content is intact **and** the signature verifies.
    /// (Does not assert trust in the signer — see the module docs.)
    pub fn is_valid(&self) -> bool {
        self.digest_valid && self.signature_valid
    }
}

/// Validate every signature in `pdf`. Returns one [`SignatureReport`] per
/// signature, in document order. An empty vector means the file is unsigned.
///
/// Signature validation is a licensed (Enterprise) capability gated behind the
/// **signatures** feature; without a granting license this returns
/// [`BuildError::License`].
pub fn verify_signatures(pdf: impl AsRef<[u8]>) -> Result<Vec<SignatureReport>, BuildError> {
    require(Feature::Signatures)?;
    let pdf = pdf.as_ref();
    let reader = PdfReader::parse(pdf).map_err(|e| BuildError::Parse(e.to_string()))?;

    let mut reports = Vec::new();
    for num in reader.object_numbers() {
        let Some(Object::Dict(dict)) = reader.get(num) else {
            continue;
        };
        // A signature dictionary has both /ByteRange and /Contents.
        let (Some(br), Some(contents)) = (dict.get("ByteRange"), dict.get("Contents")) else {
            continue;
        };
        let Some(byte_range) = byte_range_array(reader.resolve(br)) else {
            continue;
        };
        let Object::String(contents) = reader.resolve(contents) else {
            continue;
        };
        let sub_filter = dict
            .get("SubFilter")
            .and_then(|o| match reader.resolve(o) {
                Object::Name(n) => Some(n.as_str().to_string()),
                _ => None,
            })
            .unwrap_or_default();

        reports.push(verify_one(
            pdf,
            &reader,
            num,
            byte_range,
            contents.as_bytes(),
            sub_filter,
        ));
    }
    Ok(reports)
}

fn verify_one(
    pdf: &[u8],
    reader: &PdfReader,
    sig_num: u32,
    byte_range: [usize; 4],
    contents: &[u8],
    sub_filter: String,
) -> SignatureReport {
    let field_name = find_field_name(reader, sig_num);
    let covers_whole_document = byte_range[0] == 0 && byte_range[2] + byte_range[3] == pdf.len();

    // The bytes the signature covers (the two ByteRange segments concatenated).
    let signed_bytes = covered_bytes(pdf, byte_range);
    let computed_digest = Sha256::digest(&signed_bytes);

    // Parse the CMS SignedData (trimming any trailing placeholder zero padding).
    let mut report = SignatureReport {
        field_name,
        sub_filter,
        signer: None,
        covers_whole_document,
        digest_valid: false,
        signature_valid: false,
        byte_range,
    };
    let Some(signed) = parse_cms(contents) else {
        return report;
    };
    let Some(signer_info) = signed.signer_infos.0.iter().next() else {
        return report;
    };
    let cert = signer_cert(&signed);
    report.signer = cert.as_ref().map(|c| c.tbs_certificate.subject.to_string());

    // messageDigest signed attribute == digest of covered bytes.
    report.digest_valid = message_digest_attr(signer_info)
        .map(|md| md.as_slice() == computed_digest.as_slice())
        .unwrap_or(false);

    // Cryptographic check: RSA verify over the signed attributes (re-encoded as
    // a SET OF), using the signer certificate's RSA public key.
    if let Some(cert) = cert {
        report.signature_valid = verify_signature(signer_info, &cert).unwrap_or(false);
    }
    report
}

/// Concatenate the two `/ByteRange` segments (the actual signed bytes).
fn covered_bytes(pdf: &[u8], br: [usize; 4]) -> Vec<u8> {
    let mut out = Vec::with_capacity(br[1] + br[3]);
    let seg = |start: usize, len: usize, out: &mut Vec<u8>| {
        let end = start.saturating_add(len).min(pdf.len());
        if start < pdf.len() {
            out.extend_from_slice(&pdf[start..end]);
        }
    };
    seg(br[0], br[1], &mut out);
    seg(br[2], br[3], &mut out);
    out
}

/// Parse a `ContentInfo`/`SignedData`, trimming trailing padding after the DER.
fn parse_cms(contents: &[u8]) -> Option<SignedData> {
    let len = der_object_len(contents)?;
    let der = contents.get(..len)?;
    let ci = ContentInfo::from_der(der).ok()?;
    SignedData::from_der(&ci.content.to_der().ok()?).ok()
}

/// The signer certificate from the SignedData certificate set (first one).
fn signer_cert(signed: &SignedData) -> Option<Certificate> {
    use cms::cert::CertificateChoices;
    let certs = signed.certificates.as_ref()?;
    certs.0.iter().find_map(|c| match c {
        CertificateChoices::Certificate(cert) => Some(cert.clone()),
        _ => None,
    })
}

/// The `messageDigest` signed attribute value, if present.
fn message_digest_attr(si: &SignerInfo) -> Option<Vec<u8>> {
    let attrs = si.signed_attrs.as_ref()?;
    for attr in attrs.iter() {
        if attr.oid == ID_MESSAGE_DIGEST {
            let any = attr.values.iter().next()?;
            // The value is an OCTET STRING; its DER content is the digest.
            let os = der::asn1::OctetString::from_der(&any.to_der().ok()?).ok()?;
            return Some(os.as_bytes().to_vec());
        }
    }
    None
}

/// RSA-verify the signed attributes (re-encoded as a SET OF) for `cert`.
fn verify_signature(si: &SignerInfo, cert: &Certificate) -> Option<bool> {
    // Only RSA PKCS#1 v1.5 is checked cryptographically.
    let spki = &cert.tbs_certificate.subject_public_key_info;
    if spki.algorithm.oid != RSA_ENCRYPTION {
        return Some(false);
    }
    let pub_der = spki.subject_public_key.as_bytes()?;
    let public_key = RsaPublicKey::from_pkcs1_der(pub_der).ok()?;
    let vk = VerifyingKey::<Sha256>::new(public_key);

    // The signed value is the DER of the signed attributes as an explicit SET OF.
    let signed_attrs = si.signed_attrs.as_ref()?;
    let to_verify = signed_attrs.to_der().ok()?;
    let signature = RsaSignature::try_from(si.signature.as_bytes()).ok()?;
    Some(vk.verify(&to_verify, &signature).is_ok())
}

/// Find the `/T` of the field whose `/V` points at signature object `sig_num`.
fn find_field_name(reader: &PdfReader, sig_num: u32) -> Option<String> {
    for num in reader.object_numbers() {
        let Some(Object::Dict(d)) = reader.get(num) else {
            continue;
        };
        let points_at_sig = matches!(d.get("V"), Some(Object::Reference(r)) if r.number == sig_num);
        if points_at_sig {
            if let Some(Object::String(t)) = d.get("T") {
                return Some(String::from_utf8_lossy(t.as_bytes()).into_owned());
            }
        }
    }
    None
}

fn byte_range_array(obj: &Object) -> Option<[usize; 4]> {
    let Object::Array(a) = obj else {
        return None;
    };
    if a.len() != 4 {
        return None;
    }
    let mut out = [0usize; 4];
    for (i, el) in a.iter().enumerate() {
        out[i] = match el {
            Object::Integer(n) if *n >= 0 => *n as usize,
            _ => return None,
        };
    }
    Some(out)
}

/// Total length (header + content) of the DER object starting at `b[0]`.
fn der_object_len(b: &[u8]) -> Option<usize> {
    if b.len() < 2 {
        return None;
    }
    let l0 = b[1];
    if l0 < 0x80 {
        return Some(2 + l0 as usize);
    }
    let n = (l0 & 0x7f) as usize;
    if n == 0 || b.len() < 2 + n {
        return None;
    }
    let mut len = 0usize;
    for &byte in &b[2..2 + n] {
        len = (len << 8) | byte as usize;
    }
    Some(2 + n + len)
}

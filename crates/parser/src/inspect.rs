//! Lightweight, read-only document probing (issue #45 P1 #3).
//!
//! Unlike [`crate::PdfReader`], these helpers never decrypt object bodies and
//! never fail on a password-protected file: they read just the header and the
//! `/Encrypt` dictionary so a caller can report a document's version and
//! encryption posture without being able to open it.

use cos::{Dict, Object};

use crate::crypt::{detect_cipher, Decryptor};
use crate::xref::{parse_indirect_at, ObjLoc, Xref};

/// The encryption posture of a document, derived without opening it.
#[derive(Debug, Clone)]
pub struct EncryptionProbe {
    /// Whether the document carries an `/Encrypt` dictionary at all.
    pub encrypted: bool,
    /// Short cipher label: `"None"`, `"RC4"`, `"AES-128"`, `"AES-256"` or
    /// `"Identity"`.
    pub cipher: &'static str,
    /// The standard security-handler revision (`/R`), or 0 when not encrypted.
    pub revision: i64,
    /// Whether opening the document requires a non-empty password (the empty
    /// user password does not authenticate).
    pub requires_password: bool,
}

impl Default for EncryptionProbe {
    fn default() -> Self {
        EncryptionProbe {
            encrypted: false,
            cipher: "None",
            revision: 0,
            requires_password: false,
        }
    }
}

/// Read the version declared in the `%PDF-x.y` header, e.g. `"1.7"` or `"2.0"`.
/// Returns `None` if the marker is missing or malformed.
pub fn header_version(data: impl AsRef<[u8]>) -> Option<String> {
    let data = data.as_ref();
    let pos = find(data, b"%PDF-")?;
    let start = pos + 5;
    let mut end = start;
    // A version is `<digit> "." <digit>` possibly with multi-digit parts.
    while end < data.len() && (data[end].is_ascii_digit() || data[end] == b'.') {
        end += 1;
    }
    let s = std::str::from_utf8(&data[start..end]).ok()?;
    if s.is_empty() || !s.contains('.') {
        return None;
    }
    Some(s.to_string())
}

/// Probe a document's encryption posture without decrypting its body.
pub fn probe_encryption(data: impl AsRef<[u8]>) -> EncryptionProbe {
    let data = data.as_ref();
    let Ok(xref) = Xref::read(data) else {
        return EncryptionProbe::default();
    };
    let Some(encrypt_obj) = xref.trailer.get("Encrypt") else {
        return EncryptionProbe::default();
    };
    // Resolve the /Encrypt dict (it may be inline or an indirect reference).
    let enc = match encrypt_obj {
        Object::Dict(d) => Some(d.clone()),
        Object::Reference(r) => match xref.entries.get(&r.number) {
            Some(ObjLoc::Offset(off)) => match parse_indirect_at(data, *off) {
                Ok((_, _, Object::Dict(d))) => Some(d),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    };
    let Some(enc) = enc else {
        // Encrypt entry present but unreadable: report encrypted, unknown cipher.
        return EncryptionProbe {
            encrypted: true,
            cipher: "Unknown",
            revision: 0,
            requires_password: true,
        };
    };

    let v = int(enc.get("V")).unwrap_or(0);
    let r = int(enc.get("R")).unwrap_or(0);
    let cipher = if r >= 5 || v >= 5 {
        "AES-256"
    } else {
        detect_cipher(&enc, v).label()
    };

    let id0 = trailer_id0(&xref.trailer);
    let requires_password = Decryptor::new(&enc, &id0, b"").is_err();

    EncryptionProbe {
        encrypted: true,
        cipher,
        revision: r,
        requires_password,
    }
}

/// The first element of the trailer `/ID` array, as bytes (empty if absent).
fn trailer_id0(trailer: &Dict) -> Vec<u8> {
    match trailer.get("ID") {
        Some(Object::Array(a)) => match a.first() {
            Some(Object::String(s)) => s.as_bytes().to_vec(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn int(o: Option<&Object>) -> Option<i64> {
    match o {
        Some(Object::Integer(n)) => Some(*n),
        Some(Object::Real(r)) => Some(*r as i64),
        _ => None,
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

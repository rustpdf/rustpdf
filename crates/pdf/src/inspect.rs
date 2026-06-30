//! Non-mutating document inspection (issue #45 P1 #3): read a PDF's version,
//! PDF/A level and encryption posture without loading or modifying it.
//!
//! This is a lightweight alternative to [`crate::EditableDoc::load`] for the
//! "what am I about to open?" question. It works even on password-protected
//! files: the encryption fields are filled in from the `/Encrypt` dictionary
//! while the version and PDF/A level are read only when the document opens with
//! the empty password.

use cos::Object;
use parser::{probe_encryption, PdfReader};

/// A read-only summary of a PDF document.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfOverview {
    /// The PDF version, e.g. `"1.7"` or `"2.0"` (catalog `/Version` overrides the
    /// header when present).
    pub version: String,
    /// The PDF/A conformance level if the document declares one in its XMP
    /// metadata (e.g. `"2b"`, `"3a"`, `"4f"`); `None` otherwise or when the
    /// document is password-locked.
    pub pdfa_level: Option<String>,
    /// Whether the document is encrypted.
    pub encrypted: bool,
    /// Cipher label: `"None"`, `"RC4"`, `"AES-128"`, `"AES-256"`, `"Identity"`
    /// or `"Unknown"`.
    pub encryption: String,
    /// Whether opening the document requires a non-empty password.
    pub requires_password: bool,
    /// Number of pages (`0` when the document is password-locked and cannot be
    /// walked).
    pub page_count: usize,
}

/// Inspect `bytes` without mutating it. Never fails on a password-protected
/// file — it reports the encryption posture and as much else as it can read.
pub fn inspect(bytes: impl AsRef<[u8]>) -> PdfOverview {
    let bytes = bytes.as_ref();
    let enc = probe_encryption(bytes);
    let mut version = parser::header_version(bytes).unwrap_or_else(|| "1.7".to_string());
    let mut pdfa_level = None;
    let mut page_count = 0;

    // The catalog /Version override, page count and PDF/A level need the body,
    // which is only reachable when the document opens with the empty password.
    if let Ok(reader) = PdfReader::parse(bytes) {
        if let Ok(root) = reader.root() {
            if let Some(Object::Name(n)) = root.get("Version").map(|o| reader.resolve(o)) {
                let s = n.as_str();
                if s.contains('.') {
                    version = s.to_string();
                }
            }
        }
        page_count = reader.pages().len();
        pdfa_level = detect_pdfa(&reader);
    }

    PdfOverview {
        version,
        pdfa_level,
        encrypted: enc.encrypted,
        encryption: enc.cipher.to_string(),
        requires_password: enc.requires_password,
        page_count,
    }
}

/// Read the XMP `/Metadata` stream and extract the PDF/A conformance level from
/// the `pdfaid` identifier (part + conformance/rev), handling both the
/// element form (`<pdfaid:part>2</pdfaid:part>`) and the attribute form
/// (`pdfaid:part="2"`).
fn detect_pdfa(reader: &PdfReader) -> Option<String> {
    let root = reader.root().ok()?;
    let meta = root.get("Metadata")?;
    let xmp = match reader.resolve(meta) {
        Object::Stream(s) => reader.stream_data(s).ok()?,
        _ => return None,
    };
    let xmp = String::from_utf8_lossy(&xmp);
    let part = scan_value(&xmp, "pdfaid:part")?;
    let part_digit = part.chars().find(|c| c.is_ascii_digit())?;
    let conf = scan_value(&xmp, "pdfaid:conformance")
        .and_then(|s| s.chars().find(|c| c.is_ascii_alphabetic()))
        .map(|c| c.to_ascii_lowercase());

    Some(match (part_digit, conf) {
        // PDF/A-4 has no base conformance letter; e/f are amendments.
        ('4', Some('e')) => "4e".to_string(),
        ('4', Some('f')) => "4f".to_string(),
        ('4', _) => "4".to_string(),
        (p, Some(c)) => format!("{p}{c}"),
        (p, None) => format!("{p}b"),
    })
}

/// Find `key` in the XMP and return the value text that follows it, whether the
/// value is an element body or an `="..."` attribute. Returns the run of
/// characters up to the next `<` or `"`.
fn scan_value(xmp: &str, key: &str) -> Option<String> {
    let pos = xmp.find(key)?;
    let rest = &xmp[pos + key.len()..];
    // Skip past the separator: either `>` (element) or `="`/`=` (attribute).
    let value_start = rest.find(['>', '='])?;
    let after = &rest[value_start + 1..];
    let after = after.trim_start_matches(['"', '\'', ' ', '\t', '\r', '\n']);
    let end = after.find(['<', '"', '\'']).unwrap_or(after.len());
    let value = after[..end].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

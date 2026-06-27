//! Error type for the parser.

/// Errors produced while parsing or decoding an existing PDF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfError {
    /// The file is not a PDF (missing `%PDF-` header).
    NotAPdf,
    /// A structural element was malformed.
    Syntax(String),
    /// A cross-reference table/stream could not be parsed.
    Xref(String),
    /// A referenced object was not found.
    MissingObject(u32, u16),
    /// A stream filter failed to decode.
    Filter(String),
    /// Encryption is present but unsupported, or the password is wrong.
    Encryption(String),
}

impl std::fmt::Display for PdfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PdfError::NotAPdf => write!(f, "not a PDF (missing %PDF- header)"),
            PdfError::Syntax(s) => write!(f, "syntax error: {s}"),
            PdfError::Xref(s) => write!(f, "xref error: {s}"),
            PdfError::MissingObject(n, g) => write!(f, "missing object {n} {g}"),
            PdfError::Filter(s) => write!(f, "filter error: {s}"),
            PdfError::Encryption(s) => write!(f, "encryption error: {s}"),
        }
    }
}

impl std::error::Error for PdfError {}

/// Convenience result alias.
pub type Result<T> = std::result::Result<T, PdfError>;

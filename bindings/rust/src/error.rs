//! Error type bridging the C ABI's `PdfStatus` + thread-local last-error.

use std::fmt;

/// Status codes returned by every fallible export (mirrors `include/pdf.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PdfStatus {
    Ok,
    NullPointer,
    InvalidUtf8,
    Io,
    Serialize,
    Panic,
    Parse,
    Font,
    Image,
    Encrypt,
    Sign,
    InvalidArgument,
    License,
    /// The cdylib could not be located/loaded (binding-side, no C equivalent).
    LibraryNotLoaded,
    /// An unknown status code came back from the library.
    Unknown(i32),
}

impl PdfStatus {
    pub(crate) fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Ok,
            1 => Self::NullPointer,
            2 => Self::InvalidUtf8,
            3 => Self::Io,
            4 => Self::Serialize,
            5 => Self::Panic,
            6 => Self::Parse,
            7 => Self::Font,
            8 => Self::Image,
            9 => Self::Encrypt,
            10 => Self::Sign,
            11 => Self::InvalidArgument,
            12 => Self::License,
            other => Self::Unknown(other),
        }
    }
}

/// An error returned by the binding: a status code plus the engine's
/// thread-local last-error message (when available).
#[derive(Debug, Clone)]
pub struct PdfError {
    pub status: PdfStatus,
    pub message: String,
}

impl PdfError {
    pub(crate) fn new(status: PdfStatus, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    /// The cdylib could not be found or loaded.
    pub(crate) fn loader(message: impl Into<String>) -> Self {
        Self::new(PdfStatus::LibraryNotLoaded, message)
    }
}

impl fmt::Display for PdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.message.is_empty() {
            write!(f, "rustpdf error: {:?}", self.status)
        } else {
            write!(f, "rustpdf error ({:?}): {}", self.status, self.message)
        }
    }
}

impl std::error::Error for PdfError {}

/// Convenience alias used throughout the public API.
pub type Result<T> = std::result::Result<T, PdfError>;

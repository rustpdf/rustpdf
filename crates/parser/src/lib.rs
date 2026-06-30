//! PDF parser — reads existing PDF files (Fase 5 of `project.md`).
//!
//! Layers: a non-panicking [`Lexer`] tokenizes bytes; `object` builds
//! [`cos::Object`] values; `filters` decodes stream data; `xref` locates
//! objects (classic tables, cross-reference streams, object streams, and a
//! brute-force recovery scan); `crypt` handles the standard security handler;
//! and [`PdfReader`] ties it together, resolving references, decrypting, and
//! walking the page tree.

mod crypt;
mod error;
mod filters;
mod inspect;
mod lexer;
mod object;
mod reader;
mod xref;

pub use error::{PdfError, Result};
pub use inspect::{header_version, probe_encryption, EncryptionProbe};
pub use lexer::{Lexer, Token};
pub use reader::PdfReader;

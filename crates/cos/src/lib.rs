//! COS (Carousel Object System) — the PDF object model and its byte
//! serialization. This is the "core of the core" (Fase 1 of `project.md`).
//!
//! The model is deliberately allocation-friendly and `Send`: there is no
//! `Rc`/`RefCell` anywhere. Indirect references are plain numbers
//! ([`Reference`]); the object graph itself lives in an arena owned by the
//! `writer` crate, matching the PDF model where a reference is just
//! "object N generation G".

mod name;
mod object;
mod serialize;
mod string;

pub use name::Name;
pub use object::{Dict, Object, Reference, Stream};
pub use string::{PdfString, StringSyntax};

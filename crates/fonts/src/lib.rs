//! Font parsing, embedding, subsetting and shaping (Fase 3 of `project.md`).
//!
//! This crate owns all font logic but knows nothing about PDF structure: it
//! parses metrics (via `ttf-parser`), shapes text (via `rustybuzz`), subsets
//! glyph programs (via `subsetter`) and reorders bidirectional text (via
//! `unicode-bidi`). The `pdf` crate turns the data exposed here into the
//! Type0/CIDFontType2 dictionaries, `ToUnicode` CMaps and content-stream bytes.

mod bidi;
mod font;
mod shape;

pub use bidi::{reorder_runs, BidiRun};
pub use font::{Font, FontError, Subset};
pub use shape::{shape, Direction, ShapedGlyph};

/// Re-export so downstream crates use the exact same `ttf-parser` version.
pub use ttf_parser;

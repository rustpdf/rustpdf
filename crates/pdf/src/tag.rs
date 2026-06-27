//! Logical structure types for Tagged PDF / accessibility (Fase 7.5).
//!
//! A `StructTag` is the role a piece of content plays in the document's logical
//! tree: a heading level, a paragraph, a figure, a table and its cells, a list.
//! Leaves (text/figures) carry both their own tag and an *ancestor path* of
//! grouping tags (e.g. a cell is `[Table, TR]` → `TD`), from which
//! [`Document::to_bytes`](crate::Document::to_bytes) materializes the nested
//! `StructTreeRoot`.

/// A PDF logical-structure type (ISO 32000-1 §14.8.4 standard roles).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructTag {
    /// The document root grouping element.
    Document,
    /// A paragraph (the default for body text).
    P,
    /// Headings, levels 1–6.
    H1,
    H2,
    H3,
    H4,
    H5,
    H6,
    /// An illustration; pair with alternate text (`/Alt`).
    Figure,
    /// A caption for a figure or table.
    Caption,
    /// A table and its parts.
    Table,
    TR,
    TH,
    TD,
    /// A list, list item and its body.
    L,
    LI,
    LBody,
    /// A generic inline span.
    Span,
}

impl StructTag {
    /// The PDF name written as the structure element's `/S`.
    pub(crate) fn name(self) -> &'static str {
        match self {
            StructTag::Document => "Document",
            StructTag::P => "P",
            StructTag::H1 => "H1",
            StructTag::H2 => "H2",
            StructTag::H3 => "H3",
            StructTag::H4 => "H4",
            StructTag::H5 => "H5",
            StructTag::H6 => "H6",
            StructTag::Figure => "Figure",
            StructTag::Caption => "Caption",
            StructTag::Table => "Table",
            StructTag::TR => "TR",
            StructTag::TH => "TH",
            StructTag::TD => "TD",
            StructTag::L => "L",
            StructTag::LI => "LI",
            StructTag::LBody => "LBody",
            StructTag::Span => "Span",
        }
    }

    /// The heading tag for `level` (1–6), clamped into range.
    pub fn heading(level: u8) -> StructTag {
        match level {
            0 | 1 => StructTag::H1,
            2 => StructTag::H2,
            3 => StructTag::H3,
            4 => StructTag::H4,
            5 => StructTag::H5,
            _ => StructTag::H6,
        }
    }
}

/// One grouping ancestor in a leaf's structure path: a tag plus a key that
/// uniquely identifies that group instance among its siblings (e.g. table #2,
/// row #3). The full key prefix identifies a node, so the same `(tag, key)`
/// reached twice refers to the same group.
pub(crate) type StructNode = (StructTag, u64);

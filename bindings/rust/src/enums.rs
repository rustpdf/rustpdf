//! Strongly-typed enums mirroring the small-int arguments the C ABI takes.

/// PDF/A conformance level. Passed to [`crate::Document::pdfa_level`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfaLevel {
    /// PDF/A-1b.
    A1b,
    /// PDF/A-2b.
    A2b,
    /// PDF/A-2a (also enables tagging).
    A2a,
    /// PDF/A-3b.
    A3b,
    /// PDF/A-3a (also enables tagging).
    A3a,
    /// PDF/A-4 (ISO 19005-4), based on PDF 2.0.
    A4,
    /// PDF/A-4e (engineering).
    A4e,
    /// PDF/A-4f (embedded files).
    A4f,
}

impl PdfaLevel {
    pub(crate) fn code(self) -> i32 {
        match self {
            Self::A1b => 0,
            Self::A2b => 1,
            Self::A2a => 2,
            Self::A3b => 3,
            Self::A3a => 4,
            Self::A4 => 5,
            Self::A4e => 6,
            Self::A4f => 7,
        }
    }
}

/// Paragraph alignment. Passed to [`crate::Document::paragraph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
    Center,
    Justify,
}

impl Align {
    pub(crate) fn code(self) -> i32 {
        match self {
            Self::Left => 0,
            Self::Right => 1,
            Self::Center => 2,
            Self::Justify => 3,
        }
    }
}

/// Embedded-file relationship (PDF/A-3). Passed to [`crate::Document::attach_file`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AFRelationship {
    Source,
    Data,
    Alternative,
    Supplement,
    Unspecified,
}

impl AFRelationship {
    pub(crate) fn code(self) -> i32 {
        match self {
            Self::Source => 0,
            Self::Data => 1,
            Self::Alternative => 2,
            Self::Supplement => 3,
            Self::Unspecified => 4,
        }
    }
}

/// Encryption method. Passed to [`crate::EditableDoc::encrypt`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encryption {
    /// RC4 128-bit.
    Rc4_128,
    /// AES-128.
    Aes128,
    /// AES-256 (R6).
    Aes256,
}

impl Encryption {
    pub(crate) fn code(self) -> i32 {
        match self {
            Self::Rc4_128 => 0,
            Self::Aes128 => 1,
            Self::Aes256 => 2,
        }
    }
}

/// Factur-X / ZUGFeRD profile. Passed to [`crate::Document::facturx`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacturxProfile {
    /// MINIMUM profile.
    Minimum,
    /// BASIC WL (without lines) profile.
    BasicWl,
    /// BASIC profile.
    Basic,
    /// EN 16931 (COMFORT) profile.
    En16931,
    /// EXTENDED profile.
    Extended,
}

impl FacturxProfile {
    pub(crate) fn code(self) -> i32 {
        match self {
            Self::Minimum => 0,
            Self::BasicWl => 1,
            Self::Basic => 2,
            Self::En16931 => 3,
            Self::Extended => 4,
        }
    }
}

/// DocMDP certification level applied by a certifying signature (the first
/// signature only). Passed via [`crate::SigningOptions::certify`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Certify {
    /// Not a certifying signature (an ordinary approval signature).
    #[default]
    None,
    /// `/P 1` — no changes permitted after signing.
    Locked,
    /// `/P 2` — form-filling and signing permitted.
    Forms,
    /// `/P 3` — form-filling, signing and annotations permitted.
    FormsAndAnnotations,
}

impl Certify {
    pub(crate) fn code(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Locked => 1,
            Self::Forms => 2,
            Self::FormsAndAnnotations => 3,
        }
    }
}

/// Vertical anchor of positioned stamping text. Passed to
/// [`crate::EditableDoc::place_text_anchored`] and
/// [`crate::EditableDoc::place_paragraph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VerticalAnchor {
    /// `y` is the text baseline (historical [`crate::EditableDoc::place_text`]
    /// behavior). For paragraphs: the first line's baseline.
    #[default]
    Baseline,
    /// The text hangs from `y` (baseline at `y − ascent × size`, legacy layout engines
    /// `fixed-position layout` semantics). For paragraphs: top of the block.
    Top,
    /// The descender line rests on `y`. For paragraphs: bottom-pinned — the
    /// block grows upward from `y` by its real content height.
    Bottom,
    /// Top-anchored via the layout line box.
    LineTop,
    /// Bottom-anchored via the layout line box.
    LineBottom,
}

impl VerticalAnchor {
    pub(crate) fn code(self) -> i32 {
        match self {
            Self::Baseline => 0,
            Self::Top => 1,
            Self::Bottom => 2,
            Self::LineTop => 3,
            Self::LineBottom => 4,
        }
    }
}

/// Vertical alignment of the text line inside a masked-text box. Passed to
/// [`crate::EditableDoc::masked_text_padded`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VerticalAlign {
    /// The line hangs from the top edge (baseline at
    /// `y + height − ascent × size`, top line-alignment in rectangle-based text APIs).
    Top,
    /// Cap-height centering inside the box (historical
    /// [`crate::EditableDoc::masked_text`] behavior).
    #[default]
    Middle,
    /// The descender line rests on the bottom edge.
    Bottom,
}

impl VerticalAlign {
    pub(crate) fn code(self) -> i32 {
        match self {
            Self::Top => 0,
            Self::Middle => 1,
            Self::Bottom => 2,
        }
    }
}

/// Coordinate space of the positioned stamping primitives. Passed to
/// [`crate::EditableDoc::set_stamp_space`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StampSpace {
    /// The page's **displayed** space (historical default): coordinates
    /// compensate `/Rotate` so a `rotation_deg = 0` stamp reads upright.
    #[default]
    Visible,
    /// Raw PDF user space (legacy fixed-position layout/rotation
    /// semantics): no composition with the page's `/Rotate` or crop offset.
    Media,
}

impl StampSpace {
    pub(crate) fn code(self) -> i32 {
        match self {
            Self::Visible => 0,
            Self::Media => 1,
        }
    }
}

/// How a rotated stamped image is anchored. Passed to
/// [`crate::EditableDoc::draw_image_anchored`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImageAnchor {
    /// Rotate the image about its lower-left corner at `(x, y)` (historical
    /// [`crate::EditableDoc::draw_image`] behavior).
    #[default]
    Corner,
    /// Land the rotated image's axis-aligned bounding box's lower-left corner
    /// at `(x, y)`.
    BoundingBox,
}

impl ImageAnchor {
    pub(crate) fn code(self) -> i32 {
        match self {
            Self::Corner => 0,
            Self::BoundingBox => 1,
        }
    }
}

/// PDF header version. Passed to [`crate::Document::set_version`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfVersion {
    V1_4,
    V1_5,
    V1_7,
    /// PDF 2.0 (ISO 32000-2).
    V2_0,
}

impl PdfVersion {
    pub(crate) fn code(self) -> i32 {
        match self {
            Self::V1_4 => 0,
            Self::V1_5 => 1,
            Self::V1_7 => 2,
            Self::V2_0 => 3,
        }
    }
}

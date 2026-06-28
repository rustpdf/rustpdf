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
}

impl PdfaLevel {
    pub(crate) fn code(self) -> i32 {
        match self {
            Self::A1b => 0,
            Self::A2b => 1,
            Self::A2a => 2,
            Self::A3b => 3,
            Self::A3a => 4,
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

/// PDF header version. Passed to [`crate::Document::set_version`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfVersion {
    V1_4,
    V1_5,
    V1_7,
}

impl PdfVersion {
    pub(crate) fn code(self) -> i32 {
        match self {
            Self::V1_4 => 0,
            Self::V1_5 => 1,
            Self::V1_7 => 2,
        }
    }
}

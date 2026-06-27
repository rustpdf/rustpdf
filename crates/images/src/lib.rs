//! Image embedding for PDF (Fase 4 of `project.md`).
//!
//! Two ingestion paths:
//!
//! * **JPEG** ([`Image::from_jpeg`]) — embedded *verbatim* via `DCTDecode`. We
//!   only parse the SOF marker for dimensions/components and never re-encode, so
//!   no generation loss and minimal work (Fase 4.1).
//! * **PNG** ([`Image::from_png`]) — decoded with the `png` crate and re-encoded
//!   with `FlateDecode`. Palette images become an `Indexed` color space; alpha
//!   (RGBA / grayscale-alpha / palette `tRNS`) becomes a separate soft mask
//!   ([`SoftMask`]); 8- and 16-bit depths are supported (Fase 4.3/4.4).
//!
//! This crate produces a neutral description; the `pdf` crate turns it into the
//! Image XObject dictionary and content-stream `Do` invocation.

mod jpeg;
mod png_decode;

/// A PDF color space for image samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColorSpace {
    DeviceGray,
    DeviceRgb,
    DeviceCmyk,
    /// `[/Indexed base hival lookup]` — palette images.
    Indexed {
        /// Base space of the palette entries (always RGB here).
        base: Box<ColorSpace>,
        /// Highest valid index (`palette_len - 1`).
        hival: u8,
        /// Flat palette lookup table in the base space.
        lookup: Vec<u8>,
    },
}

/// The stream filter the image data is already encoded with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    /// JPEG passed through untouched.
    DctDecode,
    /// zlib/deflate (PDF `FlateDecode`).
    FlateDecode,
}

/// A grayscale soft mask carrying per-pixel alpha (`/SMask`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoftMask {
    pub width: u32,
    pub height: u32,
    pub bits_per_component: u8,
    /// Alpha plane, `FlateDecode`-encoded.
    pub data: Vec<u8>,
}

/// A decoded/ingested image ready to embed as an Image XObject.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub bits_per_component: u8,
    pub color_space: ColorSpace,
    pub filter: Filter,
    /// The (already filtered) image sample data.
    pub data: Vec<u8>,
    /// Optional soft mask (alpha channel).
    pub soft_mask: Option<SoftMask>,
    /// Optional `/Decode` array (e.g. CMYK-JPEG inversion `[1 0 1 0 1 0 1 0]`).
    pub decode: Option<Vec<f32>>,
}

/// Errors from image ingestion.
#[derive(Debug)]
pub enum ImageError {
    /// The JPEG/PNG byte stream is malformed or unsupported.
    Format(String),
    /// I/O error reading an image file.
    Io(String),
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageError::Format(s) => write!(f, "image format error: {s}"),
            ImageError::Io(s) => write!(f, "image io error: {s}"),
        }
    }
}

impl std::error::Error for ImageError {}

impl Image {
    /// Ingest a JPEG, embedding its bytes verbatim (`DCTDecode`).
    pub fn from_jpeg(data: impl Into<Vec<u8>>) -> Result<Image, ImageError> {
        jpeg::from_jpeg(data.into())
    }

    /// Decode a PNG and re-encode it as `FlateDecode`.
    pub fn from_png(data: impl AsRef<[u8]>) -> Result<Image, ImageError> {
        png_decode::from_png(data.as_ref())
    }

    /// Load an image from a file, dispatching on a JPEG/PNG signature.
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Image, ImageError> {
        let data = std::fs::read(path).map_err(|e| ImageError::Io(e.to_string()))?;
        if data.starts_with(&[0xFF, 0xD8]) {
            Image::from_jpeg(data)
        } else if data.starts_with(&[0x89, b'P', b'N', b'G']) {
            Image::from_png(&data)
        } else {
            Err(ImageError::Format("unknown image signature".into()))
        }
    }

    /// Number of color components implied by the color space.
    pub fn components(&self) -> u8 {
        match &self.color_space {
            ColorSpace::DeviceGray | ColorSpace::Indexed { .. } => 1,
            ColorSpace::DeviceRgb => 3,
            ColorSpace::DeviceCmyk => 4,
        }
    }
}

/// zlib-compress a byte slice (PDF `FlateDecode`). Also used by the `pdf`
/// crate's optimizer to recompress uncompressed streams.
pub fn flate_encode(data: &[u8]) -> Vec<u8> {
    use flate2::{write::ZlibEncoder, Compression};
    use std::io::Write;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .expect("zlib write to Vec is infallible");
    encoder.finish().expect("zlib finish")
}

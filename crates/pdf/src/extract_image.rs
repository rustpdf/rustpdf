//! Image extraction (the reverse of the Fase 4 embedding path): pull the raster
//! images out of an existing PDF's pages and hand them back as standalone PNG or
//! JPEG files, ready to write to disk.
//!
//! * **JPEG** XObjects (`/Filter /DCTDecode`) are returned *verbatim* — the
//!   stream body already is a complete JPEG file, so there is no re-encode and no
//!   generation loss.
//! * **Everything else** is decoded to its raw sample plane (Flate, predictors,
//!   etc. applied by the parser) and re-encoded as PNG via [`images::export`],
//!   honouring the color space (`DeviceGray`/`RGB`/`CMYK`/`Indexed`/`ICCBased`)
//!   and merging an `/SMask` alpha channel when it lines up.
//! * Images nested inside **Form XObjects** are found recursively.
//!
//! Opaque codecs we cannot turn into a clean PNG (`JPXDecode`, `CCITTFaxDecode`,
//! `JBIG2Decode`) are skipped rather than emitted corrupt.

use std::path::{Path, PathBuf};

use cos::{Dict, Object, Stream};
use images::export::{cmyk_pixel, encode_png, AlphaPlane, PngColor};
use parser::PdfReader;

/// The on-disk format an [`ExtractedImage`] is encoded in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
}

impl ImageFormat {
    /// The conventional file extension, without the leading dot.
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
        }
    }
}

/// One image pulled from a PDF page, ready to write to disk.
#[derive(Debug, Clone)]
pub struct ExtractedImage {
    /// Zero-based index of the page the image appears on.
    pub page: usize,
    /// The XObject resource name it was referenced by (e.g. `Im0`).
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
    /// The encoded bytes of a complete PNG or JPEG file.
    pub data: Vec<u8>,
}

impl ExtractedImage {
    /// A safe file name of the form `page{N}_{name}.{ext}` (1-based page number).
    pub fn file_name(&self) -> String {
        let safe: String = self
            .name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        format!("page{}_{}.{}", self.page + 1, safe, self.format.extension())
    }

    /// Write the image into `dir` using [`ExtractedImage::file_name`], returning
    /// the path written.
    pub fn save_in(&self, dir: impl AsRef<Path>) -> std::io::Result<PathBuf> {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir)?;
        let path = dir.join(self.file_name());
        std::fs::write(&path, &self.data)?;
        Ok(path)
    }
}

/// Extract every raster image from every page of a PDF, in page then
/// resource-name order.
pub fn extract_images(bytes: impl AsRef<[u8]>) -> Result<Vec<ExtractedImage>, parser::PdfError> {
    let reader = PdfReader::parse(bytes)?;
    let mut out = Vec::new();
    for (idx, page) in reader.pages().iter().enumerate() {
        if let Some(resources) = page.get("Resources").and_then(|o| reader.resolve_dict(o)) {
            let mut seen = Vec::new();
            collect(&reader, resources, idx, &mut seen, &mut out);
        }
    }
    Ok(out)
}

/// Walk a `/Resources` dict's `/XObject`, extracting images and recursing into
/// Form XObjects. `seen` guards against reference cycles within a page.
fn collect(
    reader: &PdfReader,
    resources: &Dict,
    page: usize,
    seen: &mut Vec<u32>,
    out: &mut Vec<ExtractedImage>,
) {
    let Some(xobjects) = resources
        .get("XObject")
        .and_then(|o| reader.resolve_dict(o))
    else {
        return;
    };

    for (name, obj) in xobjects.iter() {
        if let Object::Reference(r) = obj {
            if seen.contains(&r.number) {
                continue;
            }
            seen.push(r.number);
        }
        let Object::Stream(stream) = reader.resolve(obj) else {
            continue;
        };
        match subtype(&stream.dict).as_deref() {
            Some("Image") => {
                if let Some(img) = decode_image(reader, stream, page, name.as_str()) {
                    out.push(img);
                }
            }
            Some("Form") => {
                if let Some(res) = stream
                    .dict
                    .get("Resources")
                    .and_then(|o| reader.resolve_dict(o))
                {
                    collect(reader, res, page, seen, out);
                }
            }
            _ => {}
        }
    }
}

/// Turn a single Image XObject into an [`ExtractedImage`], or `None` if its
/// codec/color space cannot be exported.
fn decode_image(
    reader: &PdfReader,
    stream: &Stream,
    page: usize,
    name: &str,
) -> Option<ExtractedImage> {
    let dict = &stream.dict;
    let width = dict.get("Width").and_then(int)? as u32;
    let height = dict.get("Height").and_then(int)? as u32;
    let terminal = terminal_filter(reader, dict);

    // JPEG: the (post pre-filter) stream body already is a JPEG file.
    if matches!(terminal.as_deref(), Some("DCTDecode" | "DCT")) {
        let data = reader.stream_data(stream).ok()?;
        return Some(ExtractedImage {
            page,
            name: name.to_owned(),
            width,
            height,
            format: ImageFormat::Jpeg,
            data,
        });
    }
    // Codecs we cannot cleanly re-encode as PNG: skip.
    if matches!(
        terminal.as_deref(),
        Some("JPXDecode" | "CCITTFaxDecode" | "JBIG2Decode")
    ) {
        return None;
    }

    let samples = reader.stream_data(stream).ok()?;
    let image_mask = matches!(
        dict.get("ImageMask").map(|o| reader.resolve(o)),
        Some(Object::Bool(true))
    );
    let bits = if image_mask {
        1
    } else {
        dict.get("BitsPerComponent").and_then(int).unwrap_or(8) as u8
    };
    let color = if image_mask {
        PngColor::Gray
    } else {
        resolve_color(reader, dict.get("ColorSpace")?)?
    };
    let alpha = soft_mask(reader, dict);

    let data = encode_png(width, height, bits, color, &samples, alpha).ok()?;
    Some(ExtractedImage {
        page,
        name: name.to_owned(),
        width,
        height,
        format: ImageFormat::Png,
        data,
    })
}

/// The last (terminal) filter name in `/Filter`, if any.
fn terminal_filter(reader: &PdfReader, dict: &Dict) -> Option<String> {
    match reader.resolve(dict.get("Filter")?) {
        Object::Name(n) => Some(n.as_str().to_owned()),
        Object::Array(a) => match a.last().map(|o| reader.resolve(o)) {
            Some(Object::Name(n)) => Some(n.as_str().to_owned()),
            _ => None,
        },
        _ => None,
    }
}

/// Map a PDF `/ColorSpace` object to a PNG color interpretation.
fn resolve_color(reader: &PdfReader, obj: &Object) -> Option<PngColor> {
    match reader.resolve(obj) {
        Object::Name(n) => match n.as_str() {
            "DeviceGray" | "G" | "CalGray" => Some(PngColor::Gray),
            "DeviceRGB" | "RGB" | "CalRGB" => Some(PngColor::Rgb),
            "DeviceCMYK" | "CMYK" => Some(PngColor::Cmyk),
            _ => None,
        },
        Object::Array(a) => match a.first().map(|o| reader.resolve(o)) {
            Some(Object::Name(n)) => match n.as_str() {
                "ICCBased" => {
                    let n = a
                        .get(1)
                        .map(|o| reader.resolve(o))
                        .and_then(|o| match o {
                            Object::Stream(s) => s.dict.get("N").and_then(int),
                            _ => None,
                        })
                        .unwrap_or(3);
                    Some(match n {
                        1 => PngColor::Gray,
                        4 => PngColor::Cmyk,
                        _ => PngColor::Rgb,
                    })
                }
                "CalRGB" => Some(PngColor::Rgb),
                "CalGray" => Some(PngColor::Gray),
                "Indexed" | "I" => resolve_indexed(reader, a),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// Build an RGB palette from an `[/Indexed base hival lookup]` color space.
fn resolve_indexed(reader: &PdfReader, a: &[Object]) -> Option<PngColor> {
    let base = resolve_color(reader, a.get(1)?)?;
    let hival = a.get(2).map(|o| reader.resolve(o)).and_then(int)? as usize;
    let lookup = match a.get(3).map(|o| reader.resolve(o))? {
        Object::String(s) => s.as_bytes().to_vec(),
        Object::Stream(s) => reader.stream_data(s).ok()?,
        _ => return None,
    };
    let comps = match base {
        PngColor::Gray => 1,
        PngColor::Rgb => 3,
        PngColor::Cmyk => 4,
        PngColor::Indexed { .. } => return None, // nested palettes unsupported
    };

    let mut palette = Vec::with_capacity((hival + 1) * 3);
    for i in 0..=hival {
        let off = i * comps;
        if off + comps > lookup.len() {
            break;
        }
        let px = &lookup[off..off + comps];
        let rgb = match base {
            PngColor::Gray => [px[0], px[0], px[0]],
            PngColor::Rgb => [px[0], px[1], px[2]],
            PngColor::Cmyk => cmyk_pixel(px),
            PngColor::Indexed { .. } => unreachable!(),
        };
        palette.extend_from_slice(&rgb);
    }
    Some(PngColor::Indexed { palette })
}

/// Decode an image's `/SMask` into a grayscale alpha plane, if present and
/// decodable.
fn soft_mask(reader: &PdfReader, dict: &Dict) -> Option<AlphaPlane> {
    let Object::Stream(s) = reader.resolve(dict.get("SMask")?) else {
        return None;
    };
    if matches!(
        terminal_filter(reader, &s.dict).as_deref(),
        Some("DCTDecode" | "JPXDecode" | "CCITTFaxDecode" | "JBIG2Decode")
    ) {
        return None;
    }
    Some(AlphaPlane {
        width: s.dict.get("Width").and_then(int)? as u32,
        height: s.dict.get("Height").and_then(int)? as u32,
        bits: s.dict.get("BitsPerComponent").and_then(int).unwrap_or(8) as u8,
        samples: reader.stream_data(s).ok()?,
    })
}

fn subtype(dict: &Dict) -> Option<String> {
    match dict.get("Subtype") {
        Some(Object::Name(n)) => Some(n.as_str().to_owned()),
        _ => None,
    }
}

fn int(o: &Object) -> Option<i64> {
    match o {
        Object::Integer(n) => Some(*n),
        Object::Real(r) => Some(*r as i64),
        _ => None,
    }
}

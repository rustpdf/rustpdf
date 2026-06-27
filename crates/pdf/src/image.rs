//! Image XObject construction (Fase 4.2). Turns an [`images::Image`] into a PDF
//! Image XObject (and an optional `/SMask` for alpha), ready to be referenced
//! from a page's `Resources /XObject` and painted with `Do`.

use cos::{Dict, Object, PdfString, Reference, Stream};
use images::{ColorSpace, Filter, Image};
use writer::Document as WriterDoc;

/// Identifier for an image registered on a [`Document`](crate::Document).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageId(pub(crate) usize);

impl ImageId {
    /// The content-stream resource name for this image (e.g. `Im0`).
    pub(crate) fn resource_name(self) -> String {
        format!("Im{}", self.0)
    }

    /// The zero-based registration index (stable handle for bindings/FFI).
    pub fn index(self) -> usize {
        self.0
    }

    /// Reconstruct an image id from its [`index`](ImageId::index).
    pub fn from_index(index: usize) -> Self {
        ImageId(index)
    }
}

/// Build the Image XObject (and any soft mask) and return its reference.
pub(crate) fn build_image(doc: &mut WriterDoc, img: &Image) -> Reference {
    // Soft mask first so the main image can reference it.
    let smask_ref = img.soft_mask.as_ref().map(|m| {
        let dict = Dict::new()
            .with("Type", Object::name("XObject"))
            .with("Subtype", Object::name("Image"))
            .with("Width", m.width as i64)
            .with("Height", m.height as i64)
            .with("ColorSpace", Object::name("DeviceGray"))
            .with("BitsPerComponent", m.bits_per_component as i64)
            .with("Filter", Object::name("FlateDecode"));
        doc.add(Stream::with_dict(dict, m.data.clone()))
    });

    let mut dict = Dict::new()
        .with("Type", Object::name("XObject"))
        .with("Subtype", Object::name("Image"))
        .with("Width", img.width as i64)
        .with("Height", img.height as i64)
        .with("BitsPerComponent", img.bits_per_component as i64)
        .with("ColorSpace", color_space_object(&img.color_space))
        .with("Filter", filter_name(img.filter));

    if let Some(decode) = &img.decode {
        dict.set(
            "Decode",
            Object::Array(decode.iter().map(|&v| Object::Real(v as f64)).collect()),
        );
    }
    if let Some(smask) = smask_ref {
        dict.set("SMask", smask);
    }

    doc.add(Stream::with_dict(dict, img.data.clone()))
}

fn filter_name(filter: Filter) -> Object {
    Object::name(match filter {
        Filter::DctDecode => "DCTDecode",
        Filter::FlateDecode => "FlateDecode",
    })
}

fn color_space_object(cs: &ColorSpace) -> Object {
    match cs {
        ColorSpace::DeviceGray => Object::name("DeviceGray"),
        ColorSpace::DeviceRgb => Object::name("DeviceRGB"),
        ColorSpace::DeviceCmyk => Object::name("DeviceCMYK"),
        ColorSpace::Indexed {
            base,
            hival,
            lookup,
        } => Object::Array(vec![
            Object::name("Indexed"),
            color_space_object(base),
            Object::Integer(*hival as i64),
            Object::String(PdfString::literal(lookup.clone())),
        ]),
    }
}

//! PDF/A-2b conformance (Fase 7.4): adds an sRGB `OutputIntent`, XMP metadata
//! carrying the PDF/A identifier, and a document `/ID`.
//!
//! Fonts are already embedded and subsetted by the font pipeline, so with a
//! valid `OutputIntent` (for device color spaces) and the PDF/A XMP, the output
//! validates as PDF/A-2b under veraPDF.

use cos::{Dict, Object, PdfString, Stream};
use md5::{Digest, Md5};
use writer::Document as WriterDoc;

/// A small, public-domain (CC0) sRGB v2 ICC profile, bundled as the
/// `DestOutputProfile`. See `assets/icc/LICENSE.txt`.
const SRGB_ICC: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/icc/sRGB.icc"
));

/// Add the PDF/A objects, mutate `catalog`, and set the document `/ID`.
/// `part` is the PDF/A part (1, 2 or 3); `conformance` is 'A'
/// (accessible/tagged) or 'B' (basic).
pub(crate) fn apply(
    doc: &mut WriterDoc,
    catalog: &mut Dict,
    info: &[(&str, String)],
    part: u8,
    conformance: char,
) {
    // 1. Embedded sRGB ICC profile (N = 3 components).
    let icc_dict = Dict::new().with("N", 3);
    let icc_ref = doc.add(Stream::with_dict(icc_dict, SRGB_ICC.to_vec()));

    // 2. OutputIntent referencing the ICC profile.
    let output_intent = Dict::new()
        .with("Type", Object::name("OutputIntent"))
        .with("S", Object::name("GTS_PDFA1"))
        .with(
            "OutputConditionIdentifier",
            PdfString::literal(b"sRGB IEC61966-2.1".to_vec()),
        )
        .with("Info", PdfString::literal(b"sRGB IEC61966-2.1".to_vec()))
        .with("DestOutputProfile", icc_ref);
    let oi_ref = doc.add(output_intent);
    catalog.set(
        "OutputIntents",
        Object::Array(vec![Object::Reference(oi_ref)]),
    );

    // 3. XMP metadata (uncompressed) with the PDF/A identifier, kept in sync
    //    with the Info dictionary.
    let xmp = build_xmp(info, part, conformance);
    let meta_dict = Dict::new()
        .with("Type", Object::name("Metadata"))
        .with("Subtype", Object::name("XML"));
    let meta_ref = doc.add(Stream::with_dict(meta_dict, xmp.into_bytes()));
    catalog.set("Metadata", meta_ref);

    // 4. Document /ID (required by PDF/A), deterministic from the metadata.
    let id = document_id(info);
    doc.set_id([id.clone(), id]);
}

/// Build the XMP packet, mirroring the Info entries (PDF/A requires equality
/// between Info and XMP where both are present).
fn build_xmp(info: &[(&str, String)], part: u8, conformance: char) -> String {
    let get = |k: &str| {
        info.iter()
            .find(|(key, _)| *key == k)
            .map(|(_, v)| v.as_str())
    };

    let mut dc = String::new();
    if let Some(t) = get("Title") {
        dc.push_str(&format!(
            "<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:title>",
            xml(t)
        ));
    }
    if let Some(a) = get("Author") {
        dc.push_str(&format!(
            "<dc:creator><rdf:Seq><rdf:li>{}</rdf:li></rdf:Seq></dc:creator>",
            xml(a)
        ));
    }
    if let Some(s) = get("Subject") {
        dc.push_str(&format!(
            "<dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:description>",
            xml(s)
        ));
    }

    let mut pdf_ns = String::new();
    if let Some(p) = get("Producer") {
        pdf_ns.push_str(&format!("<pdf:Producer>{}</pdf:Producer>", xml(p)));
    }
    if let Some(k) = get("Keywords") {
        pdf_ns.push_str(&format!("<pdf:Keywords>{}</pdf:Keywords>", xml(k)));
    }

    let mut xmp_ns = String::new();
    if let Some(c) = get("Creator") {
        xmp_ns.push_str(&format!("<xmp:CreatorTool>{}</xmp:CreatorTool>", xml(c)));
    }

    // PDF/UA-1 identifier (level A / tagged) + the required extension-schema
    // declaration for the non-predefined `pdfuaid` namespace.
    let ua = if conformance == 'A' {
        "<rdf:Description rdf:about=\"\" xmlns:pdfuaid=\"http://www.aiim.org/pdfua/ns/id/\">\
         <pdfuaid:part>1</pdfuaid:part></rdf:Description>\n\
         <rdf:Description rdf:about=\"\" \
         xmlns:pdfaExtension=\"http://www.aiim.org/pdfa/ns/extension/\" \
         xmlns:pdfaSchema=\"http://www.aiim.org/pdfa/ns/schema#\" \
         xmlns:pdfaProperty=\"http://www.aiim.org/pdfa/ns/property#\">\
         <pdfaExtension:schemas><rdf:Bag><rdf:li rdf:parseType=\"Resource\">\
         <pdfaSchema:schema>PDF/UA identification schema</pdfaSchema:schema>\
         <pdfaSchema:namespaceURI>http://www.aiim.org/pdfua/ns/id/</pdfaSchema:namespaceURI>\
         <pdfaSchema:prefix>pdfuaid</pdfaSchema:prefix>\
         <pdfaSchema:property><rdf:Seq><rdf:li rdf:parseType=\"Resource\">\
         <pdfaProperty:name>part</pdfaProperty:name>\
         <pdfaProperty:valueType>Integer</pdfaProperty:valueType>\
         <pdfaProperty:category>internal</pdfaProperty:category>\
         <pdfaProperty:description>PDF/UA version identifier</pdfaProperty:description>\
         </rdf:li></rdf:Seq></pdfaSchema:property></rdf:li></rdf:Bag></pdfaExtension:schemas>\
         </rdf:Description>\n"
    } else {
        ""
    };

    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
         <rdf:Description rdf:about=\"\" xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\">\
         <pdfaid:part>{part}</pdfaid:part><pdfaid:conformance>{conformance}</pdfaid:conformance></rdf:Description>\n\
         <rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">{dc}</rdf:Description>\n\
         <rdf:Description rdf:about=\"\" xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\">{pdf_ns}</rdf:Description>\n\
         <rdf:Description rdf:about=\"\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\">{xmp_ns}</rdf:Description>\n\
         {ua}\
         </rdf:RDF>\n</x:xmpmeta>\n<?xpacket end=\"w\"?>"
    )
}

fn document_id(info: &[(&str, String)]) -> Vec<u8> {
    let mut h = Md5::new();
    h.update(b"rust-pdf-pdfa");
    for (k, v) in info {
        h.update(k.as_bytes());
        h.update(v.as_bytes());
    }
    h.finalize().to_vec()
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icc_is_a_valid_sized_profile() {
        // ICC header declares its own size in bytes 0..4 (big-endian).
        assert!(SRGB_ICC.len() >= 128);
        let declared = u32::from_be_bytes([SRGB_ICC[0], SRGB_ICC[1], SRGB_ICC[2], SRGB_ICC[3]]);
        assert_eq!(declared as usize, SRGB_ICC.len());
        assert_eq!(&SRGB_ICC[36..40], b"acsp"); // ICC signature
    }

    #[test]
    fn xmp_contains_pdfa_identifier() {
        let xmp = build_xmp(&[("Producer", "rust-pdf 0.1.0".into())], 2, 'B');
        assert!(xmp.contains("<pdfaid:part>2</pdfaid:part>"));
        assert!(xmp.contains("<pdfaid:conformance>B</pdfaid:conformance>"));
        assert!(xmp.contains("<pdf:Producer>rust-pdf 0.1.0</pdf:Producer>"));
    }
}

//! Embedded file attachments (PDF/A-3 and general embedded files).
//!
//! Each attachment becomes an `/EmbeddedFile` stream wrapped in a `/Filespec`
//! with an `/AFRelationship` (PDF/A-3 §6.8). Filespecs are registered in the
//! catalog's `/Names /EmbeddedFiles` name tree **and** in the document-level
//! `/AF` array, which is what makes them conforming associated files.

use cos::{Dict, Object, PdfString, Reference, Stream};
use writer::Document as WriterDoc;

use crate::Attachment;

/// A fixed modification date (deterministic output) for `/Params /ModDate`.
const MOD_DATE: &[u8] = b"D:20260101000000Z";

/// Build the embedded-file objects and wire them into `catalog`
/// (`/Names /EmbeddedFiles` + `/AF`).
pub(crate) fn apply(doc: &mut WriterDoc, catalog: &mut Dict, attachments: &[Attachment]) {
    // Name-tree entries must be emitted with keys in ascending byte order
    // (ISO 32000 §7.9.6); readers binary-search the tree and strict validators
    // (veraPDF / Factur-X) reject an unsorted `/Names` array.
    let mut pairs: Vec<(Vec<u8>, Reference)> = Vec::new();
    let mut af: Vec<Object> = Vec::new();

    for a in attachments {
        // The embedded file stream: typed, MIME-tagged, with size + mod date.
        let params = Dict::new()
            .with("Size", a.data.len() as i64)
            .with("ModDate", PdfString::literal(MOD_DATE.to_vec()));
        let ef_dict = Dict::new()
            .with("Type", Object::name("EmbeddedFile"))
            .with("Subtype", Object::name(a.mime.clone()))
            .with("Params", Object::Dict(params));
        let ef_ref = doc.add(Stream::with_dict(ef_dict, a.data.clone()));

        // The file specification, carrying the PDF/A-3 relationship.
        let filespec = Dict::new()
            .with("Type", Object::name("Filespec"))
            .with("F", PdfString::literal(a.name.clone().into_bytes()))
            .with("UF", PdfString::text(&a.name))
            .with("Desc", PdfString::text(&a.desc))
            .with("AFRelationship", Object::name(a.relationship))
            .with(
                "EF",
                Object::Dict(Dict::new().with("F", ef_ref).with("UF", ef_ref)),
            );
        let fs_ref = doc.add(filespec);

        pairs.push((a.name.clone().into_bytes(), fs_ref));
        af.push(Object::Reference(fs_ref));
    }

    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let mut names: Vec<Object> = Vec::with_capacity(pairs.len() * 2);
    for (key, fs_ref) in pairs {
        names.push(Object::String(PdfString::literal(key)));
        names.push(Object::Reference(fs_ref));
    }

    let embedded = Dict::new().with("Names", Object::Array(names));
    let name_tree = Dict::new().with("EmbeddedFiles", Object::Dict(embedded));
    catalog.set("Names", Object::Dict(name_tree));
    catalog.set("AF", Object::Array(af));
}

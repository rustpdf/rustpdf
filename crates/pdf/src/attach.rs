//! Embedded file attachments (PDF/A-3 and general embedded files).
//!
//! Each attachment becomes an `/EmbeddedFile` stream wrapped in a `/Filespec`
//! with an `/AFRelationship` (PDF/A-3 §6.8). Filespecs are registered in the
//! catalog's `/Names /EmbeddedFiles` name tree **and** in the document-level
//! `/AF` array, which is what makes them conforming associated files.

use cos::{Dict, Object, PdfString, Stream};
use writer::Document as WriterDoc;

use crate::Attachment;

/// A fixed modification date (deterministic output) for `/Params /ModDate`.
const MOD_DATE: &[u8] = b"D:20260101000000Z";

/// Build the embedded-file objects and wire them into `catalog`
/// (`/Names /EmbeddedFiles` + `/AF`).
pub(crate) fn apply(doc: &mut WriterDoc, catalog: &mut Dict, attachments: &[Attachment]) {
    let mut names: Vec<Object> = Vec::new();
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

        names.push(Object::String(PdfString::literal(
            a.name.clone().into_bytes(),
        )));
        names.push(Object::Reference(fs_ref));
        af.push(Object::Reference(fs_ref));
    }

    let embedded = Dict::new().with("Names", Object::Array(names));
    let name_tree = Dict::new().with("EmbeddedFiles", Object::Dict(embedded));
    catalog.set("Names", Object::Dict(name_tree));
    catalog.set("AF", Object::Array(af));
}

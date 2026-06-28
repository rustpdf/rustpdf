//! PDF document writer: owns the indirect-object arena and emits a complete
//! file (header, body, classic cross-reference table, trailer, `%%EOF`).
//!
//! The arena is a plain `Vec` indexed by object number — no `Rc`/`RefCell`,
//! so a [`Document`] is naturally `Send` (per `project.md` §1.2.2). Indirect
//! references are allocated up front with [`Document::reserve`] and filled in
//! later with [`Document::assign`], which lets objects reference each other
//! freely (catalog ↔ pages ↔ page).

use cos::PdfString;
use cos::{Dict, Object, Reference};

/// PDF version written into the header and used for feature gating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfVersion {
    V1_4,
    V1_5,
    V1_7,
    /// PDF 2.0 (ISO 32000-2). Also the basis for PDF/A-4.
    V2_0,
}

impl PdfVersion {
    fn header_bytes(self) -> &'static [u8] {
        match self {
            PdfVersion::V1_4 => b"%PDF-1.4",
            PdfVersion::V1_5 => b"%PDF-1.5",
            PdfVersion::V1_7 => b"%PDF-1.7",
            PdfVersion::V2_0 => b"%PDF-2.0",
        }
    }
}

/// Error produced while serializing a [`Document`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteError {
    /// An object number was reserved but never assigned a value.
    UnassignedObject(u32),
    /// No document catalog (`/Root`) was set.
    MissingRoot,
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteError::UnassignedObject(n) => {
                write!(f, "object {n} was reserved but never assigned")
            }
            WriteError::MissingRoot => write!(f, "document has no /Root catalog"),
        }
    }
}

impl std::error::Error for WriteError {}

/// An in-memory PDF document under construction.
#[derive(Debug)]
pub struct Document {
    version: PdfVersion,
    /// Index 0 is the always-free head object; real objects start at 1.
    objects: Vec<Option<Object>>,
    root: Option<Reference>,
    info: Option<Reference>,
    encrypt: Option<Reference>,
    id: Option<[Vec<u8>; 2]>,
    /// Emit object streams + a cross-reference stream (Fase 6.8) instead of the
    /// classic table. Ignored when the document is encrypted.
    compress: bool,
}

impl Default for Document {
    fn default() -> Self {
        Document::new(PdfVersion::V1_7)
    }
}

impl Document {
    /// Create an empty document targeting `version`.
    pub fn new(version: PdfVersion) -> Self {
        Document {
            version,
            // Slot 0 is the free-list head; never a real object.
            objects: vec![None],
            root: None,
            info: None,
            encrypt: None,
            id: None,
            compress: false,
        }
    }

    /// Enable object streams + a cross-reference stream on the next [`write`]
    /// (smaller files; PDF 1.5+). No effect when the document is encrypted —
    /// the classic table is used so per-object encryption stays correct.
    ///
    /// [`write`]: Document::write
    pub fn set_object_streams(&mut self, on: bool) {
        self.compress = on;
    }

    /// Reserve a fresh object number without giving it a value yet.
    pub fn reserve(&mut self) -> Reference {
        let number = self.objects.len() as u32;
        self.objects.push(None);
        Reference::new(number)
    }

    /// Assign a value to a previously reserved object number.
    ///
    /// # Panics
    /// Panics if `reference` was not produced by this document.
    pub fn assign(&mut self, reference: Reference, object: impl Into<Object>) {
        let slot = self
            .objects
            .get_mut(reference.number as usize)
            .expect("reference does not belong to this document");
        *slot = Some(object.into());
    }

    /// Reserve and assign in one step, returning the new reference.
    pub fn add(&mut self, object: impl Into<Object>) -> Reference {
        let r = self.reserve();
        self.assign(r, object);
        r
    }

    /// Mutate an already-assigned dictionary object in place (no-op if the slot
    /// is empty or holds a non-dictionary). Useful for back-patching links such
    /// as a field's `/Parent` after the field was emitted.
    pub fn patch(&mut self, reference: Reference, f: impl FnOnce(&mut Dict)) {
        if let Some(Some(Object::Dict(d))) = self.objects.get_mut(reference.number as usize) {
            f(d);
        }
    }

    /// Set the document catalog reference (`/Root`).
    pub fn set_root(&mut self, root: Reference) {
        self.root = Some(root);
    }

    /// Set the document information dictionary reference (`/Info`).
    pub fn set_info(&mut self, info: Reference) {
        self.info = Some(info);
    }

    /// Set the encryption dictionary reference (`/Encrypt` in the trailer).
    pub fn set_encrypt(&mut self, encrypt: Reference) {
        self.encrypt = Some(encrypt);
    }

    /// Set the file identifier (`/ID`, two byte strings).
    pub fn set_id(&mut self, id: [Vec<u8>; 2]) {
        self.id = Some(id);
    }

    /// Number of object slots (including the reserved slot 0).
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// Serialize the whole document to bytes.
    pub fn write(&self) -> Result<Vec<u8>, WriteError> {
        let root = self.root.ok_or(WriteError::MissingRoot)?;
        if self.compress && self.encrypt.is_none() {
            return self.write_compressed(root);
        }

        let mut out = Vec::with_capacity(1024);

        // Header + binary marker so downstream tools treat the file as binary.
        out.extend_from_slice(self.version.header_bytes());
        out.push(b'\n');
        out.extend_from_slice(&[b'%', 0xE2, 0xE3, 0xCF, 0xD3]);
        out.push(b'\n');

        // Body: one indirect object per filled slot. Record byte offsets for
        // the cross-reference table.
        let mut offsets: Vec<u32> = vec![0; self.objects.len()];
        for (number, slot) in self.objects.iter().enumerate().skip(1) {
            let object = slot
                .as_ref()
                .ok_or(WriteError::UnassignedObject(number as u32))?;
            offsets[number] = out.len() as u32;
            write_decimal(number as u64, &mut out);
            out.extend_from_slice(b" 0 obj\n");
            object.write_to(&mut out);
            out.extend_from_slice(b"\nendobj\n");
        }

        // Classic cross-reference table.
        let xref_offset = out.len();
        let size = self.objects.len();
        out.extend_from_slice(b"xref\n0 ");
        write_decimal(size as u64, &mut out);
        out.push(b'\n');
        // Object 0: the head of the free list, generation 65535.
        out.extend_from_slice(b"0000000000 65535 f\r\n");
        for offset in offsets.iter().skip(1) {
            write_xref_entry(*offset, 0, b'n', &mut out);
        }

        // Trailer.
        let mut trailer = Dict::new();
        trailer.set("Size", size as i64);
        trailer.set("Root", root);
        if let Some(info) = self.info {
            trailer.set("Info", info);
        }
        if let Some(encrypt) = self.encrypt {
            trailer.set("Encrypt", encrypt);
        }
        if let Some(id) = &self.id {
            trailer.set(
                "ID",
                Object::Array(vec![
                    Object::String(PdfString::hex(id[0].clone())),
                    Object::String(PdfString::hex(id[1].clone())),
                ]),
            );
        }
        out.extend_from_slice(b"trailer\n");
        Object::Dict(trailer).write_to(&mut out);
        out.extend_from_slice(b"\nstartxref\n");
        write_decimal(xref_offset as u64, &mut out);
        out.extend_from_slice(b"\n%%EOF\n");

        Ok(out)
    }

    /// Serialize using object streams (`/ObjStm`) for every non-stream object
    /// plus a cross-reference stream (`/XRef`), per ISO 32000-1 §7.5.7–7.5.8.
    /// Produces materially smaller files; requires PDF 1.5+.
    fn write_compressed(&self, root: Reference) -> Result<Vec<u8>, WriteError> {
        use cos::Stream;

        let size_orig = self.objects.len();
        for (number, slot) in self.objects.iter().enumerate().skip(1) {
            if slot.is_none() {
                return Err(WriteError::UnassignedObject(number as u32));
            }
        }

        // Non-stream objects go into an object stream; streams stay standalone
        // (a stream cannot be nested inside an `/ObjStm`).
        let mut compressible: Vec<u32> = Vec::new();
        let mut stream_objs: Vec<u32> = Vec::new();
        for number in 1..size_orig {
            match self.objects[number].as_ref().unwrap() {
                Object::Stream(_) => stream_objs.push(number as u32),
                _ => compressible.push(number as u32),
            }
        }

        let has_objstm = !compressible.is_empty();
        let objstm_num = size_orig as u32;
        let xref_num = if has_objstm {
            size_orig as u32 + 1
        } else {
            size_orig as u32
        };
        let total = xref_num as usize + 1;

        // Cross-reference entries: (type, field2, field3). Slot 0 is the free head.
        let mut entries: Vec<(u8, u32, u32)> = vec![(0, 0, 65535); total];

        // Build the object stream: a header of "objnum offset" pairs followed by
        // the concatenated object bodies; `/First` is the header length.
        let mut objstm: Option<Stream> = None;
        if has_objstm {
            let mut header = Vec::new();
            let mut bodies = Vec::new();
            for (idx, &num) in compressible.iter().enumerate() {
                let off = bodies.len();
                self.objects[num as usize]
                    .as_ref()
                    .unwrap()
                    .write_to(&mut bodies);
                bodies.push(b'\n');
                write_decimal(num as u64, &mut header);
                header.push(b' ');
                write_decimal(off as u64, &mut header);
                header.push(b' ');
                entries[num as usize] = (2, objstm_num, idx as u32);
            }
            let first = header.len();
            let mut data = header;
            data.extend_from_slice(&bodies);
            let dict = Dict::new()
                .with("Type", Object::name("ObjStm"))
                .with("N", compressible.len() as i64)
                .with("First", first as i64)
                .with("Filter", Object::name("FlateDecode"));
            objstm = Some(Stream {
                dict,
                data: flate_encode(&data),
            });
        }

        // Header + binary marker (force ≥1.5 for the stream xref).
        let mut out = Vec::with_capacity(1024);
        let header = match self.version {
            PdfVersion::V1_4 => b"%PDF-1.5".as_slice(),
            other => other.header_bytes(),
        };
        out.extend_from_slice(header);
        out.push(b'\n');
        out.extend_from_slice(&[b'%', 0xE2, 0xE3, 0xCF, 0xD3]);
        out.push(b'\n');

        // Standalone stream objects.
        for &num in &stream_objs {
            entries[num as usize] = (1, out.len() as u32, 0);
            write_decimal(num as u64, &mut out);
            out.extend_from_slice(b" 0 obj\n");
            self.objects[num as usize]
                .as_ref()
                .unwrap()
                .write_to(&mut out);
            out.extend_from_slice(b"\nendobj\n");
        }
        // The object stream itself.
        if let Some(stm) = objstm {
            entries[objstm_num as usize] = (1, out.len() as u32, 0);
            write_decimal(objstm_num as u64, &mut out);
            out.extend_from_slice(b" 0 obj\n");
            Object::Stream(stm).write_to(&mut out);
            out.extend_from_slice(b"\nendobj\n");
        }

        // The cross-reference stream (its own entry is type 1 at this offset).
        let xref_offset = out.len();
        entries[xref_num as usize] = (1, xref_offset as u32, 0);
        let mut xdata = Vec::with_capacity(total * 7);
        for &(t, f2, f3) in &entries {
            xdata.push(t);
            xdata.extend_from_slice(&f2.to_be_bytes());
            xdata.extend_from_slice(&(f3 as u16).to_be_bytes());
        }
        let mut xdict = Dict::new()
            .with("Type", Object::name("XRef"))
            .with("Size", total as i64)
            .with("Root", root)
            .with(
                "W",
                Object::Array(vec![
                    Object::Integer(1),
                    Object::Integer(4),
                    Object::Integer(2),
                ]),
            )
            .with(
                "Index",
                Object::Array(vec![Object::Integer(0), Object::Integer(total as i64)]),
            )
            .with("Filter", Object::name("FlateDecode"));
        if let Some(info) = self.info {
            xdict.set("Info", info);
        }
        if let Some(id) = &self.id {
            xdict.set(
                "ID",
                Object::Array(vec![
                    Object::String(PdfString::hex(id[0].clone())),
                    Object::String(PdfString::hex(id[1].clone())),
                ]),
            );
        }
        write_decimal(xref_num as u64, &mut out);
        out.extend_from_slice(b" 0 obj\n");
        Object::Stream(Stream {
            dict: xdict,
            data: flate_encode(&xdata),
        })
        .write_to(&mut out);
        out.extend_from_slice(b"\nendobj\n");

        out.extend_from_slice(b"startxref\n");
        write_decimal(xref_offset as u64, &mut out);
        out.extend_from_slice(b"\n%%EOF\n");
        Ok(out)
    }
}

/// zlib-compress (`FlateDecode`) for object/cross-reference streams.
fn flate_encode(data: &[u8]) -> Vec<u8> {
    use flate2::{write::ZlibEncoder, Compression};
    use std::io::Write;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .expect("zlib write to Vec is infallible");
    encoder.finish().expect("zlib finish")
}

/// Write a 20-byte classic xref entry: `nnnnnnnnnn ggggg t\r\n`.
fn write_xref_entry(offset: u32, generation: u16, kind: u8, out: &mut Vec<u8>) {
    write_zero_padded(offset as u64, 10, out);
    out.push(b' ');
    write_zero_padded(generation as u64, 5, out);
    out.push(b' ');
    out.push(kind);
    out.extend_from_slice(b"\r\n");
}

fn write_decimal(mut v: u64, out: &mut Vec<u8>) {
    if v == 0 {
        out.push(b'0');
        return;
    }
    let mut tmp = [0u8; 20];
    let mut idx = tmp.len();
    while v > 0 {
        idx -= 1;
        tmp[idx] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    out.extend_from_slice(&tmp[idx..]);
}

fn write_zero_padded(v: u64, width: usize, out: &mut Vec<u8>) {
    let mut tmp = [0u8; 20];
    let mut idx = tmp.len();
    let mut n = v;
    loop {
        idx -= 1;
        tmp[idx] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    let digits = &tmp[idx..];
    for _ in digits.len()..width {
        out.push(b'0');
    }
    out.extend_from_slice(digits);
}

#[cfg(test)]
mod tests {
    use super::*;
    use cos::Dict;

    #[test]
    fn missing_root_errors() {
        let doc = Document::new(PdfVersion::V1_7);
        assert_eq!(doc.write().unwrap_err(), WriteError::MissingRoot);
    }

    #[test]
    fn unassigned_object_errors() {
        let mut doc = Document::new(PdfVersion::V1_7);
        let r = doc.reserve();
        doc.set_root(r); // root points at the reserved-but-empty slot
        assert_eq!(doc.write().unwrap_err(), WriteError::UnassignedObject(1));
    }

    #[test]
    fn minimal_structure_is_byte_correct() {
        let mut doc = Document::new(PdfVersion::V1_7);
        let catalog = doc.add(Dict::new().with("Type", Object::name("Catalog")));
        doc.set_root(catalog);
        let bytes = doc.write().unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with("%PDF-1.7\n"));
        assert!(text.contains("1 0 obj\n<< /Type /Catalog >>\nendobj\n"));
        assert!(text.contains("xref\n0 2\n"));
        assert!(text.contains("0000000000 65535 f\r\n"));
        assert!(text.contains("/Root 1 0 R"));
        assert!(text.trim_end().ends_with("%%EOF"));
    }

    #[test]
    fn xref_offsets_point_at_obj_keyword() {
        let mut doc = Document::new(PdfVersion::V1_7);
        let catalog = doc.add(Dict::new().with("Type", Object::name("Catalog")));
        doc.set_root(catalog);
        let bytes = doc.write().unwrap();

        // Extract the offset recorded for object 1 from the xref table and
        // confirm the bytes there begin "1 0 obj".
        let xref_pos = find(&bytes, b"xref\n").unwrap();
        let after = &bytes[xref_pos..];
        // Lines: "xref\n", "0 2\n", entry0, entry1
        let entry1_line = String::from_utf8_lossy(after)
            .lines()
            .nth(3)
            .unwrap()
            .to_string();
        let offset: usize = entry1_line[..10].parse().unwrap();
        assert_eq!(&bytes[offset..offset + 7], b"1 0 obj");
    }

    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }
}

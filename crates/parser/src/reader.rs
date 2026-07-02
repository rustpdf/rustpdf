//! The high-level reader: ties xref, objects, filters and decryption together,
//! materializes every object (including those in object streams, Fase 5.6),
//! applies decryption (Fase 5.9) and walks the page tree.

use std::collections::BTreeMap;

use cos::{Dict, Object, Stream};

/// Shared sentinel returned when a reference dangles.
static NULL: Object = Object::Null;

use crate::crypt::Decryptor;
use crate::error::{PdfError, Result};
use crate::filters;
use crate::lexer::Lexer;
use crate::object;
use crate::xref::{filter_chain, parse_indirect_at, ObjLoc, Xref};

/// A parsed PDF document. All objects are materialized and decrypted up front,
/// so the reader owns no borrowed state and is `Send`.
#[derive(Debug, Clone)]
pub struct PdfReader {
    objects: BTreeMap<u32, Object>,
    trailer: Dict,
}

impl PdfReader {
    /// Parse a PDF with the empty (default) password.
    pub fn parse(data: impl AsRef<[u8]>) -> Result<PdfReader> {
        Self::parse_with_password(data, b"")
    }

    /// Parse a PDF from a file.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<PdfReader> {
        let data = std::fs::read(path).map_err(|e| PdfError::Syntax(e.to_string()))?;
        Self::parse(data)
    }

    /// Parse a PDF, authenticating `password` for encrypted files.
    pub fn parse_with_password(data: impl AsRef<[u8]>, password: &[u8]) -> Result<PdfReader> {
        let data = data.as_ref();
        // Tolerate junk before the header, but require the marker to exist.
        if find(data, b"%PDF-").is_none() {
            return Err(PdfError::NotAPdf);
        }

        let xref = Xref::read(data)?;
        let trailer = xref.trailer.clone();

        // Set up decryption if /Encrypt is present.
        let (decryptor, encrypt_num) = setup_decryption(data, &xref, &trailer, password)?;

        // Pass 1: materialize objects stored at byte offsets.
        let mut objects: BTreeMap<u32, Object> = BTreeMap::new();
        let mut gens: BTreeMap<u32, u16> = BTreeMap::new();
        for (&num, loc) in &xref.entries {
            if let ObjLoc::Offset(off) = loc {
                if let Ok((_n, gen, obj)) = parse_indirect_at(data, *off) {
                    gens.insert(num, gen);
                    objects.insert(num, obj);
                }
            }
        }

        // Pass 2: decrypt offset objects (never the /Encrypt dict itself).
        if let Some(dec) = &decryptor {
            for (&num, obj) in objects.iter_mut() {
                if Some(num) == encrypt_num {
                    continue;
                }
                let gen = gens.get(&num).copied().unwrap_or(0);
                decrypt_object(obj, num, gen, dec);
            }
        }

        // Pass 3: extract objects from object streams (already plaintext once the
        // containing stream is decrypted, so no per-object decryption).
        let mut grouped: BTreeMap<u32, Vec<(u32, u32)>> = BTreeMap::new();
        for (&num, loc) in &xref.entries {
            if let ObjLoc::Compressed { stream, index } = loc {
                grouped.entry(*stream).or_default().push((num, *index));
            }
        }
        for (stm_num, wanted) in grouped {
            let Some(Object::Stream(stm)) = objects.get(&stm_num) else {
                continue;
            };
            if let Ok(contained) = objstm_objects(stm) {
                for (num, index) in wanted {
                    if let Some((_, obj)) = contained.get(index as usize) {
                        objects.entry(num).or_insert_with(|| obj.clone());
                    }
                }
            }
        }

        Ok(PdfReader { objects, trailer })
    }

    /// The document trailer dictionary.
    pub fn trailer(&self) -> &Dict {
        &self.trailer
    }

    /// Every object number present, ascending.
    pub fn object_numbers(&self) -> impl Iterator<Item = u32> + '_ {
        self.objects.keys().copied()
    }

    /// Number of materialized objects.
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// True if the document has no objects.
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Look up an object by number.
    pub fn get(&self, number: u32) -> Option<&Object> {
        self.objects.get(&number)
    }

    /// Resolve a (possibly indirect) object to a concrete value (one hop is
    /// usually enough; this follows chains defensively).
    pub fn resolve<'a>(&'a self, mut obj: &'a Object) -> &'a Object {
        let mut guard = 0;
        while let Object::Reference(r) = obj {
            match self.objects.get(&r.number) {
                Some(next) => obj = next,
                None => return &NULL,
            }
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        obj
    }

    /// Resolve an object expected to be a dictionary (or a stream's dict).
    pub fn resolve_dict<'a>(&'a self, obj: &'a Object) -> Option<&'a Dict> {
        match self.resolve(obj) {
            Object::Dict(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        }
    }

    /// The document catalog (`/Root`).
    pub fn root(&self) -> Result<&Dict> {
        let root = self
            .trailer
            .get("Root")
            .ok_or_else(|| PdfError::Syntax("trailer has no /Root".into()))?;
        self.resolve_dict(root)
            .ok_or_else(|| PdfError::Syntax("/Root is not a dictionary".into()))
    }

    /// The document information dictionary (`/Info`), if any.
    pub fn info(&self) -> Option<&Dict> {
        self.trailer.get("Info").and_then(|o| self.resolve_dict(o))
    }

    /// Decode a stream's data, applying its filter chain (Fase 5.4).
    pub fn stream_data(&self, stream: &Stream) -> Result<Vec<u8>> {
        let (names, parms) = filter_chain(&stream.dict);
        filters::apply_filters(&stream.data, &names, &parms)
    }

    /// All page dictionaries in order, walking `/Pages` (Fase 5.3).
    pub fn pages(&self) -> Vec<Dict> {
        let mut out = Vec::new();
        if let Ok(root) = self.root() {
            if let Some(pages) = root.get("Pages") {
                let mut seen = Vec::new();
                self.collect_pages(pages, &mut out, &mut seen);
            }
        }
        out
    }

    fn collect_pages(&self, node: &Object, out: &mut Vec<Dict>, seen: &mut Vec<u32>) {
        if let Object::Reference(r) = node {
            if seen.contains(&r.number) {
                return;
            }
            seen.push(r.number);
        }
        let Some(dict) = self.resolve_dict(node) else {
            return;
        };
        match dict.get("Type") {
            Some(Object::Name(n)) if n.as_str() == "Pages" => {
                if let Some(Object::Array(kids)) = dict.get("Kids").map(|k| self.resolve(k)) {
                    for kid in kids {
                        self.collect_pages(kid, out, seen);
                    }
                }
            }
            _ => out.push(dict.clone()),
        }
    }
}

/// Configure decryption from the trailer's `/Encrypt`, returning the decryptor
/// and the object number of the Encrypt dict (which must not be decrypted).
fn setup_decryption(
    data: &[u8],
    xref: &Xref,
    trailer: &Dict,
    password: &[u8],
) -> Result<(Option<Decryptor>, Option<u32>)> {
    let Some(encrypt) = trailer.get("Encrypt") else {
        return Ok((None, None));
    };
    let (enc_dict, enc_num) = match encrypt {
        Object::Reference(r) => {
            let off = match xref.entries.get(&r.number) {
                Some(ObjLoc::Offset(o)) => *o,
                _ => return Err(PdfError::Encryption("/Encrypt not found".into())),
            };
            let (_, _, obj) = parse_indirect_at(data, off)?;
            match obj {
                Object::Dict(d) => (d, Some(r.number)),
                _ => return Err(PdfError::Encryption("/Encrypt is not a dict".into())),
            }
        }
        Object::Dict(d) => (d.clone(), None),
        _ => return Err(PdfError::Encryption("bad /Encrypt".into())),
    };

    let id0 = match trailer.get("ID") {
        Some(Object::Array(a)) => match a.first() {
            Some(Object::String(s)) => s.as_bytes().to_vec(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };

    let dec = Decryptor::new(&enc_dict, &id0, password)?;
    Ok((Some(dec), enc_num))
}

/// Recursively decrypt all strings and the stream body of `obj`.
fn decrypt_object(obj: &mut Object, num: u32, gen: u16, dec: &Decryptor) {
    match obj {
        Object::String(s) => {
            let plain = dec.decrypt(num, gen, s.as_bytes());
            *s = cos::PdfString::literal(plain);
        }
        Object::Array(items) => {
            for item in items {
                decrypt_object(item, num, gen, dec);
            }
        }
        Object::Dict(d) => decrypt_dict(d, num, gen, dec),
        Object::Stream(s) => {
            decrypt_dict(&mut s.dict, num, gen, dec);
            s.data = dec.decrypt(num, gen, &s.data);
        }
        _ => {}
    }
}

fn decrypt_dict(dict: &mut Dict, num: u32, gen: u16, dec: &Decryptor) {
    // A signature / document-timestamp dictionary's `/Contents` (the CMS
    // container) is excluded from encryption even in an encrypted file
    // (ISO 32000 §7.6.2). Decrypting it would corrupt the DER so every
    // signature reports invalid. Such dicts are identified by their
    // `/ByteRange` entry.
    let is_signature = dict.get("ByteRange").is_some();
    // Rebuild the dict with decrypted values (Dict has no get_mut).
    let mut rebuilt = Dict::new();
    for (k, v) in dict.iter() {
        if is_signature && k.as_str() == "Contents" {
            rebuilt.set(k.clone(), v.clone()); // leave the CMS bytes untouched
            continue;
        }
        let mut v = v.clone();
        decrypt_object(&mut v, num, gen, dec);
        rebuilt.set(k.clone(), v);
    }
    *dict = rebuilt;
}

/// Extract the `(object number, value)` list contained in an object stream.
fn objstm_objects(stream: &Stream) -> Result<Vec<(u32, Object)>> {
    let (names, parms) = filter_chain(&stream.dict);
    let raw = filters::apply_filters(&stream.data, &names, &parms)?;

    let n = stream.dict.get("N").and_then(int).unwrap_or(0).max(0) as usize;
    let first = stream.dict.get("First").and_then(int).unwrap_or(0).max(0) as usize;

    // Header: N pairs of (object number, offset relative to First). `/N` is
    // attacker-controlled — a lie like `/N 1e18` would blow `with_capacity` with
    // a `capacity overflow` panic. Each header entry costs at least a couple of
    // bytes, so the raw length is a safe upper bound for the pre-reservation;
    // the loop still stops early when tokens run out.
    let mut header = Lexer::new(&raw);
    let mut entries = Vec::with_capacity(n.min(raw.len()));
    for _ in 0..n {
        let num = match header.next_token() {
            Some(crate::lexer::Token::Integer(v)) if v >= 0 => v as u32,
            _ => break,
        };
        let off = match header.next_token() {
            Some(crate::lexer::Token::Integer(v)) if v >= 0 => v as usize,
            _ => break,
        };
        entries.push((num, off));
    }

    let no_resolve = |_: u32, _: u16| None;
    let mut out = Vec::with_capacity(entries.len());
    for (num, off) in entries {
        let mut lex = Lexer::at(&raw, first + off);
        if let Ok(value) = object::parse_value(&mut lex, &no_resolve) {
            out.push((num, value));
        }
    }
    Ok(out)
}

fn int(o: &Object) -> Option<i64> {
    match o {
        Object::Integer(n) => Some(*n),
        _ => None,
    }
}

fn find(data: &[u8], needle: &[u8]) -> Option<usize> {
    data.windows(needle.len()).position(|w| w == needle)
}

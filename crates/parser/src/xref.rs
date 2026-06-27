//! Cross-reference resolution (Fase 5.3/5.5/5.7/5.8): classic `xref` tables,
//! cross-reference streams, hybrid files, and a brute-force recovery scan for
//! damaged files.

use std::collections::BTreeMap;

use cos::{Dict, Object};

use crate::error::{PdfError, Result};
use crate::filters;
use crate::lexer::{Lexer, Token};
use crate::object;

/// Where an object's bytes live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjLoc {
    /// At a byte offset in the file.
    Offset(usize),
    /// Inside an object stream: `(objstm object number, index within it)`.
    Compressed { stream: u32, index: u32 },
}

/// The resolved cross-reference: object locations plus the merged trailer.
#[derive(Debug, Clone)]
pub struct Xref {
    pub entries: BTreeMap<u32, ObjLoc>,
    pub trailer: Dict,
}

fn no_resolve(_: u32, _: u16) -> Option<i64> {
    None
}

impl Xref {
    /// Read the cross-reference for `data`, following `startxref` and `/Prev`,
    /// and falling back to a recovery scan on any failure.
    pub fn read(data: &[u8]) -> Result<Xref> {
        match Self::read_strict(data) {
            Ok(x) if !x.entries.is_empty() && x.trailer.contains_key("Root") => Ok(x),
            _ => Self::recover(data),
        }
    }

    fn read_strict(data: &[u8]) -> Result<Xref> {
        let start = find_startxref(data).ok_or_else(|| PdfError::Xref("no startxref".into()))?;
        let mut entries = BTreeMap::new();
        let mut trailer = Dict::new();
        let mut visited = Vec::new();
        parse_section(data, start, &mut entries, &mut trailer, &mut visited)?;
        Ok(Xref { entries, trailer })
    }

    /// Brute-force recovery: scan for every `N G obj` and the last `trailer`.
    fn recover(data: &[u8]) -> Result<Xref> {
        let mut entries = BTreeMap::new();
        // Later definitions win (incremental updates), so don't early-out.
        let needle = b" obj";
        let mut i = 0;
        while let Some(rel) = find_from(data, needle, i) {
            // Walk back over "N G" before " obj".
            if let Some((num, start)) = parse_obj_header_backwards(data, rel) {
                entries.insert(num, ObjLoc::Offset(start));
            }
            i = rel + needle.len();
        }
        if entries.is_empty() {
            return Err(PdfError::Xref("recovery found no objects".into()));
        }

        let mut trailer = last_trailer(data).unwrap_or_default();
        if !trailer.contains_key("Root") {
            if let Some(root) = find_catalog(data, &entries) {
                trailer.set("Root", root);
            }
        }
        Ok(Xref { entries, trailer })
    }
}

/// Parse a classic table or xref stream at `offset`, recursing into
/// `/Prev` and `/XRefStm`. Entries already present are kept (newest wins).
fn parse_section(
    data: &[u8],
    offset: usize,
    entries: &mut BTreeMap<u32, ObjLoc>,
    trailer: &mut Dict,
    visited: &mut Vec<usize>,
) -> Result<()> {
    if offset >= data.len() || visited.contains(&offset) {
        return Ok(());
    }
    visited.push(offset);

    let mut lex = Lexer::at(data, offset);
    match lex.peek() {
        Some(Token::Keyword(kw)) if kw == b"xref" => {
            let section_trailer = parse_classic(data, offset, entries)?;
            merge_trailer(trailer, &section_trailer);
            // Hybrid-reference file: also read the cross-reference stream.
            if let Some(Object::Integer(xrefstm)) = section_trailer.get("XRefStm") {
                let _ = parse_section(data, *xrefstm as usize, entries, trailer, visited);
            }
            if let Some(Object::Integer(prev)) = section_trailer.get("Prev") {
                parse_section(data, *prev as usize, entries, trailer, visited)?;
            }
        }
        _ => {
            // Expect an xref stream object.
            let section_trailer = parse_xref_stream(data, offset, entries)?;
            merge_trailer(trailer, &section_trailer);
            if let Some(Object::Integer(prev)) = section_trailer.get("Prev") {
                parse_section(data, *prev as usize, entries, trailer, visited)?;
            }
        }
    }
    Ok(())
}

/// Parse a classic `xref` table; returns its trailer dictionary.
fn parse_classic(data: &[u8], offset: usize, entries: &mut BTreeMap<u32, ObjLoc>) -> Result<Dict> {
    let mut lex = Lexer::at(data, offset);
    match lex.next_token() {
        Some(Token::Keyword(kw)) if kw == b"xref" => {}
        _ => return Err(PdfError::Xref("expected 'xref'".into())),
    }

    loop {
        // Either a subsection header "start count" or the "trailer" keyword.
        match lex.peek() {
            Some(Token::Keyword(kw)) if kw == b"trailer" => {
                let _ = lex.next_token();
                break;
            }
            Some(Token::Integer(start)) => {
                let _ = lex.next_token();
                let count = match lex.next_token() {
                    Some(Token::Integer(c)) if c >= 0 => c as u32,
                    _ => return Err(PdfError::Xref("bad subsection count".into())),
                };
                // Entries are fixed 20-byte records; read them from raw bytes for
                // robustness against irregular whitespace.
                let mut p = lex.pos();
                while data.get(p).is_some_and(|b| crate::lexer::is_whitespace(*b)) {
                    p += 1;
                }
                for k in 0..count {
                    let rec = data.get(p..p + 18);
                    if let Some(rec) = rec {
                        let off: usize = ascii_uint(&rec[0..10]);
                        let kind = rec[17];
                        let num = start as u32 + k;
                        if kind == b'n' {
                            entries.entry(num).or_insert(ObjLoc::Offset(off));
                        }
                    }
                    p += 20;
                }
                lex.seek(p);
            }
            _ => return Err(PdfError::Xref("malformed xref subsection".into())),
        }
    }

    object::parse_value(&mut lex, &no_resolve).and_then(|o| match o {
        Object::Dict(d) => Ok(d),
        _ => Err(PdfError::Xref("trailer is not a dictionary".into())),
    })
}

/// Parse a cross-reference stream object at `offset`; returns its dict.
fn parse_xref_stream(
    data: &[u8],
    offset: usize,
    entries: &mut BTreeMap<u32, ObjLoc>,
) -> Result<Dict> {
    let (_num, _gen, obj) = parse_indirect_at(data, offset)?;
    let stream = match obj {
        Object::Stream(s) => s,
        _ => return Err(PdfError::Xref("xref offset is not a stream".into())),
    };
    let dict = stream.dict.clone();

    let raw = decode_xref_stream(&stream)?;
    let w = dict
        .get("W")
        .and_then(array_of_ints)
        .ok_or_else(|| PdfError::Xref("xref stream missing /W".into()))?;
    if w.len() < 3 {
        return Err(PdfError::Xref("xref /W must have 3 widths".into()));
    }
    let (w0, w1, w2) = (w[0] as usize, w[1] as usize, w[2] as usize);
    let row = w0 + w1 + w2;
    if row == 0 {
        return Err(PdfError::Xref("xref /W widths are all zero".into()));
    }

    let size = dict.get("Size").and_then(as_int).unwrap_or(0);
    // /Index pairs (start, count); default is [0 Size].
    let index = dict
        .get("Index")
        .and_then(array_of_ints)
        .unwrap_or_else(|| vec![0, size]);

    let mut pos = 0usize;
    let mut pairs = index.chunks_exact(2);
    for pair in &mut pairs {
        let (start, count) = (pair[0], pair[1]);
        for k in 0..count {
            if pos + row > raw.len() {
                break;
            }
            let f0 = if w0 == 0 {
                1
            } else {
                read_be(&raw[pos..pos + w0])
            };
            let f1 = read_be(&raw[pos + w0..pos + w0 + w1]);
            let f2 = read_be(&raw[pos + w0 + w1..pos + row]);
            pos += row;
            let num = (start + k) as u32;
            match f0 {
                1 => {
                    entries.entry(num).or_insert(ObjLoc::Offset(f1 as usize));
                }
                2 => {
                    entries.entry(num).or_insert(ObjLoc::Compressed {
                        stream: f1 as u32,
                        index: f2 as u32,
                    });
                }
                _ => {} // type 0 = free
            }
        }
    }
    Ok(dict)
}

/// Decode an xref stream's data with its filters and predictor.
fn decode_xref_stream(stream: &cos::Stream) -> Result<Vec<u8>> {
    let (filters, parms) = filter_chain(&stream.dict);
    filters::apply_filters(&stream.data, &filters, &parms)
}

/// Parse an indirect object `N G obj <value> endobj` at `offset`.
pub fn parse_indirect_at(data: &[u8], offset: usize) -> Result<(u32, u16, Object)> {
    let mut lex = Lexer::at(data, offset);
    let num = match lex.next_token() {
        Some(Token::Integer(n)) if n >= 0 => n as u32,
        _ => return Err(PdfError::Syntax("expected object number".into())),
    };
    let gen = match lex.next_token() {
        Some(Token::Integer(g)) if g >= 0 => g as u16,
        _ => return Err(PdfError::Syntax("expected generation".into())),
    };
    match lex.next_token() {
        Some(Token::Keyword(kw)) if kw == b"obj" => {}
        _ => return Err(PdfError::Syntax("expected 'obj'".into())),
    }
    let value = object::parse_value(&mut lex, &no_resolve)?;
    Ok((num, gen, value))
}

/// Extract `(filter names, decode-parms)` from a stream dictionary.
pub fn filter_chain(dict: &Dict) -> (Vec<Vec<u8>>, Vec<Option<Dict>>) {
    let mut filters = Vec::new();
    match dict.get("Filter") {
        Some(Object::Name(n)) => filters.push(n.as_str().as_bytes().to_vec()),
        Some(Object::Array(a)) => {
            for o in a {
                if let Object::Name(n) = o {
                    filters.push(n.as_str().as_bytes().to_vec());
                }
            }
        }
        _ => {}
    }
    let mut parms: Vec<Option<Dict>> = Vec::new();
    match dict.get("DecodeParms").or_else(|| dict.get("DP")) {
        Some(Object::Dict(d)) => parms.push(Some(d.clone())),
        Some(Object::Array(a)) => {
            for o in a {
                parms.push(match o {
                    Object::Dict(d) => Some(d.clone()),
                    _ => None,
                });
            }
        }
        _ => {}
    }
    while parms.len() < filters.len() {
        parms.push(None);
    }
    (filters, parms)
}

// ---- helpers ---------------------------------------------------------------

fn merge_trailer(into: &mut Dict, from: &Dict) {
    for (k, v) in from.iter() {
        if !into.contains_key(k.as_str()) {
            into.set(k.clone(), v.clone());
        }
    }
}

fn as_int(o: &Object) -> Option<i64> {
    match o {
        Object::Integer(n) => Some(*n),
        _ => None,
    }
}

fn array_of_ints(o: &Object) -> Option<Vec<i64>> {
    match o {
        Object::Array(a) => a.iter().map(as_int).collect(),
        _ => None,
    }
}

fn read_be(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0u64, |acc, &b| (acc << 8) | b as u64)
}

fn ascii_uint(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .filter(|b| b.is_ascii_digit())
        .fold(0usize, |acc, &b| acc * 10 + (b - b'0') as usize)
}

fn find_startxref(data: &[u8]) -> Option<usize> {
    let kw = b"startxref";
    let rel = data.windows(kw.len()).rposition(|w| w == kw)?;
    let mut lex = Lexer::at(data, rel + kw.len());
    match lex.next_token() {
        Some(Token::Integer(n)) if n >= 0 => Some(n as usize),
        _ => None,
    }
}

fn find_from(data: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= data.len() {
        return None;
    }
    data[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// Given the position of " obj", walk back over "N G" to find the object number
/// and the offset where the object definition starts.
fn parse_obj_header_backwards(data: &[u8], obj_pos: usize) -> Option<(u32, usize)> {
    let mut p = obj_pos;
    // skip back over whitespace before " obj" already handled (needle has space)
    let gen_end = skip_ws_back(data, p);
    let gen_start = skip_digits_back(data, gen_end)?;
    let num_end = skip_ws_back(data, gen_start);
    let num_start = skip_digits_back(data, num_end)?;
    let num: u32 = std::str::from_utf8(&data[num_start..num_end])
        .ok()?
        .parse()
        .ok()?;
    p = num_start;
    Some((num, p))
}

fn skip_ws_back(data: &[u8], mut p: usize) -> usize {
    while p > 0 && crate::lexer::is_whitespace(data[p - 1]) {
        p -= 1;
    }
    p
}

fn skip_digits_back(data: &[u8], end: usize) -> Option<usize> {
    let mut p = end;
    while p > 0 && data[p - 1].is_ascii_digit() {
        p -= 1;
    }
    if p == end {
        None
    } else {
        Some(p)
    }
}

fn last_trailer(data: &[u8]) -> Option<Dict> {
    let kw = b"trailer";
    let rel = data.windows(kw.len()).rposition(|w| w == kw)?;
    let mut lex = Lexer::at(data, rel + kw.len());
    match object::parse_value(&mut lex, &no_resolve) {
        Ok(Object::Dict(d)) => Some(d),
        _ => None,
    }
}

/// Scan recovered objects for one whose dict has `/Type /Catalog`.
fn find_catalog(data: &[u8], entries: &BTreeMap<u32, ObjLoc>) -> Option<Object> {
    for (&num, loc) in entries {
        if let ObjLoc::Offset(off) = loc {
            if let Ok((_, gen, Object::Dict(d))) = parse_indirect_at(data, *off) {
                if d.get("Type") == Some(&Object::name("Catalog")) {
                    return Some(Object::Reference(cos::Reference {
                        number: num,
                        generation: gen,
                    }));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_uint_parses() {
        assert_eq!(ascii_uint(b"0000000123"), 123);
        assert_eq!(ascii_uint(b"0000000000"), 0);
    }

    #[test]
    fn read_be_widths() {
        assert_eq!(read_be(&[0x01]), 1);
        assert_eq!(read_be(&[0x01, 0x00]), 256);
        assert_eq!(read_be(&[0xFF, 0xFF, 0xFF]), 0xFF_FFFF);
    }

    #[test]
    fn obj_header_backwards() {
        let data = b"%PDF\n12 0 obj";
        let pos = data.windows(4).position(|w| w == b" obj").unwrap();
        assert_eq!(parse_obj_header_backwards(data, pos), Some((12, 5)));
    }
}

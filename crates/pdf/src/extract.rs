//! Text extraction (Fase 6.4): decode page content streams, map shown glyph
//! codes back to Unicode via each font's `ToUnicode` CMap, and infer spaces and
//! line breaks from text positioning. Tuned for good-quality output (RAG use).

use std::collections::BTreeMap;

use cos::{Dict, Object};
use parser::{Lexer, PdfReader, Token};

/// Per-font decoding info derived from its dictionary.
struct FontInfo {
    two_byte: bool,
    to_unicode: BTreeMap<u32, String>,
}

/// Extract all text from a PDF (one page per block, separated by form feeds).
pub fn extract_text(bytes: impl AsRef<[u8]>) -> Result<String, parser::PdfError> {
    let reader = PdfReader::parse(bytes)?;
    let mut out = String::new();
    for (i, page) in reader.pages().iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&page_text(&reader, page));
    }
    Ok(out)
}

/// Extract the text of a single page dictionary.
pub fn page_text(reader: &PdfReader, page: &Dict) -> String {
    let mut out = String::new();
    let resources = page
        .get("Resources")
        .and_then(|o| reader.resolve_dict(o))
        .cloned()
        .unwrap_or_default();
    let content = page_content(reader, page);
    run_content(reader, &content, &resources, &mut out, 0);
    append_annotations(reader, page, &mut out);
    out
}

/// Decode one content stream, recursing into Form XObjects drawn with `Do`
/// (so text baked into XObjects — e.g. flattened form fields — is extracted).
/// `depth` guards against XObject reference cycles.
fn run_content(reader: &PdfReader, content: &[u8], resources: &Dict, out: &mut String, depth: u32) {
    if depth > 12 {
        return;
    }
    let fonts = fonts_from_resources(reader, resources);
    let mut lex = Lexer::new(content);
    let mut operands: Vec<Operand> = Vec::new();
    let mut current: Option<&FontInfo> = None;
    let mut text_y: Option<f64> = None;
    let mut wrote_on_line = false;

    while let Some(tok) = lex.next_token() {
        match tok {
            Token::Integer(n) => operands.push(Operand::Num(n as f64)),
            Token::Real(r) => operands.push(Operand::Num(r)),
            Token::Str(s) => operands.push(Operand::Str(s)),
            Token::Name(n) => operands.push(Operand::Name(n)),
            Token::ArrayOpen => operands.push(Operand::ArrayMark),
            Token::ArrayClose => collapse_array(&mut operands),
            Token::DictOpen | Token::DictClose => operands.clear(),
            Token::Keyword(kw) => {
                match kw.as_slice() {
                    b"BT" => {
                        // A new text object resets the text matrix, but the on-page
                        // baseline is absolute: do NOT clear `text_y`/`wrote_on_line`
                        // here, or a downward baseline step between two consecutive
                        // text objects (each its own `BT … Tm … ET`, which is exactly
                        // how every `show_text` is emitted) would never be detected
                        // and the two lines would be concatenated with no separator.
                    }
                    b"Tf" => {
                        if let Some(Operand::Name(name)) = nth_from_end(&operands, 1) {
                            current = fonts.get(name.as_slice());
                        }
                    }
                    b"Td" | b"TD" => {
                        // Relative move; a downward step starts a new line.
                        if let Some(Operand::Num(ty)) = nth_from_end(&operands, 0) {
                            if *ty != 0.0 && wrote_on_line {
                                out.push('\n');
                                wrote_on_line = false;
                            }
                        }
                    }
                    b"Tm" => {
                        if let Some(Operand::Num(y)) = nth_from_end(&operands, 0) {
                            let y = *y;
                            if let Some(prev) = text_y {
                                if (prev - y).abs() > 0.5 && wrote_on_line {
                                    out.push('\n');
                                    wrote_on_line = false;
                                }
                            }
                            text_y = Some(y);
                        }
                    }
                    b"T*" => {
                        out.push('\n');
                        wrote_on_line = false;
                    }
                    b"Tj" => {
                        if let Some(Operand::Str(s)) = nth_from_end(&operands, 0) {
                            show(s, current, out);
                            wrote_on_line = true;
                        }
                    }
                    b"TJ" => {
                        if let Some(Operand::Array(items)) = nth_from_end(&operands, 0) {
                            for el in items {
                                match el {
                                    Operand::Str(s) => show(s, current, out),
                                    // A large negative adjustment is a word gap.
                                    Operand::Num(n) if *n < -100.0 => out.push(' '),
                                    _ => {}
                                }
                            }
                            wrote_on_line = true;
                        }
                    }
                    b"Do" => {
                        if let Some(Operand::Name(name)) = nth_from_end(&operands, 0) {
                            do_xobject(reader, name, resources, out, depth);
                        }
                    }
                    _ => {}
                }
                operands.clear();
            }
        }
    }
}

#[derive(Debug, Clone)]
enum Operand {
    Num(f64),
    Str(Vec<u8>),
    Name(Vec<u8>),
    Array(Vec<Operand>),
    ArrayMark,
}

fn nth_from_end(stack: &[Operand], n: usize) -> Option<&Operand> {
    stack.len().checked_sub(n + 1).map(|i| &stack[i])
}

/// Collapse the operands back to the last `ArrayMark` into an `Array` operand.
fn collapse_array(stack: &mut Vec<Operand>) {
    if let Some(start) = stack.iter().rposition(|o| matches!(o, Operand::ArrayMark)) {
        let items: Vec<Operand> = stack.split_off(start + 1);
        stack.pop(); // remove the ArrayMark
        stack.push(Operand::Array(items));
    }
}

/// Map one shown string to Unicode using the current font.
fn show(bytes: &[u8], font: Option<&FontInfo>, out: &mut String) {
    let Some(font) = font else {
        // No font selected: best-effort Latin-1.
        out.extend(bytes.iter().map(|&b| b as char));
        return;
    };
    let width = if font.two_byte { 2 } else { 1 };
    for chunk in bytes.chunks(width) {
        let code = chunk.iter().fold(0u32, |a, &b| (a << 8) | b as u32);
        if let Some(s) = font.to_unicode.get(&code) {
            out.push_str(s);
        } else if !font.two_byte {
            out.push(code as u8 as char);
        }
    }
}

/// Build the page's `name -> FontInfo` table from its `/Resources /Font`.
fn fonts_from_resources(reader: &PdfReader, resources: &Dict) -> BTreeMap<Vec<u8>, FontInfo> {
    let mut map = BTreeMap::new();
    let Some(fonts) = resources.get("Font").and_then(|o| reader.resolve_dict(o)) else {
        return map;
    };
    for (name, font_ref) in fonts.iter() {
        let Some(font_dict) = reader.resolve_dict(font_ref) else {
            continue;
        };
        let two_byte = font_dict.get("Subtype").and_then(name_str).as_deref() == Some("Type0");
        let to_unicode = font_dict
            .get("ToUnicode")
            .map(|o| reader.resolve(o))
            .and_then(|o| match o {
                Object::Stream(s) => reader.stream_data(s).ok(),
                _ => None,
            })
            .map(|data| parse_cmap(&data))
            .unwrap_or_default();
        map.insert(
            name.as_str().as_bytes().to_vec(),
            FontInfo {
                two_byte,
                to_unicode,
            },
        );
    }
    map
}

/// Recurse into a Form XObject drawn with `Do`, decoding its text with the
/// form's own `/Resources` (falling back to the parent's).
fn do_xobject(reader: &PdfReader, name: &[u8], resources: &Dict, out: &mut String, depth: u32) {
    let Some(xobjs) = resources
        .get("XObject")
        .and_then(|o| reader.resolve_dict(o))
    else {
        return;
    };
    let Ok(key) = std::str::from_utf8(name) else {
        return;
    };
    let Some(Object::Stream(s)) = xobjs.get(key).map(|o| reader.resolve(o)) else {
        return;
    };
    if s.dict.get("Subtype").and_then(name_str).as_deref() != Some("Form") {
        return;
    }
    let sub_res = s
        .dict
        .get("Resources")
        .and_then(|o| reader.resolve_dict(o))
        .cloned()
        .unwrap_or_else(|| resources.clone());
    if let Ok(data) = reader.stream_data(s) {
        out.push(' ');
        run_content(reader, &data, &sub_res, out, depth + 1);
    }
}

/// Extract text from each annotation's normal appearance (`/AP /N`). Form-field
/// widgets (text/choice fields) render their value into this appearance stream,
/// so this recovers filled, not-yet-flattened form values.
fn append_annotations(reader: &PdfReader, page: &Dict, out: &mut String) {
    let Some(Object::Array(annots)) = page.get("Annots").map(|o| reader.resolve(o)) else {
        return;
    };
    for a in annots {
        let Some(annot) = reader.resolve_dict(a) else {
            continue;
        };
        let Some(ap) = annot.get("AP").and_then(|o| reader.resolve_dict(o)) else {
            continue;
        };
        let Some(n) = ap.get("N").map(|o| reader.resolve(o)) else {
            continue;
        };
        let stream = match n {
            Object::Stream(s) => Some(s.clone()),
            // Appearance subdictionary keyed by state; /AS selects the current.
            Object::Dict(states) => {
                let chosen = annot
                    .get("AS")
                    .and_then(name_str)
                    .and_then(|k| states.get(k.as_str()).cloned())
                    .or_else(|| states.iter().next().map(|(_, v)| v.clone()));
                match chosen.map(|o| reader.resolve(&o).clone()) {
                    Some(Object::Stream(s)) => Some(s),
                    _ => None,
                }
            }
            _ => None,
        };
        let Some(s) = stream else {
            continue;
        };
        let sub_res = s
            .dict
            .get("Resources")
            .and_then(|o| reader.resolve_dict(o))
            .cloned()
            .unwrap_or_default();
        if let Ok(data) = reader.stream_data(&s) {
            out.push(' ');
            run_content(reader, &data, &sub_res, out, 1);
        }
    }
}

/// Concatenate and decode a page's content stream(s).
fn page_content(reader: &PdfReader, page: &Dict) -> Vec<u8> {
    let mut out = Vec::new();
    let Some(contents) = page.get("Contents") else {
        return out;
    };
    match reader.resolve(contents) {
        Object::Stream(s) => {
            if let Ok(d) = reader.stream_data(s) {
                out.extend_from_slice(&d);
            }
        }
        Object::Array(items) => {
            for it in items {
                if let Object::Stream(s) = reader.resolve(it) {
                    if let Ok(d) = reader.stream_data(s) {
                        out.extend_from_slice(&d);
                        out.push(b'\n');
                    }
                }
            }
        }
        _ => {}
    }
    out
}

/// Parse a `ToUnicode` CMap into a code → string map.
pub(crate) fn parse_cmap(data: &[u8]) -> BTreeMap<u32, String> {
    let mut map = BTreeMap::new();
    let mut lex = Lexer::new(data);
    while let Some(tok) = lex.next_token() {
        match tok {
            Token::Keyword(kw) if kw == b"beginbfchar" => parse_bfchar(&mut lex, &mut map),
            Token::Keyword(kw) if kw == b"beginbfrange" => parse_bfrange(&mut lex, &mut map),
            _ => {}
        }
    }
    map
}

fn parse_bfchar(lex: &mut Lexer, map: &mut BTreeMap<u32, String>) {
    // Each entry is `<src> <dst>`; stop at `endbfchar` or anything unexpected.
    while let Some(Token::Str(src)) = lex.next_token() {
        let dst = match lex.next_token() {
            Some(Token::Str(s)) => s,
            _ => break,
        };
        map.insert(be(&src), utf16be_string(&dst));
    }
}

fn parse_bfrange(lex: &mut Lexer, map: &mut BTreeMap<u32, String>) {
    while let Some(Token::Str(lo_bytes)) = lex.next_token() {
        let lo = be(&lo_bytes);
        let hi = match lex.next_token() {
            Some(Token::Str(s)) => be(&s),
            _ => break,
        };
        match lex.next_token() {
            Some(Token::Str(dst)) => {
                let base = be(&dst);
                for (i, code) in (lo..=hi).enumerate() {
                    map.insert(code, char_from_u32(base + i as u32));
                }
            }
            Some(Token::ArrayOpen) => {
                let mut code = lo;
                while let Some(t) = lex.next_token() {
                    match t {
                        Token::Str(s) => {
                            map.insert(code, utf16be_string(&s));
                            code += 1;
                        }
                        Token::ArrayClose => break,
                        _ => break,
                    }
                }
            }
            _ => break,
        }
    }
}

fn be(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0u32, |a, &b| (a << 8) | b as u32)
}

/// Decode big-endian UTF-16 bytes to a String (handles surrogate pairs).
fn utf16be_string(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks(2)
        .map(|c| {
            if c.len() == 2 {
                u16::from_be_bytes([c[0], c[1]])
            } else {
                c[0] as u16
            }
        })
        .collect();
    String::from_utf16_lossy(&units)
}

fn char_from_u32(v: u32) -> String {
    char::from_u32(v).map(String::from).unwrap_or_default()
}

fn name_str(o: &Object) -> Option<String> {
    match o {
        Object::Name(n) => Some(n.as_str().to_string()),
        _ => None,
    }
}

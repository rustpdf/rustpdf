//! Object parsing (Fase 5.2): turn a token stream into [`cos::Object`] values,
//! including indirect references (`N G R`), dictionaries, arrays and streams.

use cos::{Dict, Object, PdfString, Reference, Stream};

use crate::error::{PdfError, Result};
use crate::lexer::{Lexer, Token};

/// Resolves an indirect `/Length` to a concrete integer (needed for streams
/// whose length is itself an indirect object).
pub type LengthResolver<'a> = dyn Fn(u32, u16) -> Option<i64> + 'a;

/// Maximum nesting depth for arrays/dictionaries. PDF has no legitimate need
/// for deep nesting (Acrobat/qpdf cap around 100–256); a bound here converts an
/// attacker-controlled `[[[[…` / `<< /a << /a …` from an uncatchable native
/// stack overflow into a recoverable [`PdfError::Syntax`].
const MAX_PARSE_DEPTH: u32 = 200;

/// Parse a single object value at the lexer's current position.
pub fn parse_value(lex: &mut Lexer, resolve_len: &LengthResolver) -> Result<Object> {
    let token = lex
        .next_token()
        .ok_or_else(|| PdfError::Syntax("unexpected end of input".into()))?;
    parse_from(token, lex, resolve_len, 0)
}

/// Like [`parse_value`] but continues an in-progress descent at `depth`.
fn parse_value_depth(lex: &mut Lexer, resolve_len: &LengthResolver, depth: u32) -> Result<Object> {
    let token = lex
        .next_token()
        .ok_or_else(|| PdfError::Syntax("unexpected end of input".into()))?;
    parse_from(token, lex, resolve_len, depth)
}

fn parse_from(
    token: Token,
    lex: &mut Lexer,
    resolve_len: &LengthResolver,
    depth: u32,
) -> Result<Object> {
    if depth > MAX_PARSE_DEPTH {
        return Err(PdfError::Syntax("object nesting too deep".into()));
    }
    match token {
        Token::Integer(n) => Ok(parse_int_or_reference(n, lex)),
        Token::Real(r) => Ok(Object::Real(r)),
        Token::Str(bytes) => Ok(Object::String(PdfString::literal(bytes))),
        Token::Name(bytes) => Ok(Object::Name(cos::Name::new(
            String::from_utf8_lossy(&bytes).into_owned(),
        ))),
        Token::ArrayOpen => parse_array(lex, resolve_len, depth + 1),
        Token::DictOpen => parse_dict_or_stream(lex, resolve_len, depth + 1),
        Token::Keyword(kw) => match kw.as_slice() {
            b"true" => Ok(Object::Bool(true)),
            b"false" => Ok(Object::Bool(false)),
            b"null" => Ok(Object::Null),
            other => Err(PdfError::Syntax(format!(
                "unexpected keyword {:?}",
                String::from_utf8_lossy(other)
            ))),
        },
        other => Err(PdfError::Syntax(format!("unexpected token {other:?}"))),
    }
}

/// After reading `Integer(n)`, look ahead for `g R` (a reference). If it is not
/// a reference, the cursor is restored to just after `n`.
fn parse_int_or_reference(n: i64, lex: &mut Lexer) -> Object {
    let after_n = lex.pos();
    if let Some(Token::Integer(g)) = lex.next_token() {
        if let Some(Token::Keyword(kw)) = lex.next_token() {
            if kw == b"R" && n >= 0 && (0..=u16::MAX as i64).contains(&g) {
                return Object::Reference(Reference {
                    number: n as u32,
                    generation: g as u16,
                });
            }
        }
    }
    lex.seek(after_n);
    Object::Integer(n)
}

fn parse_array(lex: &mut Lexer, resolve_len: &LengthResolver, depth: u32) -> Result<Object> {
    if depth > MAX_PARSE_DEPTH {
        return Err(PdfError::Syntax("object nesting too deep".into()));
    }
    let mut items = Vec::new();
    loop {
        let token = lex
            .next_token()
            .ok_or_else(|| PdfError::Syntax("unterminated array".into()))?;
        if token == Token::ArrayClose {
            break;
        }
        items.push(parse_from(token, lex, resolve_len, depth)?);
    }
    Ok(Object::Array(items))
}

fn parse_dict_or_stream(
    lex: &mut Lexer,
    resolve_len: &LengthResolver,
    depth: u32,
) -> Result<Object> {
    if depth > MAX_PARSE_DEPTH {
        return Err(PdfError::Syntax("object nesting too deep".into()));
    }
    let mut dict = Dict::new();
    loop {
        let token = lex
            .next_token()
            .ok_or_else(|| PdfError::Syntax("unterminated dictionary".into()))?;
        match token {
            Token::DictClose => break,
            Token::Name(key) => {
                let value = parse_value_depth(lex, resolve_len, depth)?;
                dict.set(
                    cos::Name::new(String::from_utf8_lossy(&key).into_owned()),
                    value,
                );
            }
            other => {
                return Err(PdfError::Syntax(format!(
                    "expected name key in dictionary, got {other:?}"
                )))
            }
        }
    }

    // A `stream` keyword immediately after the dictionary makes this a stream.
    if lex.peek() == Some(Token::Keyword(b"stream".to_vec())) {
        let _ = lex.next_token(); // consume `stream`
        let data = read_stream_data(lex, &dict, resolve_len)?;
        return Ok(Object::Stream(Stream { dict, data }));
    }

    Ok(Object::Dict(dict))
}

/// Read raw stream bytes starting after the `stream` keyword.
fn read_stream_data(lex: &mut Lexer, dict: &Dict, resolve_len: &LengthResolver) -> Result<Vec<u8>> {
    let data = lex.data();
    let mut pos = lex.pos();
    // After `stream` comes CRLF or LF (tolerate a lone CR).
    if data.get(pos) == Some(&b'\r') {
        pos += 1;
        if data.get(pos) == Some(&b'\n') {
            pos += 1;
        }
    } else if data.get(pos) == Some(&b'\n') {
        pos += 1;
    }
    let start = pos;

    // Resolve /Length (direct or indirect).
    let length = match dict.get("Length") {
        Some(Object::Integer(n)) if *n >= 0 => Some(*n as usize),
        Some(Object::Reference(r)) => resolve_len(r.number, r.generation)
            .filter(|n| *n >= 0)
            .map(|n| n as usize),
        _ => None,
    };

    let end = match length {
        Some(len) if start + len <= data.len() && trails_endstream(data, start + len) => {
            start + len
        }
        // /Length missing, wrong, or doesn't line up: scan for `endstream`.
        _ => find_endstream(data, start).unwrap_or(data.len()),
    };

    lex.seek(end);
    // Consume optional EOL + `endstream`.
    skip_to_after_keyword(lex, b"endstream");
    Ok(data[start..end].to_vec())
}

/// True if `endstream` appears at `pos` (allowing one EOL before it).
fn trails_endstream(data: &[u8], pos: usize) -> bool {
    let mut p = pos;
    while matches!(data.get(p), Some(b'\r' | b'\n' | b' ' | b'\t')) {
        p += 1;
    }
    data[p..].starts_with(b"endstream")
}

/// Find the byte offset where the stream content ends (before `endstream`),
/// trimming a single trailing EOL that precedes the keyword.
fn find_endstream(data: &[u8], start: usize) -> Option<usize> {
    let rel = data[start..]
        .windows(b"endstream".len())
        .position(|w| w == b"endstream")?;
    let mut end = start + rel;
    // Trim one EOL (CRLF, LF or CR) directly before `endstream`.
    if end > start && data[end - 1] == b'\n' {
        end -= 1;
        if end > start && data[end - 1] == b'\r' {
            end -= 1;
        }
    } else if end > start && data[end - 1] == b'\r' {
        end -= 1;
    }
    Some(end)
}

fn skip_to_after_keyword(lex: &mut Lexer, kw: &[u8]) {
    // Read tokens until we pass the keyword (bounded scan).
    for _ in 0..4 {
        match lex.next_token() {
            Some(Token::Keyword(k)) if k == kw => return,
            Some(_) => continue,
            None => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_resolve(_: u32, _: u16) -> Option<i64> {
        None
    }

    fn parse(s: &[u8]) -> Object {
        let mut lex = Lexer::new(s);
        parse_value(&mut lex, &no_resolve).unwrap()
    }

    #[test]
    fn scalars_and_reference() {
        assert_eq!(parse(b"42"), Object::Integer(42));
        assert_eq!(parse(b"3.5"), Object::Real(3.5));
        assert_eq!(parse(b"true"), Object::Bool(true));
        assert_eq!(parse(b"null"), Object::Null);
        assert_eq!(parse(b"7 0 R"), Object::Reference(Reference::new(7)));
        // Two plain integers in an array stay integers (not a reference).
        assert_eq!(
            parse(b"[1 2]"),
            Object::Array(vec![Object::Integer(1), Object::Integer(2)])
        );
    }

    #[test]
    fn nested_dict_and_array() {
        let o = parse(b"<< /Type /Page /Kids [4 0 R 5 0 R] /Count 2 >>");
        let d = match o {
            Object::Dict(d) => d,
            _ => panic!(),
        };
        assert_eq!(d.get("Type"), Some(&Object::name("Page")));
        assert_eq!(d.get("Count"), Some(&Object::Integer(2)));
        match d.get("Kids") {
            Some(Object::Array(a)) => assert_eq!(a.len(), 2),
            _ => panic!(),
        }
    }

    #[test]
    fn stream_with_direct_length() {
        let o = parse(b"<< /Length 5 >>\nstream\nHELLO\nendstream");
        match o {
            Object::Stream(s) => assert_eq!(s.data, b"HELLO"),
            _ => panic!("expected stream"),
        }
    }

    #[test]
    fn stream_with_wrong_length_falls_back_to_scan() {
        let o = parse(b"<< /Length 999 >>\nstream\nABC\nendstream");
        match o {
            Object::Stream(s) => assert_eq!(s.data, b"ABC"),
            _ => panic!("expected stream"),
        }
    }

    #[test]
    fn deeply_nested_array_errors_instead_of_overflowing_stack() {
        // A pathological run of openers with no closers used to recurse once
        // per `[`, blowing the native stack (uncatchable). It must now fail
        // gracefully with a syntax error.
        let bomb = vec![b'['; 100_000];
        let mut lex = Lexer::new(&bomb);
        let r = parse_value(&mut lex, &no_resolve);
        assert!(r.is_err(), "expected depth-limit syntax error, got {r:?}");
    }

    #[test]
    fn deeply_nested_dict_errors_instead_of_overflowing_stack() {
        let mut bomb = Vec::new();
        for _ in 0..100_000 {
            bomb.extend_from_slice(b"<< /a ");
        }
        let mut lex = Lexer::new(&bomb);
        let r = parse_value(&mut lex, &no_resolve);
        assert!(r.is_err(), "expected depth-limit syntax error, got {r:?}");
    }

    #[test]
    fn legitimately_nested_structure_still_parses() {
        // 50 levels is well within the cap and must round-trip.
        let mut s = vec![b'['; 50];
        s.push(b'1');
        s.extend(std::iter::repeat_n(b']', 50));
        let mut lex = Lexer::new(&s);
        assert!(parse_value(&mut lex, &no_resolve).is_ok());
    }
}

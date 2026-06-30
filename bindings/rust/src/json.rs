//! A tiny, dependency-free JSON parser.
//!
//! The binding only needs to read the small, well-formed array the engine emits
//! from `pdf_verify_signatures_json`, so a full serde stack would be overkill.
//! This handles the JSON subset that output uses: arrays, objects, strings (with
//! the standard escapes), numbers, `true`/`false` and `null`.

/// A parsed JSON value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub(crate) fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(v) => Some(v),
            _ => None,
        }
    }

    pub(crate) fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub(crate) fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Num(n) => Some(*n as i64),
            _ => None,
        }
    }

    pub(crate) fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub(crate) fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }
}

/// Parse a complete JSON document; returns `None` on any malformed input.
pub(crate) fn parse(input: &str) -> Option<Json> {
    let bytes = input.as_bytes();
    let mut p = Parser { bytes, pos: 0 };
    p.skip_ws();
    let v = p.value()?;
    p.skip_ws();
    if p.pos == bytes.len() {
        Some(v)
    } else {
        None
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn value(&mut self) -> Option<Json> {
        self.skip_ws();
        match self.peek()? {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => self.string().map(Json::Str),
            b't' | b'f' => self.boolean(),
            b'n' => self.null(),
            _ => self.number(),
        }
    }

    fn object(&mut self) -> Option<Json> {
        self.pos += 1; // '{'
        let mut entries = Vec::new();
        self.skip_ws();
        if self.peek()? == b'}' {
            self.pos += 1;
            return Some(Json::Object(entries));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            if self.peek()? != b':' {
                return None;
            }
            self.pos += 1;
            let val = self.value()?;
            entries.push((key, val));
            self.skip_ws();
            match self.peek()? {
                b',' => self.pos += 1,
                b'}' => {
                    self.pos += 1;
                    return Some(Json::Object(entries));
                }
                _ => return None,
            }
        }
    }

    fn array(&mut self) -> Option<Json> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek()? == b']' {
            self.pos += 1;
            return Some(Json::Array(items));
        }
        loop {
            let val = self.value()?;
            items.push(val);
            self.skip_ws();
            match self.peek()? {
                b',' => self.pos += 1,
                b']' => {
                    self.pos += 1;
                    return Some(Json::Array(items));
                }
                _ => return None,
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        if self.peek()? != b'"' {
            return None;
        }
        self.pos += 1;
        let mut out = String::new();
        loop {
            let c = self.peek()?;
            self.pos += 1;
            match c {
                b'"' => return Some(out),
                b'\\' => {
                    let e = self.peek()?;
                    self.pos += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000C}'),
                        b'u' => {
                            let cp = self.hex4()?;
                            // Surrogate pairs: combine with a following \uXXXX.
                            if (0xD800..=0xDBFF).contains(&cp) {
                                if self.peek()? != b'\\' {
                                    return None;
                                }
                                self.pos += 1;
                                if self.peek()? != b'u' {
                                    return None;
                                }
                                self.pos += 1;
                                let lo = self.hex4()?;
                                let c = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                out.push(char::from_u32(c)?);
                            } else {
                                out.push(char::from_u32(cp)?);
                            }
                        }
                        _ => return None,
                    }
                }
                // Raw UTF-8 byte: collect bytes until we can decode. The engine
                // emits valid UTF-8, so push the byte and rebuild lazily.
                _ => {
                    // Handle multi-byte UTF-8 by pushing the raw byte sequence.
                    if c < 0x80 {
                        out.push(c as char);
                    } else {
                        let mut buf = vec![c];
                        let extra = if c >= 0xF0 {
                            3
                        } else if c >= 0xE0 {
                            2
                        } else {
                            1
                        };
                        for _ in 0..extra {
                            buf.push(self.peek()?);
                            self.pos += 1;
                        }
                        out.push_str(std::str::from_utf8(&buf).ok()?);
                    }
                }
            }
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..4 {
            let c = self.peek()?;
            self.pos += 1;
            let d = match c {
                b'0'..=b'9' => (c - b'0') as u32,
                b'a'..=b'f' => (c - b'a' + 10) as u32,
                b'A'..=b'F' => (c - b'A' + 10) as u32,
                _ => return None,
            };
            v = v * 16 + d;
        }
        Some(v)
    }

    fn boolean(&mut self) -> Option<Json> {
        if self.bytes[self.pos..].starts_with(b"true") {
            self.pos += 4;
            Some(Json::Bool(true))
        } else if self.bytes[self.pos..].starts_with(b"false") {
            self.pos += 5;
            Some(Json::Bool(false))
        } else {
            None
        }
    }

    fn null(&mut self) -> Option<Json> {
        if self.bytes[self.pos..].starts_with(b"null") {
            self.pos += 4;
            Some(Json::Null)
        } else {
            None
        }
    }

    fn number(&mut self) -> Option<Json> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.' || c == b'e' || c == b'E' {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.pos == start {
            return None;
        }
        std::str::from_utf8(&self.bytes[start..self.pos])
            .ok()?
            .parse::<f64>()
            .ok()
            .map(Json::Num)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_signature_report_array() {
        let input = r#"[{"field_name":"Signature1","sub_filter":"adbe.pkcs7.detached",
            "signer":"Tester","covers_whole_document":true,"digest_valid":true,
            "signature_valid":false,"is_valid":false,"byte_range":[0,1234,1290,560]},
            {"field_name":null,"sub_filter":"ETSI.CAdES.detached","signer":null,
            "covers_whole_document":false,"digest_valid":false,"signature_valid":false,
            "is_valid":false,"byte_range":[0,0,0,0]}]"#;
        let v = parse(input).expect("valid json");
        let arr = v.as_array().expect("array");
        assert_eq!(arr.len(), 2);
        assert_eq!(
            arr[0].get("field_name").unwrap().as_str(),
            Some("Signature1")
        );
        assert_eq!(
            arr[0].get("covers_whole_document").unwrap().as_bool(),
            Some(true)
        );
        let br = arr[0].get("byte_range").unwrap().as_array().unwrap();
        assert_eq!(br[1].as_i64(), Some(1234));
        assert!(arr[1].get("field_name").unwrap().is_null());
        assert!(arr[1].get("signer").unwrap().is_null());
    }

    #[test]
    fn handles_empty_and_escapes() {
        assert_eq!(parse("[]").unwrap().as_array().unwrap().len(), 0);
        let v = parse(r#"{"k":"a\"b\\c\nd","u":"é"}"#).expect("escapes");
        assert_eq!(v.get("k").unwrap().as_str(), Some("a\"b\\c\nd"));
        assert_eq!(v.get("u").unwrap().as_str(), Some("é"));
    }

    #[test]
    fn rejects_malformed() {
        assert!(parse("{bad}").is_none());
        assert!(parse("[1,2,").is_none());
        assert!(parse("").is_none());
    }
}

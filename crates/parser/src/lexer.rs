//! PDF tokenizer (Fase 5.1). Splits raw PDF bytes into COS syntax tokens.
//!
//! The lexer is a cursor over a byte slice; it never panics on malformed input
//! (it returns [`Token::Error`] or stops), which is what lets the recovery path
//! (Fase 5.8) scan arbitrary bytes safely.

/// A single lexical token.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Integer(i64),
    Real(f64),
    /// A name object, already `#XX`-decoded (without the leading `/`).
    Name(Vec<u8>),
    /// A literal `(...)` or hexadecimal `<...>` string, decoded to bytes.
    Str(Vec<u8>),
    ArrayOpen,
    ArrayClose,
    DictOpen,
    DictClose,
    /// A bare keyword: `obj`, `endobj`, `stream`, `R`, `true`, `false`,
    /// `null`, `xref`, `trailer`, `startxref`, etc.
    Keyword(Vec<u8>),
}

/// A cursor-based PDF lexer.
pub struct Lexer<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    /// Create a lexer over `data` starting at byte 0.
    pub fn new(data: &'a [u8]) -> Self {
        Lexer { data, pos: 0 }
    }

    /// Create a lexer positioned at `offset`.
    pub fn at(data: &'a [u8], offset: usize) -> Self {
        Lexer {
            data,
            pos: offset.min(data.len()),
        }
    }

    /// Current byte offset.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Move the cursor to `pos`.
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos.min(self.data.len());
    }

    /// The underlying bytes.
    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    /// Peek the next token without consuming (saves/restores position).
    pub fn peek(&mut self) -> Option<Token> {
        let save = self.pos;
        let t = self.next_token();
        self.pos = save;
        t
    }

    /// Read the next token, or `None` at end of input.
    pub fn next_token(&mut self) -> Option<Token> {
        self.skip_whitespace_and_comments();
        let b = *self.data.get(self.pos)?;
        match b {
            b'[' => {
                self.pos += 1;
                Some(Token::ArrayOpen)
            }
            b']' => {
                self.pos += 1;
                Some(Token::ArrayClose)
            }
            b'<' => {
                if self.data.get(self.pos + 1) == Some(&b'<') {
                    self.pos += 2;
                    Some(Token::DictOpen)
                } else {
                    self.read_hex_string()
                }
            }
            b'>' => {
                if self.data.get(self.pos + 1) == Some(&b'>') {
                    self.pos += 2;
                    Some(Token::DictClose)
                } else {
                    // Stray '>'; skip to avoid an infinite loop.
                    self.pos += 1;
                    self.next_token()
                }
            }
            b'(' => self.read_literal_string(),
            b'/' => self.read_name(),
            b'+' | b'-' | b'.' | b'0'..=b'9' => self.read_number(),
            b')' | b'{' | b'}' => {
                // Unexpected closer/brace — skip one byte and continue.
                self.pos += 1;
                self.next_token()
            }
            _ => self.read_keyword(),
        }
    }

    fn skip_whitespace_and_comments(&mut self) {
        while let Some(&b) = self.data.get(self.pos) {
            if is_whitespace(b) {
                self.pos += 1;
            } else if b == b'%' {
                // Comment to end of line.
                while let Some(&c) = self.data.get(self.pos) {
                    self.pos += 1;
                    if c == b'\n' || c == b'\r' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    fn read_number(&mut self) -> Option<Token> {
        let start = self.pos;
        let mut is_real = false;
        if matches!(self.data.get(self.pos), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        while let Some(&b) = self.data.get(self.pos) {
            match b {
                b'0'..=b'9' => self.pos += 1,
                b'.' => {
                    is_real = true;
                    self.pos += 1;
                }
                // Some producers write malformed reals like "1.2.3" or "--1";
                // stop at the first non-numeric byte.
                _ => break,
            }
        }
        let text = std::str::from_utf8(&self.data[start..self.pos]).ok()?;
        if is_real {
            Some(Token::Real(parse_real(text)))
        } else {
            match text.parse::<i64>() {
                Ok(n) => Some(Token::Integer(n)),
                // Out-of-range or "+"/"-" alone → treat as real/zero.
                Err(_) => Some(Token::Real(parse_real(text))),
            }
        }
    }

    fn read_name(&mut self) -> Option<Token> {
        self.pos += 1; // consume '/'
        let mut out = Vec::new();
        while let Some(&b) = self.data.get(self.pos) {
            if is_whitespace(b) || is_delimiter(b) {
                break;
            }
            if b == b'#' {
                let h = self.data.get(self.pos + 1).and_then(|&c| hex_val(c));
                let l = self.data.get(self.pos + 2).and_then(|&c| hex_val(c));
                if let (Some(h), Some(l)) = (h, l) {
                    out.push(h << 4 | l);
                    self.pos += 3;
                    continue;
                }
            }
            out.push(b);
            self.pos += 1;
        }
        Some(Token::Name(out))
    }

    fn read_literal_string(&mut self) -> Option<Token> {
        self.pos += 1; // consume '('
        let mut out = Vec::new();
        let mut depth = 1i32;
        while let Some(&b) = self.data.get(self.pos) {
            self.pos += 1;
            match b {
                b'\\' => {
                    let Some(&e) = self.data.get(self.pos) else {
                        break;
                    };
                    self.pos += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0C),
                        b'(' => out.push(b'('),
                        b')' => out.push(b')'),
                        b'\\' => out.push(b'\\'),
                        b'\r' => {
                            // Line continuation: also swallow a following \n.
                            if self.data.get(self.pos) == Some(&b'\n') {
                                self.pos += 1;
                            }
                        }
                        b'\n' => {}
                        b'0'..=b'7' => {
                            let mut val = (e - b'0') as u32;
                            for _ in 0..2 {
                                match self.data.get(self.pos) {
                                    Some(&d @ b'0'..=b'7') => {
                                        val = val * 8 + (d - b'0') as u32;
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(val as u8);
                        }
                        other => out.push(other),
                    }
                }
                b'(' => {
                    depth += 1;
                    out.push(b);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push(b);
                }
                _ => out.push(b),
            }
        }
        Some(Token::Str(out))
    }

    fn read_hex_string(&mut self) -> Option<Token> {
        self.pos += 1; // consume '<'
        let mut nibbles = Vec::new();
        while let Some(&b) = self.data.get(self.pos) {
            self.pos += 1;
            if b == b'>' {
                break;
            }
            if let Some(v) = hex_val(b) {
                nibbles.push(v);
            }
            // Whitespace and other bytes inside hex strings are ignored.
        }
        let mut out = Vec::with_capacity(nibbles.len().div_ceil(2));
        let mut chunks = nibbles.chunks_exact(2);
        for c in &mut chunks {
            out.push(c[0] << 4 | c[1]);
        }
        // Odd trailing nibble is treated as the high nibble of a final byte.
        if let [last] = chunks.remainder() {
            out.push(last << 4);
        }
        Some(Token::Str(out))
    }

    fn read_keyword(&mut self) -> Option<Token> {
        let start = self.pos;
        while let Some(&b) = self.data.get(self.pos) {
            if is_whitespace(b) || is_delimiter(b) {
                break;
            }
            self.pos += 1;
        }
        if self.pos == start {
            // Not a valid keyword char; skip it to make progress.
            self.pos += 1;
            return self.next_token();
        }
        Some(Token::Keyword(self.data[start..self.pos].to_vec()))
    }
}

/// PDF whitespace (spec 7.2.3).
pub fn is_whitespace(b: u8) -> bool {
    matches!(b, 0x00 | 0x09 | 0x0A | 0x0C | 0x0D | 0x20)
}

/// PDF delimiters (spec 7.2.3).
pub fn is_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Parse a PDF real, tolerating malformed forms like "." or "1.2.3".
fn parse_real(text: &str) -> f64 {
    // Keep sign, digits and the first dot only.
    let mut seen_dot = false;
    let mut cleaned = String::new();
    for (i, c) in text.chars().enumerate() {
        match c {
            '+' | '-' if i == 0 => cleaned.push(c),
            '0'..='9' => cleaned.push(c),
            '.' if !seen_dot => {
                seen_dot = true;
                cleaned.push('.');
            }
            _ => {}
        }
    }
    cleaned.parse::<f64>().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(s: &[u8]) -> Vec<Token> {
        let mut lex = Lexer::new(s);
        std::iter::from_fn(|| lex.next_token()).collect()
    }

    #[test]
    fn numbers_and_keywords() {
        assert_eq!(
            tokens(b"1 0 obj 3.5 -42 true null"),
            vec![
                Token::Integer(1),
                Token::Integer(0),
                Token::Keyword(b"obj".to_vec()),
                Token::Real(3.5),
                Token::Integer(-42),
                Token::Keyword(b"true".to_vec()),
                Token::Keyword(b"null".to_vec()),
            ]
        );
    }

    #[test]
    fn dict_vs_hex_string() {
        assert_eq!(
            tokens(b"<< /A <4869> >>"),
            vec![
                Token::DictOpen,
                Token::Name(b"A".to_vec()),
                Token::Str(b"Hi".to_vec()),
                Token::DictClose,
            ]
        );
    }

    #[test]
    fn literal_string_escapes() {
        assert_eq!(
            tokens(b"(a\\(b\\)c\\\\d\\n)"),
            vec![Token::Str(b"a(b)c\\d\n".to_vec())]
        );
        assert_eq!(tokens(b"(\\101)"), vec![Token::Str(b"A".to_vec())]); // octal
    }

    #[test]
    fn name_hex_escape() {
        assert_eq!(tokens(b"/A#20B"), vec![Token::Name(b"A B".to_vec())]);
    }

    #[test]
    fn comments_skipped() {
        assert_eq!(
            tokens(b"1 % comment\n 2"),
            vec![Token::Integer(1), Token::Integer(2)]
        );
    }

    #[test]
    fn arrays() {
        assert_eq!(
            tokens(b"[1 2 R]"),
            vec![
                Token::ArrayOpen,
                Token::Integer(1),
                Token::Integer(2),
                Token::Keyword(b"R".to_vec()),
                Token::ArrayClose,
            ]
        );
    }
}

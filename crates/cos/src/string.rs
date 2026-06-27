//! PDF string objects, in both literal `(...)` and hexadecimal `<...>` form.

/// How a [`PdfString`] should be written on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringSyntax {
    /// `(...)` form with backslash escapes.
    Literal,
    /// `<...>` hexadecimal form.
    Hex,
}

/// A PDF string: an arbitrary byte sequence plus a preferred encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfString {
    bytes: Vec<u8>,
    syntax: StringSyntax,
}

impl PdfString {
    /// A literal `(...)` string.
    pub fn literal(bytes: impl Into<Vec<u8>>) -> Self {
        PdfString {
            bytes: bytes.into(),
            syntax: StringSyntax::Literal,
        }
    }

    /// A PDF **text string** (§7.9.2.2). Pure-ASCII text becomes a literal;
    /// anything else is encoded as UTF-16BE with a leading `U+FEFF` BOM (hex
    /// form), so accented/Unicode content is interpreted correctly by viewers
    /// and assistive technology rather than as raw bytes.
    pub fn text(s: &str) -> Self {
        if s.is_ascii() {
            PdfString::literal(s.as_bytes().to_vec())
        } else {
            let mut bytes = vec![0xFE, 0xFF];
            for unit in s.encode_utf16() {
                bytes.extend_from_slice(&unit.to_be_bytes());
            }
            PdfString::hex(bytes)
        }
    }

    /// A hexadecimal `<...>` string.
    pub fn hex(bytes: impl Into<Vec<u8>>) -> Self {
        PdfString {
            bytes: bytes.into(),
            syntax: StringSyntax::Hex,
        }
    }

    /// The raw bytes carried by this string.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The chosen serialization syntax.
    pub fn syntax(&self) -> StringSyntax {
        self.syntax
    }

    /// Append the on-wire form (including delimiters) to `out`.
    pub fn write_to(&self, out: &mut Vec<u8>) {
        match self.syntax {
            StringSyntax::Literal => self.write_literal(out),
            StringSyntax::Hex => self.write_hex(out),
        }
    }

    fn write_literal(&self, out: &mut Vec<u8>) {
        out.push(b'(');
        for &b in &self.bytes {
            match b {
                b'(' => out.extend_from_slice(b"\\("),
                b')' => out.extend_from_slice(b"\\)"),
                b'\\' => out.extend_from_slice(b"\\\\"),
                b'\n' => out.extend_from_slice(b"\\n"),
                b'\r' => out.extend_from_slice(b"\\r"),
                b'\t' => out.extend_from_slice(b"\\t"),
                0x08 => out.extend_from_slice(b"\\b"),
                0x0C => out.extend_from_slice(b"\\f"),
                // Printable ASCII passes through verbatim.
                0x20..=0x7E => out.push(b),
                // Everything else as a 3-digit octal escape.
                _ => {
                    out.push(b'\\');
                    out.push(b'0' + ((b >> 6) & 0x07));
                    out.push(b'0' + ((b >> 3) & 0x07));
                    out.push(b'0' + (b & 0x07));
                }
            }
        }
        out.push(b')');
    }

    fn write_hex(&self, out: &mut Vec<u8>) {
        out.push(b'<');
        for &b in &self.bytes {
            out.push(hex_digit(b >> 4));
            out.push(hex_digit(b & 0x0F));
        }
        out.push(b'>');
    }
}

fn hex_digit(nibble: u8) -> u8 {
    match nibble {
        0..=9 => b'0' + nibble,
        _ => b'A' + (nibble - 10),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(s: &PdfString) -> Vec<u8> {
        let mut v = Vec::new();
        s.write_to(&mut v);
        v
    }

    #[test]
    fn literal_simple() {
        assert_eq!(wire(&PdfString::literal("Hello")), b"(Hello)");
    }

    #[test]
    fn literal_escapes_parens_and_backslash() {
        assert_eq!(wire(&PdfString::literal("a(b)c\\d")), b"(a\\(b\\)c\\\\d)");
    }

    #[test]
    fn literal_nested_parens() {
        assert_eq!(wire(&PdfString::literal("((x))")), b"(\\(\\(x\\)\\))");
    }

    #[test]
    fn literal_octal_for_non_ascii() {
        // byte 0x80 -> \200
        assert_eq!(wire(&PdfString::literal(vec![0x80])), b"(\\200)");
    }

    #[test]
    fn hex_form() {
        assert_eq!(wire(&PdfString::hex(vec![0xDE, 0xAD])), b"<DEAD>");
    }
}

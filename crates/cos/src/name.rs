//! PDF name objects (`/Type`, `/Pages`, ...).

/// A PDF name object. Stored as the raw (decoded) name without the leading
/// slash; serialization re-applies `#XX` escaping for delimiters and
/// non-regular characters (PDF spec 7.3.5).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Name(pub String);

impl Name {
    /// Create a name from any string-like value.
    pub fn new(s: impl Into<String>) -> Self {
        Name(s.into())
    }

    /// The decoded name (no leading slash, no escaping).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Append the escaped on-wire form (including the leading `/`) to `out`.
    pub fn write_to(&self, out: &mut Vec<u8>) {
        out.push(b'/');
        for &b in self.0.as_bytes() {
            if is_regular_name_byte(b) {
                out.push(b);
            } else {
                out.push(b'#');
                out.push(hex_digit(b >> 4));
                out.push(hex_digit(b & 0x0F));
            }
        }
    }
}

impl From<&str> for Name {
    fn from(s: &str) -> Self {
        Name(s.to_owned())
    }
}

impl From<String> for Name {
    fn from(s: String) -> Self {
        Name(s)
    }
}

/// Regular characters: printable ASCII excluding whitespace, delimiters and
/// `#` itself (which introduces an escape).
fn is_regular_name_byte(b: u8) -> bool {
    if !(0x21..=0x7E).contains(&b) {
        return false;
    }
    !matches!(
        b,
        b'#' | b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
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

    fn wire(n: &Name) -> String {
        let mut v = Vec::new();
        n.write_to(&mut v);
        String::from_utf8(v).unwrap()
    }

    #[test]
    fn plain_name() {
        assert_eq!(wire(&Name::new("Type")), "/Type");
    }

    #[test]
    fn escapes_delimiters_and_space() {
        assert_eq!(wire(&Name::new("A B")), "/A#20B");
        assert_eq!(wire(&Name::new("a#b")), "/a#23b");
        assert_eq!(wire(&Name::new("Pa(ren")), "/Pa#28ren");
    }

    #[test]
    fn escapes_non_ascii() {
        // 'é' is 0xC3 0xA9 in UTF-8.
        assert_eq!(wire(&Name::new("é")), "/#C3#A9");
    }
}

// Included by font.rs. A compact Adobe Glyph List subset (printable ASCII +
// common Latin-1 names) and the CP1252/WinAnsi 0x80..=0x9F special range.

/// WinAnsi (CP1252) specials in the 0x80..=0x9F band that differ from Latin-1.
const WINANSI_HIGH: &[(u8, u32)] = &[
    (0x80, 0x20AC), // Euro
    (0x82, 0x201A), // quotesinglbase
    (0x83, 0x0192), // florin
    (0x84, 0x201E), // quotedblbase
    (0x85, 0x2026), // ellipsis
    (0x86, 0x2020), // dagger
    (0x87, 0x2021), // daggerdbl
    (0x88, 0x02C6), // circumflex
    (0x89, 0x2030), // perthousand
    (0x8A, 0x0160), // Scaron
    (0x8B, 0x2039), // guilsinglleft
    (0x8C, 0x0152), // OE
    (0x8E, 0x017D), // Zcaron
    (0x91, 0x2018), // quoteleft
    (0x92, 0x2019), // quoteright
    (0x93, 0x201C), // quotedblleft
    (0x94, 0x201D), // quotedblright
    (0x95, 0x2022), // bullet
    (0x96, 0x2013), // endash
    (0x97, 0x2014), // emdash
    (0x98, 0x02DC), // tilde
    (0x99, 0x2122), // trademark
    (0x9A, 0x0161), // scaron
    (0x9B, 0x203A), // guilsinglright
    (0x9C, 0x0153), // oe
    (0x9E, 0x017E), // zcaron
    (0x9F, 0x0178), // Ydieresis
];

/// Map an Adobe glyph name to a Unicode scalar (common subset).
fn agl_lookup(name: &str) -> Option<char> {
    let v: u32 = match name {
        "space" => 0x20,
        "exclam" => 0x21,
        "quotedbl" => 0x22,
        "numbersign" => 0x23,
        "dollar" => 0x24,
        "percent" => 0x25,
        "ampersand" => 0x26,
        "quotesingle" => 0x27,
        "parenleft" => 0x28,
        "parenright" => 0x29,
        "asterisk" => 0x2A,
        "plus" => 0x2B,
        "comma" => 0x2C,
        "hyphen" => 0x2D,
        "period" => 0x2E,
        "slash" => 0x2F,
        "zero" => 0x30,
        "one" => 0x31,
        "two" => 0x32,
        "three" => 0x33,
        "four" => 0x34,
        "five" => 0x35,
        "six" => 0x36,
        "seven" => 0x37,
        "eight" => 0x38,
        "nine" => 0x39,
        "colon" => 0x3A,
        "semicolon" => 0x3B,
        "less" => 0x3C,
        "equal" => 0x3D,
        "greater" => 0x3E,
        "question" => 0x3F,
        "at" => 0x40,
        "bracketleft" => 0x5B,
        "backslash" => 0x5C,
        "bracketright" => 0x5D,
        "asciicircum" => 0x5E,
        "underscore" => 0x5F,
        "grave" => 0x60,
        "braceleft" => 0x7B,
        "bar" => 0x7C,
        "braceright" => 0x7D,
        "asciitilde" => 0x7E,
        "bullet" => 0x2022,
        "ellipsis" => 0x2026,
        "endash" => 0x2013,
        "emdash" => 0x2014,
        "quoteleft" => 0x2018,
        "quoteright" => 0x2019,
        "quotedblleft" => 0x201C,
        "quotedblright" => 0x201D,
        "quotesinglbase" => 0x201A,
        "quotedblbase" => 0x201E,
        "dagger" => 0x2020,
        "daggerdbl" => 0x2021,
        "perthousand" => 0x2030,
        "trademark" => 0x2122,
        "Euro" => 0x20AC,
        "florin" => 0x0192,
        "fi" => 0xFB01,
        "fl" => 0xFB02,
        "degree" => 0xB0,
        "plusminus" => 0xB1,
        "multiply" => 0xD7,
        "divide" => 0xF7,
        "copyright" => 0xA9,
        "registered" => 0xAE,
        "paragraph" => 0xB6,
        "section" => 0xA7,
        "sterling" => 0xA3,
        "yen" => 0xA5,
        "cent" => 0xA2,
        "currency" => 0xA4,
        "nbspace" | "nonbreakingspace" => 0xA0,
        _ => {
            // Accented Latin letters: "<Letter><accent>" forms like "eacute".
            return agl_latin(name);
        }
    };
    char::from_u32(v)
}

/// A handful of common accented-letter names.
fn agl_latin(name: &str) -> Option<char> {
    let v: u32 = match name {
        "Agrave" => 0xC0,
        "Aacute" => 0xC1,
        "Acircumflex" => 0xC2,
        "Atilde" => 0xC3,
        "Adieresis" => 0xC4,
        "Aring" => 0xC5,
        "AE" => 0xC6,
        "Ccedilla" => 0xC7,
        "Egrave" => 0xC8,
        "Eacute" => 0xC9,
        "Ecircumflex" => 0xCA,
        "Edieresis" => 0xCB,
        "Igrave" => 0xCC,
        "Iacute" => 0xCD,
        "Icircumflex" => 0xCE,
        "Idieresis" => 0xCF,
        "Ntilde" => 0xD1,
        "Ograve" => 0xD2,
        "Oacute" => 0xD3,
        "Ocircumflex" => 0xD4,
        "Otilde" => 0xD5,
        "Odieresis" => 0xD6,
        "Oslash" => 0xD8,
        "Ugrave" => 0xD9,
        "Uacute" => 0xDA,
        "Ucircumflex" => 0xDB,
        "Udieresis" => 0xDC,
        "Yacute" => 0xDD,
        "germandbls" => 0xDF,
        "agrave" => 0xE0,
        "aacute" => 0xE1,
        "acircumflex" => 0xE2,
        "atilde" => 0xE3,
        "adieresis" => 0xE4,
        "aring" => 0xE5,
        "ae" => 0xE6,
        "ccedilla" => 0xE7,
        "egrave" => 0xE8,
        "eacute" => 0xE9,
        "ecircumflex" => 0xEA,
        "edieresis" => 0xEB,
        "igrave" => 0xEC,
        "iacute" => 0xED,
        "icircumflex" => 0xEE,
        "idieresis" => 0xEF,
        "ntilde" => 0xF1,
        "ograve" => 0xF2,
        "oacute" => 0xF3,
        "ocircumflex" => 0xF4,
        "otilde" => 0xF5,
        "odieresis" => 0xF6,
        "oslash" => 0xF8,
        "ugrave" => 0xF9,
        "uacute" => 0xFA,
        "ucircumflex" => 0xFB,
        "udieresis" => 0xFC,
        "yacute" => 0xFD,
        "ydieresis" => 0xFF,
        "OE" => 0x0152,
        "oe" => 0x0153,
        "Scaron" => 0x0160,
        "scaron" => 0x0161,
        "Zcaron" => 0x017D,
        "zcaron" => 0x017E,
        _ => return None,
    };
    char::from_u32(v)
}

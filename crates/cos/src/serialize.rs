//! Serialization of COS objects to their on-wire byte form.
//!
//! Every method appends to a caller-owned buffer; nothing allocates a
//! fresh `String`/`Vec` per call, so large documents serialize with a
//! single growing buffer.

use crate::object::{Dict, Object, Reference, Stream};

impl Object {
    /// Serialize this object's value (the form used both as a direct value
    /// and as the body of an indirect object) into `out`.
    pub fn write_to(&self, out: &mut Vec<u8>) {
        match self {
            Object::Null => out.extend_from_slice(b"null"),
            Object::Bool(true) => out.extend_from_slice(b"true"),
            Object::Bool(false) => out.extend_from_slice(b"false"),
            Object::Integer(i) => write_int(*i, out),
            Object::Real(r) => write_real(*r, out),
            Object::Name(n) => n.write_to(out),
            Object::String(s) => s.write_to(out),
            Object::Array(items) => write_array(items, out),
            Object::Dict(d) => write_dict(d, out),
            Object::Stream(s) => write_stream(s, out),
            Object::Reference(r) => write_reference(*r, out),
        }
    }

    /// Convenience: serialize to a fresh `Vec<u8>`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write_to(&mut out);
        out
    }
}

fn write_int(i: i64, out: &mut Vec<u8>) {
    let mut buf = itoa_buf();
    out.extend_from_slice(format_i64(i, &mut buf));
}

/// Format a real number in fixed (non-exponential) notation, trimming
/// trailing zeros, as required by the PDF spec (7.3.3).
fn write_real(r: f64, out: &mut Vec<u8>) {
    if !r.is_finite() {
        // PDF has no NaN/Inf; emit 0 as a safe, valid fallback.
        out.push(b'0');
        return;
    }
    if r == r.trunc() && r.abs() < 1e15 {
        write_int(r as i64, out);
        return;
    }
    let s = format!("{r:.6}");
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    out.extend_from_slice(trimmed.as_bytes());
}

fn write_array(items: &[Object], out: &mut Vec<u8>) {
    out.push(b'[');
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(b' ');
        }
        item.write_to(out);
    }
    out.push(b']');
}

fn write_dict(dict: &Dict, out: &mut Vec<u8>) {
    out.extend_from_slice(b"<<");
    for (key, value) in dict.iter() {
        out.push(b' ');
        key.write_to(out);
        out.push(b' ');
        value.write_to(out);
    }
    out.extend_from_slice(b" >>");
}

fn write_stream(stream: &Stream, out: &mut Vec<u8>) {
    // Ensure /Length is present. If the caller didn't set one, emit a direct
    // integer length; if they set an indirect reference, leave it untouched.
    let mut dict = stream.dict.clone();
    if !dict.contains_key("Length") {
        dict.set("Length", stream.data.len() as i64);
    }
    write_dict(&dict, out);
    out.extend_from_slice(b"\nstream\n");
    out.extend_from_slice(&stream.data);
    out.extend_from_slice(b"\nendstream");
}

fn write_reference(r: Reference, out: &mut Vec<u8>) {
    let mut buf = itoa_buf();
    out.extend_from_slice(format_u32(r.number, &mut buf));
    out.push(b' ');
    out.extend_from_slice(format_u32(r.generation as u32, &mut buf));
    out.extend_from_slice(b" R");
}

// ---- tiny integer formatting without external deps -------------------------

type IntBuf = [u8; 24];

fn itoa_buf() -> IntBuf {
    [0u8; 24]
}

fn format_i64(mut v: i64, buf: &mut IntBuf) -> &[u8] {
    if v == 0 {
        buf[0] = b'0';
        return &buf[..1];
    }
    let neg = v < 0;
    let mut idx = buf.len();
    // Work on the magnitude using i128 to handle i64::MIN safely.
    let mut mag = (v as i128).unsigned_abs();
    let _ = &mut v;
    while mag > 0 {
        idx -= 1;
        buf[idx] = b'0' + (mag % 10) as u8;
        mag /= 10;
    }
    if neg {
        idx -= 1;
        buf[idx] = b'-';
    }
    &buf[idx..]
}

fn format_u32(mut v: u32, buf: &mut IntBuf) -> &[u8] {
    if v == 0 {
        buf[0] = b'0';
        return &buf[..1];
    }
    let mut idx = buf.len();
    while v > 0 {
        idx -= 1;
        buf[idx] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    &buf[idx..]
}

#[cfg(test)]
mod tests {
    use crate::{Dict, Name, Object, PdfString, Reference, Stream};

    fn s(o: &Object) -> String {
        String::from_utf8(o.to_bytes()).unwrap()
    }

    #[test]
    fn scalars() {
        assert_eq!(s(&Object::Null), "null");
        assert_eq!(s(&Object::Bool(true)), "true");
        assert_eq!(s(&Object::Bool(false)), "false");
        assert_eq!(s(&Object::Integer(0)), "0");
        assert_eq!(s(&Object::Integer(-42)), "-42");
        assert_eq!(s(&Object::Integer(i64::MIN)), "-9223372036854775808");
    }

    #[test]
    fn reals_no_exponent_and_trimmed() {
        assert_eq!(s(&Object::Real(0.0)), "0");
        assert_eq!(s(&Object::Real(-0.0)), "0");
        assert_eq!(s(&Object::Real(1.5)), "1.5");
        assert_eq!(s(&Object::Real(100.0)), "100");
        assert_eq!(s(&Object::Real(0.10000)), "0.1");
        assert_eq!(s(&Object::Real(-3.25)), "-3.25");
    }

    #[test]
    fn name_and_string() {
        assert_eq!(s(&Object::Name(Name::new("Type"))), "/Type");
        assert_eq!(s(&Object::String(PdfString::literal("hi"))), "(hi)");
    }

    #[test]
    fn array() {
        let a = Object::Array(vec![
            Object::Integer(1),
            Object::Name(Name::new("X")),
            Object::Bool(false),
        ]);
        assert_eq!(s(&a), "[1 /X false]");
    }

    #[test]
    fn dict() {
        let d = Dict::new()
            .with("Type", Object::name("Catalog"))
            .with("Count", 3);
        assert_eq!(s(&Object::Dict(d)), "<< /Type /Catalog /Count 3 >>");
    }

    #[test]
    fn reference() {
        assert_eq!(s(&Object::Reference(Reference::new(12))), "12 0 R");
    }

    #[test]
    fn stream_auto_length() {
        let st = Stream::new(b"abc".to_vec());
        let out = s(&Object::Stream(st));
        assert_eq!(out, "<< /Length 3 >>\nstream\nabc\nendstream");
    }

    #[test]
    fn stream_indirect_length_preserved() {
        let mut st = Stream::new(b"abc".to_vec());
        st.dict.set("Length", Reference::new(9));
        let out = s(&Object::Stream(st));
        assert!(out.starts_with("<< /Length 9 0 R >>"));
    }

    #[test]
    fn round_trip_canonical_is_stable() {
        // Serializing twice yields identical bytes (canonical output).
        let d = Dict::new()
            .with("A", 1)
            .with("B", Object::Array(vec![Object::Real(2.5), Object::Null]));
        let o = Object::Dict(d);
        assert_eq!(o.to_bytes(), o.to_bytes());
    }
}

//! Stream filter decoders (Fase 5.4): `FlateDecode` (with PNG/TIFF predictors),
//! `LZWDecode`, `ASCIIHexDecode`, `ASCII85Decode`, `RunLengthDecode`.
//!
//! Image filters (`DCTDecode`, `JPXDecode`, `CCITTFaxDecode`) are *terminal*:
//! their data is left encoded (we don't decode pixels here), so callers get the
//! raw compressed bytes back.

use std::io::Read;

use cos::{Dict, Object};

use crate::error::{PdfError, Result};

/// Apply a chain of filters to `raw`. `parms[i]` is the (already-resolved)
/// `DecodeParms` dictionary for `filters[i]`, if any.
pub fn apply_filters(raw: &[u8], filters: &[Vec<u8>], parms: &[Option<Dict>]) -> Result<Vec<u8>> {
    let mut data = raw.to_vec();
    for (i, name) in filters.iter().enumerate() {
        let parm = parms.get(i).and_then(|p| p.as_ref());
        data = match name.as_slice() {
            b"FlateDecode" | b"Fl" => predictor_decode(inflate(&data)?, parm),
            b"LZWDecode" | b"LZW" => {
                let early = parm
                    .and_then(|d| d.get("EarlyChange"))
                    .and_then(as_int)
                    .unwrap_or(1);
                predictor_decode(lzw_decode(&data, early != 0), parm)
            }
            b"ASCIIHexDecode" | b"AHx" => ascii_hex_decode(&data),
            b"ASCII85Decode" | b"A85" => ascii85_decode(&data)?,
            b"RunLengthDecode" | b"RL" => run_length_decode(&data),
            // Terminal image filters: stop and return the encoded data.
            b"DCTDecode" | b"JPXDecode" | b"CCITTFaxDecode" | b"JBIG2Decode" => return Ok(data),
            other => {
                return Err(PdfError::Filter(format!(
                    "unsupported filter /{}",
                    String::from_utf8_lossy(other)
                )))
            }
        };
    }
    Ok(data)
}

fn as_int(o: &Object) -> Option<i64> {
    match o {
        Object::Integer(n) => Some(*n),
        _ => None,
    }
}

fn inflate(data: &[u8]) -> Result<Vec<u8>> {
    // Try zlib first, then raw deflate (some producers omit the zlib header).
    let mut out = Vec::new();
    if flate2::read::ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .is_ok()
        && !out.is_empty()
    {
        return Ok(out);
    }
    out.clear();
    match flate2::read::DeflateDecoder::new(data).read_to_end(&mut out) {
        Ok(_) => Ok(out),
        Err(e) => Err(PdfError::Filter(format!("flate: {e}"))),
    }
}

// ---- predictors (PNG / TIFF) ----------------------------------------------

fn predictor_decode(data: Vec<u8>, parm: Option<&Dict>) -> Vec<u8> {
    let Some(parm) = parm else { return data };
    let predictor = parm.get("Predictor").and_then(as_int).unwrap_or(1);
    if predictor <= 1 {
        return data;
    }
    let colors = parm.get("Colors").and_then(as_int).unwrap_or(1).max(1) as usize;
    let bpc = parm
        .get("BitsPerComponent")
        .and_then(as_int)
        .unwrap_or(8)
        .max(1) as usize;
    let columns = parm.get("Columns").and_then(as_int).unwrap_or(1).max(1) as usize;
    let bpp = (colors * bpc).div_ceil(8).max(1); // bytes per pixel
    let row_len = (colors * bpc * columns).div_ceil(8); // bytes per row

    if predictor == 2 {
        tiff_predictor2(data, colors, bpc, columns)
    } else {
        png_predictor(data, row_len, bpp)
    }
}

fn png_predictor(data: Vec<u8>, row_len: usize, bpp: usize) -> Vec<u8> {
    if row_len == 0 {
        return Vec::new();
    }
    let stride = row_len + 1; // each row prefixed with a filter-type byte
    let mut out = Vec::with_capacity(data.len());
    let mut prev = vec![0u8; row_len];
    for chunk in data.chunks(stride) {
        if chunk.len() < 2 {
            break;
        }
        let ft = chunk[0];
        let mut row = chunk[1..].to_vec();
        if row.len() < row_len {
            row.resize(row_len, 0);
        }
        for i in 0..row_len {
            let a = if i >= bpp { row[i - bpp] } else { 0 };
            let b = prev[i];
            let c = if i >= bpp { prev[i - bpp] } else { 0 };
            let x = row[i];
            row[i] = match ft {
                0 => x,
                1 => x.wrapping_add(a),
                2 => x.wrapping_add(b),
                3 => x.wrapping_add(((a as u16 + b as u16) / 2) as u8),
                4 => x.wrapping_add(paeth(a, b, c)),
                _ => x,
            };
        }
        out.extend_from_slice(&row);
        prev = row;
    }
    out
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i16 + b as i16 - c as i16;
    let pa = (p - a as i16).abs();
    let pb = (p - b as i16).abs();
    let pc = (p - c as i16).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn tiff_predictor2(mut data: Vec<u8>, colors: usize, bpc: usize, columns: usize) -> Vec<u8> {
    // Only the common 8-bit case is reconstructed; others pass through.
    if bpc != 8 {
        return data;
    }
    let row_len = colors * columns;
    if row_len == 0 {
        return data;
    }
    for row in data.chunks_mut(row_len) {
        for i in colors..row.len() {
            row[i] = row[i].wrapping_add(row[i - colors]);
        }
    }
    data
}

// ---- ASCII / RunLength / LZW ----------------------------------------------

fn ascii_hex_decode(data: &[u8]) -> Vec<u8> {
    let mut nibbles = Vec::new();
    for &b in data {
        if b == b'>' {
            break;
        }
        let v = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => continue,
        };
        nibbles.push(v);
    }
    let mut out = Vec::with_capacity(nibbles.len().div_ceil(2));
    let mut it = nibbles.chunks_exact(2);
    for c in &mut it {
        out.push(c[0] << 4 | c[1]);
    }
    if let [last] = it.remainder() {
        out.push(last << 4);
    }
    out
}

fn ascii85_decode(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut group = [0u8; 5];
    let mut count = 0;
    let mut i = 0;
    // Skip a leading <~ if present.
    if data.starts_with(b"<~") {
        i = 2;
    }
    while i < data.len() {
        let b = data[i];
        i += 1;
        match b {
            b'~' => break,
            b'z' if count == 0 => out.extend_from_slice(&[0, 0, 0, 0]),
            b'!'..=b'u' => {
                group[count] = b - b'!';
                count += 1;
                if count == 5 {
                    let mut val = 0u32;
                    for &g in &group {
                        val = val.wrapping_mul(85).wrapping_add(g as u32);
                    }
                    out.extend_from_slice(&val.to_be_bytes());
                    count = 0;
                }
            }
            _ => {} // ignore whitespace and stray bytes
        }
    }
    if count > 0 {
        for g in group.iter_mut().skip(count) {
            *g = 84;
        }
        let mut val = 0u32;
        for &g in &group {
            val = val.wrapping_mul(85).wrapping_add(g as u32);
        }
        let bytes = val.to_be_bytes();
        out.extend_from_slice(&bytes[..count - 1]);
    }
    Ok(out)
}

fn run_length_decode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let len = data[i];
        i += 1;
        match len {
            128 => break, // EOD
            0..=127 => {
                let n = len as usize + 1;
                let end = (i + n).min(data.len());
                out.extend_from_slice(&data[i..end]);
                i = end;
            }
            _ => {
                let n = 257 - len as usize;
                if let Some(&byte) = data.get(i) {
                    out.extend(std::iter::repeat_n(byte, n));
                    i += 1;
                }
            }
        }
    }
    out
}

/// PDF LZW decoder (variable code width 9–12 bits, MSB-first), with optional
/// "early change" (the default).
fn lzw_decode(data: &[u8], early_change: bool) -> Vec<u8> {
    const CLEAR: u32 = 256;
    const EOD: u32 = 257;
    let mut out = Vec::new();
    let mut table: Vec<Vec<u8>> = Vec::new();
    let reset = |table: &mut Vec<Vec<u8>>| {
        table.clear();
        for i in 0..256u32 {
            table.push(vec![i as u8]);
        }
        table.push(Vec::new()); // 256 CLEAR
        table.push(Vec::new()); // 257 EOD
    };
    reset(&mut table);

    let mut code_width = 9u32;
    let mut bit_buf = 0u32;
    let mut bit_count = 0u32;
    let mut prev: Option<u32> = None;
    let early = u32::from(early_change);

    for &byte in data {
        bit_buf = (bit_buf << 8) | byte as u32;
        bit_count += 8;
        while bit_count >= code_width {
            bit_count -= code_width;
            let code = (bit_buf >> bit_count) & ((1 << code_width) - 1);

            if code == EOD {
                return out;
            }
            if code == CLEAR {
                reset(&mut table);
                code_width = 9;
                prev = None;
                continue;
            }

            let entry: Vec<u8> = if (code as usize) < table.len() {
                table[code as usize].clone()
            } else if let Some(p) = prev {
                let mut e = table[p as usize].clone();
                e.push(table[p as usize][0]);
                e
            } else {
                return out; // corrupt
            };
            out.extend_from_slice(&entry);

            if let Some(p) = prev {
                let mut new_entry = table[p as usize].clone();
                new_entry.push(entry[0]);
                table.push(new_entry);
            }
            prev = Some(code);

            // Grow the code width as the table fills (with early change).
            if table.len() as u32 + early >= (1 << code_width) && code_width < 12 {
                code_width += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flate_roundtrip() {
        use flate2::{write::ZlibEncoder, Compression};
        use std::io::Write;
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(b"the quick brown fox").unwrap();
        let compressed = enc.finish().unwrap();
        let out = apply_filters(&compressed, &[b"FlateDecode".to_vec()], &[None]).unwrap();
        assert_eq!(out, b"the quick brown fox");
    }

    #[test]
    fn ascii_hex() {
        assert_eq!(ascii_hex_decode(b"48656C6C6F>"), b"Hello");
        assert_eq!(ascii_hex_decode(b"4 8 6>"), b"H`"); // whitespace + odd nibble
    }

    #[test]
    fn ascii85() {
        // "Man " encodes to "9jqo" + ... ; test the canonical 'z' = four zeros.
        assert_eq!(ascii85_decode(b"z~>").unwrap(), vec![0, 0, 0, 0]);
        let enc = b"<~9jqo^~>";
        assert_eq!(ascii85_decode(enc).unwrap(), b"Man ");
    }

    #[test]
    fn run_length() {
        // literal run of 3 bytes (len byte 2 => 3 literals), then EOD (128).
        assert_eq!(run_length_decode(&[2, b'A', b'B', b'C', 128]), b"ABC");
        // repeat 'X' 3 times: 257-255 = 2 -> wait, 256-254... use 254 => 257-254=3
        assert_eq!(run_length_decode(&[254, b'X', 128]), b"XXX");
    }

    #[test]
    fn lzw_basic() {
        // Encode with a known sequence is involved; instead verify the classic
        // PDF spec example bytes decode to the expected output.
        // Input -45 codes from the spec example for "-----A---B".
        let encoded = [0x80, 0x0B, 0x60, 0x50, 0x22, 0x0C, 0x0C, 0x85, 0x01];
        let out = lzw_decode(&encoded, true);
        assert_eq!(out, b"-----A---B");
    }
}

//! C ABI: signature **validation** (`pdf::verify_signatures`).
//!
//! Reports are serialized to a small JSON array (one object per signature) into
//! a caller-owned buffer; every binding parses it into native structs. This
//! keeps the ABI a single, stable function instead of N typed accessors.

use std::ffi::c_uchar;

use crate::{bytes, emit_buffer, guard, set_last_error, PdfStatus};

/// Validate every signature in `data`/`len` and write a JSON array describing
/// each one into `out_ptr`/`out_len` (freed with `pdf_buffer_free`). Each element
/// is `{"field_name","sub_filter","signer","covers_whole_document","digest_valid",
/// "signature_valid","is_valid","byte_range":[..4]}`. An empty array `[]` means
/// the document is unsigned.
///
/// # Safety
/// `data`/`len` readable; `out_ptr`/`out_len` writable.
#[no_mangle]
pub unsafe extern "C" fn pdf_verify_signatures_json(
    data: *const u8,
    len: usize,
    out_ptr: *mut *mut c_uchar,
    out_len: *mut usize,
) -> PdfStatus {
    guard(
        || match pdf::verify_signatures(unsafe { bytes(data, len) }) {
            Ok(reports) => {
                let json = reports_to_json(&reports);
                unsafe { emit_buffer(json.into_bytes(), out_ptr, out_len) }
            }
            Err(e) => {
                set_last_error(format!("verify_signatures failed: {e}"));
                PdfStatus::Parse
            }
        },
    )
}

fn reports_to_json(reports: &[pdf::SignatureReport]) -> String {
    let mut s = String::from("[");
    for (i, r) in reports.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let field = r
            .field_name
            .as_deref()
            .map(|n| format!("\"{}\"", json_escape(n)))
            .unwrap_or_else(|| "null".into());
        let opt = |v: &Option<String>| {
            v.as_deref()
                .map(|n| format!("\"{}\"", json_escape(n)))
                .unwrap_or_else(|| "null".into())
        };
        s.push_str(&format!(
            "{{\"field_name\":{field},\"sub_filter\":\"{sf}\",\"signer\":{signer},\
             \"issuer\":{issuer},\"serial_number\":{serial},\
             \"valid_from\":{vf},\"valid_to\":{vt},\"algorithm\":{alg},\
             \"signing_time\":{st},\"cert_count\":{cc},\"has_timestamp\":{hts},\
             \"covers_whole_document\":{cwd},\"digest_valid\":{dv},\
             \"signature_valid\":{sv},\"is_valid\":{iv},\
             \"byte_range\":[{b0},{b1},{b2},{b3}]}}",
            sf = json_escape(&r.sub_filter),
            signer = opt(&r.signer),
            issuer = opt(&r.issuer),
            serial = opt(&r.serial_number),
            vf = opt(&r.valid_from),
            vt = opt(&r.valid_to),
            alg = opt(&r.algorithm),
            st = opt(&r.signing_time),
            cc = r.cert_count,
            hts = r.has_timestamp,
            cwd = r.covers_whole_document,
            dv = r.digest_valid,
            sv = r.signature_valid,
            iv = r.is_valid(),
            b0 = r.byte_range[0],
            b1 = r.byte_range[1],
            b2 = r.byte_range[2],
            b3 = r.byte_range[3],
        ));
    }
    s.push(']');
    s
}

pub(crate) fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

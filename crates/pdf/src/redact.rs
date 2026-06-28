//! True **redaction** (Tier 2, item 6): the content under a redaction rectangle
//! is *removed* from the page's content stream — not merely covered by a black
//! box — so the text/data is gone (not extractable, not selectable, not present
//! in the bytes).
//!
//! A small content-stream interpreter tracks the CTM (`q`/`Q`/`cm`) and the text
//! matrices (`Tm`/`Td`/`TD`/`T*`). Each text-show operator (`Tj`/`TJ`/`'`/`"`)
//! and each XObject paint (`Do`) whose origin falls inside a redaction rectangle
//! is dropped; everything else is re-emitted verbatim. The caller then paints
//! opaque black rectangles over the regions for the visible redaction marks.
//!
//! Inline images (`BI … ID … EI`) carry raw bytes the tokenizer can't safely
//! re-serialize, so a page containing one is left untouched here (the caller
//! still draws the black boxes) — reported via [`redact_content`] returning
//! `None`.

use parser::{Lexer, Token};

/// 3×2 affine matrix `[a b c d e f]` (PDF row-vector convention).
type Mat = [f64; 6];

const IDENTITY: Mat = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `A` then `B` — the matrix `C` with `p·C = (p·A)·B`.
fn mat_mul(a: Mat, b: Mat) -> Mat {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

/// Apply `m` to point `(x, y)`.
fn apply(m: Mat, x: f64, y: f64) -> (f64, f64) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

fn point_in_rects(x: f64, y: f64, rects: &[[f64; 4]]) -> bool {
    rects.iter().any(|r| {
        let (x0, x1) = (r[0].min(r[2]), r[0].max(r[2]));
        let (y0, y1) = (r[1].min(r[3]), r[1].max(r[3]));
        x >= x0 && x <= x1 && y >= y0 && y <= y1
    })
}

/// Filter `content`, removing show/paint operators inside `rects`. Returns the
/// rewritten content, or `None` if the stream can't be safely rewritten (inline
/// image present) and should be left as-is.
pub(crate) fn redact_content(content: &[u8], rects: &[[f64; 4]]) -> Option<Vec<u8>> {
    let mut lex = Lexer::new(content);
    let mut out: Vec<u8> = Vec::with_capacity(content.len());

    // Graphics + text state.
    let mut ctm_stack: Vec<Mat> = Vec::new();
    let mut ctm: Mat = IDENTITY;
    let mut tm: Mat = IDENTITY;
    let mut tlm: Mat = IDENTITY;
    let mut leading = 0.0f64;

    // Buffered operands + their serialized tokens for the pending operator.
    let mut nums: Vec<f64> = Vec::new();
    let mut pending: Vec<u8> = Vec::new();

    while let Some(tok) = lex.next_token() {
        match tok {
            Token::Integer(n) => {
                nums.push(n as f64);
                serialize_token(&Token::Integer(n), &mut pending);
            }
            Token::Real(r) => {
                nums.push(r);
                serialize_token(&Token::Real(r), &mut pending);
            }
            Token::Str(s) => serialize_token(&Token::Str(s), &mut pending),
            Token::Name(n) => serialize_token(&Token::Name(n), &mut pending),
            Token::ArrayOpen => serialize_token(&Token::ArrayOpen, &mut pending),
            Token::ArrayClose => serialize_token(&Token::ArrayClose, &mut pending),
            Token::DictOpen => serialize_token(&Token::DictOpen, &mut pending),
            Token::DictClose => serialize_token(&Token::DictClose, &mut pending),
            Token::Keyword(kw) => {
                // Inline images are not safely re-serializable: bail.
                if kw == b"BI" {
                    return None;
                }
                let mut drop = false;
                match kw.as_slice() {
                    b"q" => ctm_stack.push(ctm),
                    b"Q" => {
                        if let Some(m) = ctm_stack.pop() {
                            ctm = m;
                        }
                    }
                    b"cm" => {
                        if let Some(m) = last6(&nums) {
                            ctm = mat_mul(m, ctm);
                        }
                    }
                    b"BT" => {
                        tm = IDENTITY;
                        tlm = IDENTITY;
                    }
                    b"Tm" => {
                        if let Some(m) = last6(&nums) {
                            tm = m;
                            tlm = m;
                        }
                    }
                    b"Td" => {
                        if let Some((tx, ty)) = last2(&nums) {
                            tlm = mat_mul([1.0, 0.0, 0.0, 1.0, tx, ty], tlm);
                            tm = tlm;
                        }
                    }
                    b"TD" => {
                        if let Some((tx, ty)) = last2(&nums) {
                            leading = -ty;
                            tlm = mat_mul([1.0, 0.0, 0.0, 1.0, tx, ty], tlm);
                            tm = tlm;
                        }
                    }
                    b"TL" => {
                        if let Some(&l) = nums.last() {
                            leading = l;
                        }
                    }
                    b"T*" => {
                        tlm = mat_mul([1.0, 0.0, 0.0, 1.0, 0.0, -leading], tlm);
                        tm = tlm;
                    }
                    b"Tj" | b"TJ" | b"'" | b"\"" => {
                        let (x, y) = apply(mat_mul(tm, ctm), 0.0, 0.0);
                        drop = point_in_rects(x, y, rects);
                    }
                    b"Do" => {
                        // Drop an XObject paint whose unit-square center lands in
                        // a redaction rect.
                        let (x, y) = apply(ctm, 0.5, 0.5);
                        drop = point_in_rects(x, y, rects);
                    }
                    _ => {}
                }

                if drop {
                    // Preserve the implicit line advance of ' and " so following
                    // text keeps its position.
                    if kw == b"'" || kw == b"\"" {
                        tlm = mat_mul([1.0, 0.0, 0.0, 1.0, 0.0, -leading], tlm);
                        tm = tlm;
                        out.extend_from_slice(b"T*\n");
                    }
                } else {
                    out.extend_from_slice(&pending);
                    serialize_keyword(&kw, &mut out);
                }
                pending.clear();
                nums.clear();
            }
        }
    }
    Some(out)
}

fn last6(nums: &[f64]) -> Option<Mat> {
    let n = nums.len();
    (n >= 6).then(|| {
        let s = &nums[n - 6..];
        [s[0], s[1], s[2], s[3], s[4], s[5]]
    })
}

fn last2(nums: &[f64]) -> Option<(f64, f64)> {
    let n = nums.len();
    (n >= 2).then(|| (nums[n - 2], nums[n - 1]))
}

/// Re-serialize one operand/token, appending a trailing space.
fn serialize_token(tok: &Token, out: &mut Vec<u8>) {
    match tok {
        Token::Integer(n) => out.extend_from_slice(format!("{n} ").as_bytes()),
        Token::Real(r) => {
            out.extend_from_slice(format_real(*r).as_bytes());
            out.push(b' ');
        }
        Token::Str(s) => {
            // Hex form is always byte-safe (no escaping pitfalls).
            out.push(b'<');
            for &b in s {
                out.push(hex_digit(b >> 4));
                out.push(hex_digit(b & 0xF));
            }
            out.extend_from_slice(b"> ");
        }
        Token::Name(n) => {
            out.push(b'/');
            for &b in n {
                if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+') {
                    out.push(b);
                } else {
                    out.push(b'#');
                    out.push(hex_digit(b >> 4));
                    out.push(hex_digit(b & 0xF));
                }
            }
            out.push(b' ');
        }
        Token::ArrayOpen => out.extend_from_slice(b"["),
        Token::ArrayClose => out.extend_from_slice(b"] "),
        Token::DictOpen => out.extend_from_slice(b"<< "),
        Token::DictClose => out.extend_from_slice(b">> "),
        Token::Keyword(kw) => serialize_keyword(kw, out),
    }
}

fn serialize_keyword(kw: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(kw);
    out.push(b'\n');
}

fn hex_digit(v: u8) -> u8 {
    match v {
        0..=9 => b'0' + v,
        _ => b'A' + (v - 10),
    }
}

/// Format a real without exponent notation (matches the writer's determinism).
fn format_real(r: f64) -> String {
    if r == r.trunc() && r.abs() < 1e15 {
        format!("{}", r as i64)
    } else {
        let s = format!("{r:.6}");
        let s = s.trim_end_matches('0');
        s.trim_end_matches('.').to_string()
    }
}

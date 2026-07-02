//! True **redaction**: the content under a redaction rectangle is *removed*
//! from the page's content stream — not merely covered by a black box — so the
//! text/data is gone (not extractable, not selectable, not present in the
//! bytes).
//!
//! A content-stream interpreter tracks the full graphics + text state
//! (`q`/`Q`/`cm`, `BT`/`Tm`/`Td`/`TD`/`T*`, `Tf`/`Tc`/`Tw`/`Tz`/`TL`/`Ts`) and
//! measures every shown **glyph** with the page fonts' width tables
//! (FINDING-006 — an origin-only test let a run that merely *starts* outside
//! the rectangle survive intact). A glyph whose device-space box intersects a
//! redaction rectangle is removed and replaced by an equivalent `TJ`
//! displacement, so the surviving glyphs of the same run keep their exact
//! positions. XObject paints (`Do`) are dropped when their transformed unit
//! square **overlaps** a rectangle (conservative: a partially covered image is
//! removed entirely rather than left leaking data).
//!
//! Streams that cannot be safely rewritten **fail loudly** ([`RedactError`]) —
//! never silently falling back to paint-only, which would leave the data
//! extractable under the black box.

use std::collections::{BTreeMap, BTreeSet};

use parser::{Lexer, Token};

/// Why a page could not be redacted. Returned by
/// [`EditableDoc::redact`](crate::EditableDoc::redact); **nothing is removed
/// or drawn** when redaction fails, so a black box never masks still-present
/// data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedactError {
    /// The page content contains an inline image (`BI … ID … EI`), whose raw
    /// binary body cannot be safely re-serialized by the rewriter.
    InlineImage,
    /// A content stream could not be decoded (unsupported filter chain).
    Undecodable,
}

impl std::fmt::Display for RedactError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RedactError::InlineImage => write!(
                f,
                "page content has an inline image (BI/EI) that cannot be safely rewritten; \
                 redaction aborted (nothing was removed or drawn)"
            ),
            RedactError::Undecodable => write!(
                f,
                "page content stream could not be decoded; redaction aborted \
                 (nothing was removed or drawn)"
            ),
        }
    }
}

impl std::error::Error for RedactError {}

/// Width oracle for one page font, resolved by the caller from the page's
/// `/Font` resources.
pub(crate) struct RFont {
    /// Type0 (2-byte CID) vs simple (1-byte) codes.
    pub two_byte: bool,
    /// code/CID → advance width in **em units** (already /1000).
    pub widths: BTreeMap<u32, f64>,
    /// Fallback width (em) for codes missing from `widths`.
    pub default_width: f64,
}

/// The rewritten stream plus the XObject names whose paints were dropped or
/// kept (the caller prunes resources/objects for `dropped − used`).
pub(crate) struct RedactOutcome {
    pub content: Vec<u8>,
    pub used_xobjects: BTreeSet<Vec<u8>>,
    pub dropped_xobjects: BTreeSet<Vec<u8>>,
}

/// 3×2 affine matrix `[a b c d e f]` (PDF row-vector convention).
type Mat = [f64; 6];

const IDENTITY: Mat = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// Nominal glyph box vertical extent in em (matches `find.rs`).
const ASCENT: f64 = 0.8;
const DESCENT: f64 = 0.2;

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

/// Axis-aligned bounds of `corners`.
fn bounds(corners: &[(f64, f64)]) -> [f64; 4] {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for &(x, y) in corners {
        b[0] = b[0].min(x);
        b[1] = b[1].min(y);
        b[2] = b[2].max(x);
        b[3] = b[3].max(y);
    }
    b
}

/// Public (crate) overlap test for callers (annotation rects).
pub(crate) fn rect_overlaps(b: [f64; 4], rects: &[[f64; 4]]) -> bool {
    overlaps(b, rects)
}

/// Does box `b` (`[x0, y0, x1, y1]`) overlap any redaction rect?
fn overlaps(b: [f64; 4], rects: &[[f64; 4]]) -> bool {
    rects.iter().any(|r| {
        let (rx0, rx1) = (r[0].min(r[2]), r[0].max(r[2]));
        let (ry0, ry1) = (r[1].min(r[3]), r[1].max(r[3]));
        b[2] >= rx0 && b[0] <= rx1 && b[3] >= ry0 && b[1] <= ry1
    })
}

/// Full graphics + text state (the pieces that position glyphs).
#[derive(Clone)]
struct GState {
    ctm: Mat,
    font: Option<Vec<u8>>,
    size: f64,
    tc: f64,
    tw: f64,
    /// Horizontal scaling `Tz / 100`.
    th: f64,
    rise: f64,
}

impl GState {
    fn new() -> Self {
        GState {
            ctm: IDENTITY,
            font: None,
            size: 0.0,
            tc: 0.0,
            tw: 0.0,
            th: 1.0,
            rise: 0.0,
        }
    }
}

/// A parsed operand (only the structure the rewriter needs).
#[derive(Clone)]
enum ROp {
    Num(f64),
    Str(Vec<u8>),
    Name(Vec<u8>),
    Arr(Vec<ROp>),
    ArrMark,
}

/// One element of a text-show operand list.
enum ShowItem {
    Str(Vec<u8>),
    /// A `TJ` numeric adjustment (thousandths of text space).
    Adj(f64),
}

/// Filter `content`, removing shown glyphs and XObject paints that intersect
/// `rects` (raw user space). `fonts` maps `/Font` resource names to width
/// oracles. Fails (instead of passing content through) whenever removal cannot
/// be guaranteed.
pub(crate) fn redact_content(
    content: &[u8],
    rects: &[[f64; 4]],
    fonts: &BTreeMap<Vec<u8>, RFont>,
) -> Result<RedactOutcome, RedactError> {
    let mut lex = Lexer::new(content);
    let mut out: Vec<u8> = Vec::with_capacity(content.len());
    let mut used_xobjects: BTreeSet<Vec<u8>> = BTreeSet::new();
    let mut dropped_xobjects: BTreeSet<Vec<u8>> = BTreeSet::new();

    let mut gs = GState::new();
    let mut gs_stack: Vec<GState> = Vec::new();
    let mut tm: Mat = IDENTITY;
    let mut tlm: Mat = IDENTITY;
    let mut leading = 0.0f64;

    // Structured operands + their serialized bytes for the pending operator.
    let mut ops: Vec<ROp> = Vec::new();
    let mut pending: Vec<u8> = Vec::new();

    while let Some(tok) = lex.next_token() {
        match tok {
            Token::Integer(n) => {
                ops.push(ROp::Num(n as f64));
                serialize_token(&Token::Integer(n), &mut pending);
            }
            Token::Real(r) => {
                ops.push(ROp::Num(r));
                serialize_token(&Token::Real(r), &mut pending);
            }
            Token::Str(s) => {
                ops.push(ROp::Str(s.clone()));
                serialize_token(&Token::Str(s), &mut pending);
            }
            Token::Name(n) => {
                ops.push(ROp::Name(n.clone()));
                serialize_token(&Token::Name(n), &mut pending);
            }
            Token::ArrayOpen => {
                ops.push(ROp::ArrMark);
                serialize_token(&Token::ArrayOpen, &mut pending);
            }
            Token::ArrayClose => {
                collapse_array(&mut ops);
                serialize_token(&Token::ArrayClose, &mut pending);
            }
            Token::DictOpen => serialize_token(&Token::DictOpen, &mut pending),
            Token::DictClose => serialize_token(&Token::DictClose, &mut pending),
            Token::Keyword(kw) => {
                if kw == b"BI" {
                    return Err(RedactError::InlineImage);
                }
                let nums: Vec<f64> = ops
                    .iter()
                    .filter_map(|o| match o {
                        ROp::Num(n) => Some(*n),
                        _ => None,
                    })
                    .collect();
                let mut handled = false;
                match kw.as_slice() {
                    b"q" => gs_stack.push(gs.clone()),
                    b"Q" => {
                        if let Some(s) = gs_stack.pop() {
                            gs = s;
                        }
                    }
                    b"cm" => {
                        if let Some(m) = last6(&nums) {
                            gs.ctm = mat_mul(m, gs.ctm);
                        }
                    }
                    b"BT" => {
                        tm = IDENTITY;
                        tlm = IDENTITY;
                    }
                    b"Tf" => {
                        gs.font = ops.iter().find_map(|o| match o {
                            ROp::Name(n) => Some(n.clone()),
                            _ => None,
                        });
                        gs.size = nums.last().copied().unwrap_or(0.0);
                    }
                    b"Tc" => gs.tc = nums.last().copied().unwrap_or(0.0),
                    b"Tw" => gs.tw = nums.last().copied().unwrap_or(0.0),
                    b"Tz" => gs.th = nums.last().copied().unwrap_or(100.0) / 100.0,
                    b"Ts" => gs.rise = nums.last().copied().unwrap_or(0.0),
                    b"TL" => leading = nums.last().copied().unwrap_or(0.0),
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
                    b"T*" => {
                        tlm = mat_mul([1.0, 0.0, 0.0, 1.0, 0.0, -leading], tlm);
                        tm = tlm;
                    }
                    b"Tj" | b"TJ" | b"'" | b"\"" => {
                        handled = true;
                        // Normalize the show into a list of runs/adjustments.
                        let mut items: Vec<ShowItem> = Vec::new();
                        if kw == b"TJ" {
                            if let Some(ROp::Arr(a)) =
                                ops.iter().rev().find(|o| matches!(o, ROp::Arr(_)))
                            {
                                for el in a {
                                    match el {
                                        ROp::Str(s) => items.push(ShowItem::Str(s.clone())),
                                        ROp::Num(n) => items.push(ShowItem::Adj(*n)),
                                        _ => {}
                                    }
                                }
                            }
                        } else if let Some(ROp::Str(s)) =
                            ops.iter().rev().find(|o| matches!(o, ROp::Str(_)))
                        {
                            items.push(ShowItem::Str(s.clone()));
                        }
                        if kw == b"\"" {
                            // aw ac (s) " — word/char spacing become state.
                            if let Some((aw, ac)) = last2(&nums) {
                                gs.tw = aw;
                                gs.tc = ac;
                                out.extend_from_slice(
                                    format!("{} Tw {} Tc\n", format_real(aw), format_real(ac))
                                        .as_bytes(),
                                );
                            }
                        }
                        if kw == b"'" || kw == b"\"" {
                            tlm = mat_mul([1.0, 0.0, 0.0, 1.0, 0.0, -leading], tlm);
                            tm = tlm;
                            out.extend_from_slice(b"T*\n");
                        }
                        rewrite_show(&items, &gs, &mut tm, fonts, rects, &mut out);
                    }
                    b"Do" => {
                        handled = true;
                        let name = ops.iter().rev().find_map(|o| match o {
                            ROp::Name(n) => Some(n.clone()),
                            _ => None,
                        });
                        // Unit square through the CTM: any overlap = drop (a
                        // partially covered XObject is removed entirely).
                        let corners = [
                            apply(gs.ctm, 0.0, 0.0),
                            apply(gs.ctm, 1.0, 0.0),
                            apply(gs.ctm, 0.0, 1.0),
                            apply(gs.ctm, 1.0, 1.0),
                        ];
                        if overlaps(bounds(&corners), rects) {
                            if let Some(n) = name {
                                dropped_xobjects.insert(n);
                            }
                        } else {
                            if let Some(n) = name {
                                used_xobjects.insert(n);
                            }
                            out.extend_from_slice(&pending);
                            serialize_keyword(&kw, &mut out);
                        }
                    }
                    _ => {}
                }
                if !handled {
                    out.extend_from_slice(&pending);
                    serialize_keyword(&kw, &mut out);
                }
                pending.clear();
                ops.clear();
            }
        }
    }
    Ok(RedactOutcome {
        content: out,
        used_xobjects,
        dropped_xobjects,
    })
}

/// Rewrite one text show: measure every glyph, drop the ones whose box
/// intersects a rect, replacing them with `TJ` displacements so surviving
/// glyphs keep their positions. Advances `tm` by the show's full width (kept
/// + replaced — identical to the original), so later ops stay aligned.
fn rewrite_show(
    items: &[ShowItem],
    gs: &GState,
    tm: &mut Mat,
    fonts: &BTreeMap<Vec<u8>, RFont>,
    rects: &[[f64; 4]],
    out: &mut Vec<u8>,
) {
    let font = gs.font.as_ref().and_then(|k| fonts.get(k));
    let (two_byte, widths, default_w) = match font {
        Some(f) => (f.two_byte, Some(&f.widths), f.default_width),
        // Unknown font: measure with a conservative 0.5 em default.
        None => (false, None, 0.5),
    };

    let mut emitted: Vec<u8> = Vec::new(); // serialized TJ items
    let mut run: Vec<u8> = Vec::new(); // current kept byte run
    let mut pending_ts = 0.0f64; // displacement owed (text space, pre-Th)
    let mut any = false;

    fn flush_run(run: &mut Vec<u8>, emitted: &mut Vec<u8>) {
        if !run.is_empty() {
            emitted.push(b'<');
            for &b in run.iter() {
                emitted.push(hex_digit(b >> 4));
                emitted.push(hex_digit(b & 0xF));
            }
            emitted.extend_from_slice(b"> ");
            run.clear();
        }
    }

    for item in items {
        match item {
            ShowItem::Adj(v) => {
                // Original kerning adjustment: displacement −v/1000·size (pre-Th).
                flush_run(&mut run, &mut emitted);
                let d = -v / 1000.0 * gs.size;
                pending_ts += d;
                *tm = mat_mul([1.0, 0.0, 0.0, 1.0, d * gs.th, 0.0], *tm);
                any = true;
            }
            ShowItem::Str(s) => {
                let step = if two_byte { 2 } else { 1 };
                for chunk in s.chunks(step) {
                    let code = chunk.iter().fold(0u32, |a, &b| (a << 8) | b as u32);
                    let w0 = widths
                        .and_then(|m| m.get(&code).copied())
                        .unwrap_or(default_w);
                    let adv =
                        w0 * gs.size + gs.tc + if !two_byte && code == 32 { gs.tw } else { 0.0 };
                    // Device-space glyph box.
                    let trm = mat_mul(
                        mat_mul([gs.size * gs.th, 0.0, 0.0, gs.size, 0.0, gs.rise], *tm),
                        gs.ctm,
                    );
                    let corners = [
                        apply(trm, 0.0, -DESCENT),
                        apply(trm, w0, -DESCENT),
                        apply(trm, w0, ASCENT),
                        apply(trm, 0.0, ASCENT),
                    ];
                    if overlaps(bounds(&corners), rects) {
                        flush_run(&mut run, &mut emitted);
                        pending_ts += adv;
                    } else {
                        if pending_ts != 0.0 && gs.size != 0.0 {
                            emitted.extend_from_slice(
                                format!("{} ", format_real(-pending_ts * 1000.0 / gs.size))
                                    .as_bytes(),
                            );
                            pending_ts = 0.0;
                        }
                        run.extend_from_slice(chunk);
                    }
                    any = true;
                    *tm = mat_mul([1.0, 0.0, 0.0, 1.0, adv * gs.th, 0.0], *tm);
                }
            }
        }
    }
    flush_run(&mut run, &mut emitted);
    if pending_ts != 0.0 && gs.size != 0.0 {
        // Trailing displacement keeps tm identical for any following show.
        emitted.extend_from_slice(
            format!("{} ", format_real(-pending_ts * 1000.0 / gs.size)).as_bytes(),
        );
    }
    if any && !emitted.is_empty() {
        out.push(b'[');
        out.extend_from_slice(&emitted);
        out.extend_from_slice(b"] TJ\n");
    }
}

/// Collapse operands back to the innermost `ArrMark` into one `Arr`.
fn collapse_array(ops: &mut Vec<ROp>) {
    let mut items = Vec::new();
    while let Some(op) = ops.pop() {
        match op {
            ROp::ArrMark => {
                items.reverse();
                ops.push(ROp::Arr(items));
                return;
            }
            other => items.push(other),
        }
    }
    // Unbalanced close: restore as an (empty) array.
    items.reverse();
    ops.push(ROp::Arr(items));
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

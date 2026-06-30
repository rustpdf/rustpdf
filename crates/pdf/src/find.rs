//! Positional text search (issue #41 P1 #6): find where a query string appears
//! on each page and return its bounding box(es) in PDF user space.
//!
//! This is the positional companion to [`crate::extract_text`]. Where extraction
//! flattens a page to a Unicode string, this module also tracks the text and
//! graphics matrices so every shown glyph carries a position. It then matches
//! the query against the per-page glyph stream and unions the boxes of the
//! glyphs each match covers.
//!
//! Coordinates are returned in **PDF user space** (points, origin at the page's
//! lower-left, y pointing up) — the same space content is drawn in — so a hit's
//! box can be fed straight back into an overlay/stamp (e.g. to position a
//! signature next to an anchor word). Page `/Rotate` is *not* applied: the box
//! is where the text lives in the page's own coordinate system.
//!
//! Glyph advances come from the font's `/Widths` (simple) or `/W`+`/DW`
//! (Type0/CIDFontType2) dictionaries — no font program is parsed — and the box
//! height is approximated from the font size (ascent 0.8·size, descent
//! 0.2·size), which is plenty to anchor stamps. Like extraction, the Type0 path
//! (this library's own output) is the most solid; arbitrary third-party PDFs
//! are best effort.

use std::collections::BTreeMap;

use cos::{Dict, Object};
use parser::{Lexer, PdfReader, Token};

use crate::extract::parse_cmap;

/// One occurrence of the query string on a page.
#[derive(Debug, Clone, PartialEq)]
pub struct TextHit {
    /// Zero-based page index the hit is on.
    pub page: usize,
    /// The matched text, exactly as decoded from the page.
    pub text: String,
    /// X of the bounding box's lower-left corner, in PDF user-space points.
    pub x: f64,
    /// Y of the bounding box's lower-left corner, in PDF user-space points.
    pub y: f64,
    /// Bounding-box width in points.
    pub width: f64,
    /// Bounding-box height in points.
    pub height: f64,
}

/// How [`find_text`] matches.
#[derive(Debug, Clone, Copy, Default)]
pub struct FindOptions {
    /// When false (default), matching is case-insensitive (Unicode simple
    /// case-folding via `to_lowercase`).
    pub case_sensitive: bool,
}

/// Find every occurrence of `query` across all pages, returning a bounding box
/// per occurrence in PDF user space. An empty query yields no hits.
pub fn find_text(
    bytes: impl AsRef<[u8]>,
    query: &str,
    options: FindOptions,
) -> Result<Vec<TextHit>, parser::PdfError> {
    let reader = PdfReader::parse(bytes)?;
    let mut hits = Vec::new();
    for (i, page) in reader.pages().iter().enumerate() {
        find_on_page(&reader, page, i, query, options, &mut hits);
    }
    Ok(hits)
}

/// Box accumulated for one painted glyph (axis-aligned in user space).
#[derive(Clone, Copy)]
struct GlyphBox {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
    has_box: bool,
}

/// One decoded character plus the box of the glyph that produced it. Ligatures
/// (one glyph → many chars) share the same glyph box across their chars.
struct PChar {
    ch: char,
    bbox: GlyphBox,
}

fn find_on_page(
    reader: &PdfReader,
    page: &Dict,
    page_index: usize,
    query: &str,
    options: FindOptions,
    out: &mut Vec<TextHit>,
) {
    if query.is_empty() {
        return;
    }
    let resources = page
        .get("Resources")
        .and_then(|o| reader.resolve_dict(o))
        .cloned()
        .unwrap_or_default();
    let content = page_content(reader, page);
    let mut chars: Vec<PChar> = Vec::new();
    run_content(
        reader,
        &content,
        &resources,
        Matrix::identity(),
        0,
        &mut chars,
    );
    match_chars(&chars, page_index, query, options, out);
}

/// Scan the decoded glyph stream for the query and emit a hit per match.
fn match_chars(
    chars: &[PChar],
    page_index: usize,
    query: &str,
    options: FindOptions,
    out: &mut Vec<TextHit>,
) {
    // Build a parallel lowercase (or as-is) char vector for matching.
    let fold = |s: &str| -> Vec<char> {
        if options.case_sensitive {
            s.chars().collect()
        } else {
            s.to_lowercase().chars().collect()
        }
    };
    // Each PChar contributes exactly one char to the haystack (1:1 index map).
    let hay: Vec<char> = chars
        .iter()
        .flat_map(|p| {
            let s: String = if options.case_sensitive {
                p.ch.to_string()
            } else {
                p.ch.to_lowercase().to_string()
            };
            // Keep 1:1 with PChar: fold to a single representative char so the
            // index alignment with `chars` holds (rare 1→many foldings like ß
            // are kept as their first folded char for positioning).
            s.chars().next().into_iter()
        })
        .collect();
    let needle = fold(query);
    if needle.is_empty() || hay.len() < needle.len() {
        return;
    }
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if hay[i..i + needle.len()] == needle[..] {
            // Union the boxes of the covered glyphs.
            let mut acc = GlyphBox::empty();
            let mut text = String::new();
            for p in &chars[i..i + needle.len()] {
                text.push(p.ch);
                acc.union(&p.bbox);
            }
            if acc.has_box {
                out.push(TextHit {
                    page: page_index,
                    text,
                    x: acc.min_x,
                    y: acc.min_y,
                    width: (acc.max_x - acc.min_x).max(0.0),
                    height: (acc.max_y - acc.min_y).max(0.0),
                });
            }
            i += needle.len(); // non-overlapping matches
        } else {
            i += 1;
        }
    }
}

impl GlyphBox {
    fn empty() -> Self {
        GlyphBox {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
            has_box: false,
        }
    }
    fn union(&mut self, other: &GlyphBox) {
        if !other.has_box {
            return;
        }
        self.min_x = self.min_x.min(other.min_x);
        self.min_y = self.min_y.min(other.min_y);
        self.max_x = self.max_x.max(other.max_x);
        self.max_y = self.max_y.max(other.max_y);
        self.has_box = true;
    }
}

// ---------------------------------------------------------------------------
// 2D affine matrix (PDF row-vector convention), matching `render`'s semantics:
// `a.pre_concat(b)` maps a point by applying `b` first, then `a`.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Matrix {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Matrix {
    fn identity() -> Self {
        Matrix {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        }
    }
    fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Matrix { a, b, c, d, e, f }
    }
    fn translate(tx: f64, ty: f64) -> Self {
        Matrix::new(1.0, 0.0, 0.0, 1.0, tx, ty)
    }
    /// Map a point: (x, y) → (a·x + c·y + e, b·x + d·y + f).
    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }
    /// `self ∘ m`: apply `m` first, then `self`.
    fn pre_concat(&self, m: &Matrix) -> Matrix {
        Matrix {
            a: self.a * m.a + self.c * m.b,
            b: self.b * m.a + self.d * m.b,
            c: self.a * m.c + self.c * m.d,
            d: self.b * m.c + self.d * m.d,
            e: self.a * m.e + self.c * m.f + self.e,
            f: self.b * m.e + self.d * m.f + self.f,
        }
    }
}

// ---------------------------------------------------------------------------
// Per-font decode info: Unicode mapping + advance widths.
// ---------------------------------------------------------------------------

struct FFont {
    two_byte: bool,
    to_unicode: BTreeMap<u32, String>,
    /// code (simple) or CID (Type0, Identity assumed) → width in /1000 units.
    widths: BTreeMap<u32, f64>,
    default_width: f64,
}

impl FFont {
    /// Decode a shown byte string into (unicode, width/1000) per glyph.
    fn glyphs(&self, bytes: &[u8]) -> Vec<(String, f64)> {
        let width = if self.two_byte { 2 } else { 1 };
        let mut out = Vec::new();
        for chunk in bytes.chunks(width) {
            let code = chunk.iter().fold(0u32, |a, &b| (a << 8) | b as u32);
            let uni = if let Some(s) = self.to_unicode.get(&code) {
                s.clone()
            } else if !self.two_byte {
                (code as u8 as char).to_string()
            } else {
                String::new()
            };
            let w = self
                .widths
                .get(&code)
                .copied()
                .unwrap_or(self.default_width);
            out.push((uni, w));
        }
        out
    }
}

/// Graphics + text state we track. `tm`/`tlm` live only inside BT/ET.
#[derive(Clone)]
struct GState {
    ctm: Matrix,
    font: Option<usize>,
    font_size: f64,
    char_spacing: f64,
    word_spacing: f64,
    h_scale: f64,
    leading: f64,
    rise: f64,
}

impl GState {
    fn new(ctm: Matrix) -> Self {
        GState {
            ctm,
            font: None,
            font_size: 0.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            h_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
        }
    }
}

const ASCENT: f64 = 0.8;
const DESCENT: f64 = 0.2;

#[allow(clippy::too_many_lines)]
fn run_content(
    reader: &PdfReader,
    content: &[u8],
    resources: &Dict,
    base_ctm: Matrix,
    depth: u32,
    out: &mut Vec<PChar>,
) {
    if depth > 12 {
        return;
    }
    let fonts = fonts_from_resources(reader, resources);
    let mut lex = Lexer::new(content);
    let mut operands: Vec<Op> = Vec::new();
    let mut stack: Vec<GState> = Vec::new();
    let mut gs = GState::new(base_ctm);
    let mut tm = Matrix::identity();
    let mut tlm = Matrix::identity();

    macro_rules! num {
        ($n:expr) => {
            match nth_from_end(&operands, $n) {
                Some(Op::Num(v)) => Some(*v),
                _ => None,
            }
        };
    }

    while let Some(tok) = lex.next_token() {
        match tok {
            Token::Integer(n) => operands.push(Op::Num(n as f64)),
            Token::Real(r) => operands.push(Op::Num(r)),
            Token::Str(s) => operands.push(Op::Str(s)),
            Token::Name(n) => operands.push(Op::Name(n)),
            Token::ArrayOpen => operands.push(Op::ArrayMark),
            Token::ArrayClose => collapse_array(&mut operands),
            Token::DictOpen | Token::DictClose => operands.clear(),
            Token::Keyword(kw) => {
                match kw.as_slice() {
                    b"q" => stack.push(gs.clone()),
                    b"Q" => {
                        if let Some(s) = stack.pop() {
                            gs = s;
                        }
                    }
                    b"cm" => {
                        if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) =
                            (num!(5), num!(4), num!(3), num!(2), num!(1), num!(0))
                        {
                            gs.ctm = gs.ctm.pre_concat(&Matrix::new(a, b, c, d, e, f));
                        }
                    }
                    b"BT" => {
                        tm = Matrix::identity();
                        tlm = Matrix::identity();
                    }
                    b"ET" => {}
                    b"Tc" => gs.char_spacing = num!(0).unwrap_or(0.0),
                    b"Tw" => gs.word_spacing = num!(0).unwrap_or(0.0),
                    b"Tz" => gs.h_scale = num!(0).unwrap_or(100.0) / 100.0,
                    b"TL" => gs.leading = num!(0).unwrap_or(0.0),
                    b"Ts" => gs.rise = num!(0).unwrap_or(0.0),
                    b"Tf" => {
                        if let Some(Op::Name(name)) = nth_from_end(&operands, 1) {
                            gs.font = fonts.iter().position(|(n, _)| n == name);
                        }
                        gs.font_size = num!(0).unwrap_or(0.0);
                    }
                    b"Td" => {
                        let (tx, ty) = (num!(1).unwrap_or(0.0), num!(0).unwrap_or(0.0));
                        tlm = tlm.pre_concat(&Matrix::translate(tx, ty));
                        tm = tlm;
                    }
                    b"TD" => {
                        let (tx, ty) = (num!(1).unwrap_or(0.0), num!(0).unwrap_or(0.0));
                        gs.leading = -ty;
                        tlm = tlm.pre_concat(&Matrix::translate(tx, ty));
                        tm = tlm;
                    }
                    b"Tm" => {
                        if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) =
                            (num!(5), num!(4), num!(3), num!(2), num!(1), num!(0))
                        {
                            tlm = Matrix::new(a, b, c, d, e, f);
                            tm = tlm;
                        }
                    }
                    b"T*" => {
                        tlm = tlm.pre_concat(&Matrix::translate(0.0, -gs.leading));
                        tm = tlm;
                    }
                    b"Tj" => {
                        if let Some(Op::Str(s)) = nth_from_end(&operands, 0) {
                            show(&fonts, &gs, &mut tm, s, out);
                        }
                    }
                    b"'" => {
                        tlm = tlm.pre_concat(&Matrix::translate(0.0, -gs.leading));
                        tm = tlm;
                        if let Some(Op::Str(s)) = nth_from_end(&operands, 0) {
                            show(&fonts, &gs, &mut tm, s, out);
                        }
                    }
                    b"\"" => {
                        gs.word_spacing = num!(2).unwrap_or(0.0);
                        gs.char_spacing = num!(1).unwrap_or(0.0);
                        tlm = tlm.pre_concat(&Matrix::translate(0.0, -gs.leading));
                        tm = tlm;
                        if let Some(Op::Str(s)) = nth_from_end(&operands, 0) {
                            show(&fonts, &gs, &mut tm, s, out);
                        }
                    }
                    b"TJ" => {
                        if let Some(Op::Array(items)) = nth_from_end(&operands, 0) {
                            for el in items {
                                match el {
                                    Op::Str(s) => show(&fonts, &gs, &mut tm, s, out),
                                    Op::Num(adj) => {
                                        let tx = -adj / 1000.0 * gs.font_size * gs.h_scale;
                                        tm = tm.pre_concat(&Matrix::translate(tx, 0.0));
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    b"Do" => {
                        if let Some(Op::Name(name)) = nth_from_end(&operands, 0) {
                            do_xobject(reader, name, resources, gs.ctm, depth, out);
                        }
                    }
                    _ => {}
                }
                operands.clear();
            }
        }
    }
}

/// Show one string: decode glyphs, record each glyph's box, advance `tm`.
fn show(
    fonts: &[(Vec<u8>, FFont)],
    gs: &GState,
    tm: &mut Matrix,
    bytes: &[u8],
    out: &mut Vec<PChar>,
) {
    let Some(idx) = gs.font else {
        return;
    };
    let font = &fonts[idx].1;
    let fs = gs.font_size;
    for (uni, w0) in font.glyphs(bytes) {
        // Render matrix in user space: ctm · tm · [fs·Th 0 0 fs 0 rise].
        let param = Matrix::new(fs * gs.h_scale, 0.0, 0.0, fs, 0.0, gs.rise);
        let m = gs.ctm.pre_concat(tm).pre_concat(&param);
        // Glyph box in param-local space: x∈[0, w0], y∈[-DESCENT, ASCENT].
        let bbox = if uni.is_empty() {
            GlyphBox::empty()
        } else {
            box_of(&m, w0)
        };
        let is_space = uni == " ";
        for ch in uni.chars() {
            out.push(PChar { ch, bbox });
        }
        // Advance tm in text space.
        let mut tx = (w0 * fs + gs.char_spacing) * gs.h_scale;
        if is_space {
            tx += gs.word_spacing * gs.h_scale;
        }
        *tm = tm.pre_concat(&Matrix::translate(tx, 0.0));
    }
}

/// Axis-aligned box (in user space) of a glyph spanning x∈[0, w0] (param-local).
fn box_of(m: &Matrix, w0: f64) -> GlyphBox {
    let corners = [
        m.apply(0.0, -DESCENT),
        m.apply(w0, -DESCENT),
        m.apply(w0, ASCENT),
        m.apply(0.0, ASCENT),
    ];
    let mut b = GlyphBox::empty();
    for (x, y) in corners {
        b.min_x = b.min_x.min(x);
        b.min_y = b.min_y.min(y);
        b.max_x = b.max_x.max(x);
        b.max_y = b.max_y.max(y);
    }
    b.has_box = true;
    b
}

#[derive(Debug, Clone)]
enum Op {
    Num(f64),
    Str(Vec<u8>),
    Name(Vec<u8>),
    Array(Vec<Op>),
    ArrayMark,
}

fn nth_from_end(stack: &[Op], n: usize) -> Option<&Op> {
    stack.len().checked_sub(n + 1).map(|i| &stack[i])
}

fn collapse_array(stack: &mut Vec<Op>) {
    if let Some(start) = stack.iter().rposition(|o| matches!(o, Op::ArrayMark)) {
        let items: Vec<Op> = stack.split_off(start + 1);
        stack.pop();
        stack.push(Op::Array(items));
    }
}

/// Recurse into a Form XObject drawn with `Do`, honoring its `/Matrix`.
fn do_xobject(
    reader: &PdfReader,
    name: &[u8],
    resources: &Dict,
    ctm: Matrix,
    depth: u32,
    out: &mut Vec<PChar>,
) {
    let Some(xobjs) = resources
        .get("XObject")
        .and_then(|o| reader.resolve_dict(o))
    else {
        return;
    };
    let Ok(key) = std::str::from_utf8(name) else {
        return;
    };
    let Some(Object::Stream(s)) = xobjs.get(key).map(|o| reader.resolve(o)) else {
        return;
    };
    if s.dict.get("Subtype").and_then(name_str).as_deref() != Some("Form") {
        return;
    }
    let mut m = ctm;
    if let Some(Object::Array(a)) = s.dict.get("Matrix").map(|o| reader.resolve(o)) {
        let v: Vec<f64> = a.iter().filter_map(num_of).collect();
        if v.len() == 6 {
            m = ctm.pre_concat(&Matrix::new(v[0], v[1], v[2], v[3], v[4], v[5]));
        }
    }
    let sub_res = s
        .dict
        .get("Resources")
        .and_then(|o| reader.resolve_dict(o))
        .cloned()
        .unwrap_or_else(|| resources.clone());
    if let Ok(data) = reader.stream_data(s) {
        run_content(reader, &data, &sub_res, m, depth + 1, out);
    }
}

/// Build the page's font list (`name -> FFont`) from `/Resources /Font`.
fn fonts_from_resources(reader: &PdfReader, resources: &Dict) -> Vec<(Vec<u8>, FFont)> {
    let mut list = Vec::new();
    let Some(fonts) = resources.get("Font").and_then(|o| reader.resolve_dict(o)) else {
        return list;
    };
    for (name, font_ref) in fonts.iter() {
        let Some(font_dict) = reader.resolve_dict(font_ref) else {
            continue;
        };
        let two_byte = font_dict.get("Subtype").and_then(name_str).as_deref() == Some("Type0");
        let to_unicode = font_dict
            .get("ToUnicode")
            .map(|o| reader.resolve(o))
            .and_then(|o| match o {
                Object::Stream(s) => reader.stream_data(s).ok(),
                _ => None,
            })
            .map(|data| parse_cmap(&data))
            .unwrap_or_default();
        let (widths, default_width) = load_widths(reader, font_dict, two_byte);
        list.push((
            name.as_str().as_bytes().to_vec(),
            FFont {
                two_byte,
                to_unicode,
                widths,
                default_width,
            },
        ));
    }
    list
}

/// Build the code/CID → width map (in /1000 units) and a default width.
fn load_widths(reader: &PdfReader, font_dict: &Dict, two_byte: bool) -> (BTreeMap<u32, f64>, f64) {
    let mut map = BTreeMap::new();
    if two_byte {
        // Type0: descendant CIDFont carries /W and /DW.
        let cid = font_dict
            .get("DescendantFonts")
            .map(|o| reader.resolve(o))
            .and_then(|o| match o {
                Object::Array(a) => a.first().and_then(|f| reader.resolve_dict(f)).cloned(),
                Object::Dict(d) => Some(d.clone()),
                _ => None,
            });
        let Some(cid) = cid else {
            return (map, 1.0);
        };
        let dw = cid
            .get("DW")
            .and_then(num_of)
            .map(|w| w / 1000.0)
            .unwrap_or(1.0);
        parse_w_array(reader, cid.get("W"), &mut map);
        (map, dw)
    } else {
        // Simple font: /FirstChar + /Widths, /FontDescriptor /MissingWidth.
        let first = font_dict.get("FirstChar").and_then(int_of).unwrap_or(0);
        if let Some(Object::Array(a)) = font_dict.get("Widths").map(|o| reader.resolve(o)) {
            for (i, w) in a.iter().enumerate() {
                if let Some(wv) = num_of(w) {
                    map.insert((first + i as i64) as u32, wv / 1000.0);
                }
            }
        }
        let missing = font_dict
            .get("FontDescriptor")
            .and_then(|o| reader.resolve_dict(o))
            .and_then(|d| d.get("MissingWidth").and_then(num_of))
            .map(|w| w / 1000.0)
            .unwrap_or(0.5);
        (map, missing)
    }
}

/// Parse a CIDFont `/W` array into `cid → width` (/1000 units).
fn parse_w_array(reader: &PdfReader, w: Option<&Object>, map: &mut BTreeMap<u32, f64>) {
    let arr = match w.map(|o| reader.resolve(o)) {
        Some(Object::Array(a)) => a.clone(),
        _ => return,
    };
    let mut i = 0;
    while i < arr.len() {
        let c = match int_of(reader.resolve(&arr[i])) {
            Some(v) => v as u32,
            None => break,
        };
        match arr.get(i + 1).map(|o| reader.resolve(o)) {
            Some(Object::Array(ws)) => {
                for (j, wo) in ws.iter().enumerate() {
                    if let Some(wv) = num_of(wo) {
                        map.insert(c + j as u32, wv / 1000.0);
                    }
                }
                i += 2;
            }
            Some(_) => {
                let c_last = arr
                    .get(i + 1)
                    .and_then(|o| int_of(reader.resolve(o)))
                    .unwrap_or(c as i64) as u32;
                let wv = arr.get(i + 2).and_then(num_of).unwrap_or(0.0) / 1000.0;
                for cid in c..=c_last.min(c + 65_535) {
                    map.insert(cid, wv);
                }
                i += 3;
            }
            None => break,
        }
    }
}

/// Concatenate and decode a page's content stream(s).
fn page_content(reader: &PdfReader, page: &Dict) -> Vec<u8> {
    let mut out = Vec::new();
    let Some(contents) = page.get("Contents") else {
        return out;
    };
    match reader.resolve(contents) {
        Object::Stream(s) => {
            if let Ok(d) = reader.stream_data(s) {
                out.extend_from_slice(&d);
            }
        }
        Object::Array(items) => {
            for it in items {
                if let Object::Stream(s) = reader.resolve(it) {
                    if let Ok(d) = reader.stream_data(s) {
                        out.extend_from_slice(&d);
                        out.push(b'\n');
                    }
                }
            }
        }
        _ => {}
    }
    out
}

fn name_str(o: &Object) -> Option<String> {
    match o {
        Object::Name(n) => Some(n.as_str().to_string()),
        _ => None,
    }
}

fn num_of(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(n) => Some(*n as f64),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

fn int_of(o: &Object) -> Option<i64> {
    match o {
        Object::Integer(n) => Some(*n),
        Object::Real(r) => Some(*r as i64),
        _ => None,
    }
}

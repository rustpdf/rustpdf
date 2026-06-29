//! Content stream construction: graphics state, device colors, path building
//! and painting operators (Fase 2 of `project.md`).
//!
//! [`Content`] is a thin, fluent builder over a byte buffer. It only emits
//! syntactically-valid operator sequences; it does not track or validate
//! graphics state semantics (that is the renderer's job). Numbers are written
//! in fixed notation so output is deterministic and exponent-free.

/// A 2-D affine transform `[a b c d e f]` as used by the `cm` operator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Matrix {
    /// The identity transform.
    pub const IDENTITY: Matrix = Matrix {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// A pure translation by `(tx, ty)`.
    pub fn translate(tx: f64, ty: f64) -> Matrix {
        Matrix {
            e: tx,
            f: ty,
            ..Matrix::IDENTITY
        }
    }

    /// A pure scale by `(sx, sy)`.
    pub fn scale(sx: f64, sy: f64) -> Matrix {
        Matrix {
            a: sx,
            d: sy,
            ..Matrix::IDENTITY
        }
    }
}

/// A fluent builder that accumulates content-stream operators.
#[derive(Debug, Default, Clone)]
pub struct Content {
    buf: Vec<u8>,
}

impl Content {
    /// A new, empty content stream.
    pub fn new() -> Self {
        Content { buf: Vec::new() }
    }

    /// Consume the builder and return the raw content-stream bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    /// Borrow the bytes accumulated so far.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    /// Append already-formatted content-stream bytes verbatim (used to splice
    /// pre-built segments together).
    pub fn append_raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(bytes);
        self
    }

    // ---- graphics state (2.3) ---------------------------------------------

    /// `q` — push a copy of the graphics state.
    pub fn save_state(&mut self) -> &mut Self {
        self.op0("q")
    }

    /// `Q` — pop the graphics state.
    pub fn restore_state(&mut self) -> &mut Self {
        self.op0("Q")
    }

    /// `cm` — prepend `matrix` to the current transformation matrix.
    pub fn concat_matrix(&mut self, m: Matrix) -> &mut Self {
        self.nums(&[m.a, m.b, m.c, m.d, m.e, m.f]);
        self.kw("cm")
    }

    /// `w` — set the line width.
    pub fn set_line_width(&mut self, width: f64) -> &mut Self {
        self.nums(&[width]);
        self.kw("w")
    }

    // ---- device colors (2.4) ----------------------------------------------

    /// `rg` — set the non-stroking (fill) color in DeviceRGB.
    ///
    /// Components are clamped to the valid `0.0..=1.0` range; out-of-range values
    /// are illegal per the PDF spec and would fail PDF/A validation.
    pub fn set_fill_rgb(&mut self, r: f64, g: f64, b: f64) -> &mut Self {
        self.nums(&[clamp01(r), clamp01(g), clamp01(b)]);
        self.kw("rg")
    }

    /// `RG` — set the stroking color in DeviceRGB.
    pub fn set_stroke_rgb(&mut self, r: f64, g: f64, b: f64) -> &mut Self {
        self.nums(&[clamp01(r), clamp01(g), clamp01(b)]);
        self.kw("RG")
    }

    /// `g` — set the non-stroking color in DeviceGray.
    pub fn set_fill_gray(&mut self, gray: f64) -> &mut Self {
        self.nums(&[clamp01(gray)]);
        self.kw("g")
    }

    /// `G` — set the stroking color in DeviceGray.
    pub fn set_stroke_gray(&mut self, gray: f64) -> &mut Self {
        self.nums(&[clamp01(gray)]);
        self.kw("G")
    }

    /// `k` — set the non-stroking color in DeviceCMYK.
    pub fn set_fill_cmyk(&mut self, c: f64, m: f64, y: f64, k: f64) -> &mut Self {
        self.nums(&[clamp01(c), clamp01(m), clamp01(y), clamp01(k)]);
        self.kw("k")
    }

    /// `K` — set the stroking color in DeviceCMYK.
    pub fn set_stroke_cmyk(&mut self, c: f64, m: f64, y: f64, k: f64) -> &mut Self {
        self.nums(&[clamp01(c), clamp01(m), clamp01(y), clamp01(k)]);
        self.kw("K")
    }

    // ---- path construction (2.5) ------------------------------------------

    /// `m` — begin a new subpath at `(x, y)`.
    pub fn move_to(&mut self, x: f64, y: f64) -> &mut Self {
        self.nums(&[x, y]);
        self.kw("m")
    }

    /// `l` — append a straight line to `(x, y)`.
    pub fn line_to(&mut self, x: f64, y: f64) -> &mut Self {
        self.nums(&[x, y]);
        self.kw("l")
    }

    /// `c` — cubic Bézier with both control points.
    pub fn curve_to(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) -> &mut Self {
        self.nums(&[x1, y1, x2, y2, x3, y3]);
        self.kw("c")
    }

    /// `v` — cubic Bézier using the current point as the first control point.
    pub fn curve_to_v(&mut self, x2: f64, y2: f64, x3: f64, y3: f64) -> &mut Self {
        self.nums(&[x2, y2, x3, y3]);
        self.kw("v")
    }

    /// `y` — cubic Bézier where the second control point equals the endpoint.
    pub fn curve_to_y(&mut self, x1: f64, y1: f64, x3: f64, y3: f64) -> &mut Self {
        self.nums(&[x1, y1, x3, y3]);
        self.kw("y")
    }

    /// `re` — append a rectangle as a complete subpath.
    pub fn rect(&mut self, x: f64, y: f64, width: f64, height: f64) -> &mut Self {
        self.nums(&[x, y, width, height]);
        self.kw("re")
    }

    /// `h` — close the current subpath.
    pub fn close_path(&mut self) -> &mut Self {
        self.op0("h")
    }

    // ---- painting and clipping (2.6) --------------------------------------

    /// `S` — stroke the path.
    pub fn stroke(&mut self) -> &mut Self {
        self.op0("S")
    }

    /// `s` — close and stroke the path.
    pub fn close_and_stroke(&mut self) -> &mut Self {
        self.op0("s")
    }

    /// `f` — fill the path using the nonzero winding rule.
    pub fn fill(&mut self) -> &mut Self {
        self.op0("f")
    }

    /// `f*` — fill the path using the even-odd rule.
    pub fn fill_even_odd(&mut self) -> &mut Self {
        self.op0("f*")
    }

    /// `B` — fill then stroke (nonzero winding).
    pub fn fill_and_stroke(&mut self) -> &mut Self {
        self.op0("B")
    }

    /// `B*` — fill then stroke (even-odd).
    pub fn fill_and_stroke_even_odd(&mut self) -> &mut Self {
        self.op0("B*")
    }

    /// `n` — end the path without filling or stroking (used after clipping).
    pub fn end_path(&mut self) -> &mut Self {
        self.op0("n")
    }

    /// `W` — intersect the clip path using the nonzero winding rule.
    pub fn clip(&mut self) -> &mut Self {
        self.op0("W")
    }

    /// `W*` — intersect the clip path using the even-odd rule.
    pub fn clip_even_odd(&mut self) -> &mut Self {
        self.op0("W*")
    }

    // ---- internals ---------------------------------------------------------

    /// Emit a zero-operand operator on its own line.
    fn op0(&mut self, op: &str) -> &mut Self {
        self.buf.extend_from_slice(op.as_bytes());
        self.buf.push(b'\n');
        self
    }

    /// Emit just the operator keyword that follows already-written operands.
    fn kw(&mut self, op: &str) -> &mut Self {
        self.buf.extend_from_slice(op.as_bytes());
        self.buf.push(b'\n');
        self
    }

    // ---- marked content (Tagged PDF, Fase 7.5) ---------------------------

    /// `/Tag <</MCID n>> BDC` — begin a marked-content sequence bound to a
    /// structure element via its MCID.
    pub fn begin_marked_content(&mut self, tag: &str, mcid: i32) -> &mut Self {
        self.buf.push(b'/');
        self.buf.extend_from_slice(tag.as_bytes());
        self.buf.extend_from_slice(b" <</MCID ");
        write_num(mcid as f64, &mut self.buf);
        self.buf.extend_from_slice(b">> BDC\n");
        self
    }

    /// `/Artifact BMC` — begin an artifact (non-structural) sequence. Uses
    /// `BMC` (no property list), since `BDC` would require a properties operand.
    pub fn begin_artifact(&mut self) -> &mut Self {
        self.buf.extend_from_slice(b"/Artifact BMC\n");
        self
    }

    /// `EMC` — end the current marked-content sequence.
    pub fn end_marked_content(&mut self) -> &mut Self {
        self.op0("EMC")
    }

    // ---- XObjects / images (Fase 4.2) ------------------------------------

    /// `Do` — paint the named XObject (e.g. an image resource `Im0`).
    pub fn do_xobject(&mut self, resource: &str) -> &mut Self {
        self.buf.push(b'/');
        self.buf.extend_from_slice(resource.as_bytes());
        self.buf.push(b' ');
        self.kw("Do")
    }

    /// Paint image `resource` into the rectangle `(x, y)`–`(x+w, y+h)`.
    ///
    /// Images are drawn in a unit square, so this saves state, maps the unit
    /// square onto the target rectangle with a `cm`, invokes `Do`, and restores.
    pub fn draw_image(&mut self, resource: &str, x: f64, y: f64, w: f64, h: f64) -> &mut Self {
        self.save_state()
            .concat_matrix(Matrix {
                a: w,
                b: 0.0,
                c: 0.0,
                d: h,
                e: x,
                f: y,
            })
            .do_xobject(resource)
            .restore_state()
    }

    // ---- text objects and operators (3A.4) -------------------------------

    /// `BT` — begin a text object.
    pub fn begin_text(&mut self) -> &mut Self {
        self.op0("BT")
    }

    /// `ET` — end a text object.
    pub fn end_text(&mut self) -> &mut Self {
        self.op0("ET")
    }

    /// `Tf` — select font `resource` (a name like `F0`) at `size`.
    pub fn set_font(&mut self, resource: &str, size: f64) -> &mut Self {
        self.buf.push(b'/');
        self.buf.extend_from_slice(resource.as_bytes());
        self.buf.push(b' ');
        write_num(size, &mut self.buf);
        self.buf.push(b' ');
        self.kw("Tf")
    }

    /// `Tc` — character spacing (unscaled text-space units).
    pub fn set_char_spacing(&mut self, spacing: f64) -> &mut Self {
        self.nums(&[spacing]);
        self.kw("Tc")
    }

    /// `Tw` — word spacing (single-byte fonts only).
    pub fn set_word_spacing(&mut self, spacing: f64) -> &mut Self {
        self.nums(&[spacing]);
        self.kw("Tw")
    }

    /// `Tz` — horizontal scaling, as a percentage (100 = normal).
    pub fn set_horizontal_scale(&mut self, percent: f64) -> &mut Self {
        self.nums(&[percent]);
        self.kw("Tz")
    }

    /// `TL` — text leading (line height).
    pub fn set_leading(&mut self, leading: f64) -> &mut Self {
        self.nums(&[leading]);
        self.kw("TL")
    }

    /// `Ts` — text rise (super/subscript offset).
    pub fn set_text_rise(&mut self, rise: f64) -> &mut Self {
        self.nums(&[rise]);
        self.kw("Ts")
    }

    /// `Tr` — text rendering mode (0 fill, 1 stroke, 2 fill+stroke, 3 invisible…).
    pub fn set_text_render_mode(&mut self, mode: u8) -> &mut Self {
        self.nums(&[mode as f64]);
        self.kw("Tr")
    }

    /// `Tm` — set the text matrix (also resets the line matrix).
    pub fn set_text_matrix(&mut self, m: Matrix) -> &mut Self {
        self.nums(&[m.a, m.b, m.c, m.d, m.e, m.f]);
        self.kw("Tm")
    }

    /// `Td` — move to the next line offset by `(tx, ty)` from the line start.
    pub fn next_line_offset(&mut self, tx: f64, ty: f64) -> &mut Self {
        self.nums(&[tx, ty]);
        self.kw("Td")
    }

    /// `TD` — like `Td` but also sets leading to `-ty`.
    pub fn next_line_offset_leading(&mut self, tx: f64, ty: f64) -> &mut Self {
        self.nums(&[tx, ty]);
        self.kw("TD")
    }

    /// `T*` — move to the start of the next line (uses the current leading).
    pub fn next_line(&mut self) -> &mut Self {
        self.op0("T*")
    }

    /// `Tj` — show a pre-encoded string (raw bytes, written as a hex string).
    pub fn show_text_hex(&mut self, bytes: &[u8]) -> &mut Self {
        self.write_hex_string(bytes);
        self.buf.push(b' ');
        self.kw("Tj")
    }

    /// `TJ` — show glyph runs with inter-glyph position adjustments.
    pub fn show_text_adjusted(&mut self, parts: &[TextPart]) -> &mut Self {
        self.buf.push(b'[');
        for part in parts {
            match part {
                TextPart::Glyphs(bytes) => self.write_hex_string(bytes),
                TextPart::Adjust(a) => {
                    write_num(*a, &mut self.buf);
                    self.buf.push(b' ');
                }
            }
        }
        self.buf.push(b']');
        self.buf.push(b' ');
        self.kw("TJ")
    }

    fn write_hex_string(&mut self, bytes: &[u8]) {
        self.buf.push(b'<');
        for &b in bytes {
            self.buf.push(hex_digit(b >> 4));
            self.buf.push(hex_digit(b & 0x0F));
        }
        self.buf.push(b'>');
    }

    /// Write a space-separated run of operands (trailing space before the op).
    fn nums(&mut self, values: &[f64]) {
        for &v in values {
            write_num(v, &mut self.buf);
            self.buf.push(b' ');
        }
    }
}

/// An element of a `TJ` array: either glyph bytes or a position adjustment
/// (in thousandths of an em of text space; positive moves left).
#[derive(Debug, Clone, PartialEq)]
pub enum TextPart {
    /// Raw glyph code bytes (2 bytes per CID for Identity-H).
    Glyphs(Vec<u8>),
    /// Position adjustment.
    Adjust(f64),
}

/// Clamp a color component to the valid `0.0..=1.0` range (NaN maps to 0.0).
fn clamp01(v: f64) -> f64 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(0.0, 1.0)
    }
}

fn hex_digit(nibble: u8) -> u8 {
    match nibble {
        0..=9 => b'0' + nibble,
        _ => b'A' + (nibble - 10),
    }
}

/// Format a number in fixed notation (no exponent), trimming trailing zeros.
fn write_num(v: f64, out: &mut Vec<u8>) {
    if !v.is_finite() {
        out.push(b'0');
        return;
    }
    if v == v.trunc() && v.abs() < 1e15 {
        let mut n = v as i64;
        if n == 0 {
            out.push(b'0');
            return;
        }
        if n < 0 {
            out.push(b'-');
            n = -n;
        }
        let mut tmp = [0u8; 20];
        let mut idx = tmp.len();
        while n > 0 {
            idx -= 1;
            tmp[idx] = b'0' + (n % 10) as u8;
            n /= 10;
        }
        out.extend_from_slice(&tmp[idx..]);
        return;
    }
    let s = format!("{v:.4}");
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    out.extend_from_slice(trimmed.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(c: &Content) -> String {
        String::from_utf8(c.as_bytes().to_vec()).unwrap()
    }

    #[test]
    fn state_and_matrix() {
        let mut c = Content::new();
        c.save_state()
            .concat_matrix(Matrix::translate(10.0, 20.0))
            .set_line_width(2.5)
            .restore_state();
        assert_eq!(text(&c), "q\n1 0 0 1 10 20 cm\n2.5 w\nQ\n");
    }

    #[test]
    fn colors() {
        let mut c = Content::new();
        c.set_fill_rgb(1.0, 0.0, 0.5)
            .set_stroke_gray(0.25)
            .set_fill_cmyk(0.0, 1.0, 1.0, 0.0);
        assert_eq!(text(&c), "1 0 0.5 rg\n0.25 G\n0 1 1 0 k\n");
    }

    #[test]
    fn colors_are_clamped_to_unit_range() {
        let mut c = Content::new();
        c.set_fill_rgb(5.0, -2.0, 0.5)
            .set_stroke_gray(2.0)
            .set_fill_cmyk(-1.0, 1.5, 0.5, 0.0);
        assert_eq!(text(&c), "1 0 0.5 rg\n1 G\n0 1 0.5 0 k\n");
    }

    #[test]
    fn path_and_paint() {
        let mut c = Content::new();
        c.move_to(0.0, 0.0)
            .line_to(100.0, 0.0)
            .curve_to(110.0, 0.0, 120.0, 10.0, 120.0, 20.0)
            .rect(5.0, 5.0, 30.0, 40.0)
            .close_path()
            .fill();
        assert_eq!(
            text(&c),
            "0 0 m\n100 0 l\n110 0 120 10 120 20 c\n5 5 30 40 re\nh\nf\n"
        );
    }

    #[test]
    fn clipping() {
        let mut c = Content::new();
        c.rect(0.0, 0.0, 10.0, 10.0).clip().end_path();
        assert_eq!(text(&c), "0 0 10 10 re\nW\nn\n");
    }

    #[test]
    fn text_operators() {
        let mut c = Content::new();
        c.begin_text()
            .set_font("F0", 12.0)
            .set_leading(14.0)
            .set_text_matrix(Matrix::translate(72.0, 700.0))
            .show_text_adjusted(&[
                TextPart::Glyphs(vec![0x00, 0x24]),
                TextPart::Adjust(-25.0),
                TextPart::Glyphs(vec![0x00, 0x25]),
            ])
            .next_line()
            .show_text_hex(&[0x00, 0x26])
            .end_text();
        assert_eq!(
            text(&c),
            "BT\n/F0 12 Tf\n14 TL\n1 0 0 1 72 700 Tm\n[<0024>-25 <0025>] TJ\nT*\n<0026> Tj\nET\n"
        );
    }

    #[test]
    fn number_formatting() {
        let mut out = Vec::new();
        write_num(0.0, &mut out);
        write_num(-0.0, &mut out);
        out.push(b'|');
        write_num(1.5, &mut out);
        out.push(b'|');
        write_num(-42.0, &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "00|1.5|-42");
    }
}

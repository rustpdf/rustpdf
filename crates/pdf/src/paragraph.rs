//! High-level paragraph layout (Fase 3F): automatic line breaking inside a
//! box, horizontal alignment and inline styling. This is the L2 DX layer — the
//! caller gives text and a box; the engine shapes, measures, greedily breaks
//! lines and produces a positioned [`TextObject`].

use crate::font::{FontId, RegisteredFont};
use crate::text::{Rgb, Run, TextObject};
use fonts::{shape, Direction};

/// Horizontal alignment of paragraph lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
    Center,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Style {
    font: FontId,
    size: f64,
    fill: Option<Rgb>,
}

/// An inline span of text sharing one style.
#[derive(Debug, Clone)]
struct Span {
    text: String,
    style: Style,
}

/// A paragraph: a styled body laid out into a box at `(x, y)` with `width`.
///
/// `y` is the baseline of the first line; subsequent lines step down by
/// `leading`.
#[derive(Debug, Clone)]
pub struct Paragraph {
    x: f64,
    y: f64,
    width: f64,
    leading: f64,
    align: Align,
    default_style: Style,
    spans: Vec<Span>,
    tag: crate::tag::StructTag,
    ancestors: Vec<crate::tag::StructNode>,
    col: Option<usize>,
}

impl Paragraph {
    /// Start a paragraph with a default font and size.
    pub fn new(font: FontId, size: f64) -> Self {
        Paragraph {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            leading: size * 1.2,
            align: Align::Left,
            default_style: Style {
                font,
                size,
                fill: None,
            },
            spans: Vec::new(),
            tag: crate::tag::StructTag::P,
            ancestors: Vec::new(),
            col: None,
        }
    }

    /// Set the logical-structure role of this paragraph (e.g. a heading) for
    /// Tagged PDF / accessibility. Has no effect unless the document is tagged.
    pub fn tag(mut self, tag: crate::tag::StructTag) -> Self {
        self.tag = tag;
        self
    }

    /// Internal: set the grouping ancestors (used by the layout engine to nest
    /// table cells under `Table`/`TR`).
    pub(crate) fn with_ancestors(mut self, ancestors: Vec<crate::tag::StructNode>) -> Self {
        self.ancestors = ancestors;
        self
    }

    /// Internal: set the table column index (for `TH`/`TD` header association).
    pub(crate) fn with_col(mut self, col: usize) -> Self {
        self.col = Some(col);
        self
    }

    /// Position the paragraph box: `(x, y)` is the first baseline; `width` is
    /// the wrapping width in points.
    pub fn box_at(mut self, x: f64, y: f64, width: f64) -> Self {
        self.x = x;
        self.y = y;
        self.width = width;
        self
    }

    /// Set the line leading (baseline-to-baseline distance).
    pub fn leading(mut self, leading: f64) -> Self {
        self.leading = leading;
        self
    }

    /// Set horizontal alignment.
    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// Set the default fill color.
    pub fn fill(mut self, r: f64, g: f64, b: f64) -> Self {
        self.default_style.fill = Some((r, g, b));
        self
    }

    /// Append text in the default style.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.spans.push(Span {
            text: text.into(),
            style: self.default_style,
        });
        self
    }

    /// Append an inline-styled span (Fase 3F.3).
    pub fn span(
        mut self,
        text: impl Into<String>,
        font: FontId,
        size: f64,
        fill: Option<Rgb>,
    ) -> Self {
        self.spans.push(Span {
            text: text.into(),
            style: Style { font, size, fill },
        });
        self
    }

    /// Number of lines after wrapping (for flow-layout measurement).
    pub(crate) fn line_count(&self, fonts: &[RegisteredFont]) -> usize {
        self.break_lines(fonts).len()
    }

    /// Total height in points (`line_count × leading`).
    pub(crate) fn measured_height(&self, fonts: &[RegisteredFont]) -> f64 {
        self.line_count(fonts) as f64 * self.leading
    }

    // ---- layout internals --------------------------------------------------

    /// Lay the paragraph out into a positioned [`TextObject`].
    pub(crate) fn layout(&self, fonts: &[RegisteredFont]) -> TextObject {
        let lines = self.break_lines(fonts);
        let mut obj = TextObject::new(self.default_style.font, self.default_style.size);
        obj.tag = self.tag;
        obj.ancestors = self.ancestors.clone();
        obj.col = self.col;

        for (i, line) in lines.iter().enumerate() {
            let baseline_y = self.y - i as f64 * self.leading;
            let natural = line_width(line);
            let gaps = line
                .iter()
                .filter(|t| matches!(t, Tok::Space { .. }))
                .count();
            let is_last = i + 1 == lines.len();

            let x_start = match self.align {
                Align::Left | Align::Justify => self.x,
                Align::Right => self.x + (self.width - natural).max(0.0),
                Align::Center => self.x + (self.width - natural).max(0.0) / 2.0,
            };
            obj.at(x_start, baseline_y);

            let justify = self.align == Align::Justify && !is_last && gaps > 0;
            let extra_per_gap = if justify {
                (self.width - natural).max(0.0) / gaps as f64
            } else {
                0.0
            };

            emit_line(&mut obj, line, justify, extra_per_gap, fonts);
        }
        obj
    }

    /// Greedy line breaking. Returns lines, each a list of tokens.
    fn break_lines(&self, fonts: &[RegisteredFont]) -> Vec<Vec<Tok>> {
        let tokens = self.tokenize(fonts);
        let mut lines: Vec<Vec<Tok>> = Vec::new();
        let mut current: Vec<Tok> = Vec::new();
        let mut width = 0.0;
        let mut pending_space: Option<Tok> = None;

        for tok in tokens {
            match tok {
                Tok::Break => {
                    lines.push(std::mem::take(&mut current));
                    width = 0.0;
                    pending_space = None;
                }
                Tok::Space { .. } => {
                    if !current.is_empty() {
                        pending_space = Some(tok);
                    }
                }
                Tok::Word { w, .. } => {
                    let space_w = pending_space.as_ref().map(tok_width).unwrap_or(0.0);
                    if !current.is_empty() && width + space_w + w > self.width {
                        // Wrap: drop the pending space, start a new line.
                        lines.push(std::mem::take(&mut current));
                        width = 0.0;
                        pending_space = None;
                        current.push(tok);
                        width += w;
                    } else {
                        if let Some(sp) = pending_space.take() {
                            width += tok_width(&sp);
                            current.push(sp);
                        }
                        width += w;
                        current.push(tok);
                    }
                }
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
        lines
    }

    /// Split spans into measured word/space/break tokens.
    fn tokenize(&self, fonts: &[RegisteredFont]) -> Vec<Tok> {
        let mut out = Vec::new();
        for span in &self.spans {
            let mut first_segment = true;
            for segment in span.text.split('\n') {
                if !first_segment {
                    out.push(Tok::Break);
                }
                first_segment = false;
                for (i, word) in segment.split(' ').enumerate() {
                    if i > 0 {
                        out.push(Tok::Space {
                            style: span.style,
                            width: measure(fonts, span.style, " "),
                        });
                    }
                    if !word.is_empty() {
                        out.push(Tok::Word {
                            text: word.to_string(),
                            style: span.style,
                            w: measure(fonts, span.style, word),
                        });
                    }
                }
            }
        }
        out
    }
}

#[derive(Debug, Clone)]
enum Tok {
    Word { text: String, style: Style, w: f64 },
    Space { style: Style, width: f64 },
    Break,
}

fn tok_width(t: &Tok) -> f64 {
    match t {
        Tok::Word { w, .. } => *w,
        Tok::Space { width, .. } => *width,
        Tok::Break => 0.0,
    }
}

fn line_width(line: &[Tok]) -> f64 {
    line.iter().map(tok_width).sum()
}

/// Measure text width in points using shaped (kerned) advances.
fn measure(fonts: &[RegisteredFont], style: Style, text: &str) -> f64 {
    let reg = &fonts[style.font.0];
    let glyphs = shape(&reg.font, text, Direction::LeftToRight);
    let advance: i64 = glyphs.iter().map(|g| g.x_advance as i64).sum();
    advance as f64 / reg.font.units_per_em() as f64 * style.size
}

/// Emit one laid-out line into the text object.
fn emit_line(
    obj: &mut TextObject,
    line: &[Tok],
    justify: bool,
    extra_per_gap: f64,
    _f: &[RegisteredFont],
) {
    if justify {
        // Word runs separated by explicit position adjustments so the extra
        // space is distributed evenly (TJ negative number = move right).
        let mut last_size = 0.0;
        for tok in line {
            match tok {
                Tok::Word { text, style, .. } => {
                    last_size = style.size;
                    obj.push_run(Run {
                        text: text.clone(),
                        font: style.font,
                        size: style.size,
                        fill: style.fill,
                        rtl: false,
                        tag: None,
                    });
                }
                Tok::Space { width, .. } => {
                    let gap = width + extra_per_gap;
                    let size = if last_size > 0.0 { last_size } else { 1.0 };
                    obj.push_adjust(-(gap * 1000.0 / size));
                }
                Tok::Break => {}
            }
        }
    } else {
        // Merge consecutive same-style tokens (including spaces) into one run
        // so intra-run shaping/kerning is natural.
        let mut buf = String::new();
        let mut cur: Option<Style> = None;
        for tok in line {
            let (text, style) = match tok {
                Tok::Word { text, style, .. } => (text.as_str(), *style),
                Tok::Space { style, .. } => (" ", *style),
                Tok::Break => continue,
            };
            match cur {
                Some(s) if s == style => buf.push_str(text),
                _ => {
                    flush_run(obj, &mut buf, cur);
                    cur = Some(style);
                    buf.push_str(text);
                }
            }
        }
        flush_run(obj, &mut buf, cur);
    }
}

fn flush_run(obj: &mut TextObject, buf: &mut String, style: Option<Style>) {
    if let (false, Some(style)) = (buf.is_empty(), style) {
        obj.push_run(Run {
            text: std::mem::take(buf),
            font: style.font,
            size: style.size,
            fill: style.fill,
            rtl: false,
            tag: None,
        });
    }
    buf.clear();
}

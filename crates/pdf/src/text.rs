//! Structured text objects, resolved to content-stream bytes at serialization
//! time. Text is held symbolically (runs of characters with style) rather than
//! pre-encoded, because the glyph ids written into the stream are only known
//! after subsetting (see `crate::font`). At emit time each run is shaped
//! (Fase 3D.1), mapped to subset glyph ids and written as a `TJ` array carrying
//! kerning adjustments.

use std::collections::{BTreeMap, BTreeSet};

use fonts::{shape, Direction, Subset};
use graphics::{Content, Matrix, TextPart};

use crate::font::{FontId, FontUsage, RegisteredFont};

/// An RGB fill color, components in `0.0..=1.0`.
pub type Rgb = (f64, f64, f64);

/// A unit of text with its own style (Fase 3F.3 inline styling).
#[derive(Debug, Clone)]
pub(crate) struct Run {
    pub text: String,
    pub font: FontId,
    pub size: f64,
    pub fill: Option<Rgb>,
    pub rtl: bool,
    /// An inline structure role (Tagged PDF `/Span`/etc.): when set, this run is
    /// wrapped in its own nested marked-content sequence and gets its own
    /// structure element inside the block (Fase 7.5).
    pub tag: Option<crate::tag::StructTag>,
}

#[derive(Debug, Clone)]
pub(crate) enum Action {
    /// Set the text position to an absolute page coordinate (via `Tm`).
    MoveTo(f64, f64),
    /// Move to the next line using the current leading (`T*`).
    NewLine,
    /// Set leading / line height (`TL`).
    Leading(f64),
    /// Set character spacing (`Tc`).
    CharSpacing(f64),
    /// A styled run of text to shape and show.
    Run(Run),
    /// A bare `TJ` position adjustment (used for justification spacing).
    Adjust(f64),
}

/// A text object: a default style plus an ordered list of actions.
#[derive(Debug, Clone)]
pub struct TextObject {
    default_font: FontId,
    default_size: f64,
    default_fill: Option<Rgb>,
    pub(crate) actions: Vec<Action>,
    /// Logical-structure role for Tagged PDF (Fase 7.5); defaults to `P`.
    pub(crate) tag: crate::tag::StructTag,
    /// Grouping ancestors (e.g. a table cell's `[Table, TR]`).
    pub(crate) ancestors: Vec<crate::tag::StructNode>,
    /// For table cells: the column index, used to associate `TD` with its `TH`
    /// header via `/Headers`/`/ID` (PDF/UA 7.5).
    pub(crate) col: Option<usize>,
}

impl TextObject {
    pub(crate) fn new(font: FontId, size: f64) -> Self {
        TextObject {
            default_font: font,
            default_size: size,
            default_fill: None,
            actions: Vec::new(),
            tag: crate::tag::StructTag::P,
            ancestors: Vec::new(),
            col: None,
        }
    }

    /// Set the logical-structure role of this text block (e.g. a heading) for
    /// Tagged PDF / accessibility. Has no effect unless the document is tagged.
    pub fn tag(&mut self, tag: crate::tag::StructTag) -> &mut Self {
        self.tag = tag;
        self
    }

    /// Set the default fill color for subsequent `show` calls.
    pub fn fill(&mut self, r: f64, g: f64, b: f64) -> &mut Self {
        self.default_fill = Some((r, g, b));
        self
    }

    /// Position the text cursor at an absolute page coordinate (baseline).
    pub fn at(&mut self, x: f64, y: f64) -> &mut Self {
        self.actions.push(Action::MoveTo(x, y));
        self
    }

    /// Set line leading (height between baselines).
    pub fn leading(&mut self, leading: f64) -> &mut Self {
        self.actions.push(Action::Leading(leading));
        self
    }

    /// Set additional character spacing.
    pub fn char_spacing(&mut self, spacing: f64) -> &mut Self {
        self.actions.push(Action::CharSpacing(spacing));
        self
    }

    /// Move to the next line (requires a prior [`TextObject::leading`]).
    pub fn newline(&mut self) -> &mut Self {
        self.actions.push(Action::NewLine);
        self
    }

    /// Show text using the default font, size and fill.
    pub fn show(&mut self, text: impl Into<String>) -> &mut Self {
        let run = Run {
            text: text.into(),
            font: self.default_font,
            size: self.default_size,
            fill: self.default_fill,
            rtl: false,
            tag: None,
        };
        self.actions.push(Action::Run(run));
        self
    }

    /// Show text as an inline structure span (Tagged PDF, e.g.
    /// [`StructTag::Span`](crate::StructTag::Span)). The run gets its own nested
    /// marked content and structure element inside this block. No effect unless
    /// the document is tagged.
    pub fn show_span(&mut self, text: impl Into<String>, tag: crate::tag::StructTag) -> &mut Self {
        self.actions.push(Action::Run(Run {
            text: text.into(),
            font: self.default_font,
            size: self.default_size,
            fill: self.default_fill,
            rtl: false,
            tag: Some(tag),
        }));
        self
    }

    /// Show text with an explicit inline style (font/size/fill).
    pub fn show_styled(
        &mut self,
        text: impl Into<String>,
        font: FontId,
        size: f64,
        fill: Option<Rgb>,
    ) -> &mut Self {
        self.actions.push(Action::Run(Run {
            text: text.into(),
            font,
            size,
            fill,
            rtl: false,
            tag: None,
        }));
        self
    }

    /// Internal: push a pre-built run (used by paragraph layout).
    pub(crate) fn push_run(&mut self, run: Run) {
        self.actions.push(Action::Run(run));
    }

    /// Internal: push a bare position adjustment.
    pub(crate) fn push_adjust(&mut self, amount: f64) {
        self.actions.push(Action::Adjust(amount));
    }
}

fn direction(rtl: bool) -> Direction {
    if rtl {
        Direction::RightToLeft
    } else {
        Direction::LeftToRight
    }
}

/// Pass 1: record which glyphs each font uses and their Unicode mapping.
pub(crate) fn collect_usage(obj: &TextObject, fonts: &[RegisteredFont], usage: &mut [FontUsage]) {
    for action in &obj.actions {
        let Action::Run(run) = action else { continue };
        let glyphs = shape(&fonts[run.font.0].font, &run.text, direction(run.rtl));

        // Unique cluster boundaries to split source text per glyph.
        let mut boundaries: BTreeSet<usize> = glyphs.iter().map(|g| g.cluster as usize).collect();
        boundaries.insert(run.text.len());
        let bounds: Vec<usize> = boundaries.into_iter().collect();

        let u = &mut usage[run.font.0];
        for g in &glyphs {
            u.used_gids.insert(g.gid);
            let start = g.cluster as usize;
            let end = bounds
                .iter()
                .copied()
                .find(|&b| b > start)
                .unwrap_or(run.text.len());
            if start <= end && end <= run.text.len() {
                if let Some(slice) = run.text.get(start..end) {
                    u.gid_to_unicode
                        .entry(g.gid)
                        .or_insert_with(|| slice.to_string());
                }
            }
        }
    }
}

/// Pass 2: emit the text object into `content` using the computed subsets. When
/// `mcid` is `Some` (tagged), the whole block is wrapped in a marked-content
/// sequence with role `obj.tag` and a fresh MCID drawn from the counter; any run
/// carrying its own inline `tag` (e.g. `/Span`) is wrapped in a *nested* marked
/// content with its own MCID. Returns the `(tag, mcid)` of each inline span so
/// the caller can build the corresponding child structure elements (Fase 7.5).
pub(crate) fn emit(
    obj: &TextObject,
    content: &mut Content,
    fonts: &[RegisteredFont],
    subsets: &BTreeMap<FontId, Subset>,
    mut mcid: Option<&mut i32>,
) -> Vec<(crate::tag::StructTag, i32)> {
    let mut spans = Vec::new();
    let tagged = if let Some(ctr) = mcid.as_deref_mut() {
        let id = *ctr;
        *ctr += 1;
        content.begin_marked_content(obj.tag.name(), id);
        true
    } else {
        false
    };
    content.begin_text();
    let mut current_fill: Option<Rgb> = None;

    for action in &obj.actions {
        match action {
            Action::MoveTo(x, y) => {
                content.set_text_matrix(Matrix::translate(*x, *y));
            }
            Action::NewLine => {
                content.next_line();
            }
            Action::Leading(v) => {
                content.set_leading(*v);
            }
            Action::CharSpacing(v) => {
                content.set_char_spacing(*v);
            }
            Action::Adjust(a) => {
                content.show_text_adjusted(&[TextPart::Adjust(*a)]);
            }
            Action::Run(run) => {
                // Open a nested marked content for an inline-tagged run.
                let span_open = match (mcid.as_deref_mut(), run.tag) {
                    (Some(ctr), Some(stag)) => {
                        let id = *ctr;
                        *ctr += 1;
                        content.begin_marked_content(stag.name(), id);
                        spans.push((stag, id));
                        true
                    }
                    _ => false,
                };
                content.set_font(&run.font.resource_name(), run.size);
                // A run without an explicit fill is black; always emit the color
                // when it changes so default runs after a colored span reset.
                let fill = run.fill.unwrap_or((0.0, 0.0, 0.0));
                if Some(fill) != current_fill {
                    content.set_fill_rgb(fill.0, fill.1, fill.2);
                    current_fill = Some(fill);
                }
                emit_run(run, content, &fonts[run.font.0], &subsets[&run.font]);
                if span_open {
                    content.end_marked_content();
                }
            }
        }
    }
    content.end_text();
    if tagged {
        content.end_marked_content();
    }
    spans
}

fn emit_run(run: &Run, content: &mut Content, reg: &RegisteredFont, subset: &Subset) {
    let glyphs = shape(&reg.font, &run.text, direction(run.rtl));
    let upem = reg.font.units_per_em() as f64;

    let mut parts: Vec<TextPart> = Vec::new();
    let mut buf: Vec<u8> = Vec::new();
    for g in &glyphs {
        let new_gid = subset.new_gid(g.gid).unwrap_or(0);
        buf.push((new_gid >> 8) as u8);
        buf.push((new_gid & 0xFF) as u8);

        // Kerning: difference between the font's default advance and the
        // shaper's advance, in PDF text-space thousandths (positive = tighter).
        let raw = reg.font.advance(g.gid) as i32;
        let adjust = (raw - g.x_advance) as f64 * 1000.0 / upem;
        if adjust.abs() >= 0.5 {
            parts.push(TextPart::Glyphs(std::mem::take(&mut buf)));
            parts.push(TextPart::Adjust(adjust.round()));
        }
    }
    if !buf.is_empty() {
        parts.push(TextPart::Glyphs(buf));
    }
    content.show_text_adjusted(&parts);
}

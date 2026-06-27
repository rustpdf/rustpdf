//! High-level flow layout (Fase 7.6): a `Report` stacks blocks (headings,
//! paragraphs, tables, spacers) down the page, breaking to a new page when
//! content overflows, with an optional running header and page-number footer.
//!
//! It extends the L2 paragraph engine (Fase 3F): cells and body text reuse
//! [`Paragraph`] for wrapping/measuring.

use crate::paragraph::{Align, Paragraph};
use crate::tag::StructTag;
use crate::{Document, FontId};

/// A table with fixed column widths and wrapping text cells.
#[derive(Debug, Clone)]
pub struct Table {
    font: FontId,
    size: f64,
    columns: Vec<f64>,
    rows: Vec<Vec<String>>,
    header: bool,
    padding: f64,
}

impl Table {
    /// A table with the given column widths (points) drawn in `font`/`size`.
    pub fn new(font: FontId, size: f64, columns: Vec<f64>) -> Self {
        Table {
            font,
            size,
            columns,
            rows: Vec::new(),
            header: false,
            padding: 4.0,
        }
    }

    /// Add a row (extra cells are ignored, missing cells are blank).
    pub fn row<I, S>(mut self, cells: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.rows.push(cells.into_iter().map(Into::into).collect());
        self
    }

    /// Treat the first row as a shaded, repeated header row.
    pub fn header_row(mut self, yes: bool) -> Self {
        self.header = yes;
        self
    }

    fn total_width(&self) -> f64 {
        self.columns.iter().sum()
    }
}

#[derive(Debug, Clone)]
enum Block {
    Heading { text: String, size: f64, level: u8 },
    Paragraph(String, f64, Align),
    Table(Table),
    List(Vec<String>),
    Spacer(f64),
    PageBreak,
}

/// A flowing, auto-paginated document.
#[derive(Debug, Clone)]
pub struct Report {
    width: f64,
    height: f64,
    margin: f64,
    default_font: FontId,
    default_size: f64,
    line_spacing: f64,
    header: Option<String>,
    footer_pages: bool,
    blocks: Vec<Block>,
}

impl Report {
    /// A new report on A4 with 72-pt margins, body text in `font`.
    pub fn new(font: FontId) -> Self {
        Report {
            width: crate::sizes::A4.0,
            height: crate::sizes::A4.1,
            margin: 72.0,
            default_font: font,
            default_size: 11.0,
            line_spacing: 1.45,
            header: None,
            footer_pages: false,
            blocks: Vec::new(),
        }
    }

    /// Set the page size.
    pub fn page_size(mut self, size: (f64, f64)) -> Self {
        self.width = size.0;
        self.height = size.1;
        self
    }

    /// Set the uniform page margin (points).
    pub fn margin(mut self, margin: f64) -> Self {
        self.margin = margin;
        self
    }

    /// Set the default body font size.
    pub fn body_size(mut self, size: f64) -> Self {
        self.default_size = size;
        self
    }

    /// Add a running header drawn at the top of every page.
    pub fn header(mut self, text: impl Into<String>) -> Self {
        self.header = Some(text.into());
        self
    }

    /// Draw "página N" at the bottom of every page.
    pub fn page_numbers(mut self, yes: bool) -> Self {
        self.footer_pages = yes;
        self
    }

    /// Add a top-level heading block (structure level `H1`).
    pub fn heading(mut self, text: impl Into<String>, size: f64) -> Self {
        self.blocks.push(Block::Heading {
            text: text.into(),
            size,
            level: 1,
        });
        self
    }

    /// Add a heading at an explicit level (1–6 → `H1`–`H6`) for a correct
    /// document outline in Tagged PDF / accessibility (Fase 7.5).
    pub fn heading_level(mut self, text: impl Into<String>, size: f64, level: u8) -> Self {
        self.blocks.push(Block::Heading {
            text: text.into(),
            size,
            level,
        });
        self
    }

    /// Add a body paragraph (left-aligned at the body size).
    pub fn paragraph(mut self, text: impl Into<String>) -> Self {
        self.blocks.push(Block::Paragraph(
            text.into(),
            self.default_size,
            Align::Left,
        ));
        self
    }

    /// Add a justified body paragraph.
    pub fn paragraph_justified(mut self, text: impl Into<String>) -> Self {
        self.blocks.push(Block::Paragraph(
            text.into(),
            self.default_size,
            Align::Justify,
        ));
        self
    }

    /// Add vertical space (points).
    pub fn spacer(mut self, height: f64) -> Self {
        self.blocks.push(Block::Spacer(height));
        self
    }

    /// Force a page break.
    pub fn page_break(mut self) -> Self {
        self.blocks.push(Block::PageBreak);
        self
    }

    /// Add a table block.
    pub fn table(mut self, table: Table) -> Self {
        self.blocks.push(Block::Table(table));
        self
    }

    /// Add a bulleted list (`/L` → `/LI` → `/LBody` in Tagged PDF).
    pub fn list<I, S>(mut self, items: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.blocks
            .push(Block::List(items.into_iter().map(Into::into).collect()));
        self
    }

    /// Render the report into `doc`, creating as many pages as needed.
    pub fn render(self, doc: &mut Document) {
        let content_w = self.width - 2.0 * self.margin;
        let top = self.height - self.margin;
        let bottom = self.margin;

        let mut state = RenderState {
            page: usize::MAX,
            y: 0.0,
            pages: Vec::new(),
        };
        self.new_page(doc, &mut state, top);

        let mut table_seq: u64 = 0;
        let mut list_seq: u64 = 0;
        for block in &self.blocks {
            match block {
                Block::PageBreak => self.new_page(doc, &mut state, top),
                Block::Spacer(h) => state.y -= h,
                Block::Heading { text, size, level } => {
                    let para = self
                        .make_para(text, *size, Align::Left, content_w)
                        .tag(StructTag::heading(*level));
                    let h = para_height(doc, &para) + size * 0.4;
                    self.ensure_space(doc, &mut state, top, h, bottom);
                    let para = para.box_at(self.margin, state.y - size, content_w);
                    doc.pages[state.page].paragraph(para);
                    state.y -= h;
                }
                Block::Paragraph(text, size, align) => {
                    let para = self.make_para(text, *size, *align, content_w);
                    let h = para_height(doc, &para);
                    // Break before the paragraph if it doesn't fit (and isn't
                    // taller than a whole page, in which case place it anyway).
                    let usable = top - bottom;
                    if h <= usable {
                        self.ensure_space(doc, &mut state, top, h, bottom);
                    }
                    let para = para.box_at(self.margin, state.y - size, content_w);
                    doc.pages[state.page].paragraph(para);
                    state.y -= h + size * 0.5;
                }
                Block::Table(t) => {
                    self.draw_table(doc, &mut state, top, bottom, t, table_seq);
                    table_seq += 1;
                }
                Block::List(items) => {
                    self.draw_list(doc, &mut state, top, bottom, content_w, items, list_seq);
                    list_seq += 1;
                }
            }
        }

        if self.footer_pages || self.header.is_some() {
            self.stamp_running_elements(doc, &state);
        }
    }

    // ---- internals --------------------------------------------------------

    fn make_para(&self, text: &str, size: f64, align: Align, width: f64) -> Paragraph {
        Paragraph::new(self.default_font, size)
            .leading(size * self.line_spacing)
            .align(align)
            .box_at(self.margin, 0.0, width)
            .text(text)
    }

    fn new_page(&self, doc: &mut Document, state: &mut RenderState, top: f64) {
        doc.add_page_sized(self.width, self.height);
        state.page = doc.page_count() - 1;
        state.pages.push(state.page);
        // Leave room for the header line.
        state.y = if self.header.is_some() {
            top - 24.0
        } else {
            top
        };
    }

    fn ensure_space(
        &self,
        doc: &mut Document,
        state: &mut RenderState,
        top: f64,
        needed: f64,
        bottom: f64,
    ) {
        if state.y - needed < bottom {
            self.new_page(doc, state, top);
        }
    }

    fn draw_table(
        &self,
        doc: &mut Document,
        state: &mut RenderState,
        top: f64,
        bottom: f64,
        table: &Table,
        table_key: u64,
    ) {
        let x0 = self.margin;
        let leading = table.size * 1.3;

        let header = if table.header {
            table.rows.first()
        } else {
            None
        };
        // Monotonic row id (includes repeated headers) → unique `TR` per render.
        let mut row_seq: u64 = 0;
        let mut render_row =
            |doc: &mut Document, state: &mut RenderState, row: &[String], is_header: bool| {
                let row_key = row_seq;
                row_seq += 1;
                // Measure row height from the tallest wrapping cell.
                let mut row_h: f64 = leading + 2.0 * table.padding;
                for (i, w) in table.columns.iter().enumerate() {
                    let cell = row.get(i).map(String::as_str).unwrap_or("");
                    let p = Paragraph::new(table.font, table.size)
                        .leading(leading)
                        .box_at(0.0, 0.0, w - 2.0 * table.padding)
                        .text(cell);
                    row_h = row_h.max(para_height(doc, &p) + 2.0 * table.padding);
                }

                let row_top = state.y;
                let row_bottom = row_top - row_h;

                // Background for header rows + cell borders (one graphics segment).
                {
                    let page = &mut doc.pages[state.page];
                    let c = page.content();
                    let mut x = x0;
                    for w in &table.columns {
                        if is_header {
                            c.save_state()
                                .set_fill_rgb(0.92, 0.93, 0.96)
                                .rect(x, row_bottom, *w, row_h)
                                .fill()
                                .restore_state();
                        }
                        c.set_line_width(0.5)
                            .set_stroke_gray(0.6)
                            .rect(x, row_bottom, *w, row_h)
                            .stroke();
                        x += *w;
                    }
                }
                // Cell text. Each cell is a `TH`/`TD` structure element nested
                // under its `TR` and `Table` (Tagged PDF; ignored if untagged).
                let cell_tag = if is_header {
                    StructTag::TH
                } else {
                    StructTag::TD
                };
                let mut x = x0;
                for (i, w) in table.columns.iter().enumerate() {
                    let cell = row.get(i).map(String::as_str).unwrap_or("").to_string();
                    let fill = if is_header {
                        Some((0.1, 0.1, 0.2))
                    } else {
                        None
                    };
                    let mut p = Paragraph::new(table.font, table.size)
                        .leading(leading)
                        .tag(cell_tag)
                        .with_ancestors(vec![
                            (StructTag::Table, table_key),
                            (StructTag::TR, row_key),
                        ])
                        .with_col(i)
                        .box_at(
                            x + table.padding,
                            row_top - table.padding - table.size,
                            *w - 2.0 * table.padding,
                        );
                    if let Some((r, g, b)) = fill {
                        p = p.fill(r, g, b);
                    }
                    doc.pages[state.page].paragraph(p.text(cell));
                    x += *w;
                }
                state.y = row_bottom;
                row_h
            };

        for (idx, row) in table.rows.iter().enumerate() {
            let is_header = table.header && idx == 0;
            // Pre-measure this row to decide on a page break.
            let mut row_h: f64 = leading + 2.0 * table.padding;
            for (i, w) in table.columns.iter().enumerate() {
                let cell = row.get(i).map(String::as_str).unwrap_or("");
                let p = Paragraph::new(table.font, table.size)
                    .leading(leading)
                    .box_at(0.0, 0.0, w - 2.0 * table.padding)
                    .text(cell);
                row_h = row_h.max(para_height(doc, &p) + 2.0 * table.padding);
            }
            if state.y - row_h < bottom {
                self.new_page(doc, state, top);
                if let Some(h) = header {
                    if !is_header {
                        render_row(doc, state, h, true);
                    }
                }
            }
            render_row(doc, state, row, is_header);
        }
        let _ = table.total_width();
        state.y -= table.size * 0.5;
    }

    /// Draw a bulleted list. Each item nests `L → LI → LBody` in the structure
    /// tree (Tagged PDF) and is indented with a `•` label.
    #[allow(clippy::too_many_arguments)]
    fn draw_list(
        &self,
        doc: &mut Document,
        state: &mut RenderState,
        top: f64,
        bottom: f64,
        content_w: f64,
        items: &[String],
        list_key: u64,
    ) {
        let size = self.default_size;
        let indent = size * 1.4;
        let leading = size * self.line_spacing;
        for (i, item) in items.iter().enumerate() {
            let para = Paragraph::new(self.default_font, size)
                .leading(leading)
                .tag(StructTag::LBody)
                .with_ancestors(vec![(StructTag::L, list_key), (StructTag::LI, i as u64)])
                .box_at(0.0, 0.0, content_w - indent)
                .text(format!("•  {item}"));
            let h = para_height(doc, &para);
            if h <= top - bottom {
                self.ensure_space(doc, state, top, h, bottom);
            }
            let para = para.box_at(self.margin + indent, state.y - size, content_w - indent);
            doc.pages[state.page].paragraph(para);
            state.y -= h + size * 0.2;
        }
        state.y -= size * 0.4;
    }

    /// Draw the header line and footer page numbers on every created page.
    fn stamp_running_elements(&self, doc: &mut Document, state: &RenderState) {
        let total = state.pages.len();
        for (i, &page_idx) in state.pages.iter().enumerate() {
            if let Some(h) = &self.header {
                let para = Paragraph::new(self.default_font, 9.0)
                    .fill(0.4, 0.4, 0.4)
                    .box_at(
                        self.margin,
                        self.height - self.margin + 6.0,
                        self.width - 2.0 * self.margin,
                    )
                    .text(h.clone());
                doc.pages[page_idx].paragraph(para);
            }
            if self.footer_pages {
                let para = Paragraph::new(self.default_font, 9.0)
                    .align(Align::Center)
                    .fill(0.4, 0.4, 0.4)
                    .box_at(
                        self.margin,
                        self.margin - 14.0,
                        self.width - 2.0 * self.margin,
                    )
                    .text(format!("{} / {}", i + 1, total));
                doc.pages[page_idx].paragraph(para);
            }
        }
    }
}

struct RenderState {
    page: usize,
    y: f64,
    pages: Vec<usize>,
}

fn para_height(doc: &Document, p: &Paragraph) -> f64 {
    p.measured_height(&doc.fonts)
}

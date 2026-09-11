//! High-level, idiomatic PDF API (the `pdf` crate of `project.md`).
//!
//! This is the **rich, unrestricted** layer Rust users consume. It orchestrates
//! `cos` (objects), `writer` (file structure), `graphics` (content streams) and
//! `fonts` (parsing/shaping/subsetting) into a small page-oriented API. Nothing
//! here is `Rc`/`RefCell`, so a [`Document`] is `Send` and can be moved between
//! threads or built in a pool.

mod attach;
mod edit;
mod encrypt;
mod extract;
mod extract_image;
mod find;
mod flow;
mod font;
mod form;
mod geometry;
mod helvetica;
mod image;
mod inspect;
mod outline;
mod paragraph;
mod pdfa;
mod redact;
mod render;
mod sign;
mod tag;
mod tagtree;
mod text;
mod verify;

use std::collections::{BTreeMap, BTreeSet};

use cos::{Dict, Object, PdfString, Stream};
use writer::{Document as WriterDoc, PdfVersion};

pub use edit::{
    ConvertError, EditableDoc, ImageAnchor, StampSpace, VerticalAlign, VerticalAnchor,
    WatermarkOptions,
};
pub use encrypt::{Encryption, Permissions};
pub use extract::{extract_page_text, extract_text, page_text};
pub use extract_image::{extract_images, ExtractedImage, ImageFormat};
pub use find::{find_text, FindOptions, TextHit};
pub use flow::{Report, Table};
pub use font::FontId;
pub use fonts::FontError;
pub use geometry::{measure_page, measure_pages, PageGeometry, PdfRect};
pub use graphics::{Content, Matrix};
pub use image::ImageId;
pub use images::{Image, ImageError};
pub use inspect::{inspect, PdfOverview};
pub use outline::Bookmark;
pub use paragraph::{Align, Paragraph};
pub use parser::{PdfError, PdfReader};
pub use redact::RedactError;
pub use render::{
    page_count as render_page_count, render_page_rgba, render_page_rgba_with, render_page_to_png,
    render_page_to_png_with, PageRenderError, RenderOptions, RenderedPage,
};
pub use sign::{
    add_dss, begin_signing, begin_timestamp, complete_signing, sign, sign_with, timestamp,
    timestamp_request, timestamp_token_from_response, Certify, SignError, SignOptions,
    SignaturePolicy, Signer, SigningSession, VisibleSignature,
};
pub use tag::StructTag;
pub use text::{Rgb, TextObject};
pub use verify::{list_signatures, verify_signatures, SignatureField, SignatureReport};
pub use writer::PdfVersion as Version;

use font::{FontUsage, RegisteredFont};

/// Standard page sizes in PostScript points (1 pt = 1/72 inch).
pub mod sizes {
    /// ISO A4: 595.276 × 841.890 pt.
    pub const A4: (f64, f64) = (595.276, 841.890);
    /// ISO A3: 841.890 × 1190.551 pt.
    pub const A3: (f64, f64) = (841.890, 1190.551);
    /// US Letter: 612 × 792 pt.
    pub const LETTER: (f64, f64) = (612.0, 792.0);
    /// US Legal: 612 × 1008 pt.
    pub const LEGAL: (f64, f64) = (612.0, 1008.0);
}

/// Error produced while building a document.
#[derive(Debug)]
pub enum BuildError {
    /// A font could not be subset/embedded.
    Font(FontError),
    /// The low-level writer failed.
    Write(writer::WriteError),
    /// The original file could not be parsed (incremental update).
    Parse(String),
    /// The document is structurally invalid and cannot be serialized
    /// (e.g. no pages, or a PDF/A-4f profile with no embedded file).
    Invalid(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Font(e) => write!(f, "{e}"),
            BuildError::Write(e) => write!(f, "{e}"),
            BuildError::Parse(e) => write!(f, "{e}"),
            BuildError::Invalid(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<FontError> for BuildError {
    fn from(e: FontError) -> Self {
        BuildError::Font(e)
    }
}

impl From<writer::WriteError> for BuildError {
    fn from(e: writer::WriteError) -> Self {
        BuildError::Write(e)
    }
}

/// Document information (the `/Info` dictionary).
#[derive(Debug, Clone, Default)]
pub struct Info {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub creator: Option<String>,
}

/// One drawable element on a page, kept in z-order.
#[derive(Debug, Clone)]
enum PageItem {
    Graphics(Content),
    Text(TextObject),
    Paragraph(Paragraph),
    /// A meaningful image (Tagged PDF `/Figure` with alternate text), as
    /// opposed to a decorative one drawn via [`Page::draw_image`].
    Figure {
        id: usize,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        alt: String,
    },
}

/// Where a [`Page`] link annotation points.
#[derive(Debug, Clone)]
enum LinkTarget {
    /// An external URI (web navigation).
    Uri(String),
    /// Another page in this document (document navigation), optionally scrolled
    /// so `top` (page points from the bottom) is at the top of the view.
    Page { index: usize, top: Option<f64> },
}

/// A clickable `/Link` annotation rectangle on a page.
#[derive(Debug, Clone)]
struct LinkAnnot {
    rect: [f64; 4],
    target: LinkTarget,
}

/// A single page: its media box plus an ordered list of drawables.
#[derive(Debug, Clone)]
pub struct Page {
    width: f64,
    height: f64,
    items: Vec<PageItem>,
    used_images: BTreeSet<usize>,
    links: Vec<LinkAnnot>,
}

impl Page {
    fn new(width: f64, height: f64) -> Self {
        Page {
            width,
            height,
            items: Vec::new(),
            used_images: BTreeSet::new(),
            links: Vec::new(),
        }
    }

    /// Page width in points.
    pub fn width(&self) -> f64 {
        self.width
    }

    /// Page height in points.
    pub fn height(&self) -> f64 {
        self.height
    }

    /// Mutable access to a vector-graphics content builder. Consecutive calls
    /// extend the same graphics segment; a text block in between starts a new
    /// segment so z-order is preserved.
    pub fn content(&mut self) -> &mut Content {
        if !matches!(self.items.last(), Some(PageItem::Graphics(_))) {
            self.items.push(PageItem::Graphics(Content::new()));
        }
        match self.items.last_mut() {
            Some(PageItem::Graphics(c)) => c,
            _ => unreachable!("just ensured a graphics segment"),
        }
    }

    /// Begin a text block in `font` at `size` points; returns a builder.
    pub fn text(&mut self, font: FontId, size: f64) -> &mut TextObject {
        self.items.push(PageItem::Text(TextObject::new(font, size)));
        match self.items.last_mut() {
            Some(PageItem::Text(t)) => t,
            _ => unreachable!(),
        }
    }

    /// Place a laid-out paragraph (Fase 3F). The paragraph wraps and aligns
    /// itself within its box at serialization time.
    pub fn paragraph(&mut self, paragraph: Paragraph) -> &mut Self {
        self.items.push(PageItem::Paragraph(paragraph));
        self
    }

    /// Draw image `id` into the rectangle `(x, y)`–`(x+w, y+h)` (points), in
    /// the current graphics segment (Fase 4.2). In a tagged document this image
    /// is treated as **decorative** (marked as an `/Artifact`); use
    /// [`Page::figure`] for meaningful images that need alternate text.
    pub fn draw_image(&mut self, id: ImageId, x: f64, y: f64, w: f64, h: f64) -> &mut Self {
        self.used_images.insert(id.0);
        let resource = id.resource_name();
        self.content().draw_image(&resource, x, y, w, h);
        self
    }

    /// Place a **meaningful** image with alternate text (Tagged PDF `/Figure`
    /// with `/Alt`, Fase 7.5). In an untagged document this draws exactly like
    /// [`Page::draw_image`]; when tagged it becomes its own structure element so
    /// assistive technology can announce `alt`.
    pub fn figure(
        &mut self,
        id: ImageId,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        alt: impl Into<String>,
    ) -> &mut Self {
        self.used_images.insert(id.0);
        self.items.push(PageItem::Figure {
            id: id.0,
            x,
            y,
            w,
            h,
            alt: alt.into(),
        });
        self
    }

    /// Add a clickable **web link** over the rectangle `[x0, y0, x1, y1]`
    /// (page points) that opens `uri` in a browser (a `/Link` annotation with a
    /// `/URI` action). No visible border is drawn.
    pub fn link_uri(&mut self, rect: [f64; 4], uri: impl Into<String>) -> &mut Self {
        self.links.push(LinkAnnot {
            rect,
            target: LinkTarget::Uri(uri.into()),
        });
        self
    }

    /// Add a clickable **internal link** over `[x0, y0, x1, y1]` that jumps to
    /// page `page_index` (0-based). `top` optionally scrolls the destination so
    /// that y-coordinate (page points from the bottom) sits at the top of the
    /// view; `None` keeps the current scroll position.
    pub fn link_to_page(
        &mut self,
        rect: [f64; 4],
        page_index: usize,
        top: Option<f64>,
    ) -> &mut Self {
        self.links.push(LinkAnnot {
            rect,
            target: LinkTarget::Page {
                index: page_index,
                top,
            },
        });
        self
    }
}

/// A PDF/A conformance level (archival profile).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfaLevel {
    /// PDF/A-1b — basic, based on PDF 1.4 (no transparency, no object streams).
    A1b,
    /// PDF/A-2b — basic (PDF 1.7; transparency and object streams allowed).
    A2b,
    /// PDF/A-2a — accessible (tagged) level A.
    A2a,
    /// PDF/A-3b — like A-2b but permits arbitrary embedded file attachments.
    A3b,
    /// PDF/A-3a — accessible (tagged) A-3.
    A3a,
    /// PDF/A-4 (ISO 19005-4) — based on **PDF 2.0**. No A/B/U conformance
    /// letters; tagging is optional. Object/xref streams are allowed.
    A4,
    /// PDF/A-4e — the "engineering" conformance, intended for documents with
    /// 3D/rich-media annotations (`/AFRelationship`-style data). Same as A-4
    /// here plus the `pdfaid:conformance=E` marker.
    A4e,
    /// PDF/A-4f — permits arbitrary embedded file attachments (the PDF/A-4
    /// analogue of A-3), marked with `pdfaid:conformance=F`.
    A4f,
}

impl PdfaLevel {
    /// The PDF/A part number (1, 2, 3 or 4).
    fn part(self) -> u8 {
        match self {
            PdfaLevel::A1b => 1,
            PdfaLevel::A2b | PdfaLevel::A2a => 2,
            PdfaLevel::A3b | PdfaLevel::A3a => 3,
            PdfaLevel::A4 | PdfaLevel::A4e | PdfaLevel::A4f => 4,
        }
    }

    /// The amendment revision year, for part 4 (`pdfaid:rev`). PDF/A-1/2/3 use
    /// `pdfaid:conformance` instead and return `None` here.
    fn rev(self) -> Option<u16> {
        match self {
            PdfaLevel::A4 | PdfaLevel::A4e | PdfaLevel::A4f => Some(2020),
            _ => None,
        }
    }

    /// The conformance level marker: parts 1–3 use `'A'` (accessible/tagged) or
    /// `'B'` (basic); part 4 uses `'E'`/`'F'` for the engineering/embedded-file
    /// variants and `None` for the base level.
    fn conformance(self) -> Option<char> {
        match self {
            PdfaLevel::A2a | PdfaLevel::A3a => Some('A'),
            PdfaLevel::A1b | PdfaLevel::A2b | PdfaLevel::A3b => Some('B'),
            PdfaLevel::A4 => None,
            PdfaLevel::A4e => Some('E'),
            PdfaLevel::A4f => Some('F'),
        }
    }

    /// Whether this level requires a tagged structure tree (PDF/A level A only).
    fn tagged(self) -> bool {
        matches!(self, PdfaLevel::A2a | PdfaLevel::A3a)
    }
}

/// A file attached to the document (PDF/A-3 / general embedded files).
#[derive(Debug, Clone)]
struct Attachment {
    name: String,
    mime: String,
    data: Vec<u8>,
    desc: String,
    relationship: &'static str,
}

/// The relationship an embedded file has to the document (PDF/A-3, `/AFRelationship`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AFRelationship {
    /// The source material (e.g. the spreadsheet a report was generated from).
    Source,
    /// Data used to produce the visual content.
    Data,
    /// An alternative representation.
    Alternative,
    /// Supplementary material.
    Supplement,
    /// Unspecified relationship.
    Unspecified,
}

impl AFRelationship {
    fn name(self) -> &'static str {
        match self {
            AFRelationship::Source => "Source",
            AFRelationship::Data => "Data",
            AFRelationship::Alternative => "Alternative",
            AFRelationship::Supplement => "Supplement",
            AFRelationship::Unspecified => "Unspecified",
        }
    }
}

/// A PDF document under construction.
#[derive(Debug)]
pub struct Document {
    version: PdfVersion,
    default_size: (f64, f64),
    pages: Vec<Page>,
    info: Info,
    fonts: Vec<RegisteredFont>,
    images: Vec<Image>,
    pdfa: Option<PdfaLevel>,
    tagged: bool,
    attachments: Vec<Attachment>,
    form_fields: Vec<form::FormField>,
    bookmarks: Vec<Bookmark>,
    facturx: Option<pdfa::ZugferdXmp>,
}

/// A ZUGFeRD / Factur-X conformance profile (the level of structured detail in
/// the embedded XML invoice).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacturxProfile {
    /// Minimal header data only.
    Minimum,
    /// Basic, without line items.
    BasicWl,
    /// Basic, with line items.
    Basic,
    /// The EN 16931 ("Comfort") semantic core — the common interoperable level.
    En16931,
    /// EN 16931 plus extensions.
    Extended,
}

impl FacturxProfile {
    /// The conformance level string written into the Factur-X XMP.
    fn conformance(self) -> &'static str {
        match self {
            FacturxProfile::Minimum => "MINIMUM",
            FacturxProfile::BasicWl => "BASIC WL",
            FacturxProfile::Basic => "BASIC",
            FacturxProfile::En16931 => "EN 16931",
            FacturxProfile::Extended => "EXTENDED",
        }
    }
}

impl Default for Document {
    fn default() -> Self {
        Document::new()
    }
}

impl Document {
    /// A new, empty document defaulting to A4 pages and PDF 1.7.
    pub fn new() -> Self {
        Document {
            version: PdfVersion::V1_7,
            default_size: sizes::A4,
            pages: Vec::new(),
            info: Info::default(),
            fonts: Vec::new(),
            images: Vec::new(),
            pdfa: None,
            tagged: false,
            attachments: Vec::new(),
            form_fields: Vec::new(),
            bookmarks: Vec::new(),
            facturx: None,
        }
    }

    /// Emit a **PDF/A-2b** conformant file: an sRGB `OutputIntent`, XMP metadata
    /// with the PDF/A identifier, and a document `/ID` (Fase 7.4). Fonts are
    /// already embedded/subsetted, so the result validates with veraPDF.
    pub fn pdfa(mut self) -> Self {
        self.pdfa = Some(PdfaLevel::A2b);
        self
    }

    /// Emit a PDF/A file at an explicit [`PdfaLevel`] (A-1b … A-3a). Level-A
    /// variants also enable the tagged structure tree.
    pub fn pdfa_with(mut self, level: PdfaLevel) -> Self {
        self.pdfa = Some(level);
        if level.tagged() {
            self.tagged = true;
        }
        self
    }

    /// Emit a **Tagged PDF** (logical structure tree, marked content, role map)
    /// — accessibility (Fase 7.5). Combined with [`Document::pdfa`] this yields
    /// **PDF/A-2a** (level A), which veraPDF validates.
    pub fn tagged(mut self) -> Self {
        self.tagged = true;
        self
    }

    /// Convenience: a tagged, **PDF/A-2a** (accessible) document.
    pub fn pdfa_a(self) -> Self {
        self.pdfa_with(PdfaLevel::A2a)
    }

    /// Emit a **PDF/A-4** conformant file (ISO 19005-4, based on **PDF 2.0**):
    /// the header becomes `%PDF-2.0`, the XMP carries `pdfaid:part=4` +
    /// `pdfaid:rev=2020`, plus the sRGB `OutputIntent` and document `/ID`.
    pub fn pdfa4(self) -> Self {
        self.pdfa_with(PdfaLevel::A4)
    }

    /// Convenience: **PDF/A-4f** — PDF/A-4 that permits arbitrary embedded file
    /// attachments (use with [`Document::attach_file`]).
    pub fn pdfa4f(self) -> Self {
        self.pdfa_with(PdfaLevel::A4f)
    }

    /// Convenience: **PDF/A-4e** — the engineering conformance variant.
    pub fn pdfa4e(self) -> Self {
        self.pdfa_with(PdfaLevel::A4e)
    }

    /// Attach a file to the document (an embedded file with an
    /// `/AFRelationship`). Required for meaningful **PDF/A-3**; also works in
    /// ordinary PDFs. `mime` is the MIME type (e.g. `"text/csv"`).
    pub fn attach_file(
        &mut self,
        name: impl Into<String>,
        mime: impl Into<String>,
        data: impl Into<Vec<u8>>,
        relationship: AFRelationship,
        description: impl Into<String>,
    ) -> &mut Self {
        self.attachments.push(Attachment {
            name: name.into(),
            mime: mime.into(),
            data: data.into(),
            desc: description.into(),
            relationship: relationship.name(),
        });
        self
    }

    /// Add a top-level **bookmark** (document outline entry). Build a nested
    /// tree with [`Bookmark::child`]; viewers show the outline pane and (because
    /// a document with bookmarks sets `/PageMode /UseOutlines`) open it by
    /// default.
    pub fn add_bookmark(&mut self, bookmark: Bookmark) -> &mut Self {
        self.bookmarks.push(bookmark);
        self
    }

    /// Make this a **ZUGFeRD / Factur-X** electronic invoice: embed `xml` (the
    /// Cross-Industry Invoice) as `factur-x.xml`, mark the document **PDF/A-3**,
    /// and add the Factur-X identification to the XMP metadata at `profile`. The
    /// visual PDF *is* the human-readable invoice; the embedded XML is the
    /// machine-readable twin. Validates as PDF/A-3 + Factur-X under veraPDF.
    pub fn facturx(&mut self, xml: impl Into<Vec<u8>>, profile: FacturxProfile) -> &mut Self {
        const FILENAME: &str = "factur-x.xml";
        self.pdfa = Some(PdfaLevel::A3b);
        self.attach_file(
            FILENAME,
            "text/xml",
            xml,
            AFRelationship::Alternative,
            "Factur-X invoice",
        );
        self.facturx = Some(pdfa::ZugferdXmp {
            filename: FILENAME.to_string(),
            document_type: "INVOICE".to_string(),
            version: "1.0".to_string(),
            conformance: profile.conformance().to_string(),
        });
        self
    }

    // ---- interactive forms (AcroForm, Fase 6.7) ---------------------------

    /// Add a text field on `page` (0-based) in `rect` `[x0,y0,x1,y1]` with an
    /// initial `value`, drawn at `size` pt (0 = auto). A `/AP` appearance is
    /// generated so it renders without `NeedAppearances`.
    pub fn text_field(
        &mut self,
        name: impl Into<String>,
        page: usize,
        rect: [f64; 4],
        value: impl Into<String>,
        size: f64,
    ) -> &mut Self {
        self.form_fields.push(form::FormField {
            name: name.into(),
            page,
            size,
            kind: form::FieldKind::Text {
                value: value.into(),
                multiline: false,
                rect,
            },
        });
        self
    }

    /// Add a checkbox on `page` in `rect`, initially `checked` or not.
    pub fn checkbox(
        &mut self,
        name: impl Into<String>,
        page: usize,
        rect: [f64; 4],
        checked: bool,
    ) -> &mut Self {
        self.form_fields.push(form::FormField {
            name: name.into(),
            page,
            size: 0.0,
            kind: form::FieldKind::Checkbox { checked, rect },
        });
        self
    }

    /// Add a radio-button group: one field, `buttons` of `(rect, export_value)`,
    /// with optionally one `selected` index.
    pub fn radio_group(
        &mut self,
        name: impl Into<String>,
        page: usize,
        buttons: Vec<([f64; 4], String)>,
        selected: Option<usize>,
    ) -> &mut Self {
        self.form_fields.push(form::FormField {
            name: name.into(),
            page,
            size: 0.0,
            kind: form::FieldKind::Radio { selected, buttons },
        });
        self
    }

    /// Add a dropdown (choice) field on `page` in `rect` with `options` and an
    /// optional `selected` index. `combo` makes it an editable combo box.
    pub fn dropdown(
        &mut self,
        name: impl Into<String>,
        page: usize,
        rect: [f64; 4],
        options: Vec<String>,
        selected: Option<usize>,
        size: f64,
    ) -> &mut Self {
        self.form_fields.push(form::FormField {
            name: name.into(),
            page,
            size,
            kind: form::FieldKind::Choice {
                rect,
                options,
                selected,
                combo: true,
            },
        });
        self
    }

    /// Override the default page size for subsequently added pages.
    pub fn with_default_size(mut self, size: (f64, f64)) -> Self {
        self.default_size = size;
        self
    }

    /// Set the PDF version written into the header.
    pub fn with_version(mut self, version: PdfVersion) -> Self {
        self.version = version;
        self
    }

    /// Set the document information dictionary.
    pub fn set_info(&mut self, info: Info) {
        self.info = info;
    }

    /// Register a font from raw TrueType/OpenType bytes; returns its id.
    pub fn add_font(&mut self, data: impl Into<Vec<u8>>) -> Result<FontId, FontError> {
        let font = fonts::Font::from_bytes(data, 0)?;
        self.fonts.push(RegisteredFont::new(font));
        Ok(FontId(self.fonts.len() - 1))
    }

    /// Register a font from a file path; returns its id.
    pub fn add_font_file(
        &mut self,
        path: impl AsRef<std::path::Path>,
    ) -> Result<FontId, FontError> {
        let font = fonts::Font::from_file(path)?;
        self.fonts.push(RegisteredFont::new(font));
        Ok(FontId(self.fonts.len() - 1))
    }

    /// Register a pre-built image; returns its id.
    pub fn add_image(&mut self, image: Image) -> ImageId {
        self.images.push(image);
        ImageId(self.images.len() - 1)
    }

    /// Register a JPEG (embedded verbatim via `DCTDecode`).
    pub fn add_image_jpeg(&mut self, data: impl Into<Vec<u8>>) -> Result<ImageId, ImageError> {
        Ok(self.add_image(Image::from_jpeg(data)?))
    }

    /// Register a PNG (decoded and re-encoded as `FlateDecode`).
    pub fn add_image_png(&mut self, data: impl AsRef<[u8]>) -> Result<ImageId, ImageError> {
        Ok(self.add_image(Image::from_png(data)?))
    }

    /// Register an image from a file (JPEG or PNG, by signature).
    pub fn add_image_file(
        &mut self,
        path: impl AsRef<std::path::Path>,
    ) -> Result<ImageId, ImageError> {
        Ok(self.add_image(Image::from_file(path)?))
    }

    /// Add a page using the document default size; returns a mutable handle.
    pub fn add_page(&mut self) -> &mut Page {
        let (w, h) = self.default_size;
        self.add_page_sized(w, h)
    }

    /// Add a page of an explicit size; returns a mutable handle.
    pub fn add_page_sized(&mut self, width: f64, height: f64) -> &mut Page {
        self.pages.push(Page::new(width, height));
        self.pages.last_mut().expect("just pushed")
    }

    /// Number of pages added so far.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Number of registered fonts.
    pub fn font_count(&self) -> usize {
        self.fonts.len()
    }

    /// Mutable handle to the most recently added page, if any.
    pub fn last_page_mut(&mut self) -> Option<&mut Page> {
        self.pages.last_mut()
    }

    /// Serialize the document to PDF bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, BuildError> {
        // A valid PDF must have at least one page; an empty /Pages tree is
        // rejected by qpdf/mutool ("malformed page tree").
        if self.pages.is_empty() {
            return Err(BuildError::Invalid(
                "document has no pages; add at least one page before serializing".into(),
            ));
        }

        // PDF/A-4f (ISO 19005-4) requires at least one embedded file; without
        // one veraPDF reports the file non-conformant.
        if self.pdfa == Some(PdfaLevel::A4f) && self.attachments.is_empty() {
            return Err(BuildError::Invalid(
                "PDF/A-4f requires at least one embedded file; call attach_file() first".into(),
            ));
        }

        // Form fields and internal links must reference an existing page, and
        // every rectangle must have positive area. Without these checks a field
        // on a non-existent page would be created but attached to no page (an
        // invisible orphan), and a degenerate rectangle would render an
        // invisible/inert widget or link — both silent, surprising failures.
        let npages = self.pages.len();
        for f in &self.form_fields {
            if f.page >= npages {
                return Err(BuildError::Invalid(format!(
                    "form field {:?} targets page index {} but the document has {} page(s)",
                    f.name, f.page, npages
                )));
            }
            for rect in f.rects() {
                if !form::rect_is_valid(&rect) {
                    return Err(BuildError::Invalid(format!(
                        "form field {:?} has a degenerate rectangle {:?} \
                         (need x1 > x0 and y1 > y0)",
                        f.name, rect
                    )));
                }
            }
        }
        for (i, page) in self.pages.iter().enumerate() {
            for link in &page.links {
                if !form::rect_is_valid(&link.rect) {
                    return Err(BuildError::Invalid(format!(
                        "link on page {} has a degenerate rectangle {:?} \
                         (need x1 > x0 and y1 > y0)",
                        i, link.rect
                    )));
                }
                if let LinkTarget::Page { index, .. } = &link.target {
                    if *index >= npages {
                        return Err(BuildError::Invalid(format!(
                            "internal link on page {} targets page index {} \
                             but the document has {} page(s)",
                            i, index, npages
                        )));
                    }
                }
            }
        }

        // Step 0: resolve each page's items, laying out paragraphs (needs the
        // font metrics) into positioned text objects. Owning the result lets us
        // borrow `self.fonts` freely afterwards.
        let mut resolved: Vec<Vec<Drawable>> = Vec::with_capacity(self.pages.len());
        for page in &self.pages {
            let mut drawables = Vec::with_capacity(page.items.len());
            for item in &page.items {
                drawables.push(match item {
                    PageItem::Graphics(c) => Drawable::Graphics(c.as_bytes().to_vec()),
                    PageItem::Text(t) => Drawable::Text(t.clone()),
                    PageItem::Paragraph(p) => Drawable::Text(p.layout(&self.fonts)),
                    PageItem::Figure {
                        id,
                        x,
                        y,
                        w,
                        h,
                        alt,
                    } => Drawable::Figure {
                        id: *id,
                        x: *x,
                        y: *y,
                        w: *w,
                        h: *h,
                        alt: alt.clone(),
                    },
                });
            }
            resolved.push(drawables);
        }

        // Step 1: collect glyph usage across every text object (pass 1).
        let mut usage: Vec<FontUsage> = vec![FontUsage::default(); self.fonts.len()];
        for page in &resolved {
            for d in page {
                if let Drawable::Text(obj) = d {
                    text::collect_usage(obj, &self.fonts, &mut usage);
                }
            }
        }

        // PDF/A-1 is based on PDF 1.4; PDF/A-4 is based on PDF 2.0. Other levels
        // keep the caller-chosen version.
        let version = match self.pdfa {
            Some(PdfaLevel::A1b) => PdfVersion::V1_4,
            Some(PdfaLevel::A4 | PdfaLevel::A4e | PdfaLevel::A4f) => PdfVersion::V2_0,
            _ => self.version,
        };
        // `/CIDSet` is required in PDF/A-1/2/3 font descriptors; PDF 2.0 (hence
        // PDF/A-4) deprecates it, so it is not emitted there.
        let need_cidset = matches!(self.pdfa, Some(l) if l.part() < 4);
        let mut doc = WriterDoc::new(version);
        let catalog_ref = doc.reserve();
        let pages_ref = doc.reserve();

        // Step 2: subset + build font objects for every used font.
        let mut subsets: BTreeMap<FontId, fonts::Subset> = BTreeMap::new();
        let mut font_refs: BTreeMap<FontId, cos::Reference> = BTreeMap::new();
        for (i, reg) in self.fonts.iter().enumerate() {
            if !usage[i].is_used() {
                continue;
            }
            let (subset, type0_ref) =
                font::build_font(&mut doc, &reg.font, &usage[i], i, need_cidset)?;
            subsets.insert(FontId(i), subset);
            font_refs.insert(FontId(i), type0_ref);
        }

        // Shared font resource dictionary (every used font, by /F{id}).
        let font_resources = if font_refs.is_empty() {
            None
        } else {
            let mut d = Dict::new();
            for (id, r) in &font_refs {
                d.set(id.resource_name(), *r);
            }
            Some(d)
        };

        // Step 2b: build XObjects for every image used by any page.
        let used_images: BTreeSet<usize> = self
            .pages
            .iter()
            .flat_map(|p| p.used_images.iter().copied())
            .collect();
        let mut image_refs: BTreeMap<usize, cos::Reference> = BTreeMap::new();
        for &i in &used_images {
            let r = image::build_image(&mut doc, &self.images[i]);
            image_refs.insert(i, r);
        }

        // Step 3: build each page's content stream and dictionary (pass 2).
        // When tagged, wrap text blocks in their structure role's marked content,
        // figures in `/Figure` (with `/Alt`) and decorative graphics in
        // `/Artifact`, collecting leaves for the structure tree (Fase 7.5).
        let tagged = self.tagged;
        let mut tree = tagtree::TreeBuilder::default();
        type PendingLeaf = (
            tag::StructTag,
            i32,
            Vec<tag::StructNode>,
            Option<String>,
            Option<usize>,
            Vec<(tag::StructTag, i32)>,
        );

        // Reserve form-widget refs up front so each page can list them in
        // `/Annots`; the AcroForm itself is assembled after the page loop.
        let mut form_builder = form::FormBuilder::new(&self.form_fields);
        let page_widgets = form_builder.reserve(&mut doc, self.pages.len());

        let mut page_refs: Vec<cos::Reference> = Vec::with_capacity(self.pages.len());
        let mut kids = Vec::with_capacity(self.pages.len());
        for (page_index, (page, drawables)) in self.pages.iter().zip(resolved.iter()).enumerate() {
            let mut content = Content::new();
            let mut mcid: i32 = 0;
            let mut pending: Vec<PendingLeaf> = Vec::new();
            for d in drawables {
                match d {
                    Drawable::Graphics(bytes) => {
                        if tagged {
                            content.begin_artifact();
                            content.append_raw(bytes);
                            content.end_marked_content();
                        } else {
                            content.append_raw(bytes);
                        }
                    }
                    Drawable::Text(obj) => {
                        if tagged {
                            let block_mcid = mcid;
                            let spans = text::emit(
                                obj,
                                &mut content,
                                &self.fonts,
                                &subsets,
                                Some(&mut mcid),
                            );
                            pending.push((
                                obj.tag,
                                block_mcid,
                                obj.ancestors.clone(),
                                None,
                                obj.col,
                                spans,
                            ));
                        } else {
                            text::emit(obj, &mut content, &self.fonts, &subsets, None);
                        }
                    }
                    Drawable::Figure {
                        id,
                        x,
                        y,
                        w,
                        h,
                        alt,
                    } => {
                        let resource = ImageId(*id).resource_name();
                        if tagged {
                            content.begin_marked_content("Figure", mcid);
                            content.draw_image(&resource, *x, *y, *w, *h);
                            content.end_marked_content();
                            pending.push((
                                tag::StructTag::Figure,
                                mcid,
                                Vec::new(),
                                Some(alt.clone()),
                                None,
                                Vec::new(),
                            ));
                            mcid += 1;
                        } else {
                            content.draw_image(&resource, *x, *y, *w, *h);
                        }
                    }
                }
            }
            let content_ref = doc.add(Stream::with_dict(Dict::new(), content.into_bytes()));

            let mut resources = Dict::new();
            if let Some(fr) = &font_resources {
                resources.set("Font", Object::Dict(fr.clone()));
            }
            if !page.used_images.is_empty() {
                let mut xobjects = Dict::new();
                for &i in &page.used_images {
                    if let Some(r) = image_refs.get(&i) {
                        xobjects.set(ImageId(i).resource_name(), *r);
                    }
                }
                resources.set("XObject", Object::Dict(xobjects));
            }

            let mut page_dict = Dict::new()
                .with("Type", Object::name("Page"))
                .with("Parent", pages_ref)
                .with(
                    "MediaBox",
                    Object::Array(vec![
                        Object::Integer(0),
                        Object::Integer(0),
                        Object::Real(page.width),
                        Object::Real(page.height),
                    ]),
                )
                .with("Resources", Object::Dict(resources))
                .with("Contents", content_ref);
            if tagged {
                page_dict.set("StructParents", page_index as i64);
                page_dict.set("Tabs", Object::name("S"));
            }
            if let Some(widgets) = page_widgets.get(page_index) {
                if !widgets.is_empty() {
                    page_dict.set(
                        "Annots",
                        Object::Array(widgets.iter().copied().map(Object::Reference).collect()),
                    );
                }
            }
            let page_ref = doc.add(page_dict);
            page_refs.push(page_ref);
            kids.push(Object::Reference(page_ref));

            // Now that the page reference exists, record this page's leaves for
            // the structure tree (built once, with nesting, after all pages).
            if tagged {
                for (leaf_tag, leaf_mcid, ancestors, alt, col, spans) in pending {
                    tree.push(tagtree::Leaf {
                        tag: leaf_tag,
                        page: page_ref,
                        mcid: leaf_mcid,
                        ancestors,
                        alt,
                        col,
                        spans,
                        page_index,
                    });
                }
            }
        }

        // Link annotations (Tier 1): resolved now that every page reference is
        // known, so an internal `/Dest` can point at its target page. They are
        // merged into each page's `/Annots` alongside any form widgets.
        for (page_index, page) in self.pages.iter().enumerate() {
            if page.links.is_empty() {
                continue;
            }
            let mut link_refs: Vec<cos::Reference> = Vec::with_capacity(page.links.len());
            for link in &page.links {
                let mut annot = Dict::new()
                    .with("Type", Object::name("Annot"))
                    .with("Subtype", Object::name("Link"))
                    .with("Rect", rect_array(link.rect))
                    .with(
                        "Border",
                        Object::Array(vec![
                            Object::Integer(0),
                            Object::Integer(0),
                            Object::Integer(0),
                        ]),
                    );
                match &link.target {
                    LinkTarget::Uri(uri) => {
                        annot.set(
                            "A",
                            Object::Dict(
                                Dict::new()
                                    .with("S", Object::name("URI"))
                                    .with("URI", PdfString::literal(uri.clone().into_bytes())),
                            ),
                        );
                    }
                    LinkTarget::Page { index, top } => {
                        if let Some(&target_ref) = page_refs.get(*index) {
                            let top_obj = top.map(Object::Real).unwrap_or(Object::Null);
                            annot.set(
                                "Dest",
                                Object::Array(vec![
                                    Object::Reference(target_ref),
                                    Object::name("XYZ"),
                                    Object::Null,
                                    top_obj,
                                    Object::Null,
                                ]),
                            );
                        }
                    }
                }
                link_refs.push(doc.add(Object::Dict(annot)));
            }
            let page_ref = page_refs[page_index];
            doc.patch(page_ref, move |d| {
                let mut annots = match d.get("Annots") {
                    Some(Object::Array(a)) => a.clone(),
                    _ => Vec::new(),
                };
                annots.extend(link_refs.into_iter().map(Object::Reference));
                d.set("Annots", Object::Array(annots));
            });
        }

        let count = kids.len() as i64;
        doc.assign(
            pages_ref,
            Dict::new()
                .with("Type", Object::name("Pages"))
                .with("Kids", Object::Array(kids))
                .with("Count", count),
        );
        let mut catalog = Dict::new()
            .with("Type", Object::name("Catalog"))
            .with("Pages", pages_ref);

        // PDF 2.0: record the version in the catalog as well (it overrides the
        // header), so downstream consumers see 2.0 even after an incremental
        // update that keeps the original header.
        if version == PdfVersion::V2_0 {
            catalog.set("Version", Object::name("2.0"));
        }

        // Tagged PDF structure tree (Fase 7.5) → required for PDF/A level A.
        if tagged {
            let (structtree_ref, _) = tree.build(&mut doc, self.pages.len());
            catalog.set(
                "MarkInfo",
                Object::Dict(Dict::new().with("Marked", Object::Bool(true))),
            );
            catalog.set("StructTreeRoot", structtree_ref);
            catalog.set("Lang", PdfString::literal(b"en-US".to_vec()));
            // PDF/UA: viewers must display the document title, not the file name.
            catalog.set(
                "ViewerPreferences",
                Object::Dict(Dict::new().with("DisplayDocTitle", Object::Bool(true))),
            );
        }

        // Document outline / bookmarks (Tier 1): a nested `/Outlines` tree whose
        // destinations point at the now-known page references.
        if !self.bookmarks.is_empty() {
            let outlines_ref = outline::build(&mut doc, &page_refs, &self.bookmarks);
            catalog.set("Outlines", outlines_ref);
            catalog.set("PageMode", Object::name("UseOutlines"));
        }

        // Interactive form (AcroForm + widgets + generated appearances, 6.7).
        if !self.form_fields.is_empty() {
            let acro_ref = form_builder.build(&mut doc, &page_refs);
            catalog.set("AcroForm", acro_ref);
        }

        // Embedded file attachments (`/AF` + `/Names /EmbeddedFiles`) — required
        // for meaningful PDF/A-3, valid in any PDF.
        if !self.attachments.is_empty() {
            attach::apply(&mut doc, &mut catalog, &self.attachments);
        }

        // PDF/A: OutputIntent + XMP metadata + document ID (Fase 7.4).
        if let Some(level) = self.pdfa {
            pdfa::apply(
                &mut doc,
                &mut catalog,
                &self.info_entries(),
                level.part(),
                level.conformance(),
                level.rev(),
                self.facturx.as_ref(),
            );
        }

        doc.assign(catalog_ref, catalog);
        doc.set_root(catalog_ref);

        // PDF 2.0 deprecates the document information dictionary; PDF/A-4
        // forbids `/Info` in the trailer unless the catalog carries `/PieceInfo`
        // (ISO 19005-4 6.1.3). The metadata already lives in the XMP, so for
        // PDF/A-4 we simply omit `/Info`.
        let omit_info = matches!(self.pdfa, Some(l) if l.part() == 4);
        if !omit_info {
            if let Some(info_dict) = self.build_info() {
                let info_ref = doc.add(info_dict);
                doc.set_info(info_ref);
            }
        }

        Ok(doc.write()?)
    }

    /// Save the document to a file.
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        let bytes = self
            .to_bytes()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        std::fs::write(path, bytes)
    }

    /// The `/Info` entries as `(key, value)` pairs. Shared with the PDF/A XMP so
    /// the two stay in sync (PDF/A requires equal values where both appear).
    fn info_entries(&self) -> Vec<(&'static str, String)> {
        let mut e = Vec::new();
        let mut push = |k: &'static str, v: &Option<String>| {
            if let Some(v) = v {
                e.push((k, v.clone()));
            }
        };
        push("Title", &self.info.title);
        push("Author", &self.info.author);
        push("Subject", &self.info.subject);
        push("Keywords", &self.info.keywords);
        // Default the creating application to "rustpdf <version>" unless the
        // caller set one.
        e.push((
            "Creator",
            self.info
                .creator
                .clone()
                .unwrap_or_else(|| concat!("rustpdf ", env!("CARGO_PKG_VERSION")).to_string()),
        ));
        e.push((
            "Producer",
            concat!("rust-pdf ", env!("CARGO_PKG_VERSION")).to_string(),
        ));
        e
    }

    fn build_info(&self) -> Option<Dict> {
        let mut dict = Dict::new();
        for (k, v) in self.info_entries() {
            dict.set(k, PdfString::literal(v.into_bytes()));
        }
        Some(dict)
    }
}

/// A page item resolved for serialization (paragraphs already laid out).
enum Drawable {
    Graphics(Vec<u8>),
    Text(TextObject),
    Figure {
        id: usize,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        alt: String,
    },
}

/// A PDF rectangle array `[x0 y0 x1 y1]` from `[f64; 4]`.
fn rect_array(r: [f64; 4]) -> Object {
    Object::Array(vec![
        Object::Real(r[0]),
        Object::Real(r[1]),
        Object::Real(r[2]),
        Object::Real(r[3]),
    ])
}

/// The library version string, surfaced through the FFI as `pdf_version()`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Definition-of-Done invariant (project.md §10 / ADR 0002): the core must
    // stay `Send`. This fails to compile if anyone introduces `Rc`/`RefCell`.
    const _: fn() = || {
        fn assert_send<T: Send>() {}
        assert_send::<Document>();
        assert_send::<cos::Object>();
        assert_send::<writer::Document>();
    };

    #[test]
    fn blank_page_has_pdf_structure() {
        let mut doc = Document::new();
        doc.add_page();
        let bytes = doc.to_bytes().unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with("%PDF-1.7"));
        assert!(text.contains("/Type /Catalog"));
        assert!(text.contains("/Type /Pages"));
        assert!(text.contains("/Type /Page"));
        assert!(text.contains("/Count 1"));
        assert!(text.contains("/MediaBox [0 0 595.276 841.89]"));
        assert!(text.trim_end().ends_with("%%EOF"));
    }

    #[test]
    fn multipage_count_and_kids() {
        let mut doc = Document::new();
        doc.add_page();
        doc.add_page_sized(sizes::LETTER.0, sizes::LETTER.1);
        let bytes = doc.to_bytes().unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Count 2"));
        assert_eq!(doc.page_count(), 2);
    }

    #[test]
    fn content_stream_is_embedded() {
        let mut doc = Document::new();
        doc.add_page()
            .content()
            .set_fill_rgb(1.0, 0.0, 0.0)
            .rect(0.0, 0.0, 100.0, 100.0)
            .fill();
        let bytes = doc.to_bytes().unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("1 0 0 rg"));
        assert!(text.contains("0 0 100 100 re"));
        assert!(text.contains("stream\n"));
    }

    #[test]
    fn info_dict_present() {
        let mut doc = Document::new();
        doc.add_page();
        let bytes = doc.to_bytes().unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Producer (rust-pdf"));
    }
}

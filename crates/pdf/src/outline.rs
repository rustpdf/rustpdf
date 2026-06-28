//! Document outline / bookmarks (Tier 1): a navigable, nestable tree of named
//! destinations emitted as the catalog `/Outlines` (ISO 32000-1 §12.3.3).
//!
//! Each entry carries a `/Dest` of the form `[page /XYZ null top null]` so a
//! viewer jumps to (and optionally scrolls) the target page. Sibling entries are
//! doubly linked via `/Prev`/`/Next`; parents point at their first/last child
//! via `/First`/`/Last` and carry a `/Count` of open descendants.

use cos::{Dict, Object, PdfString, Reference};
use writer::Document as WriterDoc;

/// A document outline entry (bookmark). Build a tree with [`Bookmark::child`]
/// and hand the roots to [`Document::add_bookmark`](crate::Document::add_bookmark).
#[derive(Debug, Clone)]
pub struct Bookmark {
    title: String,
    page: usize,
    top: Option<f64>,
    children: Vec<Bookmark>,
}

impl Bookmark {
    /// A bookmark titled `title` that jumps to page `page` (0-based).
    pub fn new(title: impl Into<String>, page: usize) -> Self {
        Bookmark {
            title: title.into(),
            page,
            top: None,
            children: Vec::new(),
        }
    }

    /// Scroll the destination so `top` (page points measured from the bottom)
    /// sits at the top of the view. Without this the current scroll is kept.
    #[must_use]
    pub fn at_top(mut self, top: f64) -> Self {
        self.top = Some(top);
        self
    }

    /// Nest `child` under this bookmark (builder style).
    #[must_use]
    pub fn child(mut self, child: Bookmark) -> Self {
        self.children.push(child);
        self
    }

    /// Nest several children at once (builder style).
    #[must_use]
    pub fn children(mut self, children: impl IntoIterator<Item = Bookmark>) -> Self {
        self.children.extend(children);
        self
    }
}

/// Emit the outline tree and return the `/Outlines` root reference.
pub(crate) fn build(doc: &mut WriterDoc, page_refs: &[Reference], roots: &[Bookmark]) -> Reference {
    let outlines_ref = doc.reserve();
    let (first, last, count) = emit_level(doc, outlines_ref, roots, page_refs);
    let mut root = Dict::new().with("Type", Object::name("Outlines"));
    if let (Some(f), Some(l)) = (first, last) {
        root.set("First", f);
        root.set("Last", l);
    }
    root.set("Count", count);
    doc.assign(outlines_ref, root);
    outlines_ref
}

/// Emit one sibling level. Returns `(first, last, open_descendant_count)`.
fn emit_level(
    doc: &mut WriterDoc,
    parent: Reference,
    items: &[Bookmark],
    page_refs: &[Reference],
) -> (Option<Reference>, Option<Reference>, i64) {
    if items.is_empty() {
        return (None, None, 0);
    }
    // Reserve every sibling ref first so we can wire /Prev and /Next.
    let refs: Vec<Reference> = items.iter().map(|_| doc.reserve()).collect();
    let mut total = 0i64;
    for (i, item) in items.iter().enumerate() {
        let (cfirst, clast, ccount) = emit_level(doc, refs[i], &item.children, page_refs);
        let mut d = Dict::new()
            .with("Title", PdfString::text(&item.title))
            .with("Parent", parent);
        if i > 0 {
            d.set("Prev", refs[i - 1]);
        }
        if i + 1 < refs.len() {
            d.set("Next", refs[i + 1]);
        }
        if let (Some(cf), Some(cl)) = (cfirst, clast) {
            d.set("First", cf);
            d.set("Last", cl);
            // Positive = open: the descendants are shown when the pane opens.
            d.set("Count", ccount);
        }
        if let Some(&page_ref) = page_refs.get(item.page) {
            let top = item.top.map(Object::Real).unwrap_or(Object::Null);
            d.set(
                "Dest",
                Object::Array(vec![
                    Object::Reference(page_ref),
                    Object::name("XYZ"),
                    Object::Null,
                    top,
                    Object::Null,
                ]),
            );
        }
        doc.assign(refs[i], d);
        total += 1 + ccount;
    }
    (Some(refs[0]), refs.last().copied(), total)
}

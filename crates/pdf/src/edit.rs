//! Document manipulation (Fase 6, Tier 2): load an existing PDF, edit its
//! object graph, and write it back. Built on `parser` (read) and `writer`
//! (write).
//!
//! The page tree is flattened to a single level on load, which makes merge,
//! split, reorder, delete and rotate simple list operations; the proper
//! `/Pages` tree is rebuilt on save.

use std::collections::{BTreeMap, BTreeSet};

use cos::{Dict, Object, PdfString, Reference, Stream};
use parser::PdfReader;
use writer::{Document as WriterDoc, PdfVersion};

use crate::encrypt::{self, EncryptConfig, Encryption, Permissions};
use crate::{Align, BuildError};

/// An in-memory, editable PDF document.
#[derive(Debug, Clone)]
pub struct EditableDoc {
    objects: BTreeMap<u32, Object>,
    catalog: u32,
    pages_root: u32,
    info: Option<u32>,
    metadata: Option<u32>,
    page_order: Vec<u32>,
    next_num: u32,
    encryption: Option<EncryptConfig>,
    /// Emit object streams + a cross-reference stream on save (smaller files).
    compress: bool,
    /// Lazily-created standard Helvetica font object (for generated appearances
    /// and text watermarks), shared across calls once allocated.
    helv_font: Option<u32>,
    /// PDF version written to the header (PDF/A-1 conversion forces 1.4).
    version: PdfVersion,
    /// A forced trailer `/ID` (set by PDF/A conversion).
    forced_id: Option<[Vec<u8>; 2]>,
    /// Whether [`redact`](EditableDoc::redact) was called (gates output behind
    /// the Redaction feature license).
    redacted: bool,
    /// Pages whose original content has already been wrapped in a balanced
    /// `q…Q` so appended stamps start from the page's initial CTM regardless of
    /// how the original stream left the graphics state.
    isolated_pages: BTreeSet<u32>,
    /// TrueType/OpenType fonts registered for text stamping (see
    /// [`add_font`](EditableDoc::add_font)). Each is embedded as a subset Type0
    /// font at serialization time; stamps reference it by object number.
    stamp_fonts: Vec<StampFont>,
    /// Coordinate space for the positioned stamping primitives (see
    /// [`set_stamp_space`](EditableDoc::set_stamp_space)).
    stamp_space: StampSpace,
}

/// A font registered on an [`EditableDoc`] for stamping arbitrary text
/// (`place_text`/`masked_text` with a `font_id`). The glyph program is embedded
/// as a subset at `to_bytes` time; content streams emit the *original* glyph ids
/// and a `CIDToGIDMap` remaps them into the subset, so stamps render with the
/// real font's glyphs and metrics — identical to `Document::show_text`.
#[derive(Debug, Clone)]
struct StampFont {
    font: fonts::Font,
    /// Reserved object number of the Type0 font dict (referenced by pages).
    obj: u32,
    /// Content-stream resource name (e.g. `StF0`).
    resource: String,
    /// Original glyph ids used by any stamp with this font.
    used: BTreeSet<u16>,
    /// First-seen source text per original glyph id (for `ToUnicode`).
    gid_to_unicode: BTreeMap<u16, String>,
}

impl EditableDoc {
    /// Load and parse an existing PDF (empty password).
    pub fn load(bytes: impl AsRef<[u8]>) -> Result<EditableDoc, parser::PdfError> {
        Self::load_with_password(bytes, b"")
    }

    /// Load an encrypted PDF with a password.
    pub fn load_with_password(
        bytes: impl AsRef<[u8]>,
        password: &[u8],
    ) -> Result<EditableDoc, parser::PdfError> {
        let reader = PdfReader::parse_with_password(bytes, password)?;
        let mut objects = BTreeMap::new();
        for n in reader.object_numbers() {
            if let Some(o) = reader.get(n) {
                objects.insert(n, o.clone());
            }
        }
        let catalog = reference_num(reader.trailer().get("Root"))
            .ok_or_else(|| parser::PdfError::Syntax("no /Root".into()))?;
        let info = reference_num(reader.trailer().get("Info"));
        let pages_root = as_dict(objects.get(&catalog))
            .and_then(|d| reference_num(d.get("Pages")))
            .ok_or_else(|| parser::PdfError::Syntax("no /Pages".into()))?;
        let metadata =
            as_dict(objects.get(&catalog)).and_then(|d| reference_num(d.get("Metadata")));

        let mut page_order = Vec::new();
        let mut seen = Vec::new();
        collect_pages(&objects, pages_root, &mut page_order, &mut seen);

        let next_num = objects.keys().copied().max().unwrap_or(0) + 1;
        Ok(EditableDoc {
            objects,
            catalog,
            pages_root,
            info,
            metadata,
            page_order,
            next_num,
            encryption: None,
            compress: false,
            helv_font: None,
            version: PdfVersion::V1_7,
            forced_id: None,
            redacted: false,
            isolated_pages: BTreeSet::new(),
            stamp_fonts: Vec::new(),
            stamp_space: StampSpace::Visible,
        })
    }

    /// Number of pages.
    pub fn page_count(&self) -> usize {
        self.page_order.len()
    }

    /// The page object numbers, in order.
    pub fn page_numbers(&self) -> &[u32] {
        &self.page_order
    }

    fn allocate(&mut self) -> u32 {
        let n = self.next_num;
        self.next_num += 1;
        n
    }

    // ---- 6.3 rotate / reorder / delete -----------------------------------

    /// Rotate page `index` by `degrees` (added to any existing `/Rotate`).
    pub fn rotate_page(&mut self, index: usize, degrees: i32) {
        let Some(&num) = self.page_order.get(index) else {
            return;
        };
        let current = as_dict(self.objects.get(&num))
            .and_then(|d| d.get("Rotate"))
            .and_then(int)
            .unwrap_or(0);
        let rot = (((current as i32 + degrees) % 360) + 360) % 360;
        self.update_dict(num, |d| {
            d.set("Rotate", rot as i64);
        });
    }

    /// Delete page `index` (the object becomes unreferenced; `optimize` drops it).
    pub fn delete_page(&mut self, index: usize) {
        if index < self.page_order.len() {
            self.page_order.remove(index);
        }
    }

    /// Reorder pages by a permutation of current indices.
    ///
    /// `new_order` must be a true permutation of `0..page_count` — every index
    /// exactly once. An invalid argument (wrong length, out-of-range, or a
    /// repeated index) is rejected and leaves the page order untouched, rather
    /// than producing a malformed page tree (duplicated/dropped pages).
    pub fn reorder_pages(&mut self, new_order: &[usize]) {
        let n = self.page_order.len();
        if new_order.len() != n {
            return;
        }
        // Verify it is a genuine permutation of 0..n (each index used once).
        let mut seen = vec![false; n];
        for &i in new_order {
            match seen.get_mut(i) {
                Some(slot) if !*slot => *slot = true,
                _ => return, // out-of-range or duplicate index
            }
        }
        self.page_order = new_order.iter().map(|&i| self.page_order[i]).collect();
    }

    // ---- 6.1 merge --------------------------------------------------------

    /// Append all pages of `other` to this document (Fase 6.1).
    pub fn merge(&mut self, other: &EditableDoc) {
        let base = self.next_num - 1; // other's object N becomes N + base
        let map: BTreeMap<u32, u32> = other.objects.keys().map(|&old| (old, old + base)).collect();

        for (&old, obj) in &other.objects {
            // Skip the other document's structural roots; we keep our own.
            if old == other.catalog || old == other.pages_root {
                continue;
            }
            let mut copy = obj.clone();
            remap(&mut copy, &map);
            let new_num = old + base;
            self.objects.insert(new_num, copy);
            self.next_num = self.next_num.max(new_num + 1);
        }
        for &p in &other.page_order {
            self.page_order.push(p + base);
        }
    }

    // ---- 6.2 split / extract ---------------------------------------------

    /// Build a new document containing only the pages at `indices`, with all
    /// their dependencies copied and compactly renumbered (Fase 6.2).
    pub fn extract_pages(&self, indices: &[usize]) -> EditableDoc {
        let roots: Vec<u32> = indices
            .iter()
            .filter_map(|&i| self.page_order.get(i).copied())
            .collect();

        // Reachability from the selected pages, not following `/Parent` upward.
        let mut keep = BTreeSet::new();
        let mut stack = roots.clone();
        while let Some(n) = stack.pop() {
            if !keep.insert(n) {
                continue;
            }
            if let Some(obj) = self.objects.get(&n) {
                let mut refs = Vec::new();
                collect_refs_skipping(obj, &["Parent"], &mut refs);
                stack.extend(refs);
            }
        }

        // Compact renumbering: 1 = catalog, 2 = pages root, then kept objects.
        let mut map = BTreeMap::new();
        let mut next = 3u32;
        for &n in &keep {
            map.insert(n, next);
            next += 1;
        }

        let mut objects = BTreeMap::new();
        for &n in &keep {
            let mut copy = self.objects.get(&n).unwrap().clone();
            remap(&mut copy, &map);
            objects.insert(map[&n], copy);
        }
        let page_order: Vec<u32> = roots.iter().map(|n| map[n]).collect();

        // Fresh catalog + pages root.
        objects.insert(
            1,
            Object::Dict(
                Dict::new()
                    .with("Type", Object::name("Catalog"))
                    .with("Pages", Reference::new(2)),
            ),
        );
        objects.insert(
            2,
            Object::Dict(Dict::new().with("Type", Object::name("Pages"))),
        );

        EditableDoc {
            objects,
            catalog: 1,
            pages_root: 2,
            info: None,
            metadata: None,
            page_order,
            next_num: next,
            encryption: None,
            compress: false,
            helv_font: None,
            version: PdfVersion::V1_7,
            forced_id: None,
            redacted: false,
            isolated_pages: BTreeSet::new(),
            stamp_fonts: Vec::new(),
            stamp_space: StampSpace::Visible,
        }
    }

    // ---- 6.5 metadata -----------------------------------------------------

    /// Set an `/Info` string entry (creating the Info dict if needed).
    pub fn set_info(&mut self, key: &str, value: &str) {
        let info = match self.info {
            Some(n) => n,
            None => {
                let n = self.allocate();
                self.objects.insert(n, Object::Dict(Dict::new()));
                self.info = Some(n);
                n
            }
        };
        let val = Object::String(PdfString::literal(value.as_bytes().to_vec()));
        self.update_dict(info, |d| {
            d.set(key, val.clone());
        });
    }

    /// Read an `/Info` string entry.
    pub fn get_info(&self, key: &str) -> Option<String> {
        let info = self.info?;
        match as_dict(self.objects.get(&info))?.get(key)? {
            Object::String(s) => Some(String::from_utf8_lossy(s.as_bytes()).into_owned()),
            _ => None,
        }
    }

    /// Set the XMP metadata stream (`/Metadata`, uncompressed XML).
    pub fn set_xmp(&mut self, xml: impl Into<Vec<u8>>) {
        let dict = Dict::new()
            .with("Type", Object::name("Metadata"))
            .with("Subtype", Object::name("XML"));
        let stream = Object::Stream(Stream::with_dict(dict, xml.into()));
        let num = match self.metadata {
            Some(n) => {
                self.objects.insert(n, stream);
                n
            }
            None => {
                let n = self.allocate();
                self.objects.insert(n, stream);
                self.metadata = Some(n);
                n
            }
        };
        self.update_dict(self.catalog, |d| {
            d.set("Metadata", Reference::new(num));
        });
    }

    /// Read the raw XMP metadata bytes, if present.
    pub fn get_xmp(&self) -> Option<Vec<u8>> {
        let num = self.metadata?;
        match self.objects.get(&num)? {
            Object::Stream(s) => Some(s.data.clone()),
            _ => None,
        }
    }

    // ---- 6.6 overlay / watermark -----------------------------------------

    /// Append `content` (raw content-stream operators) on top of page `index`.
    /// The bytes are wrapped in `q`/`Q` so they cannot leak graphics state.
    /// `extra_resources` are merged into the page's `/Resources`.
    pub fn overlay_page(&mut self, index: usize, content: &[u8], extra_resources: Option<Dict>) {
        let Some(&page) = self.page_order.get(index) else {
            return;
        };
        let mut wrapped = b"q\n".to_vec();
        wrapped.extend_from_slice(content);
        wrapped.extend_from_slice(b"\nQ\n");
        let overlay_ref = self.allocate();
        self.objects
            .insert(overlay_ref, Object::Stream(Stream::new(wrapped)));

        // Build the new /Contents array: existing streams, then the overlay.
        let mut contents = match as_dict(self.objects.get(&page)).and_then(|d| d.get("Contents")) {
            Some(Object::Reference(r)) => vec![Object::Reference(*r)],
            Some(Object::Array(a)) => a.clone(),
            _ => Vec::new(),
        };
        contents.push(Object::Reference(Reference::new(overlay_ref)));

        if let Some(extra) = extra_resources {
            self.merge_resources(page, extra);
        }
        self.update_dict(page, |d| {
            d.set("Contents", Object::Array(contents.clone()));
        });
    }

    fn merge_resources(&mut self, page: u32, extra: Dict) {
        let mut res = as_dict(self.objects.get(&page))
            .and_then(|d| match d.get("Resources") {
                Some(Object::Dict(r)) => Some(r.clone()),
                _ => None,
            })
            .unwrap_or_default();
        for (k, v) in extra.iter() {
            res.set(k.clone(), v.clone());
        }
        self.update_dict(page, |d| {
            d.set("Resources", Object::Dict(res.clone()));
        });
    }

    // ---- 6.7 AcroForm fill + flatten -------------------------------------

    /// Every terminal field's fully-qualified name (dotted, e.g. `address.city`).
    pub fn field_names(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(acro) = self.acroform_num() {
            for r in self.acro_field_roots(acro) {
                self.walk_fields("", r, &mut out);
            }
        }
        out
    }

    /// Set a **text field** (or text-style choice) to `value`, generating a
    /// `/AP` appearance stream so it renders without `NeedAppearances`. Returns
    /// whether a matching field was found. `field_name` is the fully-qualified
    /// (dotted) name.
    pub fn fill_text_field(&mut self, field_name: &str, value: &str) -> bool {
        let Some(field) = self.find_field(field_name) else {
            return false;
        };
        self.update_dict(field, |d| {
            d.set("V", Object::String(PdfString::text(value)));
        });
        let size = self.field_font_size(field);
        let widgets = self.field_widgets(field);
        for w in widgets {
            if let Some(rect) = self.widget_rect(w) {
                let ap = self.make_text_appearance(rect, value, size);
                self.update_dict(w, |d| {
                    d.set(
                        "AP",
                        Object::Dict(Dict::new().with("N", Reference::new(ap))),
                    );
                });
            }
        }
        true
    }

    /// Check or uncheck a **checkbox** field, updating both `/V` and each
    /// widget's `/AS` to the existing on/off appearance state. Returns whether a
    /// matching field was found.
    pub fn set_checkbox(&mut self, field_name: &str, checked: bool) -> bool {
        let Some(field) = self.find_field(field_name) else {
            return false;
        };
        let widgets = self.field_widgets(field);
        let on = self
            .widget_on_state(widgets.first().copied())
            .unwrap_or_else(|| "Yes".to_string());
        let state = if checked { on.as_str() } else { "Off" };
        self.update_dict(field, |d| {
            d.set("V", Object::name(state));
        });
        for w in &widgets {
            self.update_dict(*w, |d| {
                d.set("AS", Object::name(state));
            });
        }
        true
    }

    /// Select a **radio button** by its export value, updating `/V` and every
    /// kid widget's `/AS` (the matching widget turns on, the rest go `Off`).
    /// Returns whether a matching field was found.
    pub fn set_radio(&mut self, field_name: &str, export_value: &str) -> bool {
        let Some(field) = self.find_field(field_name) else {
            return false;
        };
        self.update_dict(field, |d| {
            d.set("V", Object::name(export_value));
        });
        let widgets = self.field_widgets(field);
        for w in &widgets {
            let on = self.widget_ap_states(*w).iter().any(|s| s == export_value);
            let state = if on { export_value } else { "Off" };
            self.update_dict(*w, |d| {
                d.set("AS", Object::name(state));
            });
        }
        true
    }

    /// Set a **choice** (dropdown / list box) field's value, generating a `/AP`
    /// appearance. Returns whether a matching field was found.
    pub fn set_choice(&mut self, field_name: &str, value: &str) -> bool {
        self.fill_text_field(field_name, value)
    }

    /// **Flatten** all interactive form fields: paint each widget's current
    /// appearance into its page's content as static graphics, drop the widget
    /// annotations, and remove the `/AcroForm`. The result has no fillable
    /// fields but looks identical in any viewer. Call before `to_bytes`/`save`.
    pub fn flatten_forms(&mut self) {
        let Some(acro) = self.acroform_num() else {
            return;
        };
        // Every field/widget object in the AcroForm tree — removed at the end so
        // the flattened file carries no leftover interactive objects.
        let form_objs = self.collect_form_objects(acro);
        let pages = self.page_order.clone();
        for page in pages {
            let annots = match as_dict(self.objects.get(&page)).and_then(|d| d.get("Annots")) {
                Some(Object::Array(a)) => a.clone(),
                _ => continue,
            };
            let mut keep: Vec<Object> = Vec::new();
            let mut content = String::new();
            let mut xobjs: Vec<(String, u32)> = Vec::new();
            for annot in &annots {
                let Object::Reference(r) = annot else {
                    keep.push(annot.clone());
                    continue;
                };
                let is_widget = as_dict(self.objects.get(&r.number))
                    .and_then(|d| d.get("Subtype"))
                    .and_then(name)
                    .as_deref()
                    == Some("Widget");
                if !is_widget {
                    keep.push(annot.clone());
                    continue;
                }
                // Draw the widget's current appearance, then drop the widget.
                if let (Some(rect), Some(xnum)) =
                    (self.widget_rect(r.number), self.widget_appearance(r.number))
                {
                    let nm = format!("FmFlat{}", xobjs.len());
                    let bbox = self.xobject_bbox(xnum).unwrap_or([0.0, 0.0, 1.0, 1.0]);
                    content.push_str(&flatten_draw(&nm, rect, bbox));
                    xobjs.push((nm, xnum));
                }
            }
            if !content.is_empty() {
                self.append_content(page, content.into_bytes());
                for (nm, xnum) in xobjs {
                    self.add_page_resource(page, "XObject", &nm, xnum);
                }
            }
            self.update_dict(page, |d| {
                if keep.is_empty() {
                    d.set("Annots", Object::Array(Vec::new()));
                } else {
                    d.set("Annots", Object::Array(keep.clone()));
                }
            });
        }
        // Drop every interactive field/widget object (their appearance streams
        // are kept — they are now referenced from the page resources).
        for n in form_objs {
            self.objects.remove(&n);
        }
        // Remove the AcroForm entirely from the catalog.
        if let Some(Object::Dict(cat)) = self.objects.get(&self.catalog).cloned() {
            let mut nc = Dict::new();
            for (k, v) in cat.iter() {
                if k.as_str() != "AcroForm" {
                    nc.set(k.clone(), v.clone());
                }
            }
            self.objects.insert(self.catalog, Object::Dict(nc));
        }
    }

    /// Every object number in the AcroForm field hierarchy (fields and all their
    /// `/Kids`, recursively), for removal during flattening.
    fn collect_form_objects(&self, acro: u32) -> Vec<u32> {
        let mut out = Vec::new();
        let mut stack = self.acro_field_roots(acro);
        while let Some(n) = stack.pop() {
            if out.contains(&n) {
                continue;
            }
            out.push(n);
            if let Some(Object::Array(kids)) =
                as_dict(self.objects.get(&n)).and_then(|d| d.get("Kids"))
            {
                for k in kids {
                    if let Object::Reference(r) = k {
                        stack.push(r.number);
                    }
                }
            }
        }
        out
    }

    // ---- form helpers -----------------------------------------------------

    fn acroform_num(&self) -> Option<u32> {
        let acro = as_dict(self.objects.get(&self.catalog))?.get("AcroForm")?;
        match acro {
            Object::Reference(r) => Some(r.number),
            // A direct AcroForm dict: not promoted to indirect (rare).
            _ => None,
        }
    }

    fn acro_field_roots(&self, acro: u32) -> Vec<u32> {
        match as_dict(self.objects.get(&acro)).and_then(|d| d.get("Fields")) {
            Some(Object::Array(a)) => a
                .iter()
                .filter_map(|o| match o {
                    Object::Reference(r) => Some(r.number),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Child object numbers of `num` whose own `/T` makes them sub-*fields*
    /// (as opposed to plain widgets, which have no `/T`).
    fn child_fields(&self, num: u32) -> Vec<u32> {
        match as_dict(self.objects.get(&num)).and_then(|d| d.get("Kids")) {
            Some(Object::Array(kids)) => kids
                .iter()
                .filter_map(|k| match k {
                    Object::Reference(r)
                        if as_dict(self.objects.get(&r.number))
                            .and_then(|d| d.get("T"))
                            .is_some() =>
                    {
                        Some(r.number)
                    }
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn qualified(&self, prefix: &str, num: u32) -> String {
        let name = as_dict(self.objects.get(&num))
            .and_then(|d| d.get("T"))
            .and_then(string_value);
        match name {
            Some(n) if prefix.is_empty() => n,
            Some(n) => format!("{prefix}.{n}"),
            None => prefix.to_string(),
        }
    }

    fn walk_fields(&self, prefix: &str, num: u32, out: &mut Vec<String>) {
        let full = self.qualified(prefix, num);
        let kids = self.child_fields(num);
        if kids.is_empty() {
            out.push(full);
        } else {
            for k in kids {
                self.walk_fields(&full, k, out);
            }
        }
    }

    /// Resolve a fully-qualified field name to its terminal field object number.
    fn find_field(&self, target: &str) -> Option<u32> {
        let acro = self.acroform_num()?;
        for r in self.acro_field_roots(acro) {
            if let Some(found) = self.resolve_field("", r, target) {
                return Some(found);
            }
        }
        None
    }

    fn resolve_field(&self, prefix: &str, num: u32, target: &str) -> Option<u32> {
        let full = self.qualified(prefix, num);
        let kids = self.child_fields(num);
        if kids.is_empty() {
            return (full == target).then_some(num);
        }
        for k in kids {
            if let Some(found) = self.resolve_field(&full, k, target) {
                return Some(found);
            }
        }
        None
    }

    /// The widget annotation object numbers for a terminal field: the field dict
    /// itself when it is a merged field/widget (has `/Rect`), otherwise its kids.
    fn field_widgets(&self, field: u32) -> Vec<u32> {
        if as_dict(self.objects.get(&field))
            .map(|d| d.contains_key("Rect"))
            .unwrap_or(false)
        {
            return vec![field];
        }
        match as_dict(self.objects.get(&field)).and_then(|d| d.get("Kids")) {
            Some(Object::Array(kids)) => kids
                .iter()
                .filter_map(|k| match k {
                    Object::Reference(r) => Some(r.number),
                    _ => None,
                })
                .collect(),
            _ => vec![field],
        }
    }

    fn widget_rect(&self, widget: u32) -> Option<[f64; 4]> {
        match as_dict(self.objects.get(&widget))?.get("Rect")? {
            Object::Array(a) if a.len() == 4 => {
                let v: Vec<f64> = a.iter().filter_map(num_f64).collect();
                (v.len() == 4).then(|| {
                    // Normalize so x0<x1, y0<y1.
                    [
                        v[0].min(v[2]),
                        v[1].min(v[3]),
                        v[0].max(v[2]),
                        v[1].max(v[3]),
                    ]
                })
            }
            _ => None,
        }
    }

    /// The `/AP /N` keys of a widget (button on/off state names).
    fn widget_ap_states(&self, widget: u32) -> Vec<String> {
        as_dict(self.objects.get(&widget))
            .and_then(|d| d.get("AP"))
            .and_then(|ap| match ap {
                Object::Dict(d) => d.get("N").cloned(),
                _ => None,
            })
            .map(|n| match n {
                Object::Dict(d) => d.iter().map(|(k, _)| k.as_str().to_string()).collect(),
                _ => Vec::new(),
            })
            .unwrap_or_default()
    }

    /// The on (non-`Off`) appearance state name of a widget, if any.
    fn widget_on_state(&self, widget: Option<u32>) -> Option<String> {
        let w = widget?;
        self.widget_ap_states(w).into_iter().find(|s| s != "Off")
    }

    /// The Form XObject object number for a widget's *current* appearance: the
    /// `/AP /N` stream, or — for a button — the substate keyed by `/AS`.
    fn widget_appearance(&self, widget: u32) -> Option<u32> {
        let d = as_dict(self.objects.get(&widget))?;
        let n = match d.get("AP")? {
            Object::Dict(ap) => ap.get("N")?,
            _ => return None,
        };
        match n {
            Object::Reference(r) => Some(r.number),
            Object::Dict(states) => {
                let asname = d.get("AS").and_then(name).unwrap_or_else(|| "Off".into());
                match states.get(&asname).or_else(|| states.get("Off")) {
                    Some(Object::Reference(r)) => Some(r.number),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn xobject_bbox(&self, num: u32) -> Option<[f64; 4]> {
        match as_dict(self.objects.get(&num))?.get("BBox")? {
            Object::Array(a) if a.len() == 4 => {
                let v: Vec<f64> = a.iter().filter_map(num_f64).collect();
                (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
            }
            _ => None,
        }
    }

    /// The text size from a field's `/DA` (`/Font size Tf …`); 0 → auto.
    fn field_font_size(&self, field: u32) -> f64 {
        let da = as_dict(self.objects.get(&field))
            .and_then(|d| d.get("DA"))
            .and_then(string_value);
        if let Some(da) = da {
            let toks: Vec<&str> = da.split_whitespace().collect();
            if let Some(i) = toks.iter().position(|t| *t == "Tf") {
                if i >= 1 {
                    if let Ok(sz) = toks[i - 1].parse::<f64>() {
                        return sz;
                    }
                }
            }
        }
        0.0
    }

    /// Lazily create (and cache) a standard Helvetica font object.
    fn helvetica(&mut self) -> u32 {
        if let Some(n) = self.helv_font {
            return n;
        }
        let n = self.allocate();
        self.objects.insert(
            n,
            Object::Dict(
                Dict::new()
                    .with("Type", Object::name("Font"))
                    .with("Subtype", Object::name("Type1"))
                    .with("BaseFont", Object::name("Helvetica"))
                    .with("Encoding", Object::name("WinAnsiEncoding")),
            ),
        );
        self.helv_font = Some(n);
        n
    }

    /// Build a text appearance Form XObject for a field value; returns its num.
    fn make_text_appearance(&mut self, rect: [f64; 4], value: &str, size: f64) -> u32 {
        let helv = self.helvetica();
        let w = rect[2] - rect[0];
        let h = rect[3] - rect[1];
        let fs = if size <= 0.0 {
            (h - 4.0).clamp(6.0, 12.0)
        } else {
            size
        };
        let ty = ((h - fs) / 2.0).max(2.0);
        let body = format!(
            "/Tx BMC\nq\nBT\n/Helv {fs:.2} Tf\n0 g\n2 {ty:.2} Td\n({}) Tj\nET\nQ\nEMC",
            escape_pdf_literal(value)
        );
        let dict = Dict::new()
            .with("Type", Object::name("XObject"))
            .with("Subtype", Object::name("Form"))
            .with("FormType", Object::Integer(1))
            .with(
                "BBox",
                Object::Array(vec![
                    Object::Real(0.0),
                    Object::Real(0.0),
                    Object::Real(w),
                    Object::Real(h),
                ]),
            )
            .with(
                "Resources",
                Object::Dict(Dict::new().with(
                    "Font",
                    Object::Dict(Dict::new().with("Helv", Reference::new(helv))),
                )),
            );
        let n = self.allocate();
        self.objects.insert(
            n,
            Object::Stream(Stream::with_dict(dict, body.into_bytes())),
        );
        n
    }

    // ---- watermarks (Tier 1) ---------------------------------------------

    /// Stamp a diagonal **text watermark** (e.g. "CONFIDENTIAL") across every
    /// page, centered and rotated, drawn semi-transparently over the existing
    /// content. Uses the standard Helvetica font, so `text` should be WinAnsi
    /// (Latin-1) — ideal for short stamps. See [`WatermarkOptions`] for size,
    /// color, opacity and angle.
    pub fn watermark_text(&mut self, text: &str, opts: WatermarkOptions) {
        let helv = self.helvetica();
        let gs = self.alloc_extgstate(opts.opacity);
        let gs_opaque = if opts.opaque_background {
            Some(self.alloc_extgstate(1.0))
        } else {
            None
        };
        let pages = self.page_order.clone();
        for page in pages {
            let geom = self.page_geometry(page);
            let (pw, ph) = geom.visible_size();
            let (cx, cy) = (pw / 2.0, ph / 2.0);
            let theta = opts.rotation_deg.to_radians();
            let (a, b) = (theta.cos(), theta.sin());
            let (c, d) = (-theta.sin(), theta.cos());
            // Rough Helvetica width to center the baseline on the page center.
            let tw = opts.size * 0.52 * text.chars().count() as f64;
            let (r, g, bl) = opts.color;
            // Draw in a frame rotated about the page center (in visible space),
            // compensating page /Rotate via `upright_cm` so it reads upright.
            let mut content = format!(
                "q\n{upright}q\n{a:.5} {b:.5} {c:.5} {d:.5} {cx:.2} {cy:.2} cm\n",
                upright = geom.upright_cm(),
            );
            if let Some(_op) = gs_opaque {
                // Opaque white box behind the text (white-out stamp).
                let pad = opts.size * 0.25;
                content.push_str(&format!(
                    "/GSwmO gs\n1 1 1 rg\n{x:.2} {y:.2} {w:.2} {h:.2} re f\n",
                    x = -tw / 2.0 - pad,
                    y = -opts.size * 0.35 - pad,
                    w = tw + 2.0 * pad,
                    h = opts.size + 2.0 * pad,
                ));
            }
            let text_gs = if gs_opaque.is_some() { "GSwmO" } else { "GSwm" };
            content.push_str(&format!(
                "/{text_gs} gs\nBT\n/Helvwm {size:.2} Tf\n{r:.3} {g:.3} {bl:.3} rg\n\
                 1 0 0 1 {ox:.2} {oy:.2} Tm\n({txt}) Tj\nET\nQ\nQ\n",
                size = opts.size,
                ox = -tw / 2.0,
                oy = -opts.size * 0.35,
                txt = escape_pdf_literal(text),
            ));
            self.append_content(page, content.into_bytes());
            self.add_page_resource(page, "Font", "Helvwm", helv);
            self.add_page_resource(page, "ExtGState", "GSwm", gs);
            if let Some(op) = gs_opaque {
                self.add_page_resource(page, "ExtGState", "GSwmO", op);
            }
        }
    }

    /// Stamp an **image watermark** centered on every page at `width`×`height`
    /// points, rotated `rotation_deg` degrees counter-clockwise about its center
    /// and drawn at `opacity`. Respects page `/Rotate` and `/CropBox` so the
    /// stamp lands centered in the visible area. `image` is decoded/encoded like
    /// [`Document::add_image`](crate::Document::add_image) inputs.
    pub fn watermark_image(
        &mut self,
        image: &images::Image,
        width: f64,
        height: f64,
        opacity: f64,
        rotation_deg: f64,
    ) {
        let img_num = self.insert_image(image);
        let gs = self.alloc_extgstate(opacity);
        let pages = self.page_order.clone();
        for page in pages {
            let geom = self.page_geometry(page);
            let (pw, ph) = geom.visible_size();
            let (cx, cy) = (pw / 2.0, ph / 2.0);
            let theta = rotation_deg.to_radians();
            let (a, b) = (theta.cos(), theta.sin());
            let (c, d) = (-theta.sin(), theta.cos());
            // upright (page /Rotate) → rotate about center → place image, drawn
            // from its own center so rotation pivots on the image middle.
            let content = format!(
                "q\n{upright}/GSwm gs\nq\n{a:.5} {b:.5} {c:.5} {d:.5} {cx:.2} {cy:.2} cm\n\
                 {width:.2} 0 0 {height:.2} {hx:.2} {hy:.2} cm\n/Imwm Do\nQ\nQ\n",
                upright = geom.upright_cm(),
                hx = -width / 2.0,
                hy = -height / 2.0,
            );
            self.append_content(page, content.into_bytes());
            self.add_page_resource(page, "XObject", "Imwm", img_num);
            self.add_page_resource(page, "ExtGState", "GSwm", gs);
        }
    }

    // ---- positioned drawing primitives (issue #45 P1 #2) -----------------

    /// Paint a **filled rectangle** at `(x, y)` with size `width`×`height` on
    /// page `index`, in the given `color` (RGB, each `0..=1`) and `opacity`
    /// (`0..=1`, where `1.0` is fully opaque). The common use is masking a
    /// placeholder by painting an opaque white box: `color = (1.0, 1.0, 1.0)`,
    /// `opacity = 1.0`.
    ///
    /// Coordinates are in the page's **visible** space — origin at the displayed
    /// lower-left, y up — so the box lands where a viewer sees it regardless of
    /// the page's `/Rotate`. Returns `false` if `index` is out of range.
    #[allow(clippy::too_many_arguments)]
    pub fn fill_rect(
        &mut self,
        index: usize,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        color: (f64, f64, f64),
        opacity: f64,
    ) -> bool {
        let Some(&page) = self.page_order.get(index) else {
            return false;
        };
        let geom = self.page_geometry(page);
        let (r, g, b) = (
            color.0.clamp(0.0, 1.0),
            color.1.clamp(0.0, 1.0),
            color.2.clamp(0.0, 1.0),
        );
        let opacity = opacity.clamp(0.0, 1.0);
        let gs = self.alloc_extgstate(opacity);
        let gs_name = format!("GSd{gs}");
        let content = format!(
            "q\n{upright}/{gs_name} gs\n{r:.3} {g:.3} {b:.3} rg\n\
             {x:.2} {y:.2} {width:.2} {height:.2} re\nf\nQ\n",
            upright = self.stamp_cm(&geom),
        );
        self.append_content(page, content.into_bytes());
        self.add_page_resource(page, "ExtGState", &gs_name, gs);
        true
    }

    /// Choose the **coordinate space** of the positioned stamping primitives
    /// (`fill_rect`, `place_text*`, `masked_text*`, `place_paragraph*`,
    /// `draw_image`) for subsequent calls (FINDING-004).
    ///
    /// The default, [`StampSpace::Visible`], keeps the historical behavior:
    /// coordinates in the page's visible space, compensating `/Rotate` (a
    /// stamp on a rotated scan reads upright). [`StampSpace::Media`] disables
    /// that compensation entirely: coordinates and `rotation_deg` are taken in
    /// the raw PDF user space, matching iText `SetFixedPosition`/
    /// `SetRotationAngle` — the stamp's text matrix is composed relative to the
    /// media, never to the page's `/Rotate`. Watermarks and redaction keep
    /// visible-space semantics regardless of this mode.
    pub fn set_stamp_space(&mut self, space: StampSpace) {
        self.stamp_space = space;
    }

    /// The active stamping coordinate space.
    pub fn stamp_space(&self) -> StampSpace {
        self.stamp_space
    }

    /// The `cm` prefix realizing the active [`StampSpace`] for a page: the
    /// visible-space up-righting transform, or nothing for raw media space.
    fn stamp_cm(&self, geom: &PageGeom) -> String {
        match self.stamp_space {
            StampSpace::Visible => geom.upright_cm(),
            StampSpace::Media => String::new(),
        }
    }

    // ---- arbitrary-font stamping (embedded TrueType/OpenType) ------------

    /// Register a TrueType/OpenType font from raw bytes for text stamping,
    /// returning a `font_id` usable with [`place_text_with_font`] and
    /// [`masked_text_with_font`]. The font is embedded as a **subset** at
    /// serialization time (same shaping/subsetting pipeline as
    /// [`Document::add_font_file`](crate::Document::add_font_file)), so stamped
    /// text renders with the real font's glyphs and metrics — e.g. a serifed
    /// Times, not the built-in Helvetica used by the plain stamp calls.
    pub fn add_font(&mut self, data: impl Into<Vec<u8>>) -> Result<usize, fonts::FontError> {
        let font = fonts::Font::from_bytes(data, 0)?;
        Ok(self.register_stamp_font(font))
    }

    /// Register a font for stamping from a file path. See [`add_font`].
    pub fn add_font_file(
        &mut self,
        path: impl AsRef<std::path::Path>,
    ) -> Result<usize, fonts::FontError> {
        let font = fonts::Font::from_file(path)?;
        Ok(self.register_stamp_font(font))
    }

    fn register_stamp_font(&mut self, font: fonts::Font) -> usize {
        let obj = self.allocate();
        let id = self.stamp_fonts.len();
        self.stamp_fonts.push(StampFont {
            font,
            obj,
            resource: format!("StF{id}"),
            used: BTreeSet::new(),
            gid_to_unicode: BTreeMap::new(),
        });
        id
    }

    /// Like [`place_text_aligned`](EditableDoc::place_text_aligned) but draws
    /// with the embedded font registered as `font_id` (from [`add_font`] /
    /// [`add_font_file`]). Text may be arbitrary Unicode (it is shaped and
    /// mapped to the font's glyphs); width for alignment comes from the real
    /// font's advances. Returns `false` if `index` or `font_id` is out of range.
    #[allow(clippy::too_many_arguments)]
    pub fn place_text_with_font(
        &mut self,
        index: usize,
        x: f64,
        y: f64,
        text: &str,
        size: f64,
        color: (f64, f64, f64),
        rotation_deg: f64,
        align: Align,
        font_id: usize,
    ) -> bool {
        self.place_text_with_font_anchored(
            index,
            x,
            y,
            text,
            size,
            color,
            rotation_deg,
            align,
            font_id,
            VerticalAnchor::Baseline,
        )
    }

    /// Like [`place_text_with_font`](EditableDoc::place_text_with_font) but with
    /// an explicit **vertical anchor**: what `y` means. `Baseline` is the
    /// historical behavior; `Top` hangs the text from `y` (baseline at
    /// `y − ascent × size`, matching iText `SetFixedPosition`); `Bottom` rests
    /// the descender line on `y`. Ascent/descent come from the embedded font's
    /// own metrics. The anchor shift follows `rotation_deg` (it is applied
    /// perpendicular to the baseline direction).
    #[allow(clippy::too_many_arguments)]
    pub fn place_text_with_font_anchored(
        &mut self,
        index: usize,
        x: f64,
        y: f64,
        text: &str,
        size: f64,
        color: (f64, f64, f64),
        rotation_deg: f64,
        align: Align,
        font_id: usize,
        anchor: VerticalAnchor,
    ) -> bool {
        if font_id >= self.stamp_fonts.len() {
            return false;
        }
        let Some(&page) = self.page_order.get(index) else {
            return false;
        };
        let geom = self.page_geometry(page);
        let (r, g, b) = (
            color.0.clamp(0.0, 1.0),
            color.1.clamp(0.0, 1.0),
            color.2.clamp(0.0, 1.0),
        );
        // Shape the run, collect used glyphs / ToUnicode, and build the 2-byte
        // CID hex string plus the run's advance width (in text-space points).
        let (hex, width) = self.shape_stamp_run(font_id, text, size);
        let theta = rotation_deg.to_radians();
        let (ca, sa) = (theta.cos(), theta.sin());
        let dx = match align {
            Align::Left | Align::Justify => 0.0,
            Align::Center => -width / 2.0,
            Align::Right => -width,
        };
        let (asc, desc) = self.stamp_font_metrics(Some(font_id));
        let line = self.stamp_line_metrics(Some(font_id));
        let dy = baseline_shift(anchor, asc, desc, line, size);
        // Map the local (dx, dy) offset through the rotation so both the
        // horizontal alignment and the vertical anchor follow the text.
        let (sx, sy) = (x + dx * ca - dy * sa, y + dx * sa + dy * ca);
        let resource = self.stamp_fonts[font_id].resource.clone();
        let font_obj = self.stamp_fonts[font_id].obj;
        let content = format!(
            "q\n{upright}BT\n/{res} {size:.2} Tf\n{r:.3} {g:.3} {b:.3} rg\n\
             {ca:.5} {sa:.5} {nsa:.5} {ca:.5} {sx:.2} {sy:.2} Tm\n<{hex}> Tj\nET\nQ\n",
            upright = self.stamp_cm(&geom),
            nsa = -sa,
            res = resource,
        );
        self.append_content(page, content.into_bytes());
        self.add_page_resource(page, "Font", &resource, font_obj);
        true
    }

    /// Like [`masked_text`](EditableDoc::masked_text) but draws the text with the
    /// embedded font registered as `font_id`. The vertical centering uses the
    /// real font's cap height. Returns `false` if `index`/`font_id` is invalid.
    #[allow(clippy::too_many_arguments)]
    pub fn masked_text_with_font(
        &mut self,
        index: usize,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        text: &str,
        size: f64,
        text_color: (f64, f64, f64),
        bg_color: (f64, f64, f64),
        align: Align,
        font_id: usize,
    ) -> bool {
        self.masked_text_with_font_valign(
            index,
            x,
            y,
            width,
            height,
            text,
            size,
            text_color,
            bg_color,
            align,
            font_id,
            VerticalAlign::Middle,
        )
    }

    /// Like [`masked_text_with_font`](EditableDoc::masked_text_with_font) but
    /// with an explicit **vertical alignment** of the line inside the box.
    /// `Middle` (the historical default) centers the cap-height block; `Top`
    /// hangs the line from the top edge (baseline at
    /// `y + height − ascent × size`, matching Syncfusion `LineAlignment = Top`);
    /// `Bottom` rests the descender line on the bottom edge. Metrics come from
    /// the embedded font. Returns `false` if `index`/`font_id` is invalid.
    #[allow(clippy::too_many_arguments)]
    pub fn masked_text_with_font_valign(
        &mut self,
        index: usize,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        text: &str,
        size: f64,
        text_color: (f64, f64, f64),
        bg_color: (f64, f64, f64),
        align: Align,
        font_id: usize,
        valign: VerticalAlign,
    ) -> bool {
        self.masked_text_with_font_padded(
            index, x, y, width, height, text, size, text_color, bg_color, align, font_id, valign,
            None,
        )
    }

    /// Like [`masked_text_padded`](EditableDoc::masked_text_padded) (same
    /// `pad` semantics — `None` = historical `min(0.15 × size, width / 4)`,
    /// `Some(0.0)` = flush with the box edge) but drawing with the embedded
    /// font `font_id`.
    #[allow(clippy::too_many_arguments)]
    pub fn masked_text_with_font_padded(
        &mut self,
        index: usize,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        text: &str,
        size: f64,
        text_color: (f64, f64, f64),
        bg_color: (f64, f64, f64),
        align: Align,
        font_id: usize,
        valign: VerticalAlign,
        pad: Option<f64>,
    ) -> bool {
        if font_id >= self.stamp_fonts.len() || self.page_order.get(index).is_none() {
            return false;
        }
        self.fill_rect(index, x, y, width, height, bg_color, 1.0);
        let font = &self.stamp_fonts[font_id].font;
        let cap = font.cap_height() as f64 / font.units_per_em() as f64;
        let (asc, desc) = self.stamp_font_metrics(Some(font_id));
        let pad = pad.unwrap_or_else(|| (size * 0.15).min(width / 4.0));
        let baseline_y = masked_baseline(valign, y, height, size, cap, asc, desc);
        let anchor_x = match align {
            Align::Left | Align::Justify => x + pad,
            Align::Center => x + width / 2.0,
            Align::Right => x + width - pad,
        };
        self.place_text_with_font(
            index, anchor_x, baseline_y, text, size, text_color, 0.0, align, font_id,
        )
    }

    /// `(ascent, descent)` in em fractions (descent ≤ 0) for a stamp font:
    /// the embedded font's own metrics, or Helvetica's AFM values for the
    /// built-in stamps (`font_id = None`).
    fn stamp_font_metrics(&self, font_id: Option<usize>) -> (f64, f64) {
        match font_id {
            Some(id) => {
                let f = &self.stamp_fonts[id].font;
                let upem = f.units_per_em() as f64;
                (f.ascender() as f64 / upem, f.descender() as f64 / upem)
            }
            None => (
                crate::helvetica::ASCENT / 1000.0,
                crate::helvetica::DESCENT / 1000.0,
            ),
        }
    }

    /// `(line_ascent, line_descent)` of the **iText 7 line box** in em
    /// fractions (both positive magnitudes), for `LineTop`/`LineBottom`
    /// anchors: iText selects OS/2 **win** ascent/descent when present and
    /// distinct from the typo values, else typo × 1.2 (its
    /// `TYPO_ASCENDER_SCALE_COEFF`), and pads each side with a half-leading of
    /// `0.175 × 1.2 = 0.21` of the raw ascent+descent sum (default multiplied
    /// leading 1.35). Calibrated against iText `SetFixedPosition` output:
    /// ≤ 0.11 pt across Times New Roman and Montserrat at 12–13 pt.
    fn stamp_line_metrics(&self, font_id: Option<usize>) -> (f64, f64) {
        const HALF_LEADING: f64 = 0.21;
        let (asc, desc, half) = match font_id {
            Some(id) => {
                let f = &self.stamp_fonts[id].font;
                let upem = f.units_per_em() as f64;
                match (f.win_ascent(), f.win_descent()) {
                    (Some(wa), Some(wd))
                        if wa > 0
                            && wd < 0
                            && !(f.typo_ascender() == Some(wa)
                                && f.typo_descender() == Some(wd)) =>
                    {
                        let (a, d) = (wa as f64 / upem, -wd as f64 / upem);
                        (a, d, HALF_LEADING * (a + d))
                    }
                    _ => {
                        let a = f.typo_ascender().unwrap_or_else(|| f.ascender()) as f64 / upem;
                        let d =
                            -(f.typo_descender().unwrap_or_else(|| f.descender()) as f64) / upem;
                        (a * 1.2, d * 1.2, HALF_LEADING * (a + d))
                    }
                }
            }
            None => {
                // Standard Helvetica: AFM metrics via iText's typo × 1.2 branch.
                let a = crate::helvetica::ASCENT / 1000.0;
                let d = -crate::helvetica::DESCENT / 1000.0;
                (a * 1.2, d * 1.2, HALF_LEADING * (a + d))
            }
        };
        (asc + half, desc + half)
    }

    /// Baseline-to-baseline **advance** (em) of the iText 7 layout for the
    /// `Line*` anchors: the selected line metrics (win raw, or typo × 1.2)
    /// plus `(L − 1) × size` with the default multiplied leading `L = 1.35` —
    /// distinct from the single-line box (`stamp_line_metrics`), which pads
    /// 0.21 em on each side. Calibrated: Times 12 → 17.49 pt (bench 17.47).
    fn stamp_line_advance(&self, font_id: Option<usize>) -> f64 {
        const LEADING_EXTRA: f64 = 0.35; // iText default multiplied leading − 1
        let sum = match font_id {
            Some(id) => {
                let f = &self.stamp_fonts[id].font;
                let upem = f.units_per_em() as f64;
                match (f.win_ascent(), f.win_descent()) {
                    (Some(wa), Some(wd))
                        if wa > 0
                            && wd < 0
                            && !(f.typo_ascender() == Some(wa)
                                && f.typo_descender() == Some(wd)) =>
                    {
                        (wa as f64 - wd as f64) / upem
                    }
                    _ => {
                        let a = f.typo_ascender().unwrap_or_else(|| f.ascender()) as f64;
                        let d = f.typo_descender().unwrap_or_else(|| f.descender()) as f64;
                        (a - d) / upem * 1.2
                    }
                }
            }
            None => (crate::helvetica::ASCENT - crate::helvetica::DESCENT) / 1000.0 * 1.2,
        };
        sum + LEADING_EXTRA
    }

    /// Shape `text` with stamp font `font_id`, accumulate glyph usage, and return
    /// `(hex CID string, run width in points)`. The width is the sum of the
    /// shaper's advances scaled to `size`, matching the emitted `/W` widths.
    fn shape_stamp_run(&mut self, font_id: usize, text: &str, size: f64) -> (String, f64) {
        use fonts::{shape, Direction};
        let sf = &self.stamp_fonts[font_id];
        let upem = sf.font.units_per_em() as f64;
        let glyphs = shape(&sf.font, text, Direction::LeftToRight);

        // Cluster boundaries for ToUnicode (first glyph of a cluster carries the
        // text; the rest map to nothing — see the collect_usage F2 fix).
        let mut boundaries: BTreeSet<usize> = glyphs.iter().map(|g| g.cluster as usize).collect();
        boundaries.insert(text.len());
        let bounds: Vec<usize> = boundaries.into_iter().collect();

        let mut hex = String::with_capacity(glyphs.len() * 4);
        let mut total_adv = 0i64;
        let mut used: Vec<u16> = Vec::with_capacity(glyphs.len());
        let mut mappings: Vec<(u16, String)> = Vec::new();
        let mut assigned: BTreeSet<usize> = BTreeSet::new();
        for g in &glyphs {
            hex.push_str(&format!("{:04X}", g.gid));
            total_adv += g.x_advance as i64;
            used.push(g.gid);
            let start = g.cluster as usize;
            if assigned.insert(start) {
                let end = bounds
                    .iter()
                    .copied()
                    .find(|&b| b > start)
                    .unwrap_or(text.len());
                if let Some(slice) = text.get(start..end) {
                    mappings.push((g.gid, slice.to_string()));
                }
            }
        }
        let width = size * total_adv as f64 / upem;

        let sf = &mut self.stamp_fonts[font_id];
        for gid in used {
            sf.used.insert(gid);
        }
        for (gid, s) in mappings {
            sf.gid_to_unicode.entry(gid).or_insert(s);
        }
        (hex, width)
    }

    /// Measure `text` with stamp font `font_id` **without** recording glyph
    /// usage: shape and sum the advances, scaled to `size` points. Used by the
    /// paragraph wrapper, which measures words before deciding what to emit.
    fn measure_stamp_text(&self, font_id: usize, text: &str, size: f64) -> f64 {
        use fonts::{shape, Direction};
        let sf = &self.stamp_fonts[font_id];
        let upem = sf.font.units_per_em() as f64;
        let glyphs = shape(&sf.font, text, Direction::LeftToRight);
        let total: i64 = glyphs.iter().map(|g| g.x_advance as i64).sum();
        size * total as f64 / upem
    }

    /// Stamp a **paragraph with automatic word wrapping** on page `index`
    /// (FINDING-003): break `text` into lines that fit `width` points (greedy,
    /// by word — the same break points as [`Paragraph`](crate::Paragraph) in
    /// document generation; a single word wider than the box gets its own
    /// overflowing line), then draw each line in the standard Helvetica.
    ///
    /// `(x, y)` is the **top-left corner** of the text box (consistent with
    /// `VerticalAnchor::Top` from FINDING-002, i.e. iText `SetFixedPosition`):
    /// the first baseline lands `ascent × size` below `y`, and each further
    /// line steps down by `size × 1.2 × line_height` (pass `line_height = 1.0`
    /// for the same default leading as document-generation paragraphs).
    /// `'\n'` forces a line break. `align` lays each line out inside
    /// `[x, x + width]`; `Justify` stretches the word gaps of every line except
    /// the last of each paragraph. `max_height` (points, from `y` downward)
    /// truncates: lines whose descender would cross `y − max_height` are not
    /// drawn (iText `SetMaxHeight` semantics).
    ///
    /// Returns the number of lines actually drawn, or `None` if `index` is out
    /// of range or `width`/`size` is not positive. Text is WinAnsi like
    /// [`place_text`](EditableDoc::place_text); for an embedded font use
    /// [`place_paragraph_with_font`](EditableDoc::place_paragraph_with_font).
    #[allow(clippy::too_many_arguments)]
    pub fn place_paragraph(
        &mut self,
        index: usize,
        x: f64,
        y: f64,
        width: f64,
        text: &str,
        size: f64,
        color: (f64, f64, f64),
        align: Align,
        max_height: Option<f64>,
        line_height: f64,
    ) -> Option<usize> {
        self.place_paragraph_anchored(
            index,
            x,
            y,
            width,
            text,
            size,
            color,
            align,
            max_height,
            line_height,
            VerticalAnchor::Top,
            0.0,
        )
        .map(|(n, _)| n)
    }

    /// Like [`place_paragraph`](EditableDoc::place_paragraph) but with an
    /// explicit **block anchor** saying what `y` means, a **rotation** about
    /// the anchor, and a measured result:
    ///
    /// * `Top` (the [`place_paragraph`] default): `y` is the top of the text
    ///   box; `max_height` truncates lines below `y − max_height`.
    /// * `LineTop`: like `Top` but using the iText line-box metrics (see
    ///   [`VerticalAnchor::LineTop`]) — first baseline **and** leading come
    ///   from the line box, so one line and N lines agree vertically.
    /// * `Baseline`: `y` is the **first line's baseline**.
    /// * `Bottom` / `LineBottom`: **bottom-pinned** — the block's bottom rests
    ///   on `y` and the block grows *upward* by its real content height.
    ///   `max_height` is a **ceiling**: when the content is taller, the excess
    ///   lines are cut **from the top** (the *last* lines stay pinned to `y`);
    ///   when the content is shorter, it does NOT inflate the position.
    ///   `Bottom` uses the geometric (hhea) metrics and the plain `1.2 em`
    ///   leading; `LineBottom` uses the iText line box for both.
    ///
    /// `rotation_deg` rotates the whole laid-out block counter-clockwise
    /// **about the anchor `(x, y)`** — the anchor point is invariant under
    /// rotation (the documented pivot for every positioned stamp).
    ///
    /// Returns `(lines_drawn, consumed_height)` — the height in points of the
    /// drawn block (first line's box top to last line's box bottom, `0.0` when
    /// nothing fit), so callers can stack blocks without re-measuring.
    ///
    /// [`place_paragraph`]: EditableDoc::place_paragraph
    #[allow(clippy::too_many_arguments)]
    pub fn place_paragraph_anchored(
        &mut self,
        index: usize,
        x: f64,
        y: f64,
        width: f64,
        text: &str,
        size: f64,
        color: (f64, f64, f64),
        align: Align,
        max_height: Option<f64>,
        line_height: f64,
        anchor: VerticalAnchor,
        rotation_deg: f64,
    ) -> Option<(usize, f64)> {
        let &page = self.page_order.get(index)?;
        if !(width.is_finite() && width > 0.0 && size.is_finite() && size > 0.0) {
            return None;
        }
        let helv = self.helvetica();
        let geom = self.page_geometry(page);
        let (r, g, b) = (
            color.0.clamp(0.0, 1.0),
            color.1.clamp(0.0, 1.0),
            color.2.clamp(0.0, 1.0),
        );
        let space_w = crate::helvetica::text_width(" ", size);
        let lines = wrap_stamp_lines(text, width, space_w, |w| {
            crate::helvetica::text_width(w, size)
        });
        let (asc, desc) = self.stamp_font_metrics(None);
        let line_box = self.stamp_line_metrics(None);
        let leading = paragraph_leading(anchor, size, line_height, self.stamp_line_advance(None));
        let plan = paragraph_plan(
            anchor,
            size,
            leading,
            lines.len(),
            asc,
            desc,
            line_box,
            max_height,
        );
        let theta = rotation_deg.to_radians();
        let (ca, sa) = (theta.cos(), theta.sin());

        let mut out = format!(
            "q\n{upright}BT\n/HelvD {size:.2} Tf\n{r:.3} {g:.3} {b:.3} rg\n",
            upright = self.stamp_cm(&geom),
        );
        let mut drawn = 0usize;
        for (i, line) in lines.iter().enumerate().skip(plan.skip) {
            if drawn >= plan.take {
                break;
            }
            // Block-local baseline (anchor at 0), then rotated about (x, y).
            let ly = plan.first_local - (i - plan.skip) as f64 * leading;
            if let Some(floor) = plan.floor {
                // Truncate once the line's bottom would cross the box floor.
                if ly - plan.drop < floor - 1e-9 {
                    break;
                }
            }
            drawn += 1;
            if line.words.is_empty() {
                continue; // blank line (consecutive '\n') — occupies space only
            }
            let justify = align == Align::Justify && !line.last && line.gaps() > 0;
            let extra = if justify {
                (width - line.width).max(0.0) / line.gaps() as f64
            } else {
                0.0
            };
            if align == Align::Justify {
                // Tw applies to byte 32 of the WinAnsi string; reset (0) on
                // non-stretched lines so the last line stays natural.
                out.push_str(&format!("{extra:.3} Tw\n"));
            }
            let lx = match align {
                Align::Left | Align::Justify => 0.0,
                Align::Center => (width - line.width).max(0.0) / 2.0,
                Align::Right => (width - line.width).max(0.0),
            };
            let (gx, gy) = (x + lx * ca - ly * sa, y + lx * sa + ly * ca);
            out.push_str(&format!(
                "{ca:.5} {sa:.5} {nsa:.5} {ca:.5} {gx:.2} {gy:.2} Tm\n({txt}) Tj\n",
                nsa = -sa,
                txt = escape_pdf_literal(&line.text()),
            ));
        }
        out.push_str("ET\nQ\n");
        self.append_content(page, out.into_bytes());
        self.add_page_resource(page, "Font", "HelvD", helv);
        Some((drawn, plan.consumed_height(drawn, leading)))
    }

    /// Like [`place_paragraph`](EditableDoc::place_paragraph) but wrapping and
    /// drawing with the embedded font registered as `font_id` (from
    /// [`add_font`](EditableDoc::add_font) /
    /// [`add_font_file`](EditableDoc::add_font_file)): line breaks are computed
    /// from the real font's shaped advances, and `Top` anchoring uses its
    /// ascent/descent. Justified gaps are emitted as `TJ` adjustments (2-byte
    /// CID text ignores the `Tw` word-spacing operator). Returns the number of
    /// lines drawn, or `None` if `index`/`font_id` is out of range or
    /// `width`/`size` is not positive.
    #[allow(clippy::too_many_arguments)]
    pub fn place_paragraph_with_font(
        &mut self,
        index: usize,
        x: f64,
        y: f64,
        width: f64,
        text: &str,
        size: f64,
        color: (f64, f64, f64),
        align: Align,
        font_id: usize,
        max_height: Option<f64>,
        line_height: f64,
    ) -> Option<usize> {
        self.place_paragraph_with_font_anchored(
            index,
            x,
            y,
            width,
            text,
            size,
            color,
            align,
            font_id,
            max_height,
            line_height,
            VerticalAnchor::Top,
            0.0,
        )
        .map(|(n, _)| n)
    }

    /// Like [`place_paragraph_anchored`](EditableDoc::place_paragraph_anchored)
    /// (same block-anchor semantics) but wrapping and drawing with the embedded
    /// font `font_id`, using its real metrics for the anchors.
    #[allow(clippy::too_many_arguments)]
    pub fn place_paragraph_with_font_anchored(
        &mut self,
        index: usize,
        x: f64,
        y: f64,
        width: f64,
        text: &str,
        size: f64,
        color: (f64, f64, f64),
        align: Align,
        font_id: usize,
        max_height: Option<f64>,
        line_height: f64,
        anchor: VerticalAnchor,
        rotation_deg: f64,
    ) -> Option<(usize, f64)> {
        if font_id >= self.stamp_fonts.len() {
            return None;
        }
        let &page = self.page_order.get(index)?;
        if !(width.is_finite() && width > 0.0 && size.is_finite() && size > 0.0) {
            return None;
        }
        let geom = self.page_geometry(page);
        let (r, g, b) = (
            color.0.clamp(0.0, 1.0),
            color.1.clamp(0.0, 1.0),
            color.2.clamp(0.0, 1.0),
        );
        let space_w = self.measure_stamp_text(font_id, " ", size);
        let lines = wrap_stamp_lines(text, width, space_w, |w| {
            self.measure_stamp_text(font_id, w, size)
        });
        let (asc, desc) = self.stamp_font_metrics(Some(font_id));
        let line_box = self.stamp_line_metrics(Some(font_id));
        let leading = paragraph_leading(
            anchor,
            size,
            line_height,
            self.stamp_line_advance(Some(font_id)),
        );
        let plan = paragraph_plan(
            anchor,
            size,
            leading,
            lines.len(),
            asc,
            desc,
            line_box,
            max_height,
        );
        let theta = rotation_deg.to_radians();
        let (ca, sa) = (theta.cos(), theta.sin());
        let resource = self.stamp_fonts[font_id].resource.clone();
        let font_obj = self.stamp_fonts[font_id].obj;

        let mut out = format!(
            "q\n{upright}BT\n/{resource} {size:.2} Tf\n{r:.3} {g:.3} {b:.3} rg\n",
            upright = self.stamp_cm(&geom),
        );
        let mut drawn = 0usize;
        for (i, line) in lines.iter().enumerate().skip(plan.skip) {
            if drawn >= plan.take {
                break;
            }
            let ly = plan.first_local - (i - plan.skip) as f64 * leading;
            if let Some(floor) = plan.floor {
                if ly - plan.drop < floor - 1e-9 {
                    break;
                }
            }
            drawn += 1;
            if line.words.is_empty() {
                continue;
            }
            let justify = align == Align::Justify && !line.last && line.gaps() > 0;
            let lx = match align {
                Align::Left | Align::Justify => 0.0,
                Align::Center => (width - line.width).max(0.0) / 2.0,
                Align::Right => (width - line.width).max(0.0),
            };
            let (gx, gy) = (x + lx * ca - ly * sa, y + lx * sa + ly * ca);
            let tm = format!(
                "{ca:.5} {sa:.5} {nsa:.5} {ca:.5} {gx:.2} {gy:.2} Tm",
                nsa = -sa
            );
            if justify {
                // Per-word runs glued by TJ adjustments carrying the (stretched)
                // gap: a positive TJ number shrinks the displacement, so the gap
                // G points becomes −G·1000/size thousandths.
                let extra = (width - line.width).max(0.0) / line.gaps() as f64;
                let gap = -((space_w + extra) * 1000.0 / size);
                let mut seg = String::new();
                for (wi, word) in line.words.iter().enumerate() {
                    if wi > 0 {
                        seg.push_str(&format!(" {gap:.1} "));
                    }
                    let (hex, _) = self.shape_stamp_run(font_id, word, size);
                    seg.push_str(&format!("<{hex}>"));
                }
                out.push_str(&format!("{tm}\n[{seg}] TJ\n"));
            } else {
                let (hex, _) = self.shape_stamp_run(font_id, &line.text(), size);
                out.push_str(&format!("{tm}\n<{hex}> Tj\n"));
            }
        }
        out.push_str("ET\nQ\n");
        self.append_content(page, out.into_bytes());
        self.add_page_resource(page, "Font", &resource, font_obj);
        Some((drawn, plan.consumed_height(drawn, leading)))
    }

    /// Draw a line of **positioned text** with its baseline starting at
    /// `(x, y)` on page `index`, using the standard Helvetica font at `size`
    /// points and the given `color` (RGB, each `0..=1`). `rotation_deg` rotates
    /// the text counter-clockwise about its anchor `(x, y)` — pass `0.0` for
    /// horizontal text, or match the page rotation to follow a rotated page.
    ///
    /// `text` should be WinAnsi (Latin-1), like other standard-font stamps.
    /// Coordinates are in the page's **visible** space (origin at the displayed
    /// lower-left, y up). Returns `false` if `index` is out of range.
    ///
    /// To stamp with an arbitrary embedded font (e.g. Times New Roman), register
    /// it with [`add_font`](EditableDoc::add_font) and use
    /// [`place_text_with_font`](EditableDoc::place_text_with_font).
    #[allow(clippy::too_many_arguments)]
    pub fn place_text(
        &mut self,
        index: usize,
        x: f64,
        y: f64,
        text: &str,
        size: f64,
        color: (f64, f64, f64),
        rotation_deg: f64,
    ) -> bool {
        self.place_text_aligned(index, x, y, text, size, color, rotation_deg, Align::Left)
    }

    /// Like [`place_text`](EditableDoc::place_text) but with horizontal
    /// **alignment** relative to the anchor `(x, y)`: `Align::Left` starts the
    /// text at the anchor (the default), `Align::Center` centers it on the
    /// anchor, and `Align::Right` ends it at the anchor. The text width is
    /// measured with the standard Helvetica metrics. `Align::Justify` behaves
    /// like `Left` (there is a single line to justify). Honors `rotation_deg`
    /// (the shift is applied along the text's baseline direction).
    #[allow(clippy::too_many_arguments)]
    pub fn place_text_aligned(
        &mut self,
        index: usize,
        x: f64,
        y: f64,
        text: &str,
        size: f64,
        color: (f64, f64, f64),
        rotation_deg: f64,
        align: Align,
    ) -> bool {
        self.place_text_anchored(
            index,
            x,
            y,
            text,
            size,
            color,
            rotation_deg,
            align,
            VerticalAnchor::Baseline,
        )
    }

    /// Like [`place_text_aligned`](EditableDoc::place_text_aligned) but with an
    /// explicit **vertical anchor**: what `y` means. `Baseline` is the
    /// historical behavior; `Top` hangs the text from `y` (baseline at
    /// `y − ascent × size`, matching iText `SetFixedPosition`); `Bottom` rests
    /// the descender line on `y`. Ascent/descent are Helvetica's AFM metrics;
    /// for an embedded font use
    /// [`place_text_with_font_anchored`](EditableDoc::place_text_with_font_anchored).
    /// The anchor shift follows `rotation_deg`.
    #[allow(clippy::too_many_arguments)]
    pub fn place_text_anchored(
        &mut self,
        index: usize,
        x: f64,
        y: f64,
        text: &str,
        size: f64,
        color: (f64, f64, f64),
        rotation_deg: f64,
        align: Align,
        anchor: VerticalAnchor,
    ) -> bool {
        let Some(&page) = self.page_order.get(index) else {
            return false;
        };
        let helv = self.helvetica();
        let geom = self.page_geometry(page);
        let (r, g, b) = (
            color.0.clamp(0.0, 1.0),
            color.1.clamp(0.0, 1.0),
            color.2.clamp(0.0, 1.0),
        );
        let theta = rotation_deg.to_radians();
        let (ca, sa) = (theta.cos(), theta.sin());
        // Shift the start point along the baseline direction (ca, sa) so the run
        // is left/center/right-aligned on the anchor.
        let dx = match align {
            Align::Left | Align::Justify => 0.0,
            Align::Center => -crate::helvetica::text_width(text, size) / 2.0,
            Align::Right => -crate::helvetica::text_width(text, size),
        };
        let (asc, desc) = self.stamp_font_metrics(None);
        let line = self.stamp_line_metrics(None);
        let dy = baseline_shift(anchor, asc, desc, line, size);
        // Map the local (dx, dy) offset through the rotation so both the
        // horizontal alignment and the vertical anchor follow the text.
        let (sx, sy) = (x + dx * ca - dy * sa, y + dx * sa + dy * ca);
        // Tm rotates about the (shifted) start: [cos sin -sin cos sx sy].
        let content = format!(
            "q\n{upright}BT\n/HelvD {size:.2} Tf\n{r:.3} {g:.3} {b:.3} rg\n\
             {ca:.5} {sa:.5} {nsa:.5} {ca:.5} {sx:.2} {sy:.2} Tm\n({txt}) Tj\nET\nQ\n",
            upright = self.stamp_cm(&geom),
            nsa = -sa,
            txt = escape_pdf_literal(text),
        );
        self.append_content(page, content.into_bytes());
        self.add_page_resource(page, "Font", "HelvD", helv);
        true
    }

    /// Draw **text over a filled background box** in one call (issue #50 follow-up
    /// #5): paint an opaque rectangle `[x, y, x+width, y+height]` in `bg_color`,
    /// then write `text` (standard Helvetica, `size` points, `text_color`)
    /// horizontally aligned per `align` and **vertically centered** within the
    /// box. The classic use is masking a placeholder and stamping the real value
    /// over it without hand-computing the baseline. Coordinates are in the page's
    /// **visible** space (origin lower-left, y up). Returns `false` if `index` is
    /// out of range.
    #[allow(clippy::too_many_arguments)]
    pub fn masked_text(
        &mut self,
        index: usize,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        text: &str,
        size: f64,
        text_color: (f64, f64, f64),
        bg_color: (f64, f64, f64),
        align: Align,
    ) -> bool {
        self.masked_text_valign(
            index,
            x,
            y,
            width,
            height,
            text,
            size,
            text_color,
            bg_color,
            align,
            VerticalAlign::Middle,
        )
    }

    /// Like [`masked_text`](EditableDoc::masked_text) but with an explicit
    /// **vertical alignment** of the line inside the box. `Middle` (the
    /// historical default) centers the cap-height block; `Top` hangs the line
    /// from the top edge (baseline at `y + height − ascent × size`, matching
    /// Syncfusion `LineAlignment = Top`); `Bottom` rests the descender line on
    /// the bottom edge. Metrics are Helvetica's AFM values; for an embedded
    /// font use
    /// [`masked_text_with_font_valign`](EditableDoc::masked_text_with_font_valign).
    #[allow(clippy::too_many_arguments)]
    pub fn masked_text_valign(
        &mut self,
        index: usize,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        text: &str,
        size: f64,
        text_color: (f64, f64, f64),
        bg_color: (f64, f64, f64),
        align: Align,
        valign: VerticalAlign,
    ) -> bool {
        self.masked_text_padded(
            index, x, y, width, height, text, size, text_color, bg_color, align, valign, None,
        )
    }

    /// Like [`masked_text_valign`](EditableDoc::masked_text_valign) but with an
    /// explicit horizontal **edge inset** (`pad`, points) for `Left`/`Right`
    /// alignment: the text starts at `x + pad` (or ends at `x + width − pad`).
    /// `None` keeps the historical default `min(0.15 × size, width / 4)`; pass
    /// `Some(0.0)` to start exactly at the box edge (Syncfusion `DrawString`
    /// has no inset — FINDING-004 follow-up dX). `Center` ignores it.
    #[allow(clippy::too_many_arguments)]
    pub fn masked_text_padded(
        &mut self,
        index: usize,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        text: &str,
        size: f64,
        text_color: (f64, f64, f64),
        bg_color: (f64, f64, f64),
        align: Align,
        valign: VerticalAlign,
        pad: Option<f64>,
    ) -> bool {
        if self.page_order.get(index).is_none() {
            return false;
        }
        // Opaque background box first.
        self.fill_rect(index, x, y, width, height, bg_color, 1.0);
        // Vertical placement per valign; horizontal anchor per align, with a
        // small inset on the left/right edges so glyphs don't touch.
        let pad = pad.unwrap_or_else(|| (size * 0.15).min(width / 4.0));
        let cap = crate::helvetica::CAP_HEIGHT / 1000.0;
        let (asc, desc) = self.stamp_font_metrics(None);
        let baseline_y = masked_baseline(valign, y, height, size, cap, asc, desc);
        let anchor_x = match align {
            Align::Left | Align::Justify => x + pad,
            Align::Center => x + width / 2.0,
            Align::Right => x + width - pad,
        };
        self.place_text_aligned(
            index, anchor_x, baseline_y, text, size, text_color, 0.0, align,
        )
    }

    /// Draw an **image** on page `index` (0-based) with its lower-left corner at
    /// `(x, y)`, scaled to `width`×`height` points, rotated `rotation_deg`
    /// degrees counter-clockwise about that corner. `image` is decoded/encoded
    /// like [`Document::add_image`](crate::Document::add_image) inputs.
    ///
    /// Coordinates are in the page's **visible** space (origin at the displayed
    /// lower-left, y up), honoring the page's `/Rotate` so the image lands where
    /// a viewer sees it. Each call inserts a fresh Image XObject under a unique
    /// resource name, so repeated calls (even of the same image) never collide.
    /// Returns `false` if `index` is out of range.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image(
        &mut self,
        index: usize,
        image: &images::Image,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        rotation_deg: f64,
    ) -> bool {
        self.draw_image_anchored(
            index,
            image,
            x,
            y,
            width,
            height,
            rotation_deg,
            ImageAnchor::Corner,
        )
    }

    /// Like [`draw_image`](EditableDoc::draw_image) but with an explicit
    /// **rotation anchor**: [`ImageAnchor::Corner`] (the default of
    /// `draw_image`) keeps `(x, y)` as the image's own lower-left corner — the
    /// image sweeps *around* it when rotated; [`ImageAnchor::BoundingBox`]
    /// places the **rotated image's bounding box** with its lower-left at
    /// `(x, y)`, so the drawn pixels always land at/above/right of the anchor
    /// (iText layout semantics — e.g. a 90° image occupies
    /// `[x, x+height] × [y, y+width]`).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image_anchored(
        &mut self,
        index: usize,
        image: &images::Image,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        rotation_deg: f64,
        anchor: ImageAnchor,
    ) -> bool {
        let Some(&page) = self.page_order.get(index) else {
            return false;
        };
        let img_num = self.insert_image(image);
        let name = format!("Imd{img_num}");
        let geom = self.page_geometry(page);
        let theta = rotation_deg.to_radians();
        let (ca, sa) = (theta.cos(), theta.sin());
        // BoundingBox: shift so the rotated rect's bbox lower-left lands on
        // the anchor (the rect corners are (0,0), (w,0), (0,h), (w,h) mapped
        // through the rotation).
        let (bx, by) = match anchor {
            ImageAnchor::Corner => (0.0, 0.0),
            ImageAnchor::BoundingBox => {
                let xs = [0.0, width * ca, -height * sa, width * ca - height * sa];
                let ys = [0.0, width * sa, height * ca, width * sa + height * ca];
                (
                    -xs.iter().cloned().fold(f64::INFINITY, f64::min),
                    -ys.iter().cloned().fold(f64::INFINITY, f64::min),
                )
            }
        };
        // Rotate about the (shifted) anchor, then scale the unit image square
        // to width×height drawn up-right from it.
        let content = format!(
            "q\n{upright}{ca:.5} {sa:.5} {nsa:.5} {ca:.5} {tx:.2} {ty:.2} cm\n\
             {width:.2} 0 0 {height:.2} 0 0 cm\n/{name} Do\nQ\n",
            upright = self.stamp_cm(&geom),
            nsa = -sa,
            tx = x + bx,
            ty = y + by,
        );
        self.append_content(page, content.into_bytes());
        self.add_page_resource(page, "XObject", &name, img_num);
        true
    }

    // ---- normalization (issue #41 P1 #8) ---------------------------------

    /// Set the output PDF **version**, written as the header on the next
    /// (non-incremental) [`to_bytes`](EditableDoc::to_bytes)/[`save`](EditableDoc::save).
    /// Also clears any catalog `/Version` override (e.g. a `2.0` inherited from a
    /// PDF 2.0 input) so the header version is authoritative — the explicit
    /// downgrade path for normalizing modern files to, say, PDF 1.7.
    pub fn set_version(&mut self, version: PdfVersion) -> &mut Self {
        self.version = version;
        let cat = self.catalog;
        self.update_dict(cat, |d| {
            d.remove("Version");
        });
        self
    }

    /// Strip **PDF/A conformance** from the document: remove the catalog
    /// `/OutputIntents`, the XMP `/Metadata` carrying the `pdfaid` identifier,
    /// and any catalog `/Version` override. Use when re-purposing a PDF/A file
    /// into a plain PDF whose later edits would otherwise break A-conformance
    /// (the claim would be false). Does not re-flag the file as PDF/A.
    pub fn strip_pdfa(&mut self) -> &mut Self {
        let cat = self.catalog;
        self.update_dict(cat, |d| {
            d.remove("OutputIntents");
            d.remove("Metadata");
            d.remove("Version");
        });
        if let Some(m) = self.metadata.take() {
            self.objects.remove(&m);
        }
        self
    }

    /// Normalize the document to a plain, self-contained PDF at `version`:
    /// strips PDF/A conformance ([`strip_pdfa`](EditableDoc::strip_pdfa)) and
    /// sets the version ([`set_version`](EditableDoc::set_version)). The file is
    /// already decrypted on load (owner password accepted), so the result is a
    /// clean, downgraded, unencrypted PDF after a non-incremental save.
    pub fn normalize(&mut self, version: PdfVersion) -> &mut Self {
        self.strip_pdfa();
        self.set_version(version);
        self
    }

    /// An `/ExtGState` setting fill+stroke alpha for translucent stamps.
    fn alloc_extgstate(&mut self, opacity: f64) -> u32 {
        let n = self.allocate();
        self.objects.insert(
            n,
            Object::Dict(
                Dict::new()
                    .with("Type", Object::name("ExtGState"))
                    .with("ca", Object::Real(opacity))
                    .with("CA", Object::Real(opacity)),
            ),
        );
        n
    }

    /// Insert an [`images::Image`] (and its optional soft mask) into the object
    /// map as an Image XObject; returns the XObject object number.
    fn insert_image(&mut self, img: &images::Image) -> u32 {
        use images::{ColorSpace, Filter};
        let smask = img.soft_mask.as_ref().map(|m| {
            let dict = Dict::new()
                .with("Type", Object::name("XObject"))
                .with("Subtype", Object::name("Image"))
                .with("Width", m.width as i64)
                .with("Height", m.height as i64)
                .with("ColorSpace", Object::name("DeviceGray"))
                .with("BitsPerComponent", m.bits_per_component as i64)
                .with("Filter", Object::name("FlateDecode"));
            let n = self.allocate();
            self.objects
                .insert(n, Object::Stream(Stream::with_dict(dict, m.data.clone())));
            n
        });
        let cs = |cs: &ColorSpace| -> Object {
            match cs {
                ColorSpace::DeviceGray => Object::name("DeviceGray"),
                ColorSpace::DeviceRgb => Object::name("DeviceRGB"),
                ColorSpace::DeviceCmyk => Object::name("DeviceCMYK"),
                ColorSpace::Indexed {
                    base,
                    hival,
                    lookup,
                } => Object::Array(vec![
                    Object::name("Indexed"),
                    color_space_object(base),
                    Object::Integer(*hival as i64),
                    Object::String(PdfString::literal(lookup.clone())),
                ]),
            }
        };
        let mut dict = Dict::new()
            .with("Type", Object::name("XObject"))
            .with("Subtype", Object::name("Image"))
            .with("Width", img.width as i64)
            .with("Height", img.height as i64)
            .with("BitsPerComponent", img.bits_per_component as i64)
            .with("ColorSpace", cs(&img.color_space))
            .with(
                "Filter",
                Object::name(match img.filter {
                    Filter::DctDecode => "DCTDecode",
                    Filter::FlateDecode => "FlateDecode",
                }),
            );
        if let Some(decode) = &img.decode {
            dict.set(
                "Decode",
                Object::Array(decode.iter().map(|&v| Object::Real(v as f64)).collect()),
            );
        }
        if let Some(s) = smask {
            dict.set("SMask", Reference::new(s));
        }
        let n = self.allocate();
        self.objects
            .insert(n, Object::Stream(Stream::with_dict(dict, img.data.clone())));
        n
    }

    // ---- page content / resource helpers ---------------------------------

    /// Look up an attribute on a page, walking up `/Parent` so inheritable
    /// attributes (`/MediaBox`, `/CropBox`, `/Rotate`, `/Resources`) resolve.
    fn inherited(&self, page: u32, key: &str) -> Option<Object> {
        let mut cur = page;
        for _ in 0..32 {
            let d = as_dict(self.objects.get(&cur))?;
            if let Some(v) = d.get(key) {
                return Some(v.clone());
            }
            match d.get("Parent") {
                Some(Object::Reference(r)) => cur = r.number,
                _ => break,
            }
        }
        None
    }

    /// The page's effective geometry: crop-box origin/size (falling back to the
    /// media box, then A4) and normalized `/Rotate`. Sizes are in *unrotated*
    /// user space.
    fn page_geometry(&self, page: u32) -> PageGeom {
        let rect = |key: &str| -> Option<[f64; 4]> {
            match self.inherited(page, key) {
                Some(Object::Array(a)) => {
                    let v: Vec<f64> = a.iter().filter_map(num_f64).collect();
                    (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
                }
                _ => None,
            }
        };
        let mb = rect("MediaBox").unwrap_or([0.0, 0.0, 595.276, 841.89]);
        let cb = rect("CropBox").unwrap_or(mb);
        let rotate = self
            .inherited(page, "Rotate")
            .and_then(|o| match o {
                Object::Integer(n) => Some(n),
                Object::Real(r) => Some(r as i64),
                _ => None,
            })
            .map(|r| r.rem_euclid(360))
            .unwrap_or(0);
        PageGeom {
            x0: cb[0].min(cb[2]),
            y0: cb[1].min(cb[3]),
            w: (cb[2] - cb[0]).abs(),
            h: (cb[3] - cb[1]).abs(),
            rotate,
        }
    }

    /// The page's **visible** size in points — crop box with width/height
    /// swapped for `/Rotate` 90/270, i.e. the dimensions a viewer sees. Useful
    /// for laying out stamps. Falls back to A4.
    pub fn page_dimensions(&self, index: usize) -> (f64, f64) {
        let Some(&page) = self.page_order.get(index) else {
            return (595.276, 841.89);
        };
        self.page_geometry(page).visible_size()
    }

    /// Append `ops` (raw content-stream operators) after the page's existing
    /// content as a new stream in the `/Contents` array.
    fn append_content(&mut self, page: u32, ops: Vec<u8>) {
        let mut contents = match as_dict(self.objects.get(&page)).and_then(|d| d.get("Contents")) {
            Some(Object::Reference(r)) => vec![Object::Reference(*r)],
            Some(Object::Array(a)) => a.clone(),
            _ => Vec::new(),
        };

        // Isolate the page's original content in a balanced `q … Q` the first
        // time we stamp it. Content streams in a `/Contents` array concatenate
        // into one stream, so without this a page whose original content ends
        // with a top-level `cm` (or an unmatched `q`) would leave a non-identity
        // CTM active, and the stamp — which assumes the page's initial CTM —
        // would render at the wrong place or scale. Misplacing a redaction box
        // is a confidentiality bug, so this matters beyond cosmetics.
        if self.isolated_pages.insert(page) && !contents.is_empty() {
            let q_ref = self.allocate();
            self.objects
                .insert(q_ref, Object::Stream(Stream::new(b"q\n".to_vec())));
            let big_q_ref = self.allocate();
            self.objects
                .insert(big_q_ref, Object::Stream(Stream::new(b"\nQ\n".to_vec())));
            let mut wrapped = Vec::with_capacity(contents.len() + 2);
            wrapped.push(Object::Reference(Reference::new(q_ref)));
            wrapped.append(&mut contents);
            wrapped.push(Object::Reference(Reference::new(big_q_ref)));
            contents = wrapped;
        }

        let stream_ref = self.allocate();
        self.objects
            .insert(stream_ref, Object::Stream(Stream::new(ops)));
        contents.push(Object::Reference(Reference::new(stream_ref)));
        self.update_dict(page, |d| {
            d.set("Contents", Object::Array(contents.clone()));
        });
    }

    /// Add `name -> object` under `Resources/<category>` on `page`, deep-merging
    /// so existing fonts/xobjects/gstates are preserved.
    fn add_page_resource(&mut self, page: u32, category: &str, name: &str, target: u32) {
        // `/Resources` (and each category dict) may be an inline dict OR an
        // indirect reference — resolve both, otherwise the existing
        // fonts/xobjects/colorspaces of a page whose `/Resources` is an
        // indirect object are silently dropped and its original content (which
        // still references them) renders blank.
        let mut res = as_dict(self.objects.get(&page))
            .and_then(|d| d.get("Resources"))
            .and_then(|o| self.deref_dict(o))
            .cloned()
            .unwrap_or_default();
        let mut cat = res
            .get(category)
            .and_then(|o| self.deref_dict(o))
            .cloned()
            .unwrap_or_default();
        cat.set(name, Reference::new(target));
        res.set(category, Object::Dict(cat));
        self.update_dict(page, |d| {
            d.set("Resources", Object::Dict(res.clone()));
        });
    }

    // ---- redaction (Tier 2) ----------------------------------------------

    /// **Redact** rectangular regions on page `index` (`rects` =
    /// `[x0, y0, x1, y1]` in raw page points): every shown **glyph** whose box
    /// intersects a rect is *removed* from the content stream (surviving glyphs
    /// of the same run keep their positions via `TJ` displacements), XObject
    /// paints (images/forms) overlapping a rect are dropped — with the page's
    /// resource entry pruned and the object itself nulled when nothing else
    /// references it — and annotations whose `/Rect` intersects are deleted.
    /// Only then are opaque black boxes painted over the regions.
    ///
    /// After a successful call the redacted text is not extractable and its
    /// glyph codes are absent from the file. Returns `Ok(false)` if `index` is
    /// out of range, and **fails loudly** ([`RedactError`]) when the content
    /// cannot be safely rewritten (undecodable stream, inline `BI` image) — in
    /// that case *nothing* is removed **or drawn**, so a black box never masks
    /// data that is still present.
    ///
    /// Conservative by design: a partially covered image is removed entirely
    /// (pixel-level masking of image data is not attempted), and a Form
    /// XObject overlapping a rect is dropped whole.
    pub fn redact(
        &mut self,
        index: usize,
        rects: &[[f64; 4]],
    ) -> Result<bool, crate::redact::RedactError> {
        let Some(&page) = self.page_order.get(index) else {
            return Ok(false);
        };
        let content_nums: Vec<u32> =
            match as_dict(self.objects.get(&page)).and_then(|d| d.get("Contents")) {
                Some(Object::Reference(r)) => vec![r.number],
                Some(Object::Array(a)) => a
                    .iter()
                    .filter_map(|o| match o {
                        Object::Reference(r) => Some(r.number),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };

        // Decode + concatenate the page content. Undecodable content is a
        // hard error — silently painting over unremoved data is a leak.
        let mut content = Vec::new();
        for &n in &content_nums {
            if let Some(Object::Stream(s)) = self.objects.get(&n) {
                match decode_stream(s) {
                    Some(d) => {
                        content.extend_from_slice(&d);
                        content.push(b'\n');
                    }
                    None => return Err(crate::redact::RedactError::Undecodable),
                }
            }
        }

        if let Some(&first) = content_nums.first() {
            let fonts = self.redact_fonts(page);
            let outcome = crate::redact::redact_content(&content, rects, &fonts)?;
            // Replace the first content stream with the redacted bytes and
            // drop the rest, so the removed text is gone from the file.
            self.objects
                .insert(first, Object::Stream(Stream::new(outcome.content)));
            for &n in &content_nums[1..] {
                self.objects.remove(&n);
            }
            self.update_dict(page, |d| {
                d.set("Contents", Object::Reference(Reference::new(first)));
            });
            // Prune XObjects whose every paint was dropped: remove this page's
            // resource entry and null the object once nothing references it.
            for name in outcome.dropped_xobjects.difference(&outcome.used_xobjects) {
                let name = String::from_utf8_lossy(name).into_owned();
                if let Some(num) = self.remove_page_xobject_entry(page, &name) {
                    if !self.is_referenced(num) {
                        self.objects.insert(num, Object::Null);
                    }
                }
            }
        }

        // Remove annotations whose /Rect intersects a redaction rect.
        self.remove_intersecting_annots(page, rects);

        // Only after removal succeeded: paint opaque black rectangles.
        // Redaction is a licensed (Enterprise) feature, enforced at output
        // (`to_bytes`/`save`) so a missing license fails serialization with a
        // clear error instead of silently dropping the redaction.
        self.redacted = true;
        let mut boxes = String::from("q\n0 g\n");
        for r in rects {
            let x0 = r[0].min(r[2]);
            let y0 = r[1].min(r[3]);
            let w = (r[2] - r[0]).abs();
            let h = (r[3] - r[1]).abs();
            boxes.push_str(&format!("{x0:.2} {y0:.2} {w:.2} {h:.2} re\n"));
        }
        boxes.push_str("f\nQ\n");
        self.append_content(page, boxes.into_bytes());
        Ok(true)
    }

    /// The page's effective `/Resources` dict (own or inherited via `/Parent`).
    fn page_resources_dict(&self, page: u32) -> Option<&Dict> {
        let mut cur = page;
        for _ in 0..32 {
            let d = as_dict(self.objects.get(&cur))?;
            if let Some(res) = d.get("Resources") {
                return self.deref_dict(res);
            }
            match d.get("Parent") {
                Some(Object::Reference(r)) => cur = r.number,
                _ => return None,
            }
        }
        None
    }

    /// Width oracles for every font in the page's resources (redaction glyph
    /// measurement): Type0 → `/W`+`/DW` keyed by CID; simple → `/FirstChar`+
    /// `/Widths` keyed by byte code, `/MissingWidth` fallback.
    fn redact_fonts(&self, page: u32) -> BTreeMap<Vec<u8>, crate::redact::RFont> {
        let mut out = BTreeMap::new();
        let Some(fonts) = self
            .page_resources_dict(page)
            .and_then(|r| r.get("Font"))
            .and_then(|o| self.deref_dict(o))
            .cloned()
        else {
            return out;
        };
        for (name, fo) in fonts.iter() {
            let Some(fd) = self.deref_dict(fo) else {
                continue;
            };
            let two_byte =
                matches!(fd.get("Subtype"), Some(Object::Name(n)) if n.as_str() == "Type0");
            let mut widths: BTreeMap<u32, f64> = BTreeMap::new();
            let mut default_width = 0.5;
            if two_byte {
                if let Some(cid) = fd
                    .get("DescendantFonts")
                    .map(|o| self.deref_obj(o))
                    .and_then(|o| match o {
                        Object::Array(a) => a.first().and_then(|f| self.deref_dict(f)),
                        Object::Dict(d) => Some(d),
                        _ => None,
                    })
                {
                    if let Some(dw) = cid.get("DW").and_then(obj_num) {
                        default_width = dw / 1000.0;
                    } else {
                        default_width = 1.0;
                    }
                    if let Some(Object::Array(w)) = cid.get("W").map(|o| self.deref_obj(o)) {
                        parse_cid_widths(w, &mut widths);
                    }
                }
            } else {
                let first = fd.get("FirstChar").and_then(obj_num).unwrap_or(0.0) as i64;
                if let Some(Object::Array(a)) = fd.get("Widths").map(|o| self.deref_obj(o)) {
                    for (i, w) in a.iter().enumerate() {
                        if let Some(wv) = obj_num(w) {
                            widths.insert((first + i as i64) as u32, wv / 1000.0);
                        }
                    }
                }
                if let Some(mw) = fd
                    .get("FontDescriptor")
                    .and_then(|o| self.deref_dict(o))
                    .and_then(|d| d.get("MissingWidth"))
                    .and_then(obj_num)
                {
                    default_width = mw / 1000.0;
                }
            }
            out.insert(
                name.as_str().as_bytes().to_vec(),
                crate::redact::RFont {
                    two_byte,
                    widths,
                    default_width,
                },
            );
        }
        out
    }

    /// Resolve a reference to its object (identity for direct objects).
    fn deref_obj<'a>(&'a self, o: &'a Object) -> &'a Object {
        match o {
            Object::Reference(r) => self.objects.get(&r.number).unwrap_or(o),
            other => other,
        }
    }

    /// Remove `name` from this page's `/Resources /XObject`, giving the page
    /// its **own** resources copy first (never mutating a dict shared with
    /// other pages). Returns the removed entry's object number.
    fn remove_page_xobject_entry(&mut self, page: u32, name: &str) -> Option<u32> {
        let mut res = self.page_resources_dict(page).cloned().unwrap_or_default();
        let mut xdict = res
            .get("XObject")
            .and_then(|o| self.deref_dict(o))
            .cloned()
            .unwrap_or_default();
        let num = match xdict.remove(name) {
            Some(Object::Reference(r)) => Some(r.number),
            Some(_) | None => None,
        };
        res.set("XObject", Object::Dict(xdict));
        self.update_dict(page, |d| {
            d.set("Resources", Object::Dict(res));
        });
        num
    }

    /// Delete page annotations whose `/Rect` overlaps any redaction rect;
    /// annotation objects that become unreferenced are nulled.
    fn remove_intersecting_annots(&mut self, page: u32, rects: &[[f64; 4]]) {
        let annots: Vec<Object> = match as_dict(self.objects.get(&page))
            .and_then(|d| d.get("Annots"))
            .map(|o| self.deref_obj(o))
        {
            Some(Object::Array(a)) => a.clone(),
            _ => return,
        };
        let mut kept: Vec<Object> = Vec::new();
        let mut removed_nums: Vec<u32> = Vec::new();
        for a in annots {
            let rect = self
                .deref_dict(&a)
                .and_then(|d| d.get("Rect"))
                .map(|o| self.deref_obj(o))
                .and_then(|o| match o {
                    Object::Array(v) if v.len() == 4 => {
                        let n: Vec<f64> = v.iter().filter_map(obj_num).collect();
                        (n.len() == 4).then(|| [n[0], n[1], n[2], n[3]])
                    }
                    _ => None,
                });
            let hit = rect.is_some_and(|r| {
                let b = [
                    r[0].min(r[2]),
                    r[1].min(r[3]),
                    r[0].max(r[2]),
                    r[1].max(r[3]),
                ];
                crate::redact::rect_overlaps(b, rects)
            });
            if hit {
                if let Object::Reference(r) = &a {
                    removed_nums.push(r.number);
                }
            } else {
                kept.push(a);
            }
        }
        if removed_nums.is_empty() {
            return;
        }
        self.update_dict(page, |d| {
            if kept.is_empty() {
                d.remove("Annots");
            } else {
                d.set("Annots", Object::Array(kept));
            }
        });
        for num in removed_nums {
            if !self.is_referenced(num) {
                self.objects.insert(num, Object::Null);
            }
        }
    }

    /// Whether any object in the graph still references object `num`
    /// (excluding `num`'s own body).
    fn is_referenced(&self, num: u32) -> bool {
        fn visit(o: &Object, num: u32) -> bool {
            match o {
                Object::Reference(r) => r.number == num,
                Object::Array(a) => a.iter().any(|x| visit(x, num)),
                Object::Dict(d) => d.iter().any(|(_, v)| visit(v, num)),
                Object::Stream(s) => s.dict.iter().any(|(_, v)| visit(v, num)),
                _ => false,
            }
        }
        self.objects.iter().any(|(&n, o)| n != num && visit(o, num))
    }

    // ---- 6.8 optimize -----------------------------------------------------

    /// Opt into object streams + a cross-reference stream on the next
    /// `to_bytes`/`save` (Fase 6.8). Ignored while encryption is enabled.
    pub fn compact(&mut self, on: bool) {
        self.compress = on;
    }

    /// Remove unreferenced objects and Flate-compress uncompressed streams,
    /// compact the numbering, then emit object streams + a cross-reference
    /// stream on save for the smallest output (Fase 6.8).
    pub fn optimize(&mut self) {
        self.compress = true;
        // Finalize structure first so the catalog/pages reflect edits.
        let mut objects = self.finalize();

        // Deduplicate byte-identical objects (e.g. fonts/resources repeated after
        // a merge): point every reference at one canonical copy; the duplicates
        // become unreachable and are dropped below. Pages and the catalog/pages
        // tree are excluded so identical blank pages stay distinct.
        let protected: BTreeSet<u32> = self
            .page_order
            .iter()
            .copied()
            .chain([self.catalog, self.pages_root])
            .collect();
        let mut canonical: BTreeMap<Vec<u8>, u32> = BTreeMap::new();
        let mut dedup: BTreeMap<u32, u32> = BTreeMap::new();
        for (&num, obj) in &objects {
            if protected.contains(&num) {
                continue;
            }
            let mut bytes = Vec::new();
            obj.write_to(&mut bytes);
            match canonical.get(&bytes) {
                Some(&c) => {
                    dedup.insert(num, c);
                }
                None => {
                    canonical.insert(bytes, num);
                }
            }
        }
        if !dedup.is_empty() {
            for obj in objects.values_mut() {
                remap(obj, &dedup);
            }
            for dup in dedup.keys() {
                objects.remove(dup);
            }
        }

        // Reachability from the catalog (follow everything, including /Parent).
        let mut keep = BTreeSet::new();
        let mut stack = vec![self.catalog];
        if let Some(i) = self.info {
            stack.push(i);
        }
        while let Some(n) = stack.pop() {
            if !keep.insert(n) {
                continue;
            }
            if let Some(obj) = objects.get(&n) {
                let mut refs = Vec::new();
                collect_refs_skipping(obj, &[], &mut refs);
                stack.extend(refs);
            }
        }

        // Compress uncompressed streams (skip Metadata and image XObjects).
        for obj in objects.values_mut() {
            if let Object::Stream(s) = obj {
                let has_filter = s.dict.contains_key("Filter");
                let subtype = s.dict.get("Subtype").and_then(name).unwrap_or_default();
                let ty = s.dict.get("Type").and_then(name).unwrap_or_default();
                if !has_filter && subtype != "Image" && ty != "Metadata" && !s.data.is_empty() {
                    let compressed = images::flate_encode(&s.data);
                    if compressed.len() < s.data.len() {
                        s.data = compressed;
                        s.dict.set("Filter", Object::name("FlateDecode"));
                    }
                }
            }
        }

        // Compact renumber kept objects.
        let mut map = BTreeMap::new();
        let mut next = 1u32;
        for &n in &keep {
            map.insert(n, next);
            next += 1;
        }
        let mut compacted = BTreeMap::new();
        for &n in &keep {
            if let Some(obj) = objects.get(&n) {
                let mut copy = obj.clone();
                remap(&mut copy, &map);
                compacted.insert(map[&n], copy);
            }
        }

        self.catalog = map.get(&self.catalog).copied().unwrap_or(self.catalog);
        self.pages_root = map
            .get(&self.pages_root)
            .copied()
            .unwrap_or(self.pages_root);
        self.info = self.info.and_then(|n| map.get(&n).copied());
        self.metadata = self.metadata.and_then(|n| map.get(&n).copied());
        self.page_order = self
            .page_order
            .iter()
            .filter_map(|n| map.get(n).copied())
            .collect();
        self.next_num = next;
        self.objects = compacted;
    }

    // ---- PDF/A conversion (Tier 2) ---------------------------------------

    /// Convert this **existing** document to **PDF/A** at `level` (a basic
    /// profile: A-1b, A-2b or A-3b). Adds an sRGB `OutputIntent`, PDF/A XMP
    /// metadata kept in sync with `/Info`, and a document `/ID`; for A-1b the
    /// header is forced to PDF 1.4 and object streams are disabled.
    ///
    /// Returns an error if any font is **not embedded** (PDF/A requires every
    /// font embedded, and missing programs cannot be synthesized), or if a
    /// level-A (tagged) profile is requested — a structure tree cannot be
    /// inferred from arbitrary content. Requires the PDF/A feature license.
    pub fn convert_to_pdfa(&mut self, level: crate::PdfaLevel) -> Result<(), ConvertError> {
        crate::require(license::Feature::Pdfa)?;
        if level.conformance() == Some('A') {
            return Err(ConvertError::TaggingRequired);
        }
        let missing = self.unembedded_fonts();
        if !missing.is_empty() {
            return Err(ConvertError::FontsNotEmbedded(missing));
        }

        // PDF/A may not be encrypted.
        self.encryption = None;

        // Info entries (synced into both /Info and the XMP).
        let info = self.pdfa_info_entries();
        for (k, v) in &info {
            self.set_info(k, v);
        }
        let info_refs: Vec<(&str, String)> = info.iter().map(|(k, v)| (*k, v.clone())).collect();

        // Embedded sRGB ICC profile + OutputIntent.
        let icc_num = self.allocate();
        self.objects.insert(
            icc_num,
            Object::Stream(Stream::with_dict(
                Dict::new().with("N", 3),
                crate::pdfa::SRGB_ICC.to_vec(),
            )),
        );
        let oi = Dict::new()
            .with("Type", Object::name("OutputIntent"))
            .with("S", Object::name("GTS_PDFA1"))
            .with(
                "OutputConditionIdentifier",
                PdfString::literal(b"sRGB IEC61966-2.1".to_vec()),
            )
            .with("Info", PdfString::literal(b"sRGB IEC61966-2.1".to_vec()))
            .with("DestOutputProfile", Reference::new(icc_num));
        let oi_num = self.allocate();
        self.objects.insert(oi_num, Object::Dict(oi));
        self.update_dict(self.catalog, |d| {
            d.set(
                "OutputIntents",
                Object::Array(vec![Object::Reference(Reference::new(oi_num))]),
            );
        });

        // XMP metadata with the PDF/A identifier (also wires catalog /Metadata).
        let xmp = crate::pdfa::build_xmp(
            &info_refs,
            level.part(),
            level.conformance(),
            level.rev(),
            None,
        );
        self.set_xmp(xmp.into_bytes());

        // Deterministic /ID + version constraints per part.
        let id = crate::pdfa::document_id(&info_refs);
        self.forced_id = Some([id.clone(), id]);
        if level.part() == 1 {
            self.version = PdfVersion::V1_4;
            self.compress = false;
        } else if level.part() == 4 {
            // PDF/A-4 is based on PDF 2.0.
            self.version = PdfVersion::V2_0;
            self.update_dict(self.catalog, |d| {
                d.set("Version", Object::name("2.0"));
            });
        }
        Ok(())
    }

    /// `/Info` entries to mirror into the PDF/A XMP (existing values + Producer).
    fn pdfa_info_entries(&self) -> Vec<(&'static str, String)> {
        let mut out: Vec<(&'static str, String)> = Vec::new();
        if let Some(info) = self.info {
            if let Some(d) = as_dict(self.objects.get(&info)) {
                for key in [
                    "Title", "Author", "Subject", "Keywords", "Creator", "Producer",
                ] {
                    if let Some(v) = d.get(key).and_then(string_value) {
                        out.push((key, v));
                    }
                }
            }
        }
        if !out.iter().any(|(k, _)| *k == "Producer") {
            out.push((
                "Producer",
                concat!("rust-pdf ", env!("CARGO_PKG_VERSION")).to_string(),
            ));
        }
        out
    }

    /// Base-font names of fonts that are **not** embedded (PDF/A violation).
    fn unembedded_fonts(&self) -> Vec<String> {
        let mut missing = Vec::new();
        for obj in self.objects.values() {
            let Some(d) = as_dict(Some(obj)) else {
                continue;
            };
            if name(d.get("Type").unwrap_or(&Object::Null)).as_deref() != Some("Font") {
                continue;
            }
            let subtype = d.get("Subtype").and_then(name).unwrap_or_default();
            let base = d
                .get("BaseFont")
                .and_then(name)
                .unwrap_or_else(|| "<unnamed>".into());
            let embedded = match subtype.as_str() {
                "Type0" => self.type0_embedded(d),
                "Type3" => true, // glyph procedures are embedded by definition
                _ => self.descriptor_embedded(d),
            };
            if !embedded {
                missing.push(base);
            }
        }
        missing.sort();
        missing.dedup();
        missing
    }

    fn deref_dict<'a>(&'a self, o: &'a Object) -> Option<&'a Dict> {
        match o {
            Object::Reference(r) => as_dict(self.objects.get(&r.number)),
            other => as_dict(Some(other)),
        }
    }

    fn descriptor_embedded(&self, font: &Dict) -> bool {
        let Some(fd) = font.get("FontDescriptor").and_then(|o| self.deref_dict(o)) else {
            return false;
        };
        fd.contains_key("FontFile") || fd.contains_key("FontFile2") || fd.contains_key("FontFile3")
    }

    fn type0_embedded(&self, font: &Dict) -> bool {
        let df = match font.get("DescendantFonts") {
            Some(Object::Array(a)) => a.first().cloned(),
            Some(Object::Reference(r)) => self.objects.get(&r.number).and_then(|o| match o {
                Object::Array(a) => a.first().cloned(),
                _ => None,
            }),
            _ => None,
        };
        df.as_ref()
            .and_then(|o| self.deref_dict(o))
            .map(|cid| self.descriptor_embedded(cid))
            .unwrap_or(false)
    }

    // ---- 7.3 encryption ---------------------------------------------------

    /// Encrypt the document with AES-128 (V4/R4) on the next `to_bytes`/`save`.
    /// An empty `user_password` lets viewers open it while honoring `perms`.
    pub fn encrypt(&mut self, user_password: &str, owner_password: &str, perms: Permissions) {
        self.encrypt_with(Encryption::Aes128, user_password, owner_password, perms);
    }

    /// Encrypt with an explicit cipher (RC4-128, AES-128 or AES-256/R6).
    pub fn encrypt_with(
        &mut self,
        method: Encryption,
        user_password: &str,
        owner_password: &str,
        perms: Permissions,
    ) {
        self.encryption = Some(EncryptConfig {
            user: user_password.as_bytes().to_vec(),
            owner: owner_password.as_bytes().to_vec(),
            perms,
            method,
        });
    }

    // ---- serialization ----------------------------------------------------

    /// Serialize to PDF bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, BuildError> {
        if self.redacted {
            crate::require(license::Feature::Redaction)?;
        }
        // A valid PDF must have at least one page; an empty /Pages tree is
        // rejected by qpdf/mutool ("malformed page tree"). This also catches
        // the case where a corrupt/truncated input parsed into zero pages —
        // without this guard we would silently write an invalid file.
        if self.page_order.is_empty() {
            return Err(BuildError::Invalid(
                "document has no pages; nothing to serialize (a corrupt or truncated \
                 input may have parsed into zero pages)"
                    .into(),
            ));
        }
        let mut objects = self.finalize();
        let mut encrypt_ref = None;
        let mut id = None;

        if let Some(cfg) = &self.encryption {
            crate::require(license::Feature::Encryption)?;
            let id0 = encrypt::derive_id(objects.len());
            let prepared = encrypt::prepare(cfg, id0.clone());
            // Encrypt every object's strings/streams (generation 0).
            let mut enc: BTreeMap<u32, Object> = BTreeMap::new();
            for (&num, obj) in &objects {
                enc.insert(num, prepared.encrypt_object(num, obj));
            }
            // The /Encrypt dict itself is added afterwards (never encrypted).
            let enc_num = objects.keys().copied().max().unwrap_or(0) + 1;
            enc.insert(enc_num, Object::Dict(prepared.dict.clone()));
            objects = enc;
            encrypt_ref = Some(enc_num);
            id = Some([id0.clone(), id0]);
        }

        // A forced /ID (e.g. set by PDF/A conversion) applies when not encrypting.
        if id.is_none() {
            id = self.forced_id.clone();
        }

        let max = objects.keys().copied().max().unwrap_or(0);
        let mut w = WriterDoc::new(self.version);
        for _ in 0..max {
            w.reserve();
        }
        for n in 1..=max {
            w.assign(Reference::new(n), Object::Null);
        }
        for (&n, obj) in &objects {
            w.assign(Reference::new(n), normalize(obj));
        }
        w.set_root(Reference::new(self.catalog));
        // Object streams (no effect when encrypting; the writer guards on that).
        w.set_object_streams(self.compress);
        if let Some(info) = self.info {
            w.set_info(Reference::new(info));
        }
        if let Some(r) = encrypt_ref {
            w.set_encrypt(Reference::new(r));
        }
        if let Some(id) = id {
            w.set_id(id);
        }
        Ok(w.write()?)
    }

    /// Save to a file.
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        let bytes = self
            .to_bytes()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        std::fs::write(path, bytes)
    }

    /// Serialize as an **incremental update** appended to `original`: only the
    /// objects that differ from (or are new since) the original file are
    /// written, followed by a fresh classic cross-reference section chained to
    /// the previous one via `/Prev`. The original bytes are preserved verbatim,
    /// so existing signatures over them stay valid. Encryption/object-stream
    /// settings are ignored for the appended section (it is a classic update).
    pub fn to_bytes_incremental(&self, original: &[u8]) -> Result<Vec<u8>, BuildError> {
        if self.redacted {
            crate::require(license::Feature::Redaction)?;
        }
        let prev_startxref = find_last_startxref(original)
            .ok_or_else(|| BuildError::Parse("no startxref".into()))?;
        let reader =
            PdfReader::parse(original).map_err(|e| BuildError::Parse(format!("original: {e}")))?;

        // Serialized form of each object in the original, to diff against.
        let mut orig: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
        for n in reader.object_numbers() {
            if let Some(o) = reader.get(n) {
                let mut b = Vec::new();
                normalize(o).write_to(&mut b);
                orig.insert(n, b);
            }
        }

        let current = self.finalize();
        // Changed or newly-added objects (by serialized bytes).
        let mut changed: Vec<(u32, Vec<u8>)> = Vec::new();
        for (&num, obj) in &current {
            let mut b = Vec::new();
            normalize(obj).write_to(&mut b);
            if orig.get(&num) != Some(&b) {
                changed.push((num, b));
            }
        }
        // Objects deleted since the original become free entries.
        let deleted: Vec<u32> = orig
            .keys()
            .copied()
            .filter(|n| !current.contains_key(n))
            .collect();

        let mut out = original.to_vec();
        if !out.ends_with(b"\n") {
            out.push(b'\n');
        }

        // Append each changed object, recording its new offset.
        let mut offsets: BTreeMap<u32, u64> = BTreeMap::new();
        for (num, _) in &changed {
            let obj = &current[num];
            offsets.insert(*num, out.len() as u64);
            out.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            normalize(obj).write_to(&mut out);
            out.extend_from_slice(b"\nendobj\n");
        }

        // Build the incremental classic xref: changed (type n) + deleted (free),
        // grouped into contiguous subsections.
        let mut entries: BTreeMap<u32, Option<u64>> = BTreeMap::new();
        for (num, off) in &offsets {
            entries.insert(*num, Some(*off));
        }
        for d in &deleted {
            entries.insert(*d, None);
        }
        let xref_offset = out.len();
        out.extend_from_slice(b"xref\n");
        let nums: Vec<u32> = entries.keys().copied().collect();
        let mut i = 0;
        while i < nums.len() {
            let start = nums[i];
            let mut j = i;
            while j + 1 < nums.len() && nums[j + 1] == nums[j] + 1 {
                j += 1;
            }
            let count = j - i + 1;
            out.extend_from_slice(format!("{start} {count}\n").as_bytes());
            for &n in &nums[i..=j] {
                match entries[&n] {
                    Some(off) => out.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes()),
                    None => out.extend_from_slice(b"0000000000 65535 f\r\n"),
                }
            }
            i = j + 1;
        }

        // Trailer chained to the previous section.
        let orig_size = reader
            .trailer()
            .get("Size")
            .and_then(|o| match o {
                Object::Integer(n) => Some(*n as u32),
                _ => None,
            })
            .unwrap_or(0);
        let max_num = current.keys().copied().max().unwrap_or(0);
        let size = orig_size.max(max_num + 1);

        let mut trailer = Dict::new()
            .with("Size", size as i64)
            .with("Root", Reference::new(self.catalog))
            .with("Prev", Object::Integer(prev_startxref as i64));
        if let Some(info) = self.info {
            trailer.set("Info", Reference::new(info));
        }
        if let Some(id) = reader.trailer().get("ID") {
            trailer.set("ID", id.clone());
        }
        out.extend_from_slice(b"trailer\n");
        Object::Dict(trailer).write_to(&mut out);
        out.extend_from_slice(b"\nstartxref\n");
        out.extend_from_slice(format!("{xref_offset}").as_bytes());
        out.extend_from_slice(b"\n%%EOF\n");
        Ok(out)
    }

    /// Save an incremental update over the file at `original_path`.
    pub fn save_incremental(
        &self,
        original_path: impl AsRef<std::path::Path>,
        out_path: impl AsRef<std::path::Path>,
    ) -> std::io::Result<()> {
        let original = std::fs::read(original_path)?;
        let bytes = self
            .to_bytes_incremental(&original)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        std::fs::write(out_path, bytes)
    }

    /// Produce the finalized object map: rebuilt page tree, fixed catalog and
    /// per-page `/Parent`/`/Type`.
    fn finalize(&self) -> BTreeMap<u32, Object> {
        let mut objects = self.objects.clone();

        // Rebuild the pages root.
        let kids: Vec<Object> = self
            .page_order
            .iter()
            .map(|&p| Object::Reference(Reference::new(p)))
            .collect();
        let pages_dict = Dict::new()
            .with("Type", Object::name("Pages"))
            .with("Kids", Object::Array(kids))
            .with("Count", self.page_order.len() as i64);
        objects.insert(self.pages_root, Object::Dict(pages_dict));

        // Push inheritable attributes onto each leaf page, then fix Parent/Type.
        // The rebuilt single-level /Pages root carries only Type/Kids/Count, so
        // any inheritable attribute the original page tree held higher up is
        // dropped here. A page that relied on inheritance — e.g. PyFPDF puts
        // /MediaBox only on /Pages, never on the leaf — would otherwise end up
        // with no resolvable /MediaBox, which renders fine in lenient viewers
        // (they default to A4) but makes our rasterizer fail with "no usable
        // MediaBox". Resolve via the ORIGINAL parent chain (`self.objects`,
        // still intact) *before* overwriting /Parent, so each page becomes
        // self-contained regardless of the source tree's depth.
        const INHERITABLE: [&str; 4] = ["MediaBox", "CropBox", "Resources", "Rotate"];
        for &p in &self.page_order {
            if let Some(Object::Dict(mut d)) = objects.get(&p).cloned() {
                for key in INHERITABLE {
                    if d.get(key).is_none() {
                        if let Some(v) = self.inherited(p, key) {
                            d.set(key, v);
                        }
                    }
                }
                d.set("Type", Object::name("Page"));
                d.set("Parent", Reference::new(self.pages_root));
                objects.insert(p, Object::Dict(d));
            }
        }

        // Ensure the catalog is well-formed.
        let mut catalog = as_dict(objects.get(&self.catalog))
            .cloned()
            .unwrap_or_default();
        catalog.set("Type", Object::name("Catalog"));
        catalog.set("Pages", Reference::new(self.pages_root));
        if let Some(m) = self.metadata {
            catalog.set("Metadata", Reference::new(m));
        }
        objects.insert(self.catalog, Object::Dict(catalog));

        // Materialize embedded stamp fonts (subset Type0) into the object graph.
        let mut next = objects.keys().copied().max().unwrap_or(0) + 1;
        for sf in &self.stamp_fonts {
            if sf.used.is_empty() {
                continue; // registered but never drawn
            }
            build_stamp_font(sf, &mut objects, &mut next);
        }

        objects
    }

    /// Apply a mutation to a dictionary object in place.
    fn update_dict(&mut self, num: u32, f: impl FnOnce(&mut Dict)) {
        if let Some(Object::Dict(d)) = self.objects.get(&num).cloned().as_mut() {
            f(d);
            self.objects.insert(num, Object::Dict(d.clone()));
        } else if let Some(Object::Stream(s)) = self.objects.get(&num).cloned().as_mut() {
            f(&mut s.dict);
            self.objects.insert(num, Object::Stream(s.clone()));
        } else {
            // Object missing or not a dict: create a fresh dict.
            let mut d = Dict::new();
            f(&mut d);
            self.objects.insert(num, Object::Dict(d));
        }
    }
}

/// Error from [`EditableDoc::convert_to_pdfa`].
#[derive(Debug)]
pub enum ConvertError {
    /// The PDF/A feature is not licensed.
    License(license::LicenseError),
    /// A level-A (tagged) profile was requested; conversion supports only the
    /// basic (level-B) profiles (A-1b/A-2b/A-3b).
    TaggingRequired,
    /// One or more fonts are not embedded (their base-font names). PDF/A
    /// requires every font embedded; embed them before converting.
    FontsNotEmbedded(Vec<String>),
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConvertError::License(e) => write!(f, "{e}"),
            ConvertError::TaggingRequired => {
                write!(f, "PDF/A conversion supports only basic (level-B) profiles")
            }
            ConvertError::FontsNotEmbedded(fonts) => {
                write!(
                    f,
                    "cannot convert: fonts not embedded: {}",
                    fonts.join(", ")
                )
            }
        }
    }
}

impl std::error::Error for ConvertError {}

impl From<license::LicenseError> for ConvertError {
    fn from(e: license::LicenseError) -> Self {
        ConvertError::License(e)
    }
}

/// Appearance, color and placement for a text watermark.
#[derive(Debug, Clone)]
pub struct WatermarkOptions {
    /// Font size in points.
    pub size: f64,
    /// Fill color `(r, g, b)`, each in `0.0..=1.0`.
    pub color: (f64, f64, f64),
    /// Fill/stroke opacity in `0.0..=1.0` (1.0 = opaque).
    pub opacity: f64,
    /// Rotation in degrees, counter-clockwise (45° is the classic diagonal).
    pub rotation_deg: f64,
    /// Draw an **opaque white box** behind the text (and render the text fully
    /// opaque) — turning the watermark into a white-out stamp that covers the
    /// content underneath, rather than a translucent overlay. `opacity` is
    /// ignored when this is set.
    pub opaque_background: bool,
}

impl Default for WatermarkOptions {
    fn default() -> Self {
        WatermarkOptions {
            size: 64.0,
            color: (0.5, 0.5, 0.5),
            opacity: 0.30,
            rotation_deg: 45.0,
            opaque_background: false,
        }
    }
}

/// What the `y` coordinate of a positioned text stamp means
/// ([`EditableDoc::place_text_anchored`] and friends).
///
/// **Which one to use:** `Baseline` for typographic control; `Top`/`Bottom`
/// for plain font geometry (ascender/descender lines); `LineTop`/`LineBottom`
/// only to reproduce iText 7's layout box (drop-in parity — the iText leading
/// model never leaks into the defaults). Full contract (units, origin,
/// spaces, rotation pivot): `docs/COORDINATES.md`.
///
/// Legacy libraries disagree on the vertical anchor: iText's
/// `SetFixedPosition` lays text down from the **top** of its box, while a raw
/// PDF `Tm` (and this library's historical behavior) anchors the **baseline**.
/// The ascent/descent used to resolve `Top`/`Bottom` come from the selected
/// font (the embedded font's own metrics, or Helvetica's AFM values for the
/// built-in stamps).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VerticalAnchor {
    /// `y` is the text **baseline** (historical default).
    #[default]
    Baseline,
    /// `y` is the **top** of the text box (ascender line): the baseline is
    /// drawn `ascent × size` below `y`.
    Top,
    /// `y` is the **bottom** of the text box (descender line): the baseline is
    /// drawn `|descent| × size` above `y`.
    Bottom,
    /// `y` is the **top of the iText line box**: the baseline is drawn
    /// `line_ascent × size` below `y`, where the line box reproduces iText 7's
    /// layout model (OS/2 **win** ascent/descent — or typo × 1.2 when the font
    /// has no distinct win metrics — plus a half-leading of
    /// `0.21 × (ascent + descent)` on each side, iText's default multiplied
    /// leading of 1.35). Use this to match `SetFixedPosition` line placement
    /// exactly, including fonts whose win and hhea metrics differ.
    LineTop,
    /// `y` is the **bottom of the iText line box**: the baseline is drawn
    /// `line_descent × size` above `y` (same model as [`LineTop`]).
    ///
    /// [`LineTop`]: VerticalAnchor::LineTop
    LineBottom,
}

/// Vertical alignment of the single text line inside a
/// [`masked_text`](EditableDoc::masked_text) box.
///
/// `Middle` (the historical default) centers the cap-height block in the box.
/// `Top` matches e.g. Syncfusion `DrawString` with `LineAlignment = Top`: the
/// line box hangs from the top edge, so the baseline sits `ascent × size`
/// below `y + height`. `Bottom` rests the descender line on the bottom edge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VerticalAlign {
    /// First line hangs from the top edge of the box.
    Top,
    /// Cap-height block vertically centered (historical default).
    #[default]
    Middle,
    /// Descender line rests on the bottom edge of the box.
    Bottom,
}

/// Coordinate space in which the **positioned stamping primitives**
/// (`fill_rect`, the `place_text`/`masked_text`/`place_paragraph` families and
/// `draw_image`) interpret their coordinates — see
/// [`set_stamp_space`](EditableDoc::set_stamp_space).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StampSpace {
    /// Coordinates in the page's **visible** space (historical default):
    /// origin at the *displayed* lower-left corner of the crop box, y up,
    /// compensating the page's `/Rotate` so a `rotation_deg = 0` stamp reads
    /// upright on screen.
    #[default]
    Visible,
    /// Coordinates in the raw **media** space (PDF user space, like an iText
    /// `PdfCanvas` / `SetFixedPosition`): origin at the media origin, no
    /// compensation for `/Rotate` or the crop-box offset. `rotation_deg` is
    /// the baseline angle *in media space* — on a `/Rotate 90` page a
    /// `rotation_deg = 0` stamp reads sideways on screen, exactly as iText
    /// draws it. Use this to reproduce coordinates computed for iText.
    ///
    /// `Visible` stays the default within 0.x; `Media` — the least surprising
    /// space for code treating the PDF as a file format — may become the
    /// default in a future major (see `docs/COORDINATES.md`).
    Media,
}

/// How a **rotated image** is anchored at `(x, y)` —
/// [`draw_image_anchored`](EditableDoc::draw_image_anchored).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImageAnchor {
    /// `(x, y)` is the image's own lower-left corner (the `draw_image`
    /// default): the image sweeps around that corner when rotated.
    #[default]
    Corner,
    /// `(x, y)` is the lower-left of the **rotated image's bounding box**:
    /// the drawn pixels always land at/above/right of the anchor, like
    /// iText's rotated-image layout.
    BoundingBox,
}

/// A page's effective geometry (crop box in unrotated user space + `/Rotate`).
struct PageGeom {
    x0: f64,
    y0: f64,
    w: f64,
    h: f64,
    rotate: i64,
}

impl PageGeom {
    /// Visible dimensions (width/height swapped for 90°/270° rotation).
    fn visible_size(&self) -> (f64, f64) {
        if self.rotate == 90 || self.rotate == 270 {
            (self.h, self.w)
        } else {
            (self.w, self.h)
        }
    }

    /// A `cm` operator mapping *visible-upright* coordinates (origin at the
    /// displayed lower-left, y up) into unrotated user space, so content drawn
    /// in this frame reads upright after the viewer applies `/Rotate`.
    fn upright_cm(&self) -> String {
        let (x0, y0, w, h) = (self.x0, self.y0, self.w, self.h);
        match self.rotate {
            90 => format!("0 1 -1 0 {:.2} {:.2} cm\n", x0 + w, y0),
            180 => format!("-1 0 0 -1 {:.2} {:.2} cm\n", x0 + w, y0 + h),
            270 => format!("0 -1 1 0 {:.2} {:.2} cm\n", x0, y0 + h),
            _ => format!("1 0 0 1 {:.2} {:.2} cm\n", x0, y0),
        }
    }
}

// ---- free helpers ----------------------------------------------------------

/// Emit a subset Type0/CIDFontType2 font for a stamp font into `objects`,
/// numbering descendants from `*next`. The Type0 dict lands at `sf.obj` (the
/// number pages already reference). Content streams emit *original* glyph ids as
/// 2-byte CIDs; a `CIDToGIDMap` stream remaps them to the subset glyph ids, and
/// `/W` + `/ToUnicode` are keyed by the original gid (= CID). This mirrors the
/// `Document` font path but keeps the CIDs stable for already-emitted content.
fn build_stamp_font(sf: &StampFont, objects: &mut BTreeMap<u32, Object>, next: &mut u32) {
    let mut alloc = |obj: Object| -> u32 {
        let n = *next;
        *next += 1;
        objects.insert(n, obj);
        n
    };

    let font = &sf.font;
    let upem = font.units_per_em();
    let to_gs = |v: f64| -> i64 { (v * 1000.0 / upem as f64).round() as i64 };

    let used: Vec<u16> = sf.used.iter().copied().collect();
    let Ok(subset) = font.subset(&used) else {
        return; // subsetting failed: skip (stamp shows nothing rather than crash)
    };

    // FontFile2 (embedded subset program).
    let mut ff_dict = Dict::new();
    ff_dict.set("Length1", subset.data.len() as i64);
    let font_file = alloc(Object::Stream(Stream::with_dict(
        ff_dict,
        subset.data.clone(),
    )));

    // CIDToGIDMap: CID (= original gid) → subset gid, big-endian 2 bytes each.
    let max_cid = used.iter().copied().max().unwrap_or(0) as usize;
    let mut c2g = vec![0u8; (max_cid + 1) * 2];
    for &old in &used {
        if let Some(new) = subset.new_gid(old) {
            let i = old as usize * 2;
            c2g[i] = (new >> 8) as u8;
            c2g[i + 1] = (new & 0xff) as u8;
        }
    }
    let cid_to_gid = alloc(Object::Stream(Stream::new(c2g)));

    // FontDescriptor.
    let bbox = font.bbox();
    let base_name = sanitize_font_name(font.postscript_name());
    let descriptor = Dict::new()
        .with("Type", Object::name("FontDescriptor"))
        .with("FontName", Object::name(base_name.clone()))
        .with("Flags", font.descriptor_flags() as i64)
        .with(
            "FontBBox",
            Object::Array(vec![
                Object::Integer(to_gs(bbox[0] as f64)),
                Object::Integer(to_gs(bbox[1] as f64)),
                Object::Integer(to_gs(bbox[2] as f64)),
                Object::Integer(to_gs(bbox[3] as f64)),
            ]),
        )
        .with("ItalicAngle", Object::Real(font.italic_angle() as f64))
        .with("Ascent", to_gs(font.ascender() as f64))
        .with("Descent", to_gs(font.descender() as f64))
        .with("CapHeight", to_gs(font.cap_height() as f64))
        .with("StemV", Object::Real(font.stem_v()))
        .with("FontFile2", Reference::new(font_file));
    let descriptor_ref = alloc(Object::Dict(descriptor));

    // /W keyed by CID (= original gid): one `cid [w]` entry per used glyph.
    let mut w_items = Vec::with_capacity(used.len() * 2);
    for &old in &used {
        w_items.push(Object::Integer(old as i64));
        w_items.push(Object::Array(vec![Object::Integer(to_gs(
            font.advance(old) as f64,
        ))]));
    }
    let cid_system_info = Dict::new()
        .with("Registry", PdfString::literal("Adobe"))
        .with("Ordering", PdfString::literal("Identity"))
        .with("Supplement", 0);
    let cid_font = Dict::new()
        .with("Type", Object::name("Font"))
        .with("Subtype", Object::name("CIDFontType2"))
        .with("BaseFont", Object::name(base_name.clone()))
        .with("CIDSystemInfo", Object::Dict(cid_system_info))
        .with("FontDescriptor", Reference::new(descriptor_ref))
        .with("CIDToGIDMap", Reference::new(cid_to_gid))
        .with("DW", 1000)
        .with("W", Object::Array(w_items));
    let cid_font_ref = alloc(Object::Dict(cid_font));

    // ToUnicode CMap keyed by CID (= original gid).
    let to_unicode = alloc(Object::Stream(Stream::new(build_stamp_tounicode(
        &sf.gid_to_unicode,
    ))));

    // Type0 root at the reserved number.
    let type0 = Dict::new()
        .with("Type", Object::name("Font"))
        .with("Subtype", Object::name("Type0"))
        .with("BaseFont", Object::name(base_name))
        .with("Encoding", Object::name("Identity-H"))
        .with(
            "DescendantFonts",
            Object::Array(vec![Reference::new(cid_font_ref).into()]),
        )
        .with("ToUnicode", Reference::new(to_unicode));
    objects.insert(sf.obj, Object::Dict(type0));
}

/// Build a minimal `ToUnicode` CMap mapping 2-byte CIDs to UTF-16BE text.
fn build_stamp_tounicode(map: &BTreeMap<u16, String>) -> Vec<u8> {
    let mut body = String::new();
    let entries: Vec<(u16, &String)> = map.iter().map(|(&g, s)| (g, s)).collect();
    for chunk in entries.chunks(100) {
        body.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (gid, s) in chunk {
            let mut u16be = String::new();
            for u in s.encode_utf16() {
                u16be.push_str(&format!("{u:04X}"));
            }
            if u16be.is_empty() {
                u16be.push_str("0000");
            }
            body.push_str(&format!("<{gid:04X}> <{u16be}>\n"));
        }
        body.push_str("endbfchar\n");
    }
    format!(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n{body}\
         endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend"
    )
    .into_bytes()
}

/// Sanitize a PostScript name for use as a PDF `/BaseFont` name.
fn sanitize_font_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| {
            c.is_ascii_graphic()
                && !matches!(c, '(' | ')' | '<' | '>' | '[' | ']' | '/' | '%' | ' ')
        })
        .collect();
    if cleaned.is_empty() {
        "Embedded".to_string()
    } else {
        cleaned
    }
}

/// A `cm` matrix that maps an appearance XObject's BBox into the widget Rect,
/// emitting `q … cm /name Do Q` (Matrix on the appearance is assumed identity).
fn flatten_draw(name: &str, rect: [f64; 4], bbox: [f64; 4]) -> String {
    let bw = bbox[2] - bbox[0];
    let bh = bbox[3] - bbox[1];
    let sx = if bw.abs() > 1e-6 {
        (rect[2] - rect[0]) / bw
    } else {
        1.0
    };
    let sy = if bh.abs() > 1e-6 {
        (rect[3] - rect[1]) / bh
    } else {
        1.0
    };
    let e = rect[0] - sx * bbox[0];
    let f = rect[1] - sy * bbox[1];
    format!("q {sx:.4} 0 0 {sy:.4} {e:.2} {f:.2} cm /{name} Do Q\n")
}

/// Numeric value of a direct object (integer or real).
fn obj_num(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

/// Parse a CIDFont `/W` array into `cid → width` (em units, i.e. /1000).
fn parse_cid_widths(arr: &[Object], map: &mut std::collections::BTreeMap<u32, f64>) {
    let mut i = 0;
    while i < arr.len() {
        let Some(c) = arr.get(i).and_then(obj_num) else {
            break;
        };
        let c = c as u32;
        match arr.get(i + 1) {
            Some(Object::Array(ws)) => {
                for (j, wo) in ws.iter().enumerate() {
                    if let Some(wv) = obj_num(wo) {
                        map.insert(c + j as u32, wv / 1000.0);
                    }
                }
                i += 2;
            }
            Some(o) => {
                let c_last = obj_num(o).map(|v| v as u32).unwrap_or(c);
                let wv = arr.get(i + 2).and_then(obj_num).unwrap_or(0.0) / 1000.0;
                for cid in c..=c_last.min(c + 65_535) {
                    map.insert(cid, wv);
                }
                i += 3;
            }
            None => break,
        }
    }
}

/// Baseline-to-baseline distance for an anchored stamped paragraph.
/// Geometric anchors (`Top`/`Baseline`/`Bottom`) keep the plain
/// `1.2 em × line_height` leading (the document-generation default); the
/// `Line*` anchors use the **iText multiplied-leading advance** (`line_adv`,
/// from `stamp_line_advance`) so wrapped blocks match iText line spacing.
fn paragraph_leading(anchor: VerticalAnchor, size: f64, line_height: f64, line_adv: f64) -> f64 {
    let lh = if line_height > 0.0 { line_height } else { 1.0 };
    let basis = match anchor {
        VerticalAnchor::LineTop | VerticalAnchor::LineBottom => line_adv,
        _ => 1.2,
    };
    size * basis * lh
}

/// Layout plan for an anchored stamped paragraph, in **block-local**
/// coordinates (the anchor `(x, y)` is the local origin, so the emitters can
/// rotate the whole block about it). Lines step down `leading` from
/// `first_local`; `skip` wrapped lines are dropped from the top (bottom-pin
/// ceiling cut), at most `take` lines are drawn, and — for the top-anchored
/// family — a line is drawn only while `local_baseline − drop ≥ floor`.
struct ParagraphPlan {
    /// Local baseline of the first *drawn* line.
    first_local: f64,
    /// Wrapped lines cut from the top (bottom-pin `max_height` overflow).
    skip: usize,
    /// Cap on drawn lines (`usize::MAX` when the floor rules instead).
    take: usize,
    /// Local-y floor for the top-anchored family (`y − max_height`).
    floor: Option<f64>,
    /// Baseline → line-box-bottom distance (points) for the floor check.
    drop: f64,
    /// Per-line box extent above/below the baseline (points), for
    /// [`consumed_height`](ParagraphPlan::consumed_height).
    box_asc: f64,
    box_desc: f64,
}

impl ParagraphPlan {
    /// Height in points actually consumed by `drawn` lines (top of the first
    /// drawn line's box to the bottom of the last one's), `0.0` when nothing
    /// was drawn.
    fn consumed_height(&self, drawn: usize, leading: f64) -> f64 {
        if drawn == 0 {
            0.0
        } else {
            self.box_asc + self.box_desc + (drawn - 1) as f64 * leading
        }
    }
}

/// Build the [`ParagraphPlan`] for an anchor. `asc`/`desc` are the font's
/// hhea metrics in em (desc ≤ 0); `line_box` the iText line box magnitudes
/// (see `stamp_line_metrics`). `Top`/`Baseline`/`Bottom` use the geometric
/// metrics; `LineTop`/`LineBottom` the line box. `Bottom`/`LineBottom` are
/// **bottom-pinned**: the block's bottom rests on the anchor, `max_height` is
/// a ceiling that cuts overflowing lines **from the top** (the last lines
/// stay pinned).
#[allow(clippy::too_many_arguments)]
fn paragraph_plan(
    anchor: VerticalAnchor,
    size: f64,
    leading: f64,
    n_lines: usize,
    asc: f64,
    desc: f64,
    line_box: (f64, f64),
    max_height: Option<f64>,
) -> ParagraphPlan {
    let geom = (asc * size, -desc * size);
    let line = (line_box.0 * size, line_box.1 * size);
    match anchor {
        VerticalAnchor::Top | VerticalAnchor::LineTop | VerticalAnchor::Baseline => {
            let (bx, first) = match anchor {
                VerticalAnchor::Top => (geom, -geom.0),
                VerticalAnchor::LineTop => (line, -line.0),
                _ => (geom, 0.0),
            };
            ParagraphPlan {
                first_local: first,
                skip: 0,
                take: usize::MAX,
                floor: max_height.map(|h| -h),
                drop: bx.1,
                box_asc: bx.0,
                box_desc: bx.1,
            }
        }
        VerticalAnchor::Bottom | VerticalAnchor::LineBottom => {
            let bx = if anchor == VerticalAnchor::Bottom {
                geom
            } else {
                line
            };
            let single = bx.0 + bx.1;
            let take = match max_height {
                Some(h) if h + 1e-9 < single => 0,
                Some(h) => ((((h - single) / leading) + 1e-9).floor() as usize + 1).min(n_lines),
                None => n_lines,
            };
            ParagraphPlan {
                first_local: bx.1 + take.saturating_sub(1) as f64 * leading,
                skip: n_lines - take,
                take,
                floor: None,
                drop: bx.1,
                box_asc: bx.0,
                box_desc: bx.1,
            }
        }
    }
}

/// Baseline offset (in text-local y, points) that realizes a
/// [`VerticalAnchor`] given the font's ascent/descent in em fractions
/// (`descent ≤ 0`). `Baseline` → 0; `Top` → the baseline drops `ascent × size`
/// below the anchor; `Bottom` → it rises `|descent| × size` above it.
fn baseline_shift(
    anchor: VerticalAnchor,
    ascent_em: f64,
    descent_em: f64,
    line: (f64, f64),
    size: f64,
) -> f64 {
    match anchor {
        VerticalAnchor::Baseline => 0.0,
        VerticalAnchor::Top => -ascent_em * size,
        VerticalAnchor::Bottom => -descent_em * size,
        VerticalAnchor::LineTop => -line.0 * size,
        VerticalAnchor::LineBottom => line.1 * size,
    }
}

/// Baseline y for a single line inside a `masked_text` box `[y, y+height]`
/// under a [`VerticalAlign`]. `Middle` keeps the historical cap-height
/// centering; `Top`/`Bottom` hang/rest the line box (ascent above the
/// baseline, `|descent|` below) on the corresponding edge. Em fractions;
/// `descent_em ≤ 0`.
fn masked_baseline(
    valign: VerticalAlign,
    y: f64,
    height: f64,
    size: f64,
    cap_em: f64,
    ascent_em: f64,
    descent_em: f64,
) -> f64 {
    match valign {
        VerticalAlign::Top => y + height - ascent_em * size,
        VerticalAlign::Middle => y + (height - size * cap_em) / 2.0,
        VerticalAlign::Bottom => y - descent_em * size,
    }
}

/// One wrapped paragraph line: its words, natural width (Σ word widths +
/// single-space gaps) in points, and whether it ends its source paragraph
/// (the last line is never justified).
struct WrapLine {
    words: Vec<String>,
    width: f64,
    last: bool,
}

impl WrapLine {
    fn gaps(&self) -> usize {
        self.words.len().saturating_sub(1)
    }

    fn text(&self) -> String {
        self.words.join(" ")
    }
}

/// Greedy word wrapping for stamped paragraphs — the same break rule as the
/// document-generation [`Paragraph`](crate::Paragraph) engine: fill each line
/// while `width + space + word ≤ box_w`; a word wider than the box gets its
/// own (overflowing) line. `'\n'` forces a break (an empty paragraph yields a
/// blank line); runs of other whitespace collapse to one space.
fn wrap_stamp_lines(
    text: &str,
    box_w: f64,
    space_w: f64,
    measure: impl Fn(&str) -> f64,
) -> Vec<WrapLine> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut cur: Vec<String> = Vec::new();
        let mut w = 0.0;
        for word in para.split_whitespace() {
            let ww = measure(word);
            if !cur.is_empty() && w + space_w + ww > box_w + 1e-9 {
                out.push(WrapLine {
                    words: std::mem::take(&mut cur),
                    width: w,
                    last: false,
                });
                w = 0.0;
            }
            if !cur.is_empty() {
                w += space_w;
            }
            w += ww;
            cur.push(word.to_string());
        }
        out.push(WrapLine {
            words: cur,
            width: w,
            last: true,
        });
    }
    out
}

/// Escape a string as a PDF literal for a **WinAnsi**-encoded standard font
/// (the Helvetica used by the stamp/watermark/form helpers). Each Unicode scalar
/// is transcoded to its WinAnsi (CP1252) byte — NOT emitted as raw UTF-8, which
/// would write a 2–3 byte sequence per accented char and render as mojibake
/// (`ç`→`Ã§`, `—`→`â€"`). Bytes outside printable ASCII are written as `\ddd`
/// octal escapes so the result stays valid ASCII (embeddable in a UTF-8 content
/// `String`); a viewer decodes the octal back to the WinAnsi code. Characters
/// with no WinAnsi representation become `?`.
fn escape_pdf_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let b = crate::helvetica::unicode_to_winansi(c).unwrap_or(b'?');
        match b {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(b as char);
            }
            0x20..=0x7E => out.push(b as char),
            // Non-printable or high (>= 0x80) WinAnsi byte → octal escape.
            _ => out.push_str(&format!("\\{b:03o}")),
        }
    }
    out
}

fn num_f64(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(n) => Some(*n as f64),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

/// Decode a content stream's bytes (uncompressed or `FlateDecode`); `None` for
/// any other filter (the caller falls back to leaving the stream untouched).
fn decode_stream(s: &Stream) -> Option<Vec<u8>> {
    match s.dict.get("Filter") {
        None => Some(s.data.clone()),
        Some(Object::Name(n)) if n.as_str() == "FlateDecode" => flate_decode(&s.data),
        Some(Object::Array(a))
            if a.len() == 1 && matches!(&a[0], Object::Name(n) if n.as_str() == "FlateDecode") =>
        {
            flate_decode(&s.data)
        }
        _ => None,
    }
}

fn flate_decode(data: &[u8]) -> Option<Vec<u8>> {
    images::flate_decode(data)
}

/// A PDF color-space object for an image color space (mirrors `image.rs`).
fn color_space_object(cs: &images::ColorSpace) -> Object {
    use images::ColorSpace;
    match cs {
        ColorSpace::DeviceGray => Object::name("DeviceGray"),
        ColorSpace::DeviceRgb => Object::name("DeviceRGB"),
        ColorSpace::DeviceCmyk => Object::name("DeviceCMYK"),
        ColorSpace::Indexed {
            base,
            hival,
            lookup,
        } => Object::Array(vec![
            Object::name("Indexed"),
            color_space_object(base),
            Object::Integer(*hival as i64),
            Object::String(PdfString::literal(lookup.clone())),
        ]),
    }
}

fn as_dict(obj: Option<&Object>) -> Option<&Dict> {
    match obj? {
        Object::Dict(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

fn reference_num(obj: Option<&Object>) -> Option<u32> {
    match obj? {
        Object::Reference(r) => Some(r.number),
        _ => None,
    }
}

fn int(o: &Object) -> Option<i64> {
    match o {
        Object::Integer(n) => Some(*n),
        _ => None,
    }
}

fn name(o: &Object) -> Option<String> {
    match o {
        Object::Name(n) => Some(n.as_str().to_string()),
        _ => None,
    }
}

fn string_value(o: &Object) -> Option<String> {
    match o {
        Object::String(s) => Some(String::from_utf8_lossy(s.as_bytes()).into_owned()),
        _ => None,
    }
}

/// Find the byte offset declared by the file's last `startxref` (the offset of
/// the most recent cross-reference section), for chaining an incremental `/Prev`.
fn find_last_startxref(bytes: &[u8]) -> Option<u64> {
    let needle = b"startxref";
    let pos = bytes.windows(needle.len()).rposition(|w| w == needle)?;
    let mut i = pos + needle.len();
    // Skip whitespace, then read the decimal offset.
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    std::str::from_utf8(&bytes[start..i]).ok()?.parse().ok()
}

fn collect_pages(
    objects: &BTreeMap<u32, Object>,
    node: u32,
    out: &mut Vec<u32>,
    seen: &mut Vec<u32>,
) {
    if seen.contains(&node) {
        return;
    }
    seen.push(node);
    let Some(dict) = as_dict(objects.get(&node)) else {
        return;
    };
    match dict.get("Type").and_then(name).as_deref() {
        Some("Pages") => {
            if let Some(Object::Array(kids)) = dict.get("Kids") {
                for kid in kids {
                    if let Object::Reference(r) = kid {
                        collect_pages(objects, r.number, out, seen);
                    }
                }
            }
        }
        // A Page (or an untyped leaf): treat as a page.
        _ => out.push(node),
    }
}

/// Remap every indirect reference number in `obj` via `map`.
fn remap(obj: &mut Object, map: &BTreeMap<u32, u32>) {
    match obj {
        Object::Reference(r) => {
            if let Some(&n) = map.get(&r.number) {
                r.number = n;
            }
        }
        Object::Array(items) => {
            for it in items {
                remap(it, map);
            }
        }
        Object::Dict(d) => *d = remap_dict(d, map),
        Object::Stream(s) => s.dict = remap_dict(&s.dict, map),
        _ => {}
    }
}

fn remap_dict(dict: &Dict, map: &BTreeMap<u32, u32>) -> Dict {
    let mut out = Dict::new();
    for (k, v) in dict.iter() {
        let mut v = v.clone();
        remap(&mut v, map);
        out.set(k.clone(), v);
    }
    out
}

/// Collect referenced object numbers, skipping the named dict keys.
fn collect_refs_skipping(obj: &Object, skip_keys: &[&str], out: &mut Vec<u32>) {
    match obj {
        Object::Reference(r) => out.push(r.number),
        Object::Array(items) => {
            for it in items {
                collect_refs_skipping(it, skip_keys, out);
            }
        }
        Object::Dict(d) => {
            for (k, v) in d.iter() {
                if !skip_keys.contains(&k.as_str()) {
                    collect_refs_skipping(v, skip_keys, out);
                }
            }
        }
        Object::Stream(s) => {
            for (k, v) in s.dict.iter() {
                if !skip_keys.contains(&k.as_str()) {
                    collect_refs_skipping(v, skip_keys, out);
                }
            }
        }
        _ => {}
    }
}

/// Drop `/Length` so the writer recomputes it from the (possibly changed) data.
fn normalize(obj: &Object) -> Object {
    match obj {
        Object::Stream(s) => {
            let mut dict = Dict::new();
            for (k, v) in s.dict.iter() {
                if k.as_str() != "Length" {
                    dict.set(k.clone(), v.clone());
                }
            }
            Object::Stream(Stream {
                dict,
                data: s.data.clone(),
            })
        }
        other => other.clone(),
    }
}

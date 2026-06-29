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
use crate::BuildError;

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
        let pages = self.page_order.clone();
        for page in pages {
            let (pw, ph) = self.page_size(page);
            let cx = pw / 2.0;
            let cy = ph / 2.0;
            let theta = opts.rotation_deg.to_radians();
            let (a, b) = (theta.cos(), theta.sin());
            let (c, d) = (-theta.sin(), theta.cos());
            // Rough Helvetica width to center the baseline on the page center.
            let tw = opts.size * 0.52 * text.chars().count() as f64;
            let (r, g, bl) = opts.color;
            let content = format!(
                "q\n/GSwm gs\nBT\n/Helvwm {size:.2} Tf\n{r:.3} {g:.3} {bl:.3} rg\n\
                 {a:.5} {b:.5} {c:.5} {d:.5} {cx:.2} {cy:.2} Tm\n\
                 {ox:.2} {oy:.2} Td\n({txt}) Tj\nET\nQ\n",
                size = opts.size,
                ox = -tw / 2.0,
                oy = -opts.size * 0.35,
                txt = escape_pdf_literal(text),
            );
            self.append_content(page, content.into_bytes());
            self.add_page_resource(page, "Font", "Helvwm", helv);
            self.add_page_resource(page, "ExtGState", "GSwm", gs);
        }
    }

    /// Stamp an **image watermark** centered on every page at `width`×`height`
    /// points, drawn at `opacity`. `image` is decoded/encoded like
    /// [`Document::add_image`](crate::Document::add_image) inputs.
    pub fn watermark_image(
        &mut self,
        image: &images::Image,
        width: f64,
        height: f64,
        opacity: f64,
    ) {
        let img_num = self.insert_image(image);
        let gs = self.alloc_extgstate(opacity);
        let pages = self.page_order.clone();
        for page in pages {
            let (pw, ph) = self.page_size(page);
            let x = (pw - width) / 2.0;
            let y = (ph - height) / 2.0;
            let content =
                format!("q\n/GSwm gs\n{width:.2} 0 0 {height:.2} {x:.2} {y:.2} cm\n/Imwm Do\nQ\n");
            self.append_content(page, content.into_bytes());
            self.add_page_resource(page, "XObject", "Imwm", img_num);
            self.add_page_resource(page, "ExtGState", "GSwm", gs);
        }
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

    /// The page's media box size in points (falls back to A4).
    fn page_size(&self, page: u32) -> (f64, f64) {
        let mb = as_dict(self.objects.get(&page)).and_then(|d| d.get("MediaBox"));
        if let Some(Object::Array(a)) = mb {
            let v: Vec<f64> = a.iter().filter_map(num_f64).collect();
            if v.len() == 4 {
                return ((v[2] - v[0]).abs(), (v[3] - v[1]).abs());
            }
        }
        (595.276, 841.89)
    }

    /// Append `ops` (raw content-stream operators) after the page's existing
    /// content as a new stream in the `/Contents` array.
    fn append_content(&mut self, page: u32, ops: Vec<u8>) {
        let stream_ref = self.allocate();
        self.objects
            .insert(stream_ref, Object::Stream(Stream::new(ops)));
        let mut contents = match as_dict(self.objects.get(&page)).and_then(|d| d.get("Contents")) {
            Some(Object::Reference(r)) => vec![Object::Reference(*r)],
            Some(Object::Array(a)) => a.clone(),
            _ => Vec::new(),
        };
        contents.push(Object::Reference(Reference::new(stream_ref)));
        self.update_dict(page, |d| {
            d.set("Contents", Object::Array(contents.clone()));
        });
    }

    /// Add `name -> object` under `Resources/<category>` on `page`, deep-merging
    /// so existing fonts/xobjects/gstates are preserved.
    fn add_page_resource(&mut self, page: u32, category: &str, name: &str, target: u32) {
        let mut res = as_dict(self.objects.get(&page))
            .and_then(|d| match d.get("Resources") {
                Some(Object::Dict(r)) => Some(r.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let mut cat = match res.get(category) {
            Some(Object::Dict(c)) => c.clone(),
            _ => Dict::new(),
        };
        cat.set(name, Reference::new(target));
        res.set(category, Object::Dict(cat));
        self.update_dict(page, |d| {
            d.set("Resources", Object::Dict(res.clone()));
        });
    }

    // ---- redaction (Tier 2) ----------------------------------------------

    /// **Redact** rectangular regions on page `index`: the text and graphics
    /// whose origin falls inside any rect in `rects` (`[x0,y0,x1,y1]`, page
    /// points) are *removed* from the content stream — not just covered — so the
    /// data is gone from the file, then opaque black rectangles are painted over
    /// the regions. Returns whether the page existed.
    ///
    /// Content is rewritten when it can be decoded (uncompressed or
    /// `FlateDecode`) and carries no inline images; otherwise the regions are
    /// still covered with black boxes but the underlying bytes are left intact
    /// (call [`EditableDoc::optimize`] is not enough there — prefer
    /// pre-decompressing such files).
    pub fn redact(&mut self, index: usize, rects: &[[f64; 4]]) -> bool {
        let Some(&page) = self.page_order.get(index) else {
            return false;
        };
        // Redaction is a licensed (Enterprise) feature, enforced at output
        // (`to_bytes`/`save`) so a missing license fails serialization with a
        // clear error instead of silently dropping the redaction.
        self.redacted = true;
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

        // Decode + concatenate the page content.
        let mut content = Vec::new();
        let mut decodable = !content_nums.is_empty();
        for &n in &content_nums {
            if let Some(Object::Stream(s)) = self.objects.get(&n) {
                match decode_stream(s) {
                    Some(d) => {
                        content.extend_from_slice(&d);
                        content.push(b'\n');
                    }
                    None => {
                        decodable = false;
                        break;
                    }
                }
            }
        }

        if decodable {
            if let (Some(filtered), Some(&first)) = (
                crate::redact::redact_content(&content, rects),
                content_nums.first(),
            ) {
                // Replace the first content stream with the redacted bytes and
                // drop the rest, so the removed text is gone from the file.
                self.objects
                    .insert(first, Object::Stream(Stream::new(filtered)));
                for &n in &content_nums[1..] {
                    self.objects.remove(&n);
                }
                self.update_dict(page, |d| {
                    d.set("Contents", Object::Reference(Reference::new(first)));
                });
            }
        }

        // Paint opaque black rectangles over the redacted regions.
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
        true
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

        // Fix each page's Parent and Type.
        for &p in &self.page_order {
            if let Some(Object::Dict(mut d)) = objects.get(&p).cloned() {
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
}

impl Default for WatermarkOptions {
    fn default() -> Self {
        WatermarkOptions {
            size: 64.0,
            color: (0.5, 0.5, 0.5),
            opacity: 0.30,
            rotation_deg: 45.0,
        }
    }
}

// ---- free helpers ----------------------------------------------------------

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

/// Escape `(`, `)` and `\` for a PDF literal string.
fn escape_pdf_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '(' | ')' | '\\') {
            out.push('\\');
        }
        out.push(c);
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

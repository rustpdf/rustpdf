//! Interactive forms (AcroForm, Fase 6.7): text fields, checkboxes, radio
//! groups and dropdown (choice) fields, each with a **generated appearance
//! stream** (`/AP /N`) so the field renders without `NeedAppearances`. Field
//! names may be hierarchical (`"address.city"`) — dotted components become
//! nested parent fields, as PDF requires for fully-qualified names.
//!
//! Field text uses the standard-14 Helvetica (and ZapfDingbats for check/dot
//! marks), declared in the AcroForm default resources (`/DR`); no embedding is
//! needed for the standard fonts.

use std::collections::BTreeMap;

use cos::{Dict, Object, PdfString, Reference, Stream};
use writer::Document as WriterDoc;

/// A rectangle `[x0, y0, x1, y1]` in page points.
pub type Rect = [f64; 4];

/// One form field declared on a [`Document`](crate::Document).
#[derive(Debug, Clone)]
pub(crate) struct FormField {
    pub name: String,
    pub page: usize,
    pub size: f64,
    pub kind: FieldKind,
}

impl FormField {
    /// Every rectangle this field draws (one for most kinds; one per button for
    /// a radio group). Used to validate against degenerate geometry.
    pub(crate) fn rects(&self) -> Vec<Rect> {
        match &self.kind {
            FieldKind::Text { rect, .. }
            | FieldKind::Checkbox { rect, .. }
            | FieldKind::Choice { rect, .. } => vec![*rect],
            FieldKind::Radio { buttons, .. } => buttons.iter().map(|(r, _)| *r).collect(),
        }
    }
}

/// A rectangle is usable only if it has positive width and height; an inverted
/// or zero-area rectangle renders nothing, so callers that pass one are almost
/// certainly making a mistake.
pub(crate) fn rect_is_valid(rect: &Rect) -> bool {
    rect[2] > rect[0] && rect[3] > rect[1]
}

#[derive(Debug, Clone)]
pub(crate) enum FieldKind {
    Text {
        value: String,
        multiline: bool,
        rect: Rect,
    },
    Checkbox {
        checked: bool,
        rect: Rect,
    },
    Radio {
        selected: Option<usize>,
        buttons: Vec<(Rect, String)>,
    },
    Choice {
        rect: Rect,
        options: Vec<String>,
        selected: Option<usize>,
        combo: bool,
    },
}

/// Builds the form objects in two phases: [`reserve`](FormBuilder::reserve)
/// allocates one widget reference per visible widget (so pages can list them in
/// `/Annots`), then [`build`](FormBuilder::build) emits the widget/field dicts,
/// appearance streams and the catalog `/AcroForm`.
pub(crate) struct FormBuilder<'a> {
    fields: &'a [FormField],
    /// Per field: the reserved widget refs (one per widget; radio has many).
    widget_refs: Vec<Vec<Reference>>,
}

impl<'a> FormBuilder<'a> {
    pub fn new(fields: &'a [FormField]) -> Self {
        FormBuilder {
            fields,
            widget_refs: Vec::new(),
        }
    }

    /// Reserve widget refs and return, per page index, the widget refs to place
    /// in that page's `/Annots`.
    pub fn reserve(&mut self, doc: &mut WriterDoc, pages: usize) -> Vec<Vec<Reference>> {
        let mut per_page = vec![Vec::new(); pages];
        for f in self.fields {
            let n = match &f.kind {
                FieldKind::Radio { buttons, .. } => buttons.len(),
                _ => 1,
            };
            let mut refs = Vec::with_capacity(n);
            for _ in 0..n {
                let r = doc.reserve();
                refs.push(r);
                if f.page < pages {
                    per_page[f.page].push(r);
                }
            }
            self.widget_refs.push(refs);
        }
        per_page
    }

    /// Emit all widget/field objects and the `/AcroForm`; returns its reference.
    /// `page_refs` is indexed by page number.
    pub fn build(self, doc: &mut WriterDoc, page_refs: &[Reference]) -> Reference {
        // Standard fonts for the default resources.
        let helv = doc.add(standard_font("Helvetica", true));
        let zadb = doc.add(standard_font("ZapfDingbats", false));

        // Build each field into one or more objects; collect the *top* field ref.
        let mut leaf_refs: Vec<(String, Reference)> = Vec::new();
        for (i, f) in self.fields.iter().enumerate() {
            let widgets = &self.widget_refs[i];
            let page = page_refs.get(f.page).copied();
            let leaf_name = last_component(&f.name);
            let field_ref = build_field(doc, f, &leaf_name, widgets, page, helv, zadb);
            leaf_refs.push((f.name.clone(), field_ref));
        }

        // Resolve hierarchy: dotted names become nested parent fields.
        let top_fields = build_hierarchy(doc, &leaf_refs);

        // Default appearance: Helvetica, black.
        let dr = Dict::new().with(
            "Font",
            Object::Dict(Dict::new().with("Helv", helv).with("ZaDb", zadb)),
        );
        let acro = Dict::new()
            .with("Fields", Object::Array(top_fields))
            .with("DR", Object::Dict(dr))
            .with("DA", PdfString::literal(b"/Helv 0 Tf 0 g".to_vec()))
            .with("NeedAppearances", Object::Bool(false));
        doc.add(acro)
    }
}

/// A standard-14 Type1 font dict.
fn standard_font(base: &str, win_ansi: bool) -> Dict {
    let mut d = Dict::new()
        .with("Type", Object::name("Font"))
        .with("Subtype", Object::name("Type1"))
        .with("BaseFont", Object::name(base));
    if win_ansi {
        d.set("Encoding", Object::name("WinAnsiEncoding"));
    }
    d
}

#[allow(clippy::too_many_arguments)]
fn build_field(
    doc: &mut WriterDoc,
    f: &FormField,
    name: &str,
    widgets: &[Reference],
    page: Option<Reference>,
    helv: Reference,
    zadb: Reference,
) -> Reference {
    match &f.kind {
        FieldKind::Text {
            value,
            multiline,
            rect,
        } => {
            let size = f.size;
            let ap = text_appearance(doc, rect, value, size, helv);
            let mut d = widget_base(name, page, *rect)
                .with("FT", Object::name("Tx"))
                .with("V", PdfString::text(value))
                .with(
                    "DA",
                    PdfString::literal(format!("/Helv {size} Tf 0 g").into_bytes()),
                )
                .with("AP", Object::Dict(Dict::new().with("N", ap)));
            if *multiline {
                d.set("Ff", Object::Integer(1 << 12)); // multiline
            }
            doc.assign(widgets[0], d);
            widgets[0]
        }
        FieldKind::Checkbox { checked, rect } => {
            let on = mark_appearance(doc, rect, "4", zadb); // ✔ in ZapfDingbats
            let off = empty_appearance(doc, rect);
            let state = if *checked { "On" } else { "Off" };
            let n = Dict::new().with("On", on).with("Off", off);
            let d = widget_base(name, page, *rect)
                .with("FT", Object::name("Btn"))
                .with("V", Object::name(state))
                .with("AS", Object::name(state))
                .with(
                    "MK",
                    Object::Dict(Dict::new().with("CA", PdfString::literal(b"4".to_vec()))),
                )
                .with("AP", Object::Dict(Dict::new().with("N", Object::Dict(n))));
            doc.assign(widgets[0], d);
            widgets[0]
        }
        FieldKind::Radio { selected, buttons } => {
            // A single field with one kid widget per button.
            let field_ref = doc.reserve();
            let mut kids = Vec::with_capacity(buttons.len());
            let on_value = selected
                .and_then(|s| buttons.get(s))
                .map(|(_, v)| v.clone());
            for (i, (rect, export)) in buttons.iter().enumerate() {
                let on = mark_appearance(doc, rect, "l", zadb); // ● dot
                let off = empty_appearance(doc, rect);
                let n = Dict::new().with(export.clone(), on).with("Off", off);
                let state = if Some(i) == *selected {
                    export.as_str()
                } else {
                    "Off"
                };
                let w = Dict::new()
                    .with("Type", Object::name("Annot"))
                    .with("Subtype", Object::name("Widget"))
                    .with("Rect", rect_array(*rect))
                    .with("F", Object::Integer(4))
                    .with("Parent", field_ref)
                    .with("AS", Object::name(state))
                    .with(
                        "MK",
                        Object::Dict(Dict::new().with("CA", PdfString::literal(b"l".to_vec()))),
                    )
                    .with("AP", Object::Dict(Dict::new().with("N", Object::Dict(n))));
                let wref = widgets[i];
                let mut w = w;
                if let Some(p) = page {
                    w.set("P", p);
                }
                doc.assign(wref, w);
                kids.push(Object::Reference(wref));
            }
            let mut field = Dict::new()
                .with("FT", Object::name("Btn"))
                .with("Ff", Object::Integer(1 << 15)) // radio
                .with("T", PdfString::text(name))
                .with("Kids", Object::Array(kids));
            field.set("V", Object::name(on_value.unwrap_or_else(|| "Off".into())));
            doc.assign(field_ref, field);
            field_ref
        }
        FieldKind::Choice {
            rect,
            options,
            selected,
            combo,
        } => {
            let value = selected
                .and_then(|s| options.get(s))
                .cloned()
                .unwrap_or_default();
            let ap = text_appearance(doc, rect, &value, f.size, helv);
            let opt = Object::Array(
                options
                    .iter()
                    .map(|o| Object::String(PdfString::text(o)))
                    .collect(),
            );
            let mut d = widget_base(name, page, *rect)
                .with("FT", Object::name("Ch"))
                .with("Opt", opt)
                .with("V", PdfString::text(&value))
                .with(
                    "DA",
                    PdfString::literal(format!("/Helv {} Tf 0 g", f.size).into_bytes()),
                )
                .with("AP", Object::Dict(Dict::new().with("N", ap)));
            if *combo {
                d.set("Ff", Object::Integer(1 << 17)); // combo box
            }
            doc.assign(widgets[0], d);
            widgets[0]
        }
    }
}

/// A merged field+widget dict (single-widget fields).
fn widget_base(name: &str, page: Option<Reference>, rect: Rect) -> Dict {
    let mut d = Dict::new()
        .with("Type", Object::name("Annot"))
        .with("Subtype", Object::name("Widget"))
        .with("T", PdfString::text(name))
        .with("Rect", rect_array(rect))
        .with("F", Object::Integer(4)); // Print
    if let Some(p) = page {
        d.set("P", p);
    }
    d
}

fn rect_array(r: Rect) -> Object {
    Object::Array(vec![
        Object::Real(r[0]),
        Object::Real(r[1]),
        Object::Real(r[2]),
        Object::Real(r[3]),
    ])
}

/// Build nested parent fields for dotted names; returns the top-level `/Fields`.
/// A name like `a.b.c` yields parent `a` → `b` → leaf `c`, each linked by
/// `/Parent` and `/Kids`, so the fully-qualified name resolves correctly.
fn build_hierarchy(doc: &mut WriterDoc, leaves: &[(String, Reference)]) -> Vec<Object> {
    let mut parents: BTreeMap<String, Reference> = BTreeMap::new();
    let mut children: BTreeMap<String, Vec<Object>> = BTreeMap::new();
    let mut top: Vec<Object> = Vec::new();
    let mut linked: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for (full, leaf_ref) in leaves {
        let comps: Vec<&str> = full.split('.').collect();
        if comps.len() == 1 {
            top.push(Object::Reference(*leaf_ref));
            continue;
        }
        // Ensure every ancestor prefix exists and is linked to its own parent.
        for k in 0..comps.len() - 1 {
            let path = comps[..=k].join(".");
            let pref = *parents.entry(path.clone()).or_insert_with(|| doc.reserve());
            if linked.insert(path.clone()) {
                if k == 0 {
                    top.push(Object::Reference(pref));
                } else {
                    let parent_path = comps[..k].join(".");
                    children
                        .entry(parent_path)
                        .or_default()
                        .push(Object::Reference(pref));
                }
            }
        }
        // Attach the leaf to its immediate parent and back-link /Parent.
        let parent_path = comps[..comps.len() - 1].join(".");
        children
            .entry(parent_path.clone())
            .or_default()
            .push(Object::Reference(*leaf_ref));
        let parent_ref = parents[&parent_path];
        doc.patch(*leaf_ref, move |d| {
            d.set("Parent", parent_ref);
        });
    }

    // Emit each parent field dict (name = last component, kids, /Parent link).
    let entries: Vec<(String, Reference)> = parents.iter().map(|(k, v)| (k.clone(), *v)).collect();
    for (path, pref) in entries {
        let name = last_component(&path);
        let kids = children.remove(&path).unwrap_or_default();
        let mut d = Dict::new()
            .with("T", PdfString::text(&name))
            .with("Kids", Object::Array(kids));
        if let Some(i) = path.rfind('.') {
            d.set("Parent", parents[&path[..i]]);
        }
        doc.assign(pref, d);
    }

    top
}

fn last_component(name: &str) -> String {
    name.rsplit('.').next().unwrap_or(name).to_string()
}

/// A text/choice appearance Form XObject showing `value`.
fn text_appearance(
    doc: &mut WriterDoc,
    rect: &Rect,
    value: &str,
    size: f64,
    helv: Reference,
) -> Reference {
    let w = rect[2] - rect[0];
    let h = rect[3] - rect[1];
    let fs = if size <= 0.0 {
        (h - 4.0).clamp(6.0, 12.0)
    } else {
        size
    };
    let ty = ((h - fs) / 2.0).max(2.0);
    let content = format!(
        "/Tx BMC\nq\nBT\n/Helv {fs:.2} Tf\n0 g\n2 {ty:.2} Td\n({}) Tj\nET\nQ\nEMC",
        escape_literal(value)
    );
    form_xobject(doc, w, h, content, helv, "Helv")
}

/// A check/dot mark appearance in ZapfDingbats (`ch` is "4" ✔ or "l" ●).
fn mark_appearance(doc: &mut WriterDoc, rect: &Rect, ch: &str, zadb: Reference) -> Reference {
    let w = rect[2] - rect[0];
    let h = rect[3] - rect[1];
    let fs = (h - 2.0).max(6.0);
    let content = format!("q\nBT\n/ZaDb {fs:.2} Tf\n0 g\n2 2 Td\n({ch}) Tj\nET\nQ",);
    form_xobject(doc, w, h, content, zadb, "ZaDb")
}

fn empty_appearance(doc: &mut WriterDoc, rect: &Rect) -> Reference {
    let w = rect[2] - rect[0];
    let h = rect[3] - rect[1];
    form_xobject(doc, w, h, String::new(), Reference::new(0), "")
}

fn form_xobject(
    doc: &mut WriterDoc,
    w: f64,
    h: f64,
    content: String,
    font: Reference,
    font_name: &str,
) -> Reference {
    let mut res = Dict::new();
    if !font_name.is_empty() {
        res.set("Font", Object::Dict(Dict::new().with(font_name, font)));
    }
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
        .with("Resources", Object::Dict(res));
    doc.add(Stream::with_dict(dict, content.into_bytes()))
}

/// Escape a field value for a content-stream literal, transcoding to the
/// appearance font's WinAnsi encoding. The `/AP` stream draws with a WinAnsi
/// Helvetica (`/Helv`), so a naive UTF-8 push would render "São" as "SÃ£o":
/// each non-ASCII char must become its single WinAnsi byte (as an octal escape
/// for the high range), matching the value stored in `/V`. Mirrors
/// `edit.rs::escape_pdf_literal`.
fn escape_literal(s: &str) -> String {
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

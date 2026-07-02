//! C ABI: document-building surface (fonts, text, images, PDF/A, tagging,
//! attachments, forms). Mirrors the high-level `pdf::Document` builder API.
//!
//! Enum arguments are passed as small integers (documented per function);
//! out-of-range values map to a sensible default rather than panicking.

use std::ffi::{c_char, c_int};

use pdf::{
    AFRelationship, Align, Bookmark, FacturxProfile, FontId, ImageId, Info, Paragraph, PdfaLevel,
    StructTag, Version,
};

use crate::{
    bytes, clear_last_error, cstr, set_last_error, transform_doc, with_current_page, with_doc,
    PdfDocument, PdfStatus,
};

// ---- enum mappings ---------------------------------------------------------

fn pdfa_level(v: c_int) -> PdfaLevel {
    match v {
        0 => PdfaLevel::A1b,
        2 => PdfaLevel::A2a,
        3 => PdfaLevel::A3b,
        4 => PdfaLevel::A3a,
        5 => PdfaLevel::A4,
        6 => PdfaLevel::A4e,
        7 => PdfaLevel::A4f,
        _ => PdfaLevel::A2b,
    }
}

pub(crate) fn version(v: c_int) -> Version {
    match v {
        0 => Version::V1_4,
        1 => Version::V1_5,
        3 => Version::V2_0,
        _ => Version::V1_7,
    }
}

fn align(v: c_int) -> Align {
    match v {
        1 => Align::Right,
        2 => Align::Center,
        3 => Align::Justify,
        _ => Align::Left,
    }
}

fn af_rel(v: c_int) -> AFRelationship {
    match v {
        0 => AFRelationship::Source,
        1 => AFRelationship::Data,
        2 => AFRelationship::Alternative,
        3 => AFRelationship::Supplement,
        _ => AFRelationship::Unspecified,
    }
}

fn facturx_profile(v: c_int) -> FacturxProfile {
    match v {
        0 => FacturxProfile::Minimum,
        1 => FacturxProfile::BasicWl,
        2 => FacturxProfile::Basic,
        4 => FacturxProfile::Extended,
        _ => FacturxProfile::En16931,
    }
}

/// An optional UTF-8 string argument: NULL → `None`.
///
/// # Safety
/// `p` must be NULL or a valid NUL-terminated UTF-8 C string.
unsafe fn opt_str(p: *const c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    unsafe { std::ffi::CStr::from_ptr(p) }
        .to_str()
        .ok()
        .map(|s| s.to_string())
}

// ---- document configuration (consuming builders) ---------------------------

/// Mark the document as **PDF/A-2b**.
///
/// # Safety
/// `doc` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn pdf_document_pdfa(doc: *mut PdfDocument) -> PdfStatus {
    transform_doc(doc, "pdf_document_pdfa", |d| d.pdfa())
}

/// Mark the document as PDF/A at `level`: 0=A-1b, 1=A-2b, 2=A-2a, 3=A-3b,
/// 4=A-3a, 5=A-4, 6=A-4e, 7=A-4f. Level-A variants also enable tagging;
/// the A-4 family is based on PDF 2.0.
///
/// # Safety
/// `doc` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn pdf_document_pdfa_level(doc: *mut PdfDocument, level: c_int) -> PdfStatus {
    transform_doc(doc, "pdf_document_pdfa_level", |d| {
        d.pdfa_with(pdfa_level(level))
    })
}

/// Enable the tagged structure tree (accessibility).
///
/// # Safety
/// `doc` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn pdf_document_tagged(doc: *mut PdfDocument) -> PdfStatus {
    transform_doc(doc, "pdf_document_tagged", |d| d.tagged())
}

/// Set the PDF version: 0=1.4, 1=1.5, 2=1.7, 3=2.0.
///
/// # Safety
/// `doc` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn pdf_document_set_version(doc: *mut PdfDocument, v: c_int) -> PdfStatus {
    transform_doc(doc, "pdf_document_set_version", |d| {
        d.with_version(version(v))
    })
}

/// Set the default page size (points) for subsequently added pages.
///
/// # Safety
/// `doc` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn pdf_document_set_default_size(
    doc: *mut PdfDocument,
    width: f64,
    height: f64,
) -> PdfStatus {
    transform_doc(doc, "pdf_document_set_default_size", |d| {
        d.with_default_size((width, height))
    })
}

/// Set document info. Any argument may be NULL to leave it unset.
///
/// # Safety
/// `doc` must be valid; each string must be NULL or a valid C string.
#[no_mangle]
pub unsafe extern "C" fn pdf_document_set_info(
    doc: *mut PdfDocument,
    title: *const c_char,
    author: *const c_char,
    subject: *const c_char,
    keywords: *const c_char,
    creator: *const c_char,
) -> PdfStatus {
    with_doc(doc, "pdf_document_set_info", |d| {
        d.set_info(Info {
            title: unsafe { opt_str(title) },
            author: unsafe { opt_str(author) },
            subject: unsafe { opt_str(subject) },
            keywords: unsafe { opt_str(keywords) },
            creator: unsafe { opt_str(creator) },
        });
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Append a page with an explicit size (points).
///
/// # Safety
/// `doc` must be a valid handle.
#[no_mangle]
pub unsafe extern "C" fn pdf_document_add_page_sized(
    doc: *mut PdfDocument,
    width: f64,
    height: f64,
) -> PdfStatus {
    with_doc(doc, "pdf_document_add_page_sized", |d| {
        d.add_page_sized(width, height);
        clear_last_error();
        PdfStatus::Ok
    })
}

// ---- fonts -----------------------------------------------------------------

/// Register a font from a file path; writes its id to `out_id`.
///
/// # Safety
/// `doc`, `path` and `out_id` must be valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_document_add_font_file(
    doc: *mut PdfDocument,
    path: *const c_char,
    out_id: *mut c_int,
) -> PdfStatus {
    with_doc(doc, "pdf_document_add_font_file", |d| {
        let path = match unsafe { cstr(path, "pdf_document_add_font_file") } {
            Ok(p) => p,
            Err(s) => return s,
        };
        match d.add_font_file(path) {
            Ok(id) => unsafe { write_id(out_id, id.index()) },
            Err(e) => {
                set_last_error(format!("font load failed for '{path}': {e}"));
                PdfStatus::Font
            }
        }
    })
}

/// Register a font from raw TrueType/OpenType bytes; writes its id to `out_id`.
///
/// # Safety
/// `doc`, `data` and `out_id` must be valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_document_add_font(
    doc: *mut PdfDocument,
    data: *const u8,
    len: usize,
    out_id: *mut c_int,
) -> PdfStatus {
    with_doc(doc, "pdf_document_add_font", |d| {
        match d.add_font(unsafe { bytes(data, len) }.to_vec()) {
            Ok(id) => unsafe { write_id(out_id, id.index()) },
            Err(e) => {
                set_last_error(format!("font load failed: {e}"));
                PdfStatus::Font
            }
        }
    })
}

// ---- text ------------------------------------------------------------------

/// Show a line of text at `(x, y)` (baseline) in `font`/`size` on the current
/// page. `heading_level` 1..=6 tags it as `H1`..`H6` (when the document is
/// tagged); 0 leaves it as a paragraph.
///
/// # Safety
/// `doc` must be valid with at least one page; `text` a valid C string.
#[no_mangle]
pub unsafe extern "C" fn pdf_page_show_text(
    doc: *mut PdfDocument,
    font: c_int,
    size: f64,
    x: f64,
    y: f64,
    text: *const c_char,
    heading_level: c_int,
) -> PdfStatus {
    let s = match unsafe { cstr(text, "pdf_page_show_text") } {
        Ok(s) => s.to_string(),
        Err(st) => return st,
    };
    with_current_page(doc, "pdf_page_show_text", |page| {
        let t = page.text(FontId::from_index(font as usize), size);
        if (1..=6).contains(&heading_level) {
            t.tag(StructTag::heading(heading_level as u8));
        }
        t.at(x, y).show(s);
    })
}

/// Lay out a wrapping paragraph in the box `(x, y, width)` (y = first baseline)
/// with `align` (0=left,1=right,2=center,3=justify) on the current page.
///
/// # Safety
/// `doc` must be valid with at least one page; `text` a valid C string.
#[no_mangle]
pub unsafe extern "C" fn pdf_page_paragraph(
    doc: *mut PdfDocument,
    font: c_int,
    size: f64,
    x: f64,
    y: f64,
    width: f64,
    align: c_int,
    text: *const c_char,
) -> PdfStatus {
    let s = match unsafe { cstr(text, "pdf_page_paragraph") } {
        Ok(s) => s.to_string(),
        Err(st) => return st,
    };
    with_current_page(doc, "pdf_page_paragraph", |page| {
        let p = Paragraph::new(FontId::from_index(font as usize), size)
            .box_at(x, y, width)
            .leading(size * 1.4)
            .align(self::align(align))
            .text(s);
        page.paragraph(p);
    })
}

// ---- images ----------------------------------------------------------------

/// Register an image from a file (JPEG or PNG by signature); id → `out_id`.
///
/// # Safety
/// `doc`, `path`, `out_id` must be valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_document_add_image_file(
    doc: *mut PdfDocument,
    path: *const c_char,
    out_id: *mut c_int,
) -> PdfStatus {
    with_doc(doc, "pdf_document_add_image_file", |d| {
        let path = match unsafe { cstr(path, "pdf_document_add_image_file") } {
            Ok(p) => p,
            Err(s) => return s,
        };
        match d.add_image_file(path) {
            Ok(id) => unsafe { write_id(out_id, id.index()) },
            Err(e) => {
                set_last_error(format!("image load failed: {e}"));
                PdfStatus::Image
            }
        }
    })
}

/// Register a PNG from bytes; id → `out_id`.
///
/// # Safety
/// `doc`, `data`, `out_id` must be valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_document_add_image_png(
    doc: *mut PdfDocument,
    data: *const u8,
    len: usize,
    out_id: *mut c_int,
) -> PdfStatus {
    with_doc(doc, "pdf_document_add_image_png", |d| {
        match d.add_image_png(unsafe { bytes(data, len) }) {
            Ok(id) => unsafe { write_id(out_id, id.index()) },
            Err(e) => {
                set_last_error(format!("png decode failed: {e}"));
                PdfStatus::Image
            }
        }
    })
}

/// Register a JPEG from bytes; id → `out_id`.
///
/// # Safety
/// `doc`, `data`, `out_id` must be valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_document_add_image_jpeg(
    doc: *mut PdfDocument,
    data: *const u8,
    len: usize,
    out_id: *mut c_int,
) -> PdfStatus {
    with_doc(doc, "pdf_document_add_image_jpeg", |d| {
        match d.add_image_jpeg(unsafe { bytes(data, len) }.to_vec()) {
            Ok(id) => unsafe { write_id(out_id, id.index()) },
            Err(e) => {
                set_last_error(format!("jpeg parse failed: {e}"));
                PdfStatus::Image
            }
        }
    })
}

/// Draw a (decorative) image in `(x, y, w, h)` on the current page.
///
/// # Safety
/// `doc` must be valid with at least one page.
#[no_mangle]
pub unsafe extern "C" fn pdf_page_draw_image(
    doc: *mut PdfDocument,
    image: c_int,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> PdfStatus {
    with_current_page(doc, "pdf_page_draw_image", |page| {
        page.draw_image(ImageId::from_index(image as usize), x, y, w, h);
    })
}

/// Draw a meaningful image (tagged `/Figure` with alternate text `alt`).
///
/// # Safety
/// `doc` must be valid with at least one page; `alt` a valid C string.
#[no_mangle]
pub unsafe extern "C" fn pdf_page_figure(
    doc: *mut PdfDocument,
    image: c_int,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    alt: *const c_char,
) -> PdfStatus {
    let alt = match unsafe { cstr(alt, "pdf_page_figure") } {
        Ok(s) => s.to_string(),
        Err(st) => return st,
    };
    with_current_page(doc, "pdf_page_figure", |page| {
        page.figure(ImageId::from_index(image as usize), x, y, w, h, alt);
    })
}

// ---- attachments -----------------------------------------------------------

/// Attach an embedded file (PDF/A-3). `relationship`: 0=Source, 1=Data,
/// 2=Alternative, 3=Supplement, 4=Unspecified.
///
/// # Safety
/// `doc`, `name`, `mime`, `data`, `desc` must be valid.
#[no_mangle]
pub unsafe extern "C" fn pdf_document_attach_file(
    doc: *mut PdfDocument,
    name: *const c_char,
    mime: *const c_char,
    data: *const u8,
    len: usize,
    relationship: c_int,
    desc: *const c_char,
) -> PdfStatus {
    with_doc(doc, "pdf_document_attach_file", |d| {
        let name = match unsafe { cstr(name, "attach_file:name") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let mime = match unsafe { cstr(mime, "attach_file:mime") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let desc = unsafe { opt_str(desc) }.unwrap_or_default();
        d.attach_file(
            name,
            mime,
            unsafe { bytes(data, len) }.to_vec(),
            af_rel(relationship),
            desc,
        );
        clear_last_error();
        PdfStatus::Ok
    })
}

// ---- forms -----------------------------------------------------------------

/// Add a text field (`rect` = x0,y0,x1,y1). `size` 0 = auto.
///
/// # Safety
/// `doc`, `name`, `value` must be valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_document_text_field(
    doc: *mut PdfDocument,
    name: *const c_char,
    page: usize,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    value: *const c_char,
    size: f64,
) -> PdfStatus {
    with_doc(doc, "pdf_document_text_field", |d| {
        let name = match unsafe { cstr(name, "text_field:name") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let value = unsafe { opt_str(value) }.unwrap_or_default();
        d.text_field(name, page, [x0, y0, x1, y1], value, size);
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Add a checkbox (`checked` != 0 = on).
///
/// # Safety
/// `doc`, `name` must be valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_document_checkbox(
    doc: *mut PdfDocument,
    name: *const c_char,
    page: usize,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    checked: c_int,
) -> PdfStatus {
    with_doc(doc, "pdf_document_checkbox", |d| {
        let name = match unsafe { cstr(name, "checkbox:name") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        d.checkbox(name, page, [x0, y0, x1, y1], checked != 0);
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Add a dropdown (choice) field. `options` is a single string with entries
/// separated by `\n`; `selected` is the 0-based index or -1 for none.
///
/// # Safety
/// `doc`, `name`, `options` must be valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_document_dropdown(
    doc: *mut PdfDocument,
    name: *const c_char,
    page: usize,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    options: *const c_char,
    selected: c_int,
    size: f64,
) -> PdfStatus {
    with_doc(doc, "pdf_document_dropdown", |d| {
        let name = match unsafe { cstr(name, "dropdown:name") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        let opts_str = unsafe { opt_str(options) }.unwrap_or_default();
        let opts: Vec<String> = opts_str.split('\n').map(|s| s.to_string()).collect();
        let sel = if selected < 0 {
            None
        } else {
            Some(selected as usize)
        };
        d.dropdown(name, page, [x0, y0, x1, y1], opts, sel, size);
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Add a radio-button group. `rects` holds `count*4` doubles (x0,y0,x1,y1 per
/// button); `exports` holds `count` C strings (the export value of each button);
/// `selected` is the 0-based index or -1.
///
/// # Safety
/// `doc`, `name`, `rects` (count*4 doubles) and `exports` (count strings) valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_document_radio_group(
    doc: *mut PdfDocument,
    name: *const c_char,
    page: usize,
    count: usize,
    rects: *const f64,
    exports: *const *const c_char,
    selected: c_int,
) -> PdfStatus {
    with_doc(doc, "pdf_document_radio_group", |d| {
        let name = match unsafe { cstr(name, "radio_group:name") } {
            Ok(s) => s.to_string(),
            Err(st) => return st,
        };
        if (rects.is_null() || exports.is_null()) && count > 0 {
            set_last_error("radio_group: null buttons");
            return PdfStatus::NullPointer;
        }
        let rects = unsafe { std::slice::from_raw_parts(rects, count * 4) };
        let exports = unsafe { std::slice::from_raw_parts(exports, count) };
        let mut buttons = Vec::with_capacity(count);
        for i in 0..count {
            let r = [
                rects[i * 4],
                rects[i * 4 + 1],
                rects[i * 4 + 2],
                rects[i * 4 + 3],
            ];
            let export = match unsafe { cstr(exports[i], "radio_group:export") } {
                Ok(s) => s.to_string(),
                Err(st) => return st,
            };
            buttons.push((r, export));
        }
        let sel = if selected < 0 {
            None
        } else {
            Some(selected as usize)
        };
        d.radio_group(name, page, buttons, sel);
        clear_last_error();
        PdfStatus::Ok
    })
}

// ---- hyperlinks (Tier 1) ---------------------------------------------------

/// Add a clickable web link over `(x0,y0,x1,y1)` opening `uri` on the current page.
///
/// # Safety
/// `doc` must be valid with at least one page; `uri` a valid C string.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_page_link_uri(
    doc: *mut PdfDocument,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    uri: *const c_char,
) -> PdfStatus {
    let uri = match unsafe { cstr(uri, "pdf_page_link_uri") } {
        Ok(s) => s.to_string(),
        Err(st) => return st,
    };
    with_current_page(doc, "pdf_page_link_uri", |page| {
        page.link_uri([x0, y0, x1, y1], uri);
    })
}

/// Add an internal link over `(x0,y0,x1,y1)` jumping to `target_page` (0-based)
/// on the current page. `has_top` != 0 scrolls so `top` is at the top of view.
///
/// # Safety
/// `doc` must be valid with at least one page.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_page_link_to_page(
    doc: *mut PdfDocument,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    target_page: usize,
    top: f64,
    has_top: c_int,
) -> PdfStatus {
    let top = if has_top != 0 { Some(top) } else { None };
    with_current_page(doc, "pdf_page_link_to_page", |page| {
        page.link_to_page([x0, y0, x1, y1], target_page, top);
    })
}

// ---- bookmarks / outline (Tier 1) ------------------------------------------

/// Add the document outline from a **flat, pre-order** list. Each entry has a
/// `level` (0 = top-level, 1 = child, …), a `title`, a target `page` (0-based),
/// and an optional `top` (used when `has_tops[i]` != 0). The nested tree is
/// rebuilt from the level sequence.
///
/// # Safety
/// All arrays have `count` entries and are readable; `titles[i]` are valid C
/// strings.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn pdf_document_add_bookmarks(
    doc: *mut PdfDocument,
    count: usize,
    levels: *const c_int,
    titles: *const *const c_char,
    pages: *const usize,
    tops: *const f64,
    has_tops: *const c_int,
) -> PdfStatus {
    with_doc(doc, "pdf_document_add_bookmarks", |d| {
        if count > 0 && (levels.is_null() || titles.is_null() || pages.is_null()) {
            set_last_error("add_bookmarks: null array");
            return PdfStatus::NullPointer;
        }
        let levels = unsafe { std::slice::from_raw_parts(levels, count) };
        let titles = unsafe { std::slice::from_raw_parts(titles, count) };
        let pages = unsafe { std::slice::from_raw_parts(pages, count) };
        let mut entries: Vec<(usize, String, usize, Option<f64>)> = Vec::with_capacity(count);
        for i in 0..count {
            let title = match unsafe { cstr(titles[i], "add_bookmarks:title") } {
                Ok(s) => s.to_string(),
                Err(st) => return st,
            };
            let top = if !tops.is_null() && !has_tops.is_null() && unsafe { *has_tops.add(i) } != 0
            {
                Some(unsafe { *tops.add(i) })
            } else {
                None
            };
            entries.push((levels[i].max(0) as usize, title, pages[i], top));
        }
        for bm in build_bookmark_tree(entries) {
            d.add_bookmark(bm);
        }
        clear_last_error();
        PdfStatus::Ok
    })
}

/// Rebuild a nested [`Bookmark`] forest from a flat, pre-order `(level, title,
/// page, top)` list.
fn build_bookmark_tree(entries: Vec<(usize, String, usize, Option<f64>)>) -> Vec<Bookmark> {
    let mut roots: Vec<Bookmark> = Vec::new();
    let mut stack: Vec<Bookmark> = Vec::new();
    let mut levels: Vec<usize> = Vec::new();

    let close_into = |stack: &mut Vec<Bookmark>, roots: &mut Vec<Bookmark>| {
        let child = stack.pop().expect("non-empty");
        if let Some(parent) = stack.last_mut() {
            let p = std::mem::replace(parent, Bookmark::new("", 0));
            *parent = p.child(child);
        } else {
            roots.push(child);
        }
    };

    for (level, title, page, top) in entries {
        let mut bm = Bookmark::new(title, page);
        if let Some(t) = top {
            bm = bm.at_top(t);
        }
        while levels.last().is_some_and(|&l| l >= level) {
            levels.pop();
            close_into(&mut stack, &mut roots);
        }
        stack.push(bm);
        levels.push(level);
    }
    while !stack.is_empty() {
        levels.pop();
        close_into(&mut stack, &mut roots);
    }
    roots
}

// ---- ZUGFeRD / Factur-X (Tier 2) -------------------------------------------

/// Make the document a ZUGFeRD / Factur-X invoice: embed `xml` as `factur-x.xml`,
/// mark it PDF/A-3b, and add the Factur-X XMP at `profile` (0=Minimum, 1=BasicWL,
/// 2=Basic, 3=EN 16931, 4=Extended).
///
/// # Safety
/// `doc` valid; `xml`/`len` readable.
#[no_mangle]
pub unsafe extern "C" fn pdf_document_facturx(
    doc: *mut PdfDocument,
    xml: *const u8,
    len: usize,
    profile: c_int,
) -> PdfStatus {
    with_doc(doc, "pdf_document_facturx", |d| {
        d.facturx(
            unsafe { bytes(xml, len) }.to_vec(),
            facturx_profile(profile),
        );
        clear_last_error();
        PdfStatus::Ok
    })
}

// ---- helpers ---------------------------------------------------------------

/// Write a registration id (usize) into a `*mut c_int` out-parameter.
///
/// # Safety
/// `out` must be a valid writable pointer (or NULL, which is an error).
unsafe fn write_id(out: *mut c_int, id: usize) -> PdfStatus {
    if out.is_null() {
        set_last_error("null out_id");
        return PdfStatus::NullPointer;
    }
    unsafe { *out = id as c_int };
    clear_last_error();
    PdfStatus::Ok
}

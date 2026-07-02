//! Raw C ABI surface: opaque handle types, the typed function-pointer table,
//! and one-time runtime loading of the cdylib.
//!
//! Every signature here mirrors `include/pdf.h` exactly. The whole table is
//! resolved once (lazily) into [`Api`]; callers reach it via [`api`].

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_double, c_int, c_void};
use std::sync::OnceLock;

use crate::error::PdfError;

/// Opaque document handle (`PdfDocument` in C).
#[repr(C)]
pub struct RawDoc {
    _private: [u8; 0],
}

/// Opaque editable-document handle (`PdfEditable` in C).
#[repr(C)]
pub struct RawEditable {
    _private: [u8; 0],
}

/// Mirror of the C ABI `PdfSigningOptions` struct (deferred/external signing).
///
/// Field order and layout are load-bearing — they must match `include/pdf.h`
/// exactly. NULL string fields and a zero `*_len` mean "absent". The last six
/// fields (`visible` … `vis_image_len`) are the visible-signature appearance
/// (issue #41 P1), appended at the end so existing offsets are unchanged.
#[repr(C)]
#[derive(Debug)]
pub(crate) struct PdfSigningOptions {
    pub reason: *const c_char,
    pub location: *const c_char,
    pub name: *const c_char,
    pub pades: c_int,
    pub certification: c_int,
    pub estimated_size: usize,
    pub policy_oid: *const c_char,
    pub policy_hash: *const u8,
    pub policy_hash_len: usize,
    pub policy_hash_alg_oid: *const c_char,
    pub policy_uri: *const c_char,
    // --- visible signature + embedded image (issue #41 P1) ---
    pub visible: c_int,
    pub vis_page: usize,
    pub vis_rect: [c_double; 4],
    pub vis_text: *const c_char,
    pub vis_image: *const u8,
    pub vis_image_len: usize,
}

impl PdfSigningOptions {
    /// An all-absent options struct (every pointer NULL, every count 0). The
    /// all-zero bit pattern is valid for every field of this `#[repr(C)]` type.
    pub(crate) fn empty() -> Self {
        // SAFETY: null pointers, zero ints and zero floats are valid here.
        unsafe { std::mem::zeroed() }
    }
}

/// The `PdfSignHashFn` callback: produce the raw RSA PKCS#1 v1.5 signature over
/// SHA-256 of `data` into `sig_buf` (capacity `sig_cap`), set `*sig_len`, return
/// 0 on success (non-zero = failure).
pub(crate) type PdfSignHashFn =
    unsafe extern "C" fn(*mut c_void, *const u8, usize, *mut u8, usize, *mut usize) -> c_int;

macro_rules! ffi_api {
    ( $( fn $name:ident ( $($arg:ty),* $(,)? ) $( -> $ret:ty )? ; )* ) => {
        /// The resolved function-pointer table plus the kept-alive library.
        pub(crate) struct Api {
            // Keep the library mapped for the whole process lifetime; the fn
            // pointers below point into it. Never dropped (lives in a static).
            _lib: libloading::Library,
            $( pub $name: unsafe extern "C" fn( $($arg),* ) $( -> $ret )?, )*
        }

        impl Api {
            unsafe fn load(
                lib: libloading::Library,
            ) -> std::result::Result<Self, libloading::Error> {
                $(
                    let $name: unsafe extern "C" fn( $($arg),* ) $( -> $ret )? = {
                        let sym: libloading::Symbol<
                            '_,
                            unsafe extern "C" fn( $($arg),* ) $( -> $ret )?,
                        > = lib.get(concat!(stringify!($name), "\0").as_bytes())?;
                        *sym
                    };
                )*
                Ok(Self { _lib: lib, $( $name, )* })
            }
        }
    };
}

ffi_api! {
    // --- core / licensing ---
    fn pdf_version() -> *const c_char;
    fn pdf_last_error_message() -> *const c_char;
    fn pdf_activate_license(*const c_char) -> c_int;
    fn pdf_buffer_free(*mut u8, usize);

    // --- document authoring ---
    fn pdf_document_new() -> *mut RawDoc;
    fn pdf_document_free(*mut RawDoc);
    fn pdf_document_add_page(*mut RawDoc) -> c_int;
    fn pdf_document_add_page_sized(*mut RawDoc, c_double, c_double) -> c_int;
    fn pdf_document_page_count(*const RawDoc) -> c_int;
    fn pdf_page_set_fill_rgb(*mut RawDoc, c_double, c_double, c_double) -> c_int;
    fn pdf_page_set_stroke_rgb(*mut RawDoc, c_double, c_double, c_double) -> c_int;
    fn pdf_page_set_line_width(*mut RawDoc, c_double) -> c_int;
    fn pdf_page_rect(*mut RawDoc, c_double, c_double, c_double, c_double) -> c_int;
    fn pdf_page_fill(*mut RawDoc) -> c_int;
    fn pdf_page_stroke(*mut RawDoc) -> c_int;
    fn pdf_document_save(*mut RawDoc, *const c_char) -> c_int;
    fn pdf_document_write(*mut RawDoc, *mut *mut u8, *mut usize) -> c_int;
    fn pdf_document_pdfa(*mut RawDoc) -> c_int;
    fn pdf_document_pdfa_level(*mut RawDoc, c_int) -> c_int;
    fn pdf_document_tagged(*mut RawDoc) -> c_int;
    fn pdf_document_set_version(*mut RawDoc, c_int) -> c_int;
    fn pdf_document_set_default_size(*mut RawDoc, c_double, c_double) -> c_int;
    fn pdf_document_set_info(
        *mut RawDoc,
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
    ) -> c_int;
    fn pdf_document_add_font_file(*mut RawDoc, *const c_char, *mut c_int) -> c_int;
    fn pdf_document_add_font(*mut RawDoc, *const u8, usize, *mut c_int) -> c_int;
    fn pdf_page_show_text(
        *mut RawDoc,
        c_int,
        c_double,
        c_double,
        c_double,
        *const c_char,
        c_int,
    ) -> c_int;
    fn pdf_page_paragraph(
        *mut RawDoc,
        c_int,
        c_double,
        c_double,
        c_double,
        c_double,
        c_int,
        *const c_char,
    ) -> c_int;
    fn pdf_document_add_image_file(*mut RawDoc, *const c_char, *mut c_int) -> c_int;
    fn pdf_document_add_image_png(*mut RawDoc, *const u8, usize, *mut c_int) -> c_int;
    fn pdf_document_add_image_jpeg(*mut RawDoc, *const u8, usize, *mut c_int) -> c_int;
    fn pdf_page_draw_image(*mut RawDoc, c_int, c_double, c_double, c_double, c_double) -> c_int;
    fn pdf_page_figure(
        *mut RawDoc,
        c_int,
        c_double,
        c_double,
        c_double,
        c_double,
        *const c_char,
    ) -> c_int;
    fn pdf_document_attach_file(
        *mut RawDoc,
        *const c_char,
        *const c_char,
        *const u8,
        usize,
        c_int,
        *const c_char,
    ) -> c_int;
    fn pdf_document_text_field(
        *mut RawDoc,
        *const c_char,
        usize,
        c_double,
        c_double,
        c_double,
        c_double,
        *const c_char,
        c_double,
    ) -> c_int;
    fn pdf_document_checkbox(
        *mut RawDoc,
        *const c_char,
        usize,
        c_double,
        c_double,
        c_double,
        c_double,
        c_int,
    ) -> c_int;
    fn pdf_document_dropdown(
        *mut RawDoc,
        *const c_char,
        usize,
        c_double,
        c_double,
        c_double,
        c_double,
        *const c_char,
        c_int,
        c_double,
    ) -> c_int;
    fn pdf_document_radio_group(
        *mut RawDoc,
        *const c_char,
        usize,
        usize,
        *const c_double,
        *const *const c_char,
        c_int,
    ) -> c_int;

    // --- hyperlinks / bookmarks / Factur-X (Tier 1 / 2) ---
    fn pdf_page_link_uri(
        *mut RawDoc,
        c_double,
        c_double,
        c_double,
        c_double,
        *const c_char,
    ) -> c_int;
    fn pdf_page_link_to_page(
        *mut RawDoc,
        c_double,
        c_double,
        c_double,
        c_double,
        usize,
        c_double,
        c_int,
    ) -> c_int;
    fn pdf_document_add_bookmarks(
        *mut RawDoc,
        usize,
        *const c_int,
        *const *const c_char,
        *const usize,
        *const c_double,
        *const c_int,
    ) -> c_int;
    fn pdf_document_facturx(*mut RawDoc, *const u8, usize, c_int) -> c_int;

    // --- manipulation / extraction ---
    fn pdf_editable_load(*const u8, usize) -> *mut RawEditable;
    fn pdf_editable_load_password(*const u8, usize, *const c_char) -> *mut RawEditable;
    fn pdf_editable_free(*mut RawEditable);
    fn pdf_editable_page_count(*const RawEditable) -> c_int;
    fn pdf_editable_merge(*mut RawEditable, *const RawEditable) -> c_int;
    fn pdf_editable_rotate_page(*mut RawEditable, usize, c_int) -> c_int;
    fn pdf_editable_delete_page(*mut RawEditable, usize) -> c_int;
    fn pdf_editable_reorder_pages(*mut RawEditable, *const usize, usize) -> c_int;
    fn pdf_editable_extract_pages(
        *const RawEditable,
        *const usize,
        usize,
        *mut *mut RawEditable,
    ) -> c_int;
    fn pdf_editable_set_info(*mut RawEditable, *const c_char, *const c_char) -> c_int;
    fn pdf_editable_get_info(
        *const RawEditable,
        *const c_char,
        *mut *mut u8,
        *mut usize,
    ) -> c_int;
    fn pdf_editable_set_xmp(*mut RawEditable, *const u8, usize) -> c_int;
    fn pdf_editable_overlay_page(*mut RawEditable, usize, *const u8, usize) -> c_int;
    fn pdf_editable_fill_text_field(
        *mut RawEditable,
        *const c_char,
        *const c_char,
        *mut c_int,
    ) -> c_int;
    fn pdf_editable_optimize(*mut RawEditable) -> c_int;
    fn pdf_editable_compact(*mut RawEditable, c_int) -> c_int;
    fn pdf_editable_encrypt(
        *mut RawEditable,
        c_int,
        *const c_char,
        *const c_char,
        c_int,
    ) -> c_int;
    fn pdf_editable_to_bytes(*const RawEditable, *mut *mut u8, *mut usize) -> c_int;
    fn pdf_editable_to_bytes_incremental(
        *const RawEditable,
        *const u8,
        usize,
        *mut *mut u8,
        *mut usize,
    ) -> c_int;
    fn pdf_editable_save(*const RawEditable, *const c_char) -> c_int;
    fn pdf_extract_text(*const u8, usize, *mut *mut u8, *mut usize) -> c_int;
    fn pdf_extract_page_text(*const u8, usize, usize, *mut *mut u8, *mut usize) -> c_int;
    fn pdf_find_text_json(
        *const u8,
        usize,
        *const c_char,
        c_int,
        *mut *mut u8,
        *mut usize,
    ) -> c_int;
    fn pdf_extract_images_to_dir(*const u8, usize, *const c_char, *mut usize) -> c_int;
    fn pdf_render_page_to_png(*const u8, usize, usize, f64, *mut *mut u8, *mut usize) -> c_int;
    fn pdf_page_count(*const u8, usize, *mut usize) -> c_int;
    fn pdf_measure_pages_json(*const u8, usize, *mut *mut u8, *mut usize) -> c_int;
    fn pdf_inspect_json(*const u8, usize, *mut *mut u8, *mut usize) -> c_int;
    fn pdf_editable_fill_rect(
        *mut RawEditable,
        c_int,
        c_double,
        c_double,
        c_double,
        c_double,
        c_double,
        c_double,
        c_double,
        c_double,
        *mut c_int,
    ) -> c_int;
    fn pdf_editable_place_text(
        *mut RawEditable,
        c_int,
        c_double,
        c_double,
        *const c_char,
        c_double,
        c_double,
        c_double,
        c_double,
        c_double,
        *mut c_int,
    ) -> c_int;
    fn pdf_editable_draw_image(
        *mut RawEditable,
        c_int,
        *const u8,
        usize,
        c_double,
        c_double,
        c_double,
        c_double,
        c_double,
        *mut c_int,
    ) -> c_int;
    fn pdf_editable_place_text_aligned(
        *mut RawEditable,
        c_int,
        c_double,
        c_double,
        *const c_char,
        c_double,
        c_double,
        c_double,
        c_double,
        c_double,
        c_int,
        *mut c_int,
    ) -> c_int;
    fn pdf_editable_masked_text(
        *mut RawEditable,
        c_int,
        c_double,
        c_double,
        c_double,
        c_double,
        *const c_char,
        c_double,
        c_double,
        c_double,
        c_double,
        c_double,
        c_double,
        c_double,
        c_int,
        *mut c_int,
    ) -> c_int;

    // --- embedded-font stamping + anchored placement (issue #54) ---
    fn pdf_editable_add_font_file(*mut RawEditable, *const c_char, *mut c_int) -> c_int;
    fn pdf_editable_add_font(*mut RawEditable, *const u8, usize, *mut c_int) -> c_int;
    fn pdf_editable_place_text_anchored(
        *mut RawEditable,
        c_int,
        c_double,
        c_double,
        *const c_char,
        c_double,
        c_double,
        c_double,
        c_double,
        c_double,
        c_int,
        c_int,
        c_int,
        *mut c_int,
    ) -> c_int;
    fn pdf_editable_masked_text_pad(
        *mut RawEditable,
        c_int,
        c_double,
        c_double,
        c_double,
        c_double,
        *const c_char,
        c_double,
        c_double,
        c_double,
        c_double,
        c_double,
        c_double,
        c_double,
        c_int,
        c_int,
        c_double,
        c_int,
        *mut c_int,
    ) -> c_int;
    fn pdf_editable_place_paragraph_anchored(
        *mut RawEditable,
        c_int,
        c_double,
        c_double,
        c_double,
        *const c_char,
        c_double,
        c_double,
        c_double,
        c_double,
        c_int,
        c_int,
        c_int,
        c_double,
        c_double,
        c_double,
        *mut c_double,
        *mut c_int,
        *mut c_int,
    ) -> c_int;
    fn pdf_editable_set_stamp_space(*mut RawEditable, c_int) -> c_int;
    fn pdf_editable_draw_image_anchored(
        *mut RawEditable,
        c_int,
        *const u8,
        usize,
        c_double,
        c_double,
        c_double,
        c_double,
        c_double,
        c_int,
        *mut c_int,
    ) -> c_int;

    // --- forms fill / flatten / watermark / redact / PDF-A convert (Tier 1 / 2) ---
    fn pdf_editable_set_checkbox(*mut RawEditable, *const c_char, c_int, *mut c_int) -> c_int;
    fn pdf_editable_set_radio(
        *mut RawEditable,
        *const c_char,
        *const c_char,
        *mut c_int,
    ) -> c_int;
    fn pdf_editable_set_choice(
        *mut RawEditable,
        *const c_char,
        *const c_char,
        *mut c_int,
    ) -> c_int;
    fn pdf_editable_flatten_forms(*mut RawEditable) -> c_int;
    fn pdf_editable_field_names(*const RawEditable, *mut *mut u8, *mut usize) -> c_int;
    fn pdf_editable_watermark_text(
        *mut RawEditable,
        *const c_char,
        c_double,
        c_double,
        c_double,
        c_double,
        c_double,
        c_double,
        c_int,
    ) -> c_int;
    fn pdf_editable_watermark_image_file(
        *mut RawEditable,
        *const c_char,
        c_double,
        c_double,
        c_double,
        c_double,
    ) -> c_int;
    fn pdf_editable_set_version(*mut RawEditable, c_int) -> c_int;
    fn pdf_editable_strip_pdfa(*mut RawEditable) -> c_int;
    fn pdf_editable_normalize(*mut RawEditable, c_int) -> c_int;
    fn pdf_editable_redact(
        *mut RawEditable,
        usize,
        *const c_double,
        usize,
        *mut c_int,
    ) -> c_int;
    fn pdf_editable_convert_to_pdfa(*mut RawEditable, c_int) -> c_int;

    // --- signature verification (Tier 2) ---
    fn pdf_verify_signatures_json(*const u8, usize, *mut *mut u8, *mut usize) -> c_int;

    // --- signatures ---
    fn pdf_sign(
        *const u8,
        usize,
        *const u8,
        usize,
        *const u8,
        usize,
        *const c_char,
        *const c_char,
        *const c_char,
        c_int,
        *mut *mut u8,
        *mut usize,
    ) -> c_int;
    fn pdf_timestamp(
        *const u8,
        usize,
        *const u8,
        usize,
        *const u8,
        usize,
        *const c_char,
        *mut *mut u8,
        *mut usize,
    ) -> c_int;
    fn pdf_add_dss(
        *const u8,
        usize,
        *const *const u8,
        *const usize,
        usize,
        *const *const u8,
        *const usize,
        usize,
        *mut *mut u8,
        *mut usize,
    ) -> c_int;

    // --- deferred / external (HSM) signing (issue #41 P0) ---
    fn pdf_sign_begin(
        *const u8,
        usize,
        *const PdfSigningOptions,
        *mut *mut u8,
        *mut usize,
        *mut *mut u8,
        *mut usize,
    ) -> c_int;
    fn pdf_sign_complete(
        *const u8,
        usize,
        *const u8,
        usize,
        *mut *mut u8,
        *mut usize,
    ) -> c_int;
    fn pdf_sign_with(
        *const u8,
        usize,
        *const u8,
        usize,
        *const *const u8,
        *const usize,
        usize,
        *const PdfSigningOptions,
        PdfSignHashFn,
        *mut c_void,
        *mut *mut u8,
        *mut usize,
    ) -> c_int;
    fn pdf_list_signatures(*const u8, usize, *mut *mut u8, *mut usize) -> c_int;

    // --- network TSA (AD-RT) (issue #41 P1) ---
    fn pdf_timestamp_begin(
        *const u8,
        usize,
        *mut *mut u8,
        *mut usize,
        *mut *mut u8,
        *mut usize,
    ) -> c_int;
    fn pdf_timestamp_request(
        *const u8,
        usize,
        *const u8,
        usize,
        c_int,
        *mut *mut u8,
        *mut usize,
    ) -> c_int;
    fn pdf_timestamp_token_from_response(
        *const u8,
        usize,
        *mut *mut u8,
        *mut usize,
    ) -> c_int;
}

static API: OnceLock<std::result::Result<Api, String>> = OnceLock::new();

/// Resolve, load and bind the cdylib (once), returning the function table.
pub(crate) fn api() -> crate::Result<&'static Api> {
    let slot = API.get_or_init(|| {
        let path = crate::loader::resolve();
        // SAFETY: loading a shared library and resolving C symbols whose
        // signatures match include/pdf.h.
        unsafe {
            match libloading::Library::new(&path) {
                Ok(lib) => Api::load(lib).map_err(|e| {
                    format!("loaded {} but a symbol was missing: {e}", path.display())
                }),
                Err(e) => Err(format!(
                    "could not load {} (set RUSTPDF_LIB to the cdylib path): {e}",
                    path.display()
                )),
            }
        }
    });
    slot.as_ref().map_err(|e| PdfError::loader(e.clone()))
}

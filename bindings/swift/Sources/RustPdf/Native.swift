//
//  Native.swift
//  RustPdf
//
//  Raw FFI surface over the rust-pdf C ABI (`libpdf_ffi`). The C declarations
//  come from the `CRustPdf` clang module (a vendored copy of `include/pdf.h`,
//  with the `PdfStatus` return type mapped to `int` so it bridges to Swift
//  `Int32`); the native library is linked at build time — statically inside the
//  distributed `.xcframework`, or against the dev build tree
//  (`target/{debug,release}`) when building in the repo. Application code uses
//  the idiomatic wrappers (`Document`, `EditableDoc`, `Pdf`), not this type.
//
//  Pointer-sized integers (`uintptr_t`) map to Swift `UInt`; the binding targets
//  64-bit platforms (as do the other language bindings).
//

import CRustPdf

// MARK: - Function-type aliases (one per distinct C signature shape)
//
// Opaque handles → `OpaquePointer?`; status/`int` returns → `Int32`;
// `uintptr_t` → `UInt`; byte buffers → `UInt8` pointers; C strings → `CChar`
// pointers. These are plain Swift function values (the linked C functions),
// not `@convention(c)` callbacks.

typealias CStrFn        = () -> UnsafePointer<CChar>?
typealias NewDocFn      = () -> OpaquePointer?
typealias CStrArgFn     = (UnsafePointer<CChar>?) -> Int32
typealias BufFreeFn     = (UnsafeMutablePointer<UInt8>?, UInt) -> Void
typealias FreeFn        = (OpaquePointer?) -> Void
typealias HFn           = (OpaquePointer?) -> Int32
typealias H3DFn         = (OpaquePointer?, Double, Double, Double) -> Int32
typealias H1DFn         = (OpaquePointer?, Double) -> Int32
typealias H2DFn         = (OpaquePointer?, Double, Double) -> Int32
typealias H4DFn         = (OpaquePointer?, Double, Double, Double, Double) -> Int32
typealias H1IFn         = (OpaquePointer?, Int32) -> Int32
typealias HCStrFn       = (OpaquePointer?, UnsafePointer<CChar>?) -> Int32
typealias HOutFn        = (OpaquePointer?, OutBuf, OutLen) -> Int32
typealias HInfoFn       = (OpaquePointer?, UnsafePointer<CChar>?, UnsafePointer<CChar>?,
                           UnsafePointer<CChar>?, UnsafePointer<CChar>?, UnsafePointer<CChar>?) -> Int32
typealias HCStrOutIdFn  = (OpaquePointer?, UnsafePointer<CChar>?, UnsafeMutablePointer<Int32>?) -> Int32
typealias HBytesOutIdFn = (OpaquePointer?, UnsafePointer<UInt8>?, UInt, UnsafeMutablePointer<Int32>?) -> Int32
typealias HShowTextFn   = (OpaquePointer?, Int32, Double, Double, Double, UnsafePointer<CChar>?, Int32) -> Int32
typealias HParagraphFn  = (OpaquePointer?, Int32, Double, Double, Double, Double, Int32, UnsafePointer<CChar>?) -> Int32
typealias HImgDrawFn    = (OpaquePointer?, Int32, Double, Double, Double, Double) -> Int32
typealias HFigureFn     = (OpaquePointer?, Int32, Double, Double, Double, Double, UnsafePointer<CChar>?) -> Int32
typealias HAttachFn     = (OpaquePointer?, UnsafePointer<CChar>?, UnsafePointer<CChar>?,
                           UnsafePointer<UInt8>?, UInt, Int32, UnsafePointer<CChar>?) -> Int32
typealias HTextFieldFn  = (OpaquePointer?, UnsafePointer<CChar>?, UInt, Double, Double, Double, Double,
                           UnsafePointer<CChar>?, Double) -> Int32
typealias HCheckboxFn   = (OpaquePointer?, UnsafePointer<CChar>?, UInt, Double, Double, Double, Double, Int32) -> Int32
typealias HDropdownFn   = (OpaquePointer?, UnsafePointer<CChar>?, UInt, Double, Double, Double, Double,
                           UnsafePointer<CChar>?, Int32, Double) -> Int32
typealias HRadioFn      = (OpaquePointer?, UnsafePointer<CChar>?, UInt, UInt, UnsafePointer<Double>?,
                           UnsafePointer<UnsafePointer<CChar>?>?, Int32) -> Int32
typealias LoadFn        = (UnsafePointer<UInt8>?, UInt) -> OpaquePointer?
typealias LoadPwFn      = (UnsafePointer<UInt8>?, UInt, UnsafePointer<CChar>?) -> OpaquePointer?
typealias HHFn          = (OpaquePointer?, OpaquePointer?) -> Int32
typealias HUIFn         = (OpaquePointer?, UInt, Int32) -> Int32
typealias HUFn          = (OpaquePointer?, UInt) -> Int32
typealias HArrUFn       = (OpaquePointer?, UnsafePointer<UInt>?, UInt) -> Int32
typealias HExtractFn    = (OpaquePointer?, UnsafePointer<UInt>?, UInt, UnsafeMutablePointer<OpaquePointer?>?) -> Int32
typealias H2CStrFn      = (OpaquePointer?, UnsafePointer<CChar>?, UnsafePointer<CChar>?) -> Int32
typealias HCStrOutFn    = (OpaquePointer?, UnsafePointer<CChar>?, OutBuf, OutLen) -> Int32
typealias HBytesFn      = (OpaquePointer?, UnsafePointer<UInt8>?, UInt) -> Int32
typealias HUBytesFn     = (OpaquePointer?, UInt, UnsafePointer<UInt8>?, UInt) -> Int32
typealias HFillFieldFn  = (OpaquePointer?, UnsafePointer<CChar>?, UnsafePointer<CChar>?, UnsafeMutablePointer<Int32>?) -> Int32
typealias HEncryptFn    = (OpaquePointer?, Int32, UnsafePointer<CChar>?, UnsafePointer<CChar>?, Int32) -> Int32
typealias HIncrFn       = (OpaquePointer?, UnsafePointer<UInt8>?, UInt, OutBuf, OutLen) -> Int32
typealias ExtractTextFn = (UnsafePointer<UInt8>?, UInt, OutBuf, OutLen) -> Int32
typealias SignFn        = (UnsafePointer<UInt8>?, UInt, UnsafePointer<UInt8>?, UInt,
                           UnsafePointer<UInt8>?, UInt, UnsafePointer<CChar>?, UnsafePointer<CChar>?,
                           UnsafePointer<CChar>?, Int32, OutBuf, OutLen) -> Int32
typealias TimestampFn   = (UnsafePointer<UInt8>?, UInt, UnsafePointer<UInt8>?, UInt,
                           UnsafePointer<UInt8>?, UInt, UnsafePointer<CChar>?, OutBuf, OutLen) -> Int32
typealias AddDssFn      = (UnsafePointer<UInt8>?, UInt,
                           UnsafePointer<UnsafePointer<UInt8>?>?, UnsafePointer<UInt>?, UInt,
                           UnsafePointer<UnsafePointer<UInt8>?>?, UnsafePointer<UInt>?, UInt,
                           OutBuf, OutLen) -> Int32

/// `out_ptr` / `out_len` out-parameters shared by the buffer-producing exports.
typealias OutBuf = UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>?
typealias OutLen = UnsafeMutablePointer<UInt>?

/// A typed handle to each linked C export. `Native.shared` holds one reference
/// per function; the idiomatic wrappers call through it so their call sites stay
/// identical to the other language bindings. The functions are linked, not
/// looked up at run time.
final class Native {
    static let shared = Native()

    // ---- core ---------------------------------------------------------------
    let pdf_version: CStrFn = CRustPdf.pdf_version
    let pdf_last_error_message: CStrFn = CRustPdf.pdf_last_error_message
    let pdf_activate_license: CStrArgFn = CRustPdf.pdf_activate_license
    let pdf_buffer_free: BufFreeFn = CRustPdf.pdf_buffer_free

    // ---- document lifecycle + graphics --------------------------------------
    let pdf_document_new: NewDocFn = CRustPdf.pdf_document_new
    let pdf_document_free: FreeFn = CRustPdf.pdf_document_free
    let pdf_document_add_page: HFn = CRustPdf.pdf_document_add_page
    let pdf_document_add_page_sized: H2DFn = CRustPdf.pdf_document_add_page_sized
    let pdf_document_page_count: HFn = CRustPdf.pdf_document_page_count
    let pdf_page_set_fill_rgb: H3DFn = CRustPdf.pdf_page_set_fill_rgb
    let pdf_page_set_stroke_rgb: H3DFn = CRustPdf.pdf_page_set_stroke_rgb
    let pdf_page_set_line_width: H1DFn = CRustPdf.pdf_page_set_line_width
    let pdf_page_rect: H4DFn = CRustPdf.pdf_page_rect
    let pdf_page_fill: HFn = CRustPdf.pdf_page_fill
    let pdf_page_stroke: HFn = CRustPdf.pdf_page_stroke
    let pdf_document_save: HCStrFn = CRustPdf.pdf_document_save
    let pdf_document_write: HOutFn = CRustPdf.pdf_document_write

    // ---- configuration ------------------------------------------------------
    let pdf_document_pdfa: HFn = CRustPdf.pdf_document_pdfa
    let pdf_document_pdfa_level: H1IFn = CRustPdf.pdf_document_pdfa_level
    let pdf_document_tagged: HFn = CRustPdf.pdf_document_tagged
    let pdf_document_set_version: H1IFn = CRustPdf.pdf_document_set_version
    let pdf_document_set_default_size: H2DFn = CRustPdf.pdf_document_set_default_size
    let pdf_document_set_info: HInfoFn = CRustPdf.pdf_document_set_info

    // ---- fonts + text -------------------------------------------------------
    let pdf_document_add_font_file: HCStrOutIdFn = CRustPdf.pdf_document_add_font_file
    let pdf_document_add_font: HBytesOutIdFn = CRustPdf.pdf_document_add_font
    let pdf_page_show_text: HShowTextFn = CRustPdf.pdf_page_show_text
    let pdf_page_paragraph: HParagraphFn = CRustPdf.pdf_page_paragraph

    // ---- images -------------------------------------------------------------
    let pdf_document_add_image_file: HCStrOutIdFn = CRustPdf.pdf_document_add_image_file
    let pdf_document_add_image_png: HBytesOutIdFn = CRustPdf.pdf_document_add_image_png
    let pdf_document_add_image_jpeg: HBytesOutIdFn = CRustPdf.pdf_document_add_image_jpeg
    let pdf_page_draw_image: HImgDrawFn = CRustPdf.pdf_page_draw_image
    let pdf_page_figure: HFigureFn = CRustPdf.pdf_page_figure

    // ---- attachments + forms ------------------------------------------------
    let pdf_document_attach_file: HAttachFn = CRustPdf.pdf_document_attach_file
    let pdf_document_text_field: HTextFieldFn = CRustPdf.pdf_document_text_field
    let pdf_document_checkbox: HCheckboxFn = CRustPdf.pdf_document_checkbox
    let pdf_document_dropdown: HDropdownFn = CRustPdf.pdf_document_dropdown
    let pdf_document_radio_group: HRadioFn = CRustPdf.pdf_document_radio_group

    // ---- editable -----------------------------------------------------------
    let pdf_editable_load: LoadFn = CRustPdf.pdf_editable_load
    let pdf_editable_load_password: LoadPwFn = CRustPdf.pdf_editable_load_password
    let pdf_editable_free: FreeFn = CRustPdf.pdf_editable_free
    let pdf_editable_page_count: HFn = CRustPdf.pdf_editable_page_count
    let pdf_editable_merge: HHFn = CRustPdf.pdf_editable_merge
    let pdf_editable_rotate_page: HUIFn = CRustPdf.pdf_editable_rotate_page
    let pdf_editable_delete_page: HUFn = CRustPdf.pdf_editable_delete_page
    let pdf_editable_reorder_pages: HArrUFn = CRustPdf.pdf_editable_reorder_pages
    let pdf_editable_extract_pages: HExtractFn = CRustPdf.pdf_editable_extract_pages
    let pdf_editable_set_info: H2CStrFn = CRustPdf.pdf_editable_set_info
    let pdf_editable_get_info: HCStrOutFn = CRustPdf.pdf_editable_get_info
    let pdf_editable_set_xmp: HBytesFn = CRustPdf.pdf_editable_set_xmp
    let pdf_editable_overlay_page: HUBytesFn = CRustPdf.pdf_editable_overlay_page
    let pdf_editable_fill_text_field: HFillFieldFn = CRustPdf.pdf_editable_fill_text_field
    let pdf_editable_optimize: HFn = CRustPdf.pdf_editable_optimize
    let pdf_editable_compact: H1IFn = CRustPdf.pdf_editable_compact
    let pdf_editable_encrypt: HEncryptFn = CRustPdf.pdf_editable_encrypt
    let pdf_editable_to_bytes: HOutFn = CRustPdf.pdf_editable_to_bytes
    let pdf_editable_to_bytes_incremental: HIncrFn = CRustPdf.pdf_editable_to_bytes_incremental
    let pdf_editable_save: HCStrFn = CRustPdf.pdf_editable_save

    // ---- text extraction + signing ------------------------------------------
    let pdf_extract_text: ExtractTextFn = CRustPdf.pdf_extract_text
    let pdf_sign: SignFn = CRustPdf.pdf_sign
    let pdf_timestamp: TimestampFn = CRustPdf.pdf_timestamp
    let pdf_add_dss: AddDssFn = CRustPdf.pdf_add_dss

    private init() {}
}

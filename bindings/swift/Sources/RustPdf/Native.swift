//
//  Native.swift
//  RustPdf
//
//  Raw FFI surface over the rust-pdf C ABI (`libpdf_ffi`). Mirrors
//  `include/pdf.h` one-to-one. The cdylib is located and bound at run time with
//  `dlopen`/`dlsym` — there is no static link name, so the same package works
//  from the repo checkout and from an installed library path. Application code
//  uses the idiomatic wrappers (`Document`, `EditableDoc`, `Pdf`), not this
//  type directly.
//
//  Pointer-sized integers (`uintptr_t`) map to Swift `UInt`; the binding targets
//  64-bit platforms (as do the other language bindings).
//

#if canImport(Darwin)
import Darwin
#elseif canImport(Glibc)
import Glibc
#endif
import Foundation

/// Thrown when the native library cannot be located or a required export is
/// missing from it.
public struct RustPdfLoadError: Error, CustomStringConvertible {
    public let description: String
    init(_ message: String) { self.description = "rustpdf: \(message)" }
}

// MARK: - C function-pointer signatures (one alias per distinct shape)

// Opaque handles are `OpaquePointer?`; `PdfStatus` and `int` returns are `Int32`;
// `uintptr_t` is `UInt`; byte buffers are `UInt8` pointers; C strings are
// `CChar` pointers.
typealias CStrFn        = @convention(c) () -> UnsafePointer<CChar>?
typealias NewDocFn      = @convention(c) () -> OpaquePointer?
typealias CStrArgFn     = @convention(c) (UnsafePointer<CChar>?) -> Int32
typealias BufFreeFn     = @convention(c) (UnsafeMutablePointer<UInt8>?, UInt) -> Void
typealias FreeFn        = @convention(c) (OpaquePointer?) -> Void
typealias HFn           = @convention(c) (OpaquePointer?) -> Int32
typealias H3DFn         = @convention(c) (OpaquePointer?, Double, Double, Double) -> Int32
typealias H1DFn         = @convention(c) (OpaquePointer?, Double) -> Int32
typealias H2DFn         = @convention(c) (OpaquePointer?, Double, Double) -> Int32
typealias H4DFn         = @convention(c) (OpaquePointer?, Double, Double, Double, Double) -> Int32
typealias H1IFn         = @convention(c) (OpaquePointer?, Int32) -> Int32
typealias HCStrFn       = @convention(c) (OpaquePointer?, UnsafePointer<CChar>?) -> Int32
typealias HOutFn        = @convention(c) (OpaquePointer?,
                                          UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>?,
                                          UnsafeMutablePointer<UInt>?) -> Int32
typealias HInfoFn       = @convention(c) (OpaquePointer?, UnsafePointer<CChar>?,
                                          UnsafePointer<CChar>?, UnsafePointer<CChar>?,
                                          UnsafePointer<CChar>?, UnsafePointer<CChar>?) -> Int32
typealias HCStrOutIdFn  = @convention(c) (OpaquePointer?, UnsafePointer<CChar>?,
                                          UnsafeMutablePointer<Int32>?) -> Int32
typealias HBytesOutIdFn = @convention(c) (OpaquePointer?, UnsafePointer<UInt8>?, UInt,
                                          UnsafeMutablePointer<Int32>?) -> Int32
typealias HShowTextFn   = @convention(c) (OpaquePointer?, Int32, Double, Double, Double,
                                          UnsafePointer<CChar>?, Int32) -> Int32
typealias HParagraphFn  = @convention(c) (OpaquePointer?, Int32, Double, Double, Double,
                                          Double, Int32, UnsafePointer<CChar>?) -> Int32
typealias HImgDrawFn    = @convention(c) (OpaquePointer?, Int32, Double, Double, Double, Double) -> Int32
typealias HFigureFn     = @convention(c) (OpaquePointer?, Int32, Double, Double, Double, Double,
                                          UnsafePointer<CChar>?) -> Int32
typealias HAttachFn     = @convention(c) (OpaquePointer?, UnsafePointer<CChar>?, UnsafePointer<CChar>?,
                                          UnsafePointer<UInt8>?, UInt, Int32, UnsafePointer<CChar>?) -> Int32
typealias HTextFieldFn  = @convention(c) (OpaquePointer?, UnsafePointer<CChar>?, UInt,
                                          Double, Double, Double, Double,
                                          UnsafePointer<CChar>?, Double) -> Int32
typealias HCheckboxFn   = @convention(c) (OpaquePointer?, UnsafePointer<CChar>?, UInt,
                                          Double, Double, Double, Double, Int32) -> Int32
typealias HDropdownFn   = @convention(c) (OpaquePointer?, UnsafePointer<CChar>?, UInt,
                                          Double, Double, Double, Double,
                                          UnsafePointer<CChar>?, Int32, Double) -> Int32
typealias HRadioFn      = @convention(c) (OpaquePointer?, UnsafePointer<CChar>?, UInt, UInt,
                                          UnsafePointer<Double>?,
                                          UnsafePointer<UnsafePointer<CChar>?>?, Int32) -> Int32
typealias LoadFn        = @convention(c) (UnsafePointer<UInt8>?, UInt) -> OpaquePointer?
typealias LoadPwFn      = @convention(c) (UnsafePointer<UInt8>?, UInt, UnsafePointer<CChar>?) -> OpaquePointer?
typealias HHFn          = @convention(c) (OpaquePointer?, OpaquePointer?) -> Int32
typealias HUIFn         = @convention(c) (OpaquePointer?, UInt, Int32) -> Int32
typealias HUFn          = @convention(c) (OpaquePointer?, UInt) -> Int32
typealias HArrUFn       = @convention(c) (OpaquePointer?, UnsafePointer<UInt>?, UInt) -> Int32
typealias HExtractFn    = @convention(c) (OpaquePointer?, UnsafePointer<UInt>?, UInt,
                                          UnsafeMutablePointer<OpaquePointer?>?) -> Int32
typealias H2CStrFn      = @convention(c) (OpaquePointer?, UnsafePointer<CChar>?, UnsafePointer<CChar>?) -> Int32
typealias HCStrOutFn    = @convention(c) (OpaquePointer?, UnsafePointer<CChar>?,
                                          UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>?,
                                          UnsafeMutablePointer<UInt>?) -> Int32
typealias HBytesFn      = @convention(c) (OpaquePointer?, UnsafePointer<UInt8>?, UInt) -> Int32
typealias HUBytesFn     = @convention(c) (OpaquePointer?, UInt, UnsafePointer<UInt8>?, UInt) -> Int32
typealias HFillFieldFn  = @convention(c) (OpaquePointer?, UnsafePointer<CChar>?, UnsafePointer<CChar>?,
                                          UnsafeMutablePointer<Int32>?) -> Int32
typealias HEncryptFn    = @convention(c) (OpaquePointer?, Int32, UnsafePointer<CChar>?,
                                          UnsafePointer<CChar>?, Int32) -> Int32
typealias HIncrFn       = @convention(c) (OpaquePointer?, UnsafePointer<UInt8>?, UInt,
                                          UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>?,
                                          UnsafeMutablePointer<UInt>?) -> Int32
typealias ExtractTextFn = @convention(c) (UnsafePointer<UInt8>?, UInt,
                                          UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>?,
                                          UnsafeMutablePointer<UInt>?) -> Int32
typealias SignFn        = @convention(c) (UnsafePointer<UInt8>?, UInt,
                                          UnsafePointer<UInt8>?, UInt,
                                          UnsafePointer<UInt8>?, UInt,
                                          UnsafePointer<CChar>?, UnsafePointer<CChar>?,
                                          UnsafePointer<CChar>?, Int32,
                                          UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>?,
                                          UnsafeMutablePointer<UInt>?) -> Int32
typealias TimestampFn   = @convention(c) (UnsafePointer<UInt8>?, UInt,
                                          UnsafePointer<UInt8>?, UInt,
                                          UnsafePointer<UInt8>?, UInt,
                                          UnsafePointer<CChar>?,
                                          UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>?,
                                          UnsafeMutablePointer<UInt>?) -> Int32
typealias AddDssFn      = @convention(c) (UnsafePointer<UInt8>?, UInt,
                                          UnsafePointer<UnsafePointer<UInt8>?>?,
                                          UnsafePointer<UInt>?, UInt,
                                          UnsafePointer<UnsafePointer<UInt8>?>?,
                                          UnsafePointer<UInt>?, UInt,
                                          UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>?,
                                          UnsafeMutablePointer<UInt>?) -> Int32

/// The loaded native library plus a typed binding for every C export.
///
/// `Native.shared` lazily `dlopen`s the cdylib on first use (thread-safe via
/// Swift's `static let` once semantics) and binds each symbol with `dlsym`.
final class Native {
    static let shared = Native()

    private let handle: UnsafeMutableRawPointer

    // ---- core ---------------------------------------------------------------
    let pdf_version: CStrFn
    let pdf_last_error_message: CStrFn
    let pdf_activate_license: CStrArgFn
    let pdf_buffer_free: BufFreeFn

    // ---- document lifecycle + graphics --------------------------------------
    let pdf_document_new: NewDocFn
    let pdf_document_free: FreeFn
    let pdf_document_add_page: HFn
    let pdf_document_add_page_sized: H2DFn
    let pdf_document_page_count: HFn
    let pdf_page_set_fill_rgb: H3DFn
    let pdf_page_set_stroke_rgb: H3DFn
    let pdf_page_set_line_width: H1DFn
    let pdf_page_rect: H4DFn
    let pdf_page_fill: HFn
    let pdf_page_stroke: HFn
    let pdf_document_save: HCStrFn
    let pdf_document_write: HOutFn

    // ---- configuration ------------------------------------------------------
    let pdf_document_pdfa: HFn
    let pdf_document_pdfa_level: H1IFn
    let pdf_document_tagged: HFn
    let pdf_document_set_version: H1IFn
    let pdf_document_set_default_size: H2DFn
    let pdf_document_set_info: HInfoFn

    // ---- fonts + text -------------------------------------------------------
    let pdf_document_add_font_file: HCStrOutIdFn
    let pdf_document_add_font: HBytesOutIdFn
    let pdf_page_show_text: HShowTextFn
    let pdf_page_paragraph: HParagraphFn

    // ---- images -------------------------------------------------------------
    let pdf_document_add_image_file: HCStrOutIdFn
    let pdf_document_add_image_png: HBytesOutIdFn
    let pdf_document_add_image_jpeg: HBytesOutIdFn
    let pdf_page_draw_image: HImgDrawFn
    let pdf_page_figure: HFigureFn

    // ---- attachments + forms ------------------------------------------------
    let pdf_document_attach_file: HAttachFn
    let pdf_document_text_field: HTextFieldFn
    let pdf_document_checkbox: HCheckboxFn
    let pdf_document_dropdown: HDropdownFn
    let pdf_document_radio_group: HRadioFn

    // ---- editable -----------------------------------------------------------
    let pdf_editable_load: LoadFn
    let pdf_editable_load_password: LoadPwFn
    let pdf_editable_free: FreeFn
    let pdf_editable_page_count: HFn
    let pdf_editable_merge: HHFn
    let pdf_editable_rotate_page: HUIFn
    let pdf_editable_delete_page: HUFn
    let pdf_editable_reorder_pages: HArrUFn
    let pdf_editable_extract_pages: HExtractFn
    let pdf_editable_set_info: H2CStrFn
    let pdf_editable_get_info: HCStrOutFn
    let pdf_editable_set_xmp: HBytesFn
    let pdf_editable_overlay_page: HUBytesFn
    let pdf_editable_fill_text_field: HFillFieldFn
    let pdf_editable_optimize: HFn
    let pdf_editable_compact: H1IFn
    let pdf_editable_encrypt: HEncryptFn
    let pdf_editable_to_bytes: HOutFn
    let pdf_editable_to_bytes_incremental: HIncrFn
    let pdf_editable_save: HCStrFn

    // ---- text extraction + signing ------------------------------------------
    let pdf_extract_text: ExtractTextFn
    let pdf_sign: SignFn
    let pdf_timestamp: TimestampFn
    let pdf_add_dss: AddDssFn

    private init() {
        do {
            handle = try Native.openLibrary()
        } catch {
            // A missing native library is unrecoverable: the whole binding is
            // unusable. Surface a clear message rather than crashing on the
            // first symbol use.
            fatalError("\(error)")
        }

        let h = handle
        func sym<T>(_ name: String, _ type: T.Type) -> T {
            guard let p = dlsym(h, name) else {
                fatalError("rustpdf: native library is missing export '\(name)' — "
                    + "rebuild it with `cargo build -p pdf-ffi`")
            }
            return unsafeBitCast(p, to: T.self)
        }

        pdf_version            = sym("pdf_version", CStrFn.self)
        pdf_last_error_message = sym("pdf_last_error_message", CStrFn.self)
        pdf_activate_license   = sym("pdf_activate_license", CStrArgFn.self)
        pdf_buffer_free        = sym("pdf_buffer_free", BufFreeFn.self)

        pdf_document_new          = sym("pdf_document_new", NewDocFn.self)
        pdf_document_free         = sym("pdf_document_free", FreeFn.self)
        pdf_document_add_page     = sym("pdf_document_add_page", HFn.self)
        pdf_document_add_page_sized = sym("pdf_document_add_page_sized", H2DFn.self)
        pdf_document_page_count   = sym("pdf_document_page_count", HFn.self)
        pdf_page_set_fill_rgb     = sym("pdf_page_set_fill_rgb", H3DFn.self)
        pdf_page_set_stroke_rgb   = sym("pdf_page_set_stroke_rgb", H3DFn.self)
        pdf_page_set_line_width   = sym("pdf_page_set_line_width", H1DFn.self)
        pdf_page_rect             = sym("pdf_page_rect", H4DFn.self)
        pdf_page_fill             = sym("pdf_page_fill", HFn.self)
        pdf_page_stroke           = sym("pdf_page_stroke", HFn.self)
        pdf_document_save         = sym("pdf_document_save", HCStrFn.self)
        pdf_document_write        = sym("pdf_document_write", HOutFn.self)

        pdf_document_pdfa             = sym("pdf_document_pdfa", HFn.self)
        pdf_document_pdfa_level       = sym("pdf_document_pdfa_level", H1IFn.self)
        pdf_document_tagged           = sym("pdf_document_tagged", HFn.self)
        pdf_document_set_version      = sym("pdf_document_set_version", H1IFn.self)
        pdf_document_set_default_size = sym("pdf_document_set_default_size", H2DFn.self)
        pdf_document_set_info         = sym("pdf_document_set_info", HInfoFn.self)

        pdf_document_add_font_file = sym("pdf_document_add_font_file", HCStrOutIdFn.self)
        pdf_document_add_font      = sym("pdf_document_add_font", HBytesOutIdFn.self)
        pdf_page_show_text         = sym("pdf_page_show_text", HShowTextFn.self)
        pdf_page_paragraph         = sym("pdf_page_paragraph", HParagraphFn.self)

        pdf_document_add_image_file = sym("pdf_document_add_image_file", HCStrOutIdFn.self)
        pdf_document_add_image_png  = sym("pdf_document_add_image_png", HBytesOutIdFn.self)
        pdf_document_add_image_jpeg = sym("pdf_document_add_image_jpeg", HBytesOutIdFn.self)
        pdf_page_draw_image         = sym("pdf_page_draw_image", HImgDrawFn.self)
        pdf_page_figure             = sym("pdf_page_figure", HFigureFn.self)

        pdf_document_attach_file = sym("pdf_document_attach_file", HAttachFn.self)
        pdf_document_text_field  = sym("pdf_document_text_field", HTextFieldFn.self)
        pdf_document_checkbox    = sym("pdf_document_checkbox", HCheckboxFn.self)
        pdf_document_dropdown    = sym("pdf_document_dropdown", HDropdownFn.self)
        pdf_document_radio_group = sym("pdf_document_radio_group", HRadioFn.self)

        pdf_editable_load             = sym("pdf_editable_load", LoadFn.self)
        pdf_editable_load_password    = sym("pdf_editable_load_password", LoadPwFn.self)
        pdf_editable_free             = sym("pdf_editable_free", FreeFn.self)
        pdf_editable_page_count       = sym("pdf_editable_page_count", HFn.self)
        pdf_editable_merge            = sym("pdf_editable_merge", HHFn.self)
        pdf_editable_rotate_page      = sym("pdf_editable_rotate_page", HUIFn.self)
        pdf_editable_delete_page      = sym("pdf_editable_delete_page", HUFn.self)
        pdf_editable_reorder_pages    = sym("pdf_editable_reorder_pages", HArrUFn.self)
        pdf_editable_extract_pages    = sym("pdf_editable_extract_pages", HExtractFn.self)
        pdf_editable_set_info         = sym("pdf_editable_set_info", H2CStrFn.self)
        pdf_editable_get_info         = sym("pdf_editable_get_info", HCStrOutFn.self)
        pdf_editable_set_xmp          = sym("pdf_editable_set_xmp", HBytesFn.self)
        pdf_editable_overlay_page     = sym("pdf_editable_overlay_page", HUBytesFn.self)
        pdf_editable_fill_text_field  = sym("pdf_editable_fill_text_field", HFillFieldFn.self)
        pdf_editable_optimize         = sym("pdf_editable_optimize", HFn.self)
        pdf_editable_compact          = sym("pdf_editable_compact", H1IFn.self)
        pdf_editable_encrypt          = sym("pdf_editable_encrypt", HEncryptFn.self)
        pdf_editable_to_bytes         = sym("pdf_editable_to_bytes", HOutFn.self)
        pdf_editable_to_bytes_incremental = sym("pdf_editable_to_bytes_incremental", HIncrFn.self)
        pdf_editable_save             = sym("pdf_editable_save", HCStrFn.self)

        pdf_extract_text = sym("pdf_extract_text", ExtractTextFn.self)
        pdf_sign         = sym("pdf_sign", SignFn.self)
        pdf_timestamp    = sym("pdf_timestamp", TimestampFn.self)
        pdf_add_dss      = sym("pdf_add_dss", AddDssFn.self)
    }

    // MARK: - Library resolution

    #if os(Windows)
    private static let libFile = "pdf_ffi.dll"
    #elseif canImport(Darwin)
    private static let libFile = "libpdf_ffi.dylib"
    #else
    private static let libFile = "libpdf_ffi.so"
    #endif

    /// Locate and `dlopen` the cdylib. Search order mirrors the C#/Java/Delphi
    /// bindings:
    ///   1. `$RUSTPDF_LIB` (an explicit file path);
    ///   2. the library next to the running executable, or in the current dir
    ///      (the normal deployment layout — ship the lib beside your app);
    ///   3. `target/{debug,release}/<libfile>` walking up from the executable
    ///      dir and the current dir (the dev tree);
    ///   4. the bare platform name, letting the OS loader resolve it
    ///      (install-name / `DYLD_*` / `LD_LIBRARY_PATH`).
    private static func openLibrary() throws -> UnsafeMutableRawPointer {
        var candidates: [String] = []

        if let explicit = ProcessInfo.processInfo.environment["RUSTPDF_LIB"], !explicit.isEmpty {
            candidates.append(explicit)
        }

        let fm = FileManager.default
        var roots: [String] = [fm.currentDirectoryPath]
        if let exe = Bundle.main.executablePath {
            roots.append((exe as NSString).deletingLastPathComponent)
        }

        for root in roots {
            candidates.append((root as NSString).appendingPathComponent(libFile))
            // Walk up to a Cargo workspace root and probe target/{debug,release}.
            var dir = root
            for _ in 0..<12 {
                for profile in ["debug", "release"] {
                    candidates.append(
                        (dir as NSString)
                            .appendingPathComponent("target/\(profile)/\(libFile)"))
                }
                let parent = (dir as NSString).deletingLastPathComponent
                if parent == dir || parent.isEmpty { break }
                dir = parent
            }
        }

        candidates.append(libFile) // bare name → OS loader

        var tried: [String] = []
        for path in candidates {
            // For real paths, only dlopen ones that exist (so a clean dlerror
            // bubbles up); the bare name is always attempted.
            if path != libFile && !fm.fileExists(atPath: path) { continue }
            if let h = dlopen(path, RTLD_NOW | RTLD_GLOBAL) {
                return h
            }
            tried.append(path)
        }

        let detail = tried.isEmpty ? "" : " (tried: \(tried.joined(separator: ", ")))"
        throw RustPdfLoadError(
            "could not load \(libFile)\(detail). Build it with "
            + "`cargo build -p pdf-ffi`, or set RUSTPDF_LIB to its path.")
    }
}

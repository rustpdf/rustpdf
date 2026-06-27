//
//  EditableDoc.swift
//  RustPdf
//
//  Idiomatic wrapper for editing an existing PDF: merge / split / reorder /
//  rotate / delete pages, info + XMP metadata, overlays, AcroForm fills,
//  optimization, encryption, and incremental updates.
//

#if canImport(Darwin)
import Darwin
#elseif canImport(Glibc)
import Glibc
#endif
import Foundation

/// An existing PDF loaded for manipulation. Native memory is released by
/// `deinit`.
public final class EditableDoc {
    private var handle: OpaquePointer?

    private init(handle: OpaquePointer) {
        self.handle = handle
    }

    /// Parse an existing PDF from bytes.
    public init(loading data: [UInt8]) throws {
        let h: OpaquePointer? = withBytes(data) { ptr, len in
            Native.shared.pdf_editable_load(ptr, len)
        }
        guard let h else { throw PdfError(status: .parse, message: lastErrorMessage()) }
        handle = h
    }

    /// Parse an encrypted PDF using `password`.
    public init(loading data: [UInt8], password: String) throws {
        let h: OpaquePointer? = withBytes(data) { ptr, len in
            password.withCString { Native.shared.pdf_editable_load_password(ptr, len, $0) }
        }
        guard let h else { throw PdfError(status: .parse, message: lastErrorMessage()) }
        handle = h
    }

    deinit {
        if let h = handle { Native.shared.pdf_editable_free(h) }
    }

    /// The number of pages.
    public var pageCount: Int { Int(Native.shared.pdf_editable_page_count(handle)) }

    /// Append all pages of `other`.
    @discardableResult
    public func merge(_ other: EditableDoc) throws -> EditableDoc {
        try check(Native.shared.pdf_editable_merge(handle, other.handle)); return self
    }

    /// Rotate page `index` by `degrees` (a multiple of 90).
    @discardableResult
    public func rotatePage(_ index: Int, degrees: Int32) throws -> EditableDoc {
        try check(Native.shared.pdf_editable_rotate_page(handle, UInt(index), degrees)); return self
    }

    /// Delete page `index`.
    @discardableResult
    public func deletePage(_ index: Int) throws -> EditableDoc {
        try check(Native.shared.pdf_editable_delete_page(handle, UInt(index))); return self
    }

    /// Reorder pages to the 0-based `order`.
    @discardableResult
    public func reorderPages(_ order: [Int]) throws -> EditableDoc {
        let arr = order.map { UInt($0) }
        try arr.withUnsafeBufferPointer {
            try check(Native.shared.pdf_editable_reorder_pages(handle, $0.baseAddress, UInt(arr.count)))
        }
        return self
    }

    /// Extract the given page `indices` into a new document.
    public func extractPages(_ indices: [Int]) throws -> EditableDoc {
        let arr = indices.map { UInt($0) }
        var out: OpaquePointer?
        try arr.withUnsafeBufferPointer {
            try check(Native.shared.pdf_editable_extract_pages(
                handle, $0.baseAddress, UInt(arr.count), &out))
        }
        guard let out else { throw PdfError(status: .invalidArgument, message: lastErrorMessage()) }
        return EditableDoc(handle: out)
    }

    /// Set an `/Info` entry (`key` → `value`).
    @discardableResult
    public func setInfo(key: String, value: String) throws -> EditableDoc {
        try key.withCString { k in
            try value.withCString { v in
                try check(Native.shared.pdf_editable_set_info(handle, k, v))
            }
        }
        return self
    }

    /// Read an `/Info` entry; returns "" if absent.
    public func getInfo(key: String) throws -> String {
        let bytes = try key.withCString { k in
            try takeBytes { out, len in Native.shared.pdf_editable_get_info(handle, k, out, len) }
        }
        return String(decoding: bytes, as: UTF8.self)
    }

    /// Replace the XMP `/Metadata` stream.
    @discardableResult
    public func setXMP(_ xml: [UInt8]) throws -> EditableDoc {
        try withBytes(xml) { ptr, len in
            try check(Native.shared.pdf_editable_set_xmp(handle, ptr, len))
        }
        return self
    }

    /// Overlay raw content-stream bytes on page `index` (e.g. a watermark).
    @discardableResult
    public func overlayPage(_ index: Int, content: [UInt8]) throws -> EditableDoc {
        try withBytes(content) { ptr, len in
            try check(Native.shared.pdf_editable_overlay_page(handle, UInt(index), ptr, len))
        }
        return self
    }

    /// Fill an AcroForm text field by name; returns whether it existed.
    @discardableResult
    public func fillTextField(name: String, value: String) throws -> Bool {
        var found: Int32 = 0
        try name.withCString { n in
            try value.withCString { v in
                try check(Native.shared.pdf_editable_fill_text_field(handle, n, v, &found))
            }
        }
        return found != 0
    }

    /// Drop unreferenced objects, recompress, dedupe and emit object streams on save.
    @discardableResult
    public func optimize() throws -> EditableDoc {
        try check(Native.shared.pdf_editable_optimize(handle)); return self
    }

    /// Toggle object streams + cross-reference stream output on save.
    @discardableResult
    public func compact(_ on: Bool) throws -> EditableDoc {
        try check(Native.shared.pdf_editable_compact(handle, on ? 1 : 0)); return self
    }

    /// Enable encryption on save. Requires a license. `user`/`owner` are the
    /// user and owner passwords (either may be empty); `readOnly` applies the
    /// read-only permission set.
    @discardableResult
    public func encrypt(method: Encryption, user: String = "", owner: String = "",
                        readOnly: Bool = false) throws -> EditableDoc {
        try user.withCString { u in
            try owner.withCString { o in
                try check(Native.shared.pdf_editable_encrypt(
                    handle, method.rawValue, u, o, readOnly ? 1 : 0))
            }
        }
        return self
    }

    /// Serialize the document to bytes.
    public func toBytes() throws -> [UInt8] {
        try takeBytes { out, len in Native.shared.pdf_editable_to_bytes(handle, out, len) }
    }

    /// Serialize as an incremental update over `original` (preserves it
    /// verbatim — signature-safe).
    public func toBytesIncremental(over original: [UInt8]) throws -> [UInt8] {
        try withBytes(original) { ptr, len in
            try takeBytes { out, outLen in
                Native.shared.pdf_editable_to_bytes_incremental(handle, ptr, len, out, outLen)
            }
        }
    }

    /// Save the document to `path`.
    public func save(to path: String) throws {
        try path.withCString { try check(Native.shared.pdf_editable_save(handle, $0)) }
    }
}

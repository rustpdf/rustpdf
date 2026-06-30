//
//  Helpers.swift
//  RustPdf
//
//  Small utilities shared by the idiomatic wrappers for crossing the C ABI:
//  passing optional C strings, and copying + freeing native out-buffers.
//

#if canImport(Darwin)
import Darwin
#elseif canImport(Glibc)
import Glibc
#endif
import Foundation

/// Run an out-buffer producer `(out_ptr, out_len) -> PdfStatus`, then copy the
/// native buffer into a Swift `[UInt8]` and free it with `pdf_buffer_free`.
func takeBytes(
    _ call: (UnsafeMutablePointer<UnsafeMutablePointer<UInt8>?>?,
             UnsafeMutablePointer<UInt>?) -> Int32
) throws -> [UInt8] {
    var ptr: UnsafeMutablePointer<UInt8>?
    var len: UInt = 0
    try check(call(&ptr, &len))
    guard let p = ptr, len > 0 else { return [] }
    defer { Native.shared.pdf_buffer_free(p, len) }
    return Array(UnsafeBufferPointer(start: p, count: Int(len)))
}

/// Copy a native out-buffer into a Swift `[UInt8]` and free it with
/// `pdf_buffer_free` (for exports with more than one out-buffer, where
/// ``takeBytes(_:)`` does not fit).
func copyAndFree(_ ptr: UnsafeMutablePointer<UInt8>?, _ len: UInt) -> [UInt8] {
    guard let p = ptr, len > 0 else { return [] }
    defer { Native.shared.pdf_buffer_free(p, len) }
    return Array(UnsafeBufferPointer(start: p, count: Int(len)))
}

/// `strdup` an optional Swift string into a heap C string (or `nil`). The
/// caller must `free` the result.
func dupCString(_ s: String?) -> UnsafeMutablePointer<CChar>? {
    guard let s = s else { return nil }
    return s.withCString { strdup($0) }
}

/// Call `body` with a borrowed pointer to the bytes of `data` (or `nil` when
/// empty). The pointer is valid only for the duration of `body`.
func withBytes<R>(_ data: [UInt8], _ body: (UnsafePointer<UInt8>?, UInt) throws -> R) rethrows -> R {
    try data.withUnsafeBufferPointer { buf in
        try body(buf.baseAddress, UInt(buf.count))
    }
}

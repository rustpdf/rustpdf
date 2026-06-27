//
//  Status.swift
//  RustPdf
//
//  Status codes and the error type surfaced by every fallible call.
//

import Foundation

/// Status code returned by every fallible native export (mirrors `PdfStatus`
/// in `include/pdf.h`).
public enum PdfStatus: Int32, Sendable {
    case ok              = 0
    case nullPointer     = 1
    case invalidUtf8     = 2
    case io              = 3
    case serialize       = 4
    case panic           = 5
    /// A document/argument could not be parsed.
    case parse           = 6
    /// A font could not be loaded/embedded.
    case font            = 7
    /// An image could not be decoded/embedded.
    case image           = 8
    /// Encryption setup failed.
    case encrypt         = 9
    /// Digital signing failed.
    case sign            = 10
    /// An out-of-range index or other invalid argument.
    case invalidArgument = 11
    /// License activation failed (bad signature, expired, or malformed).
    case license         = 12
}

/// An error raised by a native call: the `PdfStatus` plus the library's
/// thread-local last-error message.
public struct PdfError: Error, CustomStringConvertible {
    /// The status code returned by the failing export.
    public let status: PdfStatus
    /// The human-readable message from `pdf_last_error_message`, if any.
    public let message: String

    public var description: String {
        "rustpdf: status=\(status) (\(status.rawValue)): \(message)"
    }
}

/// Throw a ``PdfError`` (attaching the thread-local last-error message) unless
/// `raw` is ``PdfStatus/ok``.
func check(_ raw: Int32) throws {
    if raw == PdfStatus.ok.rawValue { return }
    let status = PdfStatus(rawValue: raw) ?? .panic
    throw PdfError(status: status, message: lastErrorMessage())
}

/// The current thread's last-error message, or a placeholder.
func lastErrorMessage() -> String {
    guard let p = Native.shared.pdf_last_error_message() else { return "unknown error" }
    return String(cString: p)
}

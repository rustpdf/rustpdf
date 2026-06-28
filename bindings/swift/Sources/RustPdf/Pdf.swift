//
//  Pdf.swift
//  RustPdf
//
//  Top-level (free-function) surface: library version, licensing, text
//  extraction and the digital-signature pipeline (sign / timestamp / DSS).
//

#if canImport(Darwin)
import Darwin
#elseif canImport(Glibc)
import Glibc
#endif
import Foundation

/// Options for ``Pdf/sign(pdf:keyDER:certDER:options:)``. Empty strings are
/// treated as absent.
public struct SignOptions: Sendable {
    /// The `/Reason` recorded in the signature dictionary.
    public var reason: String?
    /// The `/Location` recorded in the signature dictionary.
    public var location: String?
    /// The signer `/Name` recorded in the signature dictionary.
    public var name: String?
    /// Select PAdES-B-B (CAdES subfilter + ESS signing-certificate-v2 attribute).
    public var pades: Bool

    public init(reason: String? = nil, location: String? = nil,
                name: String? = nil, pades: Bool = false) {
        self.reason = reason
        self.location = location
        self.name = name
        self.pades = pades
    }
}

/// One signature's validation result (from ``Pdf/verifySignatures(_:)``).
public struct SignatureReport: Sendable, Decodable {
    /// The AcroForm field name, if any.
    public let fieldName: String?
    /// The signature sub-filter (e.g. `adbe.pkcs7.detached`, `ETSI.CAdES.detached`).
    public let subFilter: String
    /// The signer's common name, if extractable.
    public let signer: String?
    /// Whether the `/ByteRange` covers the whole document.
    public let coversWholeDocument: Bool
    /// Whether the signed digest matches the document bytes.
    public let digestValid: Bool
    /// Whether the CMS signature itself verifies.
    public let signatureValid: Bool
    /// Whether the signature is valid overall.
    public let isValid: Bool
    /// The four `/ByteRange` integers.
    public let byteRange: [Int]
}

/// The namespace for the package-level (static) entry points. Document
/// authoring lives on ``Document``; manipulation on ``EditableDoc``.
public enum Pdf {
    /// The native library version string.
    public static var version: String {
        guard let p = Native.shared.pdf_version() else { return "" }
        return String(cString: p)
    }

    /// Verify and activate a license token for this process, unlocking the
    /// corporate features it grants (PDF/A, signatures, encryption,
    /// accessibility) until it expires. Tokens may also be supplied via the
    /// `RUSTPDF_LICENSE` / `RUSTPDF_LICENSE_FILE` environment variables, which
    /// are auto-activated on first use.
    ///
    /// - Throws: ``PdfError`` with status ``PdfStatus/license`` if the token is
    ///   forged, expired or malformed.
    public static func activateLicense(_ token: String) throws {
        try token.withCString { try check(Native.shared.pdf_activate_license($0)) }
    }

    /// Extract a document's text (Unicode, via each font's `ToUnicode` map).
    public static func extractText(_ pdf: [UInt8]) throws -> String {
        let bytes = try withBytes(pdf) { ptr, len in
            try takeBytes { out, outLen in
                Native.shared.pdf_extract_text(ptr, len, out, outLen)
            }
        }
        return String(decoding: bytes, as: UTF8.self)
    }

    /// Extract every raster image from `pdf` into directory `dir` (JPEG verbatim
    /// as `.jpg`, everything else as `.png`, named `page{N}_{name}.{ext}`).
    ///
    /// - Returns: the number of images written.
    public static func extractImagesToDir(_ data: [UInt8], _ dir: String) throws -> Int {
        var count: UInt = 0
        try withBytes(data) { ptr, len in
            try dir.withCString { d in
                try check(Native.shared.pdf_extract_images_to_dir(ptr, len, d, &count))
            }
        }
        return Int(count)
    }

    /// Render page `page` (0-based) of `pdf` to a PNG image at `dpi`
    /// dots-per-inch. Page rendering is a licensed **Pro** feature: throws
    /// `PdfError` (status `License`) unless a license granting it is active.
    public static func renderPageToPng(_ pdf: [UInt8], page: Int = 0, dpi: Double = 150.0) throws -> [UInt8] {
        try withBytes(pdf) { ptr, len in
            try takeBytes { out, outLen in
                Native.shared.pdf_render_page_to_png(ptr, len, UInt(page), dpi, out, outLen)
            }
        }
    }

    /// Number of pages in `pdf` (free — no license required).
    public static func pageCount(_ pdf: [UInt8]) throws -> Int {
        var count: UInt = 0
        try withBytes(pdf) { ptr, len in
            try check(Native.shared.pdf_page_count(ptr, len, &count))
        }
        return Int(count)
    }

    /// Sign `pdf` with a PKCS#8 DER private key and a DER certificate,
    /// producing a new PDF (PKCS#7 detached, incremental update). Requires a
    /// license granting the signatures feature.
    public static func sign(pdf: [UInt8], keyDER: [UInt8], certDER: [UInt8],
                            options: SignOptions = SignOptions()) throws -> [UInt8] {
        let reason = dupCString(options.reason)
        let location = dupCString(options.location)
        let name = dupCString(options.name)
        defer { free(reason); free(location); free(name) }

        return try withBytes(pdf) { pp, pl in
            try withBytes(keyDER) { kp, kl in
                try withBytes(certDER) { cp, cl in
                    try takeBytes { out, outLen in
                        Native.shared.pdf_sign(
                            pp, pl, kp, kl, cp, cl,
                            UnsafePointer(reason), UnsafePointer(location), UnsafePointer(name),
                            options.pades ? 1 : 0, out, outLen)
                    }
                }
            }
        }
    }

    /// Append a document timestamp (`/DocTimeStamp`, PAdES-B-LTA) using a TSA
    /// key + certificate. `date` may be `nil` (a fixed, reproducible value is
    /// used). Requires a license.
    public static func timestamp(pdf: [UInt8], tsaKeyDER: [UInt8], tsaCertDER: [UInt8],
                                 date: String? = nil) throws -> [UInt8] {
        let d = dupCString(date)
        defer { free(d) }
        return try withBytes(pdf) { pp, pl in
            try withBytes(tsaKeyDER) { kp, kl in
                try withBytes(tsaCertDER) { cp, cl in
                    try takeBytes { out, outLen in
                        Native.shared.pdf_timestamp(pp, pl, kp, kl, cp, cl,
                                                    UnsafePointer(d), out, outLen)
                    }
                }
            }
        }
    }

    /// Validate every signature in `data` and return one report per signature.
    /// An empty array means the document is unsigned.
    public static func verifySignatures(_ data: [UInt8]) throws -> [SignatureReport] {
        let bytes = try withBytes(data) { ptr, len in
            try takeBytes { out, outLen in
                Native.shared.pdf_verify_signatures_json(ptr, len, out, outLen)
            }
        }
        if bytes.isEmpty { return [] }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        return try decoder.decode([SignatureReport].self, from: Data(bytes))
    }

    /// Append a Document Security Store (`/DSS`, PAdES-B-LT) with the given DER
    /// certificates and CRLs. Requires a license.
    public static func addDss(pdf: [UInt8], certs: [[UInt8]], crls: [[UInt8]]) throws -> [UInt8] {
        try withBytes(pdf) { pp, pl in
            try withByteArrays(certs) { certPtrs, certLens in
                try withByteArrays(crls) { crlPtrs, crlLens in
                    try takeBytes { out, outLen in
                        Native.shared.pdf_add_dss(
                            pp, pl,
                            certPtrs, certLens, UInt(certs.count),
                            crlPtrs, crlLens, UInt(crls.count),
                            out, outLen)
                    }
                }
            }
        }
    }
}

/// Borrow a `[[UInt8]]` as parallel `(const uint8_t *const *, const uintptr_t *)`
/// arrays for the duration of `body`. Pointers are valid only inside `body`.
private func withByteArrays<R>(
    _ items: [[UInt8]],
    _ body: (UnsafePointer<UnsafePointer<UInt8>?>?, UnsafePointer<UInt>?) throws -> R
) rethrows -> R {
    // Flatten into one contiguous buffer so every element pointer stays valid
    // simultaneously (nested withUnsafeBufferPointer would only pin one at a time).
    var flat: [UInt8] = []
    var lens: [UInt] = []
    var offsets: [Int] = []
    for item in items {
        offsets.append(flat.count)
        flat.append(contentsOf: item)
        lens.append(UInt(item.count))
    }
    if items.isEmpty {
        return try body(nil, nil)
    }
    return try flat.withUnsafeBufferPointer { base in
        let ptrs: [UnsafePointer<UInt8>?] = offsets.map { off in
            base.baseAddress.map { $0 + off }
        }
        return try ptrs.withUnsafeBufferPointer { pp in
            try lens.withUnsafeBufferPointer { lp in
                try body(pp.baseAddress, lp.baseAddress)
            }
        }
    }
}

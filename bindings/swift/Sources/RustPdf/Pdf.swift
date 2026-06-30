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
import CRustPdf
#if canImport(CryptoKit)
import CryptoKit
#endif

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
    /// The signer certificate issuer (RFC 4514 DN), if available.
    public let issuer: String?
    /// The signer certificate serial number (uppercase hex), if available.
    public let serialNumber: String?
    /// The certificate's not-before date (ISO-8601), if available.
    public let validFrom: String?
    /// The certificate's not-after date (ISO-8601), if available.
    public let validTo: String?
    /// The signature algorithm friendly name (e.g. `SHA256withRSA`), if known.
    public let algorithm: String?
    /// The signing time from the signed attributes (ISO-8601), if present.
    public let signingTime: String?
    /// The number of certificates embedded in the CMS.
    public let certCount: Int
    /// Whether the signature carries an embedded (or document) timestamp.
    public let hasTimestamp: Bool
}

/// One positional text match from ``Pdf/findText(_:query:caseSensitive:)``.
/// Coordinates are in PDF points with origin at the page's lower-left corner.
public struct TextHit: Sendable, Decodable {
    /// The 0-based page index the match was found on.
    public let page: Int
    /// The matched text.
    public let text: String
    /// The bounding-box left edge (points).
    public let x: Double
    /// The bounding-box bottom edge (points).
    public let y: Double
    /// The bounding-box width (points).
    public let width: Double
    /// The bounding-box height (points).
    public let height: Double
}

/// A signature-policy identifier (PAdES-EPES / ICP-Brasil AD-RB).
public struct SignaturePolicy: Sendable {
    /// The policy OID (dotted-decimal), e.g. the ICP-Brasil AD-RB OID.
    public var oid: String
    /// The policy document hash (under ``hashAlgorithmOid``).
    public var hash: [UInt8]
    /// Hash algorithm OID; `nil` = SHA-256.
    public var hashAlgorithmOid: String?
    /// Optional SPURI qualifier — where the policy can be retrieved.
    public var uri: String?

    public init(oid: String, hash: [UInt8] = [],
                hashAlgorithmOid: String? = nil, uri: String? = nil) {
        self.oid = oid
        self.hash = hash
        self.hashAlgorithmOid = hashAlgorithmOid
        self.uri = uri
    }
}

/// Options for deferred / external signing (issue #41). Empty strings and `nil`
/// are treated as absent.
public struct SigningOptions: Sendable {
    /// The `/Reason` recorded in the signature dictionary.
    public var reason: String?
    /// The `/Location` recorded in the signature dictionary.
    public var location: String?
    /// The signer `/Name` recorded in the signature dictionary.
    public var name: String?
    /// Produce a PAdES-B-B signature (`ETSI.CAdES.detached`).
    public var pades: Bool
    /// Certify the document (DocMDP) — use only on the first signature.
    public var certify: Certify
    /// Reserved `/Contents` bytes; 0 = library default (8192). Raise for large
    /// cloud-HSM CMS containers.
    public var containerSize: Int
    /// Signature-policy identifier (PAdES-EPES); `nil` = none.
    public var policy: SignaturePolicy?
    /// Draw a visible signature appearance (using the fields below).
    public var visible: Bool
    /// 0-based page index for the visible appearance.
    public var visiblePage: Int
    /// Appearance rectangle `[x0, y0, x1, y1]` in page points.
    public var visibleRect: (Double, Double, Double, Double)
    /// Appearance text lines (separated by `\n`); `nil` = none.
    public var visibleText: String?
    /// PNG/JPEG bytes of a handwritten-signature image; empty = none.
    public var visibleImage: [UInt8]

    public init(reason: String? = nil, location: String? = nil, name: String? = nil,
                pades: Bool = false, certify: Certify = .none,
                containerSize: Int = 0, policy: SignaturePolicy? = nil,
                visible: Bool = false, visiblePage: Int = 0,
                visibleRect: (Double, Double, Double, Double) = (0, 0, 0, 0),
                visibleText: String? = nil, visibleImage: [UInt8] = []) {
        self.reason = reason
        self.location = location
        self.name = name
        self.pades = pades
        self.certify = certify
        self.containerSize = containerSize
        self.policy = policy
        self.visible = visible
        self.visiblePage = visiblePage
        self.visibleRect = visibleRect
        self.visibleText = visibleText
        self.visibleImage = visibleImage
    }
}

/// A signature field discovered in a PDF (pre-signing inventory), from
/// ``Pdf/listSignatures(_:)``.
public struct SignatureField: Sendable {
    /// The field name.
    public let name: String
    /// Whether the field is already signed.
    public let signed: Bool

    public init(name: String, signed: Bool) {
        self.name = name
        self.signed = signed
    }
}

/// An in-progress two-phase signature (Model B). ``document`` holds the prepared
/// PDF (with a zero-filled `/Contents` placeholder) and ``bytes`` the exact bytes
/// the signature covers. Hand ``hash`` to a remote signer, build the CMS
/// container, then call ``complete(_:)``. The private key never reaches the
/// library.
public final class SigningSession {
    /// The prepared PDF (with a zero-filled `/Contents` placeholder).
    public let document: [UInt8]
    /// The exact bytes covered by the signature (the two ByteRange segments).
    public let bytes: [UInt8]

    init(document: [UInt8], bytes: [UInt8]) {
        self.document = document
        self.bytes = bytes
    }

    /// SHA-256 of ``bytes`` — the value an HSM signs.
    public var hash: [UInt8] {
        Array(SHA256.hash(data: Data(bytes)))
    }

    /// Phase 2: complete the signature by embedding a finished DER CMS / PKCS#7
    /// `container`, returning the final signed PDF.
    public func complete(_ container: [UInt8]) throws -> [UInt8] {
        try Pdf.completeSignature(document, container: container)
    }
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

    /// Find every occurrence of `query` in `pdf`, returning a positional
    /// ``TextHit`` (page + bounding box, in PDF points, origin lower-left) for
    /// each match. `caseSensitive` is `false` for case-insensitive matching.
    /// An empty array means no match.
    public static func findText(_ pdf: [UInt8], query: String,
                                caseSensitive: Bool = false) throws -> [TextHit] {
        let bytes = try withBytes(pdf) { ptr, len in
            try query.withCString { q in
                try takeBytes { out, outLen in
                    Native.shared.pdf_find_text_json(ptr, len, q, caseSensitive ? 1 : 0, out, outLen)
                }
            }
        }
        if bytes.isEmpty { return [] }
        return try JSONDecoder().decode([TextHit].self, from: Data(bytes))
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

    // MARK: - Deferred / external (HSM) signing — issue #41

    /// **Model A — external signer.** Sign `pdf` without handing the library a
    /// key: it builds the CMS signed attributes and calls `signHash` for the raw
    /// RSA PKCS#1 v1.5 signature (over SHA-256 of the bytes it passes), then
    /// assembles and embeds the CMS. `certificate` is the signer certificate;
    /// `chain` are intermediates (DER), supplied independently of the key.
    /// Requires a license granting the signatures feature.
    public static func signWith(
        _ pdf: [UInt8], certificate certDer: [UInt8],
        chain: [[UInt8]] = [], options: SigningOptions? = nil,
        sign signHash: @escaping ([UInt8]) throws -> [UInt8]
    ) throws -> [UInt8] {
        let box = SignBox(signHash)
        let ctx = Unmanaged.passRetained(box).toOpaque()
        defer { Unmanaged<SignBox>.fromOpaque(ctx).release() }

        let result: [UInt8]
        do {
            result = try withBytes(pdf) { pp, pl in
                try withBytes(certDer) { cp, cl in
                    try withByteArrays(chain) { chainPtrs, chainLens in
                        try withSigningOptions(options) { params in
                            try takeBytes { out, outLen in
                                Native.shared.pdf_sign_with(
                                    pp, pl, cp, cl,
                                    chainPtrs, chainLens, UInt(chain.count),
                                    params, signTrampoline, ctx, out, outLen)
                            }
                        }
                    }
                }
            }
        } catch {
            // Surface the signer's own error in preference to the FFI status.
            if let inner = box.error { throw inner }
            throw error
        }
        if let inner = box.error { throw inner }
        return result
    }

    /// **Model B — two-phase signing, phase 1.** Prepare `pdf` for deferred
    /// signing: returns a ``SigningSession`` whose ``SigningSession/hash`` you
    /// send to a remote HSM. Build the CMS container, then call
    /// ``SigningSession/complete(_:)`` (or ``completeSignature(_:container:)``).
    /// The key never reaches the library.
    public static func beginSigning(_ pdf: [UInt8], options: SigningOptions? = nil) throws -> SigningSession {
        var docPtr: UnsafeMutablePointer<UInt8>?
        var docLen: UInt = 0
        var tbsPtr: UnsafeMutablePointer<UInt8>?
        var tbsLen: UInt = 0
        try withBytes(pdf) { pp, pl in
            try withSigningOptions(options) { params in
                try check(Native.shared.pdf_sign_begin(
                    pp, pl, params, &docPtr, &docLen, &tbsPtr, &tbsLen))
            }
        }
        let document = copyAndFree(docPtr, docLen)
        let bytes = copyAndFree(tbsPtr, tbsLen)
        return SigningSession(document: document, bytes: bytes)
    }

    /// **Model B — two-phase signing, phase 2.** Embed a complete DER CMS /
    /// PKCS#7 `container` into a prepared `document` (from ``beginSigning(_:options:)``),
    /// producing the final signed PDF.
    public static func completeSignature(_ document: [UInt8], container: [UInt8]) throws -> [UInt8] {
        try withBytes(document) { dp, dl in
            try withBytes(container) { cp, cl in
                try takeBytes { out, outLen in
                    Native.shared.pdf_sign_complete(dp, dl, cp, cl, out, outLen)
                }
            }
        }
    }

    /// List the signature fields in `pdf` (detect existing signatures before
    /// signing — the iText `SignatureUtil.getSignatureNames` equivalent). An
    /// empty array means there are no signature fields.
    public static func listSignatures(_ pdf: [UInt8]) throws -> [SignatureField] {
        let bytes = try withBytes(pdf) { ptr, len in
            try takeBytes { out, outLen in
                Native.shared.pdf_list_signatures(ptr, len, out, outLen)
            }
        }
        let text = String(decoding: bytes, as: UTF8.self)
        var fields: [SignatureField] = []
        for line in text.split(separator: "\n", omittingEmptySubsequences: true) {
            guard let tab = line.firstIndex(of: "\t") else { continue }
            let signed = line[..<tab] == "1"
            let name = String(line[line.index(after: tab)...])
            fields.append(SignatureField(name: name, signed: signed))
        }
        return fields
    }

    // MARK: - Network timestamp (AD-RT) — issue #41

    /// **Network timestamp, phase 1.** Prepare `pdf` for a `/DocTimeStamp` from a
    /// network RFC 3161 TSA. Returns the prepared `document` (with a zero-filled
    /// `/Contents` placeholder) and the `bytes` to be timestamped. SHA-256 the
    /// bytes, build a request with ``timestampRequest(imprint:nonce:certReq:)``,
    /// POST it to the TSA, extract the token with
    /// ``timestampToken(fromResponse:)``, then embed it via
    /// ``completeSignature(_:container:)``.
    public static func beginTimestamp(_ pdf: [UInt8]) throws -> (document: [UInt8], bytes: [UInt8]) {
        var docPtr: UnsafeMutablePointer<UInt8>?
        var docLen: UInt = 0
        var tbsPtr: UnsafeMutablePointer<UInt8>?
        var tbsLen: UInt = 0
        try withBytes(pdf) { pp, pl in
            try check(Native.shared.pdf_timestamp_begin(
                pp, pl, &docPtr, &docLen, &tbsPtr, &tbsLen))
        }
        return (copyAndFree(docPtr, docLen), copyAndFree(tbsPtr, tbsLen))
    }

    /// Build an RFC 3161 `TimeStampReq` (DER) for `imprint` (the SHA-256 of the
    /// bytes to timestamp). `nonce` is optional (`nil` = none); `certReq`
    /// asks the TSA to embed its certificate.
    public static func timestampRequest(imprint: [UInt8], nonce: [UInt8]? = nil,
                                        certReq: Bool = true) throws -> [UInt8] {
        try withBytes(imprint) { ip, il in
            try withBytes(nonce ?? []) { np, nl in
                try takeBytes { out, outLen in
                    Native.shared.pdf_timestamp_request(
                        ip, il, np, nl, certReq ? 1 : 0, out, outLen)
                }
            }
        }
    }

    /// Extract the `TimeStampToken` (a CMS `ContentInfo`) from a TSA's RFC 3161
    /// `TimeStampResp`. The returned token is embedded via
    /// ``completeSignature(_:container:)``.
    public static func timestampToken(fromResponse response: [UInt8]) throws -> [UInt8] {
        try withBytes(response) { rp, rl in
            try takeBytes { out, outLen in
                Native.shared.pdf_timestamp_token_from_response(rp, rl, out, outLen)
            }
        }
    }

    /// Build a C ``PdfSigningOptions`` from `options` and run `body` with a
    /// pointer to it (or `nil` when `options` is `nil`). Heap C strings are freed
    /// afterwards; the policy hash is borrowed for the duration of `body`.
    private static func withSigningOptions<R>(
        _ options: SigningOptions?,
        _ body: (UnsafePointer<PdfSigningOptions>?) throws -> R
    ) rethrows -> R {
        guard let o = options else { return try body(nil) }
        let reason = dupCString(o.reason)
        let location = dupCString(o.location)
        let name = dupCString(o.name)
        let policyOid = dupCString(o.policy?.oid)
        let policyHashAlg = dupCString(o.policy?.hashAlgorithmOid)
        let policyUri = dupCString(o.policy?.uri)
        let visText = dupCString(o.visibleText)
        defer {
            free(reason); free(location); free(name)
            free(policyOid); free(policyHashAlg); free(policyUri)
            free(visText)
        }
        func c(_ p: UnsafeMutablePointer<CChar>?) -> UnsafePointer<CChar>? { p.map { UnsafePointer($0) } }

        let hash = o.policy?.hash ?? []
        let image = o.visibleImage
        return try hash.withUnsafeBufferPointer { hbuf in
            try image.withUnsafeBufferPointer { ibuf in
                var params = PdfSigningOptions()
                params.reason = c(reason)
                params.location = c(location)
                params.name = c(name)
                params.pades = o.pades ? 1 : 0
                params.certification = o.certify.rawValue
                params.estimated_size = o.containerSize > 0 ? UInt(o.containerSize) : 0
                params.policy_oid = c(policyOid)
                if o.policy != nil, !hash.isEmpty {
                    params.policy_hash = hbuf.baseAddress
                    params.policy_hash_len = UInt(hash.count)
                }
                params.policy_hash_alg_oid = c(policyHashAlg)
                params.policy_uri = c(policyUri)
                params.visible = o.visible ? 1 : 0
                params.vis_page = UInt(o.visiblePage)
                params.vis_rect = (o.visibleRect.0, o.visibleRect.1, o.visibleRect.2, o.visibleRect.3)
                params.vis_text = c(visText)
                if !image.isEmpty {
                    params.vis_image = ibuf.baseAddress
                    params.vis_image_len = UInt(image.count)
                }
                return try withUnsafePointer(to: &params) { try body($0) }
            }
        }
    }
}

/// A reference box carrying the Model-A signer closure across the C boundary
/// (passed as `ctx`), plus a slot for an error it threw.
final class SignBox {
    let sign: ([UInt8]) throws -> [UInt8]
    var error: Error?
    init(_ sign: @escaping ([UInt8]) throws -> [UInt8]) { self.sign = sign }
}

/// The non-capturing `@convention(c)` trampoline handed to `pdf_sign_with`. It
/// recovers the ``SignBox`` from `ctx`, calls the Swift closure with the bytes
/// to sign, and copies the returned RSA signature into `sigBuf` (respecting
/// `sigCap`). Returns 0 on success, non-zero on failure.
private let signTrampoline: @convention(c) (
    UnsafeMutableRawPointer?, UnsafePointer<UInt8>?, UInt,
    UnsafeMutablePointer<UInt8>?, UInt, UnsafeMutablePointer<UInt>?
) -> Int32 = { ctx, data, dataLen, sigBuf, sigCap, sigLen in
    guard let ctx = ctx else { return 1 }
    let box = Unmanaged<SignBox>.fromOpaque(ctx).takeUnretainedValue()
    do {
        let input: [UInt8] = data.map { Array(UnsafeBufferPointer(start: $0, count: Int(dataLen))) } ?? []
        let sig = try box.sign(input)
        if UInt(sig.count) > sigCap {
            box.error = PdfError(status: .sign, message: "signature (\(sig.count) bytes) exceeds buffer capacity")
            return 2
        }
        if let sb = sigBuf {
            sig.withUnsafeBufferPointer { buf in
                if let base = buf.baseAddress { sb.update(from: base, count: sig.count) }
            }
        }
        sigLen?.pointee = UInt(sig.count)
        return 0
    } catch {
        box.error = error
        return 1
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

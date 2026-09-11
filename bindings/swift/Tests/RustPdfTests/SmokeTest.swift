//
//  SmokeTest.swift
//  RustPdfTests
//
//  One test exercises the whole product surface serially. Mirrors the
//  Go/Java/Delphi smoke tests over the same C ABI.
//

import XCTest
import Foundation
#if canImport(CryptoKit)
import CryptoKit
#endif
@testable import RustPdf

final class SmokeTest: XCTestCase {

    // Locate the workspace root by walking up from this source file to the
    // directory holding the top-level Cargo.toml.
    private func repoRoot() throws -> URL {
        var dir = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
        for _ in 0..<12 {
            if FileManager.default.fileExists(atPath: dir.appendingPathComponent("Cargo.toml").path),
               FileManager.default.fileExists(atPath: dir.appendingPathComponent("crates").path) {
                return dir
            }
            let parent = dir.deletingLastPathComponent()
            if parent == dir { break }
            dir = parent
        }
        throw XCTSkip("could not locate the workspace root (Cargo.toml)")
    }

    private func read(_ url: URL) throws -> [UInt8] {
        [UInt8](try Data(contentsOf: url))
    }

    func testFullSurface() throws {
        let root = try repoRoot()
        let fontURL = root.appendingPathComponent("assets/fonts/Roboto-Regular.ttf")

        XCTAssertFalse(Pdf.version.isEmpty, "version must be non-empty")

        // 1. Every feature is free.
        do {
            let d = try Document()
            try d.pdfa()
            try d.addPage()
            XCTAssertFalse(try d.toBytes().isEmpty, "PDF/A must work")
        }

        let font = try read(fontURL)
        var pdfa: [UInt8] = []
        do {
            let d = try Document()
            try d.pdfa(.a2a)
            try d.setInfo(DocumentInfo(title: "Olá", author: "rustpdf"))
            let f = try d.addFont(data: font)
            try d.addPage()
            try d.showText(font: f, size: 20, x: 72, y: 760, "Título", headingLevel: 1)
            try d.paragraph(font: f, size: 12, x: 72, y: 720, width: 450,
                            text: String(repeating: "Um parágrafo. ", count: 8), align: .justify)
            pdfa = try d.toBytes()
            XCTAssertEqual(d.pageCount, 1)
        }

        // 3. Text extraction.
        let text = try Pdf.extractText(pdfa)
        XCTAssertTrue(text.contains("Título"), "extracted text: \(text)")

        // 3a. Positional text search (issue #41).
        let hits = try Pdf.findText(pdfa, query: "Título")
        XCTAssertGreaterThanOrEqual(hits.count, 1, "expected at least one text hit")
        if let h = hits.first {
            XCTAssertEqual(h.page, 0, "hit should be on page 0")
            XCTAssertFalse(h.text.isEmpty, "hit text should be set")
            XCTAssertTrue(h.width > 0 && h.height > 0, "hit should have a non-empty box: \(h)")
        }
        XCTAssertTrue(try Pdf.findText(pdfa, query: "no-such-string-xyz").isEmpty,
                      "missing query should yield no hits")

        // 3b. Page rendering.
        XCTAssertEqual(try Pdf.pageCount(pdfa), 1)
        let png = try Pdf.renderPageToPng(pdfa, page: 0, dpi: 72.0)
        XCTAssertTrue(png.count > 8 && png[1] == 0x50 && png[2] == 0x4E && png[3] == 0x47,
                      "expected a PNG header")

        // 4. Incremental update preserves the original prefix.
        do {
            let ed = try EditableDoc(loading: pdfa)
            XCTAssertEqual(ed.pageCount, 1)
            try ed.setInfo(key: "Subject", value: "via FFI")
            let incr = try ed.toBytesIncremental(over: pdfa)
            XCTAssertTrue(incr.starts(with: pdfa), "incremental must preserve the original")
            XCTAssertEqual(try ed.getInfo(key: "Subject"), "via FFI")
        }

        // 5. Merge + optimize.
        do {
            let a = try EditableDoc(loading: pdfa)
            let b = try EditableDoc(loading: pdfa)
            try a.merge(b)
            try a.optimize()
            let out = try a.toBytes()
            let merged = try EditableDoc(loading: out)
            XCTAssertEqual(merged.pageCount, 2)
        }

        // 6. AcroForm with every field type.
        do {
            let d = try Document()
            try d.addPage()
            try d.textField(name: "city", page: 0, rect: (120, 700, 300, 720), value: "SP", size: 12)
            try d.checkbox(name: "ok", page: 0, rect: (120, 670, 138, 688), checked: true)
            try d.radioGroup(name: "plan", page: 0, buttons: [
                RadioButton(rect: (120, 640, 138, 658), export: "a"),
                RadioButton(rect: (160, 640, 178, 658), export: "b"),
            ], selected: 1)
            try d.dropdown(name: "country", page: 0, rect: (120, 610, 300, 630),
                           options: ["BR", "PT"], selected: 0, size: 12)
            let form = try d.toBytes()
            XCTAssertTrue(contains(form, "/AcroForm"), "AcroForm missing")
        }

        // 7. Encryption (AES-256) round-trips.
        var plain: [UInt8] = []
        do {
            let d = try Document()
            let f = try d.addFont(data: font)
            try d.addPage()
            try d.showText(font: f, size: 14, x: 72, y: 700, "segredo")
            plain = try d.toBytes()
        }
        do {
            let ed = try EditableDoc(loading: plain)
            try ed.encrypt(method: .aes256, user: "", owner: "owner", readOnly: false)
            let enc = try ed.toBytes()
            XCTAssertTrue(contains(enc, "/AESV3"), "AES-256 marker missing")
            let dec = try Pdf.extractText(enc)
            XCTAssertTrue(dec.contains("segredo"), "decrypted text: \(dec)")
        }

        // 8. Digital signature (PKCS#7 / PAdES) with the committed test key.
        let fx = root.appendingPathComponent("crates/pdf/tests/fixtures")
        let key = try read(fx.appendingPathComponent("signer_key.pk8"))
        let cert = try read(fx.appendingPathComponent("signer_cert.der"))
        let signed = try Pdf.sign(pdf: plain, keyDER: key, certDER: cert,
                                  options: SignOptions(reason: "Aprovado", pades: true))
        XCTAssertTrue(contains(signed, "/ByteRange"), "signature ByteRange missing")

        // 9. Image extraction: embed a PNG, then pull every raster image back out.
        var withImg: [UInt8] = []
        do {
            let d = try Document()
            try d.addPage()
            let img = try d.addImagePNG(data: Self.tinyPNG)
            _ = try d.drawImage(img, x: 72, y: 600, width: 64, height: 64)
            withImg = try d.toBytes()
        }
        let outDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("rustpdf_images_\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)
        let n = try Pdf.extractImagesToDir(withImg, outDir.path)
        XCTAssertGreaterThanOrEqual(n, 1, "expected at least one image, got \(n)")
        let written = try FileManager.default.contentsOfDirectory(atPath: outDir.path)
        XCTAssertEqual(written.count, n, "count \(n) != files written \(written)")

        // 10. Links + bookmarks (Tier 1) on an authored document.
        do {
            let d = try Document()
            let f = try d.addFont(data: font)
            try d.addPage()
            try d.showText(font: f, size: 16, x: 72, y: 740, "Page 1")
            try d.linkURI(rect: (72, 700, 200, 720), uri: "https://example.com")
            try d.addPage()
            try d.showText(font: f, size: 16, x: 72, y: 740, "Page 2")
            try d.linkToPage(rect: (72, 700, 200, 720), pageIndex: 0, top: 800)
            try d.addBookmark(
                Bookmark(title: "Chapter 1", page: 0)
                    .child(Bookmark(title: "Section 1.1", page: 0, top: 720))
                    .child(Bookmark(title: "Section 1.2", page: 1)))
            try d.addBookmark(Bookmark(title: "Chapter 2", page: 1))
            let bytes = try d.toBytes()
            XCTAssertTrue(contains(bytes, "/URI"), "link annotation missing")
            XCTAssertTrue(contains(bytes, "/Outlines"), "outline missing")
        }

        // 11. Factur-X / ZUGFeRD (Tier 2, PDF/A-3b).
        do {
            let d = try Document()
            let f = try d.addFont(data: font)
            try d.addPage()
            try d.showText(font: f, size: 12, x: 72, y: 740, "Invoice")
            let xml = Array("<?xml version=\"1.0\"?><rsm:CrossIndustryInvoice/>".utf8)
            try d.facturx(xml, profile: .en16931)
            let bytes = try d.toBytes()
            XCTAssertTrue(contains(bytes, "factur-x.xml"), "factur-x attachment missing")
        }

        // 12. Form manipulation on an EditableDoc: set + flatten + field names.
        do {
            let d = try Document()
            try d.addPage()
            try d.textField(name: "city", page: 0, rect: (120, 700, 300, 720), value: "", size: 12)
            try d.checkbox(name: "ok", page: 0, rect: (120, 670, 138, 688), checked: false)
            try d.radioGroup(name: "plan", page: 0, buttons: [
                RadioButton(rect: (120, 640, 138, 658), export: "a"),
                RadioButton(rect: (160, 640, 178, 658), export: "b"),
            ], selected: -1)
            try d.dropdown(name: "country", page: 0, rect: (120, 610, 300, 630),
                           options: ["BR", "PT"], selected: -1, size: 12)
            let formBytes = try d.toBytes()

            let ed = try EditableDoc(loading: formBytes)
            let names = try ed.fieldNames()
            XCTAssertTrue(names.contains("city"), "field names: \(names)")
            XCTAssertTrue(try ed.fillTextField(name: "city", value: "SP"))
            XCTAssertTrue(try ed.setCheckbox(name: "ok", checked: true))
            XCTAssertTrue(try ed.setRadio(name: "plan", exportValue: "b"))
            XCTAssertTrue(try ed.setChoice(name: "country", value: "PT"))
            XCTAssertFalse(try ed.setCheckbox(name: "nope"))
            try ed.flattenForms()
            let flat = try ed.toBytes()
            XCTAssertFalse(contains(flat, "/AcroForm"), "AcroForm should be gone after flatten")
        }

        // 13. Watermarks + redaction on an EditableDoc.
        do {
            let ed = try EditableDoc(loading: plain)
            try ed.watermarkText("DRAFT", size: 48, color: (0.6, 0.6, 0.6), opacity: 0.25)
            let pngPath = outDir.appendingPathComponent("wm.png").path
            try Data(Self.tinyPNG).write(to: URL(fileURLWithPath: pngPath))
            try ed.watermarkImageFile(path: pngPath, width: 32, height: 32, opacity: 0.2)
            XCTAssertTrue(try ed.redact(0, rects: [(70, 695, 160, 715)]), "page 0 should exist")
            XCTAssertFalse(try ed.redact(99, rects: [(0, 0, 1, 1)]), "page 99 should not exist")
            _ = try ed.toBytes()
        }

        // 14. Convert an existing (font-embedded) document to PDF/A.
        do {
            let ed = try EditableDoc(loading: plain)
            try ed.convertToPdfa(.a2b)
            let out = try ed.toBytes()
            XCTAssertTrue(contains(out, "pdfaid"), "PDF/A identifier missing after conversion")

            // 14b. Normalize back to a plain PDF (strip PDF/A + set version).
            let ed2 = try EditableDoc(loading: out)
            try ed2.setVersion(.v17)
            try ed2.normalize(.v17)
            let plainAgain = try ed2.toBytes()
            XCTAssertFalse(contains(plainAgain, "pdfaid"), "pdfaid should be gone after normalize")
            XCTAssertTrue(plainAgain.starts(with: Array("%PDF-1.7".utf8)), "header should be 1.7")
        }

        // 15. Verify signatures on the freshly-signed document from step 8.
        let reports = try Pdf.verifySignatures(signed)
        XCTAssertGreaterThanOrEqual(reports.count, 1, "expected at least one signature")
        if let r = reports.first {
            XCTAssertEqual(r.byteRange.count, 4, "byteRange should have 4 ints")
            XCTAssertFalse(r.subFilter.isEmpty, "subFilter should be set")
            // Rich signature inspection (issue #41): new fields are accessible.
            XCTAssertGreaterThanOrEqual(r.certCount, 1, "expected at least one cert")
            XCTAssertNotNil(r.algorithm, "algorithm should be reported")
            XCTAssertNotNil(r.serialNumber, "serial number should be reported")
            XCTAssertNotNil(r.validFrom, "validFrom should be reported")
            _ = r.issuer; _ = r.validTo; _ = r.signingTime; _ = r.hasTimestamp
        }
        XCTAssertTrue(try Pdf.verifySignatures(plain).isEmpty, "unsigned doc should report no signatures")

        // 16. Deferred / external signing (issue #41).
        //
        // Model A: the library never sees the private key — it calls back into
        // Swift for the raw RSA-PKCS#1-v1.5-SHA256 signature, which we produce by
        // shelling out to `openssl` over the PKCS#8 DER fixture key (proving the
        // key stays outside the library, exactly like a remote HSM).
        let keyPK8 = fx.appendingPathComponent("signer_key.pk8").path
        guard let opensslPath = Self.findOpenssl() else {
            throw XCTSkip("openssl not found on PATH — skipping deferred-signing proof")
        }
        let rsaSign: ([UInt8]) throws -> [UInt8] = { data in
            try Self.opensslSign(opensslPath: opensslPath, keyDerPath: keyPK8, data: data)
        }

        let signedA = try Pdf.signWith(
            plain, certificate: cert,
            options: SigningOptions(reason: "Deferred", name: "HSM signer", pades: true),
            sign: rsaSign)
        XCTAssertTrue(contains(signedA, "/ByteRange"), "Model A: signature ByteRange missing")
        XCTAssertTrue(contains(signedA, "/ETSI.CAdES.detached"), "Model A: PAdES subfilter missing")

        let reportsA = try Pdf.verifySignatures(signedA)
        XCTAssertGreaterThanOrEqual(reportsA.count, 1, "Model A: expected one signature")
        if let r = reportsA.first {
            XCTAssertTrue(r.isValid, "Model A signature must verify (digest+CMS): \(r)")
            XCTAssertTrue(r.digestValid, "Model A digest must match")
            XCTAssertTrue(r.signatureValid, "Model A CMS signature must verify")
            XCTAssertTrue(r.coversWholeDocument, "Model A: ByteRange should cover the document")
        }

        // listSignatures: one (signed) field after signing; none on the unsigned doc.
        let listed = try Pdf.listSignatures(signedA)
        XCTAssertEqual(listed.count, 1, "expected exactly one signature field: \(listed)")
        if let f = listed.first {
            XCTAssertTrue(f.signed, "field should be marked signed")
            XCTAssertFalse(f.name.isEmpty, "field name should be set")
        }
        XCTAssertTrue(try Pdf.listSignatures(plain).isEmpty, "unsigned doc should list no fields")

        // Model B: two-phase session exposes the prepared doc, the to-be-signed
        // bytes and their 32-byte SHA-256 digest.
        let session = try Pdf.beginSigning(plain, options: SigningOptions(reason: "Two-phase"))
        XCTAssertFalse(session.document.isEmpty, "Model B: prepared document must be non-empty")
        XCTAssertFalse(session.bytes.isEmpty, "Model B: to-be-signed bytes must be non-empty")
        XCTAssertEqual(session.hash.count, 32, "Model B: SHA-256 hash must be 32 bytes")

        // 16b. Visible signature appearance (Model A) — issue #41.
        let visOpts = SigningOptions(
            reason: "Visible", name: "HSM signer", visible: true,
            visiblePage: 0, visibleRect: (72, 600, 272, 660),
            visibleText: "Signed by HSM\nrustpdf")
        let signedVis = try Pdf.signWith(plain, certificate: cert, options: visOpts, sign: rsaSign)
        XCTAssertTrue(contains(signedVis, "/ByteRange"), "visible sig ByteRange missing")
        XCTAssertGreaterThanOrEqual(try Pdf.verifySignatures(signedVis).count, 1,
                                    "visible signature should verify")

        // 17. Network timestamp (AD-RT) — issue #41. Offline phases only: prepare,
        // hash, and build the RFC 3161 request (no live TSA in the smoke test).
        let (tsDoc, tsBytes) = try Pdf.beginTimestamp(plain)
        XCTAssertFalse(tsDoc.isEmpty, "timestamp prepared doc must be non-empty")
        XCTAssertFalse(tsBytes.isEmpty, "timestamp to-be-signed bytes must be non-empty")
        let imprint = Array(SHA256.hash(data: Data(tsBytes)))
        let tsReq = try Pdf.timestampRequest(imprint: imprint)
        XCTAssertFalse(tsReq.isEmpty, "RFC 3161 request must be non-empty")

        // 18. Page geometry (issue #45 P1 #1): read-only per-page measurements,
        // including the width/height swap after a 90° rotation.
        let geom = try Pdf.measurePages(pdfa)
        XCTAssertEqual(geom.count, 1, "measure_pages count")
        XCTAssertTrue(geom[0].width > 0 && geom[0].height > 0, "page has a size")
        XCTAssertTrue(geom[0].mediaBox.width > 0, "media box width")
        var rotatedPdf: [UInt8] = []
        do {
            let ed = try EditableDoc(loading: pdfa)
            try ed.rotatePage(0, degrees: 90)
            rotatedPdf = try ed.toBytes()
        }
        let g0 = try Pdf.measurePage(rotatedPdf, 0)
        XCTAssertEqual(g0.rotation, 90, "rotation read back")
        XCTAssertEqual(g0.rotatedWidth, g0.height, accuracy: 0.1, "90° swaps width/height")
        XCTAssertEqual(g0.rotatedHeight, g0.width, accuracy: 0.1, "90° swaps height/width")
        XCTAssertThrowsError(try Pdf.measurePage(pdfa, 99), "out-of-range page must throw")

        // 19. Inspection (issue #45 P1 #3): version / PDF/A level / encryption.
        let info = try Pdf.inspect(pdfa)
        XCTAssertEqual(info.pageCount, 1, "inspect page count")
        XCTAssertFalse(info.encrypted, "plain doc not encrypted")
        XCTAssertEqual(info.encryption, "None", "plain doc cipher None")
        XCTAssertNotNil(info.pdfaLevel, "PDF/A level should be detected")
        var encSample: [UInt8] = []
        do {
            let ed = try EditableDoc(loading: plain)
            try ed.encrypt(method: .aes256, user: "", owner: "owner", readOnly: false)
            encSample = try ed.toBytes()
        }
        let encInfo = try Pdf.inspect(encSample)
        XCTAssertTrue(encInfo.encrypted, "encrypted doc detected")

        // 20. Positioned drawing primitives (issue #45 P1 #2): fill rect + text.
        var drawn: [UInt8] = []
        do {
            let ed = try EditableDoc(loading: pdfa)
            XCTAssertTrue(ed.fillRect(0, 100, 100, 200, 40), "fill_rect page existed")
            XCTAssertTrue(ed.placeText(0, 110, 112, "STAMPED", size: 14, color: (0, 0, 1)),
                          "place_text page existed")
            XCTAssertFalse(ed.fillRect(99, 0, 0, 1, 1), "fill_rect missing page")
            drawn = try ed.toBytes()
        }
        // NOTE: String.contains("") is true-trapping in Swift — assert the real
        // substring, never an empty one.
        XCTAssertTrue(try Pdf.extractText(drawn).contains("STAMPED"),
                      "placed text should be extractable")

        // 21. Draw an image onto an existing page (issue #50).
        do {
            let ed = try EditableDoc(loading: pdfa)
            XCTAssertTrue(ed.drawImage(0, image: Self.tinyPNG, x: 72, y: 600,
                                       width: 64, height: 64), "draw_image page existed")
            XCTAssertFalse(ed.drawImage(99, image: Self.tinyPNG, x: 0, y: 0,
                                        width: 1, height: 1), "draw_image missing page")
            let stamped = try ed.toBytes()
            XCTAssertTrue(stamped.count > 8 && stamped[0] == 0x25 && stamped[1] == 0x50,
                          "serialized PDF with drawn image")
        }

        // 22. Aligned positioned text + masked text (ForSign integration).
        var aligned: [UInt8] = []
        do {
            let ed = try EditableDoc(loading: pdfa)
            XCTAssertTrue(ed.placeText(0, 300, 200, "RIGHT", size: 12, align: .right),
                          "place_text aligned page existed")
            XCTAssertTrue(ed.maskedText(0, 100, 150, 200, 24, "MASKED",
                                        size: 12, align: .center),
                          "masked_text page existed")
            XCTAssertFalse(ed.maskedText(99, 0, 0, 1, 1, "x"), "masked_text missing page")
            aligned = try ed.toBytes()
        }
        XCTAssertTrue(try Pdf.extractText(aligned).contains("MASKED"),
                      "masked text should be extractable")

        // 23. Single-page text extraction (fast path).
        let page0 = try Pdf.extractPageText(pdfa, pageIndex: 0)
        XCTAssertTrue(page0.contains("Título"), "single-page extraction returns page text")
        XCTAssertThrowsError(try Pdf.extractPageText(pdfa, pageIndex: 99),
                             "out-of-range page index throws")

        // 24. Stamping fonts + vertical anchors + paragraph wrapping (stamping v2).
        do {
            let ed = try EditableDoc(loading: pdfa)
            // Embedded stamping font + layout line-box anchor.
            let fid = try ed.addFontFile(fontURL.path)
            XCTAssertGreaterThanOrEqual(fid, 0, "font id should be non-negative")
            XCTAssertTrue(ed.placeText(0, 72, 500, "ANCHORED", size: 12,
                                       fontId: fid, anchor: .lineBottom),
                          "place_text anchored page+font existed")
            XCTAssertFalse(ed.placeText(0, 72, 500, "x", fontId: 99),
                           "invalid font id reports not found")
            // Masked text hung from the top edge, flush with the box (padding 0).
            XCTAssertTrue(ed.maskedText(0, 100, 420, 200, 30, "VTOP", size: 12,
                                        valign: .top, padding: 0),
                          "masked_text valign page existed")
            // Paragraph wrapping: a narrow box must break between the words.
            XCTAssertTrue(ed.placeParagraph(0, 72, 400, 90,
                                            "wrapme alpha bravo charlie delta echo", size: 12),
                          "place_paragraph page existed")
            let (lines, height) = ed.placeParagraphMeasured(
                0, 72, 300, 90, "wrapme alpha bravo charlie delta echo",
                size: 12, anchor: .bottom)
            XCTAssertGreaterThanOrEqual(lines, 2, "narrow paragraph should wrap: \(lines) lines")
            XCTAssertGreaterThan(height, 12.0, "consumed height should exceed one line: \(height)")
            // Media (raw user-space) stamping coordinates.
            try ed.setStampSpace(.media)
            XCTAssertTrue(ed.placeText(0, 72, 260, "MEDIA-SPACE", size: 12),
                          "place_text in media space page existed")
            try ed.setStampSpace(.visible)
            // Bounding-box anchored image rotation (legacy layout semantics).
            XCTAssertTrue(ed.drawImage(0, image: Self.tinyPNG, x: 72, y: 200,
                                       width: 40, height: 20, rotationDeg: 90,
                                       anchor: .boundingBox),
                          "draw_image bounding-box anchored page existed")
            let stamped = try ed.toBytes()
            // NOTE: never assert contains("") — it is false in Swift.
            let stampedText = try Pdf.extractText(stamped)
            XCTAssertTrue(stampedText.contains("ANCHORED"), "embedded-font stamp extractable")
            XCTAssertTrue(stampedText.contains("VTOP"), "valign masked text extractable")
            XCTAssertTrue(stampedText.contains("wrapme"), "wrapped paragraph extractable")
            XCTAssertTrue(stampedText.contains("MEDIA-SPACE"), "media-space stamp extractable")
        }
    }

    /// Locate the `openssl` CLI for the Model-A signer (a stand-in HSM).
    private static func findOpenssl() -> String? {
        var candidates = ["/opt/homebrew/bin/openssl", "/usr/local/bin/openssl", "/usr/bin/openssl"]
        if let path = ProcessInfo.processInfo.environment["PATH"] {
            for dir in path.split(separator: ":") {
                candidates.append("\(dir)/openssl")
            }
        }
        return candidates.first { FileManager.default.isExecutableFile(atPath: $0) }
    }

    /// Produce a raw RSA PKCS#1 v1.5 signature over SHA-256 of `data` using
    /// `openssl` and the DER PKCS#8 key at `keyDerPath` — the private key stays
    /// entirely outside RustPdf.
    private static func opensslSign(opensslPath: String, keyDerPath: String, data: [UInt8]) throws -> [UInt8] {
        let proc = Process()
        proc.executableURL = URL(fileURLWithPath: opensslPath)
        proc.arguments = ["dgst", "-sha256", "-sign", keyDerPath, "-keyform", "DER"]
        let stdin = Pipe(), stdout = Pipe()
        proc.standardInput = stdin
        proc.standardOutput = stdout
        try proc.run()
        stdin.fileHandleForWriting.write(Data(data))
        stdin.fileHandleForWriting.closeFile()
        let out = stdout.fileHandleForReading.readDataToEndOfFile()
        proc.waitUntilExit()
        guard proc.terminationStatus == 0, !out.isEmpty else {
            throw PdfError(status: .sign, message: "openssl signing failed (status \(proc.terminationStatus))")
        }
        return [UInt8](out)
    }

    /// A minimal valid 1x1 red RGB PNG (built once with the stdlib).
    private static let tinyPNG: [UInt8] = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d,
        0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xde, 0x00, 0x00, 0x00,
        0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
        0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ]

    /// True if the ASCII `needle` appears in the byte buffer.
    private func contains(_ haystack: [UInt8], _ needle: String) -> Bool {
        let n = Array(needle.utf8)
        guard !n.isEmpty, haystack.count >= n.count else { return false }
        for i in 0...(haystack.count - n.count) where Array(haystack[i..<i+n.count]) == n {
            return true
        }
        return false
    }
}

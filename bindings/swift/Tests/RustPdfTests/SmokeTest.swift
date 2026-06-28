//
//  SmokeTest.swift
//  RustPdfTests
//
//  One test exercises the whole product surface serially (the license is
//  process-global). Mirrors the Go/Java/Delphi smoke tests over the same C ABI.
//

import XCTest
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
        let devLicense = String(decoding: try read(
            root.appendingPathComponent("crates/license/fixtures/dev_license.txt")),
            as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)

        XCTAssertFalse(Pdf.version.isEmpty, "version must be non-empty")

        // 1. Corporate features blocked without a license.
        unsetenv("RUSTPDF_LICENSE")
        unsetenv("RUSTPDF_LICENSE_FILE")
        do {
            let d = try Document()
            try d.pdfa()
            try d.addPage()
            XCTAssertThrowsError(try d.toBytes(), "PDF/A must be blocked without a license")
        }

        // 2. Activate and build a tagged PDF/A-2a document.
        try Pdf.activateLicense(devLicense)
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

        // 11. Factur-X / ZUGFeRD (Tier 2; license-gated, PDF/A-3b).
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

        // 14. Convert an existing (font-embedded) document to PDF/A (license-gated).
        do {
            let ed = try EditableDoc(loading: plain)
            try ed.convertToPdfa(.a2b)
            let out = try ed.toBytes()
            XCTAssertTrue(contains(out, "pdfaid"), "PDF/A identifier missing after conversion")
        }

        // 15. Verify signatures on the freshly-signed document from step 8.
        let reports = try Pdf.verifySignatures(signed)
        XCTAssertGreaterThanOrEqual(reports.count, 1, "expected at least one signature")
        if let r = reports.first {
            XCTAssertEqual(r.byteRange.count, 4, "byteRange should have 4 ints")
            XCTAssertFalse(r.subFilter.isEmpty, "subFilter should be set")
        }
        XCTAssertTrue(try Pdf.verifySignatures(plain).isEmpty, "unsigned doc should report no signatures")
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

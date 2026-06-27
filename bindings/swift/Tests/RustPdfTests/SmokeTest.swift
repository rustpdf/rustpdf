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
    }

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

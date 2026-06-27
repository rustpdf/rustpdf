//
//  main.swift
//  rustpdf-example
//
//  A minimal end-to-end demo of the RustPdf Swift binding: draw vector
//  graphics, embed a font, lay out text, and write a PDF.
//
//  Usage:  swift run rustpdf-example [output.pdf] [font.ttf]
//
//  Build the native library first: `cargo build -p pdf-ffi`, or set
//  RUSTPDF_LIB to the cdylib path.
//

import Foundation
import RustPdf

let args = CommandLine.arguments
let output = args.count > 1 ? args[1] : "rustpdf-swift-example.pdf"
let fontPath = args.count > 2 ? args[2] : nil

print("rust-pdf \(Pdf.version)")

do {
    let doc = try Document()
    try doc.setInfo(DocumentInfo(title: "RustPdf Swift example", author: "rustpdf"))
    try doc.addPage()

    // A filled rectangle.
    try doc.setFillRGB(0.12, 0.45, 0.95)
    try doc.rect(x: 72, y: 690, width: 200, height: 80)
    try doc.fill()

    // Text, if a font is available.
    if let fontPath, FileManager.default.fileExists(atPath: fontPath) {
        let font = try doc.addFont(path: fontPath)
        try doc.setFillRGB(0, 0, 0)
        try doc.showText(font: font, size: 24, x: 72, y: 640, "Hello from Swift")
        try doc.paragraph(font: font, size: 12, x: 72, y: 600, width: 420,
                          text: String(repeating: "The rust-pdf core, bound to Swift. ", count: 6),
                          align: .justify)
    } else {
        print("note: pass a TTF/OTF path as the 2nd argument to render text")
    }

    try doc.save(to: output)
    print("wrote \(doc.pageCount) page(s) to \(output)")
} catch {
    FileHandle.standardError.write(Data("error: \(error)\n".utf8))
    exit(1)
}

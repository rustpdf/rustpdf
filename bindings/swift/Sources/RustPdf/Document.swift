//
//  Document.swift
//  RustPdf
//
//  Idiomatic wrapper for authoring a PDF from scratch: pages, vector graphics,
//  embedded fonts and text, images, PDF/A, tagging, attachments and AcroForm
//  fields.
//

#if canImport(Darwin)
import Darwin
#elseif canImport(Glibc)
import Glibc
#endif
import Foundation

/// The document information dictionary; any field may be `nil`.
public struct DocumentInfo: Sendable {
    public var title: String?
    public var author: String?
    public var subject: String?
    public var keywords: String?
    public var creator: String?

    public init(title: String? = nil, author: String? = nil, subject: String? = nil,
                keywords: String? = nil, creator: String? = nil) {
        self.title = title
        self.author = author
        self.subject = subject
        self.keywords = keywords
        self.creator = creator
    }
}

/// One button of a radio group: its widget rectangle `[x0, y0, x1, y1]` and its
/// `/AP` export value.
public struct RadioButton: Sendable {
    public var rect: (Double, Double, Double, Double)
    public var export: String

    public init(rect: (Double, Double, Double, Double), export: String) {
        self.rect = rect
        self.export = export
    }
}

/// A document-outline entry. Nest entries with ``child(_:)`` to build a tree;
/// ``Document/addBookmark(_:)`` flattens one root tree in pre-order.
public struct Bookmark: Sendable {
    public var title: String
    /// Target page index (0-based).
    public var page: Int
    /// Optional scroll position (top of the view); `nil` keeps the current view.
    public var top: Double?
    public var children: [Bookmark]

    public init(title: String, page: Int, top: Double? = nil, children: [Bookmark] = []) {
        self.title = title
        self.page = page
        self.top = top
        self.children = children
    }

    /// Append a child entry, returning the modified bookmark (builder style).
    public func child(_ bookmark: Bookmark) -> Bookmark {
        var copy = self
        copy.children.append(bookmark)
        return copy
    }

    /// Pre-order flatten into parallel `(level, title, page, top)` tuples.
    func flatten(level: Int, into out: inout [(level: Int, title: String, page: Int, top: Double?)]) {
        out.append((level, title, page, top))
        for c in children { c.flatten(level: level + 1, into: &out) }
    }
}

/// A PDF being authored. Native memory is released by `deinit`; the type is a
/// reference type so the handle is freed exactly once.
public final class Document {
    private var handle: OpaquePointer?

    /// Create a new, empty A4 document.
    public init() throws {
        guard let h = Native.shared.pdf_document_new() else {
            throw PdfError(status: .nullPointer, message: "pdf_document_new returned NULL")
        }
        handle = h
    }

    deinit {
        if let h = handle { Native.shared.pdf_document_free(h) }
    }

    // MARK: - Configuration

    /// Mark the document as PDF/A-2b.
    @discardableResult
    public func pdfa() throws -> Document {
        try check(Native.shared.pdf_document_pdfa(handle)); return self
    }

    /// Mark the document at an explicit PDF/A level. Level-A variants also
    /// enable tagging.
    @discardableResult
    public func pdfa(_ level: PdfaLevel) throws -> Document {
        try check(Native.shared.pdf_document_pdfa_level(handle, level.rawValue)); return self
    }

    /// Enable the tagged / accessible structure tree.
    @discardableResult
    public func tagged() throws -> Document {
        try check(Native.shared.pdf_document_tagged(handle)); return self
    }

    /// Set the PDF version written in the header.
    @discardableResult
    public func setVersion(_ v: PdfVersion) throws -> Document {
        try check(Native.shared.pdf_document_set_version(handle, v.rawValue)); return self
    }

    /// Set the default page size (points) for subsequently added pages.
    @discardableResult
    public func setDefaultSize(width: Double, height: Double) throws -> Document {
        try check(Native.shared.pdf_document_set_default_size(handle, width, height)); return self
    }

    /// Set the document info dictionary.
    @discardableResult
    public func setInfo(_ info: DocumentInfo) throws -> Document {
        let t = dupCString(info.title), a = dupCString(info.author)
        let s = dupCString(info.subject), k = dupCString(info.keywords)
        let c = dupCString(info.creator)
        defer { free(t); free(a); free(s); free(k); free(c) }
        try check(Native.shared.pdf_document_set_info(
            handle, UnsafePointer(t), UnsafePointer(a), UnsafePointer(s),
            UnsafePointer(k), UnsafePointer(c)))
        return self
    }

    // MARK: - Pages + graphics

    /// The number of pages in the document.
    public var pageCount: Int { Int(Native.shared.pdf_document_page_count(handle)) }

    /// Append a page using the document's default size (A4).
    @discardableResult
    public func addPage() throws -> Document {
        try check(Native.shared.pdf_document_add_page(handle)); return self
    }

    /// Append a page of an explicit size (points).
    @discardableResult
    public func addPage(width: Double, height: Double) throws -> Document {
        try check(Native.shared.pdf_document_add_page_sized(handle, width, height)); return self
    }

    /// Set the fill color (DeviceRGB, components in `0...1`) on the current page.
    @discardableResult
    public func setFillRGB(_ r: Double, _ g: Double, _ b: Double) throws -> Document {
        try check(Native.shared.pdf_page_set_fill_rgb(handle, r, g, b)); return self
    }

    /// Set the stroke color (DeviceRGB) on the current page.
    @discardableResult
    public func setStrokeRGB(_ r: Double, _ g: Double, _ b: Double) throws -> Document {
        try check(Native.shared.pdf_page_set_stroke_rgb(handle, r, g, b)); return self
    }

    /// Set the line width on the current page.
    @discardableResult
    public func setLineWidth(_ width: Double) throws -> Document {
        try check(Native.shared.pdf_page_set_line_width(handle, width)); return self
    }

    /// Append a rectangle subpath on the current page.
    @discardableResult
    public func rect(x: Double, y: Double, width: Double, height: Double) throws -> Document {
        try check(Native.shared.pdf_page_rect(handle, x, y, width, height)); return self
    }

    /// Fill the current path (nonzero winding) on the current page.
    @discardableResult
    public func fill() throws -> Document {
        try check(Native.shared.pdf_page_fill(handle)); return self
    }

    /// Stroke the current path on the current page.
    @discardableResult
    public func stroke() throws -> Document {
        try check(Native.shared.pdf_page_stroke(handle)); return self
    }

    // MARK: - Fonts + text

    /// Register a TrueType/OpenType font from a file path; returns its font id.
    public func addFont(path: String) throws -> Int32 {
        var id: Int32 = 0
        try path.withCString { try check(Native.shared.pdf_document_add_font_file(handle, $0, &id)) }
        return id
    }

    /// Register a font from TrueType/OpenType bytes; returns its font id.
    public func addFont(data: [UInt8]) throws -> Int32 {
        var id: Int32 = 0
        try withBytes(data) { ptr, len in
            try check(Native.shared.pdf_document_add_font(handle, ptr, len, &id))
        }
        return id
    }

    /// Show a line of text at `(x, y)` (baseline) in `font`/`size` on the
    /// current page. `headingLevel` 1...6 tags it as `H1`...`H6` (when the
    /// document is tagged); 0 leaves it as a paragraph.
    @discardableResult
    public func showText(font: Int32, size: Double, x: Double, y: Double,
                         _ text: String, headingLevel: Int32 = 0) throws -> Document {
        try text.withCString {
            try check(Native.shared.pdf_page_show_text(handle, font, size, x, y, $0, headingLevel))
        }
        return self
    }

    /// Lay out a wrapping paragraph in the box `(x, y, width)` (y = first
    /// baseline) with the given alignment on the current page.
    @discardableResult
    public func paragraph(font: Int32, size: Double, x: Double, y: Double, width: Double,
                          text: String, align: Align = .left) throws -> Document {
        try text.withCString {
            try check(Native.shared.pdf_page_paragraph(
                handle, font, size, x, y, width, align.rawValue, $0))
        }
        return self
    }

    // MARK: - Images

    /// Register an image (JPEG or PNG, by signature) from a file; returns its id.
    public func addImage(path: String) throws -> Int32 {
        var id: Int32 = 0
        try path.withCString { try check(Native.shared.pdf_document_add_image_file(handle, $0, &id)) }
        return id
    }

    /// Register a PNG image from bytes; returns its id.
    public func addImagePNG(data: [UInt8]) throws -> Int32 {
        var id: Int32 = 0
        try withBytes(data) { ptr, len in
            try check(Native.shared.pdf_document_add_image_png(handle, ptr, len, &id))
        }
        return id
    }

    /// Register a JPEG image from bytes; returns its id.
    public func addImageJPEG(data: [UInt8]) throws -> Int32 {
        var id: Int32 = 0
        try withBytes(data) { ptr, len in
            try check(Native.shared.pdf_document_add_image_jpeg(handle, ptr, len, &id))
        }
        return id
    }

    /// Draw a (decorative) image in `(x, y, w, h)` on the current page.
    @discardableResult
    public func drawImage(_ image: Int32, x: Double, y: Double, width: Double, height: Double) throws -> Document {
        try check(Native.shared.pdf_page_draw_image(handle, image, x, y, width, height)); return self
    }

    /// Draw a meaningful image (tagged `/Figure` with alternate text `alt`).
    @discardableResult
    public func figure(_ image: Int32, x: Double, y: Double, width: Double, height: Double,
                       alt: String) throws -> Document {
        try alt.withCString {
            try check(Native.shared.pdf_page_figure(handle, image, x, y, width, height, $0))
        }
        return self
    }

    // MARK: - Attachments + forms

    /// Embed an associated file (PDF/A-3).
    @discardableResult
    public func attachFile(name: String, mime: String, data: [UInt8],
                           relationship: AFRelationship, description: String = "") throws -> Document {
        let n = dupCString(name), m = dupCString(mime), d = dupCString(description)
        defer { free(n); free(m); free(d) }
        try withBytes(data) { ptr, len in
            try check(Native.shared.pdf_document_attach_file(
                handle, UnsafePointer(n), UnsafePointer(m), ptr, len,
                relationship.rawValue, UnsafePointer(d)))
        }
        return self
    }

    /// Add an AcroForm text field. `size` 0 = auto.
    @discardableResult
    public func textField(name: String, page: Int, rect: (Double, Double, Double, Double),
                          value: String = "", size: Double = 0) throws -> Document {
        let n = dupCString(name), v = dupCString(value)
        defer { free(n); free(v) }
        try check(Native.shared.pdf_document_text_field(
            handle, UnsafePointer(n), UInt(page),
            rect.0, rect.1, rect.2, rect.3, UnsafePointer(v), size))
        return self
    }

    /// Add an AcroForm checkbox.
    @discardableResult
    public func checkbox(name: String, page: Int, rect: (Double, Double, Double, Double),
                         checked: Bool) throws -> Document {
        try name.withCString {
            try check(Native.shared.pdf_document_checkbox(
                handle, $0, UInt(page), rect.0, rect.1, rect.2, rect.3, checked ? 1 : 0))
        }
        return self
    }

    /// Add an AcroForm dropdown (choice) field. `selected` is the 0-based index
    /// or -1 for none.
    @discardableResult
    public func dropdown(name: String, page: Int, rect: (Double, Double, Double, Double),
                         options: [String], selected: Int = -1, size: Double = 0) throws -> Document {
        let n = dupCString(name), o = dupCString(options.joined(separator: "\n"))
        defer { free(n); free(o) }
        try check(Native.shared.pdf_document_dropdown(
            handle, UnsafePointer(n), UInt(page),
            rect.0, rect.1, rect.2, rect.3, UnsafePointer(o), Int32(selected), size))
        return self
    }

    /// Add an AcroForm radio-button group. `selected` is the 0-based index or -1.
    @discardableResult
    public func radioGroup(name: String, page: Int, buttons: [RadioButton],
                           selected: Int = -1) throws -> Document {
        let n = dupCString(name)
        defer { free(n) }

        var rects = [Double]()
        rects.reserveCapacity(buttons.count * 4)
        for b in buttons {
            rects.append(contentsOf: [b.rect.0, b.rect.1, b.rect.2, b.rect.3])
        }
        // Duplicate each export value; free them all afterwards.
        let exports: [UnsafeMutablePointer<CChar>?] = buttons.map { dupCString($0.export) }
        defer { for e in exports { free(e) } }

        try rects.withUnsafeBufferPointer { rp in
            let constExports: [UnsafePointer<CChar>?] = exports.map { UnsafePointer($0) }
            try constExports.withUnsafeBufferPointer { ep in
                try check(Native.shared.pdf_document_radio_group(
                    handle, UnsafePointer(n), UInt(page), UInt(buttons.count),
                    rp.baseAddress, ep.baseAddress, Int32(selected)))
            }
        }
        return self
    }

    // MARK: - Links + bookmarks + Factur-X

    /// Add a clickable web link over `rect` `(x0, y0, x1, y1)` opening `uri` on
    /// the current page.
    @discardableResult
    public func linkURI(rect: (Double, Double, Double, Double), uri: String) throws -> Document {
        try uri.withCString {
            try check(Native.shared.pdf_page_link_uri(handle, rect.0, rect.1, rect.2, rect.3, $0))
        }
        return self
    }

    /// Add an internal link over `rect` jumping to `pageIndex` (0-based) on the
    /// current page. `top` (when non-`nil`) scrolls so that y-coordinate is at
    /// the top of the view.
    @discardableResult
    public func linkToPage(rect: (Double, Double, Double, Double), pageIndex: Int,
                           top: Double? = nil) throws -> Document {
        try check(Native.shared.pdf_page_link_to_page(
            handle, rect.0, rect.1, rect.2, rect.3, UInt(pageIndex),
            top ?? 0.0, top == nil ? 0 : 1))
        return self
    }

    /// Append one outline tree (pre-order flattened) to the document outline.
    @discardableResult
    public func addBookmark(_ bookmark: Bookmark) throws -> Document {
        var entries: [(level: Int, title: String, page: Int, top: Double?)] = []
        bookmark.flatten(level: 0, into: &entries)
        let n = entries.count

        let levels = entries.map { Int32($0.level) }
        let pages = entries.map { UInt($0.page) }
        let tops = entries.map { $0.top ?? 0.0 }
        let hasTops = entries.map { $0.top == nil ? Int32(0) : Int32(1) }
        // Duplicate every title; free them all afterwards.
        let titles: [UnsafeMutablePointer<CChar>?] = entries.map { dupCString($0.title) }
        defer { for t in titles { free(t) } }

        try levels.withUnsafeBufferPointer { lp in
            try pages.withUnsafeBufferPointer { pp in
                try tops.withUnsafeBufferPointer { tp in
                    try hasTops.withUnsafeBufferPointer { hp in
                        let constTitles: [UnsafePointer<CChar>?] = titles.map { UnsafePointer($0) }
                        try constTitles.withUnsafeBufferPointer { ttp in
                            try check(Native.shared.pdf_document_add_bookmarks(
                                handle, UInt(n), lp.baseAddress, ttp.baseAddress,
                                pp.baseAddress, tp.baseAddress, hp.baseAddress))
                        }
                    }
                }
            }
        }
        return self
    }

    /// Make this a ZUGFeRD / Factur-X invoice: embed `xml` as `factur-x.xml`,
    /// mark it PDF/A-3b, and add the Factur-X XMP at `profile`.
    @discardableResult
    public func facturx(_ xml: [UInt8], profile: FacturxProfile = .en16931) throws -> Document {
        try withBytes(xml) { ptr, len in
            try check(Native.shared.pdf_document_facturx(handle, ptr, len, profile.rawValue))
        }
        return self
    }

    // MARK: - Output

    /// Serialize the document to bytes.
    public func toBytes() throws -> [UInt8] {
        try takeBytes { out, len in Native.shared.pdf_document_write(handle, out, len) }
    }

    /// Serialize the document and write it to `path`.
    public func save(to path: String) throws {
        try path.withCString { try check(Native.shared.pdf_document_save(handle, $0)) }
    }
}

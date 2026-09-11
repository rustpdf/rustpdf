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

    /// Enable encryption on save. `user`/`owner` are the
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

    // MARK: - Forms

    /// Check or uncheck a checkbox field by name; returns whether it existed.
    @discardableResult
    public func setCheckbox(name: String, checked: Bool = true) throws -> Bool {
        var found: Int32 = 0
        try name.withCString { n in
            try check(Native.shared.pdf_editable_set_checkbox(handle, n, checked ? 1 : 0, &found))
        }
        return found != 0
    }

    /// Select a radio button by its export value; returns whether it existed.
    @discardableResult
    public func setRadio(name: String, exportValue: String) throws -> Bool {
        var found: Int32 = 0
        try name.withCString { n in
            try exportValue.withCString { v in
                try check(Native.shared.pdf_editable_set_radio(handle, n, v, &found))
            }
        }
        return found != 0
    }

    /// Set a choice (dropdown/list) field value; returns whether it existed.
    @discardableResult
    public func setChoice(name: String, value: String) throws -> Bool {
        var found: Int32 = 0
        try name.withCString { n in
            try value.withCString { v in
                try check(Native.shared.pdf_editable_set_choice(handle, n, v, &found))
            }
        }
        return found != 0
    }

    /// Flatten all interactive form fields into static page content.
    @discardableResult
    public func flattenForms() throws -> EditableDoc {
        try check(Native.shared.pdf_editable_flatten_forms(handle)); return self
    }

    /// The document's terminal field names (empty entries dropped).
    public func fieldNames() throws -> [String] {
        let bytes = try takeBytes { out, len in Native.shared.pdf_editable_field_names(handle, out, len) }
        return String(decoding: bytes, as: UTF8.self).split(separator: "\n").map(String.init)
    }

    // MARK: - Watermarks + redaction

    /// Stamp a diagonal text watermark (standard Helvetica) across every page.
    /// `opaqueBackground` draws a filled rectangle behind the text (for a stamp
    /// effect) instead of a transparent overlay.
    @discardableResult
    public func watermarkText(_ text: String, size: Double = 64.0,
                              color: (Double, Double, Double) = (0.5, 0.5, 0.5),
                              opacity: Double = 0.30, rotationDeg: Double = 45.0,
                              opaqueBackground: Bool = false) throws -> EditableDoc {
        try text.withCString {
            try check(Native.shared.pdf_editable_watermark_text(
                handle, $0, size, color.0, color.1, color.2, opacity, rotationDeg,
                opaqueBackground ? 1 : 0))
        }
        return self
    }

    /// Stamp an image (from a JPEG/PNG file at `path`) centered on every page,
    /// rotated `rotationDeg` degrees.
    @discardableResult
    public func watermarkImageFile(path: String, width: Double, height: Double,
                                   opacity: Double = 0.30, rotationDeg: Double = 0.0) throws -> EditableDoc {
        try path.withCString {
            try check(Native.shared.pdf_editable_watermark_image_file(
                handle, $0, width, height, opacity, rotationDeg))
        }
        return self
    }

    /// Redact rectangular regions `[(x0, y0, x1, y1), ...]` on page `index`;
    /// returns whether the page existed.
    @discardableResult
    public func redact(_ index: Int, rects: [(Double, Double, Double, Double)]) throws -> Bool {
        var flat = [Double]()
        flat.reserveCapacity(rects.count * 4)
        for r in rects { flat.append(contentsOf: [r.0, r.1, r.2, r.3]) }
        var found: Int32 = 0
        try flat.withUnsafeBufferPointer {
            try check(Native.shared.pdf_editable_redact(
                handle, UInt(index), $0.baseAddress, UInt(rects.count), &found))
        }
        return found != 0
    }

    // MARK: - Positioned drawing primitives

    /// Paint a filled rectangle at `(x, y)` sized `width`×`height` on page
    /// `pageIndex` (0-based), in RGB `color` (each 0..=1, default opaque white)
    /// at `opacity` (0..=1). Coordinates are in the page's **visible** space
    /// (origin lower-left, y up), regardless of the page's `/Rotate`. Returns
    /// whether the page existed. The common use is masking a placeholder with an
    /// opaque white box.
    @discardableResult
    public func fillRect(_ pageIndex: Int, _ x: Double, _ y: Double, _ width: Double, _ height: Double,
                         color: (Double, Double, Double) = (1, 1, 1), opacity: Double = 1.0) -> Bool {
        var found: Int32 = 0
        try? check(Native.shared.pdf_editable_fill_rect(
            handle, Int32(pageIndex), x, y, width, height,
            color.0, color.1, color.2, opacity, &found))
        return found != 0
    }

    /// Register a TrueType/OpenType font (from a file at `path`) for text
    /// stamping; returns a `fontId` usable with the `fontId` parameter of
    /// ``placeText``/``maskedText``/``placeParagraph``. The font is embedded as
    /// a subset — stamped text renders with the real font's glyphs and metrics,
    /// exactly like ``Document/addFont(file:)`` + `showText`.
    public func addFontFile(_ path: String) throws -> Int {
        var id: Int32 = -1
        try path.withCString { p in
            try check(Native.shared.pdf_editable_add_font_file(handle, p, &id))
        }
        return Int(id)
    }

    /// Register a stamping font from raw TrueType/OpenType bytes. See
    /// ``addFontFile(_:)``.
    public func addFont(_ data: [UInt8]) throws -> Int {
        var id: Int32 = -1
        try withBytes(data) { ptr, len in
            try check(Native.shared.pdf_editable_add_font(handle, ptr, len, &id))
        }
        return Int(id)
    }

    /// Choose the coordinate space of the positioned stamping primitives
    /// (``fillRect``, ``placeText``, ``maskedText``, ``placeParagraph``,
    /// ``drawImage``) for subsequent calls. ``StampSpace/visible`` (default)
    /// keeps the historical behavior — coordinates in the page's displayed
    /// space, compensating `/Rotate`. ``StampSpace/media`` interprets
    /// coordinates and `rotationDeg` in the raw PDF user space (legacy layout engines
    /// semantics), never composing with the page's `/Rotate` — use it to
    /// reproduce legacy-engine placement on rotated/scanned pages. Watermarks and
    /// redaction are unaffected.
    @discardableResult
    public func setStampSpace(_ space: StampSpace) throws -> EditableDoc {
        try check(Native.shared.pdf_editable_set_stamp_space(handle, space.rawValue)); return self
    }

    /// Draw a line of positioned text anchored at `(x, y)` on page `pageIndex`
    /// (0-based) at `size` points in RGB `color` (each 0..=1, default black).
    /// `rotationDeg` rotates the text counter-clockwise about its anchor
    /// `(x, y)` (match the page rotation to follow a rotated page). `align`
    /// shifts the start point along the baseline so the anchor `(x, y)` is the
    /// text's left (default), right or center. Pass `fontId` from
    /// ``addFontFile(_:)``/``addFont(_:)`` to stamp with an embedded
    /// TrueType/OpenType font; `-1` (default) uses the built-in Helvetica
    /// (keep the text WinAnsi / Latin-1). `anchor` says what `y` means:
    /// ``VerticalAnchor/baseline`` (default), ``VerticalAnchor/top`` (baseline
    /// lands `ascent × size` below `y`, legacy fixed-position layout),
    /// ``VerticalAnchor/bottom`` (descender line rests on `y`), or the legacy layout engines
    /// line-box variants ``VerticalAnchor/lineTop``/``VerticalAnchor/lineBottom``.
    /// Coordinates are in the page's **visible** space (origin lower-left, y
    /// up), regardless of the page's `/Rotate` (see ``setStampSpace(_:)``).
    /// Returns whether the page (and font) existed.
    @discardableResult
    public func placeText(_ pageIndex: Int, _ x: Double, _ y: Double, _ text: String,
                          size: Double = 12, color: (Double, Double, Double) = (0, 0, 0),
                          rotationDeg: Double = 0.0, align: Align = .left,
                          fontId: Int = -1, anchor: VerticalAnchor = .baseline) -> Bool {
        var found: Int32 = 0
        text.withCString { t in
            try? check(Native.shared.pdf_editable_place_text_anchored(
                handle, Int32(pageIndex), x, y, t, size,
                color.0, color.1, color.2, rotationDeg, align.rawValue,
                anchor.rawValue, Int32(fontId), &found))
        }
        return found != 0
    }

    /// Stamp a **paragraph with automatic word wrapping** on page `pageIndex`:
    /// `text` is broken into lines that fit `width` points (greedy, by word;
    /// `\n` forces a break) and drawn from `(x, y)` per `anchor`:
    /// ``VerticalAnchor/top`` (default) — `y` is the top of the block, the
    /// first baseline lands `ascent × size` below it (legacy layout engines
    /// `fixed-position layout`); ``VerticalAnchor/baseline`` — `y` is the first
    /// line's baseline; ``VerticalAnchor/bottom``/``VerticalAnchor/lineBottom``
    /// — **bottom-pinned**: the block's bottom rests on `y` and grows upward
    /// (with `maxHeight` as a ceiling that cuts overflowing lines from the
    /// top). `align` lays lines out inside `[x, x+width]` (`.justify`
    /// stretches the word gaps of every line but the last of each paragraph).
    /// `fontId` from ``addFontFile(_:)``/``addFont(_:)`` wraps and draws with
    /// that embedded font (its real metrics drive the break points); `-1` uses
    /// the built-in Helvetica. `maxHeight` (`nil` = unlimited) truncates lines
    /// that would overflow; `lineHeight` scales the default `1.2 × size`
    /// leading. `rotationDeg` rotates the laid-out block counter-clockwise
    /// about the anchor `(x, y)`. Returns whether the page (and font) existed
    /// and the box was valid.
    @discardableResult
    public func placeParagraph(_ pageIndex: Int, _ x: Double, _ y: Double, _ width: Double,
                               _ text: String, size: Double = 12,
                               color: (Double, Double, Double) = (0, 0, 0),
                               align: Align = .left, fontId: Int = -1,
                               maxHeight: Double? = nil, lineHeight: Double = 1.0,
                               anchor: VerticalAnchor = .top, rotationDeg: Double = 0.0) -> Bool {
        var found: Int32 = 0
        text.withCString { t in
            try? check(Native.shared.pdf_editable_place_paragraph_anchored(
                handle, Int32(pageIndex), x, y, width, t, size,
                color.0, color.1, color.2, align.rawValue, anchor.rawValue,
                Int32(fontId), maxHeight ?? 0.0, lineHeight, rotationDeg,
                nil, nil, &found))
        }
        return found != 0
    }

    /// Like ``placeParagraph(_:_:_:_:_:size:color:align:fontId:maxHeight:lineHeight:anchor:rotationDeg:)``
    /// but returns both the number of **lines drawn** (0 when the page/font was
    /// invalid or nothing fit — useful to detect `maxHeight` truncation) and
    /// the **consumed block height** in points (top of the first drawn line's
    /// box to the bottom of the last one's) — stack blocks without
    /// re-measuring.
    @discardableResult
    public func placeParagraphMeasured(_ pageIndex: Int, _ x: Double, _ y: Double, _ width: Double,
                                       _ text: String, size: Double = 12,
                                       color: (Double, Double, Double) = (0, 0, 0),
                                       align: Align = .left, fontId: Int = -1,
                                       maxHeight: Double? = nil, lineHeight: Double = 1.0,
                                       anchor: VerticalAnchor = .top,
                                       rotationDeg: Double = 0.0) -> (lines: Int, height: Double) {
        var found: Int32 = 0
        var lines: Int32 = 0
        var height: Double = 0
        text.withCString { t in
            try? check(Native.shared.pdf_editable_place_paragraph_anchored(
                handle, Int32(pageIndex), x, y, width, t, size,
                color.0, color.1, color.2, align.rawValue, anchor.rawValue,
                Int32(fontId), maxHeight ?? 0.0, lineHeight, rotationDeg,
                &height, &lines, &found))
        }
        return (Int(lines), height)
    }

    /// Draw `text` over an opaque background box `[x, y, x+width, y+height]`:
    /// fills the box in `bgColor` (default white), then writes the text at
    /// `size` points in `textColor` (default black) horizontally aligned per
    /// `align` and vertically laid out per `valign`:
    /// ``VerticalAlign/middle`` (default, historical cap-height centering),
    /// ``VerticalAlign/top`` (line hangs from the top edge — baseline at
    /// `y + height − ascent × size`, top line-alignment in rectangle-based text APIs), or
    /// ``VerticalAlign/bottom`` (descender line rests on the bottom edge).
    /// `fontId` from ``addFontFile(_:)``/``addFont(_:)`` stamps with an
    /// embedded font; `-1` (default) uses the built-in Helvetica. `padding` is
    /// the horizontal edge inset (points) for `.left`/`.right` alignment: the
    /// text starts at `x + padding` (or ends at `x + width − padding`); `nil`
    /// keeps the historical `min(0.15 × size, width / 4)`, `0` starts flush
    /// with the box edge (rectangle-based DrawString semantics). The classic use
    /// is masking a placeholder and stamping the real value over it without
    /// hand-computing the baseline. Coordinates are in the page's **visible**
    /// space (origin lower-left, y up). Returns whether the page (and font)
    /// existed.
    @discardableResult
    public func maskedText(_ pageIndex: Int, _ x: Double, _ y: Double, _ width: Double, _ height: Double,
                           _ text: String, size: Double = 12,
                           textColor: (Double, Double, Double) = (0, 0, 0),
                           bgColor: (Double, Double, Double) = (1, 1, 1),
                           align: Align = .left, fontId: Int = -1,
                           valign: VerticalAlign = .middle, padding: Double? = nil) -> Bool {
        var found: Int32 = 0
        text.withCString { t in
            try? check(Native.shared.pdf_editable_masked_text_pad(
                handle, Int32(pageIndex), x, y, width, height, t, size,
                textColor.0, textColor.1, textColor.2,
                bgColor.0, bgColor.1, bgColor.2, align.rawValue,
                valign.rawValue, padding ?? -1.0, Int32(fontId), &found))
        }
        return found != 0
    }

    /// Draw an image (PNG or JPEG `image` bytes, dispatched on the file
    /// signature) on page `index` (0-based) at `(x, y)`, scaled to
    /// `width`×`height` points and rotated `rotationDeg` degrees
    /// counter-clockwise. `anchor` controls how a rotated image is anchored:
    /// ``ImageAnchor/corner`` (default) rotates the image about its own
    /// lower-left corner at `(x, y)`; ``ImageAnchor/boundingBox`` lands the
    /// rotated image's bounding box with its lower-left at `(x, y)` (legacy layout engines
    /// semantics — e.g. a 90° image occupies `[x, x+height] × [y, y+width]`).
    /// Coordinates are in the page's **visible** space (origin lower-left,
    /// y up), honoring the page's `/Rotate`. Returns whether the page existed.
    @discardableResult
    public func drawImage(_ index: Int, image: [UInt8], x: Double, y: Double,
                          width: Double, height: Double, rotationDeg: Double = 0.0,
                          anchor: ImageAnchor = .corner) -> Bool {
        var found: Int32 = 0
        withBytes(image) { ptr, len in
            try? check(Native.shared.pdf_editable_draw_image_anchored(
                handle, Int32(index), ptr, len, x, y, width, height, rotationDeg,
                anchor.rawValue, &found))
        }
        return found != 0
    }

    /// Convert the loaded document to PDF/A at `level` (B-levels only: A-1b,
    /// A-2b, A-3b).
    @discardableResult
    public func convertToPdfa(_ level: PdfaLevel = .a2b) throws -> EditableDoc {
        try check(Native.shared.pdf_editable_convert_to_pdfa(handle, level.rawValue)); return self
    }

    // MARK: - Normalization

    /// Set the output PDF version (downgrade/normalize). Clears any catalog
    /// `/Version` override.
    @discardableResult
    public func setVersion(_ version: PdfVersion) throws -> EditableDoc {
        try check(Native.shared.pdf_editable_set_version(handle, version.rawValue)); return self
    }

    /// Strip PDF/A conformance (`/OutputIntents`, XMP `pdfaid`, `/Version`) so
    /// the file is a plain PDF.
    @discardableResult
    public func stripPdfa() throws -> EditableDoc {
        try check(Native.shared.pdf_editable_strip_pdfa(handle)); return self
    }

    /// Normalize to a plain PDF at `version` (strip PDF/A + set the version).
    @discardableResult
    public func normalize(_ version: PdfVersion = .v17) throws -> EditableDoc {
        try check(Native.shared.pdf_editable_normalize(handle, version.rawValue)); return self
    }

    // MARK: - Output

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

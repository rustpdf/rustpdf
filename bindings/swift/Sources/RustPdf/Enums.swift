//
//  Enums.swift
//  RustPdf
//
//  Strongly-typed enums for the small-integer arguments the C ABI uses.
//

/// PDF/A conformance level (argument to ``Document/pdfa(_:)``).
public enum PdfaLevel: Int32, Sendable {
    case a1b = 0
    case a2b = 1
    case a2a = 2
    case a3b = 3
    case a3a = 4
    // PDF/A-4 (ISO 19005-4), based on PDF 2.0.
    case a4 = 5
    case a4e = 6
    case a4f = 7
}

/// Paragraph horizontal alignment (argument to ``Document/paragraph(font:size:x:y:width:text:align:)``).
public enum Align: Int32, Sendable {
    case left    = 0
    case right   = 1
    case center  = 2
    case justify = 3
}

/// Embedded-file relationship for PDF/A-3 attachments
/// (`/AFRelationship`; argument to ``Document/attachFile(name:mime:data:relationship:description:)``).
public enum AFRelationship: Int32, Sendable {
    case source      = 0
    case data        = 1
    case alternative = 2
    case supplement  = 3
    case unspecified = 4
}

/// Document encryption cipher (argument to ``EditableDoc/encrypt(method:user:owner:readOnly:)``).
public enum Encryption: Int32, Sendable {
    case rc4    = 0
    case aes128 = 1
    case aes256 = 2
}

/// ZUGFeRD / Factur-X invoice profile (argument to ``Document/facturx(_:profile:)``).
public enum FacturxProfile: Int32, Sendable {
    case minimum  = 0
    case basicWL  = 1
    case basic    = 2
    case en16931  = 3
    case extended = 4
}

/// DocMDP certification level applied by the first (certifying) signature
/// (``SigningOptions/certify``).
public enum Certify: Int32, Sendable {
    /// Not a certifying signature.
    case none = 0
    /// `/P 1` — no changes permitted after signing.
    case locked = 1
    /// `/P 2` — form-filling and signing permitted.
    case forms = 2
    /// `/P 3` — form-filling, signing and annotations permitted.
    case formsAndAnnotations = 3
}

/// What the `y` coordinate of a positioned text stamp means
/// (``EditableDoc/placeText(_:_:_:_:size:color:rotationDeg:align:fontId:anchor:)``
/// and the block anchor of ``EditableDoc/placeParagraph(_:_:_:_:_:size:color:align:fontId:maxHeight:lineHeight:anchor:rotationDeg:)``).
/// `baseline` is the historical default for `placeText`; `top` hangs the text
/// from `y` (baseline at `y − ascent × size`, legacy fixed-position layout
/// semantics); `bottom` rests the descender line on `y`. The `line*` cases use
/// the **layout line box** (OS/2 win metrics — or typo × 1.2 — plus the legacy engine's
/// default half-leading of 0.21 em) instead of the raw ascent/descent, matching
/// legacy layout engines line placement exactly. Ascent/descent come from the selected font
/// (embedded font metrics, or Helvetica AFM).
public enum VerticalAnchor: Int32, Sendable {
    case baseline   = 0
    case top        = 1
    case bottom     = 2
    /// Top of the layout line box.
    case lineTop    = 3
    /// Bottom of the layout line box.
    case lineBottom = 4
}

/// Vertical alignment of the text line inside a
/// ``EditableDoc/maskedText(_:_:_:_:_:_:size:textColor:bgColor:align:fontId:valign:padding:)``
/// box. `middle` (the historical default) centers the cap-height block; `top`
/// hangs the line from the top edge (baseline at `y + height − ascent × size`,
/// top line-alignment semantics of rectangle-based text APIs); `bottom` rests the descender
/// line on the bottom edge.
public enum VerticalAlign: Int32, Sendable {
    case top    = 0
    case middle = 1
    case bottom = 2
}

/// Coordinate space of the positioned stamping primitives (`fillRect`,
/// `placeText`, `maskedText`, `placeParagraph`, `drawImage`) — set via
/// ``EditableDoc/setStampSpace(_:)``. `visible` (historical default):
/// coordinates in the page's displayed space, compensating `/Rotate` so a
/// `rotationDeg = 0` stamp reads upright on screen. `media`: raw PDF user
/// space (legacy fixed-position layout semantics) — no
/// composition with the page's `/Rotate` or crop offset; `rotationDeg` is the
/// baseline angle in media space. Watermarks and redaction are unaffected.
public enum StampSpace: Int32, Sendable {
    case visible = 0
    case media   = 1
}

/// How a rotated image is anchored at `(x, y)`
/// (``EditableDoc/drawImage(_:image:x:y:width:height:rotationDeg:anchor:)``).
/// `corner` (default): the image's own lower-left corner — the image sweeps
/// around it when rotated. `boundingBox`: the rotated image's bounding box
/// lands with its lower-left at `(x, y)` (bounding-box layout semantics — pixels
/// always at/above/right of the anchor).
public enum ImageAnchor: Int32, Sendable {
    case corner      = 0
    case boundingBox = 1
}

/// The PDF version written in the header (argument to ``Document/setVersion(_:)``).
public enum PdfVersion: Int32, Sendable {
    case v14 = 0
    case v15 = 1
    case v17 = 2
    case v20 = 3
}

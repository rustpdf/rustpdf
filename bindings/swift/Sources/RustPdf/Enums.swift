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

/// The PDF version written in the header (argument to ``Document/setVersion(_:)``).
public enum PdfVersion: Int32, Sendable {
    case v14 = 0
    case v15 = 1
    case v17 = 2
}

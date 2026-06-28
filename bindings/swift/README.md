# RustPdf (Swift binding)

Idiomatic Swift binding for the `rust-pdf` core over its C ABI (`libpdf_ffi`).
It covers the whole product surface: vector graphics, embedded/subsetted fonts
and text, wrapping paragraphs, images, **PDF/A** (levels 1b–3a),
**tagged/accessible** output, embedded-file attachments, **AcroForm** fields,
manipulation (merge/split/rotate/optimize/incremental update), **text
extraction**, **page rendering** (page to PNG image), **encryption** (RC4 /
AES-128 / AES-256) and **digital signatures** (PKCS#7 / PAdES) — plus **feature
licensing**.

The native library is **linked**, not loaded at run time: the C declarations
come from the `CRustPdf` clang module (a vendored copy of `include/pdf.h`), and
the symbols resolve at link time. In the repo it links the dev build tree
(`target/{debug,release}`); when distributed, it links a **static**
`libpdf_ffi.a` carried inside an `.xcframework`, so the binding works inside an
iOS app bundle with nothing to ship alongside. Builds with SwiftPM on
macOS 11+ and iOS 13+ (Swift 5.9+).

## Layout

`Sources/RustPdf/`

* `Native.swift` — the raw FFI surface: one typed reference per linked C export,
  mirroring `include/pdf.h` one-to-one.
* `Status.swift` — `PdfStatus` and the thrown `PdfError`.
* `Enums.swift` — `PdfaLevel`, `Align`, `AFRelationship`, `Encryption`, `PdfVersion`.
* `Helpers.swift` — out-buffer copy/free and C-string helpers.
* `Pdf.swift` — package-level entry points (`Pdf.version`, `activateLicense`,
  `extractText`, `sign`, `timestamp`, `addDss`) and `SignOptions`.
* `Document.swift` — the `Document` authoring type.
* `EditableDoc.swift` — the `EditableDoc` manipulation type.

`Sources/CRustPdf/` — the C ABI module (vendored header + module map).
`Sources/Example/main.swift` — a runnable demo. `Tests/RustPdfTests/` — the
full-surface smoke test.

## Building (in the repo)

Build the native library first, then build/test the package:

```sh
cargo build -p pdf-ffi
cd bindings/swift
swift test          # full-surface smoke test (links target/debug)
swift run rustpdf-example out.pdf ../../assets/fonts/Roboto-Regular.ttf
```

The in-repo `Package.swift` links `<repo>/target/{debug,release}/libpdf_ffi.*`.

## Distribution (to consumers)

`make swift-dist` (`scripts/package.sh`) builds the static library for every
buildable Apple target, assembles a `RustPdfFFI.xcframework` (macOS universal +
iOS device + iOS simulator slices) and stages a self-contained package under
`bindings/swift/dist/`:

* `dist/RustPdf/` — the consumable package: `Sources/RustPdf/*.swift` +
  `RustPdfFFI.xcframework` + a `Package.swift` that uses a `.binaryTarget`.
  Zipped as `rustpdf-swift-<version>.zip`.
* `dist/RustPdfFFI-<version>.xcframework.zip` + `.checksum` — the standalone
  framework for URL-hosted distribution.

Customers consume it one of two ways:

**A. Local package** (unzip `rustpdf-swift-<version>.zip`):

```swift
dependencies: [.package(path: "path/to/RustPdf")],
targets: [.executableTarget(name: "MyApp",
    dependencies: [.product(name: "RustPdf", package: "RustPdf")])]
```

**B. URL-hosted xcframework** (host the zip yourself, e.g. rustpdf.dev):

```swift
// In a thin wrapper package, or compose directly:
.binaryTarget(
    name: "CRustPdf",
    url: "https://rustpdf.dev/downloads/RustPdfFFI-0.1.0.xcframework.zip",
    checksum: "<contents of the .checksum file>")
```

The static Rust library's only non-system dependency is `libiconv`, which the
generated package links for you (`.linkedLibrary("iconv")`). To add tvOS /
visionOS / Mac Catalyst slices, install those Rust std targets
(`rustup target add …`) before running `make swift-dist`.

## Using it from another package

Add this directory as a local SwiftPM dependency:

```swift
// Package.swift
dependencies: [
    .package(path: "../rust-pdf/bindings/swift")
],
targets: [
    .executableTarget(name: "MyApp", dependencies: [
        .product(name: "RustPdf", package: "RustPdf")
    ])
]
```

## Quick start

```swift
import RustPdf

let doc = try Document()
try doc.setInfo(DocumentInfo(title: "Invoice", author: "Acme"))
try doc.addPage()

// Vector graphics.
try doc.setFillRGB(0.12, 0.45, 0.95)
try doc.rect(x: 72, y: 690, width: 200, height: 80)
try doc.fill()

// Embedded font + text.
let font = try doc.addFont(path: "Roboto-Regular.ttf")
try doc.setFillRGB(0, 0, 0)
try doc.showText(font: font, size: 24, x: 72, y: 640, "Hello, world")
try doc.paragraph(font: font, size: 12, x: 72, y: 600, width: 420,
                  text: "A wrapping, justified paragraph.", align: .justify)

let bytes = try doc.toBytes()        // [UInt8]
try doc.save(to: "out.pdf")
```

Most `Document`/`EditableDoc` mutators return `self`, so calls can be chained:

```swift
try Document()
    .addPage()
    .setFillRGB(1, 0, 0)
    .rect(x: 0, y: 0, width: 100, height: 100)
    .fill()
    .save(to: "red.pdf")
```

## Corporate features (licensed)

PDF/A, tagging, encryption, signing and page rendering (a **Pro** feature)
require a license. Activate a token explicitly, or set `RUSTPDF_LICENSE` /
`RUSTPDF_LICENSE_FILE` in the environment (auto-activated on first use):

```swift
try Pdf.activateLicense(token)

// Tagged PDF/A-2a.
let doc = try Document()
try doc.pdfa(.a2a)
try doc.addPage()
let f = try doc.addFont(path: "Roboto-Regular.ttf")
try doc.showText(font: f, size: 20, x: 72, y: 760, "Title", headingLevel: 1)
let pdfa = try doc.toBytes()
```

## Editing an existing PDF

```swift
let ed = try EditableDoc(loading: bytes)
try ed.merge(EditableDoc(loading: other))
try ed.rotatePage(0, degrees: 90)
try ed.optimize()
let out = try ed.toBytes()

// Signature-safe incremental update (preserves the original verbatim).
let ed2 = try EditableDoc(loading: bytes)
try ed2.setInfo(key: "Subject", value: "Reviewed")
let incremental = try ed2.toBytesIncremental(over: bytes)

// Encryption (AES-256/R6).
let ed3 = try EditableDoc(loading: bytes)
try ed3.encrypt(method: .aes256, user: "", owner: "secret")
let encrypted = try ed3.toBytes()
```

## Text extraction & signing

```swift
let text = try Pdf.extractText(bytes)

let signed = try Pdf.sign(
    pdf: bytes, keyDER: pkcs8Key, certDER: cert,
    options: SignOptions(reason: "Approved", pades: true))

let stamped = try Pdf.timestamp(pdf: signed, tsaKeyDER: tsaKey, tsaCertDER: tsaCert)
let withDss = try Pdf.addDss(pdf: stamped, certs: [cert], crls: [crl])
```

## Error handling

Every fallible call throws `PdfError`, carrying the `PdfStatus` and the
library's last-error message:

```swift
do {
    try doc.pdfa()
    _ = try doc.toBytes()
} catch let e as PdfError {
    print(e.status, e.message)   // e.g. .license, "feature not licensed: ..."
}
```

## Notes

* `Document` and `EditableDoc` are reference types; the native handle is freed
  once in `deinit`. Binary payloads cross as Swift `[UInt8]`; the binding copies
  native out-buffers and frees them with `pdf_buffer_free` for you.
* The C ABI is the same one used by the Python, .NET, Go, PHP, Ruby, Node.js,
  Java and Delphi bindings; regenerate the header with `make header`.

# rust-pdf

**A free, MIT-licensed PDF library: one memory-safe Rust core, idiomatic bindings
for Python, C#/.NET, Node.js/TypeScript, Go, PHP, Ruby, Java, Swift, Delphi and Rust.**

Generate, edit, sign, encrypt, validate and render PDFs, with PDF/A, PDF/UA,
PAdES and AES-256 included. Every feature is free, with no license keys and no
network calls.

[![CI](https://github.com/rustpdf/rustpdf/actions/workflows/ci.yml/badge.svg)](https://github.com/rustpdf/rustpdf/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![PyPI](https://img.shields.io/pypi/v/rustpdf?label=PyPI)](https://pypi.org/project/rustpdf/)
[![npm](https://img.shields.io/npm/v/rustpdf?label=npm)](https://www.npmjs.com/package/rustpdf)
[![NuGet](https://img.shields.io/nuget/v/RustPdf?label=NuGet)](https://www.nuget.org/packages/RustPdf)
[![RubyGems](https://img.shields.io/gem/v/rustpdf?label=RubyGems)](https://rubygems.org/gems/rustpdf)
[![Packagist](https://img.shields.io/packagist/v/rust-pdf/rustpdf?label=Packagist)](https://packagist.org/packages/rust-pdf/rustpdf)

[Website](https://rustpdf.dev) · [Docs](https://rustpdf.dev/docs/) ·
[Changelog](CHANGELOG.md) · [Issues](https://github.com/rustpdf/rustpdf/issues)

---

## Features

| Area | What you get |
|------|--------------|
| **Generation** | Pages, vector graphics, embedded + subset TrueType/OpenType fonts, full Unicode (BiDi, CJK, HarfBuzz-quality shaping), justified paragraphs, auto-paginated tables, JPEG/PNG images with alpha |
| **PDF/A archival** | PDF/A-1b, 2b, 2a, 3b, 3a, 4, 4e, 4f (PDF 2.0), all validated with veraPDF; convert existing PDFs to PDF/A; ZUGFeRD / Factur-X invoices |
| **Accessibility** | Tagged PDF / **PDF/UA-1**: headings, figures with alt text, tables, lists and captions |
| **Digital signatures** | PKCS#7 and **PAdES B-B / B-LT / B-LTA** with RFC 3161 timestamps; visible and multiple signatures; DocMDP certification; **HSM / deferred signing** (the private key never enters the library); signature verification |
| **Encryption** | RC4, AES-128 and **AES-256 (R6)**, with user/owner passwords and permissions |
| **Forms** | Create AcroForm fields (text, checkbox, radio, dropdown) with generated appearances; fill and flatten existing forms |
| **Editing** | Merge, split, reorder, rotate, delete pages; watermarks; stamp text/images; true redaction; links and bookmarks; incremental updates; object-stream compression |
| **Extraction** | Text (Unicode via `ToUnicode`, per page, with coordinates), images (JPEG verbatim, others as PNG), metadata, page geometry |
| **Rendering** | Rasterize any page to PNG with a pure-Rust renderer (tiny-skia) |

Output is **deterministic**: the same input produces byte-identical PDFs in
every language.

## Install

| Language | Package | Install |
|----------|---------|---------|
| Python | [PyPI](https://pypi.org/project/rustpdf/) | `pip install rustpdf` |
| Node.js / TypeScript | [npm](https://www.npmjs.com/package/rustpdf) | `npm install rustpdf` |
| C# / .NET | [NuGet](https://www.nuget.org/packages/RustPdf) | `dotnet add package RustPdf` |
| Go | [rustpdf-go](https://github.com/rustpdf/rustpdf-go) | `go get github.com/rustpdf/rustpdf-go` |
| PHP (ext-ffi) | [Packagist](https://packagist.org/packages/rust-pdf/rustpdf) | `composer require rust-pdf/rustpdf` |
| Ruby | [RubyGems](https://rubygems.org/gems/rustpdf) | `gem install rustpdf` |
| Swift (macOS / iOS) | [xcframework](https://rustpdf.dev/docs/swift) | SwiftPM binary target |
| Delphi / Free Pascal | [zip](https://rustpdf.dev/docs/delphi) | unit + native libs |
| Java (JNA) | [`bindings/java`](bindings/java) | build from source |
| Rust | [`crates/pdf`](crates/pdf) | path / git dependency |

Every package bundles the native library for its platform, so there is nothing
to compile.

## Quick start

**Python**: create a tagged PDF/A-2a, read it back, then encrypt it:

```python
import rustpdf

with rustpdf.Document() as doc:
    doc.pdfa(rustpdf.PdfaLevel.A2A).set_info(title="Report")
    font = doc.add_font_file("Roboto-Regular.ttf")
    doc.add_page()
    doc.show_text(font, 20, 72, 760, "Title", heading_level=1)
    data = doc.to_bytes()

print(rustpdf.extract_text(data))

with rustpdf.EditableDoc.load(data) as ed:
    ed.set_info("Subject", "Quarterly numbers")
    ed.encrypt(owner="owner", method=rustpdf.Encryption.AES256)
    ed.save("secured.pdf")
```

**Node.js / TypeScript**:

```js
const { Document, PdfaLevel, extractText } = require("rustpdf");

const doc = new Document();
doc.pdfa(PdfaLevel.A2a).tagged().setInfo({ title: "Report" });
const font = doc.addFontFile("Roboto-Regular.ttf");
doc.addPage().showText(font, 20, 72, 760, "Title", 1);
const data = doc.toBytes();
doc.close();

console.log(extractText(data));
```

**Rust**:

```rust
let mut doc = pdf::Document::new();
let font = doc.add_font_file("Roboto-Regular.ttf")?;
doc.add_page().text(font, 24.0).at(72.0, 740.0).show("Olá, açúcar — café");
doc.save("out.pdf")?;

// Sign with PAdES (incremental update, PKCS#7 detached):
let signer = pdf::Signer::from_pkcs8_der(&key_der, &cert_der)?;
let signed = pdf::sign(&std::fs::read("out.pdf")?, &signer, &pdf::SignOptions::default())?;
```

Full API references for every language are at **[rustpdf.dev/docs](https://rustpdf.dev/docs/)**.

## Why rust-pdf

- **One core, many languages.** Every binding calls the same Rust engine, so
  features and fixes ship to all ten at once and behave the same everywhere.
- **Standards-validated.** Output is checked with qpdf and mutool, PDF/A and
  PDF/UA with veraPDF, and signatures with pdfsig and OpenSSL. The test suite
  runs each validator that is installed.
- **Memory-safe and local.** Pure Rust with no C dependencies in the core; all
  processing happens in-process, with no telemetry.
- **Free for commercial use.** MIT, with no AGPL and no per-document fees. See
  the [iText](https://rustpdf.dev/itext-alternative),
  [Aspose](https://rustpdf.dev/aspose-alternative) and
  [PSPDFKit](https://rustpdf.dev/pspdfkit-alternative) comparisons.

## Architecture

```
cos → writer / graphics → pdf ──→ ffi (C ABI) ──→ Python · C# · Node · Go · PHP
          parser · fonts · images · render ↗              Ruby · Java · Swift · Delphi · Rust
```

- **Idiomatic Rust core** (`crates/*`): builders, `Result`, enums; the object
  graph is an arena, `Send` but not `Sync`
  ([ADR 0002](docs/adr/0002-concurrency-model.md)).
- **`ffi` crate**: the only layer that crosses the C boundary, with opaque
  handles, status codes, panic safety and a `cbindgen`-generated
  [`include/pdf.h`](include/pdf.h) ([FFI rules](docs/FFI_RULES.md),
  [ADR 0001](docs/adr/0001-porting-strategy.md)).
- **Bindings** (`bindings/*`): thin idiomatic wrappers over the C ABI, each with
  a full-surface smoke test.

## Building from source

```sh
make build        # cargo build --workspace
make test         # cargo test --workspace (uses qpdf/mutool/veraPDF when installed)
make ci           # fmt-check + clippy + build + test
make header       # rebuild the cdylib and regenerate include/pdf.h
make python-test  # or: csharp/go/php/ruby/node/java/delphi/swift-test
```

Validators that aren't installed are reported as unavailable and skipped, so the
suite passes on a bare machine.

## Contributing

Issues and PRs are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md), and report
vulnerabilities privately as described in [SECURITY.md](SECURITY.md). Planned
and partial work is tracked in [`PENDING.md`](PENDING.md).

## License

[MIT](LICENSE). Third-party licenses are listed in [`LICENSES.md`](LICENSES.md).

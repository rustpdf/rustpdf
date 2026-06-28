# rust-pdf

A PDF library written in Rust, designed from day one for a **single core +
thin foreign-language bindings** (paid product, portable by design).

> **Business thesis:** sell the enterprise-grade PDF library (PDF/A, digital
> signatures, encryption, accessibility) that PHP, Ruby, Go, C#, Node and Java
> lack — one Rust core, many languages, licensed per feature. See
> [`TESE-DE-NEGOCIO.md`](TESE-DE-NEGOCIO.md).

This repo currently implements **Fase 0 → Fase 6 plus parts of Fase 7** of [`project.md`](project.md):
tooling, the COS object model, the document writer, vector graphics, the
text/font stack (embedded subsetted fonts, Unicode/Type0, HarfBuzz-quality
shaping, justified paragraphs), images (JPEG `DCTDecode`, PNG `FlateDecode`,
palette, alpha via `SMask`, 16-bit), and a **parser** for existing PDFs (classic
& cross-reference streams, object streams, all standard filters, recovery, and
RC4/AES decryption), plus **manipulation** (merge, split, rotate/reorder/delete,
text extraction, metadata, overlay, optimize) and **page rendering** (rasterize
a page to a PNG image, a native tiny-skia rasterizer). Output is validated with
`qpdf`/`mutool`, text round-trips through `pdftotext`, page rendering is checked
against `mutool` with a perceptual diff, and the C ABI is dogfooded from Python.

> Deferred/partial items across all phases are tracked in [`PENDING.md`](PENDING.md)
> (e.g. AcroForm appearance streams, object dedupe, network TSA/OCSP).

## Architecture

Two layers (ADR [0001](docs/adr/0001-porting-strategy.md)):

* **Idiomatic Rust core** — rich, unrestricted (`Result`, builders, enums).
* **`ffi` crate** — the *only* layer crossing the C boundary
  ([`docs/FFI_RULES.md`](docs/FFI_RULES.md)). Opaque handles, `extern "C"`,
  panic-safe, `cbindgen`-generated header.

The core is `Send` (not `Sync`): the object graph is an arena (`Vec` + indices),
never `Rc`/`RefCell` (ADR [0002](docs/adr/0002-concurrency-model.md)).

### Crates

| Crate      | Role | Status |
|------------|------|--------|
| `cos`      | object model + serialization | ✅ Fase 1 |
| `writer`   | header / xref / trailer / arena | ✅ Fase 1 |
| `graphics` | content stream, state, colors, paths | ✅ Fase 2 |
| `pdf`      | high-level API + edit/extract/layout/encrypt | ✅ Fase 1–4, 6, 7.3/7.6 |
| `fonts`    | parsing / embedding / subsetting / shaping / BiDi | ✅ Fase 3 |
| `images`   | JPEG / PNG / palette / alpha / 16-bit | ✅ Fase 4 |
| `parser`   | read existing PDFs (xref/streams, filters, crypto) | ✅ Fase 5 |
| `render`   | rasterize a page to an image (tiny-skia) | ✅ Fase 7.8 (Pro feature) |
| `ffi`      | C ABI boundary (full surface, ~78 exports) | ✅ + 9 bindings (Python on PyPI, Node on npm) |
| `license`  | Ed25519-signed feature licensing (gates PDF/A, signing, encryption) | ✅ |
| `testkit`  | external validators + visual regression | ✅ Fase 0 |
| `layout`   | high-level flow (tables, pagination) | ⏳ Fase 7 (paragraph done in `pdf`) |

## Quick start (Rust)

```rust
use pdf::Document;

let mut doc = Document::new();              // A4, sensible defaults
let page = doc.add_page();
page.content()
    .set_fill_rgb(0.86, 0.20, 0.18)
    .rect(72.0, 640.0, 200.0, 120.0)
    .fill();
doc.save("out.pdf")?;
```

Text with an embedded, subsetted font (Unicode-capable):

```rust
let mut doc = pdf::Document::new();
let font = doc.add_font_file("assets/fonts/Roboto-Regular.ttf")?;
doc.add_page()
    .text(font, 24.0)
    .at(72.0, 740.0)
    .show("Olá, açúcar — café");          // shaped, kerned, extracts via ToUnicode

// Auto-wrapped, justified paragraph with an inline-styled span (L2):
use pdf::{Paragraph, Align};
doc.last_page_mut().unwrap().paragraph(
    Paragraph::new(font, 12.0).box_at(72.0, 700.0, 451.0).align(Align::Justify)
        .text("A long paragraph that wraps and justifies automatically."),
);
doc.save("out.pdf")?;
```

See runnable examples:

```sh
cargo run -p pdf --example blank_page      -- blank.pdf      # Fase 1.6
cargo run -p pdf --example vector_graphics -- vectors.pdf    # Fase 2.7
cargo run -p pdf --example text_unicode    -- text.pdf       # Fase 3A.5/3B
cargo run -p pdf --example report          -- report.pdf     # Fase 3F (paragraphs)
cargo run -p pdf --example images_demo     -- images.pdf     # Fase 4 (JPEG + transparent PNG)
```

## Quick start (bindings, via C ABI)

Nine bindings cover the **whole product surface** over the C ABI: Python
(`bindings/python/rustpdf`, `ctypes`), **C#/.NET** (`bindings/csharp/RustPdf`,
source-generated P/Invoke), **Go** (`bindings/go/rustpdf`, cgo), **PHP**
(`bindings/php`, `ext-ffi`), **Ruby** (`bindings/ruby`, Fiddle),
**Node.js/TypeScript** (`bindings/node`, Koffi), **Java** (`bindings/java`, JNA),
**Delphi / Free Pascal** (`bindings/delphi`, dynamic-loading FFI) and **Swift**
(`bindings/swift`, SwiftPM): fonts/text/paragraphs, images, PDF/A (1b–3a),
tagging, attachments, AcroForm fields, manipulation, extraction, encryption,
signatures and licensing. Smoke tests:
`make {python,csharp,go,php,ruby,node,java,delphi,swift}-test`.

**Published packages:** Python (`pip install rustpdf`) and Node.js
(`npm install rustpdf`) are live on PyPI and npm; both bundle the native
`libpdf_ffi` per platform, so there's nothing to build. The other bindings build
from this repo.

```sh
cargo build -p pdf-ffi          # builds the cdylib + generates include/pdf.h
python3 - <<'PY'
import sys; sys.path.insert(0, "bindings/python")
import rustpdf

with rustpdf.Document() as doc:                       # author a tagged PDF/A-2a
    doc.pdfa(rustpdf.PdfaLevel.A2A).set_info(title="Report")
    f = doc.add_font_file("assets/fonts/Roboto-Regular.ttf")
    doc.add_page()
    doc.show_text(f, 20, 72, 760, "Title", heading_level=1)
    data = doc.to_bytes()

print(rustpdf.extract_text(data))                     # round-trip the text
with rustpdf.EditableDoc.load(data) as ed:            # manipulate + encrypt
    ed.set_info("Subject", "via FFI")
    ed.encrypt(owner="owner", method=rustpdf.Encryption.AES256)
    ed.save("secured.pdf")
PY
```

For the same vector drawing, the Python output is **byte-identical** to the Rust
API (dogfood, Fase 1.7) — `make python-test` checks this and exercises the full
surface.

The same surface in **Node.js / TypeScript** (`npm install rustpdf`), with
idiomatic camelCase and `Buffer` payloads:

```js
const { Document, PdfaLevel, Encryption, EditableDoc, extractText } = require("rustpdf");

const doc = new Document();
doc.pdfa(PdfaLevel.A2a).tagged().setInfo({ title: "Report" });
const f = doc.addFontFile("assets/fonts/Roboto-Regular.ttf");
doc.addPage().showText(f, 20, 72, 760, "Title", 1);   // headingLevel 1 = H1
const data = doc.toBytes();
doc.close();

console.log(extractText(data));                        // round-trip the text

const ed = EditableDoc.load(data);                     // manipulate + encrypt
ed.setInfo("Subject", "via FFI");
ed.encrypt({ owner: "owner", method: Encryption.Aes256 });
ed.save("secured.pdf");
ed.close();
```

Full per-language references live at [rustpdf.dev/docs](https://rustpdf.dev/docs/).

## Building & testing

The toolchain may be under rustup without `cargo` on `PATH`; if so prepend the
toolchain `bin/` or set `CARGO`.

```sh
make build        # cargo build --workspace
make test         # cargo test --workspace  (runs qpdf/mutool if installed)
make clippy       # -D warnings
make fmt-check
make deny         # license & advisory policy (cargo-deny)
make header       # regenerate include/pdf.h
make python-test  # FFI dogfood: Rust vs Python byte-identity
make corpus       # regenerate the golden corpus
```

### External validators

`testkit` shells out to `qpdf --check`, `mutool clean` and `verapdf`. Missing
tools are reported `Unavailable` and skipped, so tests still pass on a bare
machine. Visual regression renders with `mutool draw` and compares with a
perceptual diff.

## Status against `project.md`

* **Fase 0** — workspace, CI, corpus + catalog, validator harness, visual
  regression, ADRs, FFI rules + PR checklist, `cbindgen` header, Python
  reference binding, license tracking, platform build matrix.
* **Fase 1** — COS types, canonical serialization, string/name escaping,
  streams (direct & indirect `/Length`), document/xref/trailer, blank-page
  milestone, FFI dogfood.
* **Fase 2** — page tree, content-stream builder, graphics state, device colors
  (RGB/Gray/CMYK), paths, painting + clipping, colored-graphics milestone.
* **Fase 3** — font parsing/metrics (`ttf-parser`), embedded `FontFile2`,
  Type0/CIDFontType2 + `Identity-H` + `ToUnicode`, subsetting (`subsetter`),
  shaping with kerning/ligatures (`rustybuzz`), BiDi (`unicode-bidi`), CJK, and
  the L2 paragraph engine (wrap + align + inline styles). Complex-script RTL
  embedding (Arabic/Indic, 3E.1/3E.2) and Knuth–Plass (3D.3) remain partial.
* **Fase 4** — images: JPEG embedded verbatim (`DCTDecode`), PNG decoded and
  re-encoded (`FlateDecode`), palette → `Indexed`, alpha (RGBA / grayscale-alpha
  / palette `tRNS`) → `/SMask`, 8- and 16-bit depths; Image XObject + `Do`.
* **Fase 5** — parser: non-panicking lexer, indirect objects, classic `xref` +
  cross-reference streams, object streams, hybrid/`Prev` chains, all standard
  filters (Flate w/ predictors, LZW, ASCIIHex, ASCII85, RunLength), brute-force
  recovery, and standard-handler decryption (RC4 R2/R3, AESv2 R4, **AESv3 V5/R6
  AES-256** via Algorithm 2.A/2.B).

```rust
let doc = parser::PdfReader::parse(std::fs::read("in.pdf")?)?;
println!("{} pages", doc.pages().len());
let catalog = doc.root()?;             // resolved /Root
```
* **Fase 6** — manipulation via `pdf::EditableDoc`: `merge`, `extract_pages`,
  `rotate_page`/`reorder_pages`/`delete_page`, `set_info`/`set_xmp`,
  `overlay_page`, `fill_text_field`, `optimize`; plus `pdf::extract_text` (content
  stream → Unicode via `ToUnicode`, with space/line inference).

```rust
let mut a = pdf::EditableDoc::load(std::fs::read("a.pdf")?)?;
a.merge(&pdf::EditableDoc::load(std::fs::read("b.pdf")?)?);
a.set_info("Title", "Merged");
a.save("merged.pdf")?;
let text = pdf::extract_text(std::fs::read("merged.pdf")?)?;
```

* **Fase 7 (partial)** — `pdf::Report`/`Table`: an auto-paginated layout engine
  (tables with wrapping cells, running header/footer); `EditableDoc::encrypt` —
  standard-handler encryption (RC4-128 / AES-128) with permission flags; and
  `pdf::sign` — **digital signatures** (incremental update + `ByteRange` + PKCS#7
  detached; **visible**, **multiple**, **certificate chains**); **PAdES** —
  B-B (`SignOptions.pades`), B-LT (`pdf::add_dss` → `/DSS` with certs/CRLs),
  B-LTA (`pdf::timestamp` → RFC 3161 `/DocTimeStamp`). Validated by `pdfsig` and
  `openssl cms -verify`.
* **PDF/A** (levels **1b / 2b / 2a / 3b / 3a**) via `Document::pdfa()` /
  `pdfa_a()` / `pdfa_with(PdfaLevel)`; A-3 embeds attachments (`attach_file` →
  `/AFRelationship` + `/AF`). **Accessibility** (`tagged()`): **semantic tags** —
  `/H1`–`/H6`, `/Figure` + `/Alt`, `/Table`/`/TR`/`/TH`/`/TD` (with `/Scope` +
  `/Headers`-`/ID`), **lists** `/L`-`/LI`-`/LBody` and `/Caption` — the `Report`
  marks tables and lists automatically. **veraPDF-validated**: `-f 1b/2b/2a/3b/3a`
  and `-f ua1` (**PDF/UA-1**).
* **Interactive forms** (`text_field`/`checkbox`/`radio_group`/`dropdown`): a full
  `/AcroForm` with **generated `/AP` appearances** and **hierarchical field names**.
* **Encryption** covers RC4 / AES-128 / **AES-256 (V5/R6)** with **CSPRNG**
  IVs/keys; `optimize()` emits **object streams + a cross-reference stream**,
  **dedupes** identical objects, and `to_bytes_incremental` does **non-destructive
  incremental updates**. Only HTML→PDF (7.7) is out of scope; network TSA/OCSP and
  inline `/Span` remain deferred.

```rust
let report = pdf::Report::new(font)
    .header("Acme Inc.").page_numbers(true)
    .heading("Invoice", 24.0).paragraph_justified(intro)
    .table(pdf::Table::new(font, 10.0, vec![60.0, 300.0, 90.0]).header_row(true)
        .row(["#", "Item", "Total"]) /* ... */);
report.render(&mut doc);

let mut secured = pdf::EditableDoc::load(std::fs::read("doc.pdf")?)?;
secured.encrypt("", "owner", pdf::Permissions::read_only());  // AES-128
secured.save("secured.pdf")?;

// Digital signature (PKCS#7 detached, incremental update):
let signer = pdf::Signer::from_pkcs8_der(&key_der, &cert_der)?;
let signed = pdf::sign(&pdf_bytes, &signer, &pdf::SignOptions::default())?;
// `pdfsig signed.pdf` → "Signature is Valid. Total document signed."
```

## Licensing (corporate features)

Basic generation is always available; **PDF/A, digital signatures/PAdES, and
encryption** require an active, cryptographically-signed license (Ed25519,
offline-verified, with an expiry date). Without one, those calls return a
`License` error and produce no output.

The customer **never rebuilds** — they just supply the emailed token, either via
an env var (auto-activated, no code) or one explicit call:

```sh
export RUSTPDF_LICENSE="010f0000…"          # the emailed token; auto-activated
```
```rust
pdf::activate_license(token)?;              // …or activate explicitly
let bytes = doc.pdfa().to_bytes()?;         // now allowed
```

You (the vendor) embed your **public** key once at build
(`RUSTPDF_LICENSE_PUBKEY`) and mint per-customer **tokens** with your secret key
via the `licctl` tool. Full design: [`docs/LICENSING.md`](docs/LICENSING.md).

Deferred/partial items across all phases: see [`PENDING.md`](PENDING.md)
(AES-256, signatures, PDF/A, tagged PDF, object streams on write, …).

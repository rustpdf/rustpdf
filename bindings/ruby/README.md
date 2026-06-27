# rustpdf (Ruby binding)

Idiomatic Ruby binding for the `rust-pdf` core over its C ABI (`libpdf_ffi`),
using the built-in **Fiddle** standard library — no native gem to compile. It
covers the whole product surface: vector graphics, embedded/subsetted fonts and
text, wrapping paragraphs, images, **PDF/A** (levels 1b–3a),
**tagged/accessible** output, embedded-file attachments, **AcroForm** fields,
manipulation (merge/split/rotate/optimize/incremental update), **text
extraction**, **encryption** (RC4 / AES-128 / AES-256) and **digital signatures**
(PKCS#7 / PAdES) — plus **feature licensing**.

Files (module `RustPdf`):

* `lib/rustpdf.rb` — module functions (`version`, `activate_license`,
  `extract_text`, `sign`, `timestamp`, `add_dss`), enums, error, helpers;
* `lib/rustpdf/native.rb` — the Fiddle signature table + loader;
* `lib/rustpdf/document.rb`, `editable_doc.rb` — the `Document` / `EditableDoc`
  classes.

## Loading the native library

`Native` finds `libpdf_ffi` via `RUSTPDF_LIB`, then by walking up from `lib/` to
`target/{debug,release}`. Build it from the repo root with
`cargo build -p pdf-ffi`. Requires Ruby ≥ 2.6 (Fiddle is bundled).

## Quick start

```ruby
require "rustpdf"

RustPdf.activate_license(token)  # or set RUSTPDF_LICENSE (auto-activated)

doc = RustPdf::Document.new
doc.pdfa(RustPdf::Pdfa::A2A).info(title: "Report")
f = doc.add_font_file("assets/fonts/Roboto-Regular.ttf")
doc.add_page
   .show_text(f, 20, 72, 760, "Title", heading_level: 1)
   .paragraph(f, 12, 72, 720, 450, "A wrapping body…", align: RustPdf::Align::JUSTIFY)
data = doc.to_bytes

puts RustPdf.extract_text(data)

ed = RustPdf::EditableDoc.load(data)
ed.encrypt(method: RustPdf::Cipher::AES256, owner: "owner").save("secured.pdf")

signed = RustPdf.sign(data, key_der, cert_der, pades: true)
```

Corporate features (PDF/A, signing, encryption, accessibility) require a license;
without one they raise `RustPdf::Error`. See [`docs/LICENSING.md`](../../docs/LICENSING.md).

## Test

```sh
cargo build -p pdf-ffi
ruby bindings/ruby/test/run.rb     # or: make ruby-test
```

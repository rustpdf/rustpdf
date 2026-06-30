# rustpdf (Ruby binding)

Idiomatic Ruby binding for the `rust-pdf` core over its C ABI (`libpdf_ffi`),
using the built-in **Fiddle** standard library — no native gem to compile. It
covers the whole product surface: vector graphics, embedded/subsetted fonts and
text, wrapping paragraphs, images, **PDF/A** (levels 1b–3a),
**tagged/accessible** output, embedded-file attachments, **AcroForm** fields,
manipulation (merge/split/rotate/optimize/incremental update), **text
extraction**, **page rendering** (page to PNG image), **encryption** (RC4 / AES-128 / AES-256) and **digital signatures**
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

Corporate features (PDF/A, signing, encryption, accessibility, page rendering — a **Pro** feature) require a license;
without one they raise `RustPdf::Error`. See [`docs/LICENSING.md`](../../docs/LICENSING.md).

## Deferred / HSM signing

Sign without the private key ever entering this library — the key stays in an
HSM, cloud KMS, smartcard or PKI token. Works with any PKI (eIDAS, AATL, or
your own CA). The library builds the CMS and asks your code only for the raw
RSA signature over the bytes it hands you.

```ruby
require "rustpdf"

pdf      = File.binread("contract.pdf")
cert_der = File.binread("signing-cert.der")   # X.509 signer certificate (DER)

# Model A — your block returns the raw RSA PKCS#1 v1.5 signature; the key
# never reaches the library. Call your HSM / cloud KMS / token here.
signed = RustPdf.sign_with(pdf, cert_der,
                           chain: [issuer_der],
                           options: RustPdf::SigningOptions.new(reason: "Approved")) do |to_sign|
  hsm.sign_rsa_sha256(to_sign)   # bytes in, raw signature out
end
File.binwrite("contract.signed.pdf", signed)

# Model B — two-phase: prepare, sign the hash out of band, then complete.
session   = RustPdf.begin_signing(pdf)
container = remote.build_cms(session.hash)     # send #hash to a remote signer
final     = session.complete(container)        # embed the DER CMS / PKCS#7
```

`RustPdf.list_signatures(pdf)` inventories existing signature fields before you
sign. See [`docs/ruby.html`](../../site/public/docs/ruby.html#sign) for the full
deferred-signing API (`SigningOptions`, `Certify`, `SignaturePolicy`).

## Test

```sh
cargo build -p pdf-ffi
ruby bindings/ruby/test/run.rb     # or: make ruby-test
```

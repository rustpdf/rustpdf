# RustPdf for Ruby

Generate, edit, sign and process PDFs from Ruby: vector graphics, embedded fonts and Unicode text, wrapping paragraphs, images, **PDF/A** (1b-4f), **tagged/accessible** output, attachments, **AcroForm** fields, page manipulation (merge/split/stamp), watermarks, true **redaction**, **AES-256** encryption, **digital signatures (PAdES)** with HSM/deferred signing, timestamps/LTV, text extraction and search, and page **rendering to PNG**. Pure Ruby gem (stdlib Fiddle), no native extension to compile.

## Documentation

- **Full API reference:** https://rustpdf.dev/docs/ruby
- **Interactive positioning guide** (coordinates, anchors, rotation): https://rustpdf.dev/positioning
- All product guides (PDF/A, signatures, encryption, redaction, rendering): https://rustpdf.dev/docs/

The public API is `RustPdf::Document` (create PDFs), `RustPdf::EditableDoc`
(load and edit existing PDFs) and module functions (`version`,
`extract_text`, `sign`, `timestamp`, `add_dss`).

## Loading the native library

`Native` finds `libpdf_ffi` via `RUSTPDF_LIB`, then by walking up from `lib/` to
`target/{debug,release}`. Build it from the repo root with
`cargo build -p pdf-ffi`. Requires Ruby ≥ 2.6 (Fiddle is bundled).

## Quick start

```ruby
require "rustpdf"

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

Every feature is free — PDF/A, digital signatures/PAdES, encryption,
accessibility/tagging, redaction and page rendering are all included.

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

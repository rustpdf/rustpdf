# RustPdf for Python

Generate, edit, sign and process PDFs from Python: vector graphics, embedded fonts and Unicode text, wrapping paragraphs, images, **PDF/A** (1b-4f), **tagged/accessible** output, attachments, **AcroForm** fields, page manipulation (merge/split/stamp), watermarks, true **redaction**, **AES-256** encryption, **digital signatures (PAdES)** with HSM/deferred signing, timestamps/LTV, text extraction and search, and page **rendering to PNG**. Pure Python package, no compiler needed.

## Documentation

- **Full API reference:** https://rustpdf.dev/docs/python
- **Interactive positioning guide** (coordinates, anchors, rotation): https://rustpdf.dev/positioning
- All product guides (PDF/A, signatures, encryption, redaction, rendering): https://rustpdf.dev/docs/

The public API is `Document` (create PDFs) and `EditableDoc` (load and edit
existing PDFs), both usable as context managers; errors raise `PdfError`.

## Install

```sh
pip install rustpdf
```

Platform wheels (macOS arm64, manylinux_2_28 x86_64/aarch64, Windows x64)
bundle the native `libpdf_ffi` library — no Rust toolchain needed to install.
Basic PDF generation is free; corporate features (PDF/A, accessibility,
encryption, signatures, page rendering) unlock with a license token via the
`RUSTPDF_LICENSE` env var. Page rendering is a **Pro** feature. See
<https://rustpdf.dev>.

## Loading the native library

The wrapper finds `libpdf_ffi` in this order:

1. `$RUSTPDF_LIB` (explicit path);
2. bundled next to `rustpdf/__init__.py` (installed wheel);
3. the build tree (`target/debug` then `target/release`).

Build it from the repo root with `cargo build -p pdf-ffi`.

## Quick start

```python
import rustpdf

# Author an accessible PDF/A-2a document.
with rustpdf.Document() as doc:
    doc.pdfa(rustpdf.PdfaLevel.A2A).set_info(title="Report", author="me")
    f = doc.add_font_file("assets/fonts/Roboto-Regular.ttf")
    doc.add_page()
    doc.show_text(f, 20, 72, 760, "Title", heading_level=1)
    doc.paragraph(f, 12, 72, 720, 450, "A wrapping, justified body…",
                  rustpdf.Align.JUSTIFY)
    data = doc.to_bytes()

print(rustpdf.extract_text(data))

# Render a page to a PNG image (Pro feature).
print(f"{rustpdf.page_count(data)} page(s)")
png = rustpdf.render_page_to_png(data, page=0, dpi=150.0)
open("page1.png", "wb").write(png)

# Manipulate an existing file (non-destructive incremental update).
with rustpdf.EditableDoc.load(data) as ed:
    ed.set_info("Subject", "Edited")
    updated = ed.to_bytes_incremental(data)

# Encrypt (AES-256).
with rustpdf.EditableDoc.load(data) as ed:
    ed.encrypt(owner="owner", method=rustpdf.Encryption.AES256)
    ed.save("secured.pdf")

# Sign (PKCS#7 detached / PAdES).
signed = rustpdf.sign(data, key_der, cert_der, reason="Approved", pades=True)

# Deferred / HSM signing: the private key never enters the library.
# The library builds the CMS signed attributes and asks your signer for the
# raw RSA signature. `sign_hash` can call any HSM, cloud KMS, smartcard or
# PKI token (national PKI / eIDAS / AATL).
def sign_hash(to_be_signed: bytes) -> bytes:
    return my_hsm.sign(to_be_signed)   # raw RSA PKCS#1 v1.5 over SHA-256

signed = rustpdf.sign_with(data, cert_der, sign_hash, chain=[intermediate_der])
```

## Testing

```sh
cargo build -p pdf-ffi
python3 bindings/python/test_binding.py    # dogfood + full-surface exercise
# or: make python-test
```

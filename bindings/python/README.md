# rustpdf (Python binding)

Idiomatic Python over the `rust-pdf` C ABI (`libpdf_ffi`). It mirrors the full
product surface: vector graphics, embedded/subsetted fonts and text, wrapping
paragraphs, images, **PDF/A** (levels 1b–3a), **tagged/accessible** output,
embedded file attachments, **AcroForm** fields, manipulation
(merge/split/rotate/optimize/incremental update), **text extraction**,
**page rendering** (page to PNG image), **encryption** (RC4 / AES-128 /
AES-256) and **digital signatures** (PKCS#7 / PAdES).

Two layers, per the project's porting strategy:

* a raw `ctypes` surface bound 1:1 against `include/pdf.h`;
* `Document` / `EditableDoc` wrappers that hide opaque handles, raise
  `PdfError` on non-zero status codes, and act as context managers.

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
```

## Testing

```sh
cargo build -p pdf-ffi
python3 bindings/python/test_binding.py    # dogfood + full-surface exercise
# or: make python-test
```

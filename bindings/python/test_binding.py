#!/usr/bin/env python3
"""Reference-binding smoke test (Fase 0.8) + FFI dogfood (Fase 1.7).

Verifies:
  1. The cdylib loads and ``pdf_version()`` is callable from outside Rust.
  2. The Python-built PDF is *byte-identical* to the one the Rust API builds
     for the same drawing — proving the boundary is faithful and the output
     deterministic.
  3. No handle/buffer leaks (every handle freed; buffers freed after read).

Run: ``python3 bindings/python/test_binding.py path/to/rust_reference.pdf``
The reference file is produced by the Rust example with the same drawing.
"""

import struct
import sys
import tempfile
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import rustpdf  # noqa: E402


def build_reference_drawing(doc: "rustpdf.Document") -> None:
    # Must mirror the Rust example `ffi_reference.rs` exactly.
    doc.add_page()
    doc.set_fill_rgb(1.0, 0.0, 0.0)
    doc.rect(0.0, 0.0, 100.0, 100.0)
    doc.fill()


def main() -> int:
    print(f"pdf_version() = {rustpdf.version()!r}")
    print(f"library = {rustpdf.library_path()}")

    with rustpdf.Document() as doc:
        build_reference_drawing(doc)
        assert doc.page_count == 1, doc.page_count
        py_bytes = doc.to_bytes()

    assert py_bytes.startswith(b"%PDF-1.7"), "missing header"
    assert py_bytes.rstrip().endswith(b"%%EOF"), "missing EOF"
    print(f"python produced {len(py_bytes)} bytes")

    if len(sys.argv) > 1:
        rust_bytes = Path(sys.argv[1]).read_bytes()
        if py_bytes != rust_bytes:
            print("MISMATCH: Python and Rust output differ", file=sys.stderr)
            print(f"  python: {len(py_bytes)} bytes", file=sys.stderr)
            print(f"  rust:   {len(rust_bytes)} bytes", file=sys.stderr)
            return 1
        print("OK: Python output is byte-identical to the Rust API output")

    # Round-trip through save() too.
    out = Path(sys.argv[2]) if len(sys.argv) > 2 else Path("python_out.pdf")
    with rustpdf.Document() as doc:
        build_reference_drawing(doc)
        doc.save(out)
    print(f"saved {out}")

    exercise_full_surface()
    print("OK: full binding surface exercised")
    return 0


_FONT = (
    Path(__file__).resolve().parents[2] / "assets" / "fonts" / "Roboto-Regular.ttf"
)


def exercise_full_surface() -> None:
    """Drive the whole binding: licensing, fonts/text, PDF/A, manipulation,
    extraction, forms, encryption — proving the expanded FFI is reachable."""
    # 0. Corporate features are blocked until a valid license is activated.
    try:
        with rustpdf.Document() as doc:
            doc.pdfa().add_page()
            doc.to_bytes()
        raise AssertionError("PDF/A must be blocked without a license")
    except rustpdf.PdfError:
        pass
    lic = (
        Path(__file__).resolve().parents[2]
        / "crates" / "license" / "fixtures" / "dev_license.txt"
    ).read_text().strip()
    rustpdf.activate_license(lic)

    # 1. Tagged PDF/A-2a with an embedded font, heading and justified paragraph.
    with rustpdf.Document() as doc:
        doc.pdfa(rustpdf.PdfaLevel.A2A).set_info(title="Olá", author="rustpdf")
        f = doc.add_font_file(_FONT)
        doc.add_page()
        doc.show_text(f, 20, 72, 760, "Título", heading_level=1)
        doc.paragraph(f, 12, 72, 720, 450, "Um parágrafo. " * 8, rustpdf.Align.JUSTIFY)
        pdfa = doc.to_bytes()
    assert b"pdfaid" in pdfa and b"/StructTreeRoot" in pdfa, "PDF/A-2a markers missing"

    # 2. Text extraction round-trips the Unicode content.
    text = rustpdf.extract_text(pdfa)
    assert "Título" in text, f"extraction failed: {text!r}"

    # 3. Manipulation: load, edit metadata, incremental update preserves prefix.
    with rustpdf.EditableDoc.load(pdfa) as ed:
        assert ed.page_count == 1
        ed.set_info("Subject", "via FFI")
        incr = ed.to_bytes_incremental(pdfa)
    assert incr.startswith(pdfa), "incremental update must preserve the original"
    assert incr.count(b"%%EOF") == 2, "incremental update needs a second xref section"

    # 4. Merge + optimize.
    with rustpdf.EditableDoc.load(pdfa) as a, rustpdf.EditableDoc.load(pdfa) as b:
        a.merge(b).optimize()
        merged = a.to_bytes()
    with rustpdf.EditableDoc.load(merged) as m:
        assert m.page_count == 2

    # 5. AcroForm with every field type.
    with rustpdf.Document() as doc:
        doc.add_page()
        doc.text_field("city", 0, (120, 700, 300, 720), "SP", 12)
        doc.checkbox("ok", 0, (120, 670, 138, 688), True)
        doc.radio_group(
            "plan", 0,
            [((120, 640, 138, 658), "a"), ((160, 640, 178, 658), "b")], selected=1,
        )
        doc.dropdown("country", 0, (120, 610, 300, 630), ["BR", "PT"], selected=0, size=12)
        form = doc.to_bytes()
    assert b"/AcroForm" in form and b"/FT /Ch" in form, "form fields missing"

    # 6. Encryption round-trip (AES-256/R6) — decrypts back with empty password.
    with rustpdf.Document() as doc:
        f = doc.add_font_file(_FONT)
        doc.add_page()
        doc.show_text(f, 14, 72, 700, "segredo")
        plain = doc.to_bytes()
    with rustpdf.EditableDoc.load(plain) as ed:
        ed.encrypt(owner="owner", method=rustpdf.Encryption.AES256)
        enc = ed.to_bytes()
    assert b"/AESV3" in enc, "AES-256 marker missing"
    assert "segredo" in rustpdf.extract_text(enc), "encrypted text not recoverable"

    # 7. Image extraction: embed a PNG, then pull every raster image back out.
    with rustpdf.Document() as doc:
        doc.add_page()
        img = doc.add_image_png(_tiny_png())
        doc.draw_image(img, 72, 600, 64, 64)
        with_img = doc.to_bytes()
    out_dir = tempfile.mkdtemp(prefix="rustpdf_images_")
    n_images = rustpdf.extract_images_to_dir(with_img, out_dir)
    assert isinstance(n_images, int) and n_images >= 1, f"expected >=1 image, got {n_images}"
    written = list(Path(out_dir).iterdir())
    assert len(written) == n_images, f"count {n_images} != files {written}"


def _tiny_png() -> bytes:
    """A minimal valid 1x1 red RGB PNG, built with the stdlib only."""
    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    ihdr = struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0)  # 1x1, 8-bit, RGB
    idat = zlib.compress(b"\x00\xff\x00\x00")  # filter byte 0 + one red pixel
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", idat)
        + chunk(b"IEND", b"")
    )


if __name__ == "__main__":
    raise SystemExit(main())

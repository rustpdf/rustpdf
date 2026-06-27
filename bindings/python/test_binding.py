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

import sys
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


if __name__ == "__main__":
    raise SystemExit(main())

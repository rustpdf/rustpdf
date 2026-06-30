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

    # 8. Page rendering (Pro): rasterize a page to PNG (license already active).
    assert rustpdf.page_count(with_img) == 1
    png = rustpdf.render_page_to_png(with_img, page=0, dpi=72.0)
    assert png[:8] == b"\x89PNG\r\n\x1a\n", "render_page_to_png did not return a PNG"

    # 8a. Positional text search (issue #41 P1): locate text with a box.
    hits = rustpdf.find_text(pdfa, "Título")
    assert hits, "find_text found no matches"
    hit = hits[0]
    assert isinstance(hit, rustpdf.TextHit)
    assert "Título" in hit.text
    assert hit.width > 0 and hit.height > 0, f"bad box: {hit}"
    assert rustpdf.find_text(pdfa, "nonexistent-zzz") == [], "expected no matches"

    # 8b. Normalization (issue #41 P1): downgrade + strip PDF/A on a loaded doc.
    with rustpdf.EditableDoc.load(pdfa) as ed:
        ed.set_version(2)  # 1.7
        normalized = ed.to_bytes()
    assert normalized.startswith(b"%PDF-1.7"), "set_version did not downgrade header"
    with rustpdf.EditableDoc.load(pdfa) as ed:
        ed.normalize(2)  # strip PDF/A + version 1.7
        plain_pdfa = ed.to_bytes()
    assert b"pdfaid" not in plain_pdfa, "normalize must strip the PDF/A identifier"

    # 8c. Page geometry (issue #45 P1): measure pages, rotation swaps dimensions.
    geos = rustpdf.measure_pages(pdfa)
    assert len(geos) == 1, f"expected 1 page geometry, got {len(geos)}"
    g0 = geos[0]
    assert isinstance(g0, rustpdf.PageGeometry)
    assert g0.page == 0 and g0.rotation == 0
    assert g0.width > 0 and g0.height > 0, f"bad size: {g0}"
    assert g0.rotated_width == g0.width and g0.rotated_height == g0.height
    assert isinstance(g0.media_box, rustpdf.PdfRect)
    assert g0.media_box.width > 0 and g0.media_box.height > 0
    # measure_page mirrors the single entry; out-of-range raises IndexError.
    assert rustpdf.measure_page(pdfa, 0) == g0
    try:
        rustpdf.measure_page(pdfa, 5)
        raise AssertionError("measure_page must raise IndexError out of range")
    except IndexError:
        pass
    # Rotate 90° and confirm rotated_* swaps while width/height stay unrotated.
    with rustpdf.EditableDoc.load(pdfa) as ed:
        ed.rotate_page(0, 90)
        rotated = ed.to_bytes()
    rg = rustpdf.measure_page(rotated, 0)
    assert rg.rotation == 90, f"rotation not recorded: {rg}"
    assert abs(rg.rotated_width - g0.height) < 1e-6, f"rotated width swap failed: {rg}"
    assert abs(rg.rotated_height - g0.width) < 1e-6, f"rotated height swap failed: {rg}"

    # 8d. Document inspection (issue #45 P1): version/encryption/pdfa/page count.
    ov = rustpdf.inspect(pdfa)
    assert isinstance(ov, rustpdf.PdfOverview)
    assert ov.page_count == 1, f"bad page count: {ov}"
    assert not ov.encrypted and not ov.requires_password, f"plain doc misreported: {ov}"
    assert ov.version, "inspect returned an empty version"
    assert ov.pdfa_level, f"PDF/A doc must report a pdfa_level: {ov}"
    enc_ov = rustpdf.inspect(enc)  # the AES-256 doc from step 6
    assert enc_ov.encrypted, f"encrypted doc misreported: {enc_ov}"
    assert "AES" in enc_ov.encryption.upper(), f"unexpected encryption: {enc_ov}"

    # 8e. Stamping (issue #45 P1): mask with a white box, then place text; the
    # placed text comes back out via extract_text.
    with rustpdf.EditableDoc.load(pdfa) as ed:
        assert ed.fill_rect(0, 100, 100, 200, 50, color=(1.0, 1.0, 1.0)) is True
        assert ed.place_text(0, 110, 115, "STAMPED-45", size=18) is True
        assert ed.fill_rect(9, 0, 0, 10, 10) is False, "missing page must return False"
        stamped = ed.to_bytes()
    assert "STAMPED-45" in rustpdf.extract_text(stamped), "placed text not extractable"

    # 9. Deferred / external (HSM) signing — issue #41 P0. The private key never
    # reaches the library: it asks our remote signer for the raw RSA signature.
    fixtures = (
        Path(__file__).resolve().parents[2] / "crates" / "pdf" / "tests" / "fixtures"
    )
    key_pk8 = (fixtures / "signer_key.pk8").read_bytes()
    cert_der = (fixtures / "signer_cert.der").read_bytes()
    sign_hash = _make_rsa_signer(key_pk8)

    with rustpdf.Document() as doc:
        f = doc.add_font_file(_FONT)
        doc.add_page()
        doc.show_text(f, 14, 72, 700, "deferred")
        to_sign = doc.to_bytes()

    # list_signatures: a freshly built doc has no signature fields.
    assert rustpdf.list_signatures(to_sign) == [], "plain doc must have no signature fields"

    # Model A: end-to-end remote-callback signing, then validate it.
    external = rustpdf.sign_with(
        to_sign,
        cert_der,
        sign_hash,
        options=rustpdf.SigningOptions(pades=True, reason="HSM"),
    )
    assert b"/ByteRange" in external, "Model A output missing /ByteRange"
    sigs = rustpdf.verify_signatures(external)
    assert sigs and sigs[0]["is_valid"], f"Model A signature must verify: {sigs}"
    # Rich signature inspection (issue #41 P1): new certificate fields present.
    for key in ("issuer", "serial_number", "valid_from", "valid_to", "algorithm",
                "signing_time", "cert_count", "has_timestamp"):
        assert key in sigs[0], f"verify_signatures missing rich field {key!r}"
    assert sigs[0]["cert_count"] >= 1, f"cert_count should be >=1: {sigs[0]}"

    # list_signatures now detects exactly one signed field.
    fields = rustpdf.list_signatures(external)
    assert len(fields) == 1 and fields[0].signed, f"expected one signed field, got {fields}"

    # Model B: two-phase begin → (hash signed remotely) → complete. Here we only
    # assert the session is well-formed (non-empty buffers, 32-byte SHA-256 hash,
    # DocMDP certification embedded); building the CMS container is the
    # integrator's job and is exercised by the C# sample.
    session = rustpdf.begin_signing(
        to_sign,
        rustpdf.SigningOptions(certify=rustpdf.Certify.FORMS_AND_ANNOTATIONS),
    )
    assert session.document, "begin_signing returned an empty prepared document"
    assert session.to_be_signed, "begin_signing returned empty to-be-signed bytes"
    assert session.bytes == session.to_be_signed, ".bytes must alias .to_be_signed"
    assert len(session.hash) == 32, "session hash must be a 32-byte SHA-256 digest"
    assert b"/DocMDP" in session.document, "DocMDP certification missing"
    print("OK: deferred signing (Model A end-to-end + Model B session) verified")

    # 10. Network TSA (AD-RT) — issue #41 P1. Prepare a timestamp, build the
    # RFC 3161 request the integrator would POST to the TSA. We can't reach a
    # live TSA here, so assert the prepared buffers + DER request are well-formed.
    import hashlib

    ts_doc, ts_tbs = rustpdf.begin_timestamp(external)
    assert ts_doc and ts_tbs, "begin_timestamp returned empty buffers"
    imprint = hashlib.sha256(ts_tbs).digest()
    req = rustpdf.timestamp_request(imprint, cert_req=True)
    assert req and req[0] == 0x30, "TimeStampReq must be a DER SEQUENCE"
    print("OK: network-TSA helpers (begin_timestamp + timestamp_request) verified")


def _make_rsa_signer(key_pk8: bytes):
    """Return a ``(data: bytes) -> bytes`` callable that produces the raw
    RSA PKCS#1 v1.5 signature over SHA-256 of ``data`` — i.e. what a remote HSM
    would compute. Prefers the ``cryptography`` package; falls back to ``openssl``.

    The key (PKCS#8 DER) is loaded *only inside this helper*: from the library's
    point of view, ``sign_with`` only ever receives the resulting signature.
    """
    try:
        from cryptography.hazmat.primitives import hashes, serialization
        from cryptography.hazmat.primitives.asymmetric import padding

        priv = serialization.load_der_private_key(key_pk8, password=None)

        def sign(data: bytes) -> bytes:
            return priv.sign(data, padding.PKCS1v15(), hashes.SHA256())

        return sign
    except ImportError:
        import subprocess

        key_file = Path(tempfile.mkstemp(prefix="rustpdf_key_", suffix=".der")[1])
        key_file.write_bytes(key_pk8)

        def sign(data: bytes) -> bytes:
            proc = subprocess.run(
                ["openssl", "dgst", "-sha256", "-sign", str(key_file),
                 "-keyform", "DER"],
                input=data,
                capture_output=True,
                check=True,
            )
            return proc.stdout

        return sign


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

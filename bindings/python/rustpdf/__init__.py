"""rustpdf — Python binding over the rust-pdf C ABI.

Two layers, per ``project.md`` §1.2.1:

* a raw ``ctypes`` surface bound 1:1 against ``include/pdf.h``;
* idiomatic wrappers (:class:`Document`, :class:`EditableDoc`) that hide the
  opaque handles, turn ``PdfStatus`` codes into :class:`PdfError`, and work as
  context managers.

Nobody programs against the C ABI directly. The wrappers cover the whole product
surface: vector graphics, embedded fonts & text, paragraphs, images, PDF/A
(levels 1b–3a), tagged/accessible output, attachments, AcroForm fields,
manipulation (merge/split/rotate/optimize/incremental update), text extraction,
encryption and digital signatures.
"""

from __future__ import annotations

import ctypes
import os
import sys
from ctypes import (
    POINTER,
    byref,
    c_char_p,
    c_double,
    c_int,
    c_size_t,
    c_ubyte,
    c_void_p,
)
from enum import IntEnum
from pathlib import Path

__all__ = [
    "Document",
    "EditableDoc",
    "PdfError",
    "PdfaLevel",
    "Align",
    "AFRelationship",
    "Encryption",
    "version",
    "library_path",
    "activate_license",
    "extract_text",
    "sign",
    "timestamp",
    "add_dss",
]


class PdfError(RuntimeError):
    """Raised when a C ABI call returns a non-zero ``PdfStatus``."""


# ---- enums (mirror the integer arguments documented in pdf.h) --------------


class PdfaLevel(IntEnum):
    A1B = 0
    A2B = 1
    A2A = 2
    A3B = 3
    A3A = 4


class Align(IntEnum):
    LEFT = 0
    RIGHT = 1
    CENTER = 2
    JUSTIFY = 3


class AFRelationship(IntEnum):
    SOURCE = 0
    DATA = 1
    ALTERNATIVE = 2
    SUPPLEMENT = 3
    UNSPECIFIED = 4


class Encryption(IntEnum):
    RC4 = 0
    AES128 = 1
    AES256 = 2


# ---- locate and load the shared library -----------------------------------


def _candidate_paths() -> list[Path]:
    if env := os.environ.get("RUSTPDF_LIB"):
        return [Path(env)]
    if sys.platform == "darwin":
        name = "libpdf_ffi.dylib"
    elif sys.platform == "win32":
        name = "pdf_ffi.dll"
    else:
        name = "libpdf_ffi.so"
    here = Path(__file__).resolve().parent
    root = here.parents[2]
    return [
        here / name,  # bundled inside an installed wheel
        root / "target" / "debug" / name,  # local build tree
        root / "target" / "release" / name,
    ]


def library_path() -> Path:
    """Return the path to the shared library that will be loaded."""
    for candidate in _candidate_paths():
        if candidate.is_file():
            return candidate
    raise PdfError(
        "could not locate libpdf_ffi; build it with "
        "`cargo build -p pdf-ffi` or set RUSTPDF_LIB"
    )


_lib = ctypes.CDLL(str(library_path()))

_DOC = c_void_p  # opaque *PdfDocument
_ED = c_void_p  # opaque *PdfEditable
_U8 = POINTER(c_ubyte)
_OUTBUF = [POINTER(_U8), POINTER(c_size_t)]


def _bind(name, restype, argtypes):
    fn = getattr(_lib, name)
    fn.restype = restype
    fn.argtypes = argtypes
    return fn


# core
_version = _bind("pdf_version", c_char_p, [])
_last_error = _bind("pdf_last_error_message", c_char_p, [])
_activate_license = _bind("pdf_activate_license", c_int, [c_char_p])
_buffer_free = _bind("pdf_buffer_free", None, [_U8, c_size_t])
# document lifecycle + graphics
_new = _bind("pdf_document_new", _DOC, [])
_free = _bind("pdf_document_free", None, [_DOC])
_add_page = _bind("pdf_document_add_page", c_int, [_DOC])
_add_page_sized = _bind("pdf_document_add_page_sized", c_int, [_DOC, c_double, c_double])
_page_count = _bind("pdf_document_page_count", c_int, [_DOC])
_set_fill = _bind("pdf_page_set_fill_rgb", c_int, [_DOC, c_double, c_double, c_double])
_set_stroke = _bind("pdf_page_set_stroke_rgb", c_int, [_DOC, c_double, c_double, c_double])
_set_lw = _bind("pdf_page_set_line_width", c_int, [_DOC, c_double])
_rect = _bind("pdf_page_rect", c_int, [_DOC, c_double, c_double, c_double, c_double])
_fill = _bind("pdf_page_fill", c_int, [_DOC])
_stroke = _bind("pdf_page_stroke", c_int, [_DOC])
_save = _bind("pdf_document_save", c_int, [_DOC, c_char_p])
_write = _bind("pdf_document_write", c_int, [_DOC, *_OUTBUF])
# config
_pdfa = _bind("pdf_document_pdfa", c_int, [_DOC])
_pdfa_level = _bind("pdf_document_pdfa_level", c_int, [_DOC, c_int])
_tagged = _bind("pdf_document_tagged", c_int, [_DOC])
_set_version = _bind("pdf_document_set_version", c_int, [_DOC, c_int])
_set_size = _bind("pdf_document_set_default_size", c_int, [_DOC, c_double, c_double])
_set_info = _bind(
    "pdf_document_set_info",
    c_int,
    [_DOC, c_char_p, c_char_p, c_char_p, c_char_p, c_char_p],
)
# fonts + text
_add_font_file = _bind("pdf_document_add_font_file", c_int, [_DOC, c_char_p, POINTER(c_int)])
_add_font = _bind("pdf_document_add_font", c_int, [_DOC, _U8, c_size_t, POINTER(c_int)])
_show_text = _bind(
    "pdf_page_show_text",
    c_int,
    [_DOC, c_int, c_double, c_double, c_double, c_char_p, c_int],
)
_paragraph = _bind(
    "pdf_page_paragraph",
    c_int,
    [_DOC, c_int, c_double, c_double, c_double, c_double, c_int, c_char_p],
)
# images
_add_image_file = _bind("pdf_document_add_image_file", c_int, [_DOC, c_char_p, POINTER(c_int)])
_add_image_png = _bind("pdf_document_add_image_png", c_int, [_DOC, _U8, c_size_t, POINTER(c_int)])
_add_image_jpeg = _bind("pdf_document_add_image_jpeg", c_int, [_DOC, _U8, c_size_t, POINTER(c_int)])
_draw_image = _bind(
    "pdf_page_draw_image", c_int, [_DOC, c_int, c_double, c_double, c_double, c_double]
)
_figure = _bind(
    "pdf_page_figure",
    c_int,
    [_DOC, c_int, c_double, c_double, c_double, c_double, c_char_p],
)
# attachments + forms
_attach = _bind(
    "pdf_document_attach_file",
    c_int,
    [_DOC, c_char_p, c_char_p, _U8, c_size_t, c_int, c_char_p],
)
_text_field = _bind(
    "pdf_document_text_field",
    c_int,
    [_DOC, c_char_p, c_size_t, c_double, c_double, c_double, c_double, c_char_p, c_double],
)
_checkbox = _bind(
    "pdf_document_checkbox",
    c_int,
    [_DOC, c_char_p, c_size_t, c_double, c_double, c_double, c_double, c_int],
)
_dropdown = _bind(
    "pdf_document_dropdown",
    c_int,
    [_DOC, c_char_p, c_size_t, c_double, c_double, c_double, c_double, c_char_p, c_int, c_double],
)
_radio_group = _bind(
    "pdf_document_radio_group",
    c_int,
    [_DOC, c_char_p, c_size_t, c_size_t, POINTER(c_double), POINTER(c_char_p), c_int],
)
# editable
_ed_load = _bind("pdf_editable_load", _ED, [_U8, c_size_t])
_ed_load_pw = _bind("pdf_editable_load_password", _ED, [_U8, c_size_t, c_char_p])
_ed_free = _bind("pdf_editable_free", None, [_ED])
_ed_page_count = _bind("pdf_editable_page_count", c_int, [_ED])
_ed_merge = _bind("pdf_editable_merge", c_int, [_ED, _ED])
_ed_rotate = _bind("pdf_editable_rotate_page", c_int, [_ED, c_size_t, c_int])
_ed_delete = _bind("pdf_editable_delete_page", c_int, [_ED, c_size_t])
_ed_reorder = _bind("pdf_editable_reorder_pages", c_int, [_ED, POINTER(c_size_t), c_size_t])
_ed_extract = _bind(
    "pdf_editable_extract_pages", c_int, [_ED, POINTER(c_size_t), c_size_t, POINTER(_ED)]
)
_ed_set_info = _bind("pdf_editable_set_info", c_int, [_ED, c_char_p, c_char_p])
_ed_get_info = _bind("pdf_editable_get_info", c_int, [_ED, c_char_p, *_OUTBUF])
_ed_set_xmp = _bind("pdf_editable_set_xmp", c_int, [_ED, _U8, c_size_t])
_ed_overlay = _bind("pdf_editable_overlay_page", c_int, [_ED, c_size_t, _U8, c_size_t])
_ed_fill = _bind("pdf_editable_fill_text_field", c_int, [_ED, c_char_p, c_char_p, POINTER(c_int)])
_ed_optimize = _bind("pdf_editable_optimize", c_int, [_ED])
_ed_compact = _bind("pdf_editable_compact", c_int, [_ED, c_int])
_ed_encrypt = _bind("pdf_editable_encrypt", c_int, [_ED, c_int, c_char_p, c_char_p, c_int])
_ed_to_bytes = _bind("pdf_editable_to_bytes", c_int, [_ED, *_OUTBUF])
_ed_incremental = _bind("pdf_editable_to_bytes_incremental", c_int, [_ED, _U8, c_size_t, *_OUTBUF])
_ed_save = _bind("pdf_editable_save", c_int, [_ED, c_char_p])
# extract + sign
_extract_text = _bind("pdf_extract_text", c_int, [_U8, c_size_t, *_OUTBUF])
_sign = _bind(
    "pdf_sign",
    c_int,
    [_U8, c_size_t, _U8, c_size_t, _U8, c_size_t, c_char_p, c_char_p, c_char_p, c_int, *_OUTBUF],
)
_timestamp = _bind(
    "pdf_timestamp",
    c_int,
    [_U8, c_size_t, _U8, c_size_t, _U8, c_size_t, c_char_p, *_OUTBUF],
)
_add_dss = _bind(
    "pdf_add_dss",
    c_int,
    [
        _U8, c_size_t,
        POINTER(_U8), POINTER(c_size_t), c_size_t,
        POINTER(_U8), POINTER(c_size_t), c_size_t,
        *_OUTBUF,
    ],
)


# ---- helpers ---------------------------------------------------------------


def version() -> str:
    """Native library version string."""
    return _version().decode("utf-8")


def activate_license(token: str) -> None:
    """Activate a license token, unlocking the corporate features it grants
    (PDF/A, signatures, encryption, accessibility). Raises :class:`PdfError`
    if the token is forged, expired or malformed."""
    _check(_activate_license(_enc(token)))


def _check(status: int) -> None:
    if status == 0:
        return
    msg = _last_error()
    detail = msg.decode("utf-8", "replace") if msg else "unknown error"
    raise PdfError(f"PdfStatus={status}: {detail}")


def _enc(s) -> bytes | None:
    return None if s is None else str(s).encode("utf-8")


def _take(call) -> bytes:
    """Invoke an out-buffer producer ``call(byref(ptr), byref(len))`` and return
    the bytes, always freeing the native buffer."""
    ptr = _U8()
    length = c_size_t(0)
    _check(call(byref(ptr), byref(length)))
    try:
        if not ptr or length.value == 0:
            return b""
        return bytes(ctypes.cast(ptr, POINTER(c_ubyte * length.value)).contents)
    finally:
        _buffer_free(ptr, length)


def _as_u8(data: bytes):
    """A ``(ptr, len, keepalive)`` triple for a read-only byte buffer."""
    if not data:
        return (None, 0, None)
    arr = (c_ubyte * len(data)).from_buffer_copy(data)
    return (ctypes.cast(arr, _U8), len(data), arr)


# ---- Document (authoring) --------------------------------------------------


class Document:
    """A PDF document being authored. Use as a context manager."""

    def __init__(self) -> None:
        h = _new()
        if not h:
            raise PdfError("pdf_document_new returned NULL")
        self._h = h

    def __enter__(self) -> "Document":
        return self

    def __exit__(self, *exc) -> None:
        self.close()

    def close(self) -> None:
        if getattr(self, "_h", None):
            _free(self._h)
            self._h = None

    def _ptr(self):
        if not self._h:
            raise PdfError("operation on a closed Document")
        return self._h

    # configuration
    def pdfa(self, level: PdfaLevel | None = None) -> "Document":
        if level is None:
            _check(_pdfa(self._ptr()))
        else:
            _check(_pdfa_level(self._ptr(), int(level)))
        return self

    def tagged(self) -> "Document":
        _check(_tagged(self._ptr()))
        return self

    def set_version(self, v: int) -> "Document":
        _check(_set_version(self._ptr(), int(v)))
        return self

    def set_default_size(self, width: float, height: float) -> "Document":
        _check(_set_size(self._ptr(), width, height))
        return self

    def set_info(
        self,
        title=None,
        author=None,
        subject=None,
        keywords=None,
        creator=None,
    ) -> "Document":
        _check(
            _set_info(
                self._ptr(),
                _enc(title),
                _enc(author),
                _enc(subject),
                _enc(keywords),
                _enc(creator),
            )
        )
        return self

    # pages + graphics
    def add_page(self, size: tuple[float, float] | None = None) -> "Document":
        if size is None:
            _check(_add_page(self._ptr()))
        else:
            _check(_add_page_sized(self._ptr(), size[0], size[1]))
        return self

    def set_fill_rgb(self, r, g, b) -> "Document":
        _check(_set_fill(self._ptr(), r, g, b))
        return self

    def set_stroke_rgb(self, r, g, b) -> "Document":
        _check(_set_stroke(self._ptr(), r, g, b))
        return self

    def set_line_width(self, w) -> "Document":
        _check(_set_lw(self._ptr(), w))
        return self

    def rect(self, x, y, w, h) -> "Document":
        _check(_rect(self._ptr(), x, y, w, h))
        return self

    def fill(self) -> "Document":
        _check(_fill(self._ptr()))
        return self

    def stroke(self) -> "Document":
        _check(_stroke(self._ptr()))
        return self

    # fonts + text
    def add_font_file(self, path) -> int:
        fid = c_int(-1)
        _check(_add_font_file(self._ptr(), _enc(path), byref(fid)))
        return fid.value

    def add_font(self, data: bytes) -> int:
        ptr, n, _keep = _as_u8(bytes(data))
        fid = c_int(-1)
        _check(_add_font(self._ptr(), ptr, n, byref(fid)))
        return fid.value

    def show_text(self, font: int, size: float, x: float, y: float, text: str,
                  heading_level: int = 0) -> "Document":
        _check(_show_text(self._ptr(), font, size, x, y, _enc(text), heading_level))
        return self

    def paragraph(self, font: int, size: float, x: float, y: float, width: float,
                  text: str, align: Align = Align.LEFT) -> "Document":
        _check(_paragraph(self._ptr(), font, size, x, y, width, int(align), _enc(text)))
        return self

    # images
    def add_image_file(self, path) -> int:
        iid = c_int(-1)
        _check(_add_image_file(self._ptr(), _enc(path), byref(iid)))
        return iid.value

    def add_image_png(self, data: bytes) -> int:
        ptr, n, _keep = _as_u8(bytes(data))
        iid = c_int(-1)
        _check(_add_image_png(self._ptr(), ptr, n, byref(iid)))
        return iid.value

    def add_image_jpeg(self, data: bytes) -> int:
        ptr, n, _keep = _as_u8(bytes(data))
        iid = c_int(-1)
        _check(_add_image_jpeg(self._ptr(), ptr, n, byref(iid)))
        return iid.value

    def draw_image(self, image: int, x, y, w, h) -> "Document":
        _check(_draw_image(self._ptr(), image, x, y, w, h))
        return self

    def figure(self, image: int, x, y, w, h, alt: str) -> "Document":
        _check(_figure(self._ptr(), image, x, y, w, h, _enc(alt)))
        return self

    # attachments
    def attach_file(self, name: str, mime: str, data: bytes,
                    relationship: AFRelationship = AFRelationship.SOURCE,
                    description: str = "") -> "Document":
        ptr, n, _keep = _as_u8(bytes(data))
        _check(_attach(self._ptr(), _enc(name), _enc(mime), ptr, n,
                       int(relationship), _enc(description)))
        return self

    # forms
    def text_field(self, name, page, rect, value="", size=0.0) -> "Document":
        x0, y0, x1, y1 = rect
        _check(_text_field(self._ptr(), _enc(name), page, x0, y0, x1, y1, _enc(value), size))
        return self

    def checkbox(self, name, page, rect, checked=False) -> "Document":
        x0, y0, x1, y1 = rect
        _check(_checkbox(self._ptr(), _enc(name), page, x0, y0, x1, y1, 1 if checked else 0))
        return self

    def dropdown(self, name, page, rect, options, selected=None, size=0.0) -> "Document":
        x0, y0, x1, y1 = rect
        joined = "\n".join(options)
        sel = -1 if selected is None else int(selected)
        _check(_dropdown(self._ptr(), _enc(name), page, x0, y0, x1, y1, _enc(joined), sel, size))
        return self

    def radio_group(self, name, page, buttons, selected=None) -> "Document":
        count = len(buttons)
        rects = (c_double * (count * 4))()
        exports = (c_char_p * count)()
        keep = []
        for i, (rect, export) in enumerate(buttons):
            rects[i * 4], rects[i * 4 + 1], rects[i * 4 + 2], rects[i * 4 + 3] = rect
            b = _enc(export)
            keep.append(b)
            exports[i] = b
        sel = -1 if selected is None else int(selected)
        _check(_radio_group(self._ptr(), _enc(name), page, count, rects, exports, sel))
        return self

    # output
    @property
    def page_count(self) -> int:
        return _page_count(self._ptr())

    def to_bytes(self) -> bytes:
        return _take(lambda p, n: _write(self._ptr(), p, n))

    def save(self, path) -> None:
        _check(_save(self._ptr(), _enc(path)))


# ---- EditableDoc (manipulation) -------------------------------------------


class EditableDoc:
    """An existing PDF loaded for manipulation. Use as a context manager."""

    def __init__(self, handle) -> None:
        if not handle:
            raise PdfError(_last_error_text())
        self._h = handle

    @classmethod
    def load(cls, data: bytes, password: str | None = None) -> "EditableDoc":
        ptr, n, _keep = _as_u8(bytes(data))
        if password is None:
            return cls(_ed_load(ptr, n))
        return cls(_ed_load_pw(ptr, n, _enc(password)))

    @classmethod
    def load_file(cls, path, password: str | None = None) -> "EditableDoc":
        return cls.load(Path(path).read_bytes(), password)

    def __enter__(self) -> "EditableDoc":
        return self

    def __exit__(self, *exc) -> None:
        self.close()

    def close(self) -> None:
        if getattr(self, "_h", None):
            _ed_free(self._h)
            self._h = None

    def _ptr(self):
        if not self._h:
            raise PdfError("operation on a closed EditableDoc")
        return self._h

    @property
    def page_count(self) -> int:
        return _ed_page_count(self._ptr())

    def merge(self, other: "EditableDoc") -> "EditableDoc":
        _check(_ed_merge(self._ptr(), other._ptr()))
        return self

    def rotate_page(self, index: int, degrees: int) -> "EditableDoc":
        _check(_ed_rotate(self._ptr(), index, degrees))
        return self

    def delete_page(self, index: int) -> "EditableDoc":
        _check(_ed_delete(self._ptr(), index))
        return self

    def reorder_pages(self, order: list[int]) -> "EditableDoc":
        arr = (c_size_t * len(order))(*order)
        _check(_ed_reorder(self._ptr(), arr, len(order)))
        return self

    def extract_pages(self, indices: list[int]) -> "EditableDoc":
        arr = (c_size_t * len(indices))(*indices)
        out = _ED()
        _check(_ed_extract(self._ptr(), arr, len(indices), byref(out)))
        return EditableDoc(out)

    def set_info(self, key: str, value: str) -> "EditableDoc":
        _check(_ed_set_info(self._ptr(), _enc(key), _enc(value)))
        return self

    def get_info(self, key: str) -> str:
        return _take(lambda p, n: _ed_get_info(self._ptr(), _enc(key), p, n)).decode("utf-8")

    def set_xmp(self, xml: bytes) -> "EditableDoc":
        ptr, n, _keep = _as_u8(bytes(xml))
        _check(_ed_set_xmp(self._ptr(), ptr, n))
        return self

    def overlay_page(self, index: int, content: bytes) -> "EditableDoc":
        ptr, n, _keep = _as_u8(bytes(content))
        _check(_ed_overlay(self._ptr(), index, ptr, n))
        return self

    def fill_text_field(self, name: str, value: str) -> bool:
        found = c_int(0)
        _check(_ed_fill(self._ptr(), _enc(name), _enc(value), byref(found)))
        return bool(found.value)

    def optimize(self) -> "EditableDoc":
        _check(_ed_optimize(self._ptr()))
        return self

    def compact(self, on: bool = True) -> "EditableDoc":
        _check(_ed_compact(self._ptr(), 1 if on else 0))
        return self

    def encrypt(self, user: str = "", owner: str = "",
                method: Encryption = Encryption.AES256, read_only: bool = False) -> "EditableDoc":
        _check(_ed_encrypt(self._ptr(), int(method), _enc(user), _enc(owner), 1 if read_only else 0))
        return self

    def to_bytes(self) -> bytes:
        return _take(lambda p, n: _ed_to_bytes(self._ptr(), p, n))

    def to_bytes_incremental(self, original: bytes) -> bytes:
        ptr, n, _keep = _as_u8(bytes(original))
        return _take(lambda p, ln: _ed_incremental(self._ptr(), ptr, n, p, ln))

    def save(self, path) -> None:
        _check(_ed_save(self._ptr(), _enc(path)))


def _last_error_text() -> str:
    msg = _last_error()
    return msg.decode("utf-8", "replace") if msg else "operation failed"


# ---- module-level functions ------------------------------------------------


def extract_text(data: bytes) -> str:
    """Extract a document's text (Unicode via ``ToUnicode``)."""
    ptr, n, _keep = _as_u8(bytes(data))
    return _take(lambda p, ln: _extract_text(ptr, n, p, ln)).decode("utf-8")


def sign(pdf: bytes, key_der: bytes, cert_der: bytes, *, reason=None,
         location=None, name=None, pades=False) -> bytes:
    """Sign ``pdf`` (PKCS#7 detached, incremental update). ``pades=True`` →
    PAdES-B-B."""
    pp, pn, _k1 = _as_u8(bytes(pdf))
    kp, kn, _k2 = _as_u8(bytes(key_der))
    cp, cn, _k3 = _as_u8(bytes(cert_der))
    return _take(
        lambda p, ln: _sign(pp, pn, kp, kn, cp, cn, _enc(reason), _enc(location),
                            _enc(name), 1 if pades else 0, p, ln)
    )


def timestamp(pdf: bytes, tsa_key_der: bytes, tsa_cert_der: bytes, *, date=None) -> bytes:
    """Append a document timestamp (``/DocTimeStamp``, PAdES-B-LTA)."""
    pp, pn, _k1 = _as_u8(bytes(pdf))
    kp, kn, _k2 = _as_u8(bytes(tsa_key_der))
    cp, cn, _k3 = _as_u8(bytes(tsa_cert_der))
    return _take(lambda p, ln: _timestamp(pp, pn, kp, kn, cp, cn, _enc(date), p, ln))


def add_dss(pdf: bytes, certs=(), crls=()) -> bytes:
    """Append a Document Security Store (``/DSS``, PAdES-B-LT)."""
    pp, pn, _k0 = _as_u8(bytes(pdf))

    def arrays(items):
        items = [bytes(x) for x in items]
        keep = [(c_ubyte * len(x)).from_buffer_copy(x) for x in items]
        ptrs = (_U8 * len(items))(*[ctypes.cast(k, _U8) for k in keep])
        lens = (c_size_t * len(items))(*[len(x) for x in items])
        return ptrs, lens, len(items), keep

    cp, cl, cc, _kc = arrays(certs)
    rp, rl, rc, _kr = arrays(crls)
    return _take(lambda p, ln: _add_dss(pp, pn, cp, cl, cc, rp, rl, rc, p, ln))

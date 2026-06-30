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
    CFUNCTYPE,
    POINTER,
    byref,
    c_char_p,
    c_double,
    c_int,
    c_size_t,
    c_ubyte,
    c_void_p,
)
from dataclasses import dataclass
from enum import IntEnum
from pathlib import Path

__all__ = [
    "Document",
    "EditableDoc",
    "Bookmark",
    "PdfError",
    "PdfaLevel",
    "Align",
    "AFRelationship",
    "Encryption",
    "FacturxProfile",
    "Certify",
    "SignaturePolicy",
    "SigningOptions",
    "SignatureField",
    "SigningSession",
    "TextHit",
    "PdfRect",
    "PageGeometry",
    "PdfOverview",
    "version",
    "library_path",
    "activate_license",
    "extract_text",
    "extract_page_text",
    "find_text",
    "measure_pages",
    "measure_page",
    "inspect",
    "extract_images_to_dir",
    "render_page_to_png",
    "page_count",
    "verify_signatures",
    "sign",
    "timestamp",
    "add_dss",
    "sign_with",
    "begin_signing",
    "complete_signature",
    "list_signatures",
    "begin_timestamp",
    "timestamp_request",
    "timestamp_token_from_response",
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
    A4 = 5  # PDF/A-4 (ISO 19005-4), based on PDF 2.0
    A4E = 6  # PDF/A-4e (engineering)
    A4F = 7  # PDF/A-4f (embedded files)


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


class FacturxProfile(IntEnum):
    MINIMUM = 0
    BASIC_WL = 1
    BASIC = 2
    EN16931 = 3
    EXTENDED = 4


# ---- deferred / external (HSM) signing — issue #41 P0 ----------------------


class Certify(IntEnum):
    """DocMDP certification level applied by the first (certifying) signature."""

    NONE = 0  # not a certifying signature
    LOCKED = 1  # /P 1 — no changes permitted after signing
    FORMS = 2  # /P 2 — form-filling and signing permitted
    FORMS_AND_ANNOTATIONS = 3  # /P 3 — form-filling, signing and annotations


@dataclass
class SignaturePolicy:
    """A signature-policy identifier (PAdES-EPES / ICP-Brasil AD-RB)."""

    oid: str
    """The policy OID (dotted-decimal), e.g. the ICP-Brasil AD-RB OID."""
    hash: bytes
    """The policy document hash (under :attr:`hash_algorithm_oid`)."""
    hash_algorithm_oid: str | None = None
    """Hash algorithm OID; ``None`` = SHA-256."""
    uri: str | None = None
    """Optional SPURI qualifier — where the policy can be retrieved."""


@dataclass
class SigningOptions:
    """Options for deferred / external signing (issue #41 P0)."""

    reason: str | None = None
    location: str | None = None
    name: str | None = None
    pades: bool = False
    """Produce a PAdES-B-B signature (``ETSI.CAdES.detached``)."""
    certify: Certify = Certify.NONE
    """Certify the document (DocMDP) — use only on the first signature."""
    container_size: int = 0
    """Reserved ``/Contents`` bytes; 0 = default (8192). Raise for large
    cloud-HSM CMS containers."""
    policy: SignaturePolicy | None = None
    """Signature-policy identifier (PAdES-EPES); ``None`` = none."""
    visible: bool = False
    """Draw a visible signature appearance using the ``visible_*`` fields."""
    visible_page: int = 0
    """0-based page index for the visible appearance."""
    visible_rect: tuple[float, float, float, float] = (0.0, 0.0, 0.0, 0.0)
    """Appearance rectangle ``[x0, y0, x1, y1]`` in page points."""
    visible_text: str | None = None
    """Appearance text lines, separated by ``\\n``; ``None`` = none."""
    visible_image: bytes | None = None
    """PNG/JPEG bytes of a handwritten-signature image; ``None`` = none."""


@dataclass
class SignatureField:
    """A signature field discovered in a PDF (pre-signing inventory)."""

    name: str
    signed: bool


@dataclass
class TextHit:
    """One positional match from :func:`find_text` (coords in PDF points,
    origin lower-left)."""

    page: int
    text: str
    x: float
    y: float
    width: float
    height: float


@dataclass
class PdfRect:
    """A rectangle in PDF user space (points, origin lower-left)."""

    x0: float
    y0: float
    x1: float
    y1: float

    @property
    def width(self) -> float:
        """Width of the rectangle (non-negative)."""
        return abs(self.x1 - self.x0)

    @property
    def height(self) -> float:
        """Height of the rectangle (non-negative)."""
        return abs(self.y1 - self.y0)


@dataclass
class PageGeometry:
    """Read-only geometry of one page (from :func:`measure_page` /
    :func:`measure_pages`). Sizes are in PDF points; :attr:`width`/:attr:`height`
    ignore rotation while :attr:`rotated_width`/:attr:`rotated_height` account
    for it (swapped for 90/270 pages)."""

    page: int
    width: float
    height: float
    rotation: int
    rotated_width: float
    rotated_height: float
    media_box: PdfRect
    crop_box: PdfRect


@dataclass
class PdfOverview:
    """A non-mutating summary of a PDF (from :func:`inspect`). Works even on
    password-protected files (the encryption fields are still reported)."""

    version: str
    pdfa_level: str | None
    encrypted: bool
    encryption: str
    requires_password: bool
    page_count: int


class SigningSession:
    """An in-progress two-phase (Model B) signature.

    :attr:`document` holds the prepared PDF (with a zero-filled ``/Contents``
    placeholder) and :attr:`bytes` / :attr:`to_be_signed` the exact bytes the
    signature covers. Hand :attr:`hash` to a remote signer, build the DER CMS /
    PKCS#7 container, then call :meth:`complete`. The key never reaches this
    library.
    """

    def __init__(self, document: bytes, to_be_signed: bytes) -> None:
        self.document = document
        self.to_be_signed = to_be_signed
        self.bytes = to_be_signed

    @property
    def hash(self) -> bytes:
        """SHA-256 of :attr:`to_be_signed` — the value an HSM signs."""
        import hashlib

        return hashlib.sha256(self.to_be_signed).digest()

    def complete(self, container: bytes) -> bytes:
        """Phase 2: embed a finished DER CMS / PKCS#7 ``container``, returning
        the final signed PDF."""
        return complete_signature(self.document, container)


class Bookmark:
    """A document outline entry. Nest with :meth:`child` to build a tree."""

    def __init__(self, title: str, page: int, top: float | None = None,
                 children: list["Bookmark"] | None = None) -> None:
        self.title = title
        self.page = page
        self.top = top
        self.children: list[Bookmark] = list(children or [])

    def child(self, bookmark: "Bookmark") -> "Bookmark":
        self.children.append(bookmark)
        return self

    def _flatten(self, level: int, out: list) -> None:
        out.append((level, self.title, self.page, self.top))
        for c in self.children:
            c._flatten(level + 1, out)


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
_extract_page_text = _bind(
    "pdf_extract_page_text", c_int, [_U8, c_size_t, c_size_t, *_OUTBUF]
)
_extract_images_to_dir = _bind(
    "pdf_extract_images_to_dir", c_int, [_U8, c_size_t, c_char_p, POINTER(c_size_t)]
)
_render_page_to_png = _bind(
    "pdf_render_page_to_png", c_int, [_U8, c_size_t, c_size_t, c_double, *_OUTBUF]
)
_render_page_count = _bind("pdf_page_count", c_int, [_U8, c_size_t, POINTER(c_size_t)])
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
# Tier 1: hyperlinks + bookmarks (Document)
_link_uri = _bind(
    "pdf_page_link_uri", c_int, [_DOC, c_double, c_double, c_double, c_double, c_char_p]
)
_link_to_page = _bind(
    "pdf_page_link_to_page",
    c_int,
    [_DOC, c_double, c_double, c_double, c_double, c_size_t, c_double, c_int],
)
_add_bookmarks = _bind(
    "pdf_document_add_bookmarks",
    c_int,
    [_DOC, c_size_t, POINTER(c_int), POINTER(c_char_p), POINTER(c_size_t),
     POINTER(c_double), POINTER(c_int)],
)
# Tier 2: ZUGFeRD / Factur-X (Document)
_facturx = _bind("pdf_document_facturx", c_int, [_DOC, _U8, c_size_t, c_int])
# Tier 1: form fill + flatten + watermark (EditableDoc)
_ed_set_checkbox = _bind("pdf_editable_set_checkbox", c_int, [_ED, c_char_p, c_int, POINTER(c_int)])
_ed_set_radio = _bind(
    "pdf_editable_set_radio", c_int, [_ED, c_char_p, c_char_p, POINTER(c_int)]
)
_ed_set_choice = _bind(
    "pdf_editable_set_choice", c_int, [_ED, c_char_p, c_char_p, POINTER(c_int)]
)
_ed_flatten = _bind("pdf_editable_flatten_forms", c_int, [_ED])
_ed_field_names = _bind("pdf_editable_field_names", c_int, [_ED, *_OUTBUF])
_ed_watermark_text = _bind(
    "pdf_editable_watermark_text",
    c_int,
    [_ED, c_char_p, c_double, c_double, c_double, c_double, c_double, c_double, c_int],
)
_ed_watermark_image = _bind(
    "pdf_editable_watermark_image_file",
    c_int,
    [_ED, c_char_p, c_double, c_double, c_double, c_double],
)
# Tier 2: redaction + PDF/A conversion (EditableDoc)
_ed_redact = _bind(
    "pdf_editable_redact", c_int, [_ED, c_size_t, POINTER(c_double), c_size_t, POINTER(c_int)]
)
_ed_convert_pdfa = _bind("pdf_editable_convert_to_pdfa", c_int, [_ED, c_int])
# Normalization (issue #41 P1)
_ed_set_version = _bind("pdf_editable_set_version", c_int, [_ED, c_int])
_ed_strip_pdfa = _bind("pdf_editable_strip_pdfa", c_int, [_ED])
_ed_normalize = _bind("pdf_editable_normalize", c_int, [_ED, c_int])
# Tier 2: signature validation (module-level)
_verify_sigs = _bind("pdf_verify_signatures_json", c_int, [_U8, c_size_t, *_OUTBUF])
# Positional text search (issue #41 P1)
_find_text = _bind(
    "pdf_find_text_json", c_int, [_U8, c_size_t, c_char_p, c_int, *_OUTBUF]
)
# Page geometry + document inspection (issue #45 P1)
_measure_pages = _bind("pdf_measure_pages_json", c_int, [_U8, c_size_t, *_OUTBUF])
_inspect = _bind("pdf_inspect_json", c_int, [_U8, c_size_t, *_OUTBUF])
# Stamp filled rect + positioned text (issue #45 P1, EditableDoc)
_ed_fill_rect = _bind(
    "pdf_editable_fill_rect",
    c_int,
    [_ED, c_int, c_double, c_double, c_double, c_double,
     c_double, c_double, c_double, c_double, POINTER(c_int)],
)
_ed_place_text = _bind(
    "pdf_editable_place_text",
    c_int,
    [_ED, c_int, c_double, c_double, c_char_p, c_double,
     c_double, c_double, c_double, c_double, POINTER(c_int)],
)
_ed_place_text_aligned = _bind(
    "pdf_editable_place_text_aligned",
    c_int,
    [_ED, c_int, c_double, c_double, c_char_p, c_double,
     c_double, c_double, c_double, c_double, c_int, POINTER(c_int)],
)
_ed_masked_text = _bind(
    "pdf_editable_masked_text",
    c_int,
    [_ED, c_int, c_double, c_double, c_double, c_double, c_char_p, c_double,
     c_double, c_double, c_double, c_double, c_double, c_double, c_int, POINTER(c_int)],
)
_ed_draw_image = _bind(
    "pdf_editable_draw_image",
    c_int,
    [_ED, c_int, _U8, c_size_t, c_double, c_double, c_double, c_double,
     c_double, POINTER(c_int)],
)
# Network TSA (AD-RT) — issue #41 P1
_timestamp_begin = _bind(
    "pdf_timestamp_begin",
    c_int,
    [_U8, c_size_t, POINTER(_U8), POINTER(c_size_t), POINTER(_U8), POINTER(c_size_t)],
)
_timestamp_request = _bind(
    "pdf_timestamp_request",
    c_int,
    [_U8, c_size_t, _U8, c_size_t, c_int, *_OUTBUF],
)
_timestamp_token_from_response = _bind(
    "pdf_timestamp_token_from_response", c_int, [_U8, c_size_t, *_OUTBUF]
)


# Deferred / external (HSM) signing — issue #41 P0.
class _SigningOptionsNative(ctypes.Structure):
    """Mirrors the C-ABI ``PdfSigningOptions`` — field order is load-bearing."""

    _fields_ = [
        ("reason", c_char_p),
        ("location", c_char_p),
        ("name", c_char_p),
        ("pades", c_int),
        ("certification", c_int),
        ("estimated_size", c_size_t),
        ("policy_oid", c_char_p),
        ("policy_hash", _U8),
        ("policy_hash_len", c_size_t),
        ("policy_hash_alg_oid", c_char_p),
        ("policy_uri", c_char_p),
        # Visible signature + embedded image (issue #41 P1) — appended at the end.
        ("visible", c_int),
        ("vis_page", c_size_t),
        ("vis_rect", c_double * 4),
        ("vis_text", c_char_p),
        ("vis_image", _U8),
        ("vis_image_len", c_size_t),
    ]


# int (*)(void* ctx, const uint8_t* data, uintptr data_len,
#         uint8_t* sig_buf, uintptr sig_cap, uintptr* sig_len)
_SIGN_HASH_FN = CFUNCTYPE(
    c_int, c_void_p, _U8, c_size_t, _U8, c_size_t, POINTER(c_size_t)
)

_sign_begin = _bind(
    "pdf_sign_begin",
    c_int,
    [
        _U8, c_size_t,
        POINTER(_SigningOptionsNative),
        POINTER(_U8), POINTER(c_size_t),
        POINTER(_U8), POINTER(c_size_t),
    ],
)
_sign_complete = _bind(
    "pdf_sign_complete", c_int, [_U8, c_size_t, _U8, c_size_t, *_OUTBUF]
)
_sign_with = _bind(
    "pdf_sign_with",
    c_int,
    [
        _U8, c_size_t,
        _U8, c_size_t,
        POINTER(_U8), POINTER(c_size_t), c_size_t,
        POINTER(_SigningOptionsNative),
        _SIGN_HASH_FN, c_void_p,
        *_OUTBUF,
    ],
)
_list_signatures = _bind("pdf_list_signatures", c_int, [_U8, c_size_t, *_OUTBUF])


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
        """Set the PDF version: 0=1.4, 1=1.5, 2=1.7, 3=2.0."""
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

    # hyperlinks (Tier 1)
    def link_uri(self, rect, uri: str) -> "Document":
        x0, y0, x1, y1 = rect
        _check(_link_uri(self._ptr(), x0, y0, x1, y1, _enc(uri)))
        return self

    def link_to_page(self, rect, page_index: int, top: float | None = None) -> "Document":
        x0, y0, x1, y1 = rect
        _check(_link_to_page(self._ptr(), x0, y0, x1, y1, page_index,
                             0.0 if top is None else top, 0 if top is None else 1))
        return self

    # bookmarks / outline (Tier 1)
    def add_bookmark(self, bookmark: Bookmark) -> "Document":
        entries: list = []
        bookmark._flatten(0, entries)
        n = len(entries)
        levels = (c_int * n)(*[e[0] for e in entries])
        pages = (c_size_t * n)(*[e[2] for e in entries])
        titles = (c_char_p * n)()
        tops = (c_double * n)()
        has_tops = (c_int * n)()
        keep = []
        for i, (_lvl, title, _page, top) in enumerate(entries):
            b = _enc(title)
            keep.append(b)
            titles[i] = b
            if top is None:
                has_tops[i], tops[i] = 0, 0.0
            else:
                has_tops[i], tops[i] = 1, top
        _check(_add_bookmarks(self._ptr(), n, levels, titles, pages, tops, has_tops))
        return self

    # ZUGFeRD / Factur-X (Tier 2)
    def facturx(self, xml: bytes, profile: FacturxProfile = FacturxProfile.EN16931) -> "Document":
        ptr, n, _keep = _as_u8(bytes(xml))
        _check(_facturx(self._ptr(), ptr, n, int(profile)))
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

    def set_checkbox(self, name: str, checked: bool = True) -> bool:
        found = c_int(0)
        _check(_ed_set_checkbox(self._ptr(), _enc(name), 1 if checked else 0, byref(found)))
        return bool(found.value)

    def set_radio(self, name: str, export_value: str) -> bool:
        found = c_int(0)
        _check(_ed_set_radio(self._ptr(), _enc(name), _enc(export_value), byref(found)))
        return bool(found.value)

    def set_choice(self, name: str, value: str) -> bool:
        found = c_int(0)
        _check(_ed_set_choice(self._ptr(), _enc(name), _enc(value), byref(found)))
        return bool(found.value)

    def flatten_forms(self) -> "EditableDoc":
        _check(_ed_flatten(self._ptr()))
        return self

    def field_names(self) -> list[str]:
        text = _take(lambda p, n: _ed_field_names(self._ptr(), p, n)).decode("utf-8")
        return [s for s in text.split("\n") if s]

    def watermark_text(self, text: str, *, size: float = 64.0,
                       color: tuple[float, float, float] = (0.5, 0.5, 0.5),
                       opacity: float = 0.30, rotation_deg: float = 45.0,
                       opaque_background: bool = False) -> "EditableDoc":
        r, g, b = color
        _check(_ed_watermark_text(self._ptr(), _enc(text), size, r, g, b, opacity,
                                  rotation_deg, 1 if opaque_background else 0))
        return self

    def watermark_image_file(self, path, width: float, height: float,
                             opacity: float = 0.30,
                             rotation_deg: float = 0.0) -> "EditableDoc":
        _check(_ed_watermark_image(self._ptr(), _enc(str(path)), width, height,
                                   opacity, rotation_deg))
        return self

    # stamping (issue #45 P1)
    def fill_rect(self, page_index: int, x: float, y: float, width: float,
                  height: float, color: tuple[float, float, float] = (1.0, 1.0, 1.0),
                  opacity: float = 1.0) -> bool:
        """Paint a filled rectangle at ``(x, y)`` sized ``width``×``height`` on
        page ``page_index`` (0-based), in RGB ``color`` (default opaque white) at
        ``opacity``. Coordinates are in the page's VISIBLE space (origin
        lower-left, y up) — the box lands where a viewer sees it regardless of
        ``/Rotate``. The common use is masking a placeholder with an opaque white
        box. Returns whether the page existed."""
        r, g, b = color
        found = c_int(0)
        _check(_ed_fill_rect(self._ptr(), page_index, x, y, width, height,
                             r, g, b, opacity, byref(found)))
        return bool(found.value)

    def place_text(self, page_index: int, x: float, y: float, text: str,
                   size: float = 12.0, color: tuple[float, float, float] = (0.0, 0.0, 0.0),
                   rotation_deg: float = 0.0, align: Align = Align.LEFT) -> bool:
        """Draw a line of positioned text with baseline at ``(x, y)`` on page
        ``page_index`` (0-based), standard Helvetica at ``size`` points in RGB
        ``color``. ``rotation_deg`` rotates the text counter-clockwise about its
        anchor. ``align`` shifts the start point along the baseline so the text is
        ``Align.LEFT`` (start at ``x``), ``Align.RIGHT`` (end at ``x``) or
        ``Align.CENTER`` (centered on ``x``) — ``Align.JUSTIFY`` behaves like left.
        Coordinates are in the page's VISIBLE space (origin lower-left, y up) — the
        text lands where a viewer sees it regardless of ``/Rotate``. Returns
        whether the page existed."""
        r, g, b = color
        found = c_int(0)
        _check(_ed_place_text_aligned(self._ptr(), page_index, x, y, _enc(text), size,
                                      r, g, b, rotation_deg, int(align), byref(found)))
        return bool(found.value)

    def masked_text(self, page_index: int, x: float, y: float, width: float,
                    height: float, text: str, size: float = 12.0,
                    text_color: tuple[float, float, float] = (0.0, 0.0, 0.0),
                    bg_color: tuple[float, float, float] = (1.0, 1.0, 1.0),
                    align: Align = Align.LEFT) -> bool:
        """Draw ``text`` over an opaque background box ``[x, y, x+width, y+height]``
        on page ``page_index`` (0-based): fill the box in ``bg_color`` (default
        white), then write the text (standard Helvetica at ``size`` points in
        ``text_color``, default black) horizontally aligned per ``align`` and
        vertically centered within the box. The classic use is masking a
        placeholder and stamping the real value over it without hand-computing the
        baseline. Coordinates are in the page's VISIBLE space (origin lower-left, y
        up). Returns whether the page existed."""
        tr, tg, tb = text_color
        br, bg, bb = bg_color
        found = c_int(0)
        _check(_ed_masked_text(self._ptr(), page_index, x, y, width, height,
                               _enc(text), size, tr, tg, tb, br, bg, bb,
                               int(align), byref(found)))
        return bool(found.value)

    def draw_image(self, page_index: int, image: bytes, x: float, y: float,
                   width: float, height: float, rotation_deg: float = 0.0) -> bool:
        """Draw an ``image`` (JPEG/PNG bytes, dispatched on signature) on page
        ``page_index`` (0-based) with its lower-left corner at ``(x, y)``, scaled
        to ``width``×``height`` points and rotated ``rotation_deg`` degrees
        counter-clockwise about that corner. Coordinates are in the page's
        VISIBLE space (origin lower-left, y up) — the image lands where a viewer
        sees it regardless of ``/Rotate``. Returns whether the page existed."""
        ptr, n, _keep = _as_u8(bytes(image))
        found = c_int(0)
        _check(_ed_draw_image(self._ptr(), page_index, ptr, n, x, y, width,
                              height, rotation_deg, byref(found)))
        return bool(found.value)

    # normalization (issue #41 P1)
    def set_version(self, version: int) -> "EditableDoc":
        """Set the output PDF version: 0=1.4, 1=1.5, 2=1.7, 3=2.0."""
        _check(_ed_set_version(self._ptr(), int(version)))
        return self

    def strip_pdfa(self) -> "EditableDoc":
        """Strip PDF/A conformance (`/OutputIntents`, XMP `pdfaid`, `/Version`)."""
        _check(_ed_strip_pdfa(self._ptr()))
        return self

    def normalize(self, version: int = 2) -> "EditableDoc":
        """Normalize to a plain PDF at ``version`` (strip PDF/A + set version)."""
        _check(_ed_normalize(self._ptr(), int(version)))
        return self

    def redact(self, page_index: int, rects: list) -> bool:
        n = len(rects)
        flat = (c_double * (n * 4))()
        for i, r in enumerate(rects):
            flat[i * 4], flat[i * 4 + 1], flat[i * 4 + 2], flat[i * 4 + 3] = r
        found = c_int(0)
        _check(_ed_redact(self._ptr(), page_index, flat, n, byref(found)))
        return bool(found.value)

    def convert_to_pdfa(self, level: PdfaLevel = PdfaLevel.A2B) -> "EditableDoc":
        _check(_ed_convert_pdfa(self._ptr(), int(level)))
        return self

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


def extract_page_text(data: bytes, page_index: int) -> str:
    """Extract the text of a single page (0-based ``page_index``), without
    building an intermediate one-page document. Raises :class:`PdfError` if the
    page is out of range."""
    ptr, n, _keep = _as_u8(bytes(data))
    return _take(
        lambda p, ln: _extract_page_text(ptr, n, page_index, p, ln)
    ).decode("utf-8")


def find_text(data: bytes, query: str, case_sensitive: bool = False) -> list[TextHit]:
    """Find every occurrence of ``query`` in ``data``, returning a list of
    :class:`TextHit` with a bounding box (PDF points, origin lower-left). An
    empty list means no match."""
    import json

    ptr, n, _keep = _as_u8(bytes(data))
    js = _take(
        lambda p, ln: _find_text(ptr, n, _enc(query), 1 if case_sensitive else 0, p, ln)
    ).decode("utf-8")
    hits = json.loads(js) if js else []
    return [
        TextHit(
            page=h["page"], text=h["text"], x=h["x"], y=h["y"],
            width=h["width"], height=h["height"],
        )
        for h in hits
    ]


def measure_pages(pdf: bytes) -> list[PageGeometry]:
    """Read per-page geometry from ``pdf`` (size, ``/Rotate``, media/crop boxes).
    Returns one :class:`PageGeometry` per page; coordinates are in PDF points.
    :attr:`PageGeometry.width`/:attr:`~PageGeometry.height` are unrotated;
    :attr:`~PageGeometry.rotated_width`/:attr:`~PageGeometry.rotated_height` are
    swapped for 90/270 pages."""
    import json

    ptr, n, _keep = _as_u8(bytes(pdf))
    js = _take(lambda p, ln: _measure_pages(ptr, n, p, ln)).decode("utf-8")
    pages = json.loads(js) if js else []

    def rect(d: dict, key: str) -> PdfRect:
        v = d.get(key)
        if isinstance(v, list) and len(v) == 4:
            return PdfRect(x0=v[0], y0=v[1], x1=v[2], y1=v[3])
        return PdfRect(0.0, 0.0, 0.0, 0.0)

    return [
        PageGeometry(
            page=g["page"],
            width=g["width"],
            height=g["height"],
            rotation=g["rotation"],
            rotated_width=g["rotatedWidth"],
            rotated_height=g["rotatedHeight"],
            media_box=rect(g, "mediaBox"),
            crop_box=rect(g, "cropBox"),
        )
        for g in pages
    ]


def measure_page(pdf: bytes, index: int) -> PageGeometry:
    """Geometry of a single page (0-based ``index``). Raises :class:`IndexError`
    if ``index`` is out of range."""
    pages = measure_pages(pdf)
    if index < 0 or index >= len(pages):
        raise IndexError(f"page index {index} out of range (0..{len(pages)})")
    return pages[index]


def inspect(pdf: bytes) -> PdfOverview:
    """Inspect ``pdf`` without mutating it: PDF version, PDF/A level (if any),
    encryption posture and page count. Works even on password-protected files
    (the encryption fields are still reported)."""
    import json

    ptr, n, _keep = _as_u8(bytes(pdf))
    js = _take(lambda p, ln: _inspect(ptr, n, p, ln)).decode("utf-8")
    o = json.loads(js) if js else {}
    return PdfOverview(
        version=o.get("version", ""),
        pdfa_level=o.get("pdfaLevel"),
        encrypted=bool(o.get("encrypted", False)),
        encryption=o.get("encryption", ""),
        requires_password=bool(o.get("requiresPassword", False)),
        page_count=int(o.get("pageCount", 0)),
    )


def extract_images_to_dir(data: bytes, out_dir: str) -> int:
    """Extract every raster image from ``data`` into directory ``out_dir``.

    JPEGs are written verbatim as ``.jpg`` (no re-encoding); everything else is
    written as ``.png``. Files are named ``page{N}_{name}.{ext}``. Returns the
    number of images written.
    """
    ptr, n, _keep = _as_u8(bytes(data))
    count = c_size_t(0)
    _check(_extract_images_to_dir(ptr, n, _enc(str(out_dir)), byref(count)))
    return count.value


def render_page_to_png(data: bytes, page: int = 0, dpi: float = 150.0) -> bytes:
    """Render page ``page`` (0-based) of ``data`` to a PNG image at ``dpi``.

    Page rendering is a licensed **Pro** feature: raises :class:`PdfError`
    (``PdfStatus.License``) unless a license granting it is active.
    """
    ptr, n, _keep = _as_u8(bytes(data))
    return _take(lambda p, ln: _render_page_to_png(ptr, n, page, float(dpi), p, ln))


def page_count(data: bytes) -> int:
    """Number of pages in ``data`` (free — no license required)."""
    ptr, n, _keep = _as_u8(bytes(data))
    count = c_size_t(0)
    _check(_render_page_count(ptr, n, byref(count)))
    return count.value


def verify_signatures(data: bytes) -> list[dict]:
    """Validate every signature in ``data``. Returns one dict per signature with
    keys ``field_name``, ``sub_filter``, ``signer``, ``covers_whole_document``,
    ``digest_valid``, ``signature_valid``, ``is_valid`` and ``byte_range``, plus
    the rich certificate fields ``issuer``, ``serial_number``, ``valid_from``,
    ``valid_to``, ``algorithm``, ``signing_time`` (all may be ``None``),
    ``cert_count`` (int) and ``has_timestamp`` (bool). An empty list means the
    document is unsigned."""
    import json

    ptr, n, _keep = _as_u8(bytes(data))
    js = _take(lambda p, ln: _verify_sigs(ptr, n, p, ln)).decode("utf-8")
    return json.loads(js) if js else []


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


# ---- deferred / external (HSM) signing — issue #41 P0 ----------------------


def _build_signing_options(options: "SigningOptions | None"):
    """Marshal :class:`SigningOptions` into a ``_SigningOptionsNative`` plus a
    keep-alive list that must outlive the native call."""
    n = _SigningOptionsNative()
    keep: list = []

    def s(value) -> bytes | None:
        b = _enc(value)
        keep.append(b)
        return b

    if options is None:
        return n, keep
    n.reason = s(options.reason)
    n.location = s(options.location)
    n.name = s(options.name)
    n.pades = 1 if options.pades else 0
    n.certification = int(options.certify)
    n.estimated_size = options.container_size if options.container_size and options.container_size > 0 else 0
    pol = options.policy
    if pol is not None:
        n.policy_oid = s(pol.oid)
        if pol.hash:
            arr = (c_ubyte * len(pol.hash)).from_buffer_copy(bytes(pol.hash))
            keep.append(arr)
            n.policy_hash = ctypes.cast(arr, _U8)
            n.policy_hash_len = len(pol.hash)
        n.policy_hash_alg_oid = s(pol.hash_algorithm_oid)
        n.policy_uri = s(pol.uri)
    # Visible signature appearance (issue #41 P1).
    n.visible = 1 if options.visible else 0
    n.vis_page = int(options.visible_page)
    n.vis_rect = (c_double * 4)(*[float(c) for c in options.visible_rect])
    n.vis_text = s(options.visible_text)
    if options.visible_image:
        img = bytes(options.visible_image)
        arr = (c_ubyte * len(img)).from_buffer_copy(img)
        keep.append(arr)
        n.vis_image = ctypes.cast(arr, _U8)
        n.vis_image_len = len(img)
    return n, keep


def _copy_free(ptr, length) -> bytes:
    """Copy a native out-buffer into a ``bytes`` and free it."""
    try:
        if not ptr or length.value == 0:
            return b""
        return bytes(ctypes.cast(ptr, POINTER(c_ubyte * length.value)).contents)
    finally:
        _buffer_free(ptr, length)


def sign_with(pdf: bytes, cert_der: bytes, sign_hash, chain=(), options=None) -> bytes:
    """**Model A — remote signer.** Sign ``pdf`` without handing this library a
    key: it builds the CMS signed attributes and calls ``sign_hash`` for the raw
    RSA signature, then assembles and embeds the CMS.

    ``sign_hash`` is a callable ``(data: bytes) -> bytes`` producing the raw
    RSA PKCS#1 v1.5 signature over SHA-256 of ``data`` (typically by calling a
    remote HSM — Azure Key Vault, VIDaaS, BirdID). The private key never reaches
    this library. ``cert_der`` is the signer certificate; ``chain`` are
    intermediate certificates (DER), supplied independently of the key.
    """
    pp, pn, _k1 = _as_u8(bytes(pdf))
    cp, cn, _k2 = _as_u8(bytes(cert_der))
    chain = [bytes(x) for x in chain]
    chain_keep = [(c_ubyte * len(x)).from_buffer_copy(x) for x in chain]
    chain_ptrs = (
        (_U8 * len(chain))(*[ctypes.cast(k, _U8) for k in chain_keep])
        if chain else None
    )
    chain_lens = (c_size_t * len(chain))(*[len(x) for x in chain]) if chain else None
    opts, _keep = _build_signing_options(options)

    def _trampoline(_ctx, data, data_len, sig_buf, sig_cap, sig_len):
        # Guard the C boundary: a Python exception must never unwind into Rust.
        try:
            buf = bytes(ctypes.cast(data, POINTER(c_ubyte * data_len)).contents)
            sig = bytes(sign_hash(buf))
            if len(sig) > sig_cap:
                return 2  # buffer too small
            ctypes.memmove(sig_buf, sig, len(sig))
            sig_len[0] = len(sig)
            return 0
        except Exception:  # noqa: BLE001 — must not let it cross the FFI line
            return 1  # signer threw

    cb = _SIGN_HASH_FN(_trampoline)
    return _take(
        lambda p, ln: _sign_with(
            pp, pn, cp, cn, chain_ptrs, chain_lens, len(chain),
            byref(opts), cb, None, p, ln,
        )
    )


def begin_signing(pdf: bytes, options=None) -> SigningSession:
    """**Model B — two-phase signing, phase 1.** Prepare ``pdf`` for deferred
    signing and return a :class:`SigningSession`. Hand its :attr:`~SigningSession.hash`
    to a remote signer, build the CMS container, then call
    :meth:`SigningSession.complete`. The key never reaches this library."""
    pp, pn, _k = _as_u8(bytes(pdf))
    opts, _keep = _build_signing_options(options)
    doc_ptr, doc_len = _U8(), c_size_t(0)
    tbs_ptr, tbs_len = _U8(), c_size_t(0)
    _check(
        _sign_begin(
            pp, pn, byref(opts),
            byref(doc_ptr), byref(doc_len),
            byref(tbs_ptr), byref(tbs_len),
        )
    )
    document = _copy_free(doc_ptr, doc_len)
    tbs = _copy_free(tbs_ptr, tbs_len)
    return SigningSession(document, tbs)


def complete_signature(document: bytes, container: bytes) -> bytes:
    """**Model B — two-phase signing, phase 2.** Embed a complete DER CMS /
    PKCS#7 ``container`` into a prepared ``document`` (from :func:`begin_signing`),
    producing the final signed PDF."""
    dp, dn, _k1 = _as_u8(bytes(document))
    cp, cn, _k2 = _as_u8(bytes(container))
    return _take(lambda p, ln: _sign_complete(dp, dn, cp, cn, p, ln))


def list_signatures(pdf: bytes) -> list[SignatureField]:
    """List the signature fields in ``pdf`` (detect existing signatures before
    signing — the iText ``SignatureUtil.getSignatureNames`` equivalent). An empty
    list means there are no signature fields."""
    pp, pn, _k = _as_u8(bytes(pdf))
    text = _take(lambda p, ln: _list_signatures(pp, pn, p, ln)).decode("utf-8")
    result: list[SignatureField] = []
    for line in text.split("\n"):
        if not line:
            continue
        tab = line.find("\t")
        if tab < 0:
            continue
        result.append(SignatureField(name=line[tab + 1:], signed=line[:tab] == "1"))
    return result


# ---- network TSA (AD-RT) — issue #41 P1 ------------------------------------


def begin_timestamp(pdf: bytes) -> tuple[bytes, bytes]:
    """**Network-TSA, phase 1.** Prepare ``pdf`` for a document timestamp and
    return ``(document, to_be_signed)``: the prepared PDF (with a zero-filled
    ``/Contents`` placeholder) and the exact bytes covered by the timestamp.
    SHA-256 the ``to_be_signed`` bytes to build the TSA request."""
    pp, pn, _k = _as_u8(bytes(pdf))
    doc_ptr, doc_len = _U8(), c_size_t(0)
    tbs_ptr, tbs_len = _U8(), c_size_t(0)
    _check(
        _timestamp_begin(
            pp, pn,
            byref(doc_ptr), byref(doc_len),
            byref(tbs_ptr), byref(tbs_len),
        )
    )
    return _copy_free(doc_ptr, doc_len), _copy_free(tbs_ptr, tbs_len)


def timestamp_request(imprint: bytes, nonce: bytes | None = None,
                      cert_req: bool = True) -> bytes:
    """Build an RFC 3161 ``TimeStampReq`` (DER) for ``imprint`` (the SHA-256 of
    the bytes to timestamp). POST the result to the TSA. ``nonce`` is optional;
    ``cert_req`` asks the TSA to embed its certificate."""
    ip, iln, _k1 = _as_u8(bytes(imprint))
    np_, nln, _k2 = _as_u8(bytes(nonce) if nonce else b"")
    return _take(
        lambda p, ln: _timestamp_request(ip, iln, np_, nln, 1 if cert_req else 0, p, ln)
    )


def timestamp_token_from_response(response: bytes) -> bytes:
    """Extract the ``TimeStampToken`` (a CMS ``ContentInfo``) from a TSA's RFC
    3161 ``TimeStampResp``. Embed the result via :func:`complete_signature`."""
    rp, rn, _k = _as_u8(bytes(response))
    return _take(lambda p, ln: _timestamp_token_from_response(rp, rn, p, ln))

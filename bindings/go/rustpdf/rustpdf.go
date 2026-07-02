// Package rustpdf is an idiomatic Go binding for the rust-pdf core over its C
// ABI (libpdf_ffi), via cgo. It covers the whole product surface: vector
// graphics, embedded fonts and text, paragraphs, images, PDF/A (1b–3a),
// tagged/accessible output, attachments, AcroForm fields, manipulation
// (merge/split/rotate/optimize/incremental update), text extraction, encryption
// and digital signatures — plus feature licensing.
//
// The C header (pdf.h) is vendored alongside these sources, so the module is
// self-contained for `go get`. The native library is linked per build tag:
// link_dist.go (default) statically links the prebuilt libpdf_ffi.a vendored
// under lib/<os>_<arch>/; link_dev.go (build tag `rustpdf_dev`) links the
// dynamic library from the monorepo build tree (../../../target/{debug,release})
// for in-repo development and tests.
package rustpdf

/*
#cgo CFLAGS: -I${SRCDIR}
#include <stdlib.h>
#include "pdf.h"
*/
import "C"

import (
	"encoding/json"
	"runtime"
	"unsafe"
)

// ---- enums -----------------------------------------------------------------

// PdfaLevel is a PDF/A conformance level.
type PdfaLevel int

const (
	A1b PdfaLevel = 0
	A2b PdfaLevel = 1
	A2a PdfaLevel = 2
	A3b PdfaLevel = 3
	A3a PdfaLevel = 4
	// PDF/A-4 (ISO 19005-4), based on PDF 2.0.
	A4  PdfaLevel = 5
	A4e PdfaLevel = 6
	A4f PdfaLevel = 7
)

// Align is a paragraph horizontal alignment.
type Align int

const (
	AlignLeft    Align = 0
	AlignRight   Align = 1
	AlignCenter  Align = 2
	AlignJustify Align = 3
)

// VerticalAnchor says what the y coordinate of a positioned text stamp means
// (EditableDoc.PlaceTextAnchored, and the block anchor of
// EditableDoc.PlaceParagraph). AnchorBaseline is the historical default;
// AnchorTop hangs the text from y (baseline at y − ascent × size, legacy layout engines
// fixed-position layout semantics); AnchorBottom rests the descender line on y.
// AnchorLineTop / AnchorLineBottom use the layout line box (OS/2 win metrics —
// or typo × 1.2 — plus a fixed half-leading of 0.21 em), matching
// legacy fixed-position line placement exactly. Ascent/descent come from the
// selected font (embedded font metrics, or Helvetica AFM).
type VerticalAnchor int

const (
	AnchorBaseline   VerticalAnchor = 0
	AnchorTop        VerticalAnchor = 1
	AnchorBottom     VerticalAnchor = 2
	AnchorLineTop    VerticalAnchor = 3
	AnchorLineBottom VerticalAnchor = 4
)

// VerticalAlign is the vertical alignment of the text line inside an
// EditableDoc.MaskedTextPadded box. VAlignMiddle is the historical cap-height
// centering; VAlignTop hangs the line from the top edge (baseline at
// y + height − ascent × size, top line-alignment in rectangle-based text APIs semantics);
// VAlignBottom rests the descender line on the bottom edge.
type VerticalAlign int

const (
	VAlignTop    VerticalAlign = 0
	VAlignMiddle VerticalAlign = 1
	VAlignBottom VerticalAlign = 2
)

// StampSpace is the coordinate space of the positioned stamping primitives
// (FillRect, PlaceText*, MaskedText*, PlaceParagraph, DrawImage*) — set via
// EditableDoc.SetStampSpace. StampVisible (the default) keeps the historical
// behavior: coordinates in the page's displayed space, compensating /Rotate so
// a rotationDeg = 0 stamp reads upright on screen. StampMedia interprets
// coordinates and rotationDeg in the raw PDF user space (legacy layout engines
// fixed-position layout/rotation semantics), never composing with the
// page's /Rotate or crop offset — use it to reproduce coordinates computed for
// legacy layout engines on rotated (scanned) pages. Watermarks and redaction are unaffected.
type StampSpace int

const (
	StampVisible StampSpace = 0
	StampMedia   StampSpace = 1
)

// ImageAnchor is how a rotated image is anchored at (x, y) in
// EditableDoc.DrawImageAnchored. ImageAnchorCorner (the DrawImage default):
// (x, y) is the image's own lower-left corner and the image sweeps around it
// when rotated. ImageAnchorBoundingBox: the rotated image's bounding box lands
// with its lower-left at (x, y) (bounding-box layout semantics — the drawn pixels
// always sit at/above/right of the anchor).
type ImageAnchor int

const (
	ImageAnchorCorner      ImageAnchor = 0
	ImageAnchorBoundingBox ImageAnchor = 1
)

// AFRelationship is an embedded-file relationship (PDF/A-3 /AFRelationship).
type AFRelationship int

const (
	RelSource      AFRelationship = 0
	RelData        AFRelationship = 1
	RelAlternative AFRelationship = 2
	RelSupplement  AFRelationship = 3
	RelUnspecified AFRelationship = 4
)

// Encryption is a document encryption cipher.
type Encryption int

const (
	RC4    Encryption = 0
	AES128 Encryption = 1
	AES256 Encryption = 2
)

// FacturxProfile is a ZUGFeRD / Factur-X conformance profile.
type FacturxProfile int

const (
	FacturxMinimum  FacturxProfile = 0
	FacturxBasicWL  FacturxProfile = 1
	FacturxBasic    FacturxProfile = 2
	FacturxEN16931  FacturxProfile = 3
	FacturxExtended FacturxProfile = 4
)

// ---- errors ----------------------------------------------------------------

// Error is a failed native call: a PdfStatus code plus the last-error message.
type Error struct {
	Status  int
	Message string
}

func (e *Error) Error() string {
	return "rustpdf: status=" + itoa(e.Status) + ": " + e.Message
}

func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	neg := n < 0
	if neg {
		n = -n
	}
	var b [20]byte
	i := len(b)
	for n > 0 {
		i--
		b[i] = byte('0' + n%10)
		n /= 10
	}
	if neg {
		i--
		b[i] = '-'
	}
	return string(b[i:])
}

func lastError() string {
	p := C.pdf_last_error_message()
	if p == nil {
		return "unknown error"
	}
	return C.GoString(p)
}

func check(st C.PdfStatus) error {
	if st == C.PDF_STATUS_OK {
		return nil
	}
	return &Error{Status: int(st), Message: lastError()}
}

// ---- low-level helpers -----------------------------------------------------

func uptr(b []byte) *C.uint8_t {
	if len(b) == 0 {
		return nil
	}
	return (*C.uint8_t)(unsafe.Pointer(&b[0]))
}

// optCStr returns a C string (or nil for ""), and a free func.
func optCStr(s string) (*C.char, func()) {
	if s == "" {
		return nil, func() {}
	}
	p := C.CString(s)
	return p, func() { C.free(unsafe.Pointer(p)) }
}

// takeBytes runs an out-buffer producer and copies+frees the native buffer.
func takeBytes(call func(out **C.uchar, n *C.uintptr_t) C.PdfStatus) ([]byte, error) {
	var p *C.uchar
	var n C.uintptr_t
	if err := check(call(&p, &n)); err != nil {
		return nil, err
	}
	defer C.pdf_buffer_free(p, n)
	if p == nil || n == 0 {
		return []byte{}, nil
	}
	return C.GoBytes(unsafe.Pointer(p), C.int(n)), nil
}

// ---- package-level functions ----------------------------------------------

// Version returns the native library version string.
func Version() string {
	return C.GoString(C.pdf_version())
}

// ActivateLicense verifies and activates a license token (unlocks PDF/A,
// signing, encryption, accessibility). Tokens may also be supplied via the
// RUSTPDF_LICENSE / RUSTPDF_LICENSE_FILE environment variables (auto-activated).
func ActivateLicense(token string) error {
	c := C.CString(token)
	defer C.free(unsafe.Pointer(c))
	return check(C.pdf_activate_license(c))
}

// ExtractText extracts a document's text (Unicode via ToUnicode).
func ExtractText(pdf []byte) (string, error) {
	b, err := takeBytes(func(out **C.uchar, n *C.uintptr_t) C.PdfStatus {
		st := C.pdf_extract_text(uptr(pdf), C.uintptr_t(len(pdf)), out, n)
		runtime.KeepAlive(pdf)
		return st
	})
	return string(b), err
}

// ExtractPageText extracts the text of a single page (0-based pageIndex) from a
// PDF, decoded to Unicode via each font's ToUnicode map. It returns an error if
// pageIndex is out of range.
func ExtractPageText(pdf []byte, pageIndex int) (string, error) {
	b, err := takeBytes(func(out **C.uchar, n *C.uintptr_t) C.PdfStatus {
		st := C.pdf_extract_page_text(
			uptr(pdf), C.uintptr_t(len(pdf)), C.uintptr_t(pageIndex), out, n)
		runtime.KeepAlive(pdf)
		return st
	})
	return string(b), err
}

// ExtractImagesToDir extracts every raster image from a PDF and writes each
// into the directory dir (which must already exist): JPEG images verbatim as
// .jpg, everything else re-encoded as .png, named page{N}_{name}.{ext}. It
// returns the number of files written.
func ExtractImagesToDir(data []byte, dir string) (int, error) {
	cdir := C.CString(dir)
	defer C.free(unsafe.Pointer(cdir))
	var count C.uintptr_t
	st := C.pdf_extract_images_to_dir(uptr(data), C.uintptr_t(len(data)), cdir, &count)
	runtime.KeepAlive(data)
	if err := check(st); err != nil {
		return 0, err
	}
	return int(count), nil
}

// RenderPageToPng renders page pageIndex (0-based) of pdf to a PNG image at dpi
// dots-per-inch. Page rendering is a licensed Pro feature: it returns an error
// (PdfStatus license) unless a license granting it is active.
func RenderPageToPng(pdf []byte, pageIndex int, dpi float64) ([]byte, error) {
	return takeBytes(func(out **C.uchar, n *C.uintptr_t) C.PdfStatus {
		st := C.pdf_render_page_to_png(
			uptr(pdf), C.uintptr_t(len(pdf)), C.uintptr_t(pageIndex), C.double(dpi), out, n)
		runtime.KeepAlive(pdf)
		return st
	})
}

// PageCount returns the number of pages in pdf (free — no license required).
func PageCount(pdf []byte) (int, error) {
	var count C.uintptr_t
	st := C.pdf_page_count(uptr(pdf), C.uintptr_t(len(pdf)), &count)
	runtime.KeepAlive(pdf)
	if err := check(st); err != nil {
		return 0, err
	}
	return int(count), nil
}

// SignatureReport is the validation result for one signature in a document.
type SignatureReport struct {
	FieldName           *string `json:"field_name"`
	SubFilter           string  `json:"sub_filter"`
	Signer              *string `json:"signer"`
	CoversWholeDocument bool    `json:"covers_whole_document"`
	DigestValid         bool    `json:"digest_valid"`
	SignatureValid      bool    `json:"signature_valid"`
	IsValid             bool    `json:"is_valid"`
	ByteRange           []int   `json:"byte_range"`
	// Certificate / signature details (issue #41 P1; any may be nil/zero).
	Issuer       *string `json:"issuer"`
	SerialNumber *string `json:"serial_number"`
	ValidFrom    *string `json:"valid_from"`
	ValidTo      *string `json:"valid_to"`
	Algorithm    *string `json:"algorithm"`
	SigningTime  *string `json:"signing_time"`
	CertCount    int     `json:"cert_count"`
	HasTimestamp bool    `json:"has_timestamp"`
}

// VerifySignatures validates every signature in a PDF. It returns one report per
// signature (an empty slice means the document is unsigned).
func VerifySignatures(pdf []byte) ([]SignatureReport, error) {
	b, err := takeBytes(func(out **C.uchar, n *C.uintptr_t) C.PdfStatus {
		st := C.pdf_verify_signatures_json(uptr(pdf), C.uintptr_t(len(pdf)), out, n)
		runtime.KeepAlive(pdf)
		return st
	})
	if err != nil {
		return nil, err
	}
	if len(b) == 0 {
		return []SignatureReport{}, nil
	}
	var reports []SignatureReport
	if err := json.Unmarshal(b, &reports); err != nil {
		return nil, err
	}
	return reports, nil
}

// TextHit is one occurrence of a search query, with its bounding box in PDF
// user space (points, origin lower-left).
type TextHit struct {
	Page   int     `json:"page"`
	Text   string  `json:"text"`
	X      float64 `json:"x"`
	Y      float64 `json:"y"`
	Width  float64 `json:"width"`
	Height float64 `json:"height"`
}

// FindText returns the bounding box of every occurrence of query in pdf. With
// caseSensitive false the match is case-insensitive. An empty slice means no
// match.
func FindText(pdf []byte, query string, caseSensitive bool) ([]TextHit, error) {
	cq := C.CString(query)
	defer C.free(unsafe.Pointer(cq))
	cs := C.int(0)
	if caseSensitive {
		cs = 1
	}
	b, err := takeBytes(func(out **C.uchar, n *C.uintptr_t) C.PdfStatus {
		st := C.pdf_find_text_json(uptr(pdf), C.uintptr_t(len(pdf)), cq, cs, out, n)
		runtime.KeepAlive(pdf)
		return st
	})
	if err != nil {
		return nil, err
	}
	if len(b) == 0 {
		return []TextHit{}, nil
	}
	var hits []TextHit
	if err := json.Unmarshal(b, &hits); err != nil {
		return nil, err
	}
	return hits, nil
}

// PdfRect is a rectangle in PDF user space (points, origin lower-left).
type PdfRect struct {
	X0 float64
	Y0 float64
	X1 float64
	Y1 float64
}

// Width returns the rectangle's width (non-negative).
func (r PdfRect) Width() float64 { return abs(r.X1 - r.X0) }

// Height returns the rectangle's height (non-negative).
func (r PdfRect) Height() float64 { return abs(r.Y1 - r.Y0) }

func abs(v float64) float64 {
	if v < 0 {
		return -v
	}
	return v
}

// rawRect is the JSON wire form [x0,y0,x1,y1].
type rawRect [4]float64

func (r rawRect) toRect() PdfRect { return PdfRect{X0: r[0], Y0: r[1], X1: r[2], Y1: r[3]} }

// PageGeometry is the read-only geometry of one page. Sizes are in PDF points;
// Width/Height ignore page rotation while RotatedWidth/RotatedHeight account for
// it (swapped for 90/270-degree pages).
type PageGeometry struct {
	Page          int     `json:"page"`
	Width         float64 `json:"width"`
	Height        float64 `json:"height"`
	Rotation      int     `json:"rotation"`
	RotatedWidth  float64 `json:"rotatedWidth"`
	RotatedHeight float64 `json:"rotatedHeight"`
	MediaBox      PdfRect `json:"-"`
	CropBox       PdfRect `json:"-"`
}

// pageGeometryJSON is the wire form (boxes arrive as [x0,y0,x1,y1] arrays).
type pageGeometryJSON struct {
	Page          int     `json:"page"`
	Width         float64 `json:"width"`
	Height        float64 `json:"height"`
	Rotation      int     `json:"rotation"`
	RotatedWidth  float64 `json:"rotatedWidth"`
	RotatedHeight float64 `json:"rotatedHeight"`
	MediaBox      rawRect `json:"mediaBox"`
	CropBox       rawRect `json:"cropBox"`
}

// MeasurePages returns the geometry of every page in pdf.
func MeasurePages(pdf []byte) ([]PageGeometry, error) {
	b, err := takeBytes(func(out **C.uchar, n *C.uintptr_t) C.PdfStatus {
		st := C.pdf_measure_pages_json(uptr(pdf), C.uintptr_t(len(pdf)), out, n)
		runtime.KeepAlive(pdf)
		return st
	})
	if err != nil {
		return nil, err
	}
	if len(b) == 0 {
		return []PageGeometry{}, nil
	}
	var raw []pageGeometryJSON
	if err := json.Unmarshal(b, &raw); err != nil {
		return nil, err
	}
	pages := make([]PageGeometry, len(raw))
	for i, p := range raw {
		pages[i] = PageGeometry{
			Page:          p.Page,
			Width:         p.Width,
			Height:        p.Height,
			Rotation:      p.Rotation,
			RotatedWidth:  p.RotatedWidth,
			RotatedHeight: p.RotatedHeight,
			MediaBox:      p.MediaBox.toRect(),
			CropBox:       p.CropBox.toRect(),
		}
	}
	return pages, nil
}

// MeasurePage returns the geometry of page index (0-based) in pdf. It returns an
// error if index is out of range.
func MeasurePage(pdf []byte, index int) (PageGeometry, error) {
	pages, err := MeasurePages(pdf)
	if err != nil {
		return PageGeometry{}, err
	}
	if index < 0 || index >= len(pages) {
		return PageGeometry{}, &Error{Status: 5, Message: "page index out of range"}
	}
	return pages[index], nil
}

// PdfOverview is a non-mutating summary of a PDF (from Inspect). PdfaLevel is ""
// when the document is not PDF/A.
type PdfOverview struct {
	Version          string `json:"version"`
	PdfaLevel        string `json:"pdfaLevel"`
	Encrypted        bool   `json:"encrypted"`
	Encryption       string `json:"encryption"`
	RequiresPassword bool   `json:"requiresPassword"`
	PageCount        int    `json:"pageCount"`
}

// Inspect reads pdf without mutating it and reports its PDF version, PDF/A level
// (empty when not PDF/A), encryption state and page count. It never fails on a
// password-locked file.
func Inspect(pdf []byte) (PdfOverview, error) {
	b, err := takeBytes(func(out **C.uchar, n *C.uintptr_t) C.PdfStatus {
		st := C.pdf_inspect_json(uptr(pdf), C.uintptr_t(len(pdf)), out, n)
		runtime.KeepAlive(pdf)
		return st
	})
	if err != nil {
		return PdfOverview{}, err
	}
	var ov PdfOverview
	if err := json.Unmarshal(b, &ov); err != nil {
		return PdfOverview{}, err
	}
	return ov, nil
}

// SignOptions configures Sign. Empty strings are treated as absent.
type SignOptions struct {
	Reason   string
	Location string
	Name     string
	PAdES    bool
}

// Sign produces a signed PDF (PKCS#7 detached, incremental update). Requires a
// license.
func Sign(pdf, keyDER, certDER []byte, opts SignOptions) ([]byte, error) {
	reason, fr := optCStr(opts.Reason)
	location, fl := optCStr(opts.Location)
	name, fn := optCStr(opts.Name)
	defer fr()
	defer fl()
	defer fn()
	pades := C.int(0)
	if opts.PAdES {
		pades = 1
	}
	return takeBytes(func(out **C.uchar, n *C.uintptr_t) C.PdfStatus {
		st := C.pdf_sign(
			uptr(pdf), C.uintptr_t(len(pdf)),
			uptr(keyDER), C.uintptr_t(len(keyDER)),
			uptr(certDER), C.uintptr_t(len(certDER)),
			reason, location, name, pades, out, n)
		runtime.KeepAlive(pdf)
		runtime.KeepAlive(keyDER)
		runtime.KeepAlive(certDER)
		return st
	})
}

// Timestamp appends a document timestamp (/DocTimeStamp, PAdES-B-LTA).
func Timestamp(pdf, tsaKeyDER, tsaCertDER []byte, date string) ([]byte, error) {
	d, fd := optCStr(date)
	defer fd()
	return takeBytes(func(out **C.uchar, n *C.uintptr_t) C.PdfStatus {
		st := C.pdf_timestamp(
			uptr(pdf), C.uintptr_t(len(pdf)),
			uptr(tsaKeyDER), C.uintptr_t(len(tsaKeyDER)),
			uptr(tsaCertDER), C.uintptr_t(len(tsaCertDER)),
			d, out, n)
		runtime.KeepAlive(pdf)
		runtime.KeepAlive(tsaKeyDER)
		runtime.KeepAlive(tsaCertDER)
		return st
	})
}

// AddDss appends a Document Security Store (/DSS, PAdES-B-LT).
func AddDss(pdf []byte, certs, crls [][]byte) ([]byte, error) {
	var pinner runtime.Pinner
	defer pinner.Unpin()
	certPtrs, certLens := pinAll(&pinner, certs)
	crlPtrs, crlLens := pinAll(&pinner, crls)
	return takeBytes(func(out **C.uchar, n *C.uintptr_t) C.PdfStatus {
		st := C.pdf_add_dss(
			uptr(pdf), C.uintptr_t(len(pdf)),
			ppHead(certPtrs), upHead(certLens), C.uintptr_t(len(certs)),
			ppHead(crlPtrs), upHead(crlLens), C.uintptr_t(len(crls)),
			out, n)
		runtime.KeepAlive(pdf)
		runtime.KeepAlive(certs)
		runtime.KeepAlive(crls)
		runtime.KeepAlive(certPtrs)
		runtime.KeepAlive(crlPtrs)
		runtime.KeepAlive(certLens)
		runtime.KeepAlive(crlLens)
		return st
	})
}

// pinAll builds parallel C pointer/length slices for a list of byte slices. Each
// element points into Go-allocated memory, so the C pointer array would otherwise
// hold "Go pointers to unpinned Go pointers" — illegal to pass across cgo. Pin
// each element's backing array (via the caller's Pinner, unpinned after the call)
// so the pointer array is safe to hand to C under the default cgo pointer checks.
func pinAll(pinner *runtime.Pinner, items [][]byte) ([]*C.uint8_t, []C.uintptr_t) {
	ptrs := make([]*C.uint8_t, len(items))
	lens := make([]C.uintptr_t, len(items))
	for i, it := range items {
		if len(it) > 0 {
			pinner.Pin(&it[0])
			ptrs[i] = (*C.uint8_t)(unsafe.Pointer(&it[0]))
		}
		lens[i] = C.uintptr_t(len(it))
	}
	return ptrs, lens
}

func ppHead(s []*C.uint8_t) **C.uint8_t {
	if len(s) == 0 {
		return nil
	}
	return (**C.uint8_t)(unsafe.Pointer(&s[0]))
}

func upHead(s []C.uintptr_t) *C.uintptr_t {
	if len(s) == 0 {
		return nil
	}
	return &s[0]
}

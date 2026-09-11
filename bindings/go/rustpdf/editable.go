package rustpdf

/*
#include <stdlib.h>
#include "pdf.h"
*/
import "C"

import (
	"runtime"
	"strings"
	"unsafe"
)

// EditableDoc is an existing PDF loaded for manipulation. Call Close to free it.
type EditableDoc struct {
	h *C.PdfEditable
}

// Load parses an existing PDF from bytes.
func Load(data []byte) (*EditableDoc, error) {
	h := C.pdf_editable_load(uptr(data), C.uintptr_t(len(data)))
	runtime.KeepAlive(data)
	if h == nil {
		return nil, &Error{Status: 6, Message: lastError()}
	}
	return &EditableDoc{h: h}, nil
}

// LoadWithPassword parses an encrypted PDF using a password.
func LoadWithPassword(data []byte, password string) (*EditableDoc, error) {
	c := C.CString(password)
	defer C.free(unsafe.Pointer(c))
	h := C.pdf_editable_load_password(uptr(data), C.uintptr_t(len(data)), c)
	runtime.KeepAlive(data)
	if h == nil {
		return nil, &Error{Status: 6, Message: lastError()}
	}
	return &EditableDoc{h: h}, nil
}

// Close frees the handle. Safe to call multiple times.
func (e *EditableDoc) Close() {
	if e.h != nil {
		C.pdf_editable_free(e.h)
		e.h = nil
	}
}

// PageCount returns the number of pages.
func (e *EditableDoc) PageCount() int { return int(C.pdf_editable_page_count(e.h)) }

// Merge appends all pages of other.
func (e *EditableDoc) Merge(other *EditableDoc) error {
	return check(C.pdf_editable_merge(e.h, other.h))
}

func (e *EditableDoc) RotatePage(index, degrees int) error {
	return check(C.pdf_editable_rotate_page(e.h, C.uintptr_t(index), C.int(degrees)))
}

func (e *EditableDoc) DeletePage(index int) error {
	return check(C.pdf_editable_delete_page(e.h, C.uintptr_t(index)))
}

func (e *EditableDoc) ReorderPages(order []int) error {
	arr := toUintptrs(order)
	var head *C.uintptr_t
	if len(arr) > 0 {
		head = &arr[0]
	}
	st := C.pdf_editable_reorder_pages(e.h, head, C.uintptr_t(len(arr)))
	runtime.KeepAlive(arr)
	return check(st)
}

// ExtractPages extracts the given page indices into a new document.
func (e *EditableDoc) ExtractPages(indices []int) (*EditableDoc, error) {
	arr := toUintptrs(indices)
	var head *C.uintptr_t
	if len(arr) > 0 {
		head = &arr[0]
	}
	var out *C.PdfEditable
	st := C.pdf_editable_extract_pages(e.h, head, C.uintptr_t(len(arr)), &out)
	runtime.KeepAlive(arr)
	if err := check(st); err != nil {
		return nil, err
	}
	return &EditableDoc{h: out}, nil
}

func (e *EditableDoc) SetInfo(key, value string) error {
	ck := C.CString(key)
	cv := C.CString(value)
	defer C.free(unsafe.Pointer(ck))
	defer C.free(unsafe.Pointer(cv))
	return check(C.pdf_editable_set_info(e.h, ck, cv))
}

func (e *EditableDoc) GetInfo(key string) (string, error) {
	ck := C.CString(key)
	defer C.free(unsafe.Pointer(ck))
	b, err := takeBytes(func(out **C.uchar, n *C.uintptr_t) C.PdfStatus {
		return C.pdf_editable_get_info(e.h, ck, out, n)
	})
	return string(b), err
}

func (e *EditableDoc) SetXMP(xml []byte) error {
	st := C.pdf_editable_set_xmp(e.h, uptr(xml), C.uintptr_t(len(xml)))
	runtime.KeepAlive(xml)
	return check(st)
}

func (e *EditableDoc) OverlayPage(index int, content []byte) error {
	st := C.pdf_editable_overlay_page(e.h, C.uintptr_t(index), uptr(content), C.uintptr_t(len(content)))
	runtime.KeepAlive(content)
	return check(st)
}

// FillTextField fills an AcroForm text field; returns whether it existed.
func (e *EditableDoc) FillTextField(name, value string) (bool, error) {
	cn := C.CString(name)
	cv := C.CString(value)
	defer C.free(unsafe.Pointer(cn))
	defer C.free(unsafe.Pointer(cv))
	var found C.int
	if err := check(C.pdf_editable_fill_text_field(e.h, cn, cv, &found)); err != nil {
		return false, err
	}
	return found != 0, nil
}

// SetCheckbox checks or unchecks a checkbox field by name; returns whether it
// existed.
func (e *EditableDoc) SetCheckbox(name string, checked bool) (bool, error) {
	cn := C.CString(name)
	defer C.free(unsafe.Pointer(cn))
	cb := C.int(0)
	if checked {
		cb = 1
	}
	var found C.int
	if err := check(C.pdf_editable_set_checkbox(e.h, cn, cb, &found)); err != nil {
		return false, err
	}
	return found != 0, nil
}

// SetRadio selects a radio button by its export value; returns whether it
// existed.
func (e *EditableDoc) SetRadio(name, exportValue string) (bool, error) {
	cn := C.CString(name)
	cv := C.CString(exportValue)
	defer C.free(unsafe.Pointer(cn))
	defer C.free(unsafe.Pointer(cv))
	var found C.int
	if err := check(C.pdf_editable_set_radio(e.h, cn, cv, &found)); err != nil {
		return false, err
	}
	return found != 0, nil
}

// SetChoice sets a choice (dropdown/list) field value; returns whether it
// existed.
func (e *EditableDoc) SetChoice(name, value string) (bool, error) {
	cn := C.CString(name)
	cv := C.CString(value)
	defer C.free(unsafe.Pointer(cn))
	defer C.free(unsafe.Pointer(cv))
	var found C.int
	if err := check(C.pdf_editable_set_choice(e.h, cn, cv, &found)); err != nil {
		return false, err
	}
	return found != 0, nil
}

// FlattenForms flattens all interactive form fields into static page content.
func (e *EditableDoc) FlattenForms() error { return check(C.pdf_editable_flatten_forms(e.h)) }

// FieldNames returns the document's terminal AcroForm field names.
func (e *EditableDoc) FieldNames() ([]string, error) {
	b, err := takeBytes(func(out **C.uchar, n *C.uintptr_t) C.PdfStatus {
		return C.pdf_editable_field_names(e.h, out, n)
	})
	if err != nil {
		return nil, err
	}
	var names []string
	for _, s := range strings.Split(string(b), "\n") {
		if s != "" {
			names = append(names, s)
		}
	}
	return names, nil
}

// WatermarkText stamps a diagonal text watermark across every page (standard
// Helvetica). rotationDeg is counter-clockwise; opacity in 0..=1. When
// opaqueBackground is true the text is drawn over an opaque background box
// (covering the underlying content) instead of being semi-transparent.
func (e *EditableDoc) WatermarkText(text string, size, r, g, b, opacity, rotationDeg float64, opaqueBackground bool) error {
	c := C.CString(text)
	defer C.free(unsafe.Pointer(c))
	ob := C.int(0)
	if opaqueBackground {
		ob = 1
	}
	return check(C.pdf_editable_watermark_text(
		e.h, c, C.double(size), C.double(r), C.double(g), C.double(b),
		C.double(opacity), C.double(rotationDeg), ob))
}

// FillRect paints a filled rectangle at (x, y) sized width×height on page
// pageIndex (0-based), in RGB color (r/g/b, each 0..=1) at opacity (0..=1), and
// returns whether the page existed. Coordinates are in the page's visible space
// (origin lower-left, y up) — the rectangle lands where a viewer sees it
// regardless of the page's /Rotate.
func (e *EditableDoc) FillRect(pageIndex int, x, y, width, height, r, g, b, opacity float64) bool {
	var found C.int
	C.pdf_editable_fill_rect(
		e.h, C.int(pageIndex), C.double(x), C.double(y), C.double(width), C.double(height),
		C.double(r), C.double(g), C.double(b), C.double(opacity), &found)
	return found != 0
}

// PlaceText draws a line of text with its baseline at (x, y) on page pageIndex
// (0-based), in standard Helvetica at size points and RGB color (each 0..=1),
// and returns whether the page existed. rotationDeg rotates the text
// counter-clockwise about its anchor (x, y). Coordinates are in the page's
// visible space (origin lower-left, y up) — the text lands where a viewer sees
// it regardless of the page's /Rotate.
func (e *EditableDoc) PlaceText(pageIndex int, x, y float64, text string, size, r, g, b, rotationDeg float64) bool {
	c := C.CString(text)
	defer C.free(unsafe.Pointer(c))
	var found C.int
	C.pdf_editable_place_text(
		e.h, C.int(pageIndex), C.double(x), C.double(y), c, C.double(size),
		C.double(r), C.double(g), C.double(b), C.double(rotationDeg), &found)
	return found != 0
}

// PlaceTextAligned draws a line of text on page pageIndex (0-based) like
// PlaceText, but shifts the start point along the baseline so the text is
// horizontally aligned to the anchor (x, y) per align (left/right/center;
// justify behaves like left for a single line). It returns whether the page
// existed. Coordinates are in the page's visible space (origin lower-left,
// y up).
func (e *EditableDoc) PlaceTextAligned(pageIndex int, x, y float64, text string, size, r, g, b, rotationDeg float64, align Align) bool {
	c := C.CString(text)
	defer C.free(unsafe.Pointer(c))
	var found C.int
	C.pdf_editable_place_text_aligned(
		e.h, C.int(pageIndex), C.double(x), C.double(y), c, C.double(size),
		C.double(r), C.double(g), C.double(b), C.double(rotationDeg), C.int(align), &found)
	return found != 0
}

// MaskedText fills an opaque rectangle [x, y, x+width, y+height] in bgColor on
// page pageIndex (0-based), then writes text (standard Helvetica at size points,
// in textColor) horizontally aligned per align and vertically centered within
// the box. It returns whether the page existed. Coordinates are in the page's
// visible space (origin lower-left, y up) — useful for masking a placeholder
// region with replacement text. textColor and bgColor are [r, g, b] triples
// (each 0..=1).
func (e *EditableDoc) MaskedText(pageIndex int, x, y, width, height float64, text string, size float64, textColor, bgColor [3]float64, align Align) bool {
	c := C.CString(text)
	defer C.free(unsafe.Pointer(c))
	var found C.int
	C.pdf_editable_masked_text(
		e.h, C.int(pageIndex), C.double(x), C.double(y), C.double(width), C.double(height),
		c, C.double(size),
		C.double(textColor[0]), C.double(textColor[1]), C.double(textColor[2]),
		C.double(bgColor[0]), C.double(bgColor[1]), C.double(bgColor[2]),
		C.int(align), &found)
	return found != 0
}

// AddFontFile registers a TrueType/OpenType font (from a file path) for text
// stamping and returns its font id, usable with the fontID parameter of
// PlaceTextAnchored / MaskedTextPadded / PlaceParagraph. The font is embedded
// as a subset — stamped text renders with the real font's glyphs and metrics,
// exactly like Document.AddFontFile + ShowText.
func (e *EditableDoc) AddFontFile(path string) (int, error) {
	c := C.CString(path)
	defer C.free(unsafe.Pointer(c))
	var id C.int
	if err := check(C.pdf_editable_add_font_file(e.h, c, &id)); err != nil {
		return 0, err
	}
	return int(id), nil
}

// AddFont registers a stamping font from raw TrueType/OpenType bytes and
// returns its font id. See AddFontFile.
func (e *EditableDoc) AddFont(data []byte) (int, error) {
	var id C.int
	st := C.pdf_editable_add_font(e.h, uptr(data), C.uintptr_t(len(data)), &id)
	runtime.KeepAlive(data)
	if err := check(st); err != nil {
		return 0, err
	}
	return int(id), nil
}

// PlaceTextAnchored draws a line of text on page pageIndex (0-based) like
// PlaceTextAligned, but with an explicit vertical anchor saying what y means
// (AnchorBaseline keeps the historical behavior; AnchorTop hangs the text from
// y; AnchorBottom rests the descender line on y; AnchorLineTop/AnchorLineBottom
// use the layout line box) and an optional embedded font: pass fontID from
// AddFontFile/AddFont to stamp with that font, or -1 for the built-in
// Helvetica. It returns whether the page (and font) existed. Coordinates are in
// the page's visible space (origin lower-left, y up).
func (e *EditableDoc) PlaceTextAnchored(pageIndex int, x, y float64, text string, size, r, g, b, rotationDeg float64, align Align, anchor VerticalAnchor, fontID int) bool {
	c := C.CString(text)
	defer C.free(unsafe.Pointer(c))
	var found C.int
	C.pdf_editable_place_text_anchored(
		e.h, C.int(pageIndex), C.double(x), C.double(y), c, C.double(size),
		C.double(r), C.double(g), C.double(b), C.double(rotationDeg),
		C.int(align), C.int(anchor), C.int(fontID), &found)
	return found != 0
}

// MaskedTextPadded is MaskedText with an explicit vertical alignment of the
// line inside the box (VAlignMiddle keeps the historical cap-height centering;
// VAlignTop hangs the line from the top edge; VAlignBottom rests the descender
// line on the bottom edge), a horizontal edge inset padding (points) for
// left/right alignment — text starts at x + padding (or ends at
// x + width − padding); a negative padding keeps the historical default
// min(0.15 × size, width / 4), 0 starts flush with the box edge — and an
// optional embedded font (fontID from AddFontFile/AddFont, or -1 for the
// built-in Helvetica). It returns whether the page (and font) existed.
// textColor and bgColor are [r, g, b] triples (each 0..=1).
func (e *EditableDoc) MaskedTextPadded(pageIndex int, x, y, width, height float64, text string, size float64, textColor, bgColor [3]float64, align Align, valign VerticalAlign, padding float64, fontID int) bool {
	c := C.CString(text)
	defer C.free(unsafe.Pointer(c))
	var found C.int
	C.pdf_editable_masked_text_pad(
		e.h, C.int(pageIndex), C.double(x), C.double(y), C.double(width), C.double(height),
		c, C.double(size),
		C.double(textColor[0]), C.double(textColor[1]), C.double(textColor[2]),
		C.double(bgColor[0]), C.double(bgColor[1]), C.double(bgColor[2]),
		C.int(align), C.int(valign), C.double(padding), C.int(fontID), &found)
	return found != 0
}

// PlaceParagraph stamps a paragraph with automatic word wrapping on page
// pageIndex (0-based): text is broken into lines that fit width points (greedy,
// by word; '\n' forces a break) and drawn downward from the anchor (x, y).
// anchor says what y means for the block: AnchorTop — top of the box (the first
// baseline lands ascent × size below y, legacy fixed-position layout semantics);
// AnchorBaseline — the first line's baseline; AnchorBottom/AnchorLineBottom —
// bottom-pinned: the block's bottom rests on y and grows upward by its real
// content height (with maxHeight the box is [y, y+maxHeight] and overflowing
// lines are cut from the top). align lays lines out inside [x, x+width]
// (AlignJustify stretches the word gaps of every line but the last of each
// paragraph). Pass fontID from AddFontFile/AddFont to wrap and draw with an
// embedded font (its real metrics drive the break points), or -1 for the
// built-in Helvetica. maxHeight > 0 truncates lines that would cross the limit
// (<= 0 = unlimited); lineHeight scales the default 1.2 × size leading
// (<= 0 = 1.0); rotationDeg rotates the laid-out block counter-clockwise about
// the anchor. It returns the number of lines drawn, the consumed block height
// in points, and whether the page (and font) existed and the box was valid.
func (e *EditableDoc) PlaceParagraph(pageIndex int, x, y, width float64, text string, size, r, g, b float64, align Align, fontID int, maxHeight, lineHeight float64, anchor VerticalAnchor, rotationDeg float64) (lines int, height float64, found bool) {
	c := C.CString(text)
	defer C.free(unsafe.Pointer(c))
	var h C.double
	var n, ok C.int
	C.pdf_editable_place_paragraph_anchored(
		e.h, C.int(pageIndex), C.double(x), C.double(y), C.double(width), c, C.double(size),
		C.double(r), C.double(g), C.double(b), C.int(align), C.int(anchor),
		C.int(fontID), C.double(maxHeight), C.double(lineHeight), C.double(rotationDeg),
		&h, &n, &ok)
	return int(n), float64(h), ok != 0
}

// SetStampSpace chooses the coordinate space of the positioned stamping
// primitives (FillRect, PlaceText*, MaskedText*, PlaceParagraph, DrawImage*)
// for subsequent calls: StampVisible (the default) keeps coordinates in the
// page's displayed space, compensating /Rotate; StampMedia interprets
// coordinates and rotations in the raw PDF user space (legacy layout semantics).
// Watermarks and redaction are unaffected.
func (e *EditableDoc) SetStampSpace(space StampSpace) error {
	return check(C.pdf_editable_set_stamp_space(e.h, C.int(space)))
}

// DrawImage stamps an image (in-memory PNG or JPEG bytes, dispatched on the
// file signature) onto page index (0-based) with its lower-left corner at
// (x, y), scaled to width×height points, and returns whether the page existed.
// rotationDeg rotates the image counter-clockwise about that corner.
// Coordinates are in the page's visible space (origin lower-left, y up),
// honoring the page's /Rotate.
func (e *EditableDoc) DrawImage(index int, image []byte, x, y, width, height, rotationDeg float64) bool {
	var found C.int
	C.pdf_editable_draw_image(
		e.h, C.int(index), uptr(image), C.uintptr_t(len(image)),
		C.double(x), C.double(y), C.double(width), C.double(height),
		C.double(rotationDeg), &found)
	runtime.KeepAlive(image)
	return found != 0
}

// DrawImageAnchored is DrawImage with an explicit rotation anchor:
// ImageAnchorCorner (the DrawImage behavior) rotates the image about its own
// lower-left corner at (x, y); ImageAnchorBoundingBox lands the rotated
// image's bounding box with its lower-left at (x, y) (bounding-box layout semantics —
// e.g. a 90-degree image occupies [x, x+height] × [y, y+width]). It returns
// whether the page existed.
func (e *EditableDoc) DrawImageAnchored(index int, image []byte, x, y, width, height, rotationDeg float64, anchor ImageAnchor) bool {
	var found C.int
	C.pdf_editable_draw_image_anchored(
		e.h, C.int(index), uptr(image), C.uintptr_t(len(image)),
		C.double(x), C.double(y), C.double(width), C.double(height),
		C.double(rotationDeg), C.int(anchor), &found)
	runtime.KeepAlive(image)
	return found != 0
}

// WatermarkImageFile stamps an image (JPEG/PNG file at path) centered on every
// page at width×height points, rotated rotationDeg degrees, at opacity.
func (e *EditableDoc) WatermarkImageFile(path string, width, height, opacity, rotationDeg float64) error {
	c := C.CString(path)
	defer C.free(unsafe.Pointer(c))
	return check(C.pdf_editable_watermark_image_file(
		e.h, c, C.double(width), C.double(height), C.double(opacity), C.double(rotationDeg)))
}

// SetVersion sets the output PDF version (0 = 1.4, 1 = 1.5, 2 = 1.7, 3 = 2.0),
// clearing any catalog /Version override.
func (e *EditableDoc) SetVersion(version int) error {
	return check(C.pdf_editable_set_version(e.h, C.int(version)))
}

// StripPdfa strips PDF/A conformance (catalog /OutputIntents, the XMP pdfaid
// identifier and /Version) so the file is a plain PDF.
func (e *EditableDoc) StripPdfa() error { return check(C.pdf_editable_strip_pdfa(e.h)) }

// Normalize strips PDF/A and sets the output version (codes as in SetVersion),
// producing a plain PDF.
func (e *EditableDoc) Normalize(version int) error {
	return check(C.pdf_editable_normalize(e.h, C.int(version)))
}

// Redact removes content under the given rectangles on page index (drawing a
// black box over each) and returns whether the page existed. Each rect is
// [x0,y0,x1,y1].
func (e *EditableDoc) Redact(index int, rects [][4]float64) (bool, error) {
	flat := make([]C.double, len(rects)*4)
	for i, r := range rects {
		flat[i*4] = C.double(r[0])
		flat[i*4+1] = C.double(r[1])
		flat[i*4+2] = C.double(r[2])
		flat[i*4+3] = C.double(r[3])
	}
	var head *C.double
	if len(flat) > 0 {
		head = &flat[0]
	}
	var found C.int
	st := C.pdf_editable_redact(e.h, C.uintptr_t(index), head, C.uintptr_t(len(rects)), &found)
	runtime.KeepAlive(flat)
	if err := check(st); err != nil {
		return false, err
	}
	return found != 0, nil
}

// ConvertToPdfa converts the loaded document to PDF/A at the given level
// (only B-levels A1b/A2b/A3b are valid).
func (e *EditableDoc) ConvertToPdfa(level PdfaLevel) error {
	return check(C.pdf_editable_convert_to_pdfa(e.h, C.int(level)))
}

func (e *EditableDoc) Optimize() error { return check(C.pdf_editable_optimize(e.h)) }

func (e *EditableDoc) Compact(on bool) error {
	v := C.int(0)
	if on {
		v = 1
	}
	return check(C.pdf_editable_compact(e.h, v))
}

// Encrypt enables encryption on save.
func (e *EditableDoc) Encrypt(method Encryption, user, owner string, readOnly bool) error {
	cu := C.CString(user)
	co := C.CString(owner)
	defer C.free(unsafe.Pointer(cu))
	defer C.free(unsafe.Pointer(co))
	ro := C.int(0)
	if readOnly {
		ro = 1
	}
	return check(C.pdf_editable_encrypt(e.h, C.int(method), cu, co, ro))
}

// ToBytes serializes the document.
func (e *EditableDoc) ToBytes() ([]byte, error) {
	return takeBytes(func(out **C.uchar, n *C.uintptr_t) C.PdfStatus {
		return C.pdf_editable_to_bytes(e.h, out, n)
	})
}

// ToBytesIncremental serializes as an incremental update over original.
func (e *EditableDoc) ToBytesIncremental(original []byte) ([]byte, error) {
	b, err := takeBytes(func(out **C.uchar, n *C.uintptr_t) C.PdfStatus {
		return C.pdf_editable_to_bytes_incremental(e.h, uptr(original), C.uintptr_t(len(original)), out, n)
	})
	runtime.KeepAlive(original)
	return b, err
}

func (e *EditableDoc) Save(path string) error {
	c := C.CString(path)
	defer C.free(unsafe.Pointer(c))
	return check(C.pdf_editable_save(e.h, c))
}

func toUintptrs(xs []int) []C.uintptr_t {
	out := make([]C.uintptr_t, len(xs))
	for i, x := range xs {
		out[i] = C.uintptr_t(x)
	}
	return out
}

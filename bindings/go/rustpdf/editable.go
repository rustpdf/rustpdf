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
// Helvetica). rotationDeg is counter-clockwise; opacity in 0..=1.
func (e *EditableDoc) WatermarkText(text string, size, r, g, b, opacity, rotationDeg float64) error {
	c := C.CString(text)
	defer C.free(unsafe.Pointer(c))
	return check(C.pdf_editable_watermark_text(
		e.h, c, C.double(size), C.double(r), C.double(g), C.double(b),
		C.double(opacity), C.double(rotationDeg)))
}

// WatermarkImageFile stamps an image (JPEG/PNG file at path) centered on every
// page at width×height points, at opacity.
func (e *EditableDoc) WatermarkImageFile(path string, width, height, opacity float64) error {
	c := C.CString(path)
	defer C.free(unsafe.Pointer(c))
	return check(C.pdf_editable_watermark_image_file(
		e.h, c, C.double(width), C.double(height), C.double(opacity)))
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
// (only B-levels A1b/A2b/A3b are valid). Requires a license.
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

// Encrypt enables encryption on save (requires a license).
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

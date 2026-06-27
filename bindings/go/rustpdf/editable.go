package rustpdf

/*
#include <stdlib.h>
#include "pdf.h"
*/
import "C"

import (
	"runtime"
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

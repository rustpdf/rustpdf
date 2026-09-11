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

// Document is a PDF being authored. Call Close to free native memory.
type Document struct {
	h *C.PdfDocument
}

// Info is the document information dictionary (any field may be empty).
type Info struct {
	Title, Author, Subject, Keywords, Creator string
}

// RadioButton is one button of a radio group: a rectangle and an export value.
type RadioButton struct {
	Rect   [4]float64
	Export string
}

// Bookmark is a document outline (table of contents) entry. Nest entries via
// Children to build a tree; pass a root Bookmark to Document.AddBookmark.
type Bookmark struct {
	Title    string
	Page     int
	Top      *float64
	Children []Bookmark
}

// Child appends a child bookmark and returns the (modified) receiver.
func (b Bookmark) Child(child Bookmark) Bookmark {
	b.Children = append(b.Children, child)
	return b
}

// flatten appends this bookmark and its descendants in pre-order.
func (b Bookmark) flatten(level int, out *[]flatBookmark) {
	*out = append(*out, flatBookmark{level: level, title: b.Title, page: b.Page, top: b.Top})
	for _, c := range b.Children {
		c.flatten(level+1, out)
	}
}

type flatBookmark struct {
	level int
	title string
	page  int
	top   *float64
}

// New creates a new, empty A4 document.
func New() (*Document, error) {
	h := C.pdf_document_new()
	if h == nil {
		return nil, &Error{Status: 1, Message: "pdf_document_new returned NULL"}
	}
	return &Document{h: h}, nil
}

// Close frees the document. Safe to call multiple times.
func (d *Document) Close() {
	if d.h != nil {
		C.pdf_document_free(d.h)
		d.h = nil
	}
}

// ---- configuration ---------------------------------------------------------

// Pdfa marks the document as PDF/A-2b.
func (d *Document) Pdfa() error { return check(C.pdf_document_pdfa(d.h)) }

// PdfaLevel marks the document at an explicit PDF/A level.
func (d *Document) PdfaLevel(level PdfaLevel) error {
	return check(C.pdf_document_pdfa_level(d.h, C.int(level)))
}

// Tagged enables the tagged/accessible structure tree.
func (d *Document) Tagged() error { return check(C.pdf_document_tagged(d.h)) }

// SetVersion sets the PDF version (0 = 1.4, 1 = 1.5, 2 = 1.7, 3 = 2.0).
func (d *Document) SetVersion(v int) error { return check(C.pdf_document_set_version(d.h, C.int(v))) }

// SetDefaultSize sets the default page size for subsequent pages.
func (d *Document) SetDefaultSize(w, h float64) error {
	return check(C.pdf_document_set_default_size(d.h, C.double(w), C.double(h)))
}

// SetInfo sets the document info dictionary.
func (d *Document) SetInfo(info Info) error {
	t, ft := optCStr(info.Title)
	a, fa := optCStr(info.Author)
	s, fs := optCStr(info.Subject)
	k, fk := optCStr(info.Keywords)
	c, fc := optCStr(info.Creator)
	defer ft()
	defer fa()
	defer fs()
	defer fk()
	defer fc()
	return check(C.pdf_document_set_info(d.h, t, a, s, k, c))
}

// ---- pages + graphics ------------------------------------------------------

// AddPage appends a page using the document's default size.
func (d *Document) AddPage() error { return check(C.pdf_document_add_page(d.h)) }

// AddPageSized appends a page of an explicit size (points).
func (d *Document) AddPageSized(w, h float64) error {
	return check(C.pdf_document_add_page_sized(d.h, C.double(w), C.double(h)))
}

func (d *Document) SetFillRGB(r, g, b float64) error {
	return check(C.pdf_page_set_fill_rgb(d.h, C.double(r), C.double(g), C.double(b)))
}

func (d *Document) SetStrokeRGB(r, g, b float64) error {
	return check(C.pdf_page_set_stroke_rgb(d.h, C.double(r), C.double(g), C.double(b)))
}

func (d *Document) SetLineWidth(w float64) error {
	return check(C.pdf_page_set_line_width(d.h, C.double(w)))
}

func (d *Document) Rect(x, y, w, h float64) error {
	return check(C.pdf_page_rect(d.h, C.double(x), C.double(y), C.double(w), C.double(h)))
}

func (d *Document) Fill() error   { return check(C.pdf_page_fill(d.h)) }
func (d *Document) Stroke() error { return check(C.pdf_page_stroke(d.h)) }

// ---- fonts + text ----------------------------------------------------------

// AddFontFile registers a font from a path and returns its id.
func (d *Document) AddFontFile(path string) (int, error) {
	c := C.CString(path)
	defer C.free(unsafe.Pointer(c))
	var id C.int
	if err := check(C.pdf_document_add_font_file(d.h, c, &id)); err != nil {
		return 0, err
	}
	return int(id), nil
}

// AddFont registers a font from TrueType/OpenType bytes and returns its id.
func (d *Document) AddFont(data []byte) (int, error) {
	var id C.int
	st := C.pdf_document_add_font(d.h, uptr(data), C.uintptr_t(len(data)), &id)
	runtime.KeepAlive(data)
	if err := check(st); err != nil {
		return 0, err
	}
	return int(id), nil
}

// ShowText shows a line of text. headingLevel 1..6 tags it as H1..H6 (when the
// document is tagged); 0 leaves it as a paragraph.
func (d *Document) ShowText(font int, size, x, y float64, text string, headingLevel int) error {
	c := C.CString(text)
	defer C.free(unsafe.Pointer(c))
	return check(C.pdf_page_show_text(
		d.h, C.int(font), C.double(size), C.double(x), C.double(y), c, C.int(headingLevel)))
}

// Paragraph lays out a wrapping paragraph in the box at (x, y, width).
func (d *Document) Paragraph(font int, size, x, y, width float64, text string, align Align) error {
	c := C.CString(text)
	defer C.free(unsafe.Pointer(c))
	return check(C.pdf_page_paragraph(
		d.h, C.int(font), C.double(size), C.double(x), C.double(y), C.double(width), C.int(align), c))
}

// ---- images ----------------------------------------------------------------

func (d *Document) AddImageFile(path string) (int, error) {
	c := C.CString(path)
	defer C.free(unsafe.Pointer(c))
	var id C.int
	if err := check(C.pdf_document_add_image_file(d.h, c, &id)); err != nil {
		return 0, err
	}
	return int(id), nil
}

func (d *Document) AddImagePNG(data []byte) (int, error) {
	var id C.int
	st := C.pdf_document_add_image_png(d.h, uptr(data), C.uintptr_t(len(data)), &id)
	runtime.KeepAlive(data)
	if err := check(st); err != nil {
		return 0, err
	}
	return int(id), nil
}

func (d *Document) AddImageJPEG(data []byte) (int, error) {
	var id C.int
	st := C.pdf_document_add_image_jpeg(d.h, uptr(data), C.uintptr_t(len(data)), &id)
	runtime.KeepAlive(data)
	if err := check(st); err != nil {
		return 0, err
	}
	return int(id), nil
}

func (d *Document) DrawImage(image int, x, y, w, h float64) error {
	return check(C.pdf_page_draw_image(d.h, C.int(image), C.double(x), C.double(y), C.double(w), C.double(h)))
}

// Figure draws a meaningful image (tagged /Figure with alternate text).
func (d *Document) Figure(image int, x, y, w, h float64, alt string) error {
	c := C.CString(alt)
	defer C.free(unsafe.Pointer(c))
	return check(C.pdf_page_figure(
		d.h, C.int(image), C.double(x), C.double(y), C.double(w), C.double(h), c))
}

// ---- attachments + forms ---------------------------------------------------

// AttachFile embeds a file (PDF/A-3 associated file).
func (d *Document) AttachFile(name, mime string, data []byte, rel AFRelationship, desc string) error {
	cn := C.CString(name)
	cm := C.CString(mime)
	cd := C.CString(desc)
	defer C.free(unsafe.Pointer(cn))
	defer C.free(unsafe.Pointer(cm))
	defer C.free(unsafe.Pointer(cd))
	st := C.pdf_document_attach_file(
		d.h, cn, cm, uptr(data), C.uintptr_t(len(data)), C.int(rel), cd)
	runtime.KeepAlive(data)
	return check(st)
}

func (d *Document) TextField(name string, page int, rect [4]float64, value string, size float64) error {
	cn := C.CString(name)
	cv := C.CString(value)
	defer C.free(unsafe.Pointer(cn))
	defer C.free(unsafe.Pointer(cv))
	return check(C.pdf_document_text_field(
		d.h, cn, C.uintptr_t(page),
		C.double(rect[0]), C.double(rect[1]), C.double(rect[2]), C.double(rect[3]),
		cv, C.double(size)))
}

func (d *Document) Checkbox(name string, page int, rect [4]float64, checked bool) error {
	cn := C.CString(name)
	defer C.free(unsafe.Pointer(cn))
	cb := C.int(0)
	if checked {
		cb = 1
	}
	return check(C.pdf_document_checkbox(
		d.h, cn, C.uintptr_t(page),
		C.double(rect[0]), C.double(rect[1]), C.double(rect[2]), C.double(rect[3]), cb))
}

// Dropdown adds a choice field. selected is the 0-based index or -1 for none.
func (d *Document) Dropdown(name string, page int, rect [4]float64, options []string, selected int, size float64) error {
	cn := C.CString(name)
	co := C.CString(strings.Join(options, "\n"))
	defer C.free(unsafe.Pointer(cn))
	defer C.free(unsafe.Pointer(co))
	return check(C.pdf_document_dropdown(
		d.h, cn, C.uintptr_t(page),
		C.double(rect[0]), C.double(rect[1]), C.double(rect[2]), C.double(rect[3]),
		co, C.int(selected), C.double(size)))
}

// RadioGroup adds a radio-button group. selected is the 0-based index or -1.
func (d *Document) RadioGroup(name string, page int, buttons []RadioButton, selected int) error {
	cn := C.CString(name)
	defer C.free(unsafe.Pointer(cn))

	rects := make([]C.double, len(buttons)*4)
	exports := make([]*C.char, len(buttons))
	for i, b := range buttons {
		rects[i*4] = C.double(b.Rect[0])
		rects[i*4+1] = C.double(b.Rect[1])
		rects[i*4+2] = C.double(b.Rect[2])
		rects[i*4+3] = C.double(b.Rect[3])
		exports[i] = C.CString(b.Export)
	}
	defer func() {
		for _, e := range exports {
			C.free(unsafe.Pointer(e))
		}
	}()

	var rectsHead *C.double
	var exportsHead **C.char
	if len(buttons) > 0 {
		rectsHead = &rects[0]
		exportsHead = (**C.char)(unsafe.Pointer(&exports[0]))
	}
	st := C.pdf_document_radio_group(
		d.h, cn, C.uintptr_t(page), C.uintptr_t(len(buttons)), rectsHead, exportsHead, C.int(selected))
	runtime.KeepAlive(rects)
	runtime.KeepAlive(exports)
	return check(st)
}

// ---- hyperlinks + bookmarks ------------------------------------------------

// LinkURI adds a clickable web link over rect ([x0,y0,x1,y1]) on the current
// page opening uri.
func (d *Document) LinkURI(rect [4]float64, uri string) error {
	c := C.CString(uri)
	defer C.free(unsafe.Pointer(c))
	return check(C.pdf_page_link_uri(
		d.h, C.double(rect[0]), C.double(rect[1]), C.double(rect[2]), C.double(rect[3]), c))
}

// LinkToPage adds an internal link over rect jumping to pageIndex (0-based). If
// top is non-nil, the view scrolls so top is at the top of the page.
func (d *Document) LinkToPage(rect [4]float64, pageIndex int, top *float64) error {
	t := C.double(0)
	hasTop := C.int(0)
	if top != nil {
		t = C.double(*top)
		hasTop = 1
	}
	return check(C.pdf_page_link_to_page(
		d.h, C.double(rect[0]), C.double(rect[1]), C.double(rect[2]), C.double(rect[3]),
		C.uintptr_t(pageIndex), t, hasTop))
}

// AddBookmark adds one outline tree (pre-order flattened into the native call).
// Call it once per top-level entry.
func (d *Document) AddBookmark(bookmark Bookmark) error {
	var entries []flatBookmark
	bookmark.flatten(0, &entries)
	n := len(entries)
	if n == 0 {
		return nil
	}

	levels := make([]C.int, n)
	pages := make([]C.uintptr_t, n)
	tops := make([]C.double, n)
	hasTops := make([]C.int, n)
	titles := make([]*C.char, n)
	for i, e := range entries {
		levels[i] = C.int(e.level)
		pages[i] = C.uintptr_t(e.page)
		if e.top != nil {
			tops[i] = C.double(*e.top)
			hasTops[i] = 1
		}
		titles[i] = C.CString(e.title)
	}
	defer func() {
		for _, t := range titles {
			C.free(unsafe.Pointer(t))
		}
	}()

	st := C.pdf_document_add_bookmarks(
		d.h, C.uintptr_t(n),
		&levels[0],
		(**C.char)(unsafe.Pointer(&titles[0])),
		&pages[0], &tops[0], &hasTops[0])
	runtime.KeepAlive(levels)
	runtime.KeepAlive(pages)
	runtime.KeepAlive(tops)
	runtime.KeepAlive(hasTops)
	runtime.KeepAlive(titles)
	return check(st)
}

// Facturx makes the document a ZUGFeRD / Factur-X invoice: embeds xml as
// factur-x.xml, marks it PDF/A-3b and adds the Factur-X XMP.
func (d *Document) Facturx(xml []byte, profile FacturxProfile) error {
	st := C.pdf_document_facturx(d.h, uptr(xml), C.uintptr_t(len(xml)), C.int(profile))
	runtime.KeepAlive(xml)
	return check(st)
}

// ---- output ----------------------------------------------------------------

// PageCount returns the number of pages.
func (d *Document) PageCount() int { return int(C.pdf_document_page_count(d.h)) }

// ToBytes serializes the document.
func (d *Document) ToBytes() ([]byte, error) {
	return takeBytes(func(out **C.uchar, n *C.uintptr_t) C.PdfStatus {
		return C.pdf_document_write(d.h, out, n)
	})
}

// Save writes the document to a file.
func (d *Document) Save(path string) error {
	c := C.CString(path)
	defer C.free(unsafe.Pointer(c))
	return check(C.pdf_document_save(d.h, c))
}

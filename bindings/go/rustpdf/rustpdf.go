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
)

// Align is a paragraph horizontal alignment.
type Align int

const (
	AlignLeft    Align = 0
	AlignRight   Align = 1
	AlignCenter  Align = 2
	AlignJustify Align = 3
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
	certPtrs, certLens := pinAll(certs)
	crlPtrs, crlLens := pinAll(crls)
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
		return st
	})
}

func pinAll(items [][]byte) ([]*C.uint8_t, []C.uintptr_t) {
	ptrs := make([]*C.uint8_t, len(items))
	lens := make([]C.uintptr_t, len(items))
	for i, it := range items {
		ptrs[i] = uptr(it)
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

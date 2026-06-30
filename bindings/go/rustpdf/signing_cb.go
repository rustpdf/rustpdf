package rustpdf

// This file holds ONLY the exported cgo callback. A file that uses //export
// may not contain C function *definitions* in its preamble (only declarations),
// so the static C trampoline that forwards this function to pdf_sign_with lives
// separately, in signing.go.

/*
#include <stdint.h>
#include "pdf.h"
*/
import "C"

import (
	"runtime/cgo"
	"unsafe"
)

// goSignHashTrampoline is the C-ABI callback (PdfSignHashFn) handed to
// pdf_sign_with. The Go closure is reached through a runtime/cgo.Handle passed
// as ctx. It writes the raw RSA PKCS#1 v1.5 signature (computed by the closure
// over SHA-256 of data) into sig_buf, respecting sig_cap, and returns 0 on
// success / non-zero on failure.
//
//export goSignHashTrampoline
func goSignHashTrampoline(
	ctx unsafe.Pointer,
	data *C.uint8_t, dataLen C.uintptr_t,
	sigBuf *C.uint8_t, sigCap C.uintptr_t,
	sigLen *C.uintptr_t,
) C.int {
	h := *(*cgo.Handle)(ctx)
	fn, ok := h.Value().(func([]byte) ([]byte, error))
	if !ok {
		return 1
	}
	input := C.GoBytes(unsafe.Pointer(data), C.int(dataLen))
	sig, err := fn(input)
	if err != nil {
		return 1
	}
	if C.uintptr_t(len(sig)) > sigCap {
		return 2 // signer's output does not fit the reserved buffer
	}
	if len(sig) > 0 {
		dst := unsafe.Slice((*byte)(unsafe.Pointer(sigBuf)), int(sigCap))
		copy(dst, sig)
	}
	*sigLen = C.uintptr_t(len(sig))
	return 0
}

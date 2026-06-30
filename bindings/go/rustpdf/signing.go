package rustpdf

// Deferred / external (HSM) signing — issue #41 P0. The private key never
// reaches this library: either the caller supplies a sign-hash closure
// (Model A — SignWith) or drives a two-phase flow (Model B — BeginSigning /
// CompleteSignature) and builds the CMS container itself.

/*
#include <stdint.h>
#include "pdf.h"

// Forward declaration of the exported Go callback (defined in signing_cb.go).
extern int goSignHashTrampoline(void *ctx, uint8_t *data, uintptr_t data_len,
                                uint8_t *sig_buf, uintptr_t sig_cap, uintptr_t *sig_len);

// A file using //export may not define C functions, so this trampoline
// forwarder lives here (not in signing_cb.go). The cast bridges the non-const
// exported signature to PdfSignHashFn's const-qualified `data`.
static PdfStatus cgo_sign_with(const uint8_t *pdf, uintptr_t pdf_len,
                               const uint8_t *cert_der, uintptr_t cert_len,
                               const uint8_t *const *chain_ptrs, const uintptr_t *chain_lens,
                               uintptr_t chain_count, const PdfSigningOptions *params,
                               void *ctx, unsigned char **out_ptr, uintptr_t *out_len) {
    return pdf_sign_with(pdf, pdf_len, cert_der, cert_len, chain_ptrs, chain_lens,
                         chain_count, params, (PdfSignHashFn)goSignHashTrampoline, ctx,
                         out_ptr, out_len);
}
*/
import "C"

import (
	"crypto/sha256"
	"runtime"
	"runtime/cgo"
	"unsafe"
)

// Certify is the DocMDP certification level applied by the first (certifying)
// signature.
type Certify int

const (
	// CertifyNone — not a certifying signature.
	CertifyNone Certify = 0
	// CertifyLocked — /P 1, no changes permitted after signing.
	CertifyLocked Certify = 1
	// CertifyForms — /P 2, form-filling and signing permitted.
	CertifyForms Certify = 2
	// CertifyFormsAndAnnotations — /P 3, form-filling, signing and annotations permitted.
	CertifyFormsAndAnnotations Certify = 3
)

// SignaturePolicy is a signature-policy identifier (PAdES-EPES / ICP-Brasil
// AD-RB). HashAlgorithmOID defaults to SHA-256 when empty; URI is the optional
// SPURI qualifier.
type SignaturePolicy struct {
	OID              string
	Hash             []byte
	HashAlgorithmOID string
	URI              string
}

// SigningOptions configures deferred / external signing (issue #41 P0). Empty
// strings are treated as absent; ContainerSize 0 uses the library default
// (8192) — raise it for large cloud-HSM CMS containers.
type SigningOptions struct {
	Reason        string
	Location      string
	Name          string
	PAdES         bool
	Certify       Certify
	ContainerSize int
	Policy        *SignaturePolicy
}

// SignatureField is a signature field discovered in a PDF (a pre-signing
// inventory entry). Signed is true when the field already carries a signature.
type SignatureField struct {
	Name   string
	Signed bool
}

// SigningSession is an in-progress two-phase signature (Model B): Document holds
// the prepared PDF (with a zero-filled /Contents placeholder) and Bytes the
// exact bytes the signature covers. Hand Hash to a remote signer, build the CMS
// container, then call Complete.
type SigningSession struct {
	document []byte
	bytes    []byte
}

// Document returns the prepared PDF (with a zero-filled /Contents placeholder).
func (s *SigningSession) Document() []byte { return s.document }

// Bytes returns the exact bytes covered by the signature (the two ByteRange
// segments).
func (s *SigningSession) Bytes() []byte { return s.bytes }

// Hash returns SHA-256 of Bytes — the value a remote HSM signs.
func (s *SigningSession) Hash() [32]byte { return sha256.Sum256(s.bytes) }

// Complete embeds a finished DER CMS / PKCS#7 container into the prepared
// document, returning the final signed PDF (phase 2).
func (s *SigningSession) Complete(container []byte) ([]byte, error) {
	return CompleteSignature(s.document, container)
}

// buildSigningOptions fills a C PdfSigningOptions from opts and returns it with a
// cleanup func. A nil opts yields a zero struct (all fields absent).
func buildSigningOptions(opts *SigningOptions) (C.PdfSigningOptions, func()) {
	var p C.PdfSigningOptions
	if opts == nil {
		return p, func() {}
	}
	var frees []func()
	free := func() {
		for _, f := range frees {
			f()
		}
	}
	str := func(s string) *C.char {
		if s == "" {
			return nil
		}
		c := C.CString(s)
		frees = append(frees, func() { C.free(unsafe.Pointer(c)) })
		return c
	}
	p.reason = str(opts.Reason)
	p.location = str(opts.Location)
	p.name = str(opts.Name)
	if opts.PAdES {
		p.pades = 1
	}
	p.certification = C.int(opts.Certify)
	if opts.ContainerSize > 0 {
		p.estimated_size = C.uintptr_t(opts.ContainerSize)
	}
	if pol := opts.Policy; pol != nil {
		p.policy_oid = str(pol.OID)
		if len(pol.Hash) > 0 {
			h := C.CBytes(pol.Hash)
			frees = append(frees, func() { C.free(h) })
			p.policy_hash = (*C.uint8_t)(h)
			p.policy_hash_len = C.uintptr_t(len(pol.Hash))
		}
		p.policy_hash_alg_oid = str(pol.HashAlgorithmOID)
		p.policy_uri = str(pol.URI)
	}
	return p, free
}

// SignWith signs pdf without handing this library a key (Model A — remote
// signer). It builds the CMS signed attributes and calls signHash for the raw
// RSA PKCS#1 v1.5 signature over SHA-256 of the supplied bytes, then assembles
// and embeds the CMS. certDER is the signer certificate; chain are intermediate
// certificates (DER), supplied independently of the key. options may be nil.
func SignWith(
	pdf, certDER []byte,
	signHash func([]byte) ([]byte, error),
	chain [][]byte,
	options *SigningOptions,
) ([]byte, error) {
	params, freeOpts := buildSigningOptions(options)
	defer freeOpts()

	var pinner runtime.Pinner
	defer pinner.Unpin()
	chainPtrs, chainLens := pinAll(&pinner, chain)

	handle := cgo.NewHandle(signHash)
	defer handle.Delete()
	hh := handle

	return takeBytes(func(out **C.uchar, n *C.uintptr_t) C.PdfStatus {
		st := C.cgo_sign_with(
			uptr(pdf), C.uintptr_t(len(pdf)),
			uptr(certDER), C.uintptr_t(len(certDER)),
			ppHead(chainPtrs), upHead(chainLens), C.uintptr_t(len(chain)),
			&params, unsafe.Pointer(&hh), out, n)
		runtime.KeepAlive(pdf)
		runtime.KeepAlive(certDER)
		runtime.KeepAlive(chain)
		runtime.KeepAlive(chainPtrs)
		runtime.KeepAlive(chainLens)
		runtime.KeepAlive(&hh)
		return st
	})
}

// BeginSigning prepares pdf for deferred signing (Model B — phase 1), returning
// a SigningSession whose Hash you send to a remote HSM. Build the CMS container,
// then call SigningSession.Complete (or CompleteSignature). options may be nil.
func BeginSigning(pdf []byte, options *SigningOptions) (*SigningSession, error) {
	params, freeOpts := buildSigningOptions(options)
	defer freeOpts()

	var docPtr, tbsPtr *C.uchar
	var docLen, tbsLen C.uintptr_t
	st := C.pdf_sign_begin(
		uptr(pdf), C.uintptr_t(len(pdf)), &params,
		&docPtr, &docLen, &tbsPtr, &tbsLen)
	runtime.KeepAlive(pdf)
	if err := check(st); err != nil {
		return nil, err
	}
	var doc, tbs []byte
	if docPtr != nil && docLen != 0 {
		doc = C.GoBytes(unsafe.Pointer(docPtr), C.int(docLen))
	} else {
		doc = []byte{}
	}
	if tbsPtr != nil && tbsLen != 0 {
		tbs = C.GoBytes(unsafe.Pointer(tbsPtr), C.int(tbsLen))
	} else {
		tbs = []byte{}
	}
	C.pdf_buffer_free(docPtr, docLen)
	C.pdf_buffer_free(tbsPtr, tbsLen)
	return &SigningSession{document: doc, bytes: tbs}, nil
}

// CompleteSignature embeds a complete DER CMS / PKCS#7 container into a prepared
// document (from BeginSigning), producing the final signed PDF (Model B —
// phase 2).
func CompleteSignature(document, container []byte) ([]byte, error) {
	return takeBytes(func(out **C.uchar, n *C.uintptr_t) C.PdfStatus {
		st := C.pdf_sign_complete(
			uptr(document), C.uintptr_t(len(document)),
			uptr(container), C.uintptr_t(len(container)), out, n)
		runtime.KeepAlive(document)
		runtime.KeepAlive(container)
		return st
	})
}

// ListSignatures lists the signature fields in pdf (detect existing signatures
// before signing). An empty slice means there are no signature fields.
func ListSignatures(pdf []byte) ([]SignatureField, error) {
	b, err := takeBytes(func(out **C.uchar, n *C.uintptr_t) C.PdfStatus {
		st := C.pdf_list_signatures(uptr(pdf), C.uintptr_t(len(pdf)), out, n)
		runtime.KeepAlive(pdf)
		return st
	})
	if err != nil {
		return nil, err
	}
	var fields []SignatureField
	for _, line := range splitLines(string(b)) {
		tab := indexByte(line, '\t')
		if tab < 0 {
			continue
		}
		fields = append(fields, SignatureField{
			Name:   line[tab+1:],
			Signed: line[:tab] == "1",
		})
	}
	return fields, nil
}

// splitLines splits on '\n', dropping empty lines (mirrors the C# parser).
func splitLines(s string) []string {
	var out []string
	start := 0
	for i := 0; i < len(s); i++ {
		if s[i] == '\n' {
			if i > start {
				out = append(out, s[start:i])
			}
			start = i + 1
		}
	}
	if start < len(s) {
		out = append(out, s[start:])
	}
	return out
}

func indexByte(s string, b byte) int {
	for i := 0; i < len(s); i++ {
		if s[i] == b {
			return i
		}
	}
	return -1
}

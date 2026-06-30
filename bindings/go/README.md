# rustpdf (Go binding)

Idiomatic Go binding for the `rust-pdf` core over its C ABI (`libpdf_ffi`), via
**cgo**. It covers the whole product surface: vector graphics, embedded/subsetted
fonts and text, wrapping paragraphs, images, **PDF/A** (levels 1b–3a),
**tagged/accessible** output, embedded-file attachments, **AcroForm** fields,
manipulation (merge/split/rotate/optimize/incremental update), **text
extraction**, **page rendering** (page to PNG image), **encryption** (RC4 /
AES-128 / AES-256) and **digital signatures** (PKCS#7 / PAdES) — plus **feature
licensing**.

Files (package `rustpdf`):

* `rustpdf.go` — cgo preamble, package-level funcs (`Version`, `ActivateLicense`,
  `ExtractText`, `Sign`, `Timestamp`, `AddDss`), enums, error type, helpers;
* `document.go` — the `Document` authoring type;
* `editable.go` — the `EditableDoc` manipulation type.

## Installing (consumers)

```sh
go get github.com/rustpdf/rustpdf/bindings/go/rustpdf@latest
```

The module is **self-contained**: `pdf.h` is vendored alongside the sources and
a prebuilt static `libpdf_ffi.a` for your platform is vendored under
`rustpdf/lib/<os>_<arch>/`, so the default build statically links it with no
external native library to install. cgo (a C toolchain + `CGO_ENABLED=1`, the
default) is the only requirement. Supported slices: `darwin/arm64`,
`darwin/amd64`, `linux/amd64`, `linux/arm64`, `windows/amd64`.

## Building (in-repo development)

Inside the monorepo the static libs are **not** present (they are ~50MB each and
are staged only at release). Build the native library and use the `rustpdf_dev`
tag to link the dynamic library from the build tree instead:

```sh
cargo build -p pdf-ffi
cd bindings/go && go test -tags rustpdf_dev ./...   # or: make go-test
```

## Releasing (maintainers)

Go has no upload registry — publishing is a git tag. Because the module lives in
a subdirectory, the consumer tag is **prefixed** `bindings/go/vX.Y.Z`.

The `.github/workflows/release-go.yml` pipeline does it: push a `go-v0.1.0` tag
(this trigger tag only kicks off CI) and it builds the five `libpdf_ffi.a` slices
(with the production `RUSTPDF_LICENSE_PUBKEY`), statically smoke-tests each, then
force-adds them into a single commit and pushes the `bindings/go/v0.1.0` tag —
the dev branch never carries the binaries. `go get …@v0.1.0` then resolves it.

Manual fallback (one host can only build its own slice):

```sh
make go-dist          # build per-platform libpdf_ffi.a into rustpdf/lib/*
git add -f bindings/go/rustpdf/lib/*/libpdf_ffi.a
git commit -m "go: stage native libs for v0.1.0"
git tag bindings/go/v0.1.0 && git push origin bindings/go/v0.1.0
```

The `.a` files are kept off the development branch by `rustpdf/lib/.gitignore`.

## Quick start

```go
package main

import (
	"fmt"

	rustpdf "github.com/rustpdf/rustpdf/bindings/go/rustpdf"
)

func main() {
	// A token in RUSTPDF_LICENSE is auto-activated; or call ActivateLicense.
	_ = rustpdf.ActivateLicense(token)

	d, _ := rustpdf.New()
	defer d.Close()
	_ = d.PdfaLevel(rustpdf.A2a)
	_ = d.SetInfo(rustpdf.Info{Title: "Report"})
	f, _ := d.AddFontFile("assets/fonts/Roboto-Regular.ttf")
	_ = d.AddPage()
	_ = d.ShowText(f, 20, 72, 760, "Title", 1) // heading level 1 = H1
	data, _ := d.ToBytes()

	text, _ := rustpdf.ExtractText(data)
	fmt.Println(text)

	ed, _ := rustpdf.Load(data)
	defer ed.Close()
	_ = ed.Encrypt(rustpdf.AES256, "", "owner", false)
	_ = ed.Save("secured.pdf")

	// Open a password-protected PDF (user or owner password).
	enc, _ := os.ReadFile("secured.pdf")
	locked, _ := rustpdf.LoadWithPassword(enc, "owner")
	defer locked.Close()

	signed, _ := rustpdf.Sign(data, keyDER, certDER, rustpdf.SignOptions{PAdES: true})
	_ = signed
}
```

## Deferred / HSM signing

Sign without ever handing the library a private key. The key stays in an HSM,
cloud KMS, smartcard or PKI token; you supply only the signer certificate and a
way to produce the raw RSA PKCS#1 v1.5 signature over SHA-256. This works with
any PKI (eIDAS, AATL and other trust frameworks).

**Model A** is one call with a sign-hash callback. `SignWith` assembles the CMS
signed attributes, calls your closure for the raw signature, then embeds the
finished CMS. `chain` are intermediate certificates (DER), supplied
independently of the key; `options` may be `nil`.

```go
pdf, _ := os.ReadFile("contract.pdf")
certDER, _ := os.ReadFile("signer.der") // X.509 certificate (DER), no key

signed, err := rustpdf.SignWith(pdf, certDER,
	func(tbs []byte) ([]byte, error) {
		return hsm.SignRSA(tbs) // remote HSM / KMS / smartcard
	},
	nil, // chain: intermediate certs (DER)
	&rustpdf.SigningOptions{Reason: "Approved", PAdES: true})
if err != nil {
	log.Fatal(err)
}
_ = os.WriteFile("contract.signed.pdf", signed, 0o644)
```

**Model B** is a two-phase flow for an asynchronous signer, where you build the
DER CMS / PKCS#7 container yourself. `BeginSigning` returns a `SigningSession`
exposing `Document()` (the placeholder PDF), `Bytes()` (the covered bytes),
`Hash()` (SHA-256 of those bytes) and `Complete()`.

```go
sess, _ := rustpdf.BeginSigning(pdf, &rustpdf.SigningOptions{PAdES: true})
digest := sess.Hash()                       // [32]byte sent to the remote signer
container := buildCMS(digest, certDER, sig) // your DER CMS / PKCS#7
final, _ := sess.Complete(container)        // or rustpdf.CompleteSignature(sess.Document(), container)
```

List existing signature fields before signing with
`rustpdf.ListSignatures(pdf) ([]SignatureField, error)` (each is
`SignatureField{Name, Signed}`). `SigningOptions` also carries `Location`,
`Name`, a `Certify` DocMDP level (`CertifyNone`/`CertifyLocked`/`CertifyForms`/
`CertifyFormsAndAnnotations`, first signature only), `ContainerSize` (reserved
`/Contents` bytes; `0` = default, raise it for large cloud CMS containers) and a
PAdES-EPES `Policy` (`SignaturePolicy{OID, Hash, HashAlgorithmOID, URI}`).

Corporate features (PDF/A, signing, encryption, accessibility, page rendering
— a **Pro** feature) require a license;
without one they return an `*Error`. See [`docs/LICENSING.md`](../../docs/LICENSING.md).

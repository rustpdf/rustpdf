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

	signed, _ := rustpdf.Sign(data, keyDER, certDER, rustpdf.SignOptions{PAdES: true})
	_ = signed
}
```

Corporate features (PDF/A, signing, encryption, accessibility, page rendering
— a **Pro** feature) require a license;
without one they return an `*Error`. See [`docs/LICENSING.md`](../../docs/LICENSING.md).

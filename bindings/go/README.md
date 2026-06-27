# rustpdf (Go binding)

Idiomatic Go binding for the `rust-pdf` core over its C ABI (`libpdf_ffi`), via
**cgo**. It covers the whole product surface: vector graphics, embedded/subsetted
fonts and text, wrapping paragraphs, images, **PDF/A** (levels 1b–3a),
**tagged/accessible** output, embedded-file attachments, **AcroForm** fields,
manipulation (merge/split/rotate/optimize/incremental update), **text
extraction**, **encryption** (RC4 / AES-128 / AES-256) and **digital signatures**
(PKCS#7 / PAdES) — plus **feature licensing**.

Files (package `rustpdf`):

* `rustpdf.go` — cgo preamble, package-level funcs (`Version`, `ActivateLicense`,
  `ExtractText`, `Sign`, `Timestamp`, `AddDss`), enums, error type, helpers;
* `document.go` — the `Document` authoring type;
* `editable.go` — the `EditableDoc` manipulation type.

## Building

Build the native library first, then use the package:

```sh
cargo build -p pdf-ffi
cd bindings/go && go test ./...
```

The cgo directives point at `../../../include` (header) and
`../../../target/{debug,release}` (library). On macOS the built dylib's install
name is absolute, so binaries find it in the build tree automatically; on Linux
an `-rpath` to the build tree is added. To run a binary elsewhere, copy
`libpdf_ffi.*` next to it (or set `LD_LIBRARY_PATH` / `DYLD_LIBRARY_PATH`), and
pass `CGO_LDFLAGS`/`CGO_CFLAGS` if your layout differs.

## Quick start

```go
package main

import (
	"fmt"

	rustpdf "github.com/rust-pdf/rustpdf/rustpdf"
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

Corporate features (PDF/A, signing, encryption, accessibility) require a license;
without one they return an `*Error`. See [`docs/LICENSING.md`](../../docs/LICENSING.md).

// Minimal smoke test for CI release builds. Exercises the surface — basic
// vector graphics + serialization — against the vendored per-platform static
// library. The full surface is covered by the full suite (`make go-test`), not
// here.
//
// Run with default build tags so it statically links the vendored per-platform
// libpdf_ffi.a — this verifies the published-shape module locates + links the
// native library and round-trips a document. Exits non-zero on any failure.
package main

import (
	"bytes"
	"fmt"
	"os"

	rustpdf "github.com/rustpdf/rustpdf/bindings/go/rustpdf"
)

func main() {
	d, err := rustpdf.New()
	if err != nil {
		fail(err)
	}
	defer d.Close()

	if err := d.AddPage(); err != nil {
		fail(err)
	}
	if err := d.SetFillRGB(0.1, 0.2, 0.8); err != nil {
		fail(err)
	}
	if err := d.Rect(72, 72, 200, 100); err != nil {
		fail(err)
	}
	if err := d.Fill(); err != nil {
		fail(err)
	}

	data, err := d.ToBytes()
	if err != nil {
		fail(err)
	}
	if !bytes.HasPrefix(data, []byte("%PDF-")) {
		fail(fmt.Errorf("output does not start with %%PDF-"))
	}

	fmt.Printf("smoke OK %s %d bytes\n", rustpdf.Version(), len(data))
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, "smoke FAIL:", err)
	os.Exit(1)
}

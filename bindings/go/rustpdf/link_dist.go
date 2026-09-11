//go:build !rustpdf_dev

// Distribution linkage: statically link the prebuilt libpdf_ffi.a vendored under
// lib/<os>_<arch>/, so a consumer's `go get` + `go build` works with no external
// native library. The .a files are produced per platform by `make go-dist`
// (built in release CI) and committed
// at the released tag — they are NOT present on the development branch, where the
// `rustpdf_dev` tag (link_dev.go) is used instead.
//
// The trailing system libraries are the static dependencies the Rust staticlib
// pulls in; verify per target with:
//
//	cargo rustc -p pdf-ffi --release --crate-type staticlib -- --print native-static-libs
//
// (macOS reports `-lc -lm -liconv -lSystem`; -lc/-lSystem are implicit.)
package rustpdf

/*
#cgo darwin,amd64  LDFLAGS: -L${SRCDIR}/lib/darwin_amd64 -lpdf_ffi -liconv -lm
#cgo darwin,arm64  LDFLAGS: -L${SRCDIR}/lib/darwin_arm64 -lpdf_ffi -liconv -lm
#cgo linux,amd64   LDFLAGS: -L${SRCDIR}/lib/linux_amd64 -lpdf_ffi -lpthread -ldl -lm -lrt
#cgo linux,arm64   LDFLAGS: -L${SRCDIR}/lib/linux_arm64 -lpdf_ffi -lpthread -ldl -lm -lrt
#cgo windows,amd64 LDFLAGS: -L${SRCDIR}/lib/windows_amd64 -lpdf_ffi -lws2_32 -luserenv -lbcrypt -lntdll -ladvapi32 -lkernel32
*/
import "C"

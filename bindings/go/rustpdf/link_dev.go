//go:build rustpdf_dev

// Development linkage: link the dynamic libpdf_ffi from the monorepo build tree
// (`cargo build -p pdf-ffi`). Used by `make go-test` (which passes
// `-tags rustpdf_dev`) so the in-repo suite runs without first staging the
// per-platform static libraries. On macOS the built dylib's install name is
// absolute, so binaries find it in the build tree; on Linux an -rpath is added.
package rustpdf

/*
#cgo LDFLAGS: -L${SRCDIR}/../../../target/debug -L${SRCDIR}/../../../target/release -lpdf_ffi
#cgo linux LDFLAGS: -Wl,-rpath,${SRCDIR}/../../../target/debug -Wl,-rpath,${SRCDIR}/../../../target/release
*/
import "C"

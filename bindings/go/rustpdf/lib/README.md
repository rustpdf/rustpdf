# Vendored native libraries

Each `<os>_<arch>/` directory holds the prebuilt static library `libpdf_ffi.a`
for one platform. `link_dist.go` (the default build) statically links the one
matching the host `GOOS`/`GOARCH`, so a consumer's `go get` + `go build` needs no
external native library.

These archives are **not** committed on the development branch — they are built
by `make go-dist` (in release CI, with the production `RUSTPDF_LICENSE_PUBKEY`)
and included in the released tag. During in-repo development the `rustpdf_dev`
build tag links the dynamic library from the monorepo build tree instead, so the
test suite (`make go-test`) runs without staging these files.

Layout:

```
lib/darwin_amd64/libpdf_ffi.a
lib/darwin_arm64/libpdf_ffi.a
lib/linux_amd64/libpdf_ffi.a
lib/linux_arm64/libpdf_ffi.a
lib/windows_amd64/libpdf_ffi.a
```

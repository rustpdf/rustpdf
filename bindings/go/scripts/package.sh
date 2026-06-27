#!/usr/bin/env bash
# Build the static libpdf_ffi.a for every buildable target and stage it under
# bindings/go/rustpdf/lib/<os>_<arch>/, so the Go module is self-contained: a
# consumer's `go get` + `go build` (default tags) statically links the prebuilt
# archive with no external native library.
#
# Best effort, like bindings/swift/scripts/package.sh — it builds whatever Rust
# targets can be installed on this host and skips the rest. Run it in release CI
# (with the production RUSTPDF_LICENSE_PUBKEY exported) on Linux/macOS/Windows
# runners so all five slices get populated, then commit + tag the result; the
# .a files are NOT kept on the development branch.
#
# Usage:  bash bindings/go/scripts/package.sh
# Honors $CARGO (defaults to `cargo`).
set -euo pipefail

CARGO="${CARGO:-cargo}"
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$ROOT"
LIBROOT="bindings/go/rustpdf/lib"

# Rust target triple -> lib/<os>_<arch> directory name (matches link_dist.go).
stage() { # <triple> <os_arch>
    local triple="$1"
    local key="$2"
    local dst="$LIBROOT/$key"
    rustup target add "$triple" >/dev/null 2>&1 || true
    if "$CARGO" build -p pdf-ffi --release --target "$triple" >/dev/null 2>&1; then
        mkdir -p "$dst"
        cp "target/$triple/release/libpdf_ffi.a" "$dst/libpdf_ffi.a"
        echo "    staged $key  ($(du -h "$dst/libpdf_ffi.a" | cut -f1))"
    else
        echo "    skip   $key  (target $triple not buildable on this host)"
    fi
}

echo "==> staging Go native libs under $LIBROOT"
stage aarch64-apple-darwin       darwin_arm64
stage x86_64-apple-darwin        darwin_amd64
stage x86_64-unknown-linux-gnu   linux_amd64
stage aarch64-unknown-linux-gnu  linux_arm64
stage x86_64-pc-windows-gnu      windows_amd64
echo "==> done"

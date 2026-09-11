#!/usr/bin/env bash
# Build the dynamic libpdf_ffi for every buildable target and stage it under
# bindings/rust/dist/, plus a versioned zip per platform. The Rust binding loads
# this cdylib at RUN TIME (libloading) — the published crate carries no binary —
# so these are the artifacts a customer drops beside their app (or points
# RUSTPDF_LIB at). The crate itself is published separately to the private cargo
# registry (see .github/workflows/release-rust.yml).
#
# Best effort, like bindings/swift|go|delphi/scripts/package.sh — it builds
# whatever Rust targets can be installed on this host and skips the rest. Run it
# on Linux/macOS/Windows runners so every slice is populated. dist/ is gitignored (large libs).
#
# Usage:  bash bindings/rust/scripts/package.sh
# Honors $CARGO (defaults to `cargo`).
set -euo pipefail

CARGO="${CARGO:-cargo}"
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$ROOT"

DIST="bindings/rust/dist"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' bindings/rust/Cargo.toml | head -1)"

# Dynamic-lib file name produced by cargo for a given target triple.
libname() { # <triple>
    case "$1" in
        *apple*)   echo "libpdf_ffi.dylib" ;;
        *windows*) echo "pdf_ffi.dll" ;;
        *)         echo "libpdf_ffi.so" ;;
    esac
}

stage() { # <triple> <os-arch>
    local triple="$1"
    local key="$2"
    local lib
    lib="$(libname "$triple")"
    local dst="$DIST/$key"
    rustup target add "$triple" >/dev/null 2>&1 || true
    if "$CARGO" build -p pdf-ffi --release --target "$triple" >/dev/null 2>&1; then
        mkdir -p "$dst"
        cp "target/$triple/release/$lib" "$dst/$lib"
        echo "    staged $key  ($(du -h "$dst/$lib" | cut -f1))"
    else
        echo "    skip   $key  (target $triple not buildable on this host)"
    fi
}

echo "==> staging rustpdf native libs (v$VERSION) under $DIST"
rm -rf "$DIST"
mkdir -p "$DIST"

stage aarch64-apple-darwin       macos-arm64
stage x86_64-apple-darwin        macos-x86_64
stage x86_64-unknown-linux-gnu   linux-x86_64
stage aarch64-unknown-linux-gnu  linux-arm64
stage x86_64-pc-windows-gnu      windows-x86_64

# Ship the C header + README alongside the libs for reference.
cp include/pdf.h "$DIST/pdf.h"
cp bindings/rust/README.md "$DIST/README.md"

if command -v zip >/dev/null 2>&1; then
    out="rustpdf-native-$VERSION.zip"
    (cd "$DIST" && zip -rq "$out" . -x "$out")
    echo "==> wrote $DIST/$out"
else
    echo "==> zip not found; left unpacked slices under $DIST"
fi
echo "==> done"

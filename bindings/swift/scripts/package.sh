#!/usr/bin/env bash
# Assemble a distributable Swift package: the RustPdf Swift sources + a binary
# `RustPdfFFI.xcframework` (a static `libpdf_ffi.a` + the C header, one slice per
# Apple platform), zipped under bindings/swift/dist/.
#
# SwiftPM has no central paid registry, so customers consume either:
#   * the staged local package (unzip, then `.package(path:)`), or
#   * the standalone `RustPdfFFI.xcframework.zip` + its checksum, referenced by a
#     `.binaryTarget(url:checksum:)` hosted on your own URL (e.g. rustpdf.dev).
#
# The native library is linked STATICALLY, so the binding works inside an iOS
# app bundle (no dlopen of an arbitrary path). It builds for every Apple target
# whose Rust std can be installed (best effort) — at minimum the host macOS arch.
#
# Usage:  bash bindings/swift/scripts/package.sh [VERSION]
# Honors $CARGO (defaults to `cargo`).
set -euo pipefail

CARGO="${CARGO:-cargo}"
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$ROOT"

if ! command -v xcodebuild >/dev/null 2>&1; then
    echo "error: xcodebuild not found — an Xcode install is required to build the xcframework" >&2
    exit 1
fi

VERSION="${1:-$(grep -m1 -E '^version = ' Cargo.toml | sed -E 's/version = "(.*)"/\1/')}"
# The staged package dir is named `RustPdf` (not versioned) so that a path-based
# dependency keeps a stable identity: `.package(path: "RustPdf")` →
# `.product(name: "RustPdf", package: "RustPdf")`. The zip filename carries the
# version.
DIST="bindings/swift/dist"
STAGE="$DIST/RustPdf"
XCF="$STAGE/RustPdfFFI.xcframework"
HEADERS="$DIST/.headers"
LIBDIR="$DIST/.lib"
echo "==> packaging rustpdf-swift ${VERSION}"

rm -rf "$STAGE" "$HEADERS" "$LIBDIR" "$XCF"
mkdir -p "$STAGE/Sources/RustPdf" "$HEADERS" "$LIBDIR"

# --- headers + module map the xcframework exposes as `import CRustPdf` --------
cp include/pdf.h "$HEADERS/pdf.h"
sed -i '' 's/^PdfStatus /int /' "$HEADERS/pdf.h"   # status returns → int (Int32)
cat > "$HEADERS/module.modulemap" <<'EOF'
module CRustPdf {
    header "pdf.h"
    export *
}
EOF

# --- build the static lib per target, lipo into per-platform slices -----------
build_target() { # triple -> prints the .a path on success, nothing on failure
    rustup target add "$1" >/dev/null 2>&1 || true
    if "$CARGO" build -p pdf-ffi --release --target "$1" >/dev/null 2>&1; then
        printf '%s' "target/$1/release/libpdf_ffi.a"
    fi
}

# macOS (arm64 + x86_64 → universal)
MAC_ARM="$(build_target aarch64-apple-darwin)"
MAC_X64="$(build_target x86_64-apple-darwin)"
MAC_LIB=""
if [ -n "$MAC_ARM" ] || [ -n "$MAC_X64" ]; then
    MAC_LIB="$LIBDIR/macos/libpdf_ffi.a"; mkdir -p "$LIBDIR/macos"
    # shellcheck disable=SC2086
    lipo -create ${MAC_ARM:+"$MAC_ARM"} ${MAC_X64:+"$MAC_X64"} -output "$MAC_LIB"
    echo "    macOS slice:           $(lipo -archs "$MAC_LIB")"
fi

# iOS device (arm64)
IOS_LIB="$(build_target aarch64-apple-ios)"
[ -n "$IOS_LIB" ] && echo "    iOS device slice:      arm64"

# iOS simulator (arm64 + x86_64 → universal)
SIM_ARM="$(build_target aarch64-apple-ios-sim)"
SIM_X64="$(build_target x86_64-apple-ios)"
SIM_LIB=""
if [ -n "$SIM_ARM" ] || [ -n "$SIM_X64" ]; then
    SIM_LIB="$LIBDIR/iossim/libpdf_ffi.a"; mkdir -p "$LIBDIR/iossim"
    # shellcheck disable=SC2086
    lipo -create ${SIM_ARM:+"$SIM_ARM"} ${SIM_X64:+"$SIM_X64"} -output "$SIM_LIB"
    echo "    iOS simulator slice:   $(lipo -archs "$SIM_LIB")"
fi

# --- assemble the xcframework -------------------------------------------------
ARGS=()
[ -n "$MAC_LIB" ]            && ARGS+=(-library "$MAC_LIB" -headers "$HEADERS")
[ -n "$IOS_LIB" ]           && ARGS+=(-library "$IOS_LIB" -headers "$HEADERS")
[ -n "$SIM_LIB" ]           && ARGS+=(-library "$SIM_LIB" -headers "$HEADERS")
if [ ${#ARGS[@]} -eq 0 ]; then
    echo "error: no Apple target could be built (install Rust std with 'rustup target add ...')" >&2
    exit 1
fi
xcodebuild -create-xcframework "${ARGS[@]}" -output "$XCF" >/dev/null
echo "    xcframework:           $XCF"

# --- stage the consumable Swift package ---------------------------------------
cp bindings/swift/Sources/RustPdf/*.swift "$STAGE/Sources/RustPdf/"
cp bindings/swift/README.md "$STAGE/README.md"

cat > "$STAGE/Package.swift" <<'EOF'
// swift-tools-version:5.9
//
// RustPdf — Swift binding for the rust-pdf core. The native library is provided
// as a static binary xcframework, so nothing Rust is built by the consumer.
import PackageDescription

let package = Package(
    name: "RustPdf",
    platforms: [.macOS(.v11), .iOS(.v13)],
    products: [
        .library(name: "RustPdf", targets: ["RustPdf"])
    ],
    targets: [
        .binaryTarget(name: "CRustPdf", path: "RustPdfFFI.xcframework"),
        .target(
            name: "RustPdf",
            dependencies: ["CRustPdf"],
            path: "Sources/RustPdf",
            // The static Rust lib pulls in libiconv (its only non-system dep).
            linkerSettings: [.linkedLibrary("iconv")]
        )
    ]
)
EOF

# --- zip artifacts ------------------------------------------------------------
PKG_ZIP="rustpdf-swift-${VERSION}.zip"
XCF_ZIP="RustPdfFFI-${VERSION}.xcframework.zip"
( cd "$DIST" && zip -qr "$PKG_ZIP" "RustPdf" )
# Standalone xcframework + checksum for URL-hosted .binaryTarget(url:checksum:).
( cd "$STAGE" && zip -qr "../$XCF_ZIP" "RustPdfFFI.xcframework" )

# SwiftPM checksum (for .binaryTarget(url:checksum:)).
CHECKSUM="$(swift package compute-checksum "$DIST/$XCF_ZIP" 2>/dev/null || true)"
[ -n "$CHECKSUM" ] && echo "$CHECKSUM" > "$DIST/$XCF_ZIP.checksum"

# SHA-256 of both zips (same convention as the Delphi archive: "<hash>  <name>"),
# so the site's deploy/sync can verify integrity.
sha256() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"; else shasum -a 256 "$1"; fi; }
( cd "$DIST" && sha256 "$PKG_ZIP" > "$PKG_ZIP.sha256" )
( cd "$DIST" && sha256 "$XCF_ZIP" > "$XCF_ZIP.sha256" )

rm -rf "$HEADERS" "$LIBDIR"

# --- optionally publish to the site's static /downloads/ ----------------------
if [ "${PUBLISH:-0}" = "1" ]; then
    DEST="site/public/downloads"
    mkdir -p "$DEST"
    cp "$DIST/$PKG_ZIP" "$DIST/$PKG_ZIP.sha256" \
       "$DIST/$XCF_ZIP" "$DIST/$XCF_ZIP.sha256" "$DEST/"
    [ -n "$CHECKSUM" ] && cp "$DIST/$XCF_ZIP.checksum" "$DEST/"
    echo "==> published to $DEST/ (served at /downloads/$PKG_ZIP and /downloads/$XCF_ZIP)"
fi

echo "==> done:"
echo "    $DIST/$PKG_ZIP        (local package: unzip + .package(path:))"
echo "    $DIST/$XCF_ZIP   (host this for .binaryTarget(url:checksum:))"
[ -n "$CHECKSUM" ] && echo "    SwiftPM checksum: $CHECKSUM"

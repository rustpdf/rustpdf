#!/usr/bin/env bash
# Assemble a distributable Delphi / Free Pascal package: the RustPdf.pas unit +
# the native cdylib(s) + docs + a sample, zipped under bindings/delphi/dist/.
#
# Delphi has no central registry, so customers consume a versioned archive (or
# the git repo via Boss). This builds the native library for every Rust target
# that is currently installed (best effort) and lays the result out so the unit
# finds it next to the app on Windows, macOS and Linux.
#
# Usage:  bash bindings/delphi/scripts/package.sh [VERSION]
# Honors $CARGO (defaults to `cargo`).
set -euo pipefail

CARGO="${CARGO:-cargo}"
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$ROOT"

VERSION="${1:-$(grep -m1 -E '^version = ' Cargo.toml | sed -E 's/version = "(.*)"/\1/')}"
STAGE="bindings/delphi/dist/rustpdf-delphi-${VERSION}"
echo "==> packaging rustpdf-delphi ${VERSION}"

rm -rf "$STAGE"
mkdir -p "$STAGE/lib" "$STAGE/examples"

# triple -> "friendly-dir lib-filename"
emit_target() {
  case "$1" in
    x86_64-pc-windows-msvc|x86_64-pc-windows-gnu) echo "windows-x64 pdf_ffi.dll" ;;
    i686-pc-windows-msvc|i686-pc-windows-gnu)     echo "windows-x86 pdf_ffi.dll" ;;
    aarch64-pc-windows-msvc)                      echo "windows-arm64 pdf_ffi.dll" ;;
    aarch64-apple-darwin)                         echo "macos-arm64 libpdf_ffi.dylib" ;;
    x86_64-apple-darwin)                          echo "macos-x64 libpdf_ffi.dylib" ;;
    x86_64-unknown-linux-gnu)                     echo "linux-x64 libpdf_ffi.so" ;;
    aarch64-unknown-linux-gnu)                    echo "linux-arm64 libpdf_ffi.so" ;;
    *) echo "" ;;
  esac
}

BUILT=0
if [ -n "${STAGED_LIB_ROOT:-}" ]; then
  # CI path: native libs were built per-platform on separate runners and laid
  # out as <STAGED_LIB_ROOT>/lib/<os-arch>/<file>. Just ingest them, no cargo.
  echo "    using prebuilt libraries from $STAGED_LIB_ROOT/lib"
  cp -R "$STAGED_LIB_ROOT/lib/." "$STAGE/lib/"
  BUILT=$(find "$STAGE/lib" -type f | wc -l | tr -d ' ')
else
  INSTALLED="$(rustup target list --installed 2>/dev/null || echo)"
  for triple in \
    x86_64-pc-windows-msvc i686-pc-windows-msvc aarch64-pc-windows-msvc \
    aarch64-apple-darwin x86_64-apple-darwin \
    x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
  do
    map="$(emit_target "$triple")"
    [ -z "$map" ] && continue
    echo "$INSTALLED" | grep -qx "$triple" || { echo "    skip $triple (target not installed)"; continue; }
    dir="${map%% *}"; lib="${map##* }"
    echo "    build $triple -> lib/$dir/$lib"
    $CARGO build -q -p pdf-ffi --release --target "$triple"
    mkdir -p "$STAGE/lib/$dir"
    cp "target/$triple/release/$lib" "$STAGE/lib/$dir/$lib"
    BUILT=$((BUILT+1))
  done
fi

if [ "$BUILT" -eq 0 ]; then
  echo "!! no native libraries — install a Rust target (rustup target add …) or set STAGED_LIB_ROOT" >&2
  exit 1
fi

cp bindings/delphi/RustPdf.pas        "$STAGE/RustPdf.pas"
cp bindings/delphi/README.md          "$STAGE/README.md"
cp bindings/delphi/boss.json          "$STAGE/boss.json"
cp bindings/delphi/test/run.dpr       "$STAGE/examples/smoke_test.dpr"
cp LICENSES.md                        "$STAGE/LICENSES.md"

cat > "$STAGE/INSTALL.txt" <<EOF
rustpdf-delphi ${VERSION}

1. Add this folder to your Delphi/FPC unit search path and add  uses RustPdf;
2. Deploy the native library for your platform next to your built executable:
     lib/windows-x64/pdf_ffi.dll        -> beside your .exe (must match app bitness)
     lib/macos-universal/libpdf_ffi.dylib   (single-arch builds: macos-arm64 / macos-x64)
     lib/linux-x64/libpdf_ffi.so
   The exact macOS folder name shipped in this archive is listed below under
   "Bundled native libraries"; …or set RUSTPDF_LIB to the library's full path.
3. Every feature (PDF/A, signing, encryption, accessibility) is free.

Bundled native libraries in this archive: $(cd "$STAGE/lib" && ls -1d */ | tr -d '/' | paste -sd', ' -)
EOF

ZIP="bindings/delphi/dist/rustpdf-delphi-${VERSION}.zip"
rm -f "$ZIP" "$ZIP.sha256"
( cd "bindings/delphi/dist" && zip -qr "rustpdf-delphi-${VERSION}.zip" "rustpdf-delphi-${VERSION}" )

# Checksum (shasum on macOS, sha256sum on Linux) — written as "<hash>  <name>".
if command -v sha256sum >/dev/null 2>&1; then
  ( cd "bindings/delphi/dist" && sha256sum "rustpdf-delphi-${VERSION}.zip" > "rustpdf-delphi-${VERSION}.zip.sha256" )
else
  ( cd "bindings/delphi/dist" && shasum -a 256 "rustpdf-delphi-${VERSION}.zip" > "rustpdf-delphi-${VERSION}.zip.sha256" )
fi
echo "==> wrote $ZIP"
( cd "$STAGE" && find . -type f | sort | sed 's/^/    /' )

# Optionally publish the public trial download served at /downloads/ by the site.
if [ "${PUBLISH:-0}" = "1" ]; then
  DEST="site/public/downloads"
  mkdir -p "$DEST"
  cp "$ZIP" "$ZIP.sha256" "$DEST/"
  echo "==> published to $DEST/ (served at /downloads/rustpdf-delphi-${VERSION}.zip)"
fi

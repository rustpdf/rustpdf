#!/usr/bin/env bash
# Refresh the public Swift trial download served by the site.
#
# Pulls the artifacts built by the release-swift CI workflow (the consumable
# package zip + the standalone xcframework zip, each with its .sha256, plus the
# SwiftPM .checksum) from their GitHub Release into site/public/downloads/,
# verifying the checksums. deploy.sh calls this before the rsync, so the bytes
# get baked into the site image and served at https://rustpdf.dev/downloads/.
#
#   ./site/scripts/sync-swift-download.sh            # current version
#   ./site/scripts/sync-swift-download.sh 0.1.0      # a specific version
#
# The repo is private, so a GitHub Release is authenticated storage: this uses
# `gh` (preferred — handles auth) and falls back to curl with $GITHUB_TOKEN.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DEST="$ROOT/site/public/downloads"

VERSION="${1:-${SWIFT_PKG_VERSION:-$(grep -m1 -E '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "(.*)"/\1/')}}"
TAG="swift-v${VERSION}"
PKG_ZIP="rustpdf-swift-${VERSION}.zip"
XCF_ZIP="RustPdfFFI-${VERSION}.xcframework.zip"

# owner/repo from the git remote unless overridden.
REPO="${RUSTPDF_REPO:-$(git -C "$ROOT" config --get remote.origin.url 2>/dev/null \
  | sed -E 's#(git@|https://)github.com[:/]([^/]+/[^/.]+)(\.git)?#\2#')}"

mkdir -p "$DEST"
say() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }

say "fetching $PKG_ZIP / $XCF_ZIP (+.sha256/.checksum) from release $TAG of $REPO"
if command -v gh >/dev/null 2>&1; then
  gh release download "$TAG" --repo "$REPO" --dir "$DEST" --clobber \
    --pattern "$PKG_ZIP" --pattern "$PKG_ZIP.sha256" \
    --pattern "$XCF_ZIP" --pattern "$XCF_ZIP.sha256" --pattern "$XCF_ZIP.checksum"
else
  base="https://github.com/${REPO}/releases/download/${TAG}"
  auth=(); [ -n "${GITHUB_TOKEN:-}" ] && auth=(-H "Authorization: Bearer ${GITHUB_TOKEN}")
  for f in "$PKG_ZIP" "$PKG_ZIP.sha256" "$XCF_ZIP" "$XCF_ZIP.sha256" "$XCF_ZIP.checksum"; do
    curl -fSL "${auth[@]}" "$base/$f" -o "$DEST/$f"
  done
fi

say "verifying checksums"
check() { if command -v sha256sum >/dev/null 2>&1; then sha256sum -c "$1"; else shasum -a 256 -c "$1"; fi; }
( cd "$DEST" && check "$PKG_ZIP.sha256" && check "$XCF_ZIP.sha256" )

say "ok — $DEST/$PKG_ZIP ($(cd "$DEST" && du -h "$PKG_ZIP" | cut -f1)) will be served at /downloads/$PKG_ZIP"

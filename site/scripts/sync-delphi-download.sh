#!/usr/bin/env bash
# Refresh the public Delphi trial download served by the site.
#
# Pulls the versioned archive (zip + .sha256) built by the release-delphi CI
# workflow from its GitHub Release into site/public/downloads/, verifying the
# checksum. deploy.sh calls this before the rsync, so the bytes get baked into
# the site image and served at https://rustpdf.dev/downloads/.
#
#   ./site/scripts/sync-delphi-download.sh            # current version
#   ./site/scripts/sync-delphi-download.sh 0.1.0      # a specific version
#
# The repo is private, so a GitHub Release is authenticated storage: this uses
# `gh` (preferred — handles auth) and falls back to curl with $GITHUB_TOKEN.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DEST="$ROOT/site/public/downloads"

VERSION="${1:-${DELPHI_PKG_VERSION:-$(grep -m1 -E '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "(.*)"/\1/')}}"
TAG="delphi-v${VERSION}"
ZIP="rustpdf-delphi-${VERSION}.zip"

# owner/repo from the git remote unless overridden.
REPO="${RUSTPDF_REPO:-$(git -C "$ROOT" config --get remote.origin.url 2>/dev/null \
  | sed -E 's#(git@|https://)github.com[:/]([^/]+/[^/.]+)(\.git)?#\2#')}"

mkdir -p "$DEST"
say() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }

say "fetching $ZIP (+.sha256) from release $TAG of $REPO"
if command -v gh >/dev/null 2>&1; then
  gh release download "$TAG" --repo "$REPO" --dir "$DEST" --clobber \
    --pattern "$ZIP" --pattern "$ZIP.sha256"
else
  base="https://github.com/${REPO}/releases/download/${TAG}"
  auth=(); [ -n "${GITHUB_TOKEN:-}" ] && auth=(-H "Authorization: Bearer ${GITHUB_TOKEN}")
  curl -fSL "${auth[@]}" "$base/$ZIP"         -o "$DEST/$ZIP"
  curl -fSL "${auth[@]}" "$base/$ZIP.sha256"  -o "$DEST/$ZIP.sha256"
fi

say "verifying checksum"
if command -v sha256sum >/dev/null 2>&1; then
  ( cd "$DEST" && sha256sum -c "$ZIP.sha256" )
else
  ( cd "$DEST" && shasum -a 256 -c "$ZIP.sha256" )
fi

say "ok — $DEST/$ZIP ($(cd "$DEST" && du -h "$ZIP" | cut -f1)) will be served at /downloads/$ZIP"

#!/usr/bin/env bash
# Bump the rust-pdf version EVERYWHERE with a single argument.
#
#   scripts/bump-version.sh 0.4.2
#
# The workspace Cargo.toml is the single source of truth; this script stamps the
# same version into every binding manifest the release process touches (see
# docs/RELEASING.md §1), regenerates the Node platform packages, and refreshes
# Cargo.lock. It does NOT commit, tag, or push — it only edits files, so you can
# review the diff and then run the release.
#
# Idempotent and portable (BSD/macOS + GNU/Linux sed).
set -euo pipefail

NEW="${1:-}"
if [ -z "$NEW" ]; then
  echo "usage: $0 <new-version>   e.g. $0 0.4.2" >&2
  exit 2
fi
# Validate semver-ish X.Y.Z (optionally with a -prerelease suffix).
if ! printf '%s' "$NEW" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-.][0-9A-Za-z.]+)?$'; then
  echo "error: '$NEW' is not a valid version (expected X.Y.Z)" >&2
  exit 2
fi

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

# Single source of truth: the workspace [workspace.package] version.
OLD="$(grep -m1 -E '^version = "[0-9]' Cargo.toml | sed -E 's/^version = "([^"]+)".*/\1/')"
if [ -z "$OLD" ]; then
  echo "error: could not read current version from Cargo.toml" >&2
  exit 1
fi
if [ "$OLD" = "$NEW" ]; then
  echo "Nothing to do: already at $NEW."
  exit 0
fi
echo "Bumping $OLD -> $NEW"

# Escape dots for the regex (left-hand) side so they match literally.
OLD_RE="$(printf '%s' "$OLD" | sed 's/\./\\./g')"

# Portable in-place sed (works on both BSD and GNU sed).
sub() {
  local file="$1" expr="$2"
  [ -f "$file" ] || { echo "error: missing $file" >&2; exit 1; }
  sed -i.bak "$expr" "$file" && rm -f "$file.bak"
}

# --- per-manifest edits (anchored patterns avoid touching dependency versions) -
sub Cargo.toml                              "s/^version = \"$OLD_RE\"/version = \"$NEW\"/"
sub bindings/python/pyproject.toml          "s/^version = \"$OLD_RE\"/version = \"$NEW\"/"
sub bindings/rust/Cargo.toml                "s/^version = \"$OLD_RE\"/version = \"$NEW\"/"
sub bindings/java/pom.xml                   "s|<version>$OLD_RE</version>|<version>$NEW</version>|"
sub bindings/php/src/Installer.php          "s/VERSION = '$OLD_RE'/VERSION = '$NEW'/"
sub bindings/delphi/boss.json               "s/\"version\": \"$OLD_RE\"/\"version\": \"$NEW\"/"
sub bindings/node/package.json              "s/\"version\": \"$OLD_RE\"/\"version\": \"$NEW\"/"
sub bindings/csharp/RustPdf/RustPdf.csproj  "s|<Version>$OLD_RE</Version>|<Version>$NEW</Version>|"
sub bindings/ruby/rustpdf.gemspec           "s/spec\.version = \"$OLD_RE\"/spec.version = \"$NEW\"/"

# --- Node: propagate to the 5 platform packages + rebuild optionalDependencies -
if command -v node >/dev/null 2>&1; then
  node bindings/node/scripts/sync-versions.mjs >/dev/null
  echo "  node platform packages synced to $NEW"
else
  echo "  WARN: node not found — run 'node bindings/node/scripts/sync-versions.mjs' yourself" >&2
fi

# --- refresh Cargo.lock for the workspace crates ------------------------------
# cargo is often not on PATH in this repo; try common locations.
if ! command -v cargo >/dev/null 2>&1; then
  for d in "$HOME/.cargo/bin" \
           "$HOME/Library/Caches/puccinialin/cargo/bin" \
           "$HOME"/.rustup/toolchains/stable-*/bin; do
    [ -x "$d/cargo" ] && PATH="$d:$PATH" && break
  done
fi
if command -v cargo >/dev/null 2>&1; then
  cargo update -w >/dev/null 2>&1 && echo "  Cargo.lock updated"
else
  echo "  WARN: cargo not found — run 'cargo update -w' yourself to refresh Cargo.lock" >&2
fi

# --- verify: no stale OLD left, every manifest now on NEW ----------------------
echo "Verifying..."
stale="$(grep -rln "\"$OLD_RE\"\|>$OLD_RE<\|'$OLD_RE'\| = \"$OLD_RE\"" \
  --include='*.toml' --include='*.json' --include='*.gemspec' \
  --include='*.csproj' --include='*.xml' --include='*.php' . 2>/dev/null \
  | grep -vE 'target/|node_modules|Cargo.lock|package-lock|/site/|/docs/' || true)"
if [ -n "$stale" ]; then
  echo "error: these files still reference $OLD:" >&2
  echo "$stale" >&2
  exit 1
fi

echo "Done. All manifests are at $NEW."
echo
echo "Next steps (release — see docs/RELEASING.md):"
echo "  git commit -am \"chore: bump version to $NEW\" && git push origin HEAD:main"
echo "  then push each per-binding tag ONE AT A TIME: py-v$NEW node-v$NEW ruby-v$NEW ..."

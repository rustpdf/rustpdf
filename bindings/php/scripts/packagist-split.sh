#!/usr/bin/env bash
set -euo pipefail

# Packagist requires the composer.json to sit at the ROOT of the repository it
# indexes — Composer has no monorepo-subdirectory concept (unlike Go modules).
# This script splits bindings/php/ into a standalone history and pushes it to a
# dedicated "read-only mirror" repo, the way Symfony/Laravel split their
# components. Run it from the monorepo root.
#
#   SPLIT_REMOTE=git@github.com:rustpdf/rustpdf-php.git \
#   VERSION=0.1.0 \
#   bash bindings/php/scripts/packagist-split.sh
#
# Then submit that mirror's URL to packagist.org once. Re-run on each release.

PREFIX="bindings/php"
SPLIT_REMOTE="${SPLIT_REMOTE:?set SPLIT_REMOTE, e.g. git@github.com:rustpdf/rustpdf-php.git}"
VERSION="${VERSION:-}"          # optional; if set, also pushes tag v$VERSION
BRANCH="${BRANCH:-main}"        # branch to push on the mirror
SPLIT_BRANCH="_php_split_tmp"

root="$(git rev-parse --show-toplevel)"
cd "$root"

# composer.json version must equal Installer::VERSION (sanity check).
inst_ver="$(grep -oE "VERSION = '[0-9]+\.[0-9]+\.[0-9]+'" "$PREFIX/src/Installer.php" | grep -oE "[0-9]+\.[0-9]+\.[0-9]+")"
if [ -n "$VERSION" ] && [ "$VERSION" != "$inst_ver" ]; then
  echo "::error:: VERSION=$VERSION != Installer::VERSION=$inst_ver — bump them together" >&2
  exit 1
fi

echo "Splitting $PREFIX into $SPLIT_BRANCH ..."
git branch -D "$SPLIT_BRANCH" 2>/dev/null || true
git subtree split --prefix="$PREFIX" -b "$SPLIT_BRANCH"

echo "Pushing split history to $SPLIT_REMOTE ($BRANCH) ..."
git push --force "$SPLIT_REMOTE" "$SPLIT_BRANCH:$BRANCH"

if [ -n "$VERSION" ]; then
  echo "Tagging mirror v$VERSION ..."
  git push "$SPLIT_REMOTE" "$SPLIT_BRANCH:refs/tags/v$VERSION"
fi

git branch -D "$SPLIT_BRANCH"
echo "Done. Packagist will index $SPLIT_REMOTE; consumers run: composer require rust-pdf/rustpdf"

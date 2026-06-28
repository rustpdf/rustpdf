# Releasing the Delphi package

One command publishes a new Delphi release and updates the public download on
the site, with **no manual file edits**:

```sh
scripts/release-delphi.sh 0.3.0      # or no arg to re-release the current version
```

## What it does

1. **Bumps the version** in the workspace `Cargo.toml` (the single source of
   truth) and commits, if you passed a new `X.Y.Z`.
2. **Tags** `delphi-v<version>` and pushes the branch + tag.
3. The push triggers **`.github/workflows/release-delphi.yml`**, which builds the
   native cdylib on native runners — Windows x64+x86, macOS universal, Linux
   x64+arm64, each with the production license pubkey — assembles the archive
   with `bindings/delphi/scripts/package.sh`, and attaches
   `rustpdf-delphi-<version>.zip` + `.sha256` to a **GitHub Release**. The script
   waits for that workflow to finish.
4. **Deploys the site** via `site/scripts/deploy.sh`, whose first step
   (`sync-delphi-download.sh`) pulls the Release asset into
   `site/public/downloads/` (checksum-verified). The `docker build`/k3s rollout
   then serves it at `https://rustpdf.dev/downloads/`.
5. **Verifies** the public download (HTTP 200) and that the docs page advertises
   the new version.

## Why there's no HTML to edit

The version lives in exactly one place at runtime. `docs/delphi.html` carries a
`__DELPHI_VERSION__` placeholder; the Express server injects the current version
at startup (`src/server.js` → `renderDelphiPage`), derived from the zip actually
present in `public/downloads/` (falling back to `$DELPHI_VERSION`). The container
restarts on every deploy, so the page, the download link and the checksum link
always match the bytes that were baked in.

## Prerequisites

- `gh` authenticated (the repo is private, so the Release is authenticated
  storage — `gh` handles both the publish from CI and the pull at deploy time).
- Deploy access (SSH to the VPS) for step 4 — same as `site/scripts/deploy.sh`.
- Repo secret **`RUSTPDF_LICENSE_PUBKEY`** set to the production Ed25519 public
  key, so released libraries reject the dev token (shared with `release-python`).

## Options & manual fallback

```sh
scripts/release-delphi.sh 0.3.0 --skip-ci-wait   # don't wait on CI (Release already exists)
scripts/release-delphi.sh 0.3.0 --skip-deploy    # publish the Release only, no site deploy
scripts/release-delphi.sh 0.3.0 --force-tag      # recreate an existing tag

# Build the archive locally (host target only, or every installed Rust target):
make delphi-dist
make delphi-dist-publish        # also copies the zip into site/public/downloads/

# Refresh just the site download (no new release):
./site/scripts/sync-delphi-download.sh
./site/scripts/deploy.sh         # SKIP_DOWNLOADS=1 to skip the sync entirely
```

The very first deploy, before any `delphi-v*` Release exists, needs
`SKIP_DOWNLOADS=1 ./site/scripts/deploy.sh` (or run a release first).

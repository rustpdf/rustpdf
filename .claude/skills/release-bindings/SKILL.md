---
name: release-bindings
description: Bump every rust-pdf binding to a new version, cut and publish the per-binding release tags one at a time, wait for all release pipelines to finish, then deploy the site (site/scripts/deploy.sh) ensuring the Swift and Delphi downloads ship the new version. Use when the user asks to "release", "bump the bindings", "subir o bump das versões", "cut a new version", or "publish + deploy" the bindings.
---

# Release the rust-pdf bindings (bump → tag → wait → deploy)

This skill performs a full patch/minor release of all language bindings and
redeploys the public site. It is the codified version of `docs/RELEASING.md`
plus the operational gotchas that bite every time. Read `docs/RELEASING.md` for
the authoritative matrix; this skill is the runbook.

## Toolchain

`cargo` is not on PATH. Prepend it before any cargo command:

```sh
export RUSTUP_HOME="$HOME/.rustup"
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$HOME/Library/Caches/puccinialin/cargo/bin:$PATH"
```

## Inputs

- **version** (optional): the target `X.Y.Z`. If the user didn't give one,
  read the current `[workspace.package] version` from `Cargo.toml` and bump the
  patch (e.g. `0.4.3` → `0.4.4`). Confirm the chosen number in your first reply.

## The eight enabled bindings vs. the two disabled

Tag → workflow → registry. **Only cut tags for the ENABLED workflows.** Verify
each run before relying on it (a workflow whose `on: push tags:` is commented
out will *silently do nothing*):

```sh
for w in python node csharp go php ruby java swift rust delphi; do
  st=$(awk '/^on:/{o=1} o&&/tags:/{print (/#/?"DISABLED":"ENABLED"); exit}' ".github/workflows/release-$w.yml")
  echo "$w: ${st:-UNKNOWN}"
done
```

As of 0.4.4: **ENABLED** = python, node, csharp, go, php, ruby, swift, delphi.
**DISABLED by design** (no publishing infra yet) = **java, rust** — do NOT cut
their tags; they stay pinned at their last published tag (`java-v0.4.0`,
`rust-v0.4.0`). If the enabled set changed, follow the live check above, not this
list.

## Procedure

### 1. Bump

```sh
scripts/bump-version.sh <X.Y.Z>
```

This stamps all ten manifests + the Node platform packages + `optionalDependencies`
and refreshes `Cargo.lock`. It deliberately does **NOT** touch the site or the
Java README. Update those by hand (per `docs/RELEASING.md` §1) — they may be
pinned at an older number:

- `site/public/llms.txt` — the `Current stable version:` line
- `site/public/index.html`, `site/public/java.html`
- the `e.g. X.Y.Z` / install-snippet version lines in `site/public/docs/*.html`
  (swift, delphi, java)
- `bindings/java/README.md` install snippet

Find stragglers, then verify the cdylib reports the new version:

```sh
grep -rEn "<OLD_VERSION>" site/public bindings/java/README.md   # nothing important should remain
cargo build -p pdf-ffi --release
python3 -c "import ctypes,glob;f=ctypes.CDLL(glob.glob('target/release/libpdf_ffi.dylib')[0]);f.pdf_version.restype=ctypes.c_char_p;print(f.pdf_version().decode())"
```

### 2. PR + green CI + merge to main

Tags must be cut from `main`. Commit on a branch, open a PR, wait for **all**
checks, merge (squash). End commit messages with the Co-Authored-By trailer.

**Known CI trap — `cargo-deny` advisories.** `cargo update -w` (run by the bump
script) and the freshly-fetched RustSec DB can surface a *new* advisory
unrelated to the bump (e.g. an "unmaintained" notice). If `cargo-deny` fails on
an advisory we can't act on, add the `RUSTSEC-YYYY-NNNN` id to the `ignore` list
in `deny.toml` **with a justifying comment** (threat model / why no successor),
push, and let CI re-run. This unblocks every future bump too.

After merge, capture the squashed main SHA:

```sh
git fetch origin -q && SHA=$(git rev-parse origin/main)
git show "$SHA:Cargo.toml" | grep -m1 '^version = '   # confirm it's the new version
```

> Note: `main` is checked out in another git worktree, so `gh pr merge --delete-branch`
> may print a harmless "already used by worktree" error *after* the merge
> succeeds — verify with `gh pr view <n> --json state`.

### 3. Cut the tags ONE AT A TIME

> **The single most important rule.** GitHub creates **no** push event (so no
> workflow runs, silently) when **more than 3 tags** are pushed in one `git push`.
> Never `git push --tags`. One `git push origin <tag>` per tag, with a pause.

```sh
for t in py node csharp go php ruby swift delphi; do   # ENABLED set only
  git tag "${t}-v<X.Y.Z>" "$SHA"
  git push origin "${t}-v<X.Y.Z>"
  sleep 8
done
```

**Never push `bindings/go/v*` by hand** — the Go workflow creates that module
tag at a commit that stages the native libs.

Then confirm **every** tag actually produced a run (zero runs = the batch trap
bit you):

```sh
gh run list --event push --limit 30 --json workflowName,headBranch,status,conclusion \
  -q '.[] | select(.headBranch|test("v<X.Y.Z>$")) | "\(.headBranch)\t\(.status)\t\(.conclusion//"-")"'
```

### 4. PHP mirror split (Packagist code, not just binaries)

The `php-v*` workflow only uploads the **binaries** to the mirror release. The
**code** that Packagist indexes comes from a subtree split you run by hand, and
the mirror `v<version>` tag must exist before the workflow's publish job:

```sh
SPLIT_REMOTE=git@github.com:rustpdf/rustpdf-php.git VERSION=<X.Y.Z> \
  bash bindings/php/scripts/packagist-split.sh
```

(Needs SSH push access to the mirror. `git ls-remote --tags git@github.com:rustpdf/rustpdf-php.git`
to sanity-check access first. Restore your local HEAD afterward — the script
checks out a temp branch.)

### 5. Wait for ALL release pipelines, then verify each registry

Poll until all enabled runs reach `completed/success`. Swift and Delphi are the
ones the deploy depends on — they must publish a GitHub Release with the
`X.Y.Z` assets. Then:

```sh
curl -s https://pypi.org/pypi/rustpdf/json | jq -r .info.version                      # PyPI
curl -s https://registry.npmjs.org/rustpdf | jq -r '."dist-tags".latest'              # npm
curl -s https://rubygems.org/api/v1/gems/rustpdf.json | jq -r .version                # RubyGems
curl -s https://api.nuget.org/v3-flatcontainer/rustpdf/index.json | jq -r '.versions[-1]'  # NuGet (indexes with a few-min lag)
curl -s https://repo.packagist.org/p2/rust-pdf/rustpdf.json | jq -r '.packages["rust-pdf/rustpdf"][0].version'  # Packagist
git ls-remote --tags origin 'refs/tags/bindings/go/v<X.Y.Z>'                          # Go module tag (workflow-made)
gh release view swift-v<X.Y.Z>  --repo rustpdf/rustpdf --json assets -q '.assets[].name'
gh release view delphi-v<X.Y.Z> --repo rustpdf/rustpdf --json assets -q '.assets[].name'
```

NuGet may lag by minutes — re-check, don't panic. A transient `503 "no healthy
upstream"` from `gh release view` is also worth one retry.

### 6. Deploy the site

`site/scripts/deploy.sh` derives the version from the local `Cargo.toml`
(now the new version), pulls the **Swift + Delphi** downloads from their
`swift-v<X.Y.Z>` / `delphi-v<X.Y.Z>` GitHub Releases into
`site/public/downloads/`, bakes them into the image, and rolls out on the VPS.
So step 5 must finish (those releases must exist) **before** this:

```sh
./site/scripts/deploy.sh
```

It runs an SSH build on the VPS — can take several minutes; run it backgrounded
and tail the log, watching for `build falhou`/`ERRO:` as well as the happy path.
Success ends with `✅ deploy concluído` and a public `HTTP 200`.

### 7. Confirm Swift & Delphi shipped the new version (the user's explicit ask)

```sh
for f in rustpdf-delphi-<X.Y.Z>.zip rustpdf-swift-<X.Y.Z>.zip RustPdfFFI-<X.Y.Z>.xcframework.zip; do
  curl -s -o /dev/null -w "$f -> %{http_code}\n" -I "https://rustpdf.dev/downloads/$f"
done
curl -s -o /dev/null -w 'old delphi -> %{http_code}\n' -I "https://rustpdf.dev/downloads/rustpdf-delphi-<OLD>.zip"  # expect 404
```

All three new files `200`, the old one `404` (rsync `--delete` removed it) =
done.

## Done criteria

- All eight enabled registries/releases on the new version (NuGet lag tolerated).
- PHP mirror tagged + Packagist on the new version.
- `https://rustpdf.dev` returns 200 and serves the new Swift/Delphi downloads;
  the previous version's downloads 404.

# Releasing rust-pdf

How every language binding is published. One Rust core, ten thin bindings; each
ships on its **own git tag** that triggers its own GitHub Actions workflow.

> **The single most important rule: push release tags ONE AT A TIME.**
> GitHub does **not** create push events (and therefore does not start
> workflows) when **more than 3 tags** are pushed in one `git push`. Pushing all
> the tags in a batch is exactly why the 0.3.0 tags existed on the remote with
> **zero** workflow runs and nothing ever reached the registries. Push each tag
> with its own `git push origin <tag>`.

## 0. Prerequisites

- **GitHub Actions must be funded.** If org billing/Actions quota is exhausted,
  *every* job fails instantly with `startup_failure` (`steps=0`, no logs). Fix it
  at: org `rustpdf` → Settings → Billing → Actions. No code change helps.
- **`RUSTPDF_LICENSE_PUBKEY` repo secret** = the **production** Ed25519 public
  key. Every release workflow refuses to publish if it is unset, so the shipped
  `libpdf_ffi` is never built with the committed *dev* key (which would accept
  the public dev token — a licensing bypass). The published libs therefore
  reject the dev token; release smokes must test only the **free surface**.

## 1. Bump the version

Single source of truth is the workspace version; the crates inherit it
(`version.workspace = true`). Bump these together to `X.Y.Z`:

- `Cargo.toml` → `[workspace.package] version` (+ run `cargo update -w` for `Cargo.lock`)
- `bindings/python/pyproject.toml`, `bindings/rust/Cargo.toml`, `bindings/java/pom.xml`,
  `bindings/php/src/Installer.php` (`VERSION`), `bindings/delphi/boss.json`,
  `bindings/csharp/RustPdf/RustPdf.csproj`, `bindings/node/package.json`
  (+ the 4 `bindings/node/npm/*/package.json` and the `optionalDependencies`),
  `bindings/ruby/rustpdf.gemspec`
- Site install snippets: `site/public/index.html`, `site/public/java.html`,
  `site/public/swift.html`, and the `e.g. X.Y.Z` lines in `site/public/docs/*.html`
- `bindings/java/README.md` install snippet

Verify locally: build the cdylib and check `rustpdf.version() == "X.Y.Z"`.
Merge the bump to `main` (the tags are cut from `main`).

## 2. Cut the release — push tags one at a time

From `main` at the merged bump commit:

```sh
SHA=$(git rev-parse origin/main)
for t in py-v0.4.0 node-v0.4.0 csharp-v0.4.0 go-v0.4.0 php-v0.4.0 \
         ruby-v0.4.0 java-v0.4.0 swift-v0.4.0 rust-v0.4.0 delphi-v0.4.0; do
  git tag "$t" "$SHA"
  git push origin "$t"   # ONE push per tag — never `git push --tags`
  sleep 8
done
```

**Do NOT push `bindings/go/v*` yourself** — the Go workflow creates that tag (see
below). Pushing it by hand makes the workflow's `git push` fail ("already
exists") *and* points the module tag at a commit without the native libs.

## 3. Per-binding matrix

| Lang | Trigger tag | Workflow | Publishes to | Auth / secrets | Notes |
|------|-------------|----------|--------------|----------------|-------|
| Python | `py-v*` | `release-python.yml` | **PyPI** `rustpdf` | Trusted Publishing (OIDC, no token) + `RUSTPDF_LICENSE_PUBKEY` | manylinux + macOS + Windows wheels |
| Node | `node-v*` | `release-node.yml` | **npm** `rustpdf` + 4 platform pkgs (`@rustpdf/{darwin-arm64,linux-arm64-gnu,linux-x64-gnu,win32-x64-msvc}`) | `NPM_TOKEN` + pubkey | main pkg pulls a platform pkg via `optionalDependencies` |
| C# | `csharp-v*` | `release-csharp.yml` | **NuGet** `RustPdf` | Trusted Publishing (OIDC) + `NUGET_USER` repo var + `nuget` environment + pubkey | single RID-asset `.nupkg` |
| Ruby | `ruby-v*` | `release-ruby.yml` | **RubyGems** `rustpdf` | `RUBYGEMS_API_KEY` + pubkey | |
| Swift | `swift-v*` | `release-swift.yml` | **GitHub Release** (xcframework) | pubkey | `Package.swift` `.binaryTarget`; site deploy pulls the asset |
| Delphi | `delphi-v*` | `release-delphi.yml` | **GitHub Release** (zip + sha) | pubkey | no central registry; site offers the download |
| Go | `go-v*` | `release-go.yml` | **git tag** `bindings/go/v*` (workflow creates it, with native libs staged) | pubkey | `go get github.com/rustpdf/rustpdf/bindings/go/rustpdf@vX.Y.Z` |
| PHP | `php-v*` | `release-php.yml` | **Packagist** `rust-pdf/rustpdf` (mirror) + binaries on the mirror's GH Release | `MIRROR_RELEASE_TOKEN` (PAT, `contents:write` on the mirror) + pubkey | see PHP steps below |
| Java | `java-v*` | `release-java.yml` | **Maven Central** (Sonatype) | `MAVEN_CENTRAL_USERNAME` / `MAVEN_CENTRAL_PASSWORD` / `MAVEN_GPG_PRIVATE_KEY` / `MAVEN_GPG_PASSPHRASE` + pubkey | |
| Rust | `rust-v*` | `release-rust.yml` | **private cargo registry** | `CARGO_REGISTRY_INDEX` + `CARGO_REGISTRY_TOKEN` + pubkey | crate loads `libpdf_ffi` at runtime (no embedded binary) |

### Go specifics

Go has no registry — `go get` fetches the tree at a git tag. Because the module
lives in a subdirectory, its import path needs the tag `bindings/go/v<version>`
(not `go-v<version>`). The `go-v*` tag only **triggers** the workflow; the
workflow builds the native libs for every platform, makes a commit that stages
them under `bindings/go/.../resources/`, and pushes the `bindings/go/v*` tag at
*that* commit. Never create `bindings/go/v*` by hand.

### PHP specifics (two parts)

Packagist needs `composer.json` at the repo root, so the PHP package is served
from a **public mirror** `rustpdf/rustpdf-php`, not the monorepo.

1. **Source → Packagist:** run the subtree split (publishes the code + tag):
   ```sh
   SPLIT_REMOTE=git@github.com:rustpdf/rustpdf-php.git VERSION=0.4.0 \
     bash bindings/php/scripts/packagist-split.sh
   ```
   Then Packagist must index it: a GitHub→Packagist **webhook** on the mirror
   (URL includes your Packagist `apiToken`), or click **Update** on
   `packagist.org/packages/rust-pdf/rustpdf`. Without that it stays on the old
   version even though the tag exists.
2. **Binaries → mirror GH Release:** the `php-v*` workflow uploads the cdylibs to
   a Release on the mirror (the `Installer.php` lazy-downloads them). This needs
   the `MIRROR_RELEASE_TOKEN` secret (a PAT with `contents:write` on the mirror).

### Java specifics

`mvn exec:java` runs a smoke; the **release** smoke must be `ReleaseSmoke` (free
surface only — the prod-key lib rejects the dev token). The main class is the
`exec.mainClass` property (default `SmokeTest` for local dev); the workflow
overrides it with `-Dexec.mainClass=dev.rustpdf.ReleaseSmoke`. It must be a
property reference in the pom — a literal `<mainClass>` would win over `-D` and
wrongly run the full `SmokeTest`.

## 4. Verify each registry is on the new version

```sh
curl -s https://pypi.org/pypi/rustpdf/json | jq -r .info.version              # PyPI
curl -s https://registry.npmjs.org/rustpdf | jq -r '."dist-tags".latest'      # npm
curl -s https://rubygems.org/api/v1/gems/rustpdf.json | jq -r .version        # RubyGems
curl -s https://api.nuget.org/v3-flatcontainer/rustpdf/index.json | jq -r '.versions[-1]'  # NuGet
gh release view swift-v0.4.0 --repo rustpdf/rustpdf --json assets             # Swift
gh release view delphi-v0.4.0 --repo rustpdf/rustpdf --json assets            # Delphi
git ls-remote --tags https://github.com/rustpdf/rustpdf-php | grep v0.4.0     # PHP mirror
# Go: git ls-remote --tags origin | grep 'bindings/go/v0.4.0'
```

Confirm each `release-<lang>` run is `success` (`gh run list --event push`),
and that **each tag actually has a run** — a tag with zero runs means the
batch-push trap bit you again.

## 5. Troubleshooting

- **Tag pushed, no workflow run** → tags were pushed in a batch (>3). Delete and
  re-push them one at a time, or `gh workflow run release-<lang>.yml` per binding.
- **All jobs `startup_failure` (`steps=0`)** → Actions billing/quota. Fix billing.
- **`refusing to publish ... RUSTPDF_LICENSE_PUBKEY not set`** → set the prod
  pubkey secret.
- **Go `tag ... already exists`** → you pushed `bindings/go/v*` by hand; delete
  it (`git push origin :refs/tags/bindings/go/v0.4.0`) and rerun `release-go`.
- **A failed job after a fix** → `gh run rerun <run-id> --failed`.

# Proposal: ship a `universal-darwin` gem so `gem install rustpdf` works on macOS

## Problem (observed as an external integrator)

On the Ruby that ships **with macOS** (the system Ruby, e.g. 2.6.10 with
RubyGems 3.0.3.1), following the docs verbatim fails:

```
$ gem install rustpdf
ERROR:  Could not find a valid gem 'rustpdf' (>= 0) in any repository
ERROR:  Possible alternatives: rustpdf
```

The gem **is** published and the binary exists — `gem fetch rustpdf` downloads
`rustpdf-0.4.1-arm64-darwin` fine. Only `gem install` fails.

## Root cause (confirmed empirically, not assumed)

| Probe | Result |
|-------|--------|
| `Gem::Platform.new("arm64-darwin") === Gem::Platform.local` (local = `universal-darwin-25`) | **`true`** — the platform *does* match |
| `gem fetch rustpdf` (SpecFetcher path) | downloads `arm64-darwin` correctly |
| `gem install rustpdf` on RubyGems **3.0.3.1** | **fails** "Could not find a valid gem" |
| `gem install rustpdf --platform arm64-darwin` | also fails |
| `gem install ./rustpdf-0.4.1-arm64-darwin.gem` (local file) | **works** |
| Published platforms (RubyGems API) | `arm64-darwin`, `aarch64-linux`, `x86_64-linux`, `x64-mingw-ucrt` |

Two findings combine:

1. **RubyGems 3.0.x resolver limitation.** The system Ruby bundles a 2019-era
   RubyGems whose dependency resolver fails to install a *platform-only* gem on a
   `universal-darwin` host, even though `Gem::Platform` matching returns true and
   the simpler `gem fetch` path resolves it. Modern RubyGems (and the per-arch
   Rubies from rbenv/asdf/Homebrew, which report `arm64-darwin-23` etc.) install
   it fine — which is why the binding works in CI and for most developers.
2. **Coverage gap.** No `universal-darwin` gem and **no `x86_64-darwin` gem** are
   published at all, so Intel Macs are unserved too, and the system Ruby has no
   gem whose CPU string exactly equals its own (`universal`).

## Fix

Publish a **`universal-darwin`** gem carrying a **fat (arm64 + x86_64) dylib**
built with `lipo`. This:

- gives the old resolver an **exact-CPU match** (`universal === universal`) for
  the macOS system Ruby, the case that fails today;
- serves **Intel Macs** (`x86_64-darwin` host) from the same gem
  (`universal === x86_64` is true);
- needs **no loader change** — `native.rb` already globs `vendor/*/` and
  `dlopen` of a fat dylib auto-selects the running arch.

The per-arch `arm64-darwin` gem is **kept** (most-specific match for Apple
Silicon rbenv/Homebrew Rubies); an `x86_64-darwin` per-arch gem is **added** for
symmetry. Purely additive — no existing gem changes.

### Changes in this proposal

- `.github/workflows/release-ruby.yml`
  - Phase 1 `build`: add the `x86_64-darwin` / `x86_64-apple-darwin` slice.
  - New Phase 1b `universal` job (macOS runner): `lipo -create` the two darwin
    dylibs into one fat `libpdf_ffi.dylib`, uploaded as `cdylib-universal-darwin`.
  - Phase 2 `gem` matrix: add `x86_64-darwin` and `universal-darwin`; depend on
    both `build` and `universal`.
- No `rustpdf.gemspec` change (platform comes from `RUSTPDF_GEM_PLATFORM`).
- No `lib/rustpdf/native.rb` change (vendor glob + fat dylib already handle it).

### Verified locally
- `lipo` is available on the `macos-14` runner image.
- Edited workflow passes a YAML load; job graph `build → universal → gem` is sound.

### Doc note (already applied to `site/public/docs/ruby.html`)
The Installation section now warns that macOS system Ruby (`universal-darwin`)
may not match the per-arch gem and recommends a per-arch Ruby, `gem update
--system`, or `RUSTPDF_LIB`. The `universal-darwin` gem makes the default path
work without that workaround.

## Risk
Low / additive. Worst case the lipo step fails and only the new macOS gems are
absent; the existing per-platform gems publish unchanged (matrix `fail-fast:
false`). A follow-up smoke that actually `gem install`s the `universal-darwin`
gem under the macOS system Ruby would close the loop in CI.

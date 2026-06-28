# Dependency licenses

This is a **paid product**, so every dependency must carry a permissive license
(`project.md` §1.3). The allow-list is enforced in CI by `cargo deny`
(see `deny.toml`). Regenerate the table below with `cargo metadata`.

## Runtime core dependencies

The base layers (`cos`, `writer`, `graphics`) depend on **nothing** outside the
standard library. Text/font support (`fonts`, Fase 3) pulls in a small set of
well-known, permissively-licensed font crates:

| Crate      | Runtime deps |
|------------|--------------|
| `cos`      | none |
| `writer`   | `cos` |
| `graphics` | `cos` |
| `fonts`    | `ttf-parser`, `rustybuzz`, `subsetter`, `unicode-bidi` (+ their transitive deps) |
| `images`   | `png`, `flate2` (Fase 4) |
| `parser`   | `flate2`, `aes`, `cbc`, `md-5` (Fase 5) |
| `pdf`      | core crates + `aes`/`cbc`/`md-5` (enc 7.3) + `rsa`/`cms`/`x509-cert`/`der`/`const-oid`/`sha2`/`signature` (signatures 7.1) |
| `ffi`      | `pdf` |

### Signature crates (Fase 7.1) — all permissive (RustCrypto)

| Crate | License | Role |
|-------|---------|------|
| `rsa` | MIT OR Apache-2.0 | RSA signing |
| `cms` | Apache-2.0 OR MIT | PKCS#7/CMS SignedData |
| `x509-cert` | Apache-2.0 OR MIT | X.509 certificate parsing |
| `der`, `const-oid`, `spki`, `pkcs1`, `pkcs8`, `signature`, `sha2` | MIT OR Apache-2.0 | ASN.1/OIDs/traits/digest |

The layout engine (7.6) is pure `pdf`; encryption (7.3) reuses the vetted
`aes`/`cbc`/`md-5`. RC4 is hand-written. All confirmed permissive on 2026-06-25.

### Licensing crate (`license`) — all permissive

| Crate | License | Role |
|-------|---------|------|
| `ed25519-dalek` | BSD-3-Clause | sign/verify license tokens |
| transitive: `curve25519-dalek`, `ed25519`, `signature` | BSD-3-Clause / MIT OR Apache-2.0 | EdDSA support |
| `zeroize`, `subtle`, `sha2`, `getrandom` | MIT OR Apache-2.0 | key zeroing / constant-time / hashing / CSPRNG |

BSD-3-Clause is permitted by `deny.toml`. The Ed25519 **private** key never ships
in the library; only the **public** key is embedded (overridable at build time
via `RUSTPDF_LICENSE_PUBKEY`). See [`docs/LICENSING.md`](docs/LICENSING.md).

### Parser crates (Fase 5) — all permissive

| Crate    | License           | Role |
|----------|-------------------|------|
| `flate2` | MIT OR Apache-2.0 | `FlateDecode` |
| `aes`    | MIT OR Apache-2.0 | AESv2 decryption (RustCrypto) |
| `cbc`    | MIT OR Apache-2.0 | CBC mode (RustCrypto) |
| `md-5`   | MIT OR Apache-2.0 | encryption key derivation (RustCrypto) |
| `sha2`   | MIT OR Apache-2.0 | AES-256/R6 key derivation (Algorithm 2.B) |
| `getrandom` | MIT OR Apache-2.0 | CSPRNG for encryption IVs/salts/keys (7.3 hardening) |
| transitive: `cipher`, `crypto-common`, `generic-array`, `typenum`, `block-buffer`, `digest`, `inout`, `cpufeatures`, `cfg-if`, `libc` | MIT / MIT OR Apache-2.0 | RustCrypto + getrandom support |

RC4 is implemented by hand (no dependency).

### Image crates (Fase 4) — all permissive

| Crate    | License           | Role |
|----------|-------------------|------|
| `png`    | MIT OR Apache-2.0 | PNG decode (re-encoded to `FlateDecode`) |
| `flate2` | MIT OR Apache-2.0 | zlib/deflate encoding (`FlateDecode`) |
| `miniz_oxide` | MIT OR Zlib OR Apache-2.0 | transitive (flate2/png backend) |

JPEG needs no decoder — bytes are embedded verbatim via `DCTDecode`.

### Font crates (Fase 3) — all permissive

| Crate           | License            | Role |
|-----------------|--------------------|------|
| `ttf-parser`    | MIT OR Apache-2.0  | TrueType/OpenType parsing & metrics (3A.1) |
| `rustybuzz`     | MIT                | HarfBuzz-port shaping: kerning, ligatures, complex scripts (3D, 3E) |
| `subsetter`     | MIT OR Apache-2.0  | glyph subsetting (3C) |
| `unicode-bidi`  | MIT OR Apache-2.0  | bidirectional reordering (3E.3) |
| `skrifa`, `read-fonts` | MIT OR Apache-2.0 | transitive (subsetter) |
| `core_maths`, `unicode-ccc`, `unicode-properties`, `unicode-script`, `unicode-bidi-mirroring` | MIT / MIT OR Apache-2.0 | transitive (rustybuzz/bidi) |

All confirmed permissive on 2026-06-25 via `cargo metadata`. The `project.md`
§1.3 candidates `ttf-parser`/`rustybuzz` are now adopted; `allsorts`/`klippa`
were not needed (subsetting is covered by `subsetter`).

### Bundled ICC profile (PDF/A)

`assets/icc/sRGB.icc` is `sRGB-v2-micro.icc` from the **Compact ICC Profiles**
project (https://github.com/saucecontrol/Compact-ICC-Profiles), released into the
**public domain under CC0 1.0**. It is `include_bytes!`-embedded into the `pdf`
crate as the PDF/A `OutputIntent` destination profile. See `assets/icc/LICENSE.txt`.

### Bundled fonts (test/example assets)

`assets/fonts/Roboto-Regular.ttf` and `Roboto-Bold.ttf` are **Roboto**, ©
Google, licensed **Apache-2.0** (redistributable). See
`assets/fonts/LICENSE.txt`. Used only by tests/examples; not part of the
library. System fonts (e.g. Hiragino for the CJK test) are referenced in place,
never bundled.

`site/public/fonts/schibsted-grotesk-latin.woff2` is **Schibsted Grotesk**, ©
Schibsted, licensed **SIL Open Font License 1.1** (redistributable, self-hosted
for the marketing site headings — no runtime external font request). Site asset
only; not part of the library or any binding.

## Tooling / test-only dependencies

These are pulled in by **build scripts** (`cbindgen`) and **test/dev tooling**
(`image`/`png` for visual regression in `testkit`). They are not part of the
distributed runtime, but are tracked anyway.

All resolve to permissive licenses (MIT / Apache-2.0 / BSD / Zlib / Unicode-3.0
/ MPL-2.0). Key notes:

* `cbindgen` — **MPL-2.0**, used only as a `build-dependency` to generate the C
  header. MPL-2.0 is file-level copyleft and does not affect our source; it is
  not linked into the shipped library.
* `image`, `png`, `flate2`, `miniz_oxide` — MIT/Apache/Zlib, **dev/test only**
  (visual-regression harness).
* `r-efi` offers `LGPL-2.1-or-later` as one option but is dual-licensed
  MIT/Apache; we take the permissive option. It is a transitive,
  Windows/UEFI-only build dependency.

Full resolved set (transitive, all profiles), verified permissive on
2026-06-25:

```
adler2            0BSD/MIT/Apache-2.0      cbindgen      MPL-2.0 (build only)
bitflags          MIT/Apache-2.0          clap          MIT/Apache-2.0
bytemuck          Zlib/Apache-2.0/MIT     flate2        MIT/Apache-2.0
byteorder-lite    Unlicense/MIT           image         MIT/Apache-2.0
crc32fast         MIT/Apache-2.0          miniz_oxide   MIT/Zlib/Apache-2.0
fdeflate          MIT/Apache-2.0          moxcms        BSD-3-Clause/Apache-2.0
itoa              MIT/Apache-2.0          png           MIT/Apache-2.0
libc              MIT/Apache-2.0          pxfm          BSD-3-Clause/Apache-2.0
log               MIT/Apache-2.0          serde         MIT/Apache-2.0
memchr            Unlicense/MIT           syn           MIT/Apache-2.0
num-traits        MIT/Apache-2.0          toml          MIT/Apache-2.0
once_cell         MIT/Apache-2.0          tempfile      MIT/Apache-2.0
simd-adler32      MIT                     unicode-ident MIT/Apache-2.0 AND Unicode-3.0
```

## Future font/crypto dependencies (Fase 3 / Fase 7)

To confirm permissive before adoption (`project.md` §1.3):
`ttf-parser`, `rustybuzz`, `allsorts` (Apache-2.0), `fontations`, `klippa`,
`unicode-bidi`, RustCrypto crates. Add each to `deny.toml`'s allow-list review
and this file when introduced.

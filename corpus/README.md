# Golden corpus (Fase 0.3)

A catalogued set of PDFs used by the test suite and (from Fase 5) round-trip
regression. The catalog is `catalog.json`; regenerate everything with:

```sh
make corpus           # cargo run -p pdf --example gen_corpus
```

## Layout

```
corpus/
  catalog.json        # machine-readable index with per-file metadata
  generated/          # valid PDFs the writer produces today
  corrupted/          # deliberately broken files (recovery targets, Fase 5.8)
```

## Catalog schema

Each entry in `catalog.json` has:

| field          | meaning |
|----------------|---------|
| `id`           | stable identifier |
| `category`     | `simple` / `graphics` / `fonts` / `images` / `corrupted` / `cjk` / `forms` / `encrypted` / `conformance` / `signature` |
| `description`  | human summary |
| `file`         | path relative to `corpus/`, or `null` if not yet generated |
| `features`     | feature tags the file exercises |
| `expect_valid` | whether external validators should accept it |
| `status`       | `generated` (file present) or `pending` (needs a later phase) |

## Coverage status

* **Generated now** (Fase 0–3): blank pages across page sizes, multi-page
  documents, every vector-graphics feature (RGB/Gray/CMYK fill & stroke, paths,
  curves, even-odd, clipping, nested CTM, fill-and-stroke), three corrupted
  variants (truncated, bad `startxref`, missing `%%EOF`), and **embedded
  subsetted text**: Latin, accented Unicode, and a justified inline-styled
  paragraph (using the bundled Apache-2.0 Roboto fonts); and **images**: a
  verbatim JPEG (`DCTDecode`) and a transparent PNG (`FlateDecode` + `SMask`).
* **Pending** (need their phase): CFF/OTF fonts, CJK (system-font dependent, not
  bundled), AcroForms (Fase 6.7), encryption (Fase 7.3), PDF/A & Tagged PDF
  (Fase 7.4/7.5), signatures (Fase 7.1). Catalogued with `file: null` so the
  index is complete and the gaps are explicit.

All `generated` files with `expect_valid: true` pass `qpdf --check` and
`mutool clean`.

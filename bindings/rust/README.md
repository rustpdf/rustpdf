# RustPdf for Rust

Generate, edit, sign and process PDFs from Rust: vector graphics, embedded fonts and Unicode text, wrapping paragraphs, images, **PDF/A** (1b-4f), **tagged/accessible** output, attachments, **AcroForm** fields, page manipulation (merge/split/stamp), watermarks, true **redaction**, **AES-256** encryption, **digital signatures (PAdES)** with HSM/deferred signing, timestamps/LTV, text extraction and search, and page **rendering to PNG**. Loads the precompiled engine at runtime.

## Documentation

- **Full API reference:** the crate rustdoc (`cargo doc --open`) and https://rustpdf.dev/docs/
- **Interactive positioning guide** (coordinates, anchors, rotation): https://rustpdf.dev/positioning
- All product guides (PDF/A, signatures, encryption, redaction, rendering): https://rustpdf.dev/docs/

The PDF engine ships as a precompiled shared library (`libpdf_ffi`); this
crate is a safe, idiomatic wrapper around it. The library is located and
loaded **at run time** via
[`libloading`], so:

- `cargo build` never needs the engine present (no build script, no link step);
- you ship the cdylib next to your binary, or point `RUSTPDF_LIB` at it;
- the engine's proprietary source is never distributed.

## Distribution: private cargo registry

This crate is published to a **private cargo registry**, not crates.io.

Consumers configure the registry once in `~/.cargo/config.toml` (or
`$CARGO_HOME/config.toml`):

```toml
[registries]
rustpdf = { index = "sparse+https://cargo.example.com/index/" }
```

and provide a token (so `cargo` can authenticate):

```sh
cargo login --registry rustpdf <TOKEN>
```

Then depend on it, pinning the registry:

```toml
# Cargo.toml
[dependencies]
rustpdf = { version = "0.1", registry = "rustpdf" }
```

Publishing (from this directory, by the vendor):

```sh
# set `publish = ["rustpdf"]` in Cargo.toml first, then:
cargo publish --registry rustpdf
```

> The committed `Cargo.toml` ships with `publish = false` as a safety net
> against an accidental crates.io push. Set `publish = ["rustpdf"]` (matching
> the registry name above) in your release pipeline.

## Shipping the engine library

The binding needs `libpdf_ffi` at run time. Resolution order (first hit wins):

1. `$RUSTPDF_LIB` — an explicit path to the library file;
2. the library next to the running executable, or in the current directory
   (the normal deployment layout — **drop the cdylib beside your app**);
3. `target/debug/` or `target/release/`, walking up from the exe and the CWD
   (the dev tree);
4. the bare platform name (`libpdf_ffi.dylib` / `.so` / `pdf_ffi.dll`), letting
   the OS loader resolve it via install-name / `PATH` / `LD_LIBRARY_PATH`.

Build the cdylib for each target you ship and distribute it alongside your
application (or via your installer). It is built by `cargo build -p pdf-ffi`
in the engine repo.

## Usage

```rust
use rustpdf::{Document, Align};

fn main() -> rustpdf::Result<()> {
    println!("engine: {}", rustpdf::version());

    let mut doc = Document::new()?;
    doc.add_page()?;
    doc.set_fill_rgb(0.1, 0.2, 0.8)?
       .rect(72.0, 700.0, 200.0, 60.0)?
       .fill()?;

    let font = doc.add_font_file("Roboto-Regular.ttf")?;
    doc.show_text(font, 24.0, 72.0, 650.0, "Hello", 1)?;

    doc.save("out.pdf")?;
    Ok(())
}
```

Every feature is free — PDF/A, tagged/accessible output, encryption, digital
signatures/PAdES, redaction and page rendering are all included.

## API surface

- [`Document`] — authoring: pages, vector graphics, fonts/text, paragraphs,
  images, PDF/A levels, tagging, attachments, AcroForm fields, `save`/`write`.
- [`EditableDoc`] — manipulation: load/merge/split/reorder/rotate/delete,
  `/Info` + XMP, overlay, fill fields, optimize/compact, encrypt, incremental
  save, `to_bytes`. **Positioned stamping** on existing pages: `fill_rect`,
  `place_text` / `place_text_aligned` / `place_text_anchored`
  (`VerticalAnchor`), `masked_text` / `masked_text_padded` (`VerticalAlign` +
  edge pad), `place_paragraph` (word wrap, measured `(lines, height, found)`),
  `draw_image` / `draw_image_anchored` (`ImageAnchor`), custom embedded fonts
  via `add_font` / `add_font_file`, and `set_stamp_space` (`StampSpace`:
  visible vs raw media coordinates).
- Module functions: `version`, `ensure_loaded`,
  `extract_text`, `sign`, `timestamp`, `add_dss`. **Deferred / HSM signing**
  (the private key never reaches the library): `sign_with` (a signer callback)
  and `begin_signing` + `SigningSession::complete` (two-phase) for cloud KMS,
  HSMs, smartcards and PKI tokens, plus `list_signatures` and the
  `SigningOptions` / `Certify` / `SignaturePolicy` types.

## Testing

```sh
make rust-test    # from the repo root (builds the cdylib, runs the smoke test)
```

[`libloading`]: https://crates.io/crates/libloading

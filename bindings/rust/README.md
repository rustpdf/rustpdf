# rustpdf — Rust binding

Safe, idiomatic Rust bindings for the rust-pdf engine.

Unlike the in-tree `pdf` crate, **this crate does not contain the engine
source**. It is a thin wrapper over the precompiled `libpdf_ffi` cdylib (the
same C ABI used by the Python, C#, Go, PHP, Ruby, Node, Java, Delphi and Swift
bindings). The shared library is located and loaded **at run time** via
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

Corporate features (PDF/A, tagged/accessible output, encryption, digital
signatures) are gated behind a license — activate it once per process:

```rust
rustpdf::activate_license(&std::fs::read_to_string("license.txt")?)?;
```

or set the `RUSTPDF_LICENSE` / `RUSTPDF_LICENSE_FILE` environment variable (the
engine auto-activates from there).

## API surface

- [`Document`] — authoring: pages, vector graphics, fonts/text, paragraphs,
  images, PDF/A levels, tagging, attachments, AcroForm fields, `save`/`write`.
- [`EditableDoc`] — manipulation: load/merge/split/reorder/rotate/delete,
  `/Info` + XMP, overlay, fill fields, optimize/compact, encrypt, incremental
  save, `to_bytes`.
- Module functions: `version`, `ensure_loaded`, `activate_license`,
  `extract_text`, `sign`, `timestamp`, `add_dss`.

## Testing

```sh
make rust-test    # from the repo root (builds the cdylib, runs the smoke test)
```

[`libloading`]: https://crates.io/crates/libloading

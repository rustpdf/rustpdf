//! Build script: generate the C header (`pdf.h`) from the FFI surface using
//! cbindgen (`project.md` 0.7). Best-effort — a header-generation failure
//! emits a warning but does not fail the build, so plain `cargo build` always
//! works even if cbindgen cannot parse a transient state.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=cbindgen.toml");

    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let config = cbindgen::Config::from_file(crate_dir.join("cbindgen.toml")).unwrap_or_default();

    let builder = cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config);

    match builder.generate() {
        Ok(bindings) => {
            // Write into the workspace-level include/ dir and OUT_DIR.
            let workspace_include = crate_dir
                .parent()
                .and_then(|p| p.parent())
                .map(|root| root.join("include"));
            if let Some(dir) = workspace_include {
                let _ = std::fs::create_dir_all(&dir);
                bindings.write_to_file(dir.join("pdf.h"));
            }
            if let Ok(out_dir) = env::var("OUT_DIR") {
                bindings.write_to_file(PathBuf::from(out_dir).join("pdf.h"));
            }
        }
        Err(e) => {
            println!("cargo:warning=cbindgen header generation skipped: {e}");
        }
    }
}

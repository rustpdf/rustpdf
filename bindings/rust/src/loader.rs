//! Locating the precompiled `libpdf_ffi` cdylib at run time.
//!
//! The binding never links the engine at build time — the engine source is not
//! shipped. Instead the cdylib is resolved at run time, mirroring the resolver
//! used by the C#, Delphi, Java and Ruby bindings:
//!
//! 1. `$RUSTPDF_LIB` — an explicit path to the library file;
//! 2. the library sitting next to the running executable, or in the current
//!    directory (the normal deployment layout — ship the lib beside your app);
//! 3. `target/debug/<lib>` or `target/release/<lib>`, walking up from the exe
//!    and the current directory (the dev tree);
//! 4. the bare platform name, letting the OS loader resolve it via the
//!    install-name / `PATH` / `LD_LIBRARY_PATH` / `DYLD_LIBRARY_PATH`.

use std::env;
use std::path::{Path, PathBuf};

/// Platform file name of the cdylib.
pub(crate) fn lib_file_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "pdf_ffi.dll"
    }
    #[cfg(target_vendor = "apple")]
    {
        "libpdf_ffi.dylib"
    }
    #[cfg(all(unix, not(target_vendor = "apple")))]
    {
        "libpdf_ffi.so"
    }
}

fn exists(p: &Path) -> bool {
    p.is_file()
}

/// Build the ordered list of candidate paths to try.
fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let file = lib_file_name();

    // 1. Explicit override.
    if let Ok(explicit) = env::var("RUSTPDF_LIB") {
        if !explicit.is_empty() {
            out.push(PathBuf::from(explicit));
        }
    }

    // Collect the directories we search for plain + target/{debug,release}.
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            roots.push(dir.to_path_buf());
        }
    }
    if let Ok(cwd) = env::current_dir() {
        roots.push(cwd);
    }

    // 2. Beside the executable / in the current dir.
    for root in &roots {
        out.push(root.join(file));
    }

    // 3. target/debug and target/release, walking up from each root.
    for root in &roots {
        let mut dir: Option<&Path> = Some(root.as_path());
        while let Some(d) = dir {
            for profile in ["debug", "release"] {
                out.push(d.join("target").join(profile).join(file));
            }
            dir = d.parent();
        }
    }

    out
}

/// Resolve the library path, returning the first existing candidate, or the
/// bare platform name as a last resort (so the OS loader can still find it).
pub(crate) fn resolve() -> PathBuf {
    for cand in candidates() {
        if exists(&cand) {
            return cand;
        }
    }
    PathBuf::from(lib_file_name())
}

# Changelog

All notable changes to this project are documented here. Each binding ships
under its own tag (`py-v*`, `node-v*`, …) but all share the workspace version.

## 0.5.0

### Breaking

- **rust-pdf is now free and open source (MIT), with every feature available.**
  The license gate is gone:
  - removed `pdf::activate_license`, `pdf::active_license` and the
    `License`/`Feature`/`LicenseError` types;
  - removed the FFI export `pdf_activate_license` and the
    `PdfStatus::License` variant — **`PdfStatus::Unsupported` is now `12`**
    (was `13`); code that compares raw status integers must be updated;
  - removed `activate_license` / `ActivateLicense` from all ten bindings.

### Fixed

- Dependency advisories: `crossbeam-epoch` 0.9.21 and `spin` 0.9.9.

## Earlier versions

See the git tags for 0.1.0 – 0.4.8.

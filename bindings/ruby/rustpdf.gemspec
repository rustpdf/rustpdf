# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = "rustpdf"
  spec.version = "0.4.7"
  spec.summary = "Generate, edit, sign and process PDFs in Ruby. Docs: https://rustpdf.dev/docs/ruby"
  spec.description = "Generate, edit, sign and process PDFs in Ruby: PDF/A, AES-256 encryption, digital signatures (PAdES), forms, stamping, redaction, text extraction and page rendering. Docs: https://rustpdf.dev/docs/ruby"
  spec.authors = ["rust-pdf"]
  spec.license = "Nonstandard"
  spec.required_ruby_version = ">= 2.6"

  # Platform-specific gem: CI sets RUSTPDF_GEM_PLATFORM (e.g. "arm64-darwin",
  # "x86_64-linux", "aarch64-linux", "x64-mingw-ucrt") and stages the matching
  # prebuilt cdylib under vendor/<platform>/ before `gem build`, so each
  # published gem carries exactly one native lib — the RubyGems analog of the
  # per-platform Python wheels / npm packages. A plain `gem build` (no env)
  # produces a generic "ruby"-platform gem with no binary; that one only works
  # when RUSTPDF_LIB points at a locally built libpdf_ffi.
  spec.platform = ENV["RUSTPDF_GEM_PLATFORM"] if ENV["RUSTPDF_GEM_PLATFORM"]

  spec.files = Dir["lib/**/*.rb", "README.md"] + Dir["vendor/**/*"].select { |f| File.file?(f) }
  spec.require_paths = ["lib"]
  # Uses the `fiddle` standard library; the native libpdf_ffi is loaded at
  # runtime from RUSTPDF_LIB, then the vendored vendor/<platform>/ cdylib, then
  # the build tree (target/{debug,release}) for monorepo dev.
end

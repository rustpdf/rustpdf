# frozen_string_literal: true

Gem::Specification.new do |spec|
  spec.name = "rustpdf"
  spec.version = "0.1.0"
  spec.summary = "Ruby binding for the rust-pdf core (generate, manipulate, sign and validate PDFs)."
  spec.description = "Idiomatic Ruby binding over the rust-pdf C ABI (libpdf_ffi) using the built-in Fiddle stdlib."
  spec.authors = ["rust-pdf"]
  spec.license = "Nonstandard"
  spec.required_ruby_version = ">= 2.6"
  spec.files = Dir["lib/**/*.rb", "README.md"]
  spec.require_paths = ["lib"]
  # Uses the `fiddle` standard library; the native libpdf_ffi is loaded at
  # runtime from RUSTPDF_LIB or the build tree (target/{debug,release}).
end

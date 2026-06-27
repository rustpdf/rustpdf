# rust-pdf — common developer tasks.
#
# The Rust toolchain may live under rustup's toolchain dir without cargo on
# PATH; allow overriding CARGO. Defaults to plain `cargo`.
CARGO ?= cargo

.PHONY: all build test clippy fmt fmt-check deny header ffi examples python-test csharp-test go-test php-test ruby-test node-test java-test delphi-test swift-test delphi-dist delphi-dist-publish clean ci

all: build

build:
	$(CARGO) build --workspace

test:
	$(CARGO) test --workspace

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all --check

deny:
	$(CARGO) deny check

# Build the cdylib/staticlib and (re)generate the C header at include/pdf.h.
header ffi:
	$(CARGO) build -p pdf-ffi
	@echo "header at include/pdf.h"

examples:
	$(CARGO) build -p pdf --examples

# End-to-end FFI dogfood: Rust reference vs Python binding (must be identical).
python-test: ffi
	$(CARGO) run -q -p pdf --example ffi_reference -- /tmp/rust_reference.pdf
	python3 bindings/python/test_binding.py /tmp/rust_reference.pdf /tmp/python_out.pdf

# C#/.NET binding smoke test (exercises the whole surface over the C ABI).
csharp-test: ffi
	dotnet run --project bindings/csharp/Sample -c Release

# Go binding test (cgo; exercises the whole surface over the C ABI).
go-test: ffi
	cd bindings/go && CGO_ENABLED=1 go test ./...

# PHP binding smoke test (ext-ffi; exercises the whole surface over the C ABI).
php-test: ffi
	php bindings/php/test/run.php

# Ruby binding smoke test (Fiddle; exercises the whole surface over the C ABI).
ruby-test: ffi
	ruby bindings/ruby/test/run.rb

# Node.js binding smoke test (Koffi FFI; exercises the whole surface).
node-test: ffi
	cd bindings/node && npm install --silent && node test/run.js

# Java binding smoke test (JNA FFI; exercises the whole surface over the C ABI).
java-test: ffi
	cd bindings/java && mvn -q -e test-compile exec:java

# Delphi / Free Pascal binding smoke test (pure FFI; whole surface over the C
# ABI). Compiles with fpc or Delphi's dcc64; skips cleanly when neither exists.
delphi-test: ffi
	@if command -v fpc >/dev/null 2>&1; then \
		fpc -Mdelphi -O2 -Fubindings/delphi -FEbindings/delphi/test bindings/delphi/test/run.dpr && \
		bindings/delphi/test/run ; \
	elif command -v dcc64 >/dev/null 2>&1; then \
		dcc64 -B -Ubindings/delphi -NUbindings/delphi/test -Ebindings/delphi/test bindings/delphi/test/run.dpr && \
		bindings/delphi/test/run ; \
	else \
		echo "skip delphi-test: no Free Pascal (fpc) or Delphi (dcc64) compiler on PATH" ; \
	fi

# Swift binding smoke test (pure FFI via dlopen; whole surface over the C ABI).
# Skips cleanly when the Swift toolchain isn't on PATH.
swift-test: ffi
	@if command -v swift >/dev/null 2>&1; then \
		cd bindings/swift && swift test ; \
	else \
		echo "skip swift-test: no Swift toolchain on PATH" ; \
	fi

# Assemble a distributable Delphi/FPC archive (RustPdf.pas + native libs + sample)
# under bindings/delphi/dist/. Builds the cdylib for every installed Rust target.
delphi-dist:
	CARGO="$(CARGO)" bash bindings/delphi/scripts/package.sh

# Same, but also publish the zip + checksum to site/public/downloads/ (served as
# the public trial download at /downloads/). Run on the multi-platform build box.
delphi-dist-publish:
	CARGO="$(CARGO)" PUBLISH=1 bash bindings/delphi/scripts/package.sh

# Regenerate the synthetic golden corpus.
corpus:
	$(CARGO) run -p pdf --example gen_corpus

clean:
	$(CARGO) clean

ci: fmt-check clippy build test

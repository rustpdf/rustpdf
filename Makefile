# rust-pdf — common developer tasks.
#
# The Rust toolchain may live under rustup's toolchain dir without cargo on
# PATH; allow overriding CARGO. Defaults to plain `cargo`.
CARGO ?= cargo

.PHONY: all build test clippy fmt fmt-check deny header ffi examples python-test csharp-test go-test php-test ruby-test node-test java-test clean ci

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

# Regenerate the synthetic golden corpus.
corpus:
	$(CARGO) run -p pdf --example gen_corpus

clean:
	$(CARGO) clean

ci: fmt-check clippy build test

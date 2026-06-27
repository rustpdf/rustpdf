# ADR 0001 — Porting strategy: Rust core + hand-written C ABI + thin bindings

* Status: Accepted
* Date: 2026-06-25
* Context: `project.md` §1.1, Fase 0.6

## Decision

A **two-layer** architecture:

1. **Idiomatic Rust core** (`cos`, `parser`, `writer`, `graphics`, `fonts`,
   `images`, `layout`, `pdf`) — rich and unrestricted: `Result`, builders,
   generics, enums-with-data, lifetimes. This is what Rust users consume. It is
   **not** constrained by FFI concerns.

2. **`ffi` crate** — a thin, handle-based `extern "C"` boundary. It is the
   *only* layer that crosses the language frontier and the *only* layer bound by
   the rules in [`docs/FFI_RULES.md`](../FFI_RULES.md).

## Distribution substrate

* **Hand-written C ABI + `cbindgen`** (header generation) is the universal
  spine: every language does FFI in C (Go, PHP, Ruby, Java, C#, Elixir, …).
* **`wasm-bindgen`** is a separate track for JS/web (not yet implemented).
* **UniFFI is explicitly not** the primary frontier: it couples to its own
  model and does not cover the C/Go case. It is mutually exclusive with the C
  ABI. Re-evaluate only if mobile becomes a priority.

## Consequences

* The fluent/ergonomic API does **not** cross the C ABI — it is reconstructed in
  each binding's hand-written idiomatic wrapper (DX layer L2).
* The C ABI must not be "chatty": operations (a paragraph, a config struct, an
  array) cross in one call, never glyph-by-glyph.
* Concurrency: the core is `Send` but not `Sync` (§1.2.2). The object graph uses
  an arena (`Vec` + indices), never `Rc`/`RefCell`. See
  [ADR 0002](0002-concurrency-model.md).

## Validation

This decision is dogfooded from Fase 1.7 onward: the reference Python binding
(`bindings/python`) builds documents through the C ABI and produces output
byte-identical to the Rust API (`crates/pdf/tests/milestones.rs` and
`bindings/python/test_binding.py`).

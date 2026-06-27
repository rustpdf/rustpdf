# ADR 0002 — Concurrency model: `Send`, not `Sync`

* Status: Accepted
* Date: 2026-06-25
* Context: `project.md` §1.2.2

## Decision

The core is **`Send` but not `Sync`**. This covers the real use cases:

1. Generate many documents in parallel across a thread pool.
2. Move a `Document` between threads.

It deliberately does **not** support mutating one document from several threads
simultaneously (a rare case requiring external synchronization).

## Implications (decided at commit 1)

* The object graph lives in an **arena/slab** (`Vec` + indices), **not**
  `Rc`/`RefCell`. This matches the PDF model (an indirect reference is just
  "object N"), is naturally `Send`, and has better cache locality.
  Implemented in `writer::Document` (`objects: Vec<Option<Object>>`).
* Any global cache (fonts, …) must be `Sync` (`OnceLock`/`Mutex`) or not exist.
* No mutable global state.

## C ABI contract

A handle may be used from any thread, but **not from two threads
simultaneously** without external synchronization. Documented per-export in the
`ffi` crate.

## Enforcement

`cos::Object`, `writer::Document` and `pdf::Document` contain no `Rc`/`RefCell`.
A compile-time assertion (see `crates/pdf` tests) and the Definition-of-Done
checklist keep this invariant.

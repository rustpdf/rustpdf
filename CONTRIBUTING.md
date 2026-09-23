# Contributing

Thanks for helping! rust-pdf is a single Rust core plus thin bindings for ten
languages — read `docs/adr/0001-porting-strategy.md` and `docs/FFI_RULES.md`
before touching the `ffi` crate or a binding.

## Workflow

1. Open an issue for anything non-trivial so the approach can be agreed first.
2. Branch from `main`, keep the change focused, and use
   [Conventional Commits](https://www.conventionalcommits.org/) for messages
   (`feat(core): …`, `fix(php): …`, `docs(site): …`).
3. Run `make ci` (fmt-check + clippy + build + test) before pushing. Touching a
   binding? Also run its smoke test (`make python-test`, `make go-test`, …).
4. Open a PR; the template lists the Definition of Done.

## Ground rules

- Output must stay **deterministic** (no timestamps, no hash-map ordering).
- The core stays `Send`; no `Rc`/`RefCell` in the document arena.
- New dependencies must be permissively licensed (see `deny.toml`) and recorded
  in `LICENSES.md`.

By contributing you agree your work is licensed under the MIT license.

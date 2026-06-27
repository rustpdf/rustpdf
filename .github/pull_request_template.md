<!-- Definition of Done (project.md §10). Tick every box that applies. -->

## What & why

<!-- Brief description of the change and the task # from project.md it advances. -->

## Definition of Done

- [ ] Unit tests cover edge cases
- [ ] Passes applicable external validators (`qpdf` / `mutool` / `verapdf`)
- [ ] Visual regression added/updated (if it renders content)
- [ ] Round-trips on the golden corpus (if it touches parser/writer)
- [ ] **Core stays `Send`** — no `Rc`/`RefCell` in the object graph; global
      caches are `Sync`
- [ ] **Core types do NOT leak across the boundary** — only `ffi` touches the
      frontier (opaque handles)
- [ ] License of any new dependency recorded in `LICENSES.md` and allowed by
      `deny.toml`

## FFI checklist (only if `crates/ffi` changed)

<!-- See docs/FFI_RULES.md -->

- [ ] Opaque handles only; no Rust structs cross the line
- [ ] No generics/lifetimes/traits/`Result`/panic cross the line
- [ ] Every new export wrapped in `catch_unwind`
- [ ] Every new handle/buffer has a matching `*_free`
- [ ] `include/pdf.h` regenerated (`make header`) and reviewed
- [ ] Thread-safety documented for new handles

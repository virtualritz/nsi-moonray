# Agent Instructions

<!-- SPEC-DRIVEN DEVELOPMENT START -->
Spec-driven development is enabled for this repository.

Before creating or changing a feature surface:

- Read `.blueprints/domain/spec-driven-development.md`.
- Read the active spec pointer in `.specify/feature.json`.
- Read the current feature plan before editing code.
- Work one user story or one contract row at a time.
- Mark contract rows `Covered` only after source evidence and test/manual QA
  evidence are present.

Project-specific specs live in `specs/`. Shared rules and templates live in
`.blueprints/`.
<!-- SPEC-DRIVEN DEVELOPMENT END -->

> `.blueprints` is a private submodule. Without access, work from
> `specs/` and this file. No code here depends on it.

## Status

The crate emits `.rdla`. The binding strategy is settled — generate
`.rdla` now, an `extern "C"` shim over `scene_rdl2` later for
progressive rendering. See `specs/001-moonray-backend/research.md`.

Nothing renders yet, and the flush from `nsi_intermediate::Scene` is
blocked on being able to depend on that crate (`T0.7`).

## This Repository

Owns the flush into MoonRay and nothing else. Everything above the
flush lives in
[`nsi-intermediate`](https://github.com/virtualritz/nsi) and is shared
with [`nsi-mitsuba`](https://github.com/virtualritz/nsi-mitsuba).

Consumers may alias the dependency for brevity:

```rust
use nsi_intermediate as nsi_ir;
```

## Before Changing The Emitter

**Do not infer the format.** `tools/oracle` writes scenes through the
real `scene_rdl2` and its own `AsciiWriter`; the captured output is in
`specs/001-moonray-backend/oracle/` and `tests/oracle.rs` asserts this
crate reproduces it byte for byte. A new construct means capturing it
first and then emitting it — four assumptions a reasonable person would
have made about `.rdla` are wrong, and they are listed in `research.md`
F8.

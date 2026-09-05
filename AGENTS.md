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

**Spec only.** There is no crate yet, and that is deliberate: the
binding strategy is undecided, and choosing it changes what the crate
looks like. See `specs/001-moonray-backend/research.md`.

## This Repository

Owns the flush into MoonRay and nothing else. Everything above the
flush lives in
[`nsi-intermediate`](https://github.com/virtualritz/nsi) and is shared
with [`nsi-mitsuba`](https://github.com/virtualritz/nsi-mitsuba).

Consumers may alias the dependency for brevity:

```rust
use nsi_intermediate as nsi_ir;
```

## Before Writing Any Code

Settle the binding strategy. `.rdla` generation reuses the stream
emitter that already exists upstream and reaches a first image sooner;
an `extern "C"` shim over `scene_rdl2` is where it has to end up for
progressive rendering. Both are viable; picking one is `T0.2`.

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

**It renders, in process.** A recorded ɴsɪ scene is built straight into
a live `scene_rdl2` `SceneContext` and rendered by a `RenderContext` in
the calling process, progressively, with no file written and no binary
spawned. Editing the scene and calling `synchronize` re-sends only what
changed. Shading runs OSL through the root shaders in `dso/osl/`.

`.rdla` is still emitted, and is still checked byte for byte against
the captured oracle -- but it is a **dump**, not the transport. See
`HANDOFF.md` for what that cost to learn, and `specs/README.md` for
which feature owns what.

`T0.7` is **closed**: `nsi-intermediate`, `nsi-parse`, `nsi-trait` and
`nsi-ffi-wrap` are on crates.io as of 2026-09-08, and this crate
depends on them by version. A sibling `../nsi` checkout is no longer a
precondition for anything. To work on both at once, override the
dependency in a local `.cargo/config.toml` and do not commit it.

## Before Committing

```bash
just ci     # fmt-check, check, lint-check, test -- all renderer-free
```

The justfile is where the two traps live, with the reasoning at the
recipe: tests build the `cdylib` first, and the renderer tests need a
single-process runner. Read them before reaching for a bare `cargo`
invocation.

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
first and then emitting it -- four assumptions a reasonable person would
have made about `.rdla` are wrong, and they are listed in `research.md`
F8.

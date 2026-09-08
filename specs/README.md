# Specs

Feature specs live here. The active feature directory is
`.specify/feature.json`.

## Index

| # | Surface | Status |
| --- | --- | --- |
| [001](001-moonray-backend/) | MoonRay backend | Delivered. Scenes flush and render through MoonRay; the `.rdla` emitter is checked against a captured format oracle |
| [002](002-interactive-updates/) | Interactive updates | Delivered. MoonRay is linked, not spawned; edits cross in one `synchronize` and are asserted on pixels. The cost is measured, and the cheap tier for a visibility change is unreachable until `scene_rdl2` consults its own flag -- written up in [`upstream/`](../upstream/) |
| [003](003-osl/) | OSL under MoonRay | Delivered. Shading, displacement and lights run OSL through the root shaders in [`dso/osl/`](../dso/osl/); the substitution table is the fallback for a build without `$OSL_ROOT`. Open questions are at the end of its `research.md` |
| [004](004-osl-intermediate/) | `osl-intermediate` | A sketch, for someone else to start from. Proposes the crate the 003 work kept wanting; nothing in this repository depends on it |
| [005](005-packaging/) | Packaging | An install finds its own scene classes, and `just bundle` assembles a relocatable tree. The installers are configured and the release workflow is blocked on `T0.7`; Windows gets no renderer, and the spec says why |

## Scope Of This Repository

This repository owns **only the flush into MoonRay**. Recording an ɴsɪ
scene, classifying its connections and resolving ɴsɪ's graph semantics
all happen upstream in
[`nsi-intermediate`](https://github.com/virtualritz/nsi), and are shared
with the [Mitsuba backend](https://github.com/virtualritz/nsi-mitsuba).

If a behaviour is wanted by every backend, it belongs upstream, not
here.

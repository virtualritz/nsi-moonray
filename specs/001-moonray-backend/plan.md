# Plan: MoonRay Backend

## Status

**Phase 0 done, on a modest machine and with no renderer present.**
`scene_rdl2` builds, the `.rdla` format is captured from it, and the
crate reproduces that capture byte for byte.

The flush that consumes `nsi_intermediate::Scene` emits a mesh, a
camera, a `Layer`, a `GeometrySet` and a `RenderOutput`. Nothing has
rendered any of it, and materials are not mapped at all -- MoonRay has
no way to run an ɴsɪ shader.

`nsi-intermediate` is overlaid from a sibling checkout rather than
depended on properly; `T0.7`.

## Approach

**Generate `.rdla`**, MoonRay's Lua scene format, behind a document
model that leaves room for an `extern "C"` shim over `scene_rdl2` as a
second target.

Settled by experiment, as `T0.3` asked. Building against `scene_rdl2`
turned out to be cheap, which is what made the *oracle* cheap -- and
with the format captured exactly, an emitter for it can be checked end
to end today. The shim's one advantage, MoonRay's progressive modes,
needs `moonray` itself, which no available host can build yet. So the
shim buys nothing now and is still where this has to end up for
interactive work; `TN.1`.

## Gates

| Gate | Needs a heavy host | Met |
| --- | --- | --- |
| `scene_rdl2` builds alone | no | **yes** |
| `.rdla` oracle captured via `AsciiWriter` | no | **yes** |
| Binding strategy chosen by experiment | no | **yes** |
| Emitter matches the oracle byte for byte | no | **yes** |
| rdl2 reads back what the emitter writes | no | **yes** |
| A scene flushes from `nsi_intermediate::Scene` | no | **yes**, over a sibling-checkout overlay |
| `mrr` hands a scene to MoonRay's binary | no | **yes**, unrendered |
| Full MoonRay builds somewhere | **yes** | no |
| A triangle renders | **yes** | no |
| Two materials, two shapes, correct | **yes** | no |
| Transform motion blur | **yes** | no |
| Deformation motion blur | **yes** | no |

## Artifact Checklist

- [x] `spec.md`
- [x] `plan.md`
- [x] `research.md`
- [x] `data-model.md`
- [x] `contracts/flush.md`
- [x] `quickstart.md`
- [x] `tasks.md`
- [x] `checklists/requirements.md`

## Dependency

[`nsi-intermediate`](https://github.com/virtualritz/nsi), in the `nsi`
workspace. Its motion-sample resolution is an open task there, and
**this backend is the consumer that justifies doing it** -- Mitsuba
cannot blur at all.

It is also not reachable yet. The crate is unpublished, and a git
dependency on the workspace makes Cargo fetch that repository's private
`.blueprints` submodule; Cargo resolves dependencies whether or not the
feature gating them is on, so marking it optional does not sidestep it.
Either publishing `nsi-intermediate` or making the submodule
non-blocking unblocks this. `T0.7`.

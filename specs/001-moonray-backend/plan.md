# Plan: MoonRay Backend

## Status

**Spec only.** No code, no crate. Blocked on two things: a host that can
build MoonRay, and a binding-strategy decision.

## Approach

Undecided, deliberately. Two candidates:

1. **`extern "C"` shim over `scene_rdl2`**, mirroring what the Mitsuba
   backend does. `scene_rdl2` is not template-heavy the way Mitsuba is,
   so the shim should be simpler there than here.
2. **Generate `.rdla`**, MoonRay's Lua scene format. No C++ binding at
   all: `nsi-intermediate` already replays a scene as text, so emitting
   a second text format is a small step from work that exists. Cheaper
   to reach a first image, worse for interactive editing.

Option 2 is attractive as a *first* target precisely because it reuses
the stream emitter. Option 1 is where it has to end up for progressive
rendering, since `.rdla` is a batch authoring path.

## Gates

| Gate | Met |
| --- | --- |
| MoonRay builds | no |
| Binding strategy chosen | no |
| A triangle renders | no |
| Two materials, two shapes, correct | no |
| Transform motion blur | no |
| Deformation motion blur | no |

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

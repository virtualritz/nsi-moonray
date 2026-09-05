# Handoff

Written 2026-09-05. Read `specs/README.md`, then this.

## What Exists

Specs. No code, deliberately.

The eight blueprints artifacts are complete for `001-moonray-backend`,
and every contract row is `Open`. The renderer research is real — read
from a shallow clone of `OpenMoonRay/moonray` and `scene_rdl2`, with
each finding citing the file it came from.

## The Decision That Gates Everything

**`T0.2`: choose the binding strategy.** Nothing else can be scoped
until it is settled.

- **Generate `.rdla`** — MoonRay's Lua scene format. No C++ binding at
  all, and it reuses the stream emitter that already exists in
  `nsi-intermediate`, so it reaches a first image soonest. But `.rdla`
  is a batch authoring path, so it cannot reach MoonRay's progressive
  modes.
- **An `extern "C"` shim over `scene_rdl2`** — where this has to end up
  for interactive work. `scene_rdl2` is not template-heavy the way
  Mitsuba is, so the shim should be simpler here than the Mitsuba one.

Do not assume it mirrors the Mitsuba backend. The shim there was
*forced* by Mitsuba's two-parameter templates with CRTP; that pressure
does not exist here.

## What Would Bite You

**The build.** CMake over OpenVDB, Embree, ISPC and OpenImageIO,
normally in Docker. Heavier than Mitsuba's, which already exceeded the
machine this was specced on. A capable host is a precondition.

**Motion blur depends on upstream work that does not exist yet.**
`nsi-intermediate` records motion samples but resolves static transforms
only. **This backend is the reason to finish that** — Mitsuba cannot
blur, so the Mitsuba backend never forced the issue.

**ɴsɪ always returns an image.** Report limitations; never refuse a
scene. A render farm depends on it.

**Inherited top risk: silent material misbinding.** A misclassified ɴsɪ
connection does not error, it renders with materials on the wrong
shapes. `T1.4` is the two-shape, two-material test that catches it, and
nothing earlier can.

## What Was Verified, And What Was Not

Verified by reading source: motion blur both kinds, tessellation only
under displacement, analytic quadrics and curves, limit-surface subdiv
with view-adaptive refinement, the five render modes, absence of OSL.

**Not verified:** any performance claim. The progressive modes exist;
time-to-first-pixel parity with 3Delight is unmeasured and is not
claimed anywhere in these specs.

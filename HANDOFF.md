# Handoff

Written 2026-09-05. Read `specs/README.md`, then this.

## What Exists

Specs. No code, deliberately.

The eight blueprints artifacts are complete for `001-moonray-backend`,
and every contract row is `Open`. The renderer research is real — read
from a shallow clone of `OpenMoonRay/moonray` and `scene_rdl2`, with
each finding citing the file it came from.

## Where To Start

**`scene_rdl2` builds without MoonRay's heavy dependencies** — Boost,
Lua, CppUnit, OpenSSL, JsonCpp, Log4cplus, Python, TBB, and nothing from
Embree/OpenVDB/OpenImageIO/ISPC. See `research.md` F7.

That splits the work. Scene construction targets `scene_rdl2` alone and
starts today on a modest machine; only *rendering* needs a heavy host.

So the order is `T0.1` build `scene_rdl2`, then `T0.2` capture the
`.rdla` format oracle through its own `AsciiWriter`. **Do not infer the
format.** The `.nsi` emitter upstream is correct precisely because
3Delight's output was read first, and it corrected four assumptions —
`int64`, `doublematrix`, `int[2]`, and the bracketing rule — each of
which would have shipped a plausible, wrong file.

`T0.3`, the binding strategy, then becomes an experiment rather than an
argument:

- **Generate `.rdla`** — no C++ binding, reuses the stream-emitter shape
  that already exists upstream, reaches a first scene soonest. But it is
  a batch authoring path and cannot reach MoonRay's progressive modes.
- **An `extern "C"` shim over `scene_rdl2`** — where this has to end up
  for interactive work.

Do not assume it mirrors the Mitsuba backend. The shim there was
*forced* by two-parameter templates with CRTP; that pressure does not
exist here.

## What Would Bite You

**The renderer build is heavy, but only rendering needs it.** Full
MoonRay is CMake over OpenVDB, Embree, ISPC and OpenImageIO, normally in
Docker — heavier than Mitsuba's, which already exceeded the machine this
was specced on. Do not let that stop you starting: `scene_rdl2` is a
separate, much lighter build, and it is where the flush actually
targets.

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

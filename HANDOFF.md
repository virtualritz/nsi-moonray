# Handoff

Written 2026-09-05, after Phase 0. Read `specs/README.md`, then this.

## What Exists

A crate that flushes a recorded ɴsɪ scene into `.rdla`, and a captured
oracle proving it writes the real format rather than a plausible one.
Twenty-four tests; `cargo test` needs no renderer, no `scene_rdl2`, and
no network — but it does need a sibling `../nsi` checkout, for the
reason below.

`scene_rdl2` builds on a modest machine — four cores, stock Ubuntu
packages, about fifteen minutes. `quickstart.md` has the recipe and the
three upstream problems it has to work around, each with the error it
produces. None of them is subtle once seen, and all three cost time:

- The **Ninja generator cannot build it.** `ISPC_HEADER_DIRECTORY` is
  set with a leading slash, and the generated header ends up declared
  at the filesystem root while every consumer wants it in the build
  tree. Use the Makefile generator. Same under CMake 3.23.1, the
  version MoonRay's own script downloads, so it is not a regression.
- `BinPacketDictionary.h` uses `std::function` without including
  `<functional>`; GCC 13 rejects it. One line.
- A **consumer must repeat the compile definitions** rdl2's own build
  passes — `__cdecl=`, `PLATFORM_UNIX`, `PLATFORM_LINUX`, `__AVX__` —
  or `rdl2/Types.h` does not parse at its first function typedef.

## What The Oracle Corrected

Four things about `.rdla` that a careful guess would have got wrong,
and two findings in `research.md` that were simply wrong:

- `Vec3d` prints as `Vec3(...)`. No precision suffix, on any of the
  `Vec`/`Mat` types.
- A null object reference is `undef()`, not `nil`.
- A bound attribute keeps its own value alongside the binding.
- Numbers go through C++'s `%g` at `max_digits10`: `0.1f` is
  `0.100000001`. Rust's `{}` gives `0.1`, and every float in a scene
  would have differed.
- **`scene_rdl2` does need ISPC.** F7 said it did not.
- **MoonRay has no `use velocity` flag.** F1 said it had one. Velocity
  is `velocity_list_0` plus `motion_blur_type`, and the deformation
  attribute is `vertex_list_1` — `vertex list mb` is an alias.

This is the same discipline that made the `.nsi` emitter correct, and
it paid the same way. Keep it: capture, then emit.

## The Dependency, And Why It Is A Path

`nsi-intermediate` is overlaid from a sibling `../nsi` checkout. That
is a workaround, not a resolution: the crate is unpublished, and a git
dependency on the `nsi` workspace makes Cargo fetch that repository's
private `.blueprints` submodule. Two things worth knowing before
trying to improve it — both were tried:

- Making the dependency **optional** does not help. Cargo resolves
  every dependency whether or not the feature gating it is enabled.
- **`[patch]`** does not help either. Cargo fetches the patched git
  source anyway, and fails on the same submodule.

Publishing `nsi-intermediate`, or making `.blueprints` non-blocking for
a consumer's fetch, is what would actually settle it. `T0.7`.

## Where To Start

**`T0.6`, the authoring twin of `RdlMeshGeometry`.** Its DSO lives in
`moonray`, so nothing on a host without the renderer can read back a
scene that uses it — the flush's output is currently checked as text
only, while the oracle's is checked by rdl2 itself. But
`moonray/dso/geometry/RdlMesh/attributes.cc` needs only `scene_rdl2`
headers: build that file with a stub implementation and there is a
faithful declaration-twin to author against, and `oracle verify` starts
covering real mesh scenes.

Then **`T1.3`, materials.** Every `Layer` row this emits has `undef()`
where the material goes, because MoonRay has no way to run an ɴsɪ
shader and naming a class it does not have would make the file fail to
load. The decision to make is what to substitute — `UsdPreviewSurface`
is the one general-purpose material in `moonray/dso/material`, and how
much of an ɴsɪ shader's parameters can be fed into it is an open
question, not a mechanical mapping.

## What Would Bite You

**The renderer build is still heavy, and nothing has rendered.** Every
claim in these specs about what MoonRay *does* comes from reading its
source. No image has been produced, and no performance claim is made
anywhere.

**Two motion samples, not many.** rdl2's `AttributeTimestep` is
`TIMESTEP_BEGIN` and `TIMESTEP_END`. ɴsɪ has no such limit, so a scene
with three samples on one attribute cannot be carried across. Report
the reduction; do not quietly keep the first and last.

**`is_subd` defaults to true.** An ɴsɪ `mesh` that does not set it
false explicitly renders as a subdivision surface.

**ɴsɪ always returns an image.** Report limitations; never refuse a
scene. A render farm depends on it.

**Inherited top risk: silent material misbinding.** A misclassified ɴsɪ
connection does not error, it renders with materials on the wrong
shapes. `T1.4` is the two-shape, two-material test that catches it, and
nothing earlier can.

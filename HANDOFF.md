# Handoff

Written 2026-09-05, after Phase 0. Read `specs/README.md`, then this.

## What Exists

A crate that writes `.rdla`, and a captured oracle proving it writes the
real format rather than a plausible one. Sixteen tests; `cargo test`
needs no renderer, no `scene_rdl2`, and no network.

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

## Where To Start

**`T0.7`, and it is not a coding task.** The flush cannot be written
because `nsi-intermediate` cannot be depended on — it is unpublished,
and a git dependency on the `nsi` workspace makes Cargo fetch that
repository's private `.blueprints` submodule. Cargo resolves every
dependency whether or not the feature gating it is enabled, so
declaring it optional does not sidestep this: with the dependency
present and unreachable, `cargo test` fails outright. Publishing the
crate, or making the submodule non-blocking for consumers, unblocks
every `T1.*` and `T2.*` task.

Then `T0.5` and `T0.6`, both of which still need no renderer:

- **`T0.5`, round-trip.** Byte equality against a captured file proves
  the syntax. Only feeding what this crate writes to rdl2's
  `AsciiReader` proves rdl2 *accepts* it.
- **`T0.6`, an authoring twin of `RdlMeshGeometry`.** Its DSO lives in
  `moonray`, so a scene using it cannot be built or read back on a host
  without the renderer. But its `attributes.cc` needs only `scene_rdl2`
  headers: build that file with a stub implementation and there is a
  faithful declaration-twin to author against. It is what lets the
  `T1.*` work be checked at all before a heavy host exists.

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

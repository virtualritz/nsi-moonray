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

## Upstream Moved, And It Answers The Hard Part

`nsi-intermediate` gained a synchronise journal and dirty propagation —
`Scene::take_changes()` and `Scene::affected()` — plus motion-sample
resolution, a `.nsi` parser (`nsi-parse`) and much else. Three things
`002` listed as upstream asks were already done.

Two API changes bite a backend written against the old crate, and both
are corrections rather than churn:

- **`Node::attrs` is not the value.** `SetAttributeAtTime` on an
  attribute that is not motion data sets it for the whole shutter, so
  reading `attrs` answers "not set" for something the renderer honours.
  Use `Node::effective`, which is what the resolver reads.
- **An ɴsɪ string is bytes, not `String`.** A file name need not be
  UTF-8, and 3Delight round-trips a high byte unchanged; storing
  `String` would replace it at recording time, where nothing later
  could undo it.

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

## How This Reaches A Renderer

Two shapes, and neither asks this repository to build MoonRay:

- **`mrr`** hands a `.rdla` to MoonRay's own binary. It exists.
  Whoever renders needs `moonray` installed; nothing here needs it to
  write or check a scene.
- **`libnsi_moonray.so`** is a drop-in ɴsɪ renderer, and it exists:
  `src/capi.rs`. `nsi-ffi-wrap` `dlopen`s a library and resolves its
  whole symbol table up front, so all twelve have to be there —
  the eleven `NSI*` entry points plus `DspyRegisterDriver`, which the
  `output` feature looks up and whose absence would make the load fail
  for a consumer that has that feature on. `tests/dropin.rs` opens the
  built artefact and drives a triangle through it by symbol.

  What it is not yet: interactive. `"start"` runs MoonRay to
  completion, so `"synchronize"`, `"suspend"` and `"resume"` have
  nothing to act on and a display driver receives a file rather than
  pixels.

Taking `.nsi` *files* is a third thing and needs a parser that does not
exist: `nsi-intermediate` writes streams and does not read them, and
`nsi-stream` is the pixel-streaming driver, not a reader. That parser
belongs upstream beside the writer. `T4.3`.

Spawning the binary cannot stream samples back, so progressive
rendering means linking `libmoonray` instead. `T4.4`.

## Where To Start

**`T0.6`, the authoring twin of `RdlMeshGeometry`.** Its DSO lives in
`moonray`, so nothing on a host without the renderer can read back a
scene that uses it — the flush's output is currently checked as text
only, while the oracle's is checked by rdl2 itself. But
`moonray/dso/geometry/RdlMesh/attributes.cc` needs only `scene_rdl2`
headers: build that file with a stub implementation and there is a
faithful declaration-twin to author against, and `oracle verify` starts
covering real mesh scenes.

Materials are substituted, not translated: every ɴsɪ shader becomes a
`UsdPreviewSurface` at its defaults. Carrying its *parameters* across
(`T1.3a`) needs real scenes — which ɴsɪ shader parameter means
`roughness` depends on the shader, and a guessed name table is exactly
the kind of plausible-but-wrong the oracle discipline exists to
prevent.

## What Has Been Rendered

Three things, all through a MoonRay built here:

- a triangle, which is what found the two black-image bugs below;
- a translated quad, checked against where the transform puts it rather
  than against the matrix in the file (`T1.2`);
- two quads with two materials, red and green, asserted per channel
  (`T1.4` — the inherited top risk, and the one thing reading the file
  could never have settled).

And `examples/polyhedron` renders a `polyhedron-ops` polyhedron through
this backend loaded as `lib3delight.so`, which is the drop-in path
working end to end with an unmodified consumer.

## What Would Bite You

**Two ways a perfectly correct scene renders black**, both found by
rendering rather than by reasoning, and both now fixed:

- MoonRay renders what the `Layer` names. Geometry left out of it is
  not dim — it is absent.
- MoonRay skips a `Layer` row whose **material column is `undef()`**.
  The same triangle is missing from the image without a material and
  present with one, which is why unshaded geometry gets a default
  `UsdPreviewSurface` rather than an honest-looking `undef()`.

Neither produces a warning. Both produce a black image from a file that
parses, round-trips and reads correctly. Expect more of this shape, and
expect to find it the same way.

**The renderer build is heavy but it does work.** MoonRay built from
source here in about fifty minutes on four cores; `quickstart.md` has
the recipe and the five packaging problems that stop it. Performance is
still unmeasured and unclaimed anywhere.

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

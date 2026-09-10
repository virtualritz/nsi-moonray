# `nsi-moonray`

An [ɴsɪ](https://nsi.readthedocs.io/) backend on
[MoonRay](https://github.com/OpenMoonRay/moonray), DreamWorks
Animation's production renderer.

**Status: it renders, in process.** A recorded ɴsɪ scene is built
straight into a live `scene_rdl2` `SceneContext` and rendered by a
`RenderContext` in the calling process -- no scene file, no spawned
binary. Pixels come back as they converge, rectangle by rectangle, to
the closures an ɴsɪ application hands its output driver; a scene can be
edited between frames and only what the edit touched is re-sent.

`.rdla`, MoonRay's ASCII scene format, is still written on request --
`mnry cat`, a bug report, an oracle diff -- but it is a **dump**, not the
transport. Every byte of that format was captured from `scene_rdl2`'s
own `AsciiWriter` rather than inferred, and rdl2 reads back what is
written; see
[`specs/001-moonray-backend/oracle/`](specs/001-moonray-backend/oracle/).

What crosses: polygon meshes and subdivision surfaces with creases and
corners, instancing (native on both sides, nested, and blurred),
transform and deformation motion blur on one scene-wide shutter,
cameras -- perspective, orthographic, fisheye and spherical -- render
outputs, including built-in, primitive-variable and per-lobe AOVs,
OpenVDB volumes, `st`, `N` and arbitrary primitive variables in
any of ɴsɪ's interpolations,
and lights -- which in ɴsɪ are geometry wearing an emissive shader
rather than nodes of their own.

Shading runs OSL. ɴsɪ *is* OSL -- a `shader` node names a compiled
`.oso` -- so the network crosses as an OSL group specification and
MoonRay executes it, through the `Osl`, `OslDisplacement`, `OslMap` and
`OslVolume` root shaders in [`dso/osl/`](dso/osl/): surfaces,
displacement, a light's emission, and volumes. That needs OSL at build
time, and `just setup` provides it.

Without it, materials are substituted rather than translated: every ɴsɪ
shader becomes a `UsdPreviewSurface`, MoonRay's stock PBR surface,
carrying whatever parameters that shader is known to have. The known
shaders are a table read off 3Delight's own compiled `.oso` files
rather than guessed at; anything else is reported by name. A
displacement has no such substitute and is reported instead.

### Why OSL all the way, and not a table of names

**A light is the clearest case.** ɴsɪ has no light nodes at all:
section 4.5 says a light is geometry whose surface shader produces an
`emission()` closure. That is not an accident of the specification --
it is what lets *one* surface emit and reflect at once. A screen, a
glowing filament in a metal housing, an emissive decal on a shaded
panel: all one object, and the shader decides which parts of it glow.

A renderer that recognises lights by shader *name* cannot express any
of that. It infers a light type from a name, throws the shader away,
and the object becomes a light *or* a surface.

**This backend executes the shader instead.** Every ɴsɪ emitter becomes
a MoonRay `MeshLight` -- the one class that takes arbitrary geometry --
and its radiance comes from an `OslMap` running the ɴsɪ network itself.
The light's colour, its intensity and how both vary across its own
surface are the closure's, sampled per point, rather than a flat value
a name table guessed at. MoonRay's analytic light classes are not
reachable from ɴsɪ and are not meant to be: there is no ɴsɪ node that
would name one.

The name table survives only as a fallback for a build without OSL, and
only when no compiled `.oso` can be found behind the shader. Then
MoonRay supplies the photometry, the light is in the right place with
the wrong look, and the flush says so.

One gap remains, and it is the one the specification is written to
allow:

- A shader that emits *and* shades has **no** faithful mapping.
  `RenderContext::createMeshLightLayer` skips a light whose geometry is
  in the render layer, so one mesh is either shaded or a light. The
  surface is kept -- a metal object rendering as a featureless emitter
  is the more visibly wrong of the two -- and its emission becomes
  hit-only, lighting nothing but itself. A masked emissive decal on a
  metal panel is exactly this case, and exactly what ɴsɪ's model was
  designed to express.

So the whole shading surface now goes the same way: surfaces,
displacement, volumes and lights all cross as OSL group specifications
and MoonRay runs them. What a shader *does* is answered by executing
it, not by recognising what it is called.

## `mnry`

```bash
mnry render scene.nsi                 # in process, if built with `rdl2`
mnry render 'shot.@4.nsi' -f 1-48     # a frame sequence
mnry cat scene.nsi -l                 # what MoonRay would be given
mnry watch /spool -r                  # render what lands there
```

Modelled on [`rdl`](https://github.com/virtualritz/delight-helpers), the
`renderdl` replacement, so the two commands take the same shape -- same
subcommands, same frame-sequence syntax. What differs is underneath:
`rdl` drives 3Delight through the ɴsɪ C API, and this parses the same
streams into `nsi-intermediate` and builds MoonRay's scene from them
directly.

Every scene class is loaded from MoonRay's `rdl2dso` directory, and an
ordinary install is found without being named: beside the running
binary first, so a bundle works wherever it was unpacked, then where
the platform puts one -- `~/.local/share/moonray` on Linux,
`~/Library/Application Support/MoonRay` on macOS, `%LOCALAPPDATA%\MoonRay`
on Windows, then the system-wide equivalents.

`--dso-path`, or `$NSI_MOONRAY_DSO`, overrides that. It is not
second-guessed: a path that is not there fails rather than falling back
to an install you did not ask for, because rendering with a renderer
you did not choose is the worse outcome. When nothing is found, every
directory tried is listed -- a path that is almost right is the common
case and is invisible otherwise.

Built without the `rdl2` feature -- the default, since linking
`scene_rdl2` needs it installed -- `mnry render` writes the scene out and
runs the `moonray` binary instead. Same image, later, and `-v` says
which path it took.

## The Demo

`examples/polyhedron` builds a polyhedron with
[`polyhedron-ops`](https://github.com/virtualritz/polyhedron-ops), hands
it to the ɴsɪ context that crate already talks to, and renders it with
MoonRay. Nothing in it mentions MoonRay beyond the name: the `nsi`
crate resolves a renderer at run time, and asking it for `"moonray"`
is the whole trick.

## As A Drop-In Renderer

The crate also builds as `libnsi_moonray.so`, exporting the ɴsɪ C entry
points -- the same thirteen 3Delight exports, so it is loadable
wherever 3Delight is. The [`nsi`](https://github.com/virtualritz/nsi)
crate picks a renderer by name, at run time:

```rust
let context = nsi::Context::new(Some(&[
    nsi::string!("renderer", "moonray"),
]));
```

Two contexts in one process may name two different renderers. Without
the argument, `$NSI_RENDERER` decides.

`$NSI_MOONRAY` is where a host looks for this backend, the way
`$DELIGHT` is where it looks for 3Delight: a prefix whose `lib` holds
the library, or the directory the library is in, so a checkout's
`target/release` works as-is. `packaging/env.sh` sets it, and a bundle
is found without it.

**Not `$MOONRAY_ROOT`,** which names DreamWorks' renderer. This is the
ɴsɪ front end onto it, they are installed separately often enough, and
one variable for both would make "which MoonRay is this" unanswerable.

**Render an OSL scene with `-exec_mode scalar`** if you take a `.rdla`
dump to the stock `moonray` binary. OSL shades one point at a time and
has no vectorized path, and MoonRay's default mode skips what it
cannot call, without a word. The linked and spawned renderers here
force it; a dump cannot, so the flush says so.

`NSIRenderControl "start"` builds the scene into a live renderer and
starts a frame; with `"interactive"` it stays up, and `"synchronize"`
re-sends only what the edits since the last call touched. The output
driver's callbacks are called as the frame converges, with the
rectangles that changed. `$NSI_MOONRAY_SCENE` names where a `.rdla`
dump is written, which is how you look at what a render was made from.

## Installing

From a checkout:

```bash
just setup     # dependencies, then MoonRay (about an hour)
just install   # mnry into ~/.cargo/bin, renderer linked
```

`just install` is `cargo install --path .` with the renderer feature
turned on when there is a renderer to turn it on for, and without when
there is not. Plain `cargo install --path .` works too and gives you
the emitter -- `mnry cat`, `.nsi` to `.rdla`, scene conversion -- with
`mnry render` falling back to spawning the `moonray` binary.

**No flag is needed to find the scene classes.** `just setup` installs
MoonRay into the platform's per-user data directory, which is one of
the places `mnry` searches, so an installed binary finds an installed
renderer on its own. `tests/bundle.rs` holds those two defaults
together, since they are written in different files and drifting apart
would render a black frame in silence.

For distribution rather than a checkout:

```bash
just bundle    # a relocatable tree in dist/bundle
just package   # .deb and AppImage, or .dmg
```

The tree is `bin/mnry` beside `lib/rdl2dso`, which is the first place
the resolver looks. `patchelf` is what makes it relocatable on Linux;
without it the bundle works only where it was built, and says so.

**Windows gets no renderer.** MoonRay does not build there -- its
CMake handles Unix and Darwin and has no MSVC path -- so a Windows
package would carry the emitter and nothing to render with. That is a
different product and is not shipped as this one.
[`specs/005-packaging/`](specs/005-packaging/) has the reasoning and
what is still missing, code signing included.

## Building

Nothing but Rust and this repository:

```bash
git clone https://github.com/virtualritz/nsi-moonray.git
cd nsi-moonray && just ci
```

`just --list` has the rest. That builds and tests the emitter, the
flush and the oracle -- everything except linking a renderer.

Handles are interned by default (`interned_handles`: upstream's
`ustr_handles`, plus this crate's `Name` for class names, handles and
attribute names). `tools/footprint` measures a 100 001-node scene
recorded and flushed at 244.8 MB and 32.6 s without it, and 174.3 MB
and 11.7 s with -- 29 % off both the scene and the document, and 2.8×
faster to record. `default-features = false` turns it, and `mnry`,
off.

### With a renderer

```bash
just setup       # dependencies, then MoonRay into vendor/install
just test-rdl2   # the tests that link it and render
```

Roughly an hour, nearly all of it MoonRay. `just renderer-check`
verifies the tools and headers before anything is cloned, which beats
learning an hour in that ISPC is missing, and the build picks up where
a failure stopped.

```bash
cargo binstall --git https://github.com/prefix-dev/pixi pixi
```

**`--git`, not the plain crate name.** crates.io still has pixi 0.15.2
while the current release is 0.80, so `cargo binstall pixi` fetches
something two years old. This form reads the version from the
repository and puts the binary in `~/.cargo/bin` like any other Rust
tool, rather than creating `~/.pixi` and editing a shell profile the
way the installer at pixi.sh does.

**Dependencies come from [pixi](https://pixi.sh), and that is not a
preference.** Open Shading Language is not packaged by Ubuntu -- its
`libosl-dev` is a library for Shogi programs -- nor by Homebrew, and
without OSL every shader becomes a `UsdPreviewSurface`. The
[ASWF conda channel](https://github.com/anderslanglands/aswf-pixi) has
it, built against a matching OpenImageIO. pixi also needs no root,
installs into `.pixi/` inside the checkout, and resolves Linux and
macOS from one lockfile. `pixi.toml` says what comes from where and
why; `just deps` is the system-package route instead, and it has no
OSL.

Two things pixi does not cover, both handled by
`packaging/renderer.sh`: OpenImageDenoise, whose conda build pins an
OpenImageIO that OSL 1.15 conflicts with, so the official binary
release is downloaded; and `log4cplus` on macOS ARM, which conda-forge
does not build and which is compiled from source there.

Every renderer recipe uses `$SCENE_RDL2_ROOT` when set and
`vendor/install` otherwise, so an install you already have needs only
`SCENE_RDL2_ROOT=/path/to/install just test-rdl2`. `just env` says what
the recipes can see.

**MoonRay is cloned, not a submodule.** Four repositories and several
hundred megabytes, every one of them useless to somebody who wants the
emitter and the flush -- which is the common case, and the reason
`rdl2` is off by default. `packaging/renderer.sh` pins each ref, which
buys the same reproducibility only when asked for.

Longer term the hour goes away: `packaging/conda/` drafts a recipe so
that `pixi add nsi-moonray` installs the backend with a renderer
already built. It needs `scene_rdl2` and MoonRay packaged first, and a
conversation with the channel owner.

## Why MoonRay

Apache-2.0, actively developed, and its scene model maps onto ɴsɪ more
closely than the alternatives:

| ɴsɪ | `scene_rdl2` |
| --- | --- |
| node with a handle | `SceneObject` |
| node type | `SceneClass`, with declared typed attributes |
| shader connection with named ports | attribute **bindings**, ports carried natively |
| `attributes` node | `Layer`, a real assignment table |
| `.nsi` stream | `.rdla` / `.rdlb` |
| motion samples | `blur(a, b)` on an attribute |

And it does things the Mitsuba backend cannot:

- **Motion blur, both kinds.** `node_xform` takes blur samples and
  `RdlMeshGeometry` has `vertex_list_1` for deformation. Mitsuba 3
  dropped `AnimatedTransform` and cannot blur at all. Two samples,
  though: rdl2 has exactly two timesteps.
- **Analytic primitives stay analytic.** Spheres, boxes and nine native
  curve types go to Embree without tessellation. Polygon meshes are
  tessellated *only* when displacement is assigned.
- **Subdivision at the limit surface.** OpenSubdiv `Far::PatchTable` +
  `EvaluateBasis`, with view-dependent adaptive tessellation.
- **Progressive rendering.** `PROGRESSIVE`, `PROGRESSIVE_FAST` and
  `REALTIME` modes, the fast one substituting normals for radiance to
  get something on screen immediately.

Findings were read from the source, not the documentation; each is cited
in `specs/001-moonray-backend/research.md`.

## Architecture

This repository owns **only the flush**. Recording, connection
classification and graph resolution happen upstream in
[`nsi-intermediate`](https://github.com/virtualritz/nsi), shared with
[`nsi-mitsuba`](https://github.com/virtualritz/nsi-mitsuba).

```
ɴsɪ calls -> nsi-intermediate -> nsi-moonray -> scene_rdl2 -> MoonRay
                             \
                              -> nsi-mitsuba -> Properties -> Mitsuba 3
```

Consumers may alias the dependency:

```rust
use nsi_intermediate as nsi_ir;
```

## Documentation

Spec-driven; see [`specs/`](specs/). Shared standards come from
`.blueprints`, a private submodule -- a plain `git clone` works, and only
`--recurse-submodules` fails, on that one path.

## Licence

MIT OR Apache-2.0 OR Zlib.

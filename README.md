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

Two gaps remain:

- **An `environment` node's shader never runs.** MoonRay's `EnvLight`
  is a light class rather than a shader: its whole attribute list is a
  texture and some colour correction, with nothing an `OslMap` could
  bind to. So an environment crosses as colour, intensity, exposure and
  a texture path, and everything else the shader does -- a gradient, a
  mapping, a per-component contribution -- is lost.

  **The size of that loss is easy to miss.** 3Delight's
  `environmentLight` defaults `i_color` to 0.5 grey and *applies* it,
  because 3Delight executes the shader. A substituting backend that
  reads only the intensity gets a white dome: a full stop brighter
  across every surface it lights, with a black background where the sky
  should be. Someone using one renderer alone would tune the scene
  around that and have it come out wrong in the other. What the scene
  leaves unset is therefore read from the compiled shader's own
  declarations rather than from whatever the rdl2 class happens to
  default to.

  The route that would remove this is an enclosing dome mesh wearing
  the shader, since `MeshLight` is the one light class that takes an
  `OslMap`. It costs the sampling quality an analytic environment light
  has, which is why it is written down rather than done.

- A shader that emits *and* shades has **no** faithful mapping.
  `RenderContext::createMeshLightLayer` skips a light whose geometry is
  in the render layer, so one mesh is either shaded or a light. The
  surface is kept -- a metal object rendering as a featureless emitter
  is the more visibly wrong of the two -- and its emission becomes
  hit-only, lighting nothing but itself. A masked emissive decal on a
  metal panel is exactly this case, and exactly what ɴsɪ's model was
  designed to express.

Everything else goes the same way: surfaces, displacement, volumes and
emissive geometry all cross as OSL group specifications and MoonRay
runs them. **No built-in MoonRay shader is ever substituted for one the
scene named** -- the substitution table is the fallback for a build
without OSL, not a path a working install takes. What a shader *does* is answered by executing
it, not by recognising what it is called.

## One Scene, Two Renderers

ɴsɪ is an interface, not a renderer. `tests/shaderballs.rs` builds a row
of five spheres -- matte, plastic, glass, metal, emissive -- once,
through `nsi::Context`, and renders it twice. Every sphere wears the
same 3Delight shader, `dlPrincipled`, at five parameter sets. The two
passes differ by one string:

```rust
nsi::Context::new(Some(&[nsi::string!("renderer", "3delight")]));
nsi::Context::new(Some(&[nsi::string!("renderer", "moonray")]));
```

**3Delight**

![Five spheres rendered by 3Delight](doc/images/shaderballs-3delight.png)

**MoonRay, through this crate**

![The same five spheres rendered by MoonRay](doc/images/shaderballs-moonray.png)

**Both sides run OSL.** Every sphere and the floor cross as an OSL
group specification that MoonRay executes; no built-in stands in for a
shader the scene named. The environment is the one thing MoonRay has no
way to run a shader for, so the scene asks for MoonRay's own class by
name through the stub in [`shaders/`](shaders/) -- and the two skies
then measure identical, which is what makes the rest of the frame worth
comparing.

The disagreements that remain are the renderers', and there are two:

- **Glass.** `refract_weight` reaches the shader, but the closure walk
  here has no transmission path, so the lobe is dropped and the sphere
  renders opaque.
- **The emitter.** It is seen, at the right colour, but it lights less
  than it should and the metal sphere's reflection of it is weaker.
  `dlPrincipled` both emits *and* shades, so it is kept a surface
  rather than becoming a `MeshLight` -- and MoonRay's self-emission is
  hit-only, with no next-event estimation toward emissive geometry.
  This is the case the section above is about, and the clearest
  argument for OSL end to end: one surface that both emits and reflects
  is exactly what the interface exists to express.

Matte and plastic agree closely -- MoonRay about a third of a stop
brighter -- which is the other half of the result: geometry, camera,
and the diffuse and specular response all cross intact.

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
just pixi-install   # the dependencies, into .pixi/, no root
just install        # MoonRay, then mnry linked against it
```

`just install` takes where to put MoonRay, and defaults to the
platform's own per-user data directory:

```bash
just install                      # ~/.local/share/moonray, or
                                  # ~/Library/Application Support/MoonRay
just install /opt/moonray         # or wherever you want it
```

It builds a renderer only if that place has none -- about an hour, and
it says so before it starts -- then `cargo install`s `mnry` into
~/.cargo/bin with the renderer feature turned on. `just renderer`
rebuilds one that is already there.

Afterwards it offers to append `PREFIX/bin` to your `~/.zshrc`,
`~/.bashrc` or `~/.config/fish/config.fish`, because that is where
`moonray`, `rdl2_print` and the rest of MoonRay's own tools land and
nothing else puts them on the PATH. It asks, takes silence for no, and
prints the line either way; `packaging/path.sh --yes DIR` does it
unattended.

Plain `cargo install --path .` works too and gives you the emitter --
`mnry cat`, `.nsi` to `.rdla`, scene conversion -- with `mnry render`
falling back to spawning the `moonray` binary.

**Why the default location is not arbitrary.** `src/dso.rs` searches
it, so an installed `mnry` finds an installed renderer with no flag;
`build.rs` searches it too, so `--features rdl2` compiles without
`$SCENE_RDL2_ROOT` set. Installing anywhere else works and means
setting that variable. `tests/bundle.rs` holds those defaults
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
just setup       # dependencies, then MoonRay into the default prefix
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

Every renderer recipe uses `$SCENE_RDL2_ROOT` when set and the
platform's per-user data directory otherwise, so an install you already
have needs only `SCENE_RDL2_ROOT=/path/to/install just test-rdl2`.
`just env` says what the recipes can see.

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

## Limitations

Everything below is permanent, in the sense that no amount of code in
this crate fixes it -- the gap is in MoonRay's own control surface, not
in how much of it has been wired up yet. Attribute-by-attribute gaps
(`quality.shadingsamples` splitting into two, ray-depth counting a
bounce differently, and so on) are not repeated here; those are
conversions, done once, with the reasoning next to the code
(`src/flush.rs`, the `GLOBALS` table and its doc comments). This table
is the other kind: ɴsɪ can say something and there is nowhere on the
MoonRay side to put it, so the flush reports it and moves on. Every row
is a live `flushed.limitations.push(...)` site or a documented gap in
`upstream/`, not a guess at what might be missing.

One entry does not belong in a table of losses and is called out
separately: a geometry ɴsɪ connects under several transforms, each with
its own material, is one shared object in the interface (§ "connecting
a node to two transforms draws it twice"), and 3Delight renders it that
way. `RdlMeshGeometry` has no equivalent -- one object, one transform,
one material -- so this backend expands the shared node into one
`RdlMeshGeometry` per placement at the translator boundary
(`src/flush.rs`, around the `shared` placements branch). The render
matches what the ɴsɪ scene describes; nothing is dropped, only
duplicated internally. Tested, not a workaround-in-progress.

What follows *is* dropped, one way or another:

| Area | ɴsɪ can express | On this backend |
| --- | --- | --- |
| Caustics | `caustics.cast`, `caustics.receive`, `caustics.emit` on a shader, `quality.causticsamples` on `.global` | MoonRay's "caustic" is an eye-caustic BSDF flag, not a photon-density control surface. There is nothing to bind these to, so they are reported as unread attributes and otherwise ignored. |
| Holdouts | `matte` on a shape | No MoonRay counterpart at all. The object renders as ordinary geometry; a hole in the render stays filled. |
| Mesh lights | A `MeshLight`'s reference geometry also carrying a material | MoonRay segfaults during render prep the moment such geometry gets a `map_shader` (`upstream/moonray-meshlight-map-shader-segfault.md`). One mesh is a light or a surface, never both, at the `RdlMeshGeometry` level -- see "a shader that emits *and* shades" above for the shading-side consequence of the same limit. |
| OSL execution mode | Any OSL shading at all | OSL has no vectorized path; it shades one point at a time. MoonRay's default execution mode is vectorized and silently skips every OSL shader and light map it cannot call. This backend forces `-exec_mode scalar` when it links or spawns MoonRay itself; a `.rdla` dump handed to the stock `moonray` binary needs the flag set by hand, which is why the flush says so. |
| Environment shading | An `environment` node's full OSL network -- gradients, mappings, per-component contributions | `EnvLight` is a light class, not a shader; its attribute list is a texture and colour correction, with nothing an `OslMap` could bind to. Only colour, intensity, exposure and a texture path cross; the rest of what the shader does is lost. |
| Emissive surfaces | A single shader that both emits and shades (ɴsɪ's whole point: one surface, one closure) | `RenderContext::createMeshLightLayer` skips a light whose geometry is also in the render layer, so the object is kept a surface and its self-emission becomes hit-only -- no next-event estimation toward it. No faithful mapping exists. |
| Render globals | `.global` attributes with no `GLOBALS`-table row, e.g. `quality.denoise`, `quality.causticsamples`, most of the rest of the forty-three attributes on that node beyond sample counts, ray depths and thread count | Reported by name (`report_unread_global`) and otherwise not applied; MoonRay's own defaults are used. |
| Motion samples | More than two motion samples on a transform or on `P` | `scene_rdl2` has exactly two timesteps. Extra samples are resampled onto the shutter's open and close and the intermediate shapes or transforms are lost. |
| Instancer blur | Rotation and scale varying across the shutter on an instancer | `xform_list` cannot carry two timesteps' worth of a full transform per instance; only translation is blurred, and the flush says so per instancer. |
| Cryptomatte | One identity output per kind (object, material, ...) | MoonRay has one Cryptomatte output, not one per kind; it carries object identity and nothing else. |
| Subdivision scheme | Any scheme a shader names | MoonRay only has Catmull-Clark; anything else falls back to it. |
| Curve basis | Non-linear curve bases, `extrapolate` | MoonRay's curve geometry interpolates linearly only; other bases render as linear, and extrapolation has no counterpart -- the curve ends where its vertices do. |
| Cameras | `cylindricalcamera`, most fisheye mappings other than MoonRay's default | No MoonRay equivalent; the camera is skipped, or the mapping falls back to MoonRay's own. |
| Particles | `N` on point particles, for orientation | MoonRay renders a point as a sphere, which nothing can orient. |
| Volumes | A scalar emission grid in an OpenVDB volume | `VdbGeometry` reads only an RGB emission grid and refuses a scalar one outright, which stops the volume rendering at all. |

A polygon mesh without an `N` primvar is not on this list: MoonRay used
to invent smooth normals for it where ɴsɪ's own default is flat
shading, and that was a bug in this backend, not a MoonRay limitation
-- fixed in 09f6358, `smooth_normal` is now forced off in that case.
Listed here only so nobody goes looking for it.

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

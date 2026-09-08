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
MoonRay executes it, through the `Osl` and `OslDisplacement` root
shaders in [`dso/osl/`](dso/osl/). That needs OSL at build time:
set `$OSL_ROOT`.

Without it, materials are substituted rather than translated: every ɴsɪ
shader becomes a `UsdPreviewSurface`, MoonRay's stock PBR surface,
carrying whatever parameters that shader is known to have. The known
shaders are a table read off 3Delight's own compiled `.oso` files
rather than guessed at; anything else is reported by name. A
displacement has no such substitute and is reported instead.

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
MoonRay. Nothing in it mentions MoonRay: `nsi-core` resolves a renderer
at run time, and pointing that resolution at `libnsi_moonray.so` is the
whole trick.

## As A Drop-In Renderer

The crate also builds as `libnsi_moonray.so`, exporting the ɴsɪ C entry
points. `nsi-ffi-wrap` reaches a renderer by `dlopen` -- the library name
and the environment variable that finds it are parameters of its
`define_nsi_renderer!` macro, not constants -- so an ɴsɪ application can
load MoonRay exactly where it loads 3Delight:

```rust
nsi_ffi_wrap::define_nsi_renderer! {
    name: MoonRay,
    dynamic: {
        linux: "libnsi_moonray.so",
        macos: "libnsi_moonray.dylib",
        windows: "nsi_moonray.dll",
    },
    env_var: "MOONRAY_NSI",
    link_feature: "link_moonray",
}
```

`NSIRenderControl "start"` builds the scene into a live renderer and
starts a frame; with `"interactive"` it stays up, and `"synchronize"`
re-sends only what the edits since the last call touched. The output
driver's callbacks are called as the frame converges, with the
rectangles that changed. `$NSI_MOONRAY_SCENE` names where a `.rdla`
dump is written, which is how you look at what a render was made from.

## Installing

An install carries MoonRay with it and finds its own scene classes, so
nothing has to be set:

```bash
just bundle /path/to/moonray-install   # a relocatable tree in dist/bundle
just package                           # .deb and AppImage, or .dmg
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

With a renderer, point at it and turn the feature on:

```bash
export SCENE_RDL2_ROOT=/path/to/install
export MOONRAY_ROOT=/path/to/install
export NSI_MOONRAY_DSO=$MOONRAY_ROOT/rdl2dso
just test-rdl2
```

`just env` says which of those the recipes can see, which is worth
running first when one of them does something surprising.

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

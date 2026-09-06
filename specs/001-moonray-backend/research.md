# Research: MoonRay Backend

Findings are from reading `OpenMoonRay/moonray` and
`OpenMoonRay/scene_rdl2` at a shallow clone taken 2026-09-05, not from
documentation. Findings marked **built** were since checked against a
`scene_rdl2` that was actually built and run; two of the originals were
wrong, and both are corrected in place below.

## Why MoonRay

Apache-2.0, active, and its scene model is closer to ɴsɪ than Mitsuba's.

| ɴsɪ | `scene_rdl2` |
| --- | --- |
| node with a handle | `SceneObject` |
| node type | `SceneClass`, with *declared* typed attributes |
| attribute | `Attribute` / `AttributeKey`, typed |
| shader-network connection with ports | attribute **bindings**, which carry named ports |
| `attributes` node binding material to geometry | `Layer`, a real assignment table |
| the context | `SceneContext` |
| `.nsi` stream | `.rdla` (ascii) / `.rdlb` (binary) |
| motion samples | `blur(a, b)` on an attribute |

Two of these are better matches than Mitsuba offers. Mitsuba references
point at whole objects, so ɴsɪ's named shader ports need an adapter
there; MoonRay's bindings carry ports natively. And Mitsuba wants a
`bsdf` on each shape, where `Layer` is an assignment table shaped like
the ɴsɪ `attributes` node being dissolved.

## Findings

### F1: Motion blur, both kinds

- **Transform:** `scene_rdl2/lib/scene/rdl2/Node.cc` declares
  `node_xform` as a `Mat4d` attribute. `SceneVariables` carries
  `use_rotation_motion_blur` and a slerp option for interpolating it.
  `.rdla` has a `blur(...)` construct for multi-sample attributes.
- **Deformation:** `moonray/dso/geometry/RdlMesh/attributes.cc`
  declares `vertex_list_0` and `vertex_list_1` -- two motion steps of
  `Vec3fVector`. **Corrected (built):** the names originally recorded
  here, `vertex list` and `vertex list mb`, are *aliases* of those two;
  the canonical names are the underscored ones.
- **Velocity: there is no `use velocity` flag.** A search for
  `use_velocity` across `OpenMoonRay/moonray` returns nothing.
  Velocity is `velocity_list_0` / `velocity_list_1` plus
  `velocity_scale`, and which of position, velocity and acceleration
  gets used is chosen by `motion_blur_type`, declared in
  `scene_rdl2/lib/scene/rdl2/CommonAttributes.h` with values `static`,
  `velocity`, `frame delta`, `acceleration`, `hermite` and `best`
  (the default).

So ɴsɪ `set_attribute_at_time` maps directly: on `transformationmatrix`
to `node_xform` blur samples, on `P` to `vertex_list_1`. This is the
capability Mitsuba lacks entirely.

**But rdl2 takes exactly two motion samples.** `AttributeTimestep` in
`Types.h` is `TIMESTEP_BEGIN`, `TIMESTEP_END` and nothing else, and the
`.rdla` construct is `blur(a, b)`. ɴsɪ places no such limit. A scene
with three or more samples on one attribute therefore cannot be carried
across, and the backend must report that rather than silently keeping
the first and last.

### F2: Tessellation only under displacement

`moonray/lib/rendering/geom/prim/PolyMesh.cc`:

```cpp
PolyMesh::shouldTessellate(bool enableDisplacement, ...) {
    return enableDisplacement && ... && hasDisplacementAssignment(pRdlLayer);
}
```

with `// set tessellation factor to 0 if no displacement`. Undisplaced
polygon meshes go to Embree as-is.

### F3: Analytic primitives stay analytic

Embree geometry types used across `moonray/lib`:

| Type | Count | Used for |
| --- | --- | --- |
| `RTC_GEOMETRY_TYPE_USER` | 3 | Sphere, Box, VDB volume -- custom intersection |
| `RTC_GEOMETRY_TYPE_QUAD` | 3 | quad meshes |
| `RTC_GEOMETRY_TYPE_TRIANGLE` | 2 | triangle meshes |
| curve types | 9 | round/flat/normal-oriented x linear/bspline/bezier |

Nine native curve types, no curve tessellation. Quadrics are analytic.

### F4: Subdivision at the limit surface, view-adaptive

`geom/prim/OpenSubdivMesh.cc` uses OpenSubdiv `Far::PatchTable`,
`PatchMap` and `EvaluateBasis`; `limitSurface` appears 107 times and
there is a `LimitSurfaceSample` struct. Tessellated vertices are
evaluated **on the limit surface**, not on a subdivided cage.

Refinement is view-dependent: `// only do adaptive tessellation when
adaptiveError > 0`, with `pixelsPerEdge = mAdaptiveError`, gated on
`haveViewInfo`.

It does not do analytic ray-vs-limit-patch intersection, which
essentially no production renderer does.

### F5: Progressive rendering is first-class

`moonray/lib/rendering/rndr/Types.h`:

```cpp
enum class RenderMode {
    BATCH,               // tile to completion
    PROGRESSIVE,         // samples to the GUI as soon as available
    PROGRESSIVE_FAST,    // stop path tracing, render a simplified version
    REALTIME,            // new frame every n ms, no refinement between
    PROGRESS_CHECKPOINT, // whole image at intervals
};
```

`PROGRESSIVE_FAST` has a `FastRenderMode` companion that renders normals
instead of radiance -- something on screen immediately, then converge.

**Not benchmarked.** The modes exist; parity with 3Delight's
time-to-first-pixel is unmeasured and should not be claimed.

### F6: No OSL

A code search for `OpenShadingLanguage` across `OpenMoonRay/moonray`
returns nothing. Shading is `BsdfBuilder`, `BsdfComponent`, `MapApi.h`,
`MaterialApi.h`, `EvalShader`, with ISPC-vectorised DSOs.

`BsdfBuilder` is closure-shaped lobe assembly, which would be a natural
target for OSL closures should generic OSL ever be built.

### F7: `scene_rdl2` builds without MoonRay's heavy dependencies

**Built.** Done on a four-core, 16 GB container in about fifteen
minutes; the recipe, and the three upstream problems it has to work
around, are in `quickstart.md`.

`scene_rdl2/CMakeLists.txt` requires Boost, Lua, CppUnit, OpenSSL,
JsonCpp, Log4cplus, Python and TBB. No Embree, no OpenVDB, no
OpenImageIO -- those belong to `moonray`, the renderer.

**Corrected: ISPC *is* required.** The same `CMakeLists.txt` appends
`ISPC` to `project(... LANGUAGES ...)` on every platform but Xcode, and
five library sources under `lib/common/math/ispc` and
`lib/common/fb_util/ispc` are `.ispc`. It is one distribution package
and does not drag the renderer's stack in with it, so the conclusion
holds; the original list did not.

This splits the work in two, and only the second half needs a heavy
host:

- **Scene construction** targets `scene_rdl2` alone: `SceneContext`,
  `SceneObject`, `SceneClass`, `Layer`. Buildable on a modest machine
  from ordinary distribution packages.
- **Rendering** needs full MoonRay.

It also supplies a format oracle. `scene_rdl2` ships `AsciiWriter` and
`BinaryWriter`, so a scene built through the real library can be written
out and compared against what this backend emits -- the same technique
that made the `.nsi` emitter correct, where reading 3Delight's own
output corrected four wrong assumptions rather than shipping a
plausible format.

Consequence: **the binding-strategy question is smaller than it
looked.** Building against `scene_rdl2` is cheap enough to try, so the
choice between a shim and `.rdla` generation can be settled by
experiment rather than by argument.

### F8: The `.rdla` grammar, captured rather than inferred

**Built.** `tools/oracle` writes four scenes through rdl2's own
`AsciiWriter`; the output is in `oracle/`. Four things in it would not
have survived a plausible guess:

- `Vec2` / `Vec3` / `Vec4` / `Mat4` carry **no precision suffix**. A
  `Mat4d` attribute prints `Mat4(...)`, exactly as a `Mat4f` one does.
- A null object reference is `undef()`, not `nil`.
- A bound attribute keeps its own value: `bind(Source("/s"), "pizza")`.
- Numbers print through C++'s `%g` at `max_digits10` -- nine
  significant digits for `Float`, seventeen for `Double`. `0.1f` is
  `0.100000001`, `1e20f` is `1.00000002e+20`, `-0.0f` is `-0`. Rust's
  `{}` prints the shortest round-tripping form and matches none of
  them.

`SceneVariables` is written without a name or parentheses, sets write
bare references, and a vector is `{ a, b}` -- a space after the brace,
none before the close.

**rdl2 reads back what it writes, with one exception.** Feeding each
captured scene to `AsciiReader` and writing it out again reproduces the
file byte for byte -- except negative zero, which the writer prints as
`-0` and the reader turns back into `0`. The emitter follows the
writer; `oracle/signed_zero.rdla` records the asymmetry rather than
rounding it away, and is the one capture excluded from the round-trip
check.

## Settled Questions

- **Binding strategy: generate `.rdla` first — superseded for
  interactive work.** The reasoning below held while no host could
  build MoonRay. It cannot hold for a viewport: a scene file cannot
  express "this one attribute changed", so every edit becomes a whole
  new scene and a renderer that starts from nothing. MoonRay applies
  edits without rebuilding, and at a finer grain than ɴsɪ asks for; see
  `specs/002-interactive-updates/research.md`. `.rdla` remains what it
  should have been called from the start: an **output**, for batch and
  for reading what a render was made from.

  The original reasoning, unedited: The format is small,
  now fully captured, and an emitter for it can be checked end to end
  today against real `AsciiWriter` output. A shim would buy nothing
  until a host can build the renderer, since its only advantage --
  MoonRay's progressive modes -- needs `moonray` present. The emitter
  is kept behind a document model so a `scene_rdl2` shim can be added
  as a second target rather than a rewrite. `TN.1` still needs it.
- **`Layer` wants one entry per geometry and part.**
  `AsciiWriter::writeLayer` writes a nine-column row -- geometry, part
  name, material, light set, displacement, volume shader, light filter
  set, shadow set, shadow receiver set -- keyed on the geometry and
  part pair. An ɴsɪ scene without face groups yields one row per shape
  with an empty part name.

## Open Questions

- **How a consumer is meant to depend on `nsi-intermediate`.** It is
  unpublished, and a git dependency on the `nsi` workspace makes Cargo
  fetch that repository's private `.blueprints` submodule, which fails
  without access to it -- and Cargo resolves every dependency whether
  or not the feature gating it is enabled, so making it optional does
  not help. This blocks the flush layer, not the format layer.

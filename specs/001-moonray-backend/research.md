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

### F9: Instancing is native on both sides, and this backend uses neither

Read from `moonray/dso/geometry/RdlInstancerGeometry/attributes.cc` and
`scene_rdl2/lib/scene/rdl2/Geometry.cc` on a MoonRay built here, and
from `nsi-intermediate`'s `resolve/mod.rs`.

**MoonRay instances natively**, and richly. `RdlInstancerGeometry`
declares:

| Attribute | Type | What it is |
| --- | --- | --- |
| `references` | `SceneObjectVector` | the prototypes — declared on `Geometry` itself, `INTERFACE_GEOMETRY` |
| `method` | `Int` enum | `0` = positions/orientations/scales, `2` = `xform_list` |
| `xform_list` | `Mat4dVector` | one matrix per instance |
| `positions` / `orientations` / `scales` | `Vec3f`/`Vec4f`/`Vec3f` vectors | the decomposed form |
| `ref_indices` | `IntVector` | which reference each instance draws; empty or out of range falls back to `0` |
| `disable_indices` | `IntVector` | instances to hide |
| `velocities` | `Vec3fVector` | per-instance motion blur |
| `instance_level` | `Int` enum, `0`–`4` | **nested** instancing, exposing `instance_level_N` as a shading primitive attribute |
| `use_reference_xforms` / `use_reference_attributes` | `Bool` | whether the prototype's own transform and attributes come along |

`Geometry::sReferenceGeometries` is base-class, and its comment names
this exact use: "an instancer geometry procedural can instance
primitives generated by the reference geometry procedural."

**`nsi-intermediate` already resolves ɴsɪ's side**, and did before this
was asked for:

- `Scene::instance_sources(instancer)` — prototypes ordered by their
  `index` attribute, which is what ɴsɪ matches `modelindices` against,
  *not* connection order.
- `Scene::instance_transforms(instancer)` / `instance_transforms_at(t)`
  — `Vec<Instance { source: usize, transform: [f64; 16] }>`, honouring
  `modelindices` and `disabledinstances` including when those are
  motion-sampled.
- `Scene::relative_transform(prototype, instancer)` — the prototype's
  own chain up to the instancer, which is the transform
  `use_reference_xforms` is about.
- `ResolveError::Instanced` — `world_transform` *refuses* for a
  prototype rather than answering with the instancer's own matrix,
  which would put every instance at one place.

`Instance`'s own doc comment says why it exists: "so a backend building
a MoonRay `InstanceGeometry` or a Mitsuba `shapegroup` reference does
not have to."

**And this backend calls none of it.** `flush.rs` handles the *refusal*
— it reports `Instanced` and leaves the prototype where it is — but
never asks the question that would succeed. So an ɴsɪ `instances` node
contributes nothing: a crowd of a thousand renders as **one prototype
at the origin**, reported but wrong.

The mapping is close to one-to-one and needs no invention:

| ɴsɪ, resolved upstream | `RdlInstancerGeometry` |
| --- | --- |
| `instance_sources` | `references` |
| `Instance::transform` | `xform_list`, with `method` = `2` |
| `Instance::source` | `ref_indices` |
| `relative_transform(prototype, instancer)` | the prototype's own `node_xform`, with `use_reference_xforms` |
| a disabled instance | omitted upstream, so nothing to emit |

Nothing here argues for expanding instances into separate objects.
Both sides model instancing directly; flattening would throw away the
memory win that is the entire point, and MoonRay's `instance_level`
says it handles nesting too. `T6.1`.

### F10: A moving instancer cannot use `xform_list`

`RdlInstancerGeometry` declares

```cpp
sceneClass.declareAttribute<Mat4dVector>("xform_list");
```

with **no flags**, and `FLAGS_BLURRABLE` is what makes an attribute
carry two timesteps (`Types.h:285`). So ɴsɪ's sampled
`transformationmatrices` -- which 3Delight renders, and which
`Scene::instance_transforms_at` exists to serve -- cannot cross as a
`blur()` pair. The flush takes the shutter-open sample and reports the
reduction.

The route MoonRay intends is `velocities`, a `Vec3fVector` of one
vector per instance. `InstanceProceduralLeaf.cc:344` applies it as

```cpp
positions[i] + velocities[i] * dt0
```

with

```cpp
dt0 = (motionSteps[0] - evaluationFrame) / fps;
```

so **velocities are in units per second**, and ɴsɪ's motion times are
in whatever unit the scene's shutter uses -- which this crate has no
way to confirm from here.

**The conversion does not have to guess, but `fps` does not cancel.**
An earlier draft of this note claimed it did; working the arithmetic
through says otherwise. With `motion_steps` set to the shutter's two
ends and `evaluation_frame` to the first:

```
dt0 = (open  - open) / fps = 0
dt1 = (close - open) / fps
```

and MoonRay computes `position + velocity * dt`. For the second
timestep to land on the second sample:

```
p0 + velocity * dt1 = p0 + delta
velocity = delta * fps / (close - open)
```

So `fps` is in it. That is fine, because `fps` is a `SceneVariable`
this backend writes, so the two agree by construction rather than by
assumption -- but it has to be written rather than left at rdl2's
default, or the conversion depends on a number nobody stated.

`motion_steps` written from the scene is `T2.0` and is done.

**Only translation.** `velocities` is a position offset; an instance
that rotates or scales across the shutter needs the decomposed form
(`method` 0, with `orientations` and `use_rotation_motion_blur`).
Reporting that is part of the task.

### F11: 3Delight itself, as the oracle for the ɴsɪ side

Everything above reads MoonRay. Four tasks were parked because they
needed the *other* side -- what ɴsɪ means -- and reading it off a
plausible-looking name is the failure `T1.3a` refuses. 3Delight
2.9.209 for Linux is a free download; it ships `doc/nsi.pdf` (the
specification), `renderdl`, `oslc`, and 178 compiled ɴsɪ shaders. That
turns three of the four from opinion into measurement and the fourth
into a table.

The probes are `tools/probe/`, run against a 3Delight unpacked
anywhere; `tools/probe/README.md` says how.

#### `fov` is vertical -- measured, not inferred

The specification says only "the field of view angle, in degrees" for
`perspectivecamera`. Two hints point at vertical:
`depthoffield.focallength` is documented as the *vertical* focal
length, and the default screen window is `[-f, -1] .. [f, 1]` for
`f = xres / yres`, whose vertical extent is fixed while the horizontal
grows with aspect. `cylindricalcamera` says "vertical" outright.

Hints are not a measurement, so:

```
quad, half-extent 1, at z = -1     camera at the origin, fov = 90
resolution 400 x 200               (aspect 2, so vertical and
                                    horizontal cannot be confused)
```

At `fov = 90` the visible half-extent at distance 1 is 1 along
whichever axis the angle names. 3Delight lit:

```
x: 100..299   (200 of 400 columns, centred)
y:   0..199   (all 200 rows)
```

The quad fills the **height** exactly and half the width. `fov` is
**vertical**. Had it been horizontal the quad would have filled the
width and overflowed the height.

`focal()` already read it as vertical, from how
`nsi_toolbelt::look_at_bounding_box_perspective_camera` uses it. That
reading is now confirmed rather than borrowed, and
`inprocess::the_frame_matches_3delights_framing` renders the same
probe through MoonRay and asserts the same lit rectangle.

#### ɴsɪ has no velocity attribute for meshes

`T2.4` was parked on "which ɴsɪ attribute carries velocity". The
answer is that none does. The specification defines velocity only on
the two OpenVDB nodes:

| Node | Attributes |
| --- | --- |
| `volume` | `velocitygrid` (a grid *name* inside the `.vdb`), `velocityscale`, `velocityreferencetime` |
| `vdbparticles` | the grid's own `v` attribute, `velocityscale`, `velocityreferencetime` |

`mesh`, `particles` and `curves` have none. Their motion is
`NSISetAttributeAtTime` on `P`, which is `T2.3` and is done. The
`mesh` node's `referencetime`, whose text mentions "velocity blur", is
the reference time *for that VDB machinery*; on a mesh it has nothing
to point at.

Measured rather than read: the same quad rendered six times with a
one-frame shutter, once per candidate attribute name.

| What was set | Lit columns |
| --- | --- |
| nothing | 167..232 |
| `P` at `t=0` and `t=1` | **218..313** |
| `velocity` | 167..232 |
| `v` | 167..232 |
| `V` | 167..232 |
| `vel` | 167..232 |
| `motion` | 167..232 |

Two position samples smear. Every velocity name is ignored silently.
So MoonRay's `velocity_list_0` / `velocity_list_1` have no ɴsɪ input to
carry, and `T2.4` is closed as not applicable rather than unfinished.
The `velocity()` function in `flush.rs` is unaffected -- that one
converts a *transform* delta for `RdlInstancerGeometry`, which is
MoonRay's own requirement (`F10`), not an ɴsɪ attribute.

#### Shader parameter names belong to the shader, and the shaders are enumerable

`T1.3a` is right that there is no ɴsɪ-level naming: an ɴsɪ shader node
carries a `shaderfilename` and whatever parameters that OSL shader
declares. What was missing is that the shaders in practical use are a
short, readable list -- 3Delight ships them compiled, and `.oso` is a
text format, so the parameter names can be read rather than guessed:

```
grep -a '^param' dlPrincipled.oso
```

| Shader | Base colour | Roughness | Metallic | IOR | Opacity | Emission |
| --- | --- | --- | --- | --- | --- | --- |
| `dlPrincipled` | `i_color` | `roughness` | `metallic` | `refract_ior` | `opacity` | `incandescence` |
| `dlStandard` | `base_color` | `specular_roughness` | `metalness` | `specular_IOR` | `opacity` | `emission_color` |
| `openPBRSurface` | `baseColor` | `specularRoughness` | `baseMetalness` | `specularIOR` | `geometryOpacity` | `emissionColor` |
| `dlMetal` | `i_color` | `roughness` | — (always 1) | — | `opacity` | — |
| `dlGlass` | `i_color` | `refract_roughness` | — | `refract_ior` | — | `incandescence` |
| `dlPrelit` | `i_color` | — | — | — | — | `i_incandescence` |
| `UsdPreviewSurface` | `diffuseColor` | `roughness` | `metallic` | `ior` | `opacity` | `emissiveColor` |

This is a table, not a heuristic. A shader not in it still carries the
six `UsdPreviewSurface` names by exact match, and everything else is
reported by name.

#### An area light is geometry whose shader emits, and nothing else

Section 4.5 is unambiguous: "There are no special light source nodes
in ɴsɪ ... Any scene geometry can become a light source if its surface
shader produces an `emission()` closure." An area light is a mesh
wearing an emitter; a spot light is "an epsilon sized geometry (a
small disk, a particle, etc.)" wearing a shader that shapes the
emission with a cone angle; a directional light is an `environment`
node with `angle` 0.

So recognising one means knowing what the shader does, and MoonRay
runs no OSL (`F6`). There is no attribute to read. What *is* readable
is the shader's name and its parameters, which is the table above: a
shader that is one of the known emitters, or that carries a known
emissive parameter set to something other than black, is emissive.
Anything else is reported rather than guessed at -- a mesh silently
promoted to a light is worse than a mesh that stays a mesh and says
so.

### F12: `MeshLight` needs a shader that is not in `moonray`

An ɴsɪ area light is a mesh wearing an emitter, and MoonRay's
`MeshLight` is the structural equivalent: it takes a `geometry`
pointing at the mesh and lights from its surface. Two things about it
were captured rather than assumed, both from
`rndr/RenderContext.cc:316` (`createMeshLightLayer`).

**The geometry must not be in the render layer.** MoonRay builds a
`Layer` of its own for a mesh light's geometry, and refuses geometry
that is already in the main one:

> We cannot load in a geometry that already exists in the main scene
> layer ... `rdlLight->warn(...)`; `continue;`

So the flush emits the mesh, leaves it out of both the `Layer` and the
`GeometrySet`, and reports the consequence: in ɴsɪ an emissive mesh is
*also* visible to camera rays, and here it is not.

**And it hard-codes a shader `moonray` does not ship.** The same
function does:

```cpp
mSceneContext->createSceneObject("DwaBaseMaterial",
        geom->getName() + "_MeshLightMaterial")
```

`DwaBaseMaterial` lives in `moonshine_dwa`, not in `moonray` or
`scene_rdl2`. On the build here -- `scene_rdl2` plus `moonray`, 68
DSOs -- rendering any scene containing a `MeshLight` fails in render
prep:

```
Error: Couldn't find DSO for 'DwaBaseMaterial' in search path ...
```

`startFrame` then returns `CANCELLED` rather than `FINISHED`, which
this backend reports as a frame that did not start. A full OpenMoonRay
install has `moonshine_dwa` and does not hit this; a minimal one does,
and nothing in the scene says why.

A `MeshLight` is not, however, invisible: `MeshLight::intersect`
ray-traces the *real mesh* through an Embree scene of its own, and
`Scene::updateActiveLights` puts a bounded light into the
camera-visible set when `visible_in_camera` says so. So one object is
both seen and sampled, which is what ɴsɪ means by an emissive mesh
being ordinary geometry, and the flush forces that flag on.
`specs/003-osl/research.md` O3 has the reading.

That is why `T1.7a`'s render test uses a `pointLight` rather than an
`areaLight`. The two differ only in which row of `LIGHTS` matches, so
the recognition rule -- the thing the task was actually blocked on --
is tested end to end either way; `SphereLight`, `SpotLight` and
`DistantLight` have no such dependency. The `MeshLight` mapping itself
is asserted as a document, which is where the two rules above live.

### F13: How MoonRay uses OpenSubdiv, and what replacing it would take

Asked because `subdiv-kernels` exists and a C API could be added to it.
The answer is that MoonRay's use is narrow but *deep*: one file, one
OpenSubdiv layer, and four load-bearing capabilities.

#### The surface actually used

All of it is in `geom/prim/OpenSubdivMesh.cc`. Three other files only
`#include` its header. Nothing uses `Osd::` -- there is no GPU
subdivision and no OpenSubdiv drawing anywhere in MoonRay. The whole
dependency is `Sdc` (options) and `Far` (CPU refinement and patches):

| Called | For |
| --- | --- |
| `Far::TopologyRefinerFactory<TopologyDescriptor>::Create` | the cage, with per-edge crease and per-vertex corner sharpness and the face-varying channels |
| `Sdc::Options` -- `VtxBoundaryInterpolation`, `FVarLinearInterpolation` | rdl2's boundary and face-varying rules, mapped one for one |
| `TopologyRefiner::RefineAdaptive(AdaptiveOptions)` | Catmull-Clark. `maxDepth` and `secondaryLevel` come from the **camera**: `log2(maxEdgeResolution) + 1`, with creases forcing depth 6 |
| `TopologyRefiner::RefineUniform(UniformOptions(1))` | bilinear and Loop, plus a documented OpenSubdiv-3.1 face-varying workaround |
| `Far::PatchTableFactory::Create` with `ENDCAP_GREGORY_BASIS` | one patch per limit-surface region, irregular ones included |
| `Far::PrimvarRefiner::{Interpolate, InterpolateVarying, InterpolateFaceVarying}` | patch control points, per level, per motion sample |
| `PatchTable::ComputeLocalPointValues{,Varying,FaceVarying}` | the end-cap patches' own extra points |
| `Far::PatchMap::FindPatch(faceId, u, v)` | the inner loop: which patch covers this sample |
| `PatchTable::EvaluateBasis{,Varying,FaceVarying}` | weights **and first derivatives** at that `(u, v)` |

The last two are what the renderer is really buying: **arbitrary
`(u, v)` on the limit surface**, returning position, `dPdu`, `dPdv`,
normal, `st` and every primitive attribute, uniformly for regular and
irregular patches alike. Tessellation is view-adaptive, so the sample
pattern is decided per edge from the camera and then evaluated exactly
-- which is `F2` and `F4` in this document, and the reason this backend
does not tessellate anything itself.

#### Where `subdiv-kernels` already lines up

More than one might expect. It has Catmull-Clark with the same crease
and corner inputs (`Mesh::edge_creases`, `Mesh::vertex_corners`),
face-varying channels with the interpolation modes, stencil tables and
a composed cage-to-final table, limit stencils with tangents, a
`PatchTable` of regular bicubic B-spline patches with an explicit
exactness contract, and a `LimitEvaluator` that answers arbitrary
`(u, v)` **with derivatives** on any quad -- feature quads by recursive
local isolation rather than an eigenbasis. It also has three schemes
OpenSubdiv does not (Loop it shares; √3 and Doo-Sabin it does not),
sparse-edit queries (`affected_outputs`), and a wgpu path.

#### What is missing, and it is not the C API

1. **Feature-adaptive refinement.** The request type is
   `UniformRefine`. MoonRay's depth is per-edge and camera-derived,
   and uniform refinement to the depth a crease forces (6) is a very
   different memory profile on a production cage.
2. **No Gregory end caps.** `subdiv-kernels` classifies irregular
   quads as `QuadClass::Feature` and sends them to `LimitEvaluator`;
   MoonRay expects every patch to answer the same `EvaluateBasis`
   call. The capability is there, the *shape* is not, and the cost
   model differs -- a recursion per sample against a table lookup.
3. **No patch map.** `FindPatch(faceId, u, v)` is MoonRay's inner
   loop; `subdiv-kernels` indexes by refined quad.
4. **Ptex face numbering.** `generateSubdQuadTopology` deliberately
   mirrors `Far::PtexIndices::initializePtexIndices` so MoonRay's own
   quad ids agree with the patch table's face ids. A replacement has
   to agree on that numbering or the entire tessellated index buffer
   points at the wrong faces -- and renders something plausible.

So a C API is necessary and nowhere near sufficient.

#### Two honest routes, and neither is a swap

- **Emulate `Far`.** Implement the nine entry points above behind
  `extern "C"`, including adaptive refinement, Gregory end caps, a
  patch map and ptex numbering. That is most of what OpenSubdiv's
  `Far` *is*, and the exactness bar is a renderer's.
- **Rewrite `OpenSubdivMesh.cc` to the shape `subdiv-kernels` offers**
  -- stencils plus `LimitEvaluator`, isolating only where samples
  land. Arguably the better architecture, since the isolation is
  demand-driven where the adaptive refinement is speculative. But it
  is a change to MoonRay, not a library swap, and it lands in the file
  that decides what every subdivision surface looks like.

Neither is on this backend's path. Nothing here tessellates: ɴsɪ
subdivision crosses as `is_subd` plus creases and corners (`T3.1`), and
MoonRay decides the rest. A third option -- subdividing on this side
and handing MoonRay a polygon mesh -- would work today and is exactly
what `F4` says not to do, because it throws away the view-adaptive
tessellation that made MoonRay worth linking.

### F14: The flush now costs more than the scene it flushes

Upstream interns handles behind `ustr_handles` and quotes its own
numbers. `tools/footprint` measures the same thing on the shape this
backend actually sees -- long hierarchical handles, two nodes per
shape, two connections each -- and, unlike upstream's benchmark,
**flushes at the end**, which turns out to be where the story is.

50 000 shapes: 100 001 ɴsɪ nodes, 50 005 rdl2 objects.

| | scene | per node | build | flush | per object |
| --- | --- | --- | --- | --- | --- |
| as upstream ships | 144.6 MB | 1516 B | 35.3 s | +109.8 MB | 2303 B |
| `interned` | 103.1 MB | 1081 B | 11.3 s | +109.1 MB | 2288 B |

**29% smaller and 3.1x faster to record.** The speed is not a side
effect of the size: `edges_to_attribute` had to build two `String`s to
probe its key on every call, and interned it probes with a pair of
`u64`s. Flushing takes half a second either way, so all 24 seconds of
the difference is scene recording. `interned_handles` is on by default
here on the strength of that, with one caveat worth stating: `ustr`'s
table is global and never freed, so a host running many *different*
scenes in one process accumulates their handles. A renderer re-renders
the same ones.

The second row of that table is about **this** crate, and it is the
uncomfortable one. The flushed document is now *larger than the scene
it came from* -- 109 MB against 103 MB, 2288 bytes an object -- and it
did not shrink at all when the scene did. `Document` copies every
handle into `String`s it owns:

- `Object::name`, once per object;
- `Reference`, which holds a `String` class **and** a `String` name,
  and a `Layer` row holds up to nine of them;
- `Value::Object` and `Value::Objects` inside attributes.

So a scene whose handles upstream just stopped duplicating gets them
duplicated again on the way out, five or six times over for a shape
with a material and a light set. The document outlives the flush -- it
is what `apply_affected` diffs against between frames -- so this is
resident for the life of an interactive session, not a transient.

Two ways out, neither taken yet:

- **Borrow.** `Document<'a>` with `&'a str` names, tied to the scene it
  was flushed from. Cheapest in bytes, and it makes the lifetime
  relationship explicit -- but `Session` holds the previous document
  across an edit to the scene that produced it, which is exactly the
  borrow that cannot outlive its source. It would have to clone at the
  point it is stored, which is most of them.
- **Intern.** The same `ustr` table upstream already populates: a
  `Reference`'s class is one of about twenty strings and its name is a
  handle the scene has already interned, so both are free lookups.
  `Value` and `Object` keep owning their names and nothing about the
  API changes.

Interning is the one that fits, and it was done: [`Name`](../../src/name.rs).
A `Ustr` with `interned_handles` and a `Box<str>` without, carrying
`Object::class`, `Object::name`, both halves of a `Reference`, every
attribute name and a `Layer` row's part. `Value::String` is
deliberately **not** one: file paths and channel names are neither
short nor repeated, and interning them would put unbounded,
never-freed strings in a global table.

Measured the same way, on the same scene:

| | scene | record | flush | per object | total |
| --- | --- | --- | --- | --- | --- |
| before `Name`, interned | 103.1 MB | 11.3 s | +109.1 MB | 2288 B | 212.3 MB |
| after, `Box<str>` | 144.7 MB | 32.6 s | +100.1 MB | 2098 B | 244.8 MB |
| after, `Ustr` | 103.1 MB | 11.7 s | **+71.2 MB** | **1493 B** | **174.3 MB** |

The document is **35% smaller** and back to costing less than the
scene it came from. `Box<str>` alone -- what a build without the
feature gets -- takes 9% off on its own, from the sixteen bytes and
exact-size allocation against a `String`'s twenty-four and its spare
capacity.

Two things fell out of doing it that are worth keeping.

**`Borrow<str>` is a promise about hashing.** `Ustr`'s own `Hash` is a
precomputed hash of the pointer, so a derived `Hash` on the wrapper
made `HashMap<Name, _>::get("quad")` answer `None` for a key that was
there -- silently, since `Borrow` makes it compile. `Name` hashes its
*text*, which is also what makes the two feature configurations behave
identically rather than merely compile alike. A test covers it.

**`Debug` had to be hand-written** to print a quoted string as `String`
does: handles reach users through `{handle:?}` in the limitation
messages this backend reports, and a derived one would have put
`Name("…")` in every message it writes.

The oracle tests are what say the bytes did not move: they rebuild
four scenes by hand and assert this crate writes exactly what rdl2's
`AsciiWriter` wrote. They pass unchanged.

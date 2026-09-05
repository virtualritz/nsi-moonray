# Research: MoonRay Backend

Findings are from reading `OpenMoonRay/moonray` and
`OpenMoonRay/scene_rdl2` at a shallow clone taken 2026-09-05, not from
documentation.

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
- **Deformation:** `moonray/dso/geometry/RdlMesh` has a **`vertex list
  mb`** attribute -- a second motion step for vertices -- and a **`use
  velocity`** flag that takes a velocity list instead.

So ɴsɪ `set_attribute_at_time` maps directly: on `transformationmatrix`
to `node_xform` blur samples, on `P` to `vertex list mb`. This is the
capability Mitsuba lacks entirely.

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

## Open Questions

- **Binding strategy.** Mitsuba's answer was a hand-written `extern "C"`
  shim, forced by its templates. `scene_rdl2` is not template-heavy in
  the same way, so a shim may be easier -- but `.rdla` is a scripted
  authoring path that might be a cheaper first target. Decide before
  planning tasks.
- **Whether `Layer` wants one entry per shape or per assignment group.**
  Affects how `geometry_binding` results are consumed.

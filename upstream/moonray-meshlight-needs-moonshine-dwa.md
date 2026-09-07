<!--
Ready to file at https://github.com/OpenMoonRay/moonray/issues/new

Title: MeshLight hard-codes DwaBaseMaterial, so it cannot render on a
       moonray-only build and the error names neither

Not filed from here: this session's GitHub access is scoped to
`virtualritz`, and the MoonRay repository is on another tier.

Everything below was read from the source at `eef67ae` and reproduced by
running it -- see `specs/001-moonray-backend/research.md` F12.
-->

# `MeshLight` hard-codes `DwaBaseMaterial`

## Summary

`RenderContext::createMeshLightLayer` creates a `DwaBaseMaterial` for
every `MeshLight`'s geometry. `DwaBaseMaterial` is not part of
`moonray`: it ships with `moonshine_dwa`. On a build of `scene_rdl2` +
`moonray` alone, any scene containing a `MeshLight` fails in render prep
with

```
Error: Couldn't find DSO for 'DwaBaseMaterial' in search path '…'.
```

`startFrame()` then returns `RP_RESULT::CANCELLED` rather than
`FINISHED`, and a host embedding `RenderContext` sees a frame that did
not start, with no indication that a light was the reason.

This is not a crash and not wrong output -- it is a **dependency from
`moonray` on a package outside it, taken at render time and only when a
particular light class is present**, which makes it invisible until a
scene happens to use one.

## Where

`lib/rendering/rndr/RenderContext.cc:360-368` (at `eef67ae`):

```cpp
if (geom && geom->updateRequired()) {
    // Material contains "map shader" so that it can grab requested primitive attributes
    scene_rdl2::rdl2::Material *material = mSceneContext->
        createSceneObject("DwaBaseMaterial",
                geom->getName() + "_MeshLightMaterial")->asA<scene_rdl2::rdl2::Material>();
    scene_rdl2::rdl2::SceneObject* mapShader= rdlLight->get<scene_rdl2::rdl2::SceneObject*>("map_shader");
    material->beginUpdate();
    material->setBinding("albedo", mapShader);
    material->endUpdate();
```

The comment says why a material is needed at all: it carries the
`map_shader` binding so the primitive-attribute request reaches the
geometry. Nothing about that needs *this* material in particular.

## Reproducing

Any scene with a `MeshLight` whose `geometry` is set, rendered against
a `moonray` build without `moonshine_dwa` on the DSO path. Minimal:

```lua
SceneVariables {
    ["camera"] = PerspectiveCamera("cam"),
    ["layer"] = Layer("/layer"),
    ["image_width"] = 64,
    ["image_height"] = 48,
}

PerspectiveCamera("cam") {}

RdlMeshGeometry("quad") {
    ["face_vertex_count"] = { 4},
    ["vertices_by_index"] = { 0, 1, 2, 3},
    ["vertex_list_0"] = { Vec3(-1, -1, -5), Vec3(1, -1, -5), Vec3(1, 1, -5), Vec3(-1, 1, -5)},
}

RdlMeshGeometry("lamp") {
    ["face_vertex_count"] = { 4},
    ["vertices_by_index"] = { 0, 1, 2, 3},
    ["vertex_list_0"] = { Vec3(3, -2, -6), Vec3(3, -2, -2), Vec3(3, 2, -2), Vec3(3, 2, -6)},
}

MeshLight("lamp/light") {
    ["geometry"] = RdlMeshGeometry("lamp"),
}

LightSet("/lights") { MeshLight("lamp/light"), }
UsdPreviewSurface("/material") {}

Layer("/layer") {
    {RdlMeshGeometry("quad"), "", UsdPreviewSurface("/material"), LightSet("/lights"), undef(), undef(), undef(), undef(), undef()},
}
```

```
$ moonray -in meshlight.rdla -out out.exr
Loading Scene File(s): meshlight.rdla
Starting render prep...Error: Couldn't find DSO for 'DwaBaseMaterial' in search path '…'.
```

Note that `lamp` is deliberately **not** in the `Layer`:
`createMeshLightLayer` refuses geometry that already is, which is
correct and documented in the code. So this is the supported shape of a
mesh light, not a misuse.

## Why it matters outside DreamWorks

`MeshLight` is the natural target for any front end whose light model is
"geometry that emits" -- ɴsɪ's is exactly that (specification 4.5: there
are no light nodes, geometry becomes a light when its surface shader
produces an `emission()` closure), and USD's `MeshLightAPI` is the same
idea. A backend mapping either onto MoonRay reaches `MeshLight` first
and finds it unavailable on a build that has every other light class.

## Suggested fixes, in order of preference

1. **Use a material `moonray` owns.** The material exists only to carry
   the `albedo` binding; any `Material` with a bindable colour input
   would do. If none in `moonray` qualifies, a minimal internal one
   would -- it is never shaded through the main layer.
2. **Fall back.** Try `DwaBaseMaterial`, and on failure fall back to
   whatever is available, warning once.
3. **Fail with the reason.** At minimum, catch the missing class and
   say "`MeshLight(...)` needs `DwaBaseMaterial`, which is in
   `moonshine_dwa`" rather than letting render prep cancel with a
   DSO-path error naming a class the scene never mentions.

Any of the three turns a silent capability gap into something a user
can act on.

## Workaround here

`nsi-moonray` maps ɴsɪ's `areaLight` and the specification's own
`emitter` shader to `MeshLight`, and maps `pointLight`, `spotLight`,
`distantLight` and `directionalLight` to `SphereLight`, `SpotLight` and
`DistantLight`, which have no such dependency. The `MeshLight` mapping
is asserted against the emitted scene rather than a render, because a
render of it is not available on this build. `research.md` F12.

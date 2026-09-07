<!--
Ready to file at https://github.com/OpenMoonRay/moonray/issues/new

Title: MeshLight with any map_shader segfaults in render prep

Not filed from here: this session's GitHub access is scoped to
`virtualritz`, and the MoonRay repository is on another tier.

Read from the source at `eef67ae` and reproduced by running it.
-->

# `MeshLight` with any `map_shader` segfaults

## Summary

Giving a `MeshLight` a `map_shader` — the documented way to make its
radiance vary over its surface — segfaults during render prep, before
the first pixel:

```
SIGSEGV(segfault) callstack
 QuadMesh::getQuadAttributes<Vec2f>(...)
  QuadMesh::getQuadST(...)
   QuadMesh::getST(...)
    MeshLight::sampleMapShader(...)
     MeshLight::setMesh(geom::internal::Primitive*)
      PrimitiveGroup::forEachPrimitive(...)
       Scene::generateMeshLightGeometry(...)
        Scene::populateLightList(...)
         Scene::updateLightList(...)
          Scene::preFrame(...)
           RenderContext::renderPrep(bool, bool)
```

The same light **without** a `map_shader` renders correctly.

## Reproducer

Stock classes only — `RdlMeshGeometry`, `MeshLight`,
`CheckerboardMap`, `UsdPreviewSurface`:

```lua
SceneVariables { ["camera"] = PerspectiveCamera("/cam"), ["image_width"] = 200, ["image_height"] = 150 }
PerspectiveCamera("/cam") { ["focal"] = 14, ["node_xform"] = translate(0, 1, 6) * rotate(-12, 1, 0, 0) }

RdlMeshGeometry("/lamp") {
    ["vertex_list_0"] = { Vec3(-0.5,1.5,0), Vec3(0.5,1.5,0), Vec3(0.5,1.5,1), Vec3(-0.5,1.5,1) },
    ["vertices_by_index"] = { 0,1,2,3 }, ["face_vertex_count"] = { 4 }, ["is_subd"] = false,
}
RdlMeshGeometry("/floor") {
    ["vertex_list_0"] = { Vec3(-4,-1,-4), Vec3(4,-1,-4), Vec3(4,-1,4), Vec3(-4,-1,4) },
    ["vertices_by_index"] = { 0,1,2,3 }, ["face_vertex_count"] = { 4 }, ["is_subd"] = false,
}
UsdPreviewSurface("/mat") { ["diffuseColor"] = Rgb(0.7,0.7,0.7) }

CheckerboardMap("/checker") { }
MeshLight("/lamp_l") {
    ["geometry"] = RdlMeshGeometry("/lamp"),
    ["map_shader"] = CheckerboardMap("/checker"),   -- remove this line and it renders
    ["color"] = Rgb(1,0.9,0.7), ["intensity"] = 20, ["normalized"] = false,
}

GeometrySet("/set") { RdlMeshGeometry("/floor") }
LightSet("/lights") { MeshLight("/lamp_l") }
Layer("/layer") { { RdlMeshGeometry("/floor"), "", UsdPreviewSurface("/mat"), LightSet("/lights") } }
```

Note that this needs a `DwaBaseMaterial` on the DSO path, because
`RenderContext::createMeshLightLayer` hard-codes one — see
`moonray-meshlight-needs-moonshine-dwa.md`, which has to be worked
around before this bug is even reachable on a `moonray`-only build.

## What was ruled out

Each of these was tried on its own and the crash is unchanged:

- **The map.** First found with a third-party `Map`; reproduces
  identically with MoonRay's own `CheckerboardMap`.
- **Execution mode.** Crashes in both `-exec_mode scalar` and the
  default vectorized mode.
- **Mesh topology.** Crashes on a quad mesh (`QuadMesh::getQuadST`)
  and on the same mesh triangulated (`TriMesh::getTriangleST`).
- **The mesh having primitive attributes at all.** Adding a `uv_list`
  to the light's geometry does not help.
- **The mesh-light material requesting texture coordinates.** Putting
  `StandardAttributes::sSt`, `sSurfaceST` and `sUv` in the material's
  `mRequiredAttributes` does not help.

## Where

`lib/rendering/geom/prim/QuadMesh.cc:1217` (at `eef67ae`):

```cpp
const Attributes* attributes = getAttributes();
if (attributes->isSupported(key)) {
```

`NamedPrimitive::getAttributes()` returns `mAttributes.get()`, a
`unique_ptr` "generally set by the procedural at primitive construction
time". There is no null check here, and `MeshLight::setMesh` reaches
this through `sampleMapShader` -> `getST` while walking the mesh light
layer's primitives to compute per-face energies.

`lib/rendering/pbr/core/Scene.cc:177` is the other half of the picture:

```cpp
const shading::AttributeTable* table = nullptr;
if (material->hasExtension()) {
    table = material->get<shading::RootShader>().getAttributeTable();
}
meshLight->setAttributeTable(table);
```

`table` is explicitly allowed to be null here, and
`MeshLight::sampleMapShader` passes it straight into
`Intersection::initMapEvaluation` at `MeshLight.cc:1877`.

## Why it matters

`map_shader` is the only way a mesh light's emission can vary across
its own surface. Without it a mesh light is one colour times one
intensity, which rules out every emissive material whose brightness is
textured or procedural — exactly the case the attribute exists for.

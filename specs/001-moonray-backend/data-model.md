# Data Model: MoonRay Backend

## Entities

### `SceneContext`

MoonRay's container, equivalent to an ɴsɪ context. Owns every
`SceneObject`.

### `SceneObject` / `SceneClass`

A `SceneObject` is a named instance of a `SceneClass`. A `SceneClass`
**declares its attributes with types**, which ɴsɪ does not -- ɴsɪ
attributes are untyped at the interface and validated by the renderer.

That difference is useful: a mistyped ɴsɪ attribute can be caught at
flush time, against the declared `SceneClass`, rather than producing a
silently ignored parameter.

### `Layer`

The geometry-to-material assignment table. This is where a dissolved ɴsɪ
`attributes` node lands, and it is a closer fit than Mitsuba's per-shape
`bsdf` field.

**One row per geometry and part**, nine columns wide, in the order
`AsciiWriter::writeLayer` writes them:

```
{geometry, "part", material, lightSet, displacement, volumeShader,
 lightFilterSet, shadowSet, shadowReceiverSet}
```

Unassigned columns print as `undef()`. An ɴsɪ scene without face groups
yields one row per shape with an empty part name.

## Type Mapping

| ɴsɪ `Type` | `scene_rdl2` |
| --- | --- |
| `F32`, `F64` | `Float` / `Double` |
| `I32`, `I64` | `Int` / `Long` |
| `String` | `String` |
| `Color` | `Rgb` |
| `Point`, `Vector`, `Normal` | `Vec3f` |
| `MatrixF32`, `MatrixF64` | `Mat4f` / `Mat4d` |
| `Reference` | **none -- never crosses** |

**Confirmed** against `scene_rdl2/lib/scene/rdl2/Types.h` (T0.4). The
typedefs are `Bool`, `Int` (`int32_t`), `Long` (`int64_t`), `Float`,
`Double`, `String`, `Rgb` (`math::Color`), `Rgba`, `Vec2f`/`Vec2d`,
`Vec3f`/`Vec3d`, `Vec4f`/`Vec4d`, `Mat3f`/`Mat3d`, `Mat4f`/`Mat4d`,
`SceneObject*`, and a `*Vector` of each. `BoolVector` is a `std::deque`
-- "`std::vector<bool>` is evil", says the header.

`.rdla` prints all four `Vec`/`Mat` pairs **without a precision
suffix**: a `Mat4d` attribute writes `Mat4(...)`. See `research.md` F8.

## Node Mapping

The class is `RdlMeshGeometry`, whose DSO source is
`moonray/dso/geometry/RdlMesh`.

| ɴsɪ node | MoonRay |
| --- | --- |
| `mesh` | `RdlMeshGeometry` with `is_subd` **false** |
| `subdivisionmesh` | `RdlMeshGeometry` with `is_subd` true, limit-evaluated |
| `transform` | resolved into `node_xform` by `nsi-intermediate` |
| `attributes` | dissolved into a `Layer` assignment |
| `shader` | `Material` / `Map` / `NormalMap` |
| `perspectivecamera` | `Camera` |
| `outputlayer` | `RenderOutput` |
| `outputdriver` | `RenderOutput` file or callback |
| `environment` | `EnvMap` |
| `instances` | `GeometrySet` + instancing |
| `nurbs` | none; tessellate first |

`is_subd` **defaults to true**, so an ɴsɪ `mesh` has to set it false
explicitly or it renders as a subdivision surface. Read from
`moonray/dso/geometry/RdlMesh/attributes.cc`.

Topology is `face_vertex_count` and `vertices_by_index`, both
`IntVector`; positions are `vertex_list_0`, a `Vec3fVector`.

## Motion Samples

The one place this backend does more than the Mitsuba one.

| ɴsɪ | MoonRay |
| --- | --- |
| `set_attribute_at_time` on `transformationmatrix` | `node_xform` with `blur(a, b)` |
| `set_attribute_at_time` on `P` | `vertex_list_0` and `vertex_list_1` |
| a velocity attribute | `velocity_list_0`, with `motion_blur_type` `velocity` |

**Two samples, and no more.** rdl2's `AttributeTimestep` is
`TIMESTEP_BEGIN` and `TIMESTEP_END`; `.rdla` writes `blur(a, b)`. ɴsɪ
has no such limit, so a scene with three or more samples on one
attribute must be reported as reduced, never quietly flattened.

The attribute names above are the canonical ones. `vertex list mb`,
which earlier drafts of these specs used, is an alias of
`vertex_list_1`, and the `use velocity` flag they mention **does not
exist** -- see `research.md` F1.

`nsi-intermediate` currently resolves static transforms only; motion
resolution is its open task, and **this backend is the consumer that
justifies it**.

## Migrations

None yet.

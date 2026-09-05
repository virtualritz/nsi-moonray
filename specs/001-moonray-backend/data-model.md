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

Exact `scene_rdl2` type names to be confirmed against `Types.h` when the
binding strategy is settled.

## Node Mapping

| ɴsɪ node | MoonRay |
| --- | --- |
| `mesh` | `RdlMesh` geometry |
| `subdivisionmesh` | `RdlMesh` with subdivision, limit-evaluated |
| `transform` | resolved into `node_xform` by `nsi-intermediate` |
| `attributes` | dissolved into a `Layer` assignment |
| `shader` | `Material` / `Map` / `NormalMap` |
| `perspectivecamera` | `Camera` |
| `outputlayer` | `RenderOutput` |
| `outputdriver` | `RenderOutput` file or callback |
| `environment` | `EnvMap` |
| `instances` | `GeometrySet` + instancing |
| `nurbs` | none; tessellate first |

## Motion Samples

The one place this backend does more than the Mitsuba one.

| ɴsɪ | MoonRay |
| --- | --- |
| `set_attribute_at_time` on `transformationmatrix` | `node_xform` with `blur(...)` samples |
| `set_attribute_at_time` on `P` | `RdlMesh` **`vertex list mb`** |
| a velocity attribute | `RdlMesh` **`use velocity`** + velocity list |

`nsi-intermediate` currently resolves static transforms only; motion
resolution is its open task, and **this backend is the consumer that
justifies it**.

## Migrations

None yet.

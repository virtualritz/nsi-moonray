# `nsi-moonray`

An [ɴsɪ](https://nsi.readthedocs.io/) backend on
[MoonRay](https://github.com/OpenMoonRay/moonray), DreamWorks
Animation's production renderer.

**Status: spec only.** No crate yet — the binding strategy is undecided,
and that decision changes what the crate looks like. See
[`specs/`](specs/).

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

- **Motion blur, both kinds.** `node_xform` takes blur samples;
  `RdlMesh` has `vertex list mb` for deformation and a velocity path.
  Mitsuba 3 dropped `AnimatedTransform` and cannot blur at all.
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
ɴsɪ calls → nsi-intermediate → nsi-moonray → scene_rdl2 → MoonRay
                            ↘ nsi-mitsuba → Properties → Mitsuba 3
```

Consumers may alias the dependency:

```rust
use nsi_intermediate as nsi_ir;
```

## Documentation

Spec-driven; see [`specs/`](specs/). Shared standards come from
`.blueprints`, a private submodule — a plain `git clone` works, and only
`--recurse-submodules` fails, on that one path.

## Licence

MIT OR Apache-2.0 OR Zlib.

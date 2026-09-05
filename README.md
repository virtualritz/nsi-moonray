# `nsi-moonray`

An [ɴsɪ](https://nsi.readthedocs.io/) backend on
[MoonRay](https://github.com/OpenMoonRay/moonray), DreamWorks
Animation's production renderer.

**Status: it emits scenes, it does not render them yet.** A recorded
ɴsɪ scene flushes into `.rdla`, MoonRay's ASCII scene format — mesh
geometry with its world transform, a camera, a `Layer`, a
`GeometrySet` and render outputs. Every byte of that format was
captured from `scene_rdl2`'s own `AsciiWriter` rather than inferred,
and rdl2 reads back what is written; see
[`specs/001-moonray-backend/oracle/`](specs/001-moonray-backend/oracle/).

Materials are substituted rather than translated: MoonRay runs no OSL,
so every ɴsɪ shader becomes a `UsdPreviewSurface` — stock MoonRay's PBR
surface — at its defaults, and the flush reports the substitution.

Nothing has been rendered yet. **Building** MoonRay is heavy;
**running** it is not, and the two are not the same problem:

```bash
mrr scene.rdla -o image.exr     # runs `moonray -in … -out …`
mrr scene.rdla --print          # the command, without running it
```

The other delivery shape is a drop-in: `nsi-ffi-wrap` loads a renderer
by `dlopen` and looks up eleven C entry points, so a `cdylib` exporting
those over this flush would let an existing ɴsɪ consumer load MoonRay
exactly where it loads 3Delight today. That is `T4.2`.

## Building

`nsi-intermediate` is overlaid from a sibling checkout for now, since
it is unpublished:

```bash
git clone https://github.com/virtualritz/nsi.git      # ../nsi
git clone https://github.com/virtualritz/nsi-moonray.git
cd nsi-moonray && cargo test
```

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
  `RdlMeshGeometry` has `vertex_list_1` for deformation and a velocity
  path. Mitsuba 3 dropped `AnimatedTransform` and cannot blur at all.
  Two samples, though: rdl2 has exactly two timesteps.
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

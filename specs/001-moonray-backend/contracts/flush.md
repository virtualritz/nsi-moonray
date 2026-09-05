# Contract: Flushing A Recorded Scene Into MoonRay

## Scope

Covers turning a `nsi_intermediate::Scene` into `scene_rdl2` objects.
Depends on `nsi-intermediate` having resolved graph semantics already.

## Matrix

| Behavior | Status | Source Evidence | Test/QA Evidence | Required Next Evidence |
| --- | --- | --- | --- | --- |
| MoonRay builds on a capable host | Open | None | None | Build `OpenMoonRay/openmoonray`; record the recipe. Docker is the documented path. |
| A binding strategy is chosen | Open | None | None | Decide between an `extern "C"` shim over `scene_rdl2` and generating `.rdla`. See `research.md` Open Questions. **Nothing else can start until this is settled.** |
| Geometry flushes with its world transform | Open | None | None | Flush a translated mesh; assert it renders where the transform puts it. |
| **Two materials land on the right two shapes** | Open | None | None | Two shapes, two materials via `Layer`, assert each is correct. Inherited top risk: a misclassified connection does not error, it renders wrongly. |
| Transform motion blur | Open | None | None | Two time samples on a `transform`; assert the result differs from the static render and blurs along the path. Blocked on `nsi-intermediate` motion resolution. |
| Deformation motion blur | Open | None | None | Two time samples on `P`; assert `vertex list mb` is populated and the render blurs. |
| Subdivision reaches the limit surface | Open | None | None | Render a cube as a subdivision mesh; assert the silhouette is the limit surface, not the cage. |
| Analytic primitives are not tessellated | Open | None | None | Flush an ɴsɪ sphere; assert it becomes an analytic primitive rather than a mesh. |
| Render outputs map to `RenderOutput` | Open | None | None | Flush `render_outputs()`; assert one `RenderOutput` per layer. |
| An unmapped shader fails loudly | Open | None | None | ɴsɪ always returns an image, so this must warn and substitute rather than abort -- but it must not silently render untextured. |

## Invariants

- No ɴsɪ graph semantics live here. Composition, dissolution and output
  resolution all happen in `nsi-intermediate`.
- **ɴsɪ always returns an image.** A limitation is reported, never
  raised as a refusal. This is the interface's philosophy and a farm
  depends on it.
- Analytic stays analytic. MoonRay tessellates only under displacement;
  this backend must not pre-tessellate what MoonRay would keep.

## Failure Modes

- **Unmapped node type or shader:** warn, substitute, still render.
- **Motion samples present but unresolved upstream:** must not silently
  flatten. Report that the render is sharp.

## Required Evidence Before Marking Complete

- A build host, and a settled binding strategy.
- The two-material test specifically. Do not mark this complete because
  a scene merely rendered.

# Contract: Flushing A Recorded Scene Into MoonRay

## Scope

Covers turning a `nsi_intermediate::Scene` into `scene_rdl2` objects.
Depends on `nsi-intermediate` having resolved graph semantics already.

## Matrix

| Behavior | Status | Source Evidence | Test/QA Evidence | Required Next Evidence |
| --- | --- | --- | --- | --- |
| MoonRay builds on a capable host | Open | None | None | Build `OpenMoonRay/openmoonray`; record the recipe. Docker is the documented path. |
| A binding strategy is chosen | **Covered** | `research.md` Settled Questions; the oracle was captured against a built `scene_rdl2`, which is what made the choice an experiment rather than an argument | `tools/oracle` builds and runs; recipe in `quickstart.md` | -- |
| The `.rdla` grammar is known, not guessed | **Covered** | `oracle/*.rdla`, written by rdl2's own `AsciiWriter`; `research.md` F8 | `tests/oracle.rs` -- four tests rebuild those scenes and assert byte equality | -- |
| rdl2 reads back what this crate writes | **Covered** | `AsciiReader` accepts each captured scene and `AsciiWriter` reproduces it byte for byte | `oracle verify` over `types`, `scene`, `blur`, `binding`; `tests/oracle.rs` shows this crate emits those same bytes | -- |
| Negative zero survives | **Partial** | rdl2's writer prints `-0`; its reader returns `0` | `oracle/signed_zero.rdla` plus `tests/oracle.rs::signed_zero` -- the emitter matches the writer | Upstream asymmetry. Nothing to fix here unless a scene turns out to depend on the sign of zero. |
| A `Layer` row's shape | **Covered** | `AsciiWriter::writeLayer` -- nine columns keyed on geometry and part | `document.rs` `a_layer_row_has_nine_columns` | -- |
| Geometry flushes with its world transform | Open | None | None | Flush a translated mesh; assert it renders where the transform puts it. |
| **Two materials land on the right two shapes** | Open | None | None | Two shapes, two materials via `Layer`, assert each is correct. Inherited top risk: a misclassified connection does not error, it renders wrongly. |
| Transform motion blur | Open | None | None | Two time samples on a `transform`; assert the result differs from the static render and blurs along the path. Blocked on `nsi-intermediate` motion resolution. |
| Deformation motion blur | Open | None | None | Two time samples on `P`; assert `vertex_list_1` is populated and the render blurs. `vertex list mb` is an alias of it. |
| More than two motion samples are reported, not flattened | Open | `Types.h` -- `AttributeTimestep` is `TIMESTEP_BEGIN` and `TIMESTEP_END`, nothing else | None | Flush three samples on one attribute; assert the reduction is reported. rdl2 cannot carry them. |
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

- A build host for the rendering rows. `scene_rdl2` alone is enough for
  everything above the render.
- The two-material test specifically. Do not mark this complete because
  a scene merely rendered.

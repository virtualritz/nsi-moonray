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
| Geometry flushes with its world transform | **Covered** | `flush.rs` -- `Scene::world_transform` into `node_xform` | `render::a_transform_moves_the_shape` renders a quad with and without a translation and reads both frames; `flush::tests::geometry_carries_its_world_transform` covers the matrix without a renderer | -- |
| An ɴsɪ mesh is not silently subdivided | **Covered** | `RdlMesh/attributes.cc` -- `is_subd` defaults to **true** | `flush::tests::a_mesh_says_it_is_not_a_subdivision_surface` | -- |
| The camera's field of view | **Partial** | `PerspectiveCamera::computeProjectionMatrix` -- the vertical half-angle is `atan(halfFilmWidth * height / width / focal)` | `flush::tests::fov_becomes_a_focal_length` | ɴsɪ's `fov` is read as vertical, from how `nsi-toolbelt` uses it. Confirm against a 3Delight render. `T1.6` |
| **Two materials land on the right two shapes** | **Covered** | `flush.rs` -- one `Layer` row per geometry, its material from `Scene::geometry_binding` | `render::two_materials_land_on_the_right_two_shapes` renders a red and a green quad and asserts per channel which half is which. Reading the file would not catch a swap that renders. | -- |
| Transform motion blur | **Covered** | `flush.rs` -- `motion_times` and `world_transform_interpolated_at` into rdl2's two-sample `blur(a, b)` | `flush::tests::a_moving_transform_blurs` for the emission; `render::a_moving_shape_renders_blurred` counts the partially covered columns a smear leaves and a sharp edge does not | -- |
| Deformation motion blur | Open | None | None | Two time samples on `P`; assert `vertex_list_1` is populated and the render blurs. `vertex list mb` is an alias of it. |
| More than two motion samples are reported, not flattened | **Covered** | `Types.h` -- `AttributeTimestep` is `TIMESTEP_BEGIN` and `TIMESTEP_END`, nothing else | `flush::tests::more_than_two_motion_samples_are_reported` | -- |
| Subdivision reaches the limit surface | **Partial** | `flush.rs` -- `subdivision.scheme` on a `mesh` sets `is_subd`, with creases and corners carried across | `flush::tests::subdivision_scheme_on_a_mesh_makes_it_a_subdivision_surface` and `creases_and_corners_cross`; a `polyhedron-ops` polyhedron rendered at four crease hardnesses visibly rounds | Assert the silhouette against the limit surface rather than by eye. |
| **An `instances` node is instanced, not expanded** | **Covered** | `research.md` F9 -- MoonRay's `RdlInstancerGeometry` (`references`, `xform_list`, `ref_indices`, `instance_level` to 4) and upstream's `instance_sources` / `instance_transforms` / `Instance` meet almost one to one | `flush::tests::a_prototype_is_referenced_once_not_expanded` counts the prototype's declarations, since a flattened scene renders an identical image; `inprocess::an_instanced_scene_renders_its_copies` renders two copies with a dark gap between them; `a_prototypes_own_transform_is_applied_once` measures where they land, which is the only way to tell applied-once from dropped or doubled | Motion (`T6.3`) and nesting past four levels (`T6.4`). |
| Analytic primitives are not tessellated | Open | None | None | Flush an ɴsɪ sphere; assert it becomes an analytic primitive rather than a mesh. |
| Render outputs map to `RenderOutput` | **Partial** | `flush.rs` -- one per output layer, with `channel_name` and the first driver's `file_name` | `flush::tests::a_triangle_becomes_a_mesh_a_camera_and_an_output` | A layer fanning out to several drivers, and a render that writes the file. |
| An unmapped shader fails loudly | **Partial** | `flush.rs` -- every ɴsɪ shader is unmapped, since MoonRay has no OSL (`research.md` F6), so each becomes a `UsdPreviewSurface` at its defaults and the `Layer` row points at that | `flush::tests::a_bound_shader_becomes_the_default_surface` -- the substitution happens, the scene emits, and the limitation is reported | Carrying the shader's parameters into the substitute. `T1.3a` |
| An ɴsɪ consumer can load this as its renderer | **Partial** | `capi.rs` -- the twelve symbols `nsi-ffi-wrap` resolves, over `Scene` and the flush | `tests/dropin.rs` `dlopen`s the built `cdylib`, records a triangle through the C entry points and asserts the written `.rdla` | An application actually rendering through it. Its display driver's callbacks are reached -- see [`display.md`](display.md) -- but with one bucket at the end rather than as the render converges. `T5.3` |
| A scene reaches MoonRay | **Covered** | `render.rs` -- `moonray -in scene.rdla -out image.exr`, flags read from `RenderOptions.cc` | `tests/render.rs` renders a flushed triangle through a MoonRay built from source and asserts the image is not black; `render::tests` cover the argument list without a renderer | -- |
| **A shape disconnected from `.root` stops rendering** | **Covered** | `flush.rs` — the walk visits every recorded node, not only reachable ones, so a detached shape is emitted with all nine `visible_*` attributes false. Turned off rather than left out, because omitting it makes an interactive disconnect a change of set and layer *membership*, which forces a full re-apply (`002` `research.md` F3) | `flush::tests::a_detached_shape_is_not_rendered` and `a_connected_shape_keeps_its_visibility`; `incremental::disconnecting_a_shape_turns_it_off_without_a_rebuild` renders it | The first flush hands MoonRay geometry it will never draw. `T7.1`. |
| Geometry with no ɴsɪ shader still renders | **Covered** | MoonRay skips a `Layer` row whose material column is `undef()` -- the same triangle is absent without a material and present with one | `flush::tests::unbound_geometry_gets_a_row_and_a_default_material`, and the render above | -- |
| Motion samples present but unresolved are reported | **Covered** | `flush.rs` -- upstream resolves static transforms only | `flush::tests::motion_samples_are_reported` | -- |

| An ɴsɪ `environment` lights the scene | **Partial** | `flush.rs` -- `EnvLight` in a `LightSet` that every `Layer` row references; `Light.cc` declares `color`, `intensity` and `texture`, all left at defaults | `flush::tests::an_environment_lights_the_scene` | A render. And the environment's texture, which lives in an OSL shader MoonRay cannot run. |
| A scene with no light says so | **Covered** | `flush.rs` -- a scene whose `Layer` rows have no light set renders black | `flush::tests::a_triangle_becomes_a_mesh_a_camera_and_an_output` asserts the limitation is reported | -- |

**Area lights are not mapped.** In ɴsɪ they are geometry wearing an
emissive shader, and recognising one means reading a shader MoonRay
cannot run. `T1.7a`.

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

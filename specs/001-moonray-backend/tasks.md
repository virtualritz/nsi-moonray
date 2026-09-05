# Tasks: MoonRay Backend

Nothing started. Ordered by risk retired.

## Blocking

- [ ] T0.1 Build MoonRay on a capable host; record the recipe.
- [ ] T0.2 **Choose the binding strategy** -- `extern "C"` shim over
      `scene_rdl2`, or generate `.rdla`. Nothing below can be scoped
      until this is settled. See `research.md` Open Questions.

## User Story 1: Render A Recorded Scene (P1)

- [ ] T1.1 Minimal path from a `nsi_intermediate::Scene` to an image.
- [ ] T1.2 Geometry with its world transform.
- [ ] T1.3 Materials through `Layer`.
- [ ] T1.4 **Two shapes, two materials, each correct.** Inherited top
      risk; nothing earlier catches a misbinding.
- [ ] T1.5 `render_outputs()` to `RenderOutput`.

## User Story 2: Motion Blur (P1)

The capability that distinguishes this backend.

- [ ] T2.1 Depends on `nsi-intermediate` resolving motion samples.
      **This backend is why that task exists.**
- [ ] T2.2 Transform motion to `node_xform` blur samples.
- [ ] T2.3 Deformation motion to `RdlMesh` `vertex list mb`.
- [ ] T2.4 Velocity-based motion via `use velocity`, if ɴsɪ carries a
      velocity attribute.

## User Story 3: Subdivision (P2)

- [ ] T3.1 ɴsɪ `subdivisionmesh` to a subdivided `RdlMesh`.
- [ ] T3.2 Confirm limit-surface evaluation and view-adaptive
      tessellation are reached, not bypassed.

## Not Now

- [ ] TN.1 Progressive rendering. MoonRay has `PROGRESSIVE`,
      `PROGRESSIVE_FAST` and `REALTIME`; reaching them requires the
      shim, not `.rdla`.
- [ ] TN.2 OSL. MoonRay has none. Shared work, separate surface.

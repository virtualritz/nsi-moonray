# Tasks: MoonRay Backend

Nothing started. Ordered by risk retired.

## Startable Now

`scene_rdl2` builds without MoonRay's heavy dependencies -- Boost, Lua,
CppUnit, OpenSSL, JsonCpp, Log4cplus, Python, TBB, and nothing from
Embree/OpenVDB/OpenImageIO/ISPC. See `research.md` F7. So scene
construction can begin before the renderer can be built at all.

- [ ] T0.1 Build `scene_rdl2` alone; record the recipe.
- [ ] T0.2 Write a scene by hand through `scene_rdl2` and dump it with
      `AsciiWriter`. **This is the format oracle**, and it is what makes
      the emitter correct rather than plausible -- reading 3Delight's
      real output corrected four wrong assumptions in the `.nsi` case.
- [ ] T0.3 **Settle the binding strategy by experiment**, now that
      building against `scene_rdl2` is cheap: an `extern "C"` shim, or
      generating `.rdla`. See `research.md` Open Questions.
- [ ] T0.4 Confirm the `scene_rdl2` type names in `data-model.md`
      against its `Types.h`; they are currently marked unconfirmed.

## Blocked On A Heavy Host

- [ ] T0.9 Build full MoonRay (Embree, OpenVDB, OpenImageIO, ISPC;
      Docker is the documented path). Needed only to *render*, not to
      construct a scene.

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

# Tasks: MoonRay Backend

Phase 0 is done: `scene_rdl2` builds, the `.rdla` format is captured
from it, and the emitter reproduces that capture byte for byte.
Ordered by risk retired.

## Startable Now

`scene_rdl2` builds without MoonRay's heavy dependencies -- Boost, Lua,
CppUnit, OpenSSL, JsonCpp, Log4cplus, Python, TBB and ISPC, and nothing
from Embree/OpenVDB/OpenImageIO. (ISPC is required after all; see
`research.md` F7.) So scene construction begins before the renderer can
be built at all, and has.

- [x] T0.1 Build `scene_rdl2` alone; record the recipe. Done; recipe
      and its three workarounds in `quickstart.md`.
- [x] T0.2 Write a scene by hand through `scene_rdl2` and dump it with
      `AsciiWriter`. Done: `tools/oracle`, output in `oracle/`. It
      corrected four assumptions, exactly as reading 3Delight's output
      did for `.nsi` -- see `research.md` F8.
- [x] T0.3 **Settle the binding strategy by experiment.** Decided:
      generate `.rdla` first, behind a document model that leaves room
      for a `scene_rdl2` shim as a second target. `research.md` Settled
      Questions.
- [x] T0.4 Confirm the `scene_rdl2` type names in `data-model.md`
      against its `Types.h`. Done, and the `Vec`/`Mat` suffix rule
      corrected with them.
- [ ] T0.5 Round-trip the emitted `.rdla` back through rdl2's
      `AsciiReader` and out through `AsciiWriter`, and diff. Byte
      equality against a captured file proves the syntax; only a
      round-trip proves rdl2 *accepts* what we write.
- [ ] T0.6 An authoring twin of `RdlMeshGeometry`: a DSO built from
      moonray's own `attributes.cc` with a stub implementation, so a
      real mesh scene can be built and read back without the renderer.
      Nothing else lets `T1.*` be checked on a modest host.
- [ ] T0.7 **Settle how to depend on `nsi-intermediate`.** It is
      unpublished and a git dependency drags in a private submodule, so
      the flush layer cannot be built from a clean checkout. Upstream
      question; blocks every `T1.*` and `T2.*` task below.

## Blocked On A Heavy Host

- [ ] T0.9 Build full MoonRay (Embree, OpenVDB, OpenImageIO, ISPC;
      Docker is the documented path). Needed only to *render*, not to
      construct a scene.

## User Story 1: Render A Recorded Scene (P1)

Every task here needs `T0.7` first.

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
- [ ] T2.3 Deformation motion to `RdlMeshGeometry`'s
      `vertex_list_1`.
- [ ] T2.4 Velocity-based motion via `velocity_list_0` and
      `motion_blur_type`, if ɴsɪ carries a velocity attribute. There is
      no `use velocity` flag; an earlier draft of these specs was wrong
      about that.
- [ ] T2.5 Report, never flatten, a scene with more than two motion
      samples on one attribute. rdl2 has exactly two timesteps.

## User Story 3: Subdivision (P2)

- [ ] T3.1 ɴsɪ `subdivisionmesh` to a `RdlMeshGeometry` with
      `is_subd` true -- and an ɴsɪ `mesh` to one with it explicitly
      false, since it defaults to true.
- [ ] T3.2 Confirm limit-surface evaluation and view-adaptive
      tessellation are reached, not bypassed.

## Not Now

- [ ] TN.1 Progressive rendering. MoonRay has `PROGRESSIVE`,
      `PROGRESSIVE_FAST` and `REALTIME`; reaching them requires the
      shim, not `.rdla`.
- [ ] TN.2 OSL. MoonRay has none. Shared work, separate surface.

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
- [x] T0.5 Round-trip the emitted `.rdla` back through rdl2's
      `AsciiReader` and out through `AsciiWriter`, and diff. Done:
      `oracle verify`. All four scenes round-trip; negative zero does
      not, which is upstream's asymmetry and is captured separately.
- [ ] T0.6 An authoring twin of `RdlMeshGeometry`: a DSO built from
      moonray's own `attributes.cc` with a stub implementation, so a
      real mesh scene can be built and read back without the renderer.
      Nothing else lets `T1.*` be checked on a modest host.
- [~] T0.7 **Settle how to depend on `nsi-intermediate`.** Worked
      around, not settled: a path dependency on a sibling `nsi`
      checkout. `[patch]` was tried first and does not help -- Cargo
      fetches the patched git source anyway. Publishing the crate, or
      making `.blueprints` non-blocking, is what would actually settle
      it.

## Delivery

How a consumer actually gets a MoonRay render out of an ɴsɪ scene.
Neither of these needs *this* repository to build MoonRay -- they need
whoever renders to have it installed.

- [x] T4.1 `mrr`, a CLI that hands a scene to MoonRay's own binary:
      `moonray -in scene.rdla -out image.exr`. Flags read from
      `RenderOptions.cc`.
- [~] T4.2 **`libnsi_moonray.so`: a drop-in ɴsɪ renderer.** Built and
      loadable: `src/capi.rs` exports all twelve symbols, records into
      `nsi_intermediate::Scene`, and on `NSIRenderControl "start"`
      writes the `.rdla` and runs MoonRay. `tests/dropin.rs` `dlopen`s
      the artefact and drives a scene through it. Still batch, so
      display drivers get nothing (`T4.4`) and `NSIEvaluate` is a
      no-op (`T4.3`).
      `nsi-ffi-wrap` loads a renderer through `dlopen` and looks up
      eleven C entry points -- `NSIBegin`, `NSIEnd`, `NSICreate`,
      `NSIDelete`, `NSISetAttribute`, `NSISetAttributeAtTime`,
      `NSIDeleteAttribute`, `NSIConnect`, `NSIDisconnect`,
      `NSIEvaluate`, `NSIRenderControl`. A `cdylib` exporting those over
      `nsi_intermediate::Recorder` and this crate's flush is what lets
      an existing ɴsɪ consumer load MoonRay where it loads 3Delight
      today, with no change to the consumer beyond which library it
      resolves. `DspyRegisterDriver` counts as a twelfth: `nsi-ffi-wrap`
      resolves the whole symbol table up front, so a consumer built
      with the `output` feature cannot load a library missing it.
- [ ] T4.3 `.nsi` stream input. There is no parser for the format:
      `nsi-intermediate` writes streams and does not read them, and
      `nsi-stream` is the pixel-streaming driver, not a reader. The
      parser belongs upstream next to the writer, where the Mitsuba
      backend gets it too.
- [ ] T4.4 Link `libmoonray` rather than spawning its CLI. A spawned
      batch render cannot stream samples back, so this is what
      progressive rendering and the pixel-streaming driver both need.
      See `TN.1`.

## Needs MoonRay Installed

- [ ] T0.9 Build full MoonRay (Embree, OpenVDB, OpenImageIO, ISPC;
      Docker is the documented path) *somewhere*, so a render can
      actually be checked. Nothing in this repository needs it to
      construct or verify a scene -- only to see one rendered.

## User Story 1: Render A Recorded Scene (P1)

- [~] T1.1 Minimal path from a `nsi_intermediate::Scene` to an image.
      The scene flushes -- mesh, camera, layer, geometry set, render
      output -- and nothing has rendered it. Needs `T0.9`.
- [~] T1.2 Geometry with its world transform. Emitted into
      `node_xform` and unit-tested; unrendered.
- [~] T1.3 Materials through `Layer`. Every ɴsɪ shader becomes a
      `UsdPreviewSurface` at its defaults -- stock MoonRay's PBR
      surface -- and the row points at it. MoonRay runs no OSL, so the
      shader itself cannot be translated; the substitution is reported.
- [ ] T1.3a Carry what parameters can be carried into the substitute
      surface: `diffuseColor`, `metallic`, `roughness`, `ior`,
      `opacity`, `emissiveColor`. Which ɴsɪ shader parameters map onto
      those depends on the shader, so this needs real scenes, not a
      guessed name table.
- [ ] T1.4 **Two shapes, two materials, each correct.** Inherited top
      risk; nothing earlier catches a misbinding.
- [~] T1.5 `render_outputs()` to `RenderOutput`. One per output layer,
      carrying `channel_name` and the first driver's `file_name`; a
      layer fanning out to several drivers is not handled.
- [ ] T1.6 Confirm ɴsɪ's `fov` is vertical, and the focal length
      derived from it. Read as vertical because
      `nsi_toolbelt::look_at_bounding_box_perspective_camera` treats it
      that way; unverified against a 3Delight render.

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

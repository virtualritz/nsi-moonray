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
      the artefact and drives a scene through it. A display driver's
      callbacks are called (`T5.1`), but with one bucket at the end
      rather than as the render converges (`T5.3`), and `NSIEvaluate`
      is a no-op (`T4.3`).
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
- [~] T4.3 `.nsi` stream input. **The parser exists upstream now**:
      `nsi-parse`, which drives `nsi_trait::Nsi` rather than producing
      a scene type of its own -- so it feeds a `Recorder`, and through
      it this backend, with nothing to write here but the wiring.
- [ ] T4.4 Link `libmoonray` rather than spawning its CLI. **Moved to
      [`002`](../002-interactive-updates/tasks.md)**, which is where the
      reason for it lives. A spawned
      batch render cannot stream samples back, so this is what
      progressive rendering and the pixel-streaming driver both need.
      See `TN.1`.

## Getting Pixels Out

Contract: [`contracts/display.md`](contracts/display.md). MoonRay has
no display-driver interface and an ɴsɪ consumer expects one;
`nsi-ffi-wrap`'s `output` feature is where the two meet, and it is
Rust on both sides, so **no ndspy marshalling is involved**.

- [x] T5.1 **Deliver pixels to an application's closures.** An
      `outputdriver` carries `callback.open`, `callback.write` and
      `callback.finish` as `Reference` attributes; `display.rs` reads
      them and calls them. `capi.rs` was dropping every `Type::Reference`
      before this, so a driver that asked for closures got a perfect
      render and an empty viewport, with no error anywhere.
      `render::an_applications_callback_receives_the_rendered_pixels`
      asserts the closure receives the pixels.
- [ ] T5.2 The `dlopen` route. A `Box<dyn FnWrite>` is a trait object
      whose vtable belongs to the compilation that made it, so `T5.1`
      holds only where the application and this backend share one
      `nsi-ffi-wrap`. A separately built `cdylib` needs the `extern "C"`
      entry points `DspyRegisterDriver` hands over -- already the
      twelfth symbol this crate exports, so the mechanism is present
      and only the delivery path is missing.
- [ ] T5.3 **Progressive delivery.** Today one bucket covering the
      frame, read back off the file MoonRay wrote: the application sees
      a finished render rather than a converging one, and a closure
      returning `Error::Stop` is ignored because there is nothing left
      to stop. **Not a MoonRay limitation** -- it renders progressively
      already (`research.md` F5), it just wants to be *pulled*
      (`snapshotDelta` + `ActivePixels`) where ɴsɪ pushes. The adapter
      is a snapshot loop on this side; what it needs is a
      `RenderContext` to snapshot, which a spawned batch binary does
      not have. So this waits on
      [`002`](../002-interactive-updates/tasks.md) `R1`-`R3` and on
      nothing upstream.

## Needs MoonRay Installed

- [x] T0.9 Build full MoonRay *somewhere*, so a render can actually be
      checked. Done from source on the same four cores, in about fifty
      minutes; recipe and its five workarounds in `quickstart.md`. It
      rendered a flushed triangle, which is what turned two silent
      black-image bugs into fixed ones.

## User Story 1: Render A Recorded Scene (P1)

- [x] T1.1 Minimal path from a `nsi_intermediate::Scene` to an image.
      A flushed triangle renders. `tests/render.rs`, skipped where
      there is no MoonRay.
- [x] T1.2 Geometry with its world transform. A translated quad
      renders where the transform puts it: the centre of frame is
      covered before the translation and empty after it, the left is
      the reverse. `tests/render.rs`.
- [~] T1.3 Materials through `Layer`. Every ɴsɪ shader becomes a
      `UsdPreviewSurface` at its defaults -- stock MoonRay's PBR
      surface -- and the row points at it. MoonRay runs no OSL, so the
      shader itself cannot be translated; the substitution is reported.
- [~] T1.3a Carry what parameters can be carried into the substitute
      surface. Six are, **by exact name only** -- `diffuseColor`,
      `emissiveColor`, `roughness`, `metallic`, `ior`, `opacity` -- and
      every other parameter on the shader is reported by name rather
      than dropped quietly. Mapping by anything looser is guesswork: an
      ɴsɪ shader is an OSL shader and its parameter names are its
      author's, so a wrong guess renders plausibly and silently.
- [x] T1.4 **Two shapes, two materials, each correct.** Inherited top
      risk, and now checked by reading pixels rather than the file: two
      quads, red left and green right, asserted per channel.
      `tests/render.rs`.
- [~] T1.5 `render_outputs()` to `RenderOutput`. One per output layer,
      carrying `channel_name` and the first driver's `file_name`; a
      layer fanning out to several drivers is not handled.
- [~] T1.7 **Lights.** `environment` becomes an `EnvLight` at its
      defaults, collected into a `LightSet` that every `Layer` row
      points at -- a row with no light set is lit by nothing. A scene
      with no light at all is reported, because a correct scene
      rendering black otherwise looks like a bug here.
- [ ] T1.7a Area lights. In ɴsɪ they are geometry wearing an emissive
      shader, and spotting one means reading a shader MoonRay cannot
      run. MoonRay has `RectLight`, `DiskLight`, `SphereLight`,
      `DistantLight`, `SpotLight` and `CylinderLight` waiting; what is
      missing is a rule for recognising them that is not a guess.
- [ ] T1.6 Confirm ɴsɪ's `fov` is vertical, and the focal length
      derived from it. Read as vertical because
      `nsi_toolbelt::look_at_bounding_box_perspective_camera` treats it
      that way; unverified against a 3Delight render.

## User Story 2: Motion Blur (P1)

The capability that distinguishes this backend.

- [x] T2.1 Depends on `nsi-intermediate` resolving motion samples.
      Done upstream: `motion_times`, `world_transform_samples` and
      `world_transform_interpolated_at`, which interpolates
      element-wise and holds the ends, as 3Delight does.
- [x] T2.2 Transform motion to `node_xform` blur samples. A moving quad
      renders blurred; `tests/render.rs` counts the partially covered
      columns a smear leaves and a sharp edge does not.
- [ ] T2.3 Deformation motion to `RdlMeshGeometry`'s
      `vertex_list_1`.
- [ ] T2.4 Velocity-based motion via `velocity_list_0` and
      `motion_blur_type`, if ɴsɪ carries a velocity attribute. There is
      no `use velocity` flag; an earlier draft of these specs was wrong
      about that.
- [ ] T2.5 Report, never flatten, a scene with more than two motion
      samples on one attribute. rdl2 has exactly two timesteps.

## User Story 3: Subdivision (P2)

- [~] T3.1 ɴsɪ subdivision to a `RdlMeshGeometry` with `is_subd` true.
      **ɴsɪ marks this with an attribute, not a node type**: a `mesh`
      carrying `subdivision.scheme`. Keying off the node type alone --
      which this did until a real subdivision surface was rendered --
      renders the faceted cage instead, and looks like a perfectly good
      render of the wrong thing. Creases and corners cross too
      (`subdivision.crease*` / `corner*` to `subd_crease_*` /
      `subd_corner_*`), as does `clockwisewinding` to `orientation`.
- [ ] T3.2 Confirm limit-surface evaluation and view-adaptive
      tessellation are reached, not bypassed.

## User Story 4: Instancing (P1)

**Neither side needs convincing** (`research.md` F9): MoonRay has
`RdlInstancerGeometry` with `references` / `xform_list` / `ref_indices`
and nesting to five levels, and `nsi-intermediate` already resolves
ɴsɪ's `instances` node into exactly that shape — its `Instance` type
says so in its own doc. This backend calls none of it.

- [ ] T6.1 **Map `instances` to `RdlInstancerGeometry`.** Today an ɴsɪ
      `instances` node contributes *nothing*: `flush.rs` handles
      `ResolveError::Instanced` by reporting it and leaving the
      prototype at its own transform, so a crowd of a thousand renders
      as one prototype at the origin. Reported, and wrong.
      `instance_sources` to `references`, `Instance::transform` to
      `xform_list` with `method` = `2`, `Instance::source` to
      `ref_indices`.
- [ ] T6.2 A prototype's own transform. `relative_transform(prototype,
      instancer)` is the chain ɴsɪ composes below the instancer, and
      `use_reference_xforms` is what decides whether MoonRay applies
      the reference geometry's own. Settle which by rendering, not by
      reading: getting it wrong double-applies or drops a transform,
      and both look plausible.
- [ ] T6.3 A moving instancer. `instance_transforms_at(t)` exists
      because 3Delight renders a sampled `transformationmatrices`;
      rdl2 has two timesteps, so this is `T2.5`'s reduction rule again.
      `velocities` is the other route and is per-instance.
- [ ] T6.4 Nested instancing. MoonRay's `instance_level` goes to `4`
      and ɴsɪ nests `instances` under `instances`. Confirm the depth
      maps, and report past four rather than flattening.
- [ ] T6.5 **Assert it is instanced, not expanded.** A test that only
      checks the image passes on a flattened scene, which is the whole
      thing this avoids. Count `SceneObject`s, or read the memory.

## Not Now

- [ ] TN.1 Progressive rendering. MoonRay has `PROGRESSIVE`,
      `PROGRESSIVE_FAST` and `REALTIME`; reaching them requires the
      shim, not `.rdla`.
- [ ] TN.2 OSL. MoonRay has none. Shared work, separate surface.

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
- [x] T5.3 **Progressive delivery.** `src/stream.rs`: a snapshot loop
      paced by `areCoarsePassesComplete` and `isFrameComplete`, giving
      each snapshot to `callback.write` and honouring a closure that
      answers `Error::Stop` -- which the file stopgap could not, since
      by then there was nothing left to stop. Six buckets for a frame
      converging in a third of a second, on the machine this was
      written on.
      It was never a MoonRay limitation: it renders progressively
      already (`research.md` F5) and wants to be *pulled* where ɴsɪ
      pushes. The only blocker was spawning, since a separate process
      has no `RenderContext` to snapshot.
- [ ] T5.3a **A bucket is the whole frame, not a tile.**
      `snapshotRenderBuffer` untiles, so the rectangle that is actually
      *new* is not something the loop can name -- and naming a
      sub-rectangle it has not verified would be a lie an application
      would draw. `snapshotDelta` with its `ActivePixels` is what makes
      a real sub-rectangle possible, and it is worth having for a large
      frame, or for one crossing a network.

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
- [x] T2.3 Deformation motion to `RdlMeshGeometry`'s
      `vertex_list_1`. rdl2 carries this as **two attributes**, not as
      a `blur()` pair -- that form is for scalars and matrices.
      More than two samples takes the **ends**, so the extent of the
      motion survives; keeping the first two would shorten every blur
      in the scene and read as a shutter setting. A changing vertex
      count is not deformation and cannot be interpolated: the first
      sample is used and it is reported.
      `flush::tests::a_deforming_mesh_gets_two_vertex_lists` and three
      siblings; `inprocess::a_deforming_mesh_renders_blurred` counts
      the partially covered columns a smear leaves and a sharp edge
      does not.
- [ ] T2.4 Velocity-based motion via `velocity_list_0` and
      `motion_blur_type`, if ɴsɪ carries a velocity attribute. There is
      no `use velocity` flag; an earlier draft of these specs was wrong
      about that.
- [x] T2.5 Report, never flatten, a scene with more than two motion
      samples on one attribute. rdl2 has exactly two timesteps.
      `flush::tests::more_than_two_motion_samples_are_reported` for a
      transform and `more_than_two_deformation_samples_are_reported`
      for `P`.

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

- [x] T6.1 **Map `instances` to `RdlInstancerGeometry`.**
      `instance_sources` to `references`, `Instance::transform` to
      `xform_list` with `method` = `2`, `Instance::source` to
      `ref_indices`. Before this an `instances` node contributed
      *nothing* -- `flush.rs` handled `ResolveError::Instanced` by
      reporting it and never asked the question that succeeds -- so a
      crowd of a thousand rendered as one prototype at the origin.
- [x] T6.2 A prototype's own transform, **applied exactly once** --
      settled by rendering, as it had to be.
      `a_prototypes_own_transform_is_applied_once` puts the prototype
      one unit right of its instancer and places instances at -3 and
      +3, so applied once the left copy centres on -2, dropped on -3,
      doubled on -1: three distinguishable places in the frame. The
      columns either side stay dark.
      **This found a bug that made an instanced scene render nothing.**
      A `sourcemodels` edge need not point at geometry -- ɴsɪ connects
      the *model root*, commonly a `transform` with the geometry under
      it, which is how a prototype gets its own placement. `references`
      named the transform, the attribute failed to set entirely, and
      nothing drew. Reported, at least, rather than silent.
- [ ] T6.3 A moving instancer. `instance_transforms_at(t)` exists
      because 3Delight renders a sampled `transformationmatrices`;
      rdl2 has two timesteps, so this is `T2.5`'s reduction rule again.
      `velocities` is the other route and is per-instance.
- [ ] T6.4 Nested instancing. MoonRay's `instance_level` goes to `4`
      and ɴsɪ nests `instances` under `instances`. Confirm the depth
      maps, and report past four rather than flattening.
- [x] T6.5 **Assert it is instanced, not expanded.**
      `a_prototype_is_referenced_once_not_expanded` counts the
      prototype's declarations, because a flattened scene renders an
      identical image and no image test can tell them apart.
- [x] T6.6 Render an instanced scene.
      `an_instanced_scene_renders_its_copies`: two copies of one
      prototype, left and right, with a dark gap between them --
      which is what a prototype drawn once at the origin would not
      produce.

- [x] T5.4 **A batch render writes its file from the linked
      renderer.** Through MoonRay's own output machinery --
      `writeImageWithMessage` for the beauty and
      `writeRenderOutputsWithMessages` for every `RenderOutput` --
      rather than encoding an EXR here, which would mean
      reimplementing layer naming, header metadata and the
      aperture/region windows and getting all of it subtly wrong.
      `inprocess::a_batch_render_writes_the_image_it_was_asked_for`
      reads the image back rather than checking a file appeared: an
      empty or black EXR passes the weaker check, and this crate has
      produced both.

## Cost Of The Choices Above

- [ ] T7.1 A detached shape is emitted and turned off rather than left
      out, so the first flush hands MoonRay geometry it will never
      draw -- tessellated, and in the accelerator. Right for an
      interactive session, where it makes a hide an attribute edge
      rather than a structural one; waste for a batch render that will
      never show it. The flush knows which it is doing only if it is
      told, so this wants a flag rather than a guess.

## Not Now

- [ ] TN.1 Progressive rendering. MoonRay has `PROGRESSIVE`,
      `PROGRESSIVE_FAST` and `REALTIME`; reaching them requires the
      shim, not `.rdla`.
- [ ] TN.2 OSL. MoonRay has none. Shared work, separate surface.

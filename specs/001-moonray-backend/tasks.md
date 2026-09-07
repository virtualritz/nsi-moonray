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
- [x] T0.6 Authoring twins: `tools/twin`. Declarations with no
      implementation, built against `scene_rdl2` alone, so a real mesh
      scene can be built and read back on a host that has not built
      MoonRay -- fifteen minutes from stock packages against fifty and
      five packaging problems.
      **MoonRay's own `attributes.cc`, compiled out of a source
      checkout rather than copied**, so a twin cannot drift from what
      the renderer declares. `RdlMeshGeometry`,
      `RdlInstancerGeometry`, `PerspectiveCamera` and `EnvLight`.
      `UsdPreviewSurface` has none and cannot: its `attributes.cc` is
      *generated* from an `.ispc` by MoonRay's build, so a twin would
      need the thing these exist to avoid needing -- and declaring its
      six carried parameters by hand is the copy that drifts, which
      would be worse than the gap. A scene checked this way reports it
      as an unknown class, which is honest.
      `apply::a_mesh_scene_applies_through_the_authoring_twins`, which
      found that **an enumerable `Int` reads back as its enum name**:
      the flush writes `subd_scheme` as `1` and rdl2's writer emits
      `"catclark"`, so a text diff against the emitter differs on every
      enumerable attribute even when the value is identical.
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
- [x] T4.2 **`libnsi_moonray.so`: a drop-in ɴsɪ renderer.**
      `src/capi.rs` exports all twelve symbols, records into
      `nsi_intermediate::Scene`, and renders **in process** through a
      linked MoonRay: `"start"` with `"interactive"`, `"synchronize"`,
      `"wait"` and `"stop"` all act on a live
      [`Session`](../../src/session.rs), and the application's
      `outputdriver` callbacks receive each snapshot as the frame
      converges. Spawning the binary is the fallback for when there is
      no linked renderer. `tests/dropin.rs` `dlopen`s the artefact and
      drives a scene through it.
      `"suspend"` and `"resume"` are deliberately unmapped: MoonRay can
      `stopFrame`/`startFrame`, but restarting loses the samples taken
      so far, and a viewport that dimmed whenever it was touched would
      be worse than one that ignores the call.
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
- [x] T4.3 `.nsi` stream input. `mrr` takes an ɴsɪ stream, flushes it
      to `.rdla` beside itself, and renders that -- which is also what
      someone debugging the translation wants to look at. The parser is
      upstream's (`nsi-parse`) and drives `nsi_trait::Nsi`, which
      `Recorder` implements, so an ɴsɪ file feeds the same `Scene` the
      C entry points record into and there was nothing to write here
      but the wiring. gzip too, since 3Delight writes compressed
      streams.
      **Told apart by content, not by extension**: a file named `.nsi`
      that is really `.rdla` is a thing that happens, and guessing from
      the name would fail with a parse error about the wrong format.
      `tests/nsi_input.rs`.
- [x] T4.4 Link `libmoonray` rather than spawning its CLI. **Done in
      [`002`](../002-interactive-updates/tasks.md)** `R1`, which is
      where the reason for it lived: a spawned batch render has no
      `SceneContext` to edit and no `RenderContext` to snapshot, so it
      foreclosed incremental updates, progressive delivery and
      concurrent rendering at once. `tests/inprocess.rs`.

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
- [x] T5.3a **A bucket is the rectangle that changed.** Through
      `snapshotDelta` and its `ActivePixels`: the first covers the
      frame, later ones only what the renderer refined -- which is what
      a driver over a network wants and what `snapshotRenderBuffer`
      cannot say.
      **Not a drop-in.** `snapshotDelta` does "no resize, no
      extrapolation and no untiling" and its buffer is *not normalized
      by weight*, so the shim undoes the tiling and divides each pixel
      by its own sample count. Both have wrong versions that look
      plausible -- a mis-untiled frame is scrambled, an unnormalised
      one merely darker -- so
      `inprocess::a_delta_snapshot_agrees_with_a_full_one` compares the
      two rather than eyeballing one.
      Two things had to be given the frame's shape or MoonRay crashed
      inside its own parallel loop: the buffers are **tile-aligned**
      (8 either way), and `ActivePixels::init` allocates the per-tile
      masks that `snapshotDelta` writes into.

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

- [x] T2.0 **One shutter for the scene, not one per object.** MoonRay
      evaluates every `blur(a, b)` at two *global* timesteps, so
      sampling each node over its own recorded times renders a shape
      that moved between `t=10` and `t=11` as though it had moved
      during another shape's shutter -- and two objects moving over
      different ranges come out with the same smear, which looks like
      motion blur working.
      The interval is the camera's `shutterrange` when it has one and
      the union of every recorded motion time otherwise, every blurred
      value is sampled at its two ends, and `motion_steps` tells
      MoonRay which two they are. Held at the ends rather than
      extrapolated, so a shape that stopped moving early stays put.
      `flush::tests::two_objects_share_one_shutter`,
      `a_shutter_range_beats_the_union`,
      `deformation_is_resampled_onto_the_shutter`.

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
- [~] T2.4 Velocity-based motion. **MoonRay's side is fully known; ɴsɪ's
      name is not.** `RdlMesh` declares `velocity_list_0` and
      `velocity_list_1` (`Vec3fVector`), the first documented as being
      used "instead of vertex positions from a second motion step", and
      `CommonAttributes.h` declares `motion_blur_type` with
      `MotionBlurType::{STATIC, VELOCITY, FRAME_DELTA, ...}` defaulting
      to `BEST` -- which is why the deformation mapping (`T2.3`) needs
      no flag: `BEST` picks the two-position path when two positions
      are what it has. There is no `use velocity` boolean; an earlier
      draft of these specs was wrong about that.
      What is missing is **which ɴsɪ attribute carries velocity**.
      `nsi-intermediate` has no concept of one, and guessing a name is
      the same failure `T1.3a` refuses: a wrong guess renders
      plausibly, blurred by the wrong amount, and looks like a shutter
      setting. Whoever has the spec can finish this in minutes.
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
- [x] T3.2 Confirm limit-surface evaluation is reached, not bypassed.
      `inprocess::subdivision_reaches_the_limit_surface` renders a
      **cube** as a polygon mesh and as a Catmull-Clark surface and
      compares coverage: the limit surface rounds inward, so it covers
      measurably fewer pixels.
      A cube because the first version used a planar 2x2 grid and both
      renders covered exactly 3598 pixels -- a planar cage subdivides
      to itself, and with sharp boundaries the outline is preserved
      exactly. The subject has to be closed and non-planar for the
      limit surface to differ at the silhouette.
      View-adaptive tessellation is a separate question and is not
      asserted: it would need the tessellation counts at two camera
      distances.

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
- [x] T6.3 A moving instancer, through `velocities`.
      **`xform_list` is not blurrable** -- declared with no flags, and
      `FLAGS_BLURRABLE` is what carries two timesteps (`research.md`
      F10) -- so ɴsɪ's sampled `transformationmatrices` cannot cross as
      a `blur()` pair. MoonRay's route is a per-instance velocity,
      applied as `position + velocity * dt` with
      `dt = (motionStep - evaluationFrame) / fps`.
      So the magnitude is `delta * fps / (close - open)`. **`fps` does
      not cancel** -- an earlier note in `research.md` said it did, and
      the arithmetic says otherwise -- which is harmless only because
      this backend now *writes* `fps` rather than relying on rdl2's
      default, so the two agree by construction.
      `flush::tests::a_moving_instancer_gets_velocities` checks the
      number (6 units across a `[0, 1]` shutter at 24fps is 144);
      `inprocess::a_moving_instancer_renders_blurred` checks it reaches
      the image, through a different mechanism from every other moving
      thing here.
      Only translation. Rotation and scale across the shutter need the
      decomposed form and `use_rotation_motion_blur`, and that is
      reported rather than silently dropped.
- [x] T6.4 Nested instancing. An `instances` node connected to
      another's `sourcemodels` works through the same recursion in
      `fillGenerateList` that stops a prototype drawing on its own --
      nothing extra to map. `inprocess::instancers_nest` places two
      copies of a two-copy instancer and counts four runs of lit
      columns, which one collapsed level would not give.
      `instance_level` is left unset: its own comment says it adds a
      shading primitive attribute, not that it is needed for the
      nesting to work.
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

- [x] T7.1 A detached shape is emitted and turned off for an
      interactive session and **left out** for a batch render.
      `flush::Purpose`, and the flush is told rather than guessing.
      Keeping it is right where something will show it again -- hiding
      becomes nine attribute writes on an object MoonRay already has,
      an accelerator rebuild rather than a re-tessellation -- and waste
      where nothing will: a tessellation and a place in the accelerator
      for something never drawn. `mrr` and the spawned path ask for
      `Batch`; a `Session` is `Interactive` by construction.
      `flush::tests::a_batch_flush_omits_a_detached_shape`.

## Not Now

- [x] TN.1 Progressive rendering. `Mode::{Batch, Progressive,
      ProgressiveFast, Realtime}` in `src/rdl2/render.rs`; the drop-in
      renders `Progressive`. Reached through the shim, as predicted.
- [ ] TN.2 OSL. MoonRay has none. Shared work, separate surface.

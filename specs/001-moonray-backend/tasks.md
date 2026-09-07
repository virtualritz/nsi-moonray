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

- [x] T4.1 **`mnry`, the command.** Modelled on
      [`rdl`](https://github.com/virtualritz/delight-helpers), the
      `renderdl` replacement, so the two take the same shape: `render`,
      `cat`, `watch` and `generate-completions`, the same
      frame-sequence syntax down to the binary-splitting form, and the
      same short options where they mean the same thing. `rdl`'s
      `--collective` and `--cloud` are not carried, because MoonRay has
      neither and an option accepted and ignored is worse than one that
      is not there.

      `render` renders **in process** through a `Session` when the
      crate is built with `rdl2`, and falls back to writing the scene
      out and running the `moonray` binary otherwise -- or for an
      `.rdla` input, which only rdl2's own reader parses. `-v` says
      which path it took, because the two have different capabilities
      and someone wondering why their viewport does not update should
      be able to find out. `--output` redirects by editing every ɴsɪ
      `outputdriver`'s `imagefilename`, which is the attribute a host
      would have set, so the two paths cannot disagree about where the
      image went. `cat` answers "what did my ɴsɪ scene become?" without
      rendering it, which is the question every translation bug starts
      as.

      Behind a default-on `cli` feature: a library consumer that wants
      the flush and nothing else takes `default-features = false` and
      none of the argument parsing comes with it.
      `tests/nsi_input.rs` drives the command.
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
- [x] T4.3 `.nsi` stream input. `mnry` takes an ɴsɪ stream and builds
      MoonRay's scene straight from it; `mnry cat` writes the `.rdla`
      that would have been built, which is what someone debugging the
      translation wants to look at. The parser is
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
- [x] T1.3a Carry what parameters can be carried into the substitute
      surface. An ɴsɪ shader is an OSL shader and its parameter names
      are its author's, so there is no ɴsɪ spelling of "roughness" to
      look up and a guessed one renders plausibly and silently. What
      there is instead is a short list of shaders in practical use,
      shipped compiled with 3Delight, and `.oso` is a text format: the
      `PARAMETERS` table in `flush.rs` was **read** off them with
      `tools/probe/parameters.sh`, keyed on `shaderfilename`. Six
      shaders are known -- `dlPrincipled`, `dlStandard`,
      `openPBRSurface`, `dlMetal`, `dlGlass`, `dlPrelit` -- and a
      seventh row carries `UsdPreviewSurface`'s own names for anything
      the table does not know, which is the exact-name behaviour that
      was here before. Everything not carried is still reported by
      name. `research.md` F11;
      `flush::tests::a_known_shader_is_carried_by_its_own_parameter_names`
      and two siblings.
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
- [x] T1.7a Lights. ɴsɪ has **no light nodes at all**: section 4.5 of
      the specification says any geometry whose surface shader produces
      an `emission()` closure is a light. So recognising one means
      knowing what a shader does, and MoonRay runs no OSL -- there is
      no attribute to read. What *is* readable is the shader's name,
      and the emitters in practical use are the same short list
      `PARAMETERS` is built from. `LIGHTS` in `flush.rs` is that rule:
      `areaLight` and the specification's own `emitter` become a
      `MeshLight`, `pointLight` a `SphereLight`, `spotLight` a
      `SpotLight`, `distantLight` and `directionalLight` a
      `DistantLight`. `i_color`, `intensity` and `exposure` -- which
      every 3Delight light shader declares -- are the three
      `scene_rdl2`'s `Light` base class declares, so that part is a
      correspondence rather than an interpretation; the spot's cone
      angles are derived from the specification's listing 4.3, where
      `penumbraAngle` is added to the *half* angle and so counts twice.

      A shader not on the list leaves its geometry a shape, which is
      the safe direction: a mesh that should have been a light renders
      dark and visible, whereas one silently promoted to a light
      disappears from the frame.

      `research.md` F11 and F12;
      `flush::tests::a_mesh_wearing_an_emitter_becomes_a_mesh_light`
      and four siblings; `inprocess::a_light_shader_lights_the_scene`
      renders one. A `MeshLight` cannot be rendered on this build --
      MoonRay creates a `DwaBaseMaterial` for it, which ships with
      `moonshine_dwa` rather than `moonray` (F12) -- so the render test
      uses a `pointLight`, which differs only in which row matches.
- [x] T1.6 ɴsɪ's `fov` is **vertical** -- measured against 3Delight,
      not inferred. The specification says only "the field of view
      angle, in degrees", and reading it as horizontal renders a
      plausible picture framed wrong, in a way that looks like the
      camera was placed differently. `tools/probe/framing.nsi` puts a
      quad of half-extent 1 one unit in front of the camera at `fov`
      90, on a 400x200 frame where the two axes cannot be confused;
      3Delight lit the full 200 rows and 200 of the 400 columns.
      `inprocess::the_frame_matches_3delights_framing` renders the same
      probe through MoonRay and lands on the same rectangle to within a
      pixel. `research.md` F11.

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
- [x] T2.4 Velocity-based motion. **Not applicable: ɴsɪ has no such
      attribute.** `RdlMesh` declares `velocity_list_0` and
      `velocity_list_1` (`Vec3fVector`), the first documented as being
      used "instead of vertex positions from a second motion step", and
      `CommonAttributes.h` declares `motion_blur_type` with
      `MotionBlurType::{STATIC, VELOCITY, FRAME_DELTA, ...}` defaulting
      to `BEST` -- which is why the deformation mapping (`T2.3`) needs
      no flag: `BEST` picks the two-position path when two positions
      are what it has. There is no `use velocity` boolean; an earlier
      draft of these specs was wrong about that.
      **ɴsɪ has no velocity attribute for meshes**, so there is
      nothing to carry and this is closed as not applicable rather than
      left unfinished. The specification defines velocity only on the
      two OpenVDB nodes, as the *name of a grid* inside the `.vdb`
      (`velocitygrid`, `velocityscale`, `velocityreferencetime`);
      `mesh`, `particles` and `curves` have none, and their motion is
      `NSISetAttributeAtTime` on `P`, which is `T2.3`. Measured as well
      as read: `tools/probe/motion.sh` renders the same quad once per
      candidate name, and two position samples smear while `velocity`,
      `v`, `V`, `vel` and `motion` are ignored in silence.
      `research.md` F11.

      The `velocity()` function in `flush.rs` is a different thing and
      is unaffected: it converts a *transform* delta for
      `RdlInstancerGeometry`, which is MoonRay's own requirement
      (`F10`), not an ɴsɪ attribute.
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
      for something never drawn. `mnry cat` and the spawned path ask
      for `Batch`; a `Session` is `Interactive` by construction.
      `flush::tests::a_batch_flush_omits_a_detached_shape`.

- [x] T7.2 **Interned handles, on measured numbers.** Upstream stores a
      handle interned behind `ustr_handles`; `interned_handles`
      forwards to it and is on by default. `tools/footprint` is why:
      a 100 001-node scene costs 144.6 MB and 35.3 seconds to record
      as upstream ships, and 103.1 MB and 11.3 seconds interned. The
      speed is not a side effect of the size --
      `edges_to_attribute` had to build two `String`s to probe its key
      on every call. Additive, so a consumer that disagrees still
      compiles. `research.md` F14.
- [x] T7.3 **The flushed document was bigger than the scene.** Fell
      out of `T7.2`: 109 MB against 103 MB, and it did not shrink when
      the scene did, because `Document` copied every handle into
      `String`s it owned -- `Object::name` once, and `Reference` twice
      per `Layer` row, up to nine of them -- and it is resident for the
      life of an interactive session, since `apply_affected` diffs
      against it between frames.

      `src/name.rs` is the fix: `Name`, a `Ustr` with
      `interned_handles` and a `Box<str>` without, carrying every class
      name, handle, attribute name and part. `Value::String` is
      deliberately not one -- file paths are neither short nor
      repeated, and interning them would put unbounded, never-freed
      strings in a global table. The document is now **71.2 MB, 35%
      smaller**, and costs less than the scene again.

      Borrowing was the other candidate and does not work: `Session`
      holds the previous document across an edit to the scene that
      produced it, which is exactly the borrow that cannot outlive its
      source.

      Two things worth keeping from doing it. `Borrow<str>` is a
      promise that a `Name` hashes as the `str` it borrows as, and
      `Ustr`'s own `Hash` is not that -- a derived one made
      `HashMap<Name, _>::get("quad")` answer `None` for a key that was
      there, silently. And `Debug` is hand-written to print quoted, as
      `String` does, because handles reach users through `{handle:?}`
      in every limitation message. `research.md` F14; the oracle tests
      are what say the written bytes did not move.

## Not Now

- [x] TN.1 Progressive rendering. `Mode::{Batch, Progressive,
      ProgressiveFast, Realtime}` in `src/rdl2/render.rs`; the drop-in
      renders `Progressive`. Reached through the shim, as predicted.
- [x] TN.2 **OSL runs.** MoonRay has none, and ɴsɪ *is* OSL -- a `shader` node
      names a compiled `.oso`, and a light is a shader that emits. So
      everything this backend does with shaders and lights is a table
      of names standing in for a language it cannot run.
      `specs/003-osl/research.md` reads what closing that would take.
      Three things came out of the reading and two are already acted
      on:

      - A `Material` with no vectorised `shade` renders **black**
        rather than failing -- `Material::shadev` null-checks and
        `canRunVectorized` never asks. The default execution mode is
        `AUTO`, which picks vectorized, and OSL's `ShadingSystem` is
        scalar. So an OSL material has to force scalar execution, and
        the missing check is worth reporting upstream.
      - MoonRay's self-emission is **hit-only**: the integrator does
        `radiance += pathThroughput * bsdf->getSelfEmission()` and
        nothing else. No next-event estimation, no shadow rays. So
        `emission()` cannot simply become `addEmission` -- that turns
        every ɴsɪ light into noise.
      - A `MeshLight` with `visible_in_camera` forced on is **both seen
        and sampled**, against its own geometry: `MeshLight::intersect`
        ray-traces the real mesh and `Scene::updateActiveLights` puts a
        bounded light in the camera-visible set. That is the
        reconciliation, and `T1.7a` now sets it.

      Built: `dso/osl/` is an `Osl` material holding an OSL
      `ShaderGroup`, executing it at each shading point and walking the
      closure tree into `BsdfBuilder` calls; `src/osl.rs` turns an ɴsɪ
      shader network into the group specification it carries, which is
      a text transformation because rdl2 has no dynamic attributes and
      `ShaderGroupBegin` takes one string (`003` O6). `build.rs` builds
      the DSO when `$OSL_ROOT` is set and puts it on MoonRay's DSO
      path; `Render::new` forces scalar execution, without which the
      material renders black.

      `Shading::{Osl, Substitute}` is the axis, defaulting to whichever
      the build can do, and it is a *choice* rather than a `cfg` at the
      point of use because the flush is a pure transformation -- a
      scene dumped on a machine with no OSL and rendered on a farm that
      has one should say `Osl`. A shader with no `shaderfilename` falls
      back to the substitute for that shader alone, because ɴsɪ always
      returns an image.

      `inprocess::an_nsi_osl_shader_renders` compiles a shader with
      `oslc` that this crate has never seen, records it as an ɴsɪ
      shader node, and asserts its colour per channel -- which is the
      only thing that separates "OSL ran" from "something plausible
      happened", since a substitute at its defaults renders a perfectly
      good grey quad.

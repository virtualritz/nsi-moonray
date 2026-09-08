# Handoff

Updated 2026-09-08. Read `specs/README.md`, then this.

## What Exists

**MoonRay is linked, not spawned.** A recorded ɴsɪ scene reaches live
`SceneObject`s, renders in this process, and each snapshot reaches the
application's own closures as the frame converges. Editing the scene
and calling `synchronize` re-sends only what changed. No file is
written on the render path and no process is started.

That is the whole of `001`'s delivery question and most of `002`. What
is *not* done is proving the incremental path is incremental -- see
"What Would Bite You".

```
ɴsɪ calls ──▶ nsi-intermediate ──▶ flush ──▶ Document
              (records + journals)              │
                        apply │                 │ to_rdla
                              ▼                 ▼
                   MoonRay, linked          .rdla, a *dump*
                              │
                     snapshot │
                              ▼
                       callback.write
```

`.rdla` is a dump you can ask for with `$NSI_MOONRAY_SCENE`, not a
transport. The oracle tests still check it, and still earn their place:
they check the *values* this backend computes without needing a
renderer, which is what let the transport change underneath them.

A hundred and twenty-three tests without a renderer, and forty-three more
integration tests behind one. `just test` needs no renderer, no
`scene_rdl2`, and nothing beside this repository. The `rdl2` feature is what asks for a
renderer, and it is off by default so that this crate stays workable
from a machine that cannot build MoonRay.

**Use the justfile.** `just ci` is what CI runs and what to run before
committing; `just test-rdl2` is the renderer half. Both traps that cost
a debugging session are encoded there rather than only written down:
the `cdylib` `tests/dropin.rs` opens by path has to be rebuilt first,
and the renderer tests need `cargo test` rather than nextest, because
they serialize on a process-wide mutex that nextest's process-per-test
model makes inert.

`scene_rdl2` builds on a modest machine -- four cores, stock Ubuntu
packages, about fifteen minutes. `quickstart.md` has the recipe and the
three upstream problems it has to work around, each with the error it
produces. None of them is subtle once seen, and all three cost time:

- The **Ninja generator cannot build it.** `ISPC_HEADER_DIRECTORY` is
  set with a leading slash, and the generated header ends up declared
  at the filesystem root while every consumer wants it in the build
  tree. Use the Makefile generator. Same under CMake 3.23.1, the
  version MoonRay's own script downloads, so it is not a regression.
- `BinPacketDictionary.h` uses `std::function` without including
  `<functional>`; GCC 13 rejects it. One line.
- A **consumer must repeat the compile definitions** rdl2's own build
  passes -- `__cdecl=`, `PLATFORM_UNIX`, `PLATFORM_LINUX`, `__AVX__` --
  or `rdl2/Types.h` does not parse at its first function typedef.

## What The Oracle Corrected

Four things about `.rdla` that a careful guess would have got wrong,
and two findings in `research.md` that were simply wrong:

- `Vec3d` prints as `Vec3(...)`. No precision suffix, on any of the
  `Vec`/`Mat` types.
- A null object reference is `undef()`, not `nil`.
- A bound attribute keeps its own value alongside the binding.
- Numbers go through C++'s `%g` at `max_digits10`: `0.1f` is
  `0.100000001`. Rust's `{}` gives `0.1`, and every float in a scene
  would have differed.
- **`scene_rdl2` does need ISPC.** F7 said it did not.
- **MoonRay has no `use velocity` flag.** F1 said it had one. Velocity
  is `velocity_list_0` plus `motion_blur_type`, and the deformation
  attribute is `vertex_list_1` -- `vertex list mb` is an alias.

This is the same discipline that made the `.nsi` emitter correct, and
it paid the same way. Keep it: capture, then emit.

## Upstream Moved, And It Answers The Hard Part

`nsi-intermediate` gained a synchronise journal and dirty propagation --
`Scene::take_changes()` and `Scene::affected()` -- plus motion-sample
resolution, a `.nsi` parser (`nsi-parse`) and much else. Three things
`002` listed as upstream asks were already done.

Two API changes bite a backend written against the old crate, and both
are corrections rather than churn:

- **`Node::attrs` is not the value.** `SetAttributeAtTime` on an
  attribute that is not motion data sets it for the whole shutter, so
  reading `attrs` answers "not set" for something the renderer honours.
  Use `Node::effective`, which is what the resolver reads.
- **An ɴsɪ string is bytes, not `String`.** The spec says UTF-8 --
  3Delight has agreed to write that down -- but the C API takes a
  `const char*`, and a promise does not stop a caller handing over
  something else. Recording bytes and converting lossily where text is
  actually needed keeps a malformed name a mangled name rather than a
  panic at the boundary, which is what "always returns an image"
  requires.

## The Dependency, Which Was A Path And Is Not

`nsi-intermediate`, `nsi-parse`, `nsi-trait` and `nsi-ffi-wrap` are on
crates.io as of 2026-09-08, and this crate depends on them by version.
**`T0.7` is closed.** A sibling `../nsi` checkout is no longer a
precondition for anything, and CI is possible for the first time.

Worth keeping, because both were tried and both cost a day:

- A **git** dependency on the `nsi` workspace makes Cargo fetch its
  private `.blueprints` submodule, and fail. `[patch]` does not help --
  Cargo fetches the patched source anyway.
- A **path** dependency has no version to disagree about, so an
  out-of-date sibling does not report a version conflict. It reports a
  missing method in *this* crate's source, which reads like a bug here
  and is not. Master did not compile for a day for exactly that: the
  commit adopting upstream's primitive-variable resolver landed against
  six upstream commits the checkout beside it did not have.

To work on both at once, override the dependency in a local
`.cargo/config.toml` and do not commit it. Putting a `[patch]` in
`Cargo.toml` puts the sibling checkout back in everyone's way.

## How This Reaches A Renderer

Two shapes, and neither asks this repository to build MoonRay:

- **`mnry`** is the command. `render`, `cat`, `watch`, modelled on
  `rdl` from `virtualritz/delight-helpers` so the two take the same
  shape. Built with `rdl2` it renders in process; without it -- or for
  an `.rdla`, which only rdl2's own reader parses -- it writes the
  scene out and runs the `moonray` binary. `-v` says which.
- **`libnsi_moonray.so`** is a drop-in ɴsɪ renderer, and it exists:
  `src/capi.rs`. `nsi-ffi-wrap` `dlopen`s a library and resolves its
  whole symbol table up front, so all twelve have to be there --
  the eleven `NSI*` entry points plus `DspyRegisterDriver`, which the
  `output` feature looks up and whose absence would make the load fail
  for a consumer that has that feature on. `tests/dropin.rs` opens the
  built artefact and drives a triangle through it by symbol.

  It is interactive now: `"start"` with `"interactive"`,
  `"synchronize"`, `"wait"` and `"stop"` all act on a live
  `Session`. `"suspend"` and `"resume"` are deliberately unmapped --
  MoonRay can `stopFrame`/`startFrame`, but restarting loses the
  samples taken so far, and a viewport that dimmed whenever it was
  touched would be worse than one that ignores the call.

## Pixels Reach The Application, Without ndspy

MoonRay delivers progressively; it just does not *push*.
`RenderMode::PROGRESSIVE` puts samples up as they exist and a consumer
*pulls* them with `snapshotDelta` -- `moonray_gui` is a loop around
that. An ɴsɪ consumer expects a push. The gap is pull-versus-push, not
a missing capability, and it closes on this side. They meet in
`nsi-ffi-wrap`'s
`output` feature, which is Rust on both sides: an application hands
over `callback.open`, `callback.write` and `callback.finish` as
`Reference` attributes on an `outputdriver`, and `src/display.rs`
calls them. **No ndspy struct is marshalled anywhere in between.**

Three things worth knowing before touching it, each of which cost a
debugging session:

- **`Reference` must survive recording.** It never reaches MoonRay,
  which is exactly why `capi.rs` dropped it -- and dropping it leaves
  an application with a driver, a perfect render, and an empty
  viewport. No error anywhere.
- **MoonRay does not write `RGBA`.** It names channels after the ɴsɪ
  output layer: `Ci.R`, `Ci.G`, `Ci.B`. A reader asking for an RGBA
  layer fails with "no layer matched", which reads like a broken file
  and is a broken read.
- **A trait object's vtable belongs to its compilation.** Calling
  `Box<dyn FnWrite>` is sound where the application and this backend
  share one `nsi-ffi-wrap`. A separately built `cdylib` reached by
  `dlopen` is a different compilation, and the safe route there is the
  `extern "C"` entry points `DspyRegisterDriver` hands over. `T5.2`.

Delivery is still one bucket at the end, read back off the file --
because this backend *spawns* MoonRay, and a batch process has no
`RenderContext` to snapshot. That is the whole reason, and linking
`libmoonray` (`002` `R1`--`R3`) is the whole fix. `T5.3`.

Taking `.nsi` *files* works: `mnry render scene.nsi` parses it and
builds MoonRay's scene from it. The parser is upstream's `nsi-parse`, which drives
`nsi_trait::Nsi` rather than producing a scene type of its own, so it
feeds the same `Recorder` the C entry points do. Which kind of file it
is comes from the content rather than the name.

Spawning is now the *fallback*, for when there is no linked renderer:
`rdl2dso` not found, a session already running, or render prep
refusing the scene. ɴsɪ always returns an image, so a host that cannot
have the fast path still gets its render.

## The Two Layers That Matter Now

- **`shim/`** is an `extern "C"` surface over `scene_rdl2` and MoonRay.
  Two rules hold at every entry point, and both are load-bearing: no
  C++ exception crosses the boundary (`set` by name throws, and
  unwinding into Rust is undefined behaviour), and nothing refuses a
  scene. `shim/tests/smoke.cc` drives every setter through rdl2's own
  `ExtensiveObject` -- which is how the calls were checked against the
  library rather than against its headers.
- **`src/session.rs`** is the interactive loop: a scene you keep
  editing and a renderer that keeps what it already built.
  `src/capi.rs` is a thin shim over it, so an application reaches the
  same code whether it embeds this crate or `dlopen`s it.

## Where To Start

**Almost everything actionable is done.** What is left splits three
ways.

**Waiting on upstream, with the reports written and ready to file in
[`upstream/`](upstream/):**

- The **empty-camera crash** (`002` F5). Worked around here by
  emitting a default camera, so nothing is blocked on it.
- The **ignored `FLAGS_GEOM_RELOAD_BVH_ONLY`** (`002` F9). This one
  *does* block something: hiding a shape re-tessellates it, when
  MoonRay's own comment says it should cost a BVH rebuild. Two lines
  in `scene_rdl2`, and the evidence that they are safe is in the
  report.

**The four that were waiting on knowledge are answered**, and none of
them by a guess. They are worth reading as a set, because each was
settled by finding something readable rather than by picking the
plausible name:

- `T1.3a`, which ɴsɪ shader parameter means `roughness`. There is no
  answer in general -- an ɴsɪ shader is an OSL shader and its parameter
  names are its author's. `.oso` is a text format, so the `PARAMETERS`
  table in `flush.rs` was *read* off the shaders 3Delight ships.
- `T1.7a`, how to recognise a light. ɴsɪ has no light nodes: geometry
  whose surface shader produces an `emission()` closure is one. `LIGHTS`
  in `flush.rs` matches the same short list of emitters by name.
- `T1.6`, whether ɴsɪ's `fov` is vertical. It is, measured against
  3Delight with `tools/probe/framing.nsi` on a 400x200 frame where the
  two axes cannot be confused.
- `T2.4`, velocity. **Not applicable**: ɴsɪ has no such attribute.
  MoonRay's side was fully mapped and there was nothing to map it to.

The substitution those two tables drive is now the *fallback*. Shading
runs OSL -- see `specs/003-osl/` -- and the tables are what a build
without `$OSL_ROOT` gets.

**An install finds itself now.** `src/dso.rs` searches for MoonRay's
`rdl2dso` -- beside the running binary first, so a bundle works
wherever it was unpacked, then where each platform puts an install --
and `--dso-path` is an override rather than a requirement.
`packaging/bundle.sh` assembles the tree and `cargo packager` wraps it.
`specs/005-packaging/` has the layout contract, the reason there is no
Windows renderer, and what is still missing: a release workflow, which
is blocked on `T0.7` like all CI here, and code signing.

Building a `.deb` was what found the defect in it: the packager
installs resources under `/usr/lib/<binary>/`, which the bundle layout
missed entirely, so an installed package would have resolved no
classes. That is the same shape as everything else in this file, and it
was found the same way.

**Still worth doing here**, and the first two are the ones with real
weight:

- **The orthographic camera renders through `moonray` and not through
  the in-process path.** The flush emits the right class and the right
  attributes, and the emitted `.rdla` renders. The same document
  applied to a live `SceneContext` and rendered progressively comes
  back empty, with no error from `apply` and no complaint from render
  prep. Batch against progressive is the difference that has not been
  ruled out. This is the only open item that is a *silent wrong
  answer* rather than a missing feature, which is why it is first.
- **An OSL volume shader.** The geometry crosses -- the interface's
  `volume` node is OpenVDB and `VdbGeometry` reads exactly that -- but
  it renders through MoonRay's stock `VdbVolume` rather than the
  network bound through `volumeshader`. OSL's `anisotropic_vdf` and
  `medium_vdf` against `VolumeShader`'s four separate virtuals, from
  one OSL execution, is the missing piece. Two things already measured
  on the way are in `specs/003-osl/research.md`, and both are the kind
  that cost an afternoon: a volume is shaded through the `Layer`'s
  *sixth* column, and `emission_grid` must name an RGB grid.
- **`T0.6`, the authoring twin of `RdlMeshGeometry`.** Less valuable
  than it was -- `tests/apply.rs` now checks against real rdl2 -- but it
  is what would let a host with no MoonRay check a mesh scene.
- **`T5.2`, the `dlopen` route for callbacks.** Deprioritised
  deliberately: the closures work where the application and this
  backend share one `nsi-ffi-wrap`, which is the case that was asked
  for.

The rest of the OSL open questions -- `vdbparticles`, AOV forwarding,
3Delight's hair closure, scoped `getattribute` -- are listed with their
reasoning at the end of `specs/003-osl/research.md`.

## What Has Been Rendered

Through a spawned MoonRay:

- a triangle, which is what found the two black-image bugs below;
- a translated quad, checked against where the transform puts it rather
  than against the matrix in the file (`T1.2`);
- two quads with two materials, red and green, asserted per channel
  (`T1.4` -- the inherited top risk, and the one thing reading the file
  could never have settled);
- a moving quad, blurred (`T2.2`);
- a `polyhedron-ops` polyhedron through the backend loaded as
  `lib3delight.so` -- the drop-in path end to end with an unmodified
  consumer (`examples/polyhedron`).

Through a **linked** MoonRay, with no file and no process:

- a lit quad, asserted at the centre of frame and across a tenth of
  the pixels -- "any pixel is non-zero" would pass on a stray sample;
- the same, streamed to an application's closures as it converged: six
  buckets for a frame taking a third of a second;
- a red quad turning green through one `synchronize`, and a quad
  moving to where a transform edit put it -- both asserted on pixels,
  because "applied but not marked" is a scene holding the new value
  while the render shows the old one, and reading the scene back would
  pass on exactly that.

With OSL executing, which is `003`:

- an arbitrary OSL shader, compiled by `oslc` during the test rather
  than checked in, so what is proved is that *a* shader crosses and not
  that one does;
- an ɴsɪ shader network as an OSL group specification, and a shader
  that transforms coordinates -- which is what caught render space
  following the camera (`O9`);
- the shaders 3Delight ships, run as themselves rather than
  substituted;
- an OSL displacement, which is a usage of the same system rather than
  a second one (`O11`);
- an OSL light lighting the scene and varying across its own surface;
- OpenVDB volumes;
- an output layer naming a lobe, which reaches a MoonRay AOV -- and the
  material has to stay unlabelled for it to (`O16`).

## Eight Things Found By Running It

Each cost real time, none is inferable from a header, and every one is
a crash or a silent wrong answer rather than an error:

- **`initGlobalDriver` is not optional** (`002` F4).
  `RenderContext`'s constructor does not set up the process;
  `moonray`'s own `main` calls this first. Skip it and `initialize`
  SIGSEGVs inside *logging*, with a backtrace pointing nowhere near
  the cause.
- **One renderer per process** (`002` F4). MoonRay's driver state is
  global. Two live ones abort in the allocator. The shim refuses the
  second and says why; sequential use is fine.
- **A scene with no camera crashes MoonRay** (`002` F5).
  `initialize` indexes `getActiveCameras()[0]` on an empty vector,
  which is undefined behaviour rather than the `KeyError` its own
  `catch` waits for. A camera-less ɴsɪ scene is legal, so the flush
  now emits a default camera. Written up in `upstream/` to send.
- **`setSceneUpdated` is not what carries an edit across** (`002` F6).
  It reads like the load-bearing call and is not: a transform and a
  shader parameter both reach the render without it. Two tests
  asserted otherwise and both failed. It is still called; nothing
  claims more than that.
- **MoonRay's cost counters read zero unless read before
  `stopFrame`** (`002` F8), which resets them. Three wrong hypotheses
  were eliminated before that one.
- **A `sourcemodels` edge need not point at geometry.** ɴsɪ connects
  the model *root*, usually a transform. `references` named it, the
  attribute failed to set, and an instanced scene rendered **nothing**
  from a perfectly valid description.
- **A planar cage subdivides to itself.** The first limit-surface test
  used a flat grid and both renders covered exactly 3598 pixels. The
  subject has to be closed and non-planar for the limit surface to
  differ at the silhouette.
- **The `cdylib` goes stale under `cargo test`** (`002` F7).
  `tests/dropin.rs` `dlopen`s it by path, so Cargo does not know the
  test depends on it. This cost two debugging sessions and both times
  pointed somewhere else entirely: a stale artefact looks like a bug
  you have already fixed. `cargo build --lib` first.

## What Would Bite You

**The incremental path is measured now, and the measurement says
"not yet".** A shader edit, a transform, a deformation and a hide each
cross in one synchronise and are asserted on pixels -- but
`Session::last_cost` says every one of them re-tessellates. Two
different reasons, and only one is ours to fix:

- A **material** change re-tessellates *by design*. `Layer.cc:497`
  marks the geometry changed because MoonRay does not know which
  primitive attributes the new material wants until after the update.
  Conservative, upstream's, and nothing a backend can map around.
- A **visibility** change should not, and does. `scene_rdl2`'s
  `Geometry::requiresGeometryUpdate` never consults
  `FLAGS_GEOM_RELOAD_BVH_ONLY`, so the cheap tier `F3` promises is
  unreachable from any path. Two lines, written up in `upstream/`.

The counters had to be read **before `stopFrame`**, which resets them,
and on a scene heavy enough that tessellating it costs milliseconds
rather than microseconds. Both of those made them look like they were
never wired up at all.


**Two ways a perfectly correct scene renders black**, both found by
rendering rather than by reasoning, and both now fixed:

- MoonRay renders what the `Layer` names. Geometry left out of it is
  not dim -- it is absent.
- MoonRay skips a `Layer` row whose **material column is `undef()`**.
  The same triangle is missing from the image without a material and
  present with one, which is why unshaded geometry gets a default
  `UsdPreviewSurface` rather than an honest-looking `undef()`.

Neither produces a warning. Both produce a black image from a file that
parses, round-trips and reads correctly. Expect more of this shape, and
expect to find it the same way.

**The renderer build is heavy but it does work.** MoonRay built from
source here in about fifty minutes on four cores; `quickstart.md` has
the recipe and the five packaging problems that stop it. Performance is
still unmeasured and unclaimed anywhere.

**Two motion samples, not many.** rdl2's `AttributeTimestep` is
`TIMESTEP_BEGIN` and `TIMESTEP_END`. ɴsɪ has no such limit, so a scene
with three samples on one attribute cannot be carried across. Report
the reduction; do not quietly keep the first and last.

**`is_subd` defaults to true.** An ɴsɪ `mesh` that does not set it
false explicitly renders as a subdivision surface.

**ɴsɪ always returns an image.** Report limitations; never refuse a
scene. A render farm depends on it.

**Inherited top risk: silent material misbinding.** A misclassified ɴsɪ
connection does not error, it renders with materials on the wrong
shapes. `T1.4` is the two-shape, two-material test that catches it, and
nothing earlier can.

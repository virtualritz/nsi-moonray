# Research: Interactive Updates

Read from `OpenMoonRay/moonray` and `OpenMoonRay/scene_rdl2`, both built
locally, on 2026-09-06. Every finding cites the file it came from.

The question: an application edits one attribute on one node, or severs
one connection, and expects the renderer to apply *that* and carry on —
reusing the tessellation, the accelerator and everything else it has
already built. Does MoonRay support it?

**Yes, and at a finer grain than ɴsɪ asks for.**

## F1: rdl2 tracks what changed, per object and per attribute

`SceneObject` buffers edits between `beginUpdate()` and `endUpdate()`
and keeps an `mAttributeUpdateMask` (`SceneObject.h`). After an update
the renderer can ask `hasChanged(key)` and `hasBindingChanged(key)` for
any single attribute.

`Layer` accumulates the consequences: `getChangedRootShaders()`,
`getChangedGeometryToRootShaders()` and `getChangedOrDeformedGeometries()`
(`Layer.h`), and `SceneContext` answers
`getUpdatedOrDeformedGeometrySets(layer, sets)` (`SceneContext.h`).

`SceneContext::applyUpdates(layer)` is what turns buffered edits into
those answers, and `resetUpdates(layer)` clears them for the next round.

## F2: MoonRay regenerates only what changed

`RenderContext::startFrame` (`rndr/RenderContext.cc`) picks a change
flag: `ChangeFlag::ALL` on the first frame, otherwise
`ChangeFlag::UPDATE` — unless the frame number, motion blur settings,
a global toggle **or the camera** changed, each of which forces a full
reload. The source is explicit about the camera: *"A camera change
requires a geometry rebuild, our definition of render space depends on
this."*

Under `ChangeFlag::UPDATE`, `GeometryManager::loadGeometries` asks the
layer for `getChangedGeometryToRootShaders()` and generates only those.
The accelerator follows the same rule, and the comment says it outright:

```cpp
// if a primitive is not (re)generated, we don't want to
// (re)tessellate it.
```

with `getUpdatedOrDeformedGeometrySets` deciding which sets are rebuilt
(`rt/GeometryManager.cc`).

## F3: Attributes declare their own update cost

This is the part that decides how an ɴsɪ edit should be *mapped*, not
just whether it can be. `Types.h`:

```cpp
FLAGS_CAN_SKIP_GEOM_RELOAD = 1 << 4,
FLAGS_GEOM_RELOAD_BVH_ONLY = 1 << 5
```

So each attribute is one of three costs: no geometry work, rebuild the
accelerator only, or regenerate the procedural and re-tessellate.

`Geometry`'s visibility flags — `visible_in_camera`, `visible_shadow`,
`visible_diffuse_reflection` and the rest — are all declared
`FLAGS_GEOM_RELOAD_BVH_ONLY` (`Geometry.cc`).

**Consequence for the ɴsɪ mapping:** turning a piece of geometry off by
severing its connection to `.root` should become a visibility change,
not a `Layer` edit or a deleted object. Same visible result, and it
costs a BVH rebuild instead of a re-tessellation.

## F4: There is a delta wire format as well

`RenderContext::updateScene(manifest, payload)` applies *binary rdl2
deltas* between frames (`rndr/RenderContext.h`), and
`BinaryWriter::setDeltaEncoding(true)` / `AsciiWriter::setDeltaEncoding`
produce them. `setSceneUpdated()` covers the other case, where the
`SceneContext` was modified in process.

So an out-of-process arrangement is possible later without changing the
mapping: the same edits, serialised.

## What This Costs Us

The `.rdla`-first decision in `001`'s `research.md` was made when no
host could build the renderer, and it does not survive this. Writing a
whole scene as text and spawning a process cannot express "this one
attribute changed" — every edit is a full reload of everything, and the
renderer starts from nothing each time.

The path has to be: an in-process `SceneContext`, edited in place. See
`plan.md`.

## What `nsi-intermediate` Already Does

This section asked for three things upstream. All three exist:

- `Scene::changes()` / `take_changes()` returns a **net** `Changes`
  record — created, deleted with the type they had, `(handle,
  attribute)` pairs set, and edges added, removed and **re-armed**.
  That last one is a connection whose arguments a repeated `connect`
  replaced in place: no edge appeared or disappeared, but ɴsɪ's
  `priority` rides on those arguments and decides which of two shaders
  wins. A journal keyed on additions and removals misses it silently.
- `Scene::affected(&Changes)` walks *down* from those handles and
  returns `Affected { nodes, shaders, outputs, everything }` — the
  inverse of the climb resolution already does, deliberately
  over-approximate, keyed by handle.
- Motion samples resolve: `motion_times`, `world_transform_samples`
  and `world_transform_interpolated_at`, the last interpolating
  element-wise and holding the ends, which is what 3Delight does.

**`Affected` splits `shaders` from `nodes` because a shader edit costs
no geometry work.** That is MoonRay's own distinction, arrived at
independently — F3's three cost tiers. The two models line up, and this
backend has to be taught to use them.

## F4: Two things MoonRay needs that no header says

Both found by running it, both fatal, and neither produces a
diagnostic that points anywhere near its cause. Read from
`rndr/RenderContext.cc` and `mcrt_common/AffinityManager.h` on a
MoonRay built here.

### `initGlobalDriver` is not optional

`RenderContext`'s constructor does not set up the process. A consumer
must call `rndr::initGlobalDriver(options)` first -- `moonray`'s own
`main` does, at `cmd/raas_cmd/moonray/moonray.cc:117`. It starts
`ProcKeeper`, the `AffinityManager`, the render driver's thread-local
pools and the image-write driver.

Skipping it does not fail cleanly. `RenderContext::initialize` reaches
`RenderStats::logHardwareConfiguration`, which asks
`AffinityManager::get()` for a manager nothing ever made, and the
process dies with **SIGSEGV inside `setupLogInfo`** -- no message, no
exception to catch, and a backtrace pointing at *logging*. It is
declared in `rndr/RenderContext.h`, so it is public API; it is simply
not something the type's own construction implies.

### One renderer per process

That driver state is global, not per-context. Two live
`RenderContext`s share what one is already using, and the failure is
an **abort in the allocator** -- `munmap_chunk(): invalid pointer` --
which names nothing involved.

Sequential use is fine: create, render, drop, create again. So the
shim refuses a second live renderer rather than leaving the abort
waiting, and says why. An application that wants two concurrent
MoonRay renders needs two processes, which is a real constraint on
what this backend can offer and is better stated than discovered.

## F5: A scene with no camera crashes MoonRay

`RenderContext::initialize` (`rndr/RenderContext.cc:434`):

```cpp
try {
    std::vector<const rdl2::Camera*> cameras =
        mSceneContext->getActiveCameras();
    initActiveCamera(cameras[0]);
} catch (scene_rdl2::except::KeyError& e) {
    ...
}
```

`SceneContext::getActiveCameras` returns an **empty vector** when the
context holds no camera (`SceneContext.cc:245`), and `operator[]` on an
empty vector is undefined behaviour rather than an exception -- so the
`catch` wrapped around it never fires. The process dies with a SIGSEGV
inside `initialize`, and in a renderer loaded by `dlopen` it takes the
host application down with it.

An ɴsɪ scene with no `perspectivecamera` connected is legal to record
and an ordinary thing to hand a renderer -- `tests/dropin.rs` builds
exactly one, being a test of the C entry points rather than of a
render. So the check belongs on this side of the boundary, ahead of the
call, and `nmr_render_initialize` refuses such a scene with a message
instead. The C API then falls back to the spawned binary, which reports
rather than crashing.

Upstream would fix it with `.at(0)`, or by testing `empty()`. Worth
sending.

**The general shape is worth remembering**: MoonRay's own entry points
assume a scene assembled by MoonRay's own front end. A backend feeding
it scenes built from somewhere else will keep finding these, and each
one is a crash rather than an error. Guard at the shim, where one check
protects every caller.

## F6: `setSceneUpdated` is not what carries an edit across

`RenderContext::startFrame` branches three ways: the first frame
builds everything; `mSceneUpdated` runs `applyUpdates`, which calls
`update()` on every `SceneObject` and rebuilds the attribute tables
for shaders the layer says changed; neither reuses everything.

`mSceneUpdated` is set only by MoonRay's own update entry points --
`updateScene(manifest, payload)`, `updateScene(filename)`,
`updateGeometry`. A scene edited directly through a live
`SceneContext` sets none of them, and `setSceneUpdated`'s own comment
says it is "for when the SceneContext is modified externally". So it
reads like the load-bearing call.

**It is not what makes an edit visible.** Two tests asserted that it
was, and both failed:

- A **transform** edit reaches the image without it. `GeometryManager`
  recomputes geometry-to-render matrices from `node_xform` as it
  loads geometry.
- A **shader parameter** edit reaches the image without it. A
  material's parameters are read from the rdl2 object at shade time
  rather than from anything `applyUpdates` rebuilds. The quad went
  from red to green with the mark deliberately withheld.

So `scene_updated()` is still called on the real path -- it is the
documented hook, it costs nothing, and `applyUpdates` is what rebuilds
primitive-attribute tables and flags geometry for reload, which
neither of those two edits needs. But nothing may claim it is what
carries an edit across, and no test may assert that its absence hides
one.

**Where the difference would actually show is cost, not pixels**, and
that is `I5`: what got regenerated. This is the same trap the contract
already names -- "marked but not applied" is a full rebuild that
renders correctly and slowly, and only timing finds it. It now has a
sibling: *unmarked and applied anyway*, which renders correctly and
may or may not be doing more work than it needs.

## F7: The `cdylib` goes stale under `cargo test`

`tests/dropin.rs` `dlopen`s `target/debug/libnsi_moonray.so` by path,
so Cargo does not know the test depends on it. A `cargo test` run
after editing `src/capi.rs` can load the *previous* library.

This cost two debugging sessions, and both times the symptom pointed
somewhere else entirely: once a SIGSEGV whose backtrace named a
function whose signature had already been changed, once a missing file
whose writer had already been added. A stale artefact does not look
like a stale artefact; it looks like a bug you have already fixed.

`cargo build --lib` before `cargo test` is the reliable order.

## F8: MoonRay's cost counters do not accumulate for a library consumer

`I5` -- "assert the cost, not only the pixels" -- needs a number that
says whether an edit re-tessellated anything, because a synchronise
that rebuilds the whole scene renders exactly the right image, slightly
later, and no pixel test can tell the difference.

`RenderStats` looks like the answer and is reachable:
`RenderContext::getSceneRenderStats()` is public, its counters are
public (`// stats are public for ease of access`), and
`RenderContext.cc:2605` copies `mTessellationTime`,
`mPerPrimitiveTessellationTime`, `mBuildAcceleratorTime` and the rest
across from the `GeometryManager` after every `finalizeChanges`.

**They do not move.** Adding a second shape to a live session -- first
a polygon mesh, then a subdivision surface, which certainly
tessellates -- leaves `primitives_tessellated` at 1 and every timer at
`0.0`. The shape renders correctly, so the incremental path is fine;
it is the measurement that is not there. The likely cause is that the
timers are gated on stats logging a library consumer does not enable,
but that was not chased down.

Three things were tried and ruled out:

- **A subdivision surface** rather than a polygon mesh, in case
  polygons never reach `GeometryManager::tessellate`. No change.
- **`log_info` on the scene variables**, in case the timers are gated
  on stats logging. No change -- and note that what *is* gated on the
  log flags is `reportGeometryTessellationTime`, the reporting, not
  the accumulation (`RenderContext.cc:2006`).
- **Checking the shape actually arrived.** It does: the left of frame
  goes from `0` to `11.2` when the second shape is added, so the
  incremental path is correct and it is only the measurement that is
  missing. This was worth checking rather than assuming -- the first
  version of the check asked whether the left column was non-zero,
  which the environment light satisfies on its own.

The next thing to look at is `RenderContext.cc:2001`: `loadGeometries`
-- which is where the stats are copied across from the
`GeometryManager` -- runs only `if (geomChangeFlag != ChangeFlag::NONE)`.
Even the *first* frame reports `load_procedurals: 0.0` here, so
something more basic than the incremental path is not wiring these up.

`Render::cost` is kept, because the fields are real and the accessor is
right. What is *not* kept is an assertion built on it: a test saying "a
shader edit costs no geometry work" would pass because nothing ever
moves, which is worse than no assertion at all. The control test --
"adding geometry shows up in the counters" -- is committed and
`#[ignore]`d, so `cargo test -- --ignored` says whether this is still
true.

`I5` stays open, and it is the last thing standing between "the
incremental path renders the right image" and "the incremental path is
actually incremental".

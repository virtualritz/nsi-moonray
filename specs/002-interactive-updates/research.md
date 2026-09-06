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

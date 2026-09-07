# Plan: Interactive Updates

## Status

Specified, not built.

**Linking MoonRay directly is the first thing, not a later one.**
`001` ships a backend that *spawns* `moonray` and hands it a `.rdla`.
That was the right first step and it is now the thing in the way: a
separate process has no `SceneContext` to edit and no `RenderContext`
to snapshot, so it forecloses every capability below at once —
incremental updates, progressive delivery, and the render running
while the application carries on. None of those are separate features
waiting their turn. They are one prerequisite.

So the order is: **shim, then in-process render, then everything
else.** The spawn path stays reachable for batch work and stops being
the path.

### What `.rdla` is now

A dump, and only a dump — `--print`, a bug report, a diff against the
oracle. It is not a transport and nothing on the render path writes
one. The oracle tests keep covering it *because* it is a dump: they
check the values this backend computes without needing a renderer, and
they go on doing that after the transport changes underneath them.

## Approach

```
ɴsɪ calls ──▶ nsi-intermediate ──▶ nsi-moonray ──▶ scene_rdl2 ──▶ MoonRay
              (records + journals)   (applies)     SceneContext    RenderContext
                                          │                             │
                                          │                     snapshot│
                    .rdla / .rdlb ◀───────┘                             ▼
                    a *dump*, off the render path        callback.write
```

All of it in one process, in one address space. The application's
closure and the renderer's sample buffer are a `memcpy` apart.

### 1. An in-process `SceneContext`

An `extern "C"` shim over `scene_rdl2` — the option `001` weighed and
deferred. Its reasoning ("the shim buys nothing until a host can build
the renderer") expired when MoonRay built; `002` needs it regardless,
because a text file cannot express an edit.

The `Document` the flush already produces is the right shape for this:
an in-memory description of rdl2 objects, sets, `Layer` rows and typed
values. Writing it as `.rdla` becomes one consumer of that structure and
replaying it into live `SceneObject`s becomes another. The oracle tests
keep covering the text path unchanged, which is what keeps the *values*
honest while the transport changes underneath.

### 2. Incremental apply

An ɴsɪ edit becomes the narrowest rdl2 edit that expresses it, inside
`beginUpdate()`/`endUpdate()`, then `RenderContext::setSceneUpdated()`
and `startFrame()`.

The mapping choices that matter are cost choices (`research.md` F3):

| ɴsɪ edit | rdl2 edit | Costs |
| --- | --- | --- |
| attribute on a `shader` | attribute on the `Material` | no geometry work |
| `transform` matrix | `node_xform` on the geometry | accelerator |
| geometry disconnected from `.root` | `visible_in_camera` and friends **false** | accelerator |
| `P` on a `mesh` | `vertex_list_0` | regenerate, re-tessellate |
| camera changed | `SceneVariables` camera | **full reload** — MoonRay says render space depends on it |

Turning geometry off through visibility rather than by editing the
`Layer` or deleting the object is the whole point of reading F3 first:
same image, one tier cheaper.

### 3. Pixels out

`RenderContext` snapshots into buffers rather than files. The
receiving end is **already built and tested** (`001` `T5.1`): an
application's `callback.open`/`write`/`finish` closures are called
directly, no ndspy marshalling. What is missing is only the source of
the pixels.

MoonRay renders progressively already (`001` `research.md` F5); it
expects to be *pulled* (`snapshotDelta` + `ActivePixels`) where ɴsɪ
pushes. The adapter is a snapshot loop here, and it is small. It has
no `RenderContext` to loop over until §1 and `R1` exist, which is the
only reason it does not exist today.

## What Upstream Provides

Everything this plan asked of `nsi-intermediate` is already there:
`Scene::take_changes()` for the journal and `Scene::affected()` for the
walk down. See `research.md`.

So the remaining work is entirely here: hold a live `SceneContext`,
and turn an `Affected` batch into the narrowest rdl2 edits that express
it.

## Gates

| Gate | Met |
| --- | --- |
| MoonRay is linked, not spawned | **yes** — `tests/inprocess.rs` renders a quad through a linked `RenderContext`; no file, no process |
| A `Document` reaches a live `SceneContext` with no file | **yes** — `src/apply.rs`, `tests/apply.rs` |
| A scene renders through `RenderContext` in process | **yes** — `R1` |
| One attribute edit restarts a render without re-tessellation | **half** — the edit is narrowed and the image changes (`I1`); that MoonRay re-tessellated nothing is unasserted (`I5`) |
| Geometry turned off costs an accelerator rebuild, not a reload | no |
| Pixels reach a display driver | **yes** — `src/stream.rs`, as the frame converges |
| `nsi-intermediate` reports what changed | no — upstream |

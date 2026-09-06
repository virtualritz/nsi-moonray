# Plan: Interactive Updates

## Status

Specified, not built. The three layers below are the work, and the
first is a prerequisite for anything interactive at all.

## Approach

```
ɴsɪ calls ──▶ nsi-intermediate ──▶ nsi-moonray ──▶ scene_rdl2 ──▶ MoonRay
              (records + journals)   (applies)     SceneContext    RenderContext
                                          │
                                          └──▶ .rdla / .rdlb, an *output*
```

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

`RenderContext` snapshots into buffers rather than files, which is what
feeds a display driver and what `DspyRegisterDriver` currently has
nothing to do. Progressive modes come with it.

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
| A `Document` reaches a live `SceneContext` with no file | no |
| A scene renders through `RenderContext` in process | no |
| One attribute edit restarts a render without re-tessellation | no |
| Geometry turned off costs an accelerator rebuild, not a reload | no |
| Pixels reach a display driver | no |
| `nsi-intermediate` reports what changed | no — upstream |

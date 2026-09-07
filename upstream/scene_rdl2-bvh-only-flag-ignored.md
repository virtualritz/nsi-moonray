<!--
Ready to file at https://github.com/OpenMoonRay/scene_rdl2/issues/new

Title: Geometry::requiresGeometryUpdate ignores FLAGS_GEOM_RELOAD_BVH_ONLY,
       so a visibility change forces a full geometry reload

Not filed from here: this session's GitHub access is scoped to
`virtualritz`, and the MoonRay projects are on another tier.

Measured rather than reasoned: see
`specs/002-interactive-updates/research.md` F9, and the test
`a_synchronise_is_measured_not_assumed`.
-->

# `requiresGeometryUpdate` ignores `FLAGS_GEOM_RELOAD_BVH_ONLY`

## Summary

`Geometry::requiresGeometryUpdate` consults `FLAGS_CAN_SKIP_GEOM_RELOAD` but not `FLAGS_GEOM_RELOAD_BVH_ONLY`, so changing a visibility attribute puts the geometry into the changed-geometry list and it is regenerated and re-tessellated. `Attribute::updateOnlyRequiresBVHRebuild()` exists for exactly this case and has one caller in either repository — on a code path that only MoonRay's own `updateScene` entry points reach.

## Where

`lib/scene/rdl2/Geometry.cc:232`:

```cpp
bool
Geometry::requiresGeometryUpdate(UpdateHelper& sceneObjects, int depth)
{
    for (auto it = mSceneClass.beginAttributes();
        it != mSceneClass.endAttributes(); ++it) {
        const Attribute* attribute = *it;
        // some attributes (like "ray_epsilon") updates would not change the geometry
        if (!attribute->updateRequiresGeomReload()) {
            continue;
        }
        // whether this attribute or its dependency is dirtied
        bool updateRequired = hasChanged(attribute) || hasBindingChanged(attribute);
        if (updateRequired) {
            return true;
        }
        ...
```

`updateRequiresGeomReload()` is false only for `FLAGS_CAN_SKIP_GEOM_RELOAD` (`Attribute.h:365`). The nine `visible_*` attributes on `Geometry` are declared `FLAGS_GEOM_RELOAD_BVH_ONLY` *without* it (`Geometry.cc:89-147`), so `updateRequiresGeomReload()` returns true for them and a changed visibility flag returns `true` here.

The predicate that would answer correctly is right beside it, `Attribute.h:372`:

```cpp
bool
Attribute::updateOnlyRequiresBVHRebuild() const
{
    return ((mFlags & FLAGS_GEOM_RELOAD_BVH_ONLY) != 0) &&
           ((mFlags & FLAGS_CAN_SKIP_GEOM_RELOAD) == 0);
}
```

## Why I think this is unintended

MoonRay's `RenderContext::checkGeometryChangesRequireReload` (`moonray`, `lib/rendering/rndr/RenderContext.cc:712`) asks the same question and *does* consult it, with a comment that reads as the specification for the flag:

```cpp
//   - FLAGS_GEOM_RELOAD_BVH_ONLY: change needs a BVH rebuild but no
//     reload (e.g. the visibility flags, baked into the BVH ray mask).
if (!attribute->updateRequiresGeomReload() ||
    attribute->updateOnlyRequiresBVHRebuild()) {
    continue;
}
```

That function is reached only from `RenderContext::updateScene(manifest, payload)` and `updateScene(filename)`. A consumer that edits the `SceneContext` in place and calls `setSceneUpdated()` — which is what `setSceneUpdated`'s own comment invites, "for when the SceneContext is modified externally" — never reaches it, and pays a full reload for a flag that is documented as needing only a BVH rebuild.

## Measurement

Driving `RenderContext` as a library with a scene built in memory, on a Catmull-Clark subdivided 40×40 grid (1600 faces), reading `RenderStats` before `stopFrame` resets it:

| frame | `mTessellationTime` |
| --- | --- |
| first | 0.0101 s |
| after a material colour change | 0.0092 s |
| after `visible_in_camera` etc. set false | 0.0094 s |

The material case is expected — `Layer.cc:497` marks the geometry changed deliberately, since the new material's primitive-attribute requests are not known yet. The visibility case is the one this issue is about: it should be a BVH rebuild.

## Suggested fix

```cpp
if (!attribute->updateRequiresGeomReload() ||
    attribute->updateOnlyRequiresBVHRebuild()) {
    continue;
}
```

— the same two lines MoonRay already uses.

As far as I can tell this is safe, because regeneration and the BVH rebuild are driven by *different* lists:

- `mChangedOrDeformedGeometries` being non-empty is what triggers the BVH rebuild (`moonray`, `GeometryManager.cc:1117` returns early only when it is empty).
- `Layer::getChangedGeometryToRootShaders` is what drives regeneration, and it filters that same list by `geom->updateRequired()` (`Layer.cc:775`) — which is what `requiresGeometryUpdate` sets through `mAttributeTreeChanged`.

`Layer.cc:766` describes exactly the behaviour the fix would produce:

> An attribute that requires a geometry update changes. Note that if an attribute changes that does not require a geometry update, special care is taken to set the `mAttributeTreeChanged` flag to false.

So the geometry would stay in `mChangedOrDeformedGeometries` and its BVH would still be rebuilt, while `updateRequired()` went false and it would not be regenerated. That said, you know the invariants here and I do not — happy to send a PR if the shape looks right to you.

## Context

Found while writing an ɴsɪ (Nodal Scene Interface) backend on MoonRay. ɴsɪ turns geometry off by severing its connection to the scene root, which maps naturally onto the visibility flags — and the mapping was chosen *because* those flags are declared `BVH_ONLY`, so hiding a shape would cost an accelerator rebuild rather than a re-tessellation. It does not, currently.

## Environment

- `scene_rdl2` and `moonray` at `eef67ae`, Ubuntu 24.04, GCC 13.3, Release
- `-DMOONRAY_USE_OPTIX=NO`, CPU-only OpenSubdiv 3.5

---
_Generated by [Claude Code](https://claude.ai/code)_

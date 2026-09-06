# Tasks: Interactive Updates

Nothing built. Ordered so that each step is checkable on its own.

## The Shim

- [ ] S1 An `extern "C"` surface over `scene_rdl2`: create a
      `SceneContext`, create objects by class and name, set the typed
      attributes rdl2 has, set bindings and blur pairs, `Layer::assign`,
      set membership.
- [ ] S2 `apply(&Document, &Context)` — the same structure the `.rdla`
      writer consumes, replayed into live objects.
- [ ] S3 `AsciiWriter`/`BinaryWriter` behind the shim, so a live context
      can be dumped. This is what keeps `.rdla` honest once it is no
      longer the path: dump the live scene, diff against the emitter.
- [ ] S4 Gate all of it behind a `rdl2` feature. A checkout with no
      `scene_rdl2` still builds and still runs the oracle tests.

## In-Process Rendering

- [ ] R1 `RenderContext`: `startFrame`, `stopFrame`, snapshot to a
      buffer. No file.
- [ ] R2 Progressive modes, and the render running while the
      application carries on.
- [ ] R3 Pixels to a registered display driver, which is what
      `DspyRegisterDriver` has nothing to do with today.

## Incremental Apply

- [ ] I1 Apply one attribute edit inside `beginUpdate`/`endUpdate`,
      `setSceneUpdated`, restart. Assert the image changed.
- [ ] I2 Geometry off through **visibility**, not a `Layer` edit or a
      delete: `research.md` F3 says that is one cost tier cheaper.
- [ ] I3 A moved transform touches only its own geometry.
- [ ] I4 Deformation: `P` changes, and only that mesh regenerates.
- [ ] I5 Assert the *cost*, not only the pixels. MoonRay logs what it
      regenerated; a test that only checks the image passes on a full
      rebuild.
- [ ] I6 Fall back to a full rebuild for anything not mapped, and
      report it. Never fail a synchronise.

## Upstream — **done**

All three landed in `nsi-intermediate` before this spec was a day old.
What is left is consuming them.

- [x] U1 A journal. `Scene::changes()` / `take_changes()` returns
      `Changes`: created, deleted (with the type they had), the
      `(handle, attribute)` pairs set, and edges added, removed **and
      re-armed** — that last one being a connection whose arguments a
      repeated `connect` replaced, which carries ɴsɪ's `priority` and
      so changes which shader wins without any edge appearing or
      disappearing. A record keyed on additions and removals misses it
      entirely. It is a *net* record carrying no values, which is what
      a synchronise wants.
- [x] U2, U3 Dirty propagation. `Scene::affected(&Changes)` returns
      `Affected { nodes, shaders, outputs, everything }`, walking down
      the transform tree and through `attributes` bindings — the
      inverse of the climb resolution already does.
- [ ] U5 **Consume them.** `Affected` separates `shaders` from `nodes`
      because a shader edit costs no geometry work — which is exactly
      MoonRay's own distinction (`research.md` F3). The two models line
      up; this backend has to be told to use them.

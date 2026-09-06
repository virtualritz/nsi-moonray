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

## Upstream

- [ ] U1 **A journal in `nsi-intermediate`**: creates, deletes,
      attribute sets, connection changes since the last synchronise,
      and a way to clear it.
- [ ] U2 Dirty propagation down the transform tree.
- [ ] U3 Dirty propagation through `attributes` bindings.
- [ ] U4 Until U1 exists, a whole-scene diff in this backend, so the
      interactive path can be built and tested against something.

# Tasks: Interactive Updates

Nothing built. Ordered so that each step is checkable on its own.

## The Shim

- [x] S1 An `extern "C"` surface over `scene_rdl2`: `shim/`. Creates a
      `SceneContext`, creates objects by class and name, sets every
      typed attribute rdl2 has, sets bindings and blur pairs, assigns
      `Layer` rows and set membership. **Checked by running it** --
      `shim/tests/smoke.cc` drives every setter through rdl2's own
      `ExtensiveObject` and writes the result out, which is how the
      calls were confirmed against the library rather than against its
      headers. Two rules hold at every entry point: no C++ exception
      crosses the boundary (`set` by name throws, and unwinding into
      Rust is undefined behaviour), and nothing refuses a scene.
- [x] S2 `apply(&Document, &Context)` -- `src/apply.rs`. Two passes,
      because an attribute can name an object the document declares
      later and one pass would resolve it to `undef()`; for a `Layer`
      row's material that means MoonRay skips the shape entirely, so
      the failure is a black render rather than an error.
- [x] S3 `AsciiWriter` behind the shim: `Context::write_ascii`. What
      keeps `.rdla` honest now that it is a dump -- apply a document,
      write the live scene, diff against the emitter.
- [x] S4 Gated behind a `rdl2` feature, off by default. A checkout with
      no `scene_rdl2` builds and runs everything but `tests/apply.rs`.
      `cc` is an unconditional build-dependency: a build script sees
      features only as `CARGO_FEATURE_*`, never as a `cfg`.
- [x] S5 A round trip against the emitter. `oracle verify` proves rdl2
      reads what the emitter writes; the twin check is that applying a
      document and dumping it gives the same bytes. Any divergence is a
      setter that took a different path from the writer, and nothing
      else would find it.

## In-Process Rendering

- [x] R1 `RenderContext`: `startFrame`, `stopFrame`, snapshot to a
      buffer. **No file, no spawned process.** `shim/src/render.cc` and
      `src/rdl2/render.rs`; `tests/inprocess.rs` renders a lit quad and
      asserts the centre of frame carries light and that a tenth of the
      frame does -- "any pixel is non-zero" would pass on a stray
      sample. Two things had to be found by running it, neither of
      which fails legibly (`research.md` F4): `initGlobalDriver` must
      be called before any `RenderContext` or the process SIGSEGVs
      inside *logging*, and only one renderer may live at a time or two
      abort in the allocator.
      **The renderer owns the scene.** `getSceneContext()` hands out a
      reference to its own, so a scene meant to be rendered is built
      inside the renderer rather than built separately and handed
      across.
- [x] R2 Progressive modes. `Mode::{Batch, Progressive,
      ProgressiveFast, Realtime}`, set before `initGlobalDriver`
      because that reads the mode to size the thread-local pools.
      `startFrame` returns once render *prep* is done with the frame
      converging behind it, which
      `a_frame_can_be_snapshotted_while_it_converges` pins down.
- [x] R3 Pixels to a registered display driver **as the render
      converges**. `src/stream.rs`. Half of this is already done and is not the hard
      half: an application's `callback.open`/`write`/`finish` closures
      are reached with no ndspy marshalling at all
      ([`001`'s `display.md`](../001-moonray-backend/contracts/display.md),
      `T5.1`). What is missing is the source of the pixels —
      `snapshotDelta` against a live `RenderContext` and its
      `ActivePixels`, instead of one bucket read back off the file a
      batch render wrote — and honouring a closure that answers
      `Error::Stop`, which today has nothing left to stop. MoonRay
      already renders progressively; the only reason this backend
      cannot follow along is that it *spawns* the renderer, and a
      separate process has no `RenderContext` to snapshot. `R1` is the
      unblock. `T5.3`.

## Incremental Apply

Driven by `Session` (`src/session.rs`), which is where the loop lives;
`capi` is a thin shim over it, so an application reaches the same code
whether it embeds this crate or `dlopen`s it.

- [x] I1 Apply one attribute edit, restart, assert the *image*
      changed. `Session::synchronize`; `tests/incremental.rs`. A shader
      parameter and a transform both cross.
      `setSceneUpdated` turned out **not** to be what carries these
      across -- two tests asserted it was and both failed. See
      `research.md` F6; it is still called, and nothing claims more
      than that.
- [x] I2 Geometry off through **visibility**, not a `Layer` edit or a
      delete: `research.md` F3 says that is one cost tier cheaper.
      All nine `visible_*` attributes, read from `Geometry.cc` --
      setting only `visible_in_camera` leaves a shape casting shadows
      and appearing in reflections, which reads as a lighting bug.
      `disconnecting_a_shape_turns_it_off_without_a_rebuild`.
      **This found a real bug**: the flush walks every recorded node
      rather than only the reachable ones, so a shape disconnected from
      `.root` went on rendering, at identity. Correct scene, successful
      render, shape that should be gone sitting at the origin.
- [~] I3 A moved transform. `a_session_moves_a_shape` renders the
      quad in its new place through one synchronise, and
      `apply_affected` re-sends only what upstream's `Affected` named.
      What is *not* asserted is that the other geometry was untouched
      -- that is `I5`, and an image cannot show it.
- [~] I4 Deformation: `P` changes and the shape moves.
      `a_deformation_edit_moves_the_vertices` -- a narrow edit, not a
      rebuild. That *only that mesh* regenerates is `I5`'s to prove,
      and this is the case that should cost the expensive tier:
      `vertex_list_*` is not `FLAGS_CAN_SKIP_GEOM_RELOAD`.
- [~] I5 Assert the *cost*, not only the pixels. **Measurable now, and
      the measurement found a gap.** The counters had to be read before
      `stopFrame` resets them (`Session::last_cost`), and the scene had
      to be heavy enough that tessellating it costs milliseconds rather
      than microseconds -- a subdivided 40x40 grid.
      `a_synchronise_is_measured_not_assumed` pins down that the
      counters discriminate, and that a material change re-tessellates
      **by design** (`Layer.cc:497`, quoted in `research.md` F8).
      What is left: a *visibility* edit re-tessellates too, and
      `research.md` F9 says exactly why -- the BVH-only tier is
      implemented in `checkGeometryChangesRequireReload`, which only
      MoonRay's own `updateScene` entry points call, while
      `Geometry::requiresGeometryUpdate` does not consult the flag at
      all. Two ways out, `I7` and an upstream ask.
- [ ] I7 **Nothing to do here; the fix is upstream.** A first reading
      suggested moving to the `updateScene(manifest, payload)` delta
      path to reach the BVH-only tier. That was wrong:
      `checkGeometryChangesRequireReload`'s answer is *returned to the
      caller* for a distributed renderer to act on, and skips no work
      itself, so both paths share the same lists (`research.md` F9).
      Consulting `updateOnlyRequiresBVHRebuild()` in
      `Geometry::requiresGeometryUpdate` is the whole fix, and the
      lists are separate in exactly the way that makes it safe: the
      geometry stays in `mChangedOrDeformedGeometries`, so the BVH
      still rebuilds, while `updateRequired()` goes false so it is not
      regenerated. Report written; `a_synchronise_is_measured_not_assumed`
      is where a fix shows up.
- [x] I6 Fall back to a full re-apply for anything not narrowable,
      and report it. `Affected::everything` (an edit to `.root` or
      `.global`) and any node created or deleted -- set and layer
      membership is not carried by any one object.
      `a_created_node_falls_back_and_reports`.

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
- [x] U5 **Consume them.** `apply_affected` takes upstream's `Changes`
      and `Affected` and re-applies only the objects they name -- plus
      only the *attributes* whose values differ, since rdl2 tracks that
      an attribute was set rather than that it changed, so re-sending a
      mesh's `vertex_list_0` regenerates its geometry either way.
      **The premise this task was written on is wrong, though.**
      `Affected` separates `shaders` from `nodes` because a shader edit
      *should* cost no geometry work, and this task assumed MoonRay
      agreed. It does not: `Layer.cc:497` marks a material's geometry
      changed on any edit, deliberately, because the new material's
      primitive-attribute requests are not known until after the
      update. The two models line up on everything except the case the
      split exists for (`research.md` F8).

# Contract: Applying An Edit

## Scope

Turning one ɴsɪ edit into the narrowest rdl2 edit that expresses it,
and getting MoonRay to redraw from what it already has.

## Matrix

| Behavior | Status | Source Evidence | Test/QA Evidence | Required Next Evidence |
| --- | --- | --- | --- | --- |
| MoonRay applies edits without a full rebuild | **Covered** | `RenderContext::startFrame` picks `ChangeFlag::UPDATE`; `GeometryManager` regenerates only changed geometry and does not re-tessellate what it did not regenerate (`research.md` F1, F2) | Read from a built MoonRay; nothing driven through it yet | -- |
| A `Document` reaches a live `SceneContext` | Open | None | None | Build the shim; assert the objects exist with the values the `.rdla` would have carried. |
| A render runs in process | Open | None | None | `RenderContext::startFrame`/`stopFrame` and a snapshot, with no file written. |
| One attribute edit restarts without re-tessellation | Open | None | None | Edit a shader parameter; assert the pixels change **and** that MoonRay reports no geometry regenerated. Pixels alone would pass on a full rebuild. |
| Geometry turned off costs a BVH rebuild only | Open | `Geometry`'s visibility attributes are `FLAGS_GEOM_RELOAD_BVH_ONLY` (`research.md` F3) | None | Disconnect a shape; assert it leaves the image and that the other shape was not regenerated. |
| A moved transform rebuilds only its own geometry | Open | None | None | Move one of two shapes; assert the other is untouched. |
| Pixels reach a display driver | **Partial** | `display.rs` -- an application's `callback.open`/`write`/`finish` closures are called directly, with no ndspy marshalling; see [`001`'s `display.md`](../../001-moonray-backend/contracts/display.md) | `render::an_applications_callback_receives_the_rendered_pixels` -- but from the file a batch render wrote, so one bucket at the end | `snapshotDelta` against a live `RenderContext`, delivering buckets as they converge and honouring a closure that returns `Error::Stop`. `T5.3` |
| An unsupported edit falls back and says so | Open | None | None | ɴsɪ always returns an image: an edit that cannot be applied incrementally must trigger a full rebuild and report it, never fail the synchronise. |

## Invariants

- **The cost of an edit is MoonRay's cost for it.** A mapping that
  works but re-tessellates where it need not is a defect, not a
  detail — and it looks correct in every image.
- **No ɴsɪ graph knowledge here.** Which rdl2 objects an ɴsɪ edit
  touches is `nsi-intermediate`'s answer to give.
- `.rdla` is an output, not the transport.

## Failure Modes

- **Applied but not marked:** the scene holds the new value, MoonRay
  was not told, and the render shows the old one. No error, no warning.
  Every incremental test must assert the *image* changed.
- **Marked but not applied:** a full rebuild that renders correctly and
  slowly. Only timing catches it.

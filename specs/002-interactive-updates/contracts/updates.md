# Contract: Applying An Edit

## Scope

Turning one ɴsɪ edit into the narrowest rdl2 edit that expresses it,
and getting MoonRay to redraw from what it already has.

## Matrix

| Behavior | Status | Source Evidence | Test/QA Evidence | Required Next Evidence |
| --- | --- | --- | --- | --- |
| MoonRay applies edits without a full rebuild | **Covered** | `RenderContext::startFrame` picks `ChangeFlag::UPDATE`; `GeometryManager` regenerates only changed geometry and does not re-tessellate what it did not regenerate (`research.md` F1, F2) | Read from a built MoonRay; nothing driven through it yet | -- |
| A `Document` reaches a live `SceneContext` | **Covered** | `shim/src/scene.cc` and `src/apply.rs` -- create-or-get by class and name, every typed setter, sets, `Layer` rows, and two-timestep blur | `shim/tests/smoke.cc` drives every setter through rdl2's `ExtensiveObject` against the real library; `tests/apply.rs` applies documents and reads the result back through rdl2's own `AsciiWriter`, rather than against this crate's emitter -- comparing the two consumers of `Document` to each other could pass with both wrong | -- |
| A render runs in process | **Covered** | `shim/src/render.cc` -- `initGlobalDriver`, `RenderContext::startFrame`/`stopFrame`, `snapshotRenderBuffer` into caller memory. MoonRay's buffer is `PixelBuffer<Vec4f>`, so RGBA float per pixel | `tests/inprocess.rs::a_scene_renders_in_this_process` renders a lit quad with no file written and no process spawned, asserting the centre of frame and a tenth of the pixels carry light | -- |
| One attribute edit restarts without re-tessellation | **Partial** | `Session::synchronize` re-applies only what upstream's `Affected` names | `tests/incremental.rs` — a shader edit and a transform each change the image through one synchronise, neither forcing a whole-scene re-apply | The half that matters is unproven: **that MoonRay re-tessellated nothing**. `Render::cost` reads the counters and they do not move (`research.md` F8), so the assertion would pass for the wrong reason. `I5`. |
| Geometry turned off costs a BVH rebuild only | Open | `Geometry`'s visibility attributes are `FLAGS_GEOM_RELOAD_BVH_ONLY` (`research.md` F3) | None | Disconnect a shape; assert it leaves the image and that the other shape was not regenerated. |
| A moved transform rebuilds only its own geometry | Open | None | None | Move one of two shapes; assert the other is untouched. |
| Pixels reach a display driver | **Partial** | `display.rs` -- an application's `callback.open`/`write`/`finish` closures are called directly, with no ndspy marshalling; see [`001`'s `display.md`](../../001-moonray-backend/contracts/display.md) | `render::an_applications_callback_receives_the_rendered_pixels` -- but from the file a batch render wrote, so one bucket at the end | `snapshotDelta` against a live `RenderContext`, delivering buckets as they converge and honouring a closure that returns `Error::Stop`. `T5.3` |
| A scene MoonRay would crash on is refused, not passed | **Partial** | `research.md` F5 — `RenderContext::initialize` indexes `getActiveCameras()[0]` on an empty vector, which is undefined behaviour and not the `KeyError` its own `catch` expects | `nmr_render_initialize` refuses a camera-less scene and the C API falls back to the spawned binary; `tests/dropin.rs` records a camera-less scene and no longer takes the process down | Camera is the one found so far. MoonRay's entry points assume a scene its own front end assembled, so expect more, each a crash rather than an error. |
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

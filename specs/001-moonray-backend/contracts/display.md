# Contract: Getting Pixels Out

## Scope

How a rendered image reaches the application that asked for it, when
that application is an ɴsɪ consumer and the renderer is MoonRay.

**MoonRay delivers pixels progressively. It just does not push
them.** `RenderMode::PROGRESSIVE` puts samples on screen as soon as
they exist (`research.md` F5), and a consumer collects them by
*pulling*: `snapshotRenderBuffer` for a whole frame, `snapshotDelta`
plus `ActivePixels` for only what changed since the last call, paced
by `isFrameReadyForDisplay` / `areCoarsePassesComplete` /
`isFrameComplete`. `moonray_gui` is a loop around exactly that. Files
come from `RenderOutputDriver` and are a separate, batch-side thing.

**An ɴsɪ consumer expects a push**: an `outputdriver` node, and
buckets arriving at its callbacks as they are rendered.

So the two ends do not meet on their own — but the gap is pull-versus-
push, not a missing capability. Nothing needs changing inside MoonRay:
the adapter is a snapshot loop on this side, turning each delta into a
`callback.write`. What it needs is the renderer running **in process**,
because a spawned batch binary has no `RenderContext` to snapshot.

`nsi-ffi-wrap`'s `output` feature is the meeting point, and it is
already Rust on both sides. An application creates an `outputdriver`
with `drivername` `"ferris_f32"` (or the `u32`/`i32`/`u16`/`i16`/`u8`/
`i8` variants) and hands over three closures as `Reference`
attributes — `callback.open`, `callback.write`, `callback.finish`.
Reaching those closures needs no ndspy marshalling at all: the pixels
go from a `&[f32]` to the application's own `Fn`.

So the answer to "can the Rust side look the same as it does under
3Delight?" is yes, for the delivery interface, with one constraint
recorded below.

## Matrix

| Behavior | Status | Source Evidence | Test/QA Evidence | Required Next Evidence |
| --- | --- | --- | --- | --- |
| `Reference` arguments survive recording | **Covered** | `capi.rs` — `Type::Reference` becomes `OwnedData::Reference(HostPtr)`. It never reaches MoonRay (`spec.md` R2) and must still be recorded: this is how the callbacks arrive, and dropping them left a viewport with a driver and nothing to call | `display::tests::a_bucket_reaches_the_applications_closure` reads back a pointer put on the node the way ɴsɪ puts it | -- |
| An application's closure receives the pixels | **Covered** | `display.rs` — `Callbacks::of` reads the three pointers off the `outputdriver` node; `nsi-ffi-wrap` double-boxes each closure so the fat `dyn` pointer fits through the C API, and this reads it back the same way | `render::an_applications_callback_receives_the_rendered_pixels` renders through the backend and asserts the closure receives `width * height * channels` values, not all black | -- |
| The channel names come from the image, not a guess | **Covered** | MoonRay names channels after the ɴsɪ output layer — `Ci.R`, `Ci.G`, `Ci.B` for a beauty pass with no alpha. A reader insisting on `R`,`G`,`B`,`A` finds *no layer at all*, which is what the first version did | `display::deliver_file` reads all channels of the first valid layer and builds the `PixelFormat` from their names | -- |
| A driver that writes a file is left alone | **Covered** | `display.rs` — `Callbacks::of` returns `None` for a node carrying no callback references | `display::tests::a_file_driver_has_no_callbacks` | -- |
| A malformed bucket is refused before the closure | **Covered** | `Callbacks::write` checks `pixels.len()` against `x.len() * y.len() * channels()`; an over-read would otherwise happen inside the application's code | `display::tests::a_short_bucket_is_refused` | -- |
| Pixels arrive *while* the render runs | **Covered** | MoonRay supplies this already, pull-shaped: `RenderMode::PROGRESSIVE` (`research.md` F5) with `snapshotDelta` plus `ActivePixels`, paced by `isFrameReadyForDisplay` / `areCoarsePassesComplete`. Names read from `rndr/RenderContext.h` on a MoonRay built here; that tree is gone with the container, so re-check them against a fresh build before writing code against them | `tests/inprocess.rs::a_converging_render_streams_to_the_applications_closures` composites the buckets into a frame, as a driver does, and reads the result; `a_delta_snapshot_agrees_with_a_full_one` checks the delta path against `snapshot` | -- |
| A driver reached through `dlopen` gets pixels | Open | A `Box<dyn FnWrite>` is a trait object whose vtable belongs to the compilation that made it | None | Register through `DspyRegisterDriver` and deliver over the `extern "C"` entry points instead. `T5.2` |
| **A bucket names only what changed** | **Covered** | `shim/src/render.cc` — `snapshotDelta` plus `ActivePixels`, with the tiling undone and each pixel divided by its own sample weight, since the delta buffer is tiled and unnormalised | `a_delta_snapshot_agrees_with_a_full_one` — a mis-untiled frame is scrambled and an unnormalised one merely darker, and comparing against `snapshot` catches either | -- |
| A driver that says stop, stops | **Covered** | `stream.rs` — a `write` answering `Error::Stop` ends the loop and stops the frame, and the caller is told which of completion, stop or deadline ended it | `tests/inprocess.rs::a_callback_that_says_stop_stops_the_render` | The file-delivery path still discards it, which is harmless there: by the time a finished image is handed over there is nothing left to stop. |

| A batch render writes its file | **Covered** | `shim/src/render.cc` — MoonRay's own `writeImageWithMessage` and `writeRenderOutputsWithMessages`, given the buffers its own `renderOutput` gathers | `inprocess::a_batch_render_writes_the_image_it_was_asked_for` reads the written EXR back and asserts its size and that it is not black | -- |

## Invariants

- **No ndspy structs between the renderer and the application.** The
  one place an ndspy type appears is `PixelFormat::from_ndspy`, which
  is the only public constructor for the format the closures are
  handed.
- **The closures are borrowed, never freed.** ɴsɪ's copy contract
  makes `Reference` the one argument type that is *not* copied: the
  application owns the closure and keeps it alive across the render.
- **The delivery interface is the one it keeps.** What changes when
  the renderer runs in process is where the pixels come from, not what
  receives them.

## The One Constraint

A trait object's vtable belongs to the compilation that produced it.
Calling `Box<dyn FnWrite>` is sound when the application and this
backend **share one `nsi-ffi-wrap`** — a Rust dependency, or a
`cdylib` built from the same workspace.

A separately built `cdylib` reached by `dlopen` is a different
compilation. There the safe route is the `extern "C"` entry points
`DspyRegisterDriver` hands over, which live in the application's own
binary and cross the boundary as C. `nsi-ffi-wrap` already resolves
that symbol — it is the twelfth this crate exports — so the mechanism
is present and only the delivery path is missing. `T5.2`.

## Failure Modes

- **A driver with nothing to call.** Recording drops `Reference`, the
  scene is otherwise perfect, the render succeeds, and the viewport
  stays empty. No error anywhere. This is what `capi.rs` did.
- **The wrong layer name.** Asking the image for `R`,`G`,`B`,`A` when
  it holds `Ci.R`,`Ci.G`,`Ci.B` fails as "no layer matched", not as a
  missing channel — the file is fine and the read is wrong.
- **A vtable from the wrong compilation.** Undefined behaviour, and it
  will not look like a display problem.

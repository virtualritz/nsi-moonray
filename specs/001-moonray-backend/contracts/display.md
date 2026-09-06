# Contract: Getting Pixels Out

## Scope

How a rendered image reaches the application that asked for it, when
that application is an ɴsɪ consumer and the renderer is MoonRay.

The two ends do not meet on their own. **MoonRay has no display-driver
interface**: it writes files through its own `RenderOutputDriver`, and
an interactive consumer instead *snapshots* buffers off a live
`RenderContext` — `snapshotRenderBuffer` for a whole frame,
`snapshotDelta` for the pixels that changed since the last one.
**An ɴsɪ consumer expects the other shape**: an `outputdriver` node,
and buckets arriving as they are rendered.

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
| Pixels arrive *while* the render runs | Open | `RenderContext::snapshotDelta` plus `ActivePixels`, and `isFrameReadyForDisplay` / `areCoarsePassesComplete`, are what a converging viewport reads | None — batch MoonRay produces one finished image, so one bucket covering the frame is delivered from the file it wrote | The in-process `RenderContext` (`002` `R1`–`R3`). `T5.3` |
| A driver reached through `dlopen` gets pixels | Open | A `Box<dyn FnWrite>` is a trait object whose vtable belongs to the compilation that made it | None | Register through `DspyRegisterDriver` and deliver over the `extern "C"` entry points instead. `T5.2` |
| A driver that says stop, stops | Open | `Error::Stop` is what a closure returns to abort a render | None — `deliver_file` discards the returned `Error`, which is harmless for one final bucket and wrong for a progressive one | Honour the return value once delivery is progressive. `T5.3` |

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

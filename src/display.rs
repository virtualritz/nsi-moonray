//! Pixels out, to the callbacks an ɴsɪ consumer already writes.
//!
//! MoonRay has no display-driver interface. It writes files through its
//! own `RenderOutputDriver`, and an interactive consumer *snapshots*
//! buffers off a `RenderContext` — `snapshotRenderBuffer` for a whole
//! frame, `snapshotDelta` for only the pixels that changed since the
//! last one.
//!
//! An ɴsɪ consumer expects the other shape. `nsi-ffi-wrap`'s `output`
//! feature lets an application hand the renderer three closures on an
//! `outputdriver` node — `callback.open`, `callback.write`,
//! `callback.finish` — and receive buckets as they are rendered. That
//! is what a viewport uses, and it is what this module feeds.
//!
//! Nothing here touches the ndspy C ABI. The closures are Rust, the
//! call is Rust, and a driver written for 3Delight works unchanged.
//!
//! # How the closures arrive
//!
//! Through ɴsɪ's `Reference` type — a pointer parameter — which is why
//! `Reference` has to survive recording even though it never reaches
//! MoonRay. `nsi-ffi-wrap` double-boxes each closure so the fat
//! `dyn` pointer becomes a thin one that fits through the C API; this
//! reads it back the same way.
//!
//! # The one constraint
//!
//! A `Box<dyn FnWrite>` is a Rust trait object, and its vtable belongs
//! to the compilation that produced it. Calling it is sound when the
//! application and this backend share one `nsi-ffi-wrap` — a Rust
//! dependency, or a `cdylib` built from the same workspace. A
//! separately-built `cdylib` reached by `dlopen` is a different
//! compilation, and the safe route there is the `extern "C"` entry
//! points `DspyRegisterDriver` hands over, which live in the
//! application's own binary. That route is `T5.2`.
//!
//! # What is not here yet
//!
//! Delivery *while* the render runs. A batch render produces one
//! finished image, so a driver receives one bucket covering the frame,
//! read back off the file MoonRay wrote. Progressive delivery is
//! `snapshotDelta` against a live `RenderContext`, where the pixels
//! never reach a file at all; see `specs/002-interactive-updates`.

use nsi_ffi_wrap::output::{Error, FnFinish, FnOpen, FnWrite, PixelFormat};
use nsi_intermediate::{OwnedData, Scene};
use std::ffi::CString;

/// A [`PixelFormat`] for a list of channel names.
///
/// The callbacks are handed one of these, and
/// `PixelFormat::from_ndspy` is the only public way to build it — so
/// this is the one place an ndspy type appears, as the shape that
/// constructor takes. The names are `<layer>.<channel>` or bare, and
/// the renderer's own layer list is where they come from rather than a
/// guess: see `nsi-ffi-wrap`'s note on why a layer boundary is a change
/// of name.
pub fn pixel_format(channels: &[&str]) -> PixelFormat {
    // The C structs borrow these, so they have to outlive the call.
    let names: Vec<CString> = channels
        .iter()
        .map(|name| CString::new(*name).unwrap_or_default())
        .collect();

    let format: Vec<ndspy_sys::PtDspyDevFormat> = names
        .iter()
        .map(|name| ndspy_sys::PtDspyDevFormat {
            name: name.as_ptr(),
            type_: ndspy_sys::PkDspyFloat32,
        })
        .collect();

    PixelFormat::from_ndspy(&format)
}

/// The callbacks one ɴsɪ `outputdriver` node carries.
///
/// Each is a pointer the application produced and still owns. This
/// borrows them for the length of a render and frees nothing: the
/// closure belongs to whoever created it, and ɴsɪ's copy contract makes
/// `Reference` the one argument type that is *not* copied.
#[derive(Debug, Clone, Copy, Default)]
pub struct Callbacks {
    open: Option<*mut Box<dyn FnOpen<'static>>>,
    write: Option<*mut Box<dyn FnWrite<'static, f32>>>,
    finish: Option<*mut Box<dyn FnFinish<'static>>>,
}

impl Callbacks {
    /// Read the callbacks off an `outputdriver` node.
    ///
    /// Returns `None` when the node carries none, which is the ordinary
    /// case for a driver that writes a file.
    pub fn of(scene: &Scene, driver: &str) -> Option<Self> {
        let node = scene.node(driver)?;

        let pointer = |name: &str| -> Option<*mut core::ffi::c_void> {
            match &node.effective(name)?.data {
                OwnedData::Reference(pointers) => {
                    let first = pointers.first()?.0;
                    (!first.is_null()).then_some(first.cast_mut())
                }
                _ => None,
            }
        };

        let callbacks = Self {
            open: pointer("callback.open").map(|p| p.cast()),
            write: pointer("callback.write").map(|p| p.cast()),
            finish: pointer("callback.finish").map(|p| p.cast()),
        };

        (callbacks.open.is_some()
            || callbacks.write.is_some()
            || callbacks.finish.is_some())
        .then_some(callbacks)
    }

    /// Whether anything is listening.
    pub fn is_empty(&self) -> bool {
        self.open.is_none() && self.write.is_none() && self.finish.is_none()
    }

    /// Tell the driver a render is starting.
    ///
    /// # Safety
    ///
    /// The pointers came from `nsi-ffi-wrap`'s callback wrappers in a
    /// compilation sharing this crate's `nsi-ffi-wrap`, and the
    /// closures outlive this call. See the module's "one constraint".
    pub unsafe fn open(
        &self,
        name: &str,
        width: usize,
        height: usize,
        format: &PixelFormat,
    ) -> Error {
        let Some(open) = self.open else {
            return Error::None;
        };

        // SAFETY: the caller guarantees the pointer and the closure's
        // lifetime; the double box is `nsi-ffi-wrap`'s own thin-pointer
        // representation, read back the way it was written.
        let open = unsafe { &mut *open };
        open(name, width, height, format)
    }

    /// Hand the driver one bucket.
    ///
    /// `pixels` is interleaved, row major, and covers exactly the
    /// rectangle given: `(x.len() * y.len() * format.channels())`
    /// values. A shorter slice is refused rather than read past, since
    /// the over-read would happen inside the application's own closure.
    ///
    /// # Safety
    ///
    /// As [`Callbacks::open`].
    #[allow(clippy::too_many_arguments)]
    pub unsafe fn write(
        &self,
        name: &str,
        width: usize,
        height: usize,
        x: core::ops::Range<usize>,
        y: core::ops::Range<usize>,
        format: &PixelFormat,
        pixels: &[f32],
    ) -> Error {
        let Some(write) = self.write else {
            return Error::None;
        };

        let expected = x.len() * y.len() * format.channels();
        if pixels.len() != expected {
            return Error::BadParameters;
        }

        // SAFETY: as `open`.
        let write = unsafe { &*write };
        write(
            name, width, height, x.start, x.end, y.start, y.end, format, pixels,
        )
    }

    /// Tell the driver the render is over.
    ///
    /// # Safety
    ///
    /// As [`Callbacks::open`].
    pub unsafe fn finish(
        &self,
        name: &str,
        width: usize,
        height: usize,
        format: PixelFormat,
    ) -> Error {
        let Some(finish) = self.finish else {
            return Error::None;
        };

        // SAFETY: as `open`.
        let finish = unsafe { &mut *finish };
        finish(name.to_string(), width, height, format)
    }
}

// SAFETY: the pointers are read-only handles to closures the
// application owns and keeps alive across the render. Sending them
// between threads is what a renderer does with a display driver; the
// closures' own thread safety is their author's to declare, which is
// what `FnWrite` taking `&self` says.
unsafe impl Send for Callbacks {}
unsafe impl Sync for Callbacks {}

/// Hand a finished image to an application's callbacks.
///
/// **A stopgap, and shaped like one.** MoonRay wrote a file, so this
/// reads it back and delivers one bucket covering the frame: the
/// application sees a completed render rather than a converging one.
/// The delivery *interface* is the one it will keep — the same
/// closures, the same bucket call — so what changes when the renderer
/// runs in process is where the pixels come from, not what receives
/// them.
pub fn deliver_file(
    callbacks: &Callbacks,
    name: &str,
    image: &std::path::Path,
) -> Result<(), String> {
    use exr::prelude::*;

    // Read whatever channels are there rather than assuming RGBA.
    // MoonRay names them after the ɴsɪ output layer -- `Ci.R`, `Ci.G`,
    // `Ci.B` for a beauty pass with no alpha -- so a reader that
    // insists on `R`,`G`,`B`,`A` finds no layer at all, which is what
    // the first version of this did.
    let read = read()
        .no_deep_data()
        .largest_resolution_level()
        .all_channels()
        .first_valid_layer()
        .all_attributes()
        .from_file(image)
        .map_err(|error| format!("reading {}: {error}", image.display()))?;

    let layer = &read.layer_data;
    let width = layer.size.width();
    let height = layer.size.height();

    // ndspy interleaves a pixel's channels, and its layer split reads
    // the name before the dot -- so the names go across as the file has
    // them, lowercased, which is the spelling the channel heuristics
    // expect (`r`, `g`, `b`, `a`).
    let names: Vec<String> = layer
        .channel_data
        .list
        .iter()
        .map(|channel| channel.name.to_string().to_lowercase())
        .collect();
    let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
    let format = pixel_format(&borrowed);

    let channels = names.len();
    let mut pixels = vec![0.0f32; width * height * channels];
    for (index, channel) in layer.channel_data.list.iter().enumerate() {
        for y in 0..height {
            for x in 0..width {
                let sample: f32 = channel
                    .sample_data
                    .value_by_flat_index(y * width + x)
                    .to_f32();
                pixels[(y * width + x) * channels + index] = sample;
            }
        }
    }

    // SAFETY: the caller is the ɴsɪ context that recorded these
    // pointers, and the application owns the closures for the length of
    // the render. See the module's "one constraint".
    unsafe {
        callbacks.open(name, width, height, &format);
        callbacks.write(
            name,
            width,
            height,
            0..width,
            0..height,
            &format,
            &pixels,
        );
        callbacks.finish(name, width, height, format);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nsi_ffi_wrap::output::{FinishCallback, WriteCallback};
    use nsi_intermediate::{OwnedArg, OwnedData};
    use nsi_trait::Type;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    /// Put a callback on an `outputdriver` node the way ɴsɪ does: as a
    /// `Reference`, which is the one argument type ɴsɪ does not copy.
    fn with_callback(
        scene: &mut Scene,
        driver: &str,
        name: &str,
        pointer: *const core::ffi::c_void,
    ) {
        scene
            .set_attribute(
                driver,
                vec![OwnedArg::new(
                    name,
                    Type::Reference,
                    1,
                    0,
                    OwnedData::Reference(vec![nsi_intermediate::HostPtr(
                        pointer,
                    )]),
                )],
            )
            .expect("a recordable edit");
    }

    /// The whole point: pixels reach a closure an application wrote,
    /// with no ndspy structs anywhere between.
    #[test]
    fn a_bucket_reaches_the_applications_closure() {
        let received = Arc::new(Mutex::new(Vec::<f32>::new()));
        let seen = Arc::clone(&received);
        let finished = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&finished);

        let write = WriteCallback::<f32>::new(
            move |_name,
                  _width,
                  _height,
                  _x0,
                  _x1,
                  _y0,
                  _y1,
                  _format,
                  pixels: &[f32]| {
                seen.lock().expect("not poisoned").extend_from_slice(pixels);
                Error::None
            },
        );
        let finish = FinishCallback::new(
            move |_name, _width, _height, _format: PixelFormat| {
                counted.fetch_add(1, Ordering::SeqCst);
                Error::None
            },
        );

        let mut scene = Scene::default();
        scene
            .create("driver", "outputdriver")
            .expect("a fresh handle");

        use nsi_ffi_wrap::argument::CallbackPtr;
        with_callback(&mut scene, "driver", "callback.write", write.to_ptr());
        with_callback(&mut scene, "driver", "callback.finish", finish.to_ptr());

        let callbacks =
            Callbacks::of(&scene, "driver").expect("both were recorded");
        assert!(!callbacks.is_empty());

        let format = pixel_format(&["r", "g", "b", "a"]);
        let pixels: Vec<f32> = (0..2 * 2 * 4).map(|i| i as f32).collect();

        // SAFETY: the closures were built in this compilation and
        // outlive the calls.
        let error = unsafe {
            callbacks.write("memory", 2, 2, 0..2, 0..2, &format, &pixels)
        };
        assert_eq!(error, Error::None);
        assert_eq!(*received.lock().expect("not poisoned"), pixels);

        let error = unsafe { callbacks.finish("memory", 2, 2, format) };
        assert_eq!(error, Error::None);
        assert_eq!(finished.load(Ordering::SeqCst), 1);
    }

    /// A bucket that does not match the rectangle is refused here,
    /// rather than over-read inside the application's closure.
    #[test]
    fn a_short_bucket_is_refused() {
        let write = WriteCallback::<f32>::new(
            |_name, _w, _h, _x0, _x1, _y0, _y1, _format, _pixels: &[f32]| {
                panic!("the closure should not be reached");
            },
        );

        let mut scene = Scene::default();
        scene
            .create("driver", "outputdriver")
            .expect("a fresh handle");

        use nsi_ffi_wrap::argument::CallbackPtr;
        with_callback(&mut scene, "driver", "callback.write", write.to_ptr());

        let callbacks = Callbacks::of(&scene, "driver").expect("recorded");
        let format = pixel_format(&["r"]);

        // SAFETY: as above; the call returns before reaching the
        // closure.
        let error = unsafe {
            callbacks.write("memory", 2, 2, 0..2, 0..2, &format, &[0.0])
        };
        assert_eq!(error, Error::BadParameters);
    }

    /// A driver node with no callbacks is not one of these.
    #[test]
    fn a_file_driver_has_no_callbacks() {
        let mut scene = Scene::default();
        scene
            .create("driver", "outputdriver")
            .expect("a fresh handle");
        scene
            .set_attribute(
                "driver",
                vec![OwnedArg::new(
                    "imagefilename",
                    Type::String,
                    1,
                    0,
                    OwnedData::String(vec![b"beauty.exr".to_vec()]),
                )],
            )
            .expect("a recordable edit");

        assert!(Callbacks::of(&scene, "driver").is_none());
    }
}

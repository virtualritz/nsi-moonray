//! MoonRay, rendering in this process.
//!
//! # Why this and not the binary
//!
//! MoonRay renders progressively already -- `RenderMode::PROGRESSIVE`
//! puts samples up as they exist. What it does not do is *push* them: a
//! consumer pulls, with `snapshotRenderBuffer` or `snapshotDelta`, and
//! `moonray_gui` is a loop around exactly that. ɴsɪ pushes to an output
//! driver's callbacks, so the adapter is a snapshot loop on this side
//! and is small.
//!
//! The single thing that made it impossible was spawning. A separate
//! process has no `RenderContext` to snapshot and no `SceneContext` to
//! edit, so it forecloses progressive delivery, incremental updates and
//! concurrent rendering at once.
//!
//! # The renderer owns the scene
//!
//! `RenderContext::getSceneContext()` hands out a reference to its own,
//! and its comment says "only call this when not rendering". So a scene
//! meant to be rendered is built **inside** the renderer rather than
//! built separately and handed over. [`Render::scene`] is that scene,
//! and the borrow is what keeps it from outliving the renderer.

use super::{Context, Error, ffi, result};
use std::ffi::CString;

/// How the frame is rendered.
///
/// From `moonray/rendering/rndr/Types.h`. Not a quality setting: it
/// decides whether a snapshot part way through shows anything useful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Each tile to completion before moving on. What a file-writing
    /// render wants, and what makes a viewport look frozen until the
    /// end.
    #[default]
    Batch,
    /// Samples up as they arrive. What a snapshot loop is for.
    Progressive,
    /// Stops path tracing and renders a simplified frame -- something
    /// on screen immediately, then converge.
    ProgressiveFast,
    /// A new frame every n milliseconds, no refinement between.
    Realtime,
}

impl Mode {
    fn code(self) -> std::ffi::c_int {
        match self {
            Self::Batch => 0,
            Self::Progressive => 1,
            Self::ProgressiveFast => 2,
            Self::Realtime => 3,
        }
    }
}

/// What MoonRay has spent, cumulatively, since the renderer was made.
///
/// Times are in seconds. `primitives_tessellated` is a count of
/// tessellations performed, which is the most direct answer to "did
/// that edit re-tessellate anything".
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Cost {
    pub tessellation: f64,
    pub build_accelerator: f64,
    pub load_procedurals: f64,
    pub rebuild_geometry: f64,
    pub primitives_tessellated: usize,
}

/// The part of a frame that changed, and its pixels.
///
/// `pixels` is row major within the rectangle, RGBA float per pixel --
/// `width * height * 4` values, not the whole frame's worth.
#[derive(Debug, Clone, PartialEq)]
pub struct Delta {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<f32>,
}

/// A MoonRay renderer.
pub struct Render {
    raw: *mut ffi::NmrRender,
}

// SAFETY: `RenderContext` is not internally synchronised for scene
// edits -- MoonRay's own rule is that the scene is touched only between
// frames -- so this is `Send` and not `Sync`.
unsafe impl Send for Render {}

impl Render {
    /// A renderer, with somewhere to find scene classes.
    ///
    /// `dso_path` must point at MoonRay's `rdl2dso`, or no MoonRay
    /// class resolves and every object in the scene fails to create.
    /// `threads` of `None` means every core.
    ///
    /// # One at a time
    ///
    /// `None` if a [`Render`] is already alive in this process.
    /// MoonRay's driver state is *global* -- thread-local pools, the
    /// affinity manager, the image-write driver -- so two renderers
    /// share what one is using, and the failure is an abort inside the
    /// allocator rather than anything diagnosable. Sequential use is
    /// fine: drop one and make the next.
    pub fn new(
        dso_path: Option<&str>,
        threads: Option<u32>,
        mode: Mode,
    ) -> Option<Self> {
        let path = dso_path.and_then(|path| CString::new(path).ok());
        let pointer = path.as_ref().map_or(std::ptr::null(), |p| p.as_ptr());

        // SAFETY: a valid NUL-terminated string or null.
        let raw = unsafe {
            ffi::nmr_render_new(pointer, threads.unwrap_or(0), mode.code())
        };
        (!raw.is_null()).then_some(Self { raw })
    }

    /// The scene to build into.
    ///
    /// Borrowed from the renderer, which is what makes this the *same*
    /// scene the frame will be rendered from rather than a copy that
    /// has to be pushed across.
    pub fn scene(&self) -> Option<Context> {
        // SAFETY: a live renderer; the returned wrapper borrows the
        // renderer's own scene and knows not to free it.
        let raw = unsafe { ffi::nmr_render_scene(self.raw) };
        (!raw.is_null()).then(|| Context::from_raw(raw))
    }

    /// Prepare. Once, after the scene is built.
    pub fn initialize(&self) -> Result<(), Error> {
        // SAFETY: a live renderer.
        result(unsafe { ffi::nmr_render_initialize(self.raw) })
    }

    /// Begin a frame.
    ///
    /// Returns once render *prep* is done. The frame goes on converging
    /// behind it, which is the whole reason a snapshot loop is worth
    /// having rather than a single wait.
    pub fn start(&self) -> Result<(), Error> {
        // SAFETY: a live renderer.
        result(unsafe { ffi::nmr_render_start(self.raw) })
    }

    /// Tell the renderer its scene changed under it.
    ///
    /// A scene edited through [`Render::scene`] is changed *externally*
    /// as far as `RenderContext` is concerned -- it watches its own
    /// `updateScene` entry points, not the objects. Without this the
    /// next [`Render::start`] renders the previous scene: correctly,
    /// quickly, and wrongly.
    ///
    /// **Applied but not marked** is the failure this prevents, and it
    /// has no symptom other than a stale image. Its twin, **marked but
    /// not applied**, is a full rebuild that renders correctly and
    /// slowly; only timing finds that one.
    pub fn scene_updated(&self) -> Result<(), Error> {
        // SAFETY: a live renderer.
        result(unsafe { ffi::nmr_render_scene_updated(self.raw) })
    }

    /// End the frame, blocking until the threads are down.
    pub fn stop(&self) -> Result<(), Error> {
        // SAFETY: a live renderer.
        result(unsafe { ffi::nmr_render_stop(self.raw) })
    }

    /// Whether there is anything worth showing yet.
    pub fn ready_for_display(&self) -> bool {
        // SAFETY: a live renderer.
        unsafe { ffi::nmr_render_is_ready_for_display(self.raw) != 0 }
    }

    /// Whether the coarse passes are done.
    ///
    /// The point at which a progressive frame stops looking blocky and
    /// starts refining -- when a viewport's first frame is worth
    /// showing, well before it is complete.
    pub fn coarse_passes_complete(&self) -> bool {
        // SAFETY: a live renderer.
        unsafe { ffi::nmr_render_are_coarse_passes_complete(self.raw) != 0 }
    }

    /// Whether nothing more is coming.
    pub fn frame_complete(&self) -> bool {
        // SAFETY: a live renderer.
        unsafe { ffi::nmr_render_is_frame_complete(self.raw) != 0 }
    }

    /// The rectangle that changed since the last delta, and its
    /// pixels.
    ///
    /// [`Render::snapshot`] hands over the whole frame however little
    /// of it moved. This is what lets a driver send only what is new,
    /// which matters for a large frame or one crossing a network.
    ///
    /// **Not a drop-in for `snapshot`.** MoonRay's `snapshotDelta`
    /// does "no resize, no extrapolation and no untiling", and its
    /// buffer is *not normalized by weight* -- so the shim undoes the
    /// tiling and divides each pixel by its own sample count. An
    /// unnormalised buffer is not obviously wrong; it is just darker,
    /// which is why `a_delta_snapshot_agrees_with_a_full_one` compares
    /// the two rather than eyeballing one.
    ///
    /// `None` when nothing changed. The first call after a frame
    /// starts reports the whole frame, since everything is new.
    pub fn snapshot_delta(&self) -> Result<Option<Delta>, Error> {
        let (width, height) = self.resolution()?;
        let mut pixels = vec![0.0f32; width as usize * height as usize * 4];
        let (mut x, mut y, mut w, mut h) = (0, 0, 0, 0);

        // SAFETY: the buffer is the whole frame, so it is never
        // smaller than the changed rectangle within it.
        result(unsafe {
            ffi::nmr_render_snapshot_delta(
                self.raw,
                pixels.as_mut_ptr(),
                pixels.len(),
                &mut x,
                &mut y,
                &mut w,
                &mut h,
            )
        })?;

        if w == 0 || h == 0 {
            return Ok(None);
        }

        pixels.truncate(w as usize * h as usize * 4);
        Ok(Some(Delta {
            x,
            y,
            width: w,
            height: h,
            pixels,
        }))
    }

    /// What the frames so far have cost.
    ///
    /// **The only way to tell an incremental update from a rebuild.** A
    /// synchronise that re-tessellates the whole scene renders exactly
    /// the right image, slightly later: no test that reads pixels can
    /// see the difference, and on a small scene neither can a person.
    ///
    /// The counters are cumulative across frames, so the question to
    /// ask is "did these go up", not "are these zero".
    pub fn cost(&self) -> Result<Cost, Error> {
        let mut cost = ffi::NmrCost::default();
        // SAFETY: a live renderer and a valid out-pointer.
        result(unsafe { ffi::nmr_render_cost(self.raw, &mut cost) })?;
        Ok(Cost {
            tessellation: cost.tessellation,
            build_accelerator: cost.build_accelerator,
            load_procedurals: cost.load_procedurals,
            rebuild_geometry: cost.rebuild_geometry,
            primitives_tessellated: cost.primitives_tessellated,
        })
    }

    /// The frame's size, as the renderer resolved it.
    ///
    /// Not what the scene asked for: `SceneVariables::res` scales the
    /// image, so a buffer sized from the scene is the wrong size.
    pub fn resolution(&self) -> Result<(u32, u32), Error> {
        let mut width = 0;
        let mut height = 0;
        // SAFETY: both out-pointers are valid for the call.
        result(unsafe {
            ffi::nmr_render_resolution(self.raw, &mut width, &mut height)
        })?;
        Ok((width, height))
    }

    /// Write the frame to the files the scene names.
    ///
    /// The `SceneVariables`' `output_file` for the beauty, plus every
    /// `RenderOutput` -- AOVs, cryptomatte, deep -- through MoonRay's
    /// own output machinery. Encoding an EXR here instead would mean
    /// reimplementing layer naming, header metadata and the
    /// aperture/region windows, and getting all of it subtly wrong.
    ///
    /// For a batch render. An interactive one sends pixels to the
    /// application's callbacks and has no file to write.
    pub fn write(&self) -> Result<(), Error> {
        // SAFETY: a live renderer.
        result(unsafe { ffi::nmr_render_write(self.raw) })
    }

    /// The frame so far, RGBA float per pixel.
    ///
    /// MoonRay's render buffer is `PixelBuffer<Vec4f>`, so this is the
    /// renderer's own layout with the tiling undone -- four channels,
    /// row major.
    pub fn snapshot(&self) -> Result<(u32, u32, Vec<f32>), Error> {
        let (width, height) = self.resolution()?;
        let mut pixels = vec![0.0f32; width as usize * height as usize * 4];

        // SAFETY: the buffer is exactly the length the shim checks for,
        // and its capacity is passed alongside so a mismatch is refused
        // rather than written past.
        result(unsafe {
            ffi::nmr_render_snapshot(
                self.raw,
                pixels.as_mut_ptr(),
                pixels.len(),
            )
        })?;

        Ok((width, height, pixels))
    }

    /// What MoonRay last complained about.
    pub fn error(&self) -> Option<String> {
        // SAFETY: the shim owns the string until the next failing call.
        unsafe {
            let message = ffi::nmr_render_error(self.raw);
            (!message.is_null()).then(|| {
                std::ffi::CStr::from_ptr(message)
                    .to_string_lossy()
                    .into_owned()
            })
        }
    }
}

impl Drop for Render {
    fn drop(&mut self) {
        // SAFETY: created by `nmr_render_new` and freed once. The shim
        // stops a running frame first: leaving one running past the
        // destructor is a crash, and dropping a viewport mid-render is
        // the ordinary way to get there.
        unsafe { ffi::nmr_render_free(self.raw) }
    }
}

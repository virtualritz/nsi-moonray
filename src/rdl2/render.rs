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
    pub fn new(dso_path: Option<&str>, threads: Option<u32>) -> Option<Self> {
        let path = dso_path.and_then(|path| CString::new(path).ok());
        let pointer = path.as_ref().map_or(std::ptr::null(), |p| p.as_ptr());

        // SAFETY: a valid NUL-terminated string or null.
        let raw = unsafe { ffi::nmr_render_new(pointer, threads.unwrap_or(0)) };
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

    /// Whether nothing more is coming.
    pub fn frame_complete(&self) -> bool {
        // SAFETY: a live renderer.
        unsafe { ffi::nmr_render_is_frame_complete(self.raw) != 0 }
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

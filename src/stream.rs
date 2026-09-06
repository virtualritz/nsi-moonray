//! The snapshot loop: a converging render reaching an application's
//! closures.
//!
//! This is where the two halves finally meet. MoonRay renders
//! progressively and expects to be **pulled** --
//! `snapshotRenderBuffer`, paced by `areCoarsePassesComplete` and
//! `isFrameComplete`. ɴsɪ **pushes**, to an `outputdriver`'s
//! `callback.open` / `callback.write` / `callback.finish`. Neither side
//! needed changing; the adapter is this loop.
//!
//! It could not exist while the renderer was a spawned process,
//! because a separate process has no `RenderContext` to snapshot. That
//! is the whole reason `002` put linking first.
//!
//! # What the application sees
//!
//! `open` once, then a `write` per snapshot covering the whole frame,
//! then `finish`. Buckets covering the frame rather than tiles is
//! deliberate: MoonRay renders in tile order and `snapshotRenderBuffer`
//! untiles for us, so the rectangle that is actually *new* is not
//! something this can name without `snapshotDelta`'s `ActivePixels`.
//! Sending the frame is honest; naming a sub-rectangle would not be.
//!
//! # Stopping
//!
//! A closure answering [`Error::Stop`] stops the render. That is what
//! it is for -- a viewport closing, a user cancelling -- and ignoring
//! it, as the file-delivery stopgap had to, means an application cannot
//! get its renderer back.

use crate::{
    display::{Callbacks, pixel_format},
    rdl2::Render,
};
use nsi_ffi_wrap::output::Error;
use std::time::{Duration, Instant};

/// How long to wait between snapshots.
///
/// Not a frame rate: it is how often the loop asks whether there is
/// something new. Too short and the snapshot copy costs more than the
/// render; too long and the viewport lags behind the samples.
const POLL: Duration = Duration::from_millis(50);

/// What stopped the loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// The frame finished.
    Complete,
    /// A closure answered `Error::Stop`.
    ByCallback,
    /// The deadline passed with the frame still converging.
    TimedOut,
}

/// Render, delivering each snapshot to the callbacks as it converges.
///
/// Blocks until the frame completes, a callback stops it, or `deadline`
/// passes. The renderer is left stopped either way: leaving a frame
/// running is a crash at drop rather than a leak.
///
/// # Errors
///
/// Only for a snapshot that cannot be taken. A callback answering with
/// an error is *reported through the return value*, not as a failure:
/// ɴsɪ always returns an image, and a driver refusing one bucket is not
/// grounds for refusing the render.
pub fn stream(
    render: &Render,
    callbacks: &Callbacks,
    name: &str,
    deadline: Option<Duration>,
) -> Result<Stopped, crate::rdl2::Error> {
    let (width, height) = render.resolution()?;
    let (width, height) = (width as usize, height as usize);

    // MoonRay's render buffer is `PixelBuffer<Vec4f>` -- RGBA float per
    // pixel -- and the names go across lowercased, which is the
    // spelling the channel heuristics expect.
    let format = pixel_format(&["r", "g", "b", "a"]);

    // SAFETY: the caller owns the closures and keeps them alive across
    // the render; see `display`'s "one constraint".
    unsafe { callbacks.open(name, width, height, &format) };

    let started = Instant::now();
    let mut outcome = Stopped::Complete;
    // Deliver at least one frame even for a render that completes
    // before the first poll -- otherwise a fast scene reaches `finish`
    // having shown nothing.
    let mut delivered = false;

    loop {
        let complete = render.frame_complete();

        // Nothing worth sending until there is something to see. A
        // snapshot before the coarse passes is a buffer of zeroes, and
        // an application cannot tell that from a black scene.
        if complete || render.coarse_passes_complete() {
            let (_, _, pixels) = render.snapshot()?;

            // SAFETY: as `open`; the slice is exactly the frame.
            let answer = unsafe {
                callbacks.write(
                    name,
                    width,
                    height,
                    0..width,
                    0..height,
                    &format,
                    &pixels,
                )
            };
            delivered = true;

            if answer == Error::Stop {
                outcome = Stopped::ByCallback;
                break;
            }
        }

        if complete {
            break;
        }

        if let Some(deadline) = deadline
            && started.elapsed() >= deadline
        {
            outcome = Stopped::TimedOut;
            break;
        }

        std::thread::sleep(POLL);
    }

    if !delivered {
        let (_, _, pixels) = render.snapshot()?;
        // SAFETY: as above.
        unsafe {
            callbacks.write(
                name,
                width,
                height,
                0..width,
                0..height,
                &format,
                &pixels,
            )
        };
    }

    render.stop()?;

    // SAFETY: as `open`.
    unsafe { callbacks.finish(name, width, height, format) };

    Ok(outcome)
}

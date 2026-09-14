//! This backend as a renderer a Rust host *links* rather than loads.
//!
//! `render::an_applications_callback_receives_the_rendered_pixels`
//! proves the closures work when this crate's own code calls them. That
//! is not the same question. The question here is whether an
//! application that never names this crate in its render code -- one
//! that builds a scene through `nsi::Context` and asks for a renderer
//! by name -- gets its pixels back.
//!
//! **Why it has to be a test rather than a paragraph.** The pixels
//! arrive through `callback.write`, a `Box<dyn Fn…>` whose vtable
//! belongs to the compilation that made it. Through a loaded library
//! that is a different compilation and calling it is undefined
//! behaviour; linked, it is the same compilation and calling it is
//! ordinary. Nothing in the *types* distinguishes those two cases, so
//! only running it says which one this is.

// See the `linked-route` feature in `Cargo.toml` for why this is not
// built by default: the route works against the published
// `nsi-ffi-wrap`, but this test's `nsi::backend::register` does not
// exist there yet.
#![cfg(all(feature = "rdl2", moonray, feature = "linked-route"))]

use nsi_ffi_wrap as nsi;
use nsi_ffi_wrap::output::{Error, PixelFormat, WriteCallback};
use std::sync::{Arc, Mutex, Once};

/// Register once: the registry is process-wide, and two tests in one
/// binary would otherwise race to fill it.
static REGISTER: Once = Once::new();

fn linked() {
    REGISTER.call_once(|| {
        nsi::backend::register("moonray", Arc::new(nsi_moonray::MoonRay));
    });
}

/// What the application's closure saw.
#[derive(Default)]
struct Delivered {
    pixels: Vec<f32>,
    width: usize,
    height: usize,
    channels: usize,
}

/// **A host links this crate, names it, and receives pixels.**
///
/// Every step is the one an application actually performs: register,
/// `Context::new` with a `"renderer"`, build the scene through the ɴsɪ
/// API, hand over a closure, render. Nothing reaches into this crate.
#[test]
fn a_linked_host_receives_its_pixels() {
    if nsi_moonray::render::binary().is_err() {
        eprintln!("skipped: no `moonray` binary");
        return;
    }
    linked();

    let received = Arc::new(Mutex::new(Delivered::default()));
    let seen = Arc::clone(&received);
    let write = WriteCallback::<f32>::new(
        move |_name,
              width,
              height,
              _x0,
              _x1,
              _y0,
              _y1,
              format: &PixelFormat,
              pixels: &[f32]| {
            let mut seen = seen.lock().expect("not poisoned");
            seen.pixels.extend_from_slice(pixels);
            seen.width = width;
            seen.height = height;
            seen.channels = format.channels();
            Error::None
        },
    );

    let (width, height) = (32i32, 24i32);
    let directory = std::env::temp_dir().join("nsi-moonray-linked");
    std::fs::create_dir_all(&directory).expect("a writable directory");
    let image = directory.join("linked.exr");
    let _ = std::fs::remove_file(&image);

    let context =
        nsi::Context::new(Some(&[nsi::string!("renderer", "moonray")]))
            .expect("the linked backend answers to its name");

    // A camera looking at a lit quad. Built through the interface, not
    // through this crate's `Scene`.
    context.create("cam", nsi::node::PERSPECTIVE_CAMERA, None);
    context.set_attribute("cam", &[nsi::f32!("fov", 45.0)]);
    context.connect("cam", None, nsi::ROOT, "objects", None);

    context.create("screen", nsi::node::SCREEN, None);
    context.set_attribute(
        "screen",
        &[nsi::i32_slice!("resolution", &[width, height]).array_len(
            std::num::NonZeroUsize::new(2).expect("two is not zero"),
        )],
    );
    context.connect("screen", None, "cam", "screens", None);

    context.create("env", nsi::node::ENVIRONMENT, None);
    context.connect("env", None, nsi::ROOT, "objects", None);

    context.create("quad", nsi::node::MESH, None);
    context.set_attribute(
        "quad",
        &[
            nsi::i32!("nvertices", 4),
            nsi::i32_slice!("P.indices", &[0, 1, 2, 3]),
            nsi::point_slice!(
                "P",
                &[
                    [-1.0f32, -1.0, -5.0],
                    [1.0, -1.0, -5.0],
                    [1.0, 1.0, -5.0],
                    [-1.0, 1.0, -5.0],
                ]
            ),
        ],
    );
    context.connect("quad", None, nsi::ROOT, "objects", None);

    context.create("beauty", nsi::node::OUTPUT_LAYER, None);
    context.set_attribute("beauty", &[nsi::string!("variablename", "Ci")]);
    context.connect("beauty", None, "screen", "outputlayers", None);

    context.create("driver", nsi::node::OUTPUT_DRIVER, None);
    context.set_attribute(
        "driver",
        &[
            nsi::string!("imagefilename", image.to_string_lossy().as_ref()),
            nsi::callback!("callback.write", write),
        ],
    );
    context.connect("driver", None, "beauty", "outputdrivers", None);

    context.render_control(nsi::Action::Start, None);
    context.render_control(nsi::Action::Wait, None);

    let delivered = received.lock().expect("not poisoned");
    assert!(
        !delivered.pixels.is_empty(),
        "the application's closure received no pixels; the linked \
         backend either did not answer to its name or did not deliver"
    );
    assert_eq!(delivered.width, width as usize);
    assert_eq!(delivered.height, height as usize);
    assert!(
        delivered.channels > 0,
        "a delivery with no channels describes nothing"
    );
    // **Not the frame's exact size.** Delivery is progressive: a
    // converging render hands over several snapshots, and a later one
    // covers pixels an earlier one already did. So what is checked is
    // that every delivery was a whole number of pixels -- a buffer that
    // does not divide by the channel count is one the application would
    // read off the end of.
    assert_eq!(
        delivered.pixels.len() % delivered.channels,
        0,
        "a delivery of {} values is not whole pixels at {} channels",
        delivered.pixels.len(),
        delivered.channels
    );

    // Lit, rather than merely delivered: an all-black frame would pass
    // every assertion above and mean the scene never rendered.
    assert!(
        delivered.pixels.iter().any(|value| *value > 0.0),
        "every pixel is zero, so nothing was rendered"
    );
}

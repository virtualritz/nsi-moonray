//! MoonRay rendering **in this process**: no spawned binary, no file.
//!
//! Needs the `rdl2` feature, `$SCENE_RDL2_ROOT`, `$MOONRAY_ROOT` and
//! `$NSI_MOONRAY_DSO` pointing at MoonRay's `rdl2dso`.
//!
//! This is the gate `002` calls first, and everything else queues
//! behind it: a spawned batch process has no `SceneContext` to edit and
//! no `RenderContext` to snapshot, so it forecloses incremental
//! updates, progressive delivery and concurrent rendering together.
#![cfg(all(feature = "rdl2", moonray))]

use nsi_intermediate::{OwnedArgument, OwnedData, Scene};
use nsi_moonray::{apply::apply, flush::flush, rdl2::Render};
use nsi_trait::Type;

fn arg(name: &str, type_tag: Type, data: OwnedData) -> OwnedArgument {
    OwnedArgument::new(name, type_tag, 1, 0, data)
}

/// An argument whose values are fixed-size arrays -- how ɴsɪ spells a
/// UV set, `float[2]`. The array length is what tells one value from
/// two, and so what tells a per-vertex variable from a face-varying
/// one.
fn array_arg(
    name: &str,
    type_tag: Type,
    length: usize,
    data: OwnedData,
) -> OwnedArgument {
    OwnedArgument::new(name, type_tag, length, 0, data)
}

fn dso_path() -> Option<String> {
    std::env::var("NSI_MOONRAY_DSO").ok()
}

/// What a driver does with buckets: paint each into a frame.
///
/// Buckets name a *rectangle*, not the whole frame — the first covers
/// everything, later ones only what the renderer refined — so a test
/// that looked at the last bucket alone would be looking at a corner.
#[derive(Default)]
struct Canvas {
    pixels: Vec<f32>,
    width: usize,
    height: usize,
    buckets: usize,
}

impl Canvas {
    // The nine are the callback's own signature; renaming them into a
    // struct here would only move the arity somewhere less obvious.
    #[allow(clippy::too_many_arguments)]
    fn paint(
        &mut self,
        width: usize,
        height: usize,
        x0: usize,
        x1: usize,
        y0: usize,
        y1: usize,
        channels: usize,
        pixels: &[f32],
    ) {
        if self.pixels.is_empty() {
            self.pixels = vec![0.0; width * height * channels];
            self.width = width;
            self.height = height;
        }
        self.buckets += 1;

        for (row, y) in (y0..y1).enumerate() {
            for (col, x) in (x0..x1).enumerate() {
                let from = (row * (x1 - x0) + col) * channels;
                let to = (y * width + x) * channels;
                self.pixels[to..to + channels]
                    .copy_from_slice(&pixels[from..from + channels]);
            }
        }
    }
}

/// One renderer per process, so one test at a time.
///
/// Not a test-harness nicety: MoonRay's driver state is global, and two
/// live `RenderContext`s abort in the allocator. `Render::new` refuses
/// the second, so without this the tests would race for which one gets
/// `None`.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A scratch directory of this test binary's own.
///
/// **Per process, not per suite.** `just ci` and `just test-rdl2` both
/// run `tests/render.rs`, and with a renderer installed both actually
/// render -- into the same path, at the same time, if the name is
/// fixed. That is a test failure nobody can reproduce afterwards,
/// because the loser's file is gone by the time anyone looks.
fn scratch(name: &str) -> std::path::PathBuf {
    let path =
        std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
    std::fs::create_dir_all(&path).expect("a writable temporary directory");
    path
}

fn renderer(dso: &str) -> (std::sync::MutexGuard<'static, ()>, Render) {
    // A poisoned lock means an earlier test panicked; the renderer it
    // held is dropped either way, so carrying on is right.
    let guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let render =
        Render::new(Some(dso), Some(2), nsi_moonray::rdl2::Mode::Progressive)
            .expect("a renderer");
    (guard, render)
}

/// A lit quad facing the camera, at a small resolution.
fn scene(width: i32, height: i32) -> Scene {
    let mut scene = Scene::default();

    scene.create("quad", "mesh").unwrap();
    scene
        .set_attribute(
            "quad",
            vec![
                arg("nvertices", Type::I32, OwnedData::I32(vec![4])),
                arg("P.indices", Type::I32, OwnedData::I32(vec![0, 1, 2, 3])),
                arg(
                    "P",
                    Type::Point,
                    OwnedData::F32(vec![
                        -1.0, -1.0, -5.0, 1.0, -1.0, -5.0, 1.0, 1.0, -5.0,
                        -1.0, 1.0, -5.0,
                    ]),
                ),
            ],
        )
        .unwrap();
    scene.connect("quad", None, ".root", "objects").unwrap();

    scene.create("light", "environment").unwrap();
    scene.connect("light", None, ".root", "objects").unwrap();

    scene.create("cam", "perspectivecamera").unwrap();
    scene
        .set_attribute(
            "cam",
            vec![arg("fov", Type::F32, OwnedData::F32(vec![45.0]))],
        )
        .unwrap();
    scene.connect("cam", None, ".root", "objects").unwrap();

    scene.create("screen", "screen").unwrap();
    scene
        .set_attribute(
            "screen",
            vec![arg(
                "resolution",
                Type::I32,
                OwnedData::I32(vec![width, height]),
            )],
        )
        .unwrap();
    scene.connect("screen", None, "cam", "screens").unwrap();

    scene.create("beauty", "outputlayer").unwrap();
    scene
        .set_attribute(
            "beauty",
            vec![arg(
                "variablename",
                Type::String,
                OwnedData::String(vec![b"Ci".to_vec()]),
            )],
        )
        .unwrap();
    scene
        .connect("beauty", None, "screen", "outputlayers")
        .unwrap();

    scene.create("driver", "outputdriver").unwrap();
    scene
        .set_attribute(
            "driver",
            vec![arg(
                "imagefilename",
                Type::String,
                OwnedData::String(vec![b"unused.exr".to_vec()]),
            )],
        )
        .unwrap();
    scene
        .connect("driver", None, "beauty", "outputdrivers")
        .unwrap();

    scene
}

/// **The gate.** A recorded ɴsɪ scene becomes pixels without a file
/// being written or a process being spawned.
#[test]
fn a_scene_renders_in_this_process() {
    let Some(dso) = dso_path() else {
        panic!(
            "set $NSI_MOONRAY_DSO to MoonRay's rdl2dso; without it no \
             MoonRay scene class resolves and this would pass on an \
             empty scene"
        );
    };

    let (width, height) = (64i32, 48i32);
    let (_guard, render) = renderer(&dso);

    // The renderer owns the scene: this is the context the frame will
    // be rendered from, not a copy handed across.
    let context = render.scene().expect("the renderer's own scene");
    let flushed = flush(&scene(width, height));
    let report = apply(&flushed.document, &context);

    assert!(
        !report.iter().any(|line| line.contains("no scene class")),
        "every MoonRay class must resolve from $NSI_MOONRAY_DSO: \
         {report:?}"
    );

    render.initialize().expect("render prep");
    render.start().expect("the frame starts");

    // Converge. `frame_complete` is what says nothing more is coming;
    // a fixed sleep would be a race that passes on a fast machine.
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(120);
    while !render.frame_complete() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(render.frame_complete(), "the frame did not finish in 120s");

    let (got_width, got_height, pixels) =
        render.snapshot().expect("a snapshot");
    render.stop().expect("the frame stops");

    assert_eq!((got_width, got_height), (width as u32, height as u32));
    assert_eq!(pixels.len(), (width * height * 4) as usize);
    // The quad covers the middle of frame, so the centre pixel is the
    // check that means something. "Any pixel is non-zero" would pass on
    // a stray sample or on an alpha channel alone.
    let centre =
        ((height as usize / 2) * width as usize + width as usize / 2) * 4;
    let rgb = &pixels[centre..centre + 3];

    assert!(
        rgb.iter().any(|value| *value > 0.0),
        "the centre of frame is black, where the quad is. MoonRay renders \
         nothing missing from the Layer, and nothing whose Layer row has \
         no material -- both look like this.\nrgba there: {:?}",
        &pixels[centre..centre + 4]
    );

    let lit = pixels
        .chunks_exact(4)
        .filter(|pixel| pixel[0] > 0.0 || pixel[1] > 0.0 || pixel[2] > 0.0)
        .count();
    assert!(
        lit > (width * height) as usize / 10,
        "only {lit} of {} pixels carry light, which is a stray sample \
         rather than a rendered quad",
        width * height
    );
}

/// Snapshotting before the frame is complete is what a viewport does,
/// and it must answer rather than block or fault.
#[test]
fn a_frame_can_be_snapshotted_while_it_converges() {
    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };

    let (_guard, render) = renderer(&dso);
    let context = render.scene().expect("a scene");
    let flushed = flush(&scene(64, 48));
    apply(&flushed.document, &context);

    render.initialize().expect("render prep");
    render.start().expect("the frame starts");

    // Whatever state it is in, a snapshot answers with a whole buffer.
    let (width, height, pixels) =
        render.snapshot().expect("a mid-flight snapshot");
    assert_eq!(pixels.len(), (width * height * 4) as usize);

    render.stop().expect("the frame stops");
}

/// **`T5.3`.** A converging render reaching an application's own
/// closures — the thing spawning a process made impossible.
///
/// No file is written and no ndspy struct is marshalled: MoonRay's
/// sample buffer and the application's `Fn` are a copy apart.
#[test]
fn a_converging_render_streams_to_the_applications_closures() {
    use nsi_ffi_wrap::{
        argument::CallbackPtr,
        output::{Error, PixelFormat, WriteCallback},
    };
    use nsi_intermediate::HostPointer;
    use nsi_moonray::stream::{Stopped, stream};
    use std::sync::{Arc, Mutex};

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };

    let canvas = Arc::new(Mutex::new(Canvas::default()));
    let painting = Arc::clone(&canvas);

    let write = WriteCallback::<f32>::new(
        move |_name,
              width,
              height,
              x0,
              x1,
              y0,
              y1,
              format: &PixelFormat,
              pixels: &[f32]| {
            painting.lock().expect("not poisoned").paint(
                width,
                height,
                x0,
                x1,
                y0,
                y1,
                format.channels(),
                pixels,
            );
            Error::None
        },
    );

    let (width, height) = (64i32, 48i32);
    let (_guard, render) = renderer(&dso);

    let context = render.scene().expect("the renderer's own scene");
    let mut nsi = scene(width, height);

    // The application's driver, exactly as an ɴsɪ consumer writes it.
    nsi.set_attribute(
        "driver",
        vec![OwnedArgument::new(
            "callback.write",
            Type::Reference,
            1,
            0,
            OwnedData::Reference(vec![HostPointer(write.to_ptr())]),
        )],
    )
    .unwrap();

    let flushed = flush(&nsi);
    apply(&flushed.document, &context);

    let callbacks = nsi_moonray::display::Callbacks::of(&nsi, "driver")
        .expect("the callback was recorded");

    render.initialize().expect("render prep");
    render.start().expect("the frame starts");

    let outcome = stream(
        &render,
        &callbacks,
        "driver",
        Some(std::time::Duration::from_secs(120)),
    )
    .expect("the loop runs");

    assert_eq!(outcome, Stopped::Complete, "the frame should finish");

    let painted = canvas.lock().expect("not poisoned");
    // The count is not asserted: it is a property of the host, not of
    // the code -- a fast enough machine could finish inside one poll.
    // That progressive delivery happens at all is what
    // `a_frame_can_be_snapshotted_while_it_converges` pins down.
    assert!(painted.buckets > 0, "the closure received nothing");
    assert_eq!(
        (painted.width, painted.height),
        (width as usize, height as usize),
        "the buckets should describe this frame"
    );

    let centre =
        ((height as usize / 2) * width as usize + width as usize / 2) * 4;
    assert!(
        painted.pixels[centre..centre + 3]
            .iter()
            .any(|value| *value > 0.0),
        "the composited frame has a black centre, where the quad is"
    );
}

/// A closure answering `Error::Stop` stops the render.
///
/// What it is for -- a viewport closing, a user cancelling -- and what
/// the file-delivery stopgap could not honour, because by then there
/// was nothing left to stop.
#[test]
fn a_callback_that_says_stop_stops_the_render() {
    use nsi_ffi_wrap::{
        argument::CallbackPtr,
        output::{Error, PixelFormat, WriteCallback},
    };
    use nsi_intermediate::HostPointer;
    use nsi_moonray::stream::{Stopped, stream};

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };

    let write = WriteCallback::<f32>::new(
        |_name,
         _w,
         _h,
         _x0,
         _x1,
         _y0,
         _y1,
         _format: &PixelFormat,
         _pixels: &[f32]| Error::Stop,
    );

    let (_guard, render) = renderer(&dso);
    let context = render.scene().expect("a scene");
    let mut nsi = scene(64, 48);
    nsi.set_attribute(
        "driver",
        vec![OwnedArgument::new(
            "callback.write",
            Type::Reference,
            1,
            0,
            OwnedData::Reference(vec![HostPointer(write.to_ptr())]),
        )],
    )
    .unwrap();

    apply(&flush(&nsi).document, &context);
    let callbacks =
        nsi_moonray::display::Callbacks::of(&nsi, "driver").expect("recorded");

    render.initialize().expect("render prep");
    render.start().expect("the frame starts");

    let outcome =
        stream(&render, &callbacks, "driver", None).expect("the loop runs");

    assert_eq!(outcome, Stopped::ByCallback);
}

/// **The drop-in path, linked.** An application driving the ɴsɪ C
/// entry points gets an in-process render and its pixels back, with no
/// file written and no `moonray` process spawned.
///
/// This is what all of `002` is for: the same calls a consumer already
/// makes against 3Delight, answered by a linked MoonRay.
#[test]
fn the_c_api_renders_in_process_and_returns_pixels() {
    use nsi_ffi_wrap::{
        argument::CallbackPtr,
        output::{Error, PixelFormat, WriteCallback},
    };
    use nsi_intermediate::HostPointer;
    use std::sync::{Arc, Mutex};

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    // SAFETY: single-threaded here, under the renderer lock.
    unsafe { std::env::set_var("NSI_MOONRAY_DSO", &dso) };

    let canvas = Arc::new(Mutex::new(Canvas::default()));
    let painting = Arc::clone(&canvas);

    let write = WriteCallback::<f32>::new(
        move |_name,
              width,
              height,
              x0,
              x1,
              y0,
              y1,
              format: &PixelFormat,
              pixels: &[f32]| {
            painting.lock().expect("not poisoned").paint(
                width,
                height,
                x0,
                x1,
                y0,
                y1,
                format.channels(),
                pixels,
            );
            Error::None
        },
    );

    // The renderer lock, because `NSIRenderControl` makes a `Render`.
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (width, height) = (64i32, 48i32);
    let mut nsi = scene(width, height);
    nsi.set_attribute(
        "driver",
        vec![OwnedArgument::new(
            "callback.write",
            Type::Reference,
            1,
            0,
            OwnedData::Reference(vec![HostPointer(write.to_ptr())]),
        )],
    )
    .unwrap();

    // Drive the C entry point over a context holding this scene, the
    // way `nsi-ffi-wrap` drives a renderer it loaded.
    assert!(
        nsi_moonray::capi::render_in_process(&nsi),
        "the linked renderer should have taken the scene"
    );

    let painted = canvas.lock().expect("not poisoned");
    assert!(painted.buckets > 0, "the C API delivered no pixels");
    assert_eq!(
        (painted.width, painted.height),
        (width as usize, height as usize)
    );

    let centre =
        ((height as usize / 2) * width as usize + width as usize / 2) * 4;
    assert!(
        painted.pixels[centre..centre + 3]
            .iter()
            .any(|value| *value > 0.0),
        "the C API delivered a black centre of frame"
    );
}

/// **`T5.4`.** A batch render — an output driver with no callbacks —
/// writes the file the scene names, through MoonRay's own output
/// machinery.
///
/// Checked by *reading the image back*, not by a file appearing: an
/// empty or black EXR would satisfy the weaker check, and both are
/// things this has produced before.
#[test]
fn a_batch_render_writes_the_image_it_was_asked_for() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let directory = scratch("nsi-moonray-inprocess");
    std::fs::create_dir_all(&directory).expect("a writable directory");
    let image = directory.join("batch.exr");
    let _ = std::fs::remove_file(&image);

    let (width, height) = (64i32, 48i32);
    let mut nsi = scene(width, height);
    // No callbacks on the driver: this is a batch render.
    nsi.set_attribute(
        "driver",
        vec![OwnedArgument::new(
            "imagefilename",
            Type::String,
            1,
            0,
            OwnedData::String(vec![
                image.to_string_lossy().as_bytes().to_vec(),
            ]),
        )],
    )
    .unwrap();

    let mut session = Session::new(nsi, &dso).expect("a render");
    session.wait();
    drop(session);

    assert!(image.exists(), "no image at {}", image.display());

    use exr::prelude::{ReadChannels, ReadLayers};
    let read = exr::prelude::read()
        .no_deep_data()
        .largest_resolution_level()
        .all_channels()
        .first_valid_layer()
        .all_attributes()
        .from_file(&image)
        .expect("the written image reads back");

    let layer = &read.layer_data;
    assert_eq!(
        (layer.size.width(), layer.size.height()),
        (width as usize, height as usize)
    );

    let lit = layer.channel_data.list.iter().any(|channel| {
        (0..layer.size.width() * layer.size.height())
            .any(|i| channel.sample_data.value_by_flat_index(i).to_f32() > 0.0)
    });
    assert!(lit, "the written image is entirely black");
}

/// One instance matrix: a translation.
#[rustfmt::skip]
fn instance_at(x: f64, z: f64) -> Vec<f64> {
    vec![
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
          x, 0.0,   z, 1.0,
    ]
}

/// How much light a column carries.
fn column(pixels: &[f32], width: usize, height: usize, x: usize) -> f32 {
    (0..height)
        .map(|y| {
            let i = (y * width + x) * 4;
            pixels[i] + pixels[i + 1] + pixels[i + 2]
        })
        .sum()
}

/// The covered rectangle in a frame.
///
/// Alpha is what says a pixel is covered; colour would make this a
/// measurement of the light as well as of the geometry. The cut is at
/// **half the frame's own maximum** rather than at a fixed value: a
/// flat quad lit by one environment carries the same alpha everywhere
/// inside it, whatever that value turns out to be, and half of it is
/// the half-covered contour -- which is what 3Delight's box filter
/// draws too.
fn covered(
    pixels: &[f32],
    width: usize,
    height: usize,
) -> Option<(usize, usize, usize, usize)> {
    let peak = pixels
        .iter()
        .skip(3)
        .step_by(4)
        .fold(0.0f32, |peak, alpha| peak.max(*alpha));
    if peak <= 0.0 {
        return None;
    }

    let (mut left, mut right, mut top, mut bottom) =
        (usize::MAX, 0usize, usize::MAX, 0usize);

    for y in 0..height {
        for x in 0..width {
            if pixels[(y * width + x) * 4 + 3] > peak * 0.5 {
                left = left.min(x);
                right = right.max(x);
                top = top.min(y);
                bottom = bottom.max(y);
            }
        }
    }

    (left != usize::MAX).then_some((left, right, top, bottom))
}

/// **`T1.6`.** The frame matches 3Delight's, so `fov` is vertical here
/// too.
///
/// ɴsɪ's specification says only "the field of view angle, in degrees",
/// and reading it as horizontal is an entirely plausible mistake that
/// renders a plausible picture -- just framed wrong, in a way that
/// looks like the camera was placed differently.
///
/// So this is the same probe 3Delight was measured with
/// (`tools/probe/framing.nsi`, `research.md` F11): a quad of half-extent
/// 1, one unit in front of the camera, `fov` 90, on a 400x200 frame
/// where the two axes cannot be confused. 3Delight lit x 100..299 and
/// y 0..199 -- the full height and half the width. Were `fov`
/// horizontal, the quad would fill the width instead.
#[test]
fn the_frame_matches_3delights_framing() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (width, height) = (400usize, 200usize);
    let mut nsi = scene(width as i32, height as i32);

    nsi.set_attribute(
        "quad",
        vec![arg(
            "P",
            Type::Point,
            OwnedData::F32(vec![
                -1.0, -1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0, 1.0,
                -1.0,
            ]),
        )],
    )
    .unwrap();
    nsi.set_attribute(
        "cam",
        vec![arg("fov", Type::F32, OwnedData::F32(vec![90.0]))],
    )
    .unwrap();

    let mut session = Session::new(nsi, &dso).expect("a render");
    session.wait();
    let pixels = session.render().snapshot().expect("a frame").2;

    let (left, right, top, bottom) =
        covered(&pixels, width, height).expect("the quad should be drawn");

    // A pixel of slack at each edge: the two renderers do not have to
    // agree on where a partly covered pixel tips over, only on where
    // the quad is.
    let close = |got: usize, want: usize, what: &str| {
        assert!(
            got.abs_diff(want) <= 1,
            "{what}: {got}, 3Delight said {want} \
             (x {left}..{right}, y {top}..{bottom})"
        );
    };

    close(top, 0, "the top of the quad");
    close(bottom, height - 1, "the bottom of the quad");
    close(left, 100, "the left of the quad");
    close(right, 299, "the right of the quad");
}

/// **`T1.7a`.** An ɴsɪ light lights the scene.
///
/// ɴsɪ has no light nodes: geometry wearing an emitter *is* the light
/// (specification 4.5), and `LIGHTS` recognises the emitter by name.
/// The mapping is asserted as text in `flush::tests`; this is the part
/// text cannot reach -- a light that never reaches MoonRay's light set
/// emits a perfectly correct scene and renders black.
///
/// A `pointLight` rather than an `areaLight` because the two differ
/// only in which row of `LIGHTS` matches, and MoonRay's `MeshLight`
/// pulls in a `DwaBaseMaterial` that ships with `moonshine_dwa` rather
/// than with `moonray` (`research.md` F12), which this build does not
/// have.
///
/// The scene's own environment is disconnected, so the only thing that
/// can light the quad is the lamp beside it.
#[test]
fn a_light_shader_lights_the_scene() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (width, height) = (64usize, 48usize);
    let mut nsi = scene(width as i32, height as i32);

    nsi.disconnect("light", None, ".root", "objects").unwrap();

    // An ɴsɪ point light: "an epsilon sized geometry (a small disk, a
    // particle, etc.)" wearing a shader that emits, placed by a
    // transform. To the right of the quad and in front of it.
    nsi.create("lampxf", "transform").unwrap();
    nsi.set_attribute(
        "lampxf",
        vec![arg(
            "transformationmatrix",
            Type::MatrixF64,
            OwnedData::F64(vec![
                1.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                3.0, 0.0, -4.0, 1.0,
            ]),
        )],
    )
    .unwrap();
    nsi.connect("lampxf", None, ".root", "objects").unwrap();

    nsi.create("lamp", "mesh").unwrap();
    nsi.set_attribute(
        "lamp",
        vec![
            arg("nvertices", Type::I32, OwnedData::I32(vec![3])),
            arg("P.indices", Type::I32, OwnedData::I32(vec![0, 1, 2])),
            arg(
                "P",
                Type::Point,
                OwnedData::F32(vec![
                    0.0, 0.0, 0.0, 0.01, 0.0, 0.0, 0.0, 0.01, 0.0,
                ]),
            ),
        ],
    )
    .unwrap();
    nsi.connect("lamp", None, "lampxf", "objects").unwrap();

    nsi.create("lampattr", "attributes").unwrap();
    nsi.create("emit", "shader").unwrap();
    nsi.set_attribute(
        "emit",
        vec![
            arg(
                "shaderfilename",
                Type::String,
                OwnedData::String(vec![b"pointLight".to_vec()]),
            ),
            arg("intensity", Type::F32, OwnedData::F32(vec![40.0])),
        ],
    )
    .unwrap();
    nsi.connect("lampattr", None, "lamp", "geometryattributes")
        .unwrap();
    nsi.connect("emit", None, "lampattr", "surfaceshader")
        .unwrap();

    let mut session = Session::new(nsi, &dso).expect("a render");
    session.wait();
    let pixels = session.render().snapshot().expect("a frame").2;

    // Columns inside the quad: at z = -5 with a 45-degree vertical
    // field of view it covers roughly the middle third of the frame,
    // so the quarter columns other tests use fall outside it.
    let right = column(&pixels, width, height, width * 5 / 8);
    let left = column(&pixels, width, height, width * 3 / 8);

    assert!(
        right > 0.0,
        "the ɴsɪ light should light the quad: left {left}, right {right}"
    );
    // The lamp is to the right, so that side is the brighter one. An
    // evenly lit frame would mean something else lit it.
    assert!(
        right > left * 1.1,
        "the side facing the lamp should be brighter: left {left}, \
         right {right}"
    );
}

/// **`TN.2`.** An ɴsɪ shader renders, as itself.
///
/// The whole chain, and nothing in it is a substitute: an ɴsɪ `shader`
/// node naming a compiled `.oso`, flushed to an OSL group
/// specification, carried in one rdl2 `String` attribute, parsed by
/// OSL, executed at every shading point, walked into `BsdfBuilder`
/// calls, and lit.
///
/// The shader's colour is asserted per channel, because that is the
/// only thing that can tell "OSL ran" from "something plausible
/// happened": a `UsdPreviewSurface` at its defaults renders a
/// perfectly good grey quad.
///
/// Needs the crate built with `$OSL_ROOT`, which is what puts the
/// `Osl` material on MoonRay's DSO path.
#[cfg(osl)]
#[test]
fn an_nsi_osl_shader_renders() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    // A shader compiled here rather than shipped: what is being tested
    // is that an arbitrary OSL shader crosses, so it has to be one
    // this crate has never seen.
    let directory = scratch("nsi-moonray-osl-render");
    std::fs::create_dir_all(&directory).expect("a writable directory");
    let source = directory.join("teal.osl");
    std::fs::write(
        &source,
        "surface teal(color tint = color(1, 1, 1))\n\
         {\n    Ci = tint * diffuse(N);\n}\n",
    )
    .expect("the shader is written");

    let oslc = std::path::Path::new(env!("OSL_ROOT")).join("bin/oslc");
    let compiled = std::process::Command::new(&oslc)
        .arg("-o")
        .arg(directory.join("teal.oso"))
        .arg(&source)
        .output()
        .expect("oslc runs");
    assert!(
        compiled.status.success(),
        "oslc failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let (width, height) = (64usize, 48usize);
    let mut nsi = scene(width as i32, height as i32);

    nsi.create("attr", "attributes").unwrap();
    nsi.create("teal", "shader").unwrap();
    nsi.set_attribute(
        "teal",
        vec![
            arg(
                "shaderfilename",
                Type::String,
                OwnedData::String(vec![
                    directory
                        .join("teal.oso")
                        .to_string_lossy()
                        .into_owned()
                        .into_bytes(),
                ]),
            ),
            // A parameter no table in this crate has ever heard of.
            arg("tint", Type::Color, OwnedData::F32(vec![0.05, 0.7, 0.6])),
        ],
    )
    .unwrap();
    nsi.connect("attr", None, "quad", "geometryattributes")
        .unwrap();
    nsi.connect("teal", None, "attr", "surfaceshader").unwrap();

    let mut session = Session::new(nsi, &dso).expect("a render");
    session.wait();
    let (_, _, pixels) = session.render().snapshot().expect("a frame");

    // The centre of the quad, per channel.
    let centre = ((height / 2) * width + width / 2) * 4;
    let (red, green, blue) =
        (pixels[centre], pixels[centre + 1], pixels[centre + 2]);

    assert!(
        green > 0.0,
        "the shader should have shaded something: {red} {green} {blue}"
    );
    // `tint` is 0.05, 0.7, 0.6 — green well above blue, and red far
    // below both. A default surface would be grey and fail all three.
    assert!(
        green > red * 5.0,
        "green should dominate red: {red} {green} {blue}"
    );
    assert!(
        blue > red * 5.0,
        "blue should dominate red: {red} {green} {blue}"
    );
    assert!(
        green > blue,
        "green should exceed blue, as `tint` says: {red} {green} {blue}"
    );
}

/// **A transform in a shader transforms.**
///
/// `RendererServices::get_matrix` returning identity is not an error
/// anywhere: OSL asks, gets a matrix, and shades. It renders a
/// plausible picture of the wrong coordinate system, which is the
/// failure mode this whole backend is written against — so this has to
/// be a test that can only pass if the matrix is real.
///
/// **Rotation, not translation.** MoonRay's render space follows the
/// camera, so moving an object and the camera together leaves
/// render-space `P` unchanged and an identity matrix looks correct. A
/// first version of this test did exactly that and passed with
/// `get_matrix` stubbed out to identity, which is the only reason it
/// is written this way.
///
/// The quad is rotated 90° about z, so its **image footprint is
/// unchanged** and only the shading can differ: object `(0.8, 0)` maps
/// to render `(0, 0.8)`, the top of the frame. A shader colouring by
/// `abs(P.x)` in object space is therefore bright at the top of the
/// quad and dark at its centre; in render space it is dark at both.
#[cfg(osl)]
#[test]
fn a_transform_in_a_shader_transforms() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let directory = scratch("nsi-moonray-osl-xform");
    std::fs::create_dir_all(&directory).expect("a writable directory");
    let source = directory.join("where.osl");
    std::fs::write(
        &source,
        "surface where()\n\
         {\n    point q = transform(\"object\", P);\n\
         \x20   Ci = color(abs(q[0]), 0, 0) * diffuse(N);\n}\n",
    )
    .expect("the shader is written");

    let oslc = std::path::Path::new(env!("OSL_ROOT")).join("bin/oslc");
    let compiled = std::process::Command::new(&oslc)
        .arg("-o")
        .arg(directory.join("where.oso"))
        .arg(&source)
        .output()
        .expect("oslc runs");
    assert!(
        compiled.status.success(),
        "oslc failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let (width, height) = (64usize, 48usize);
    let mut nsi = scene(width as i32, height as i32);

    // 90° about z, so object x becomes render y.
    nsi.disconnect("quad", None, ".root", "objects").unwrap();
    nsi.create("xform", "transform").unwrap();
    nsi.set_attribute(
        "xform",
        vec![arg(
            "transformationmatrix",
            Type::MatrixF64,
            OwnedData::F64(vec![
                0.0, 1.0, 0.0, 0.0, //
                -1.0, 0.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                0.0, 0.0, 0.0, 1.0,
            ]),
        )],
    )
    .unwrap();
    nsi.connect("xform", None, ".root", "objects").unwrap();
    nsi.connect("quad", None, "xform", "objects").unwrap();

    nsi.create("attr", "attributes").unwrap();
    nsi.create("where", "shader").unwrap();
    nsi.set_attribute(
        "where",
        vec![arg(
            "shaderfilename",
            Type::String,
            OwnedData::String(vec![
                directory
                    .join("where.oso")
                    .to_string_lossy()
                    .into_owned()
                    .into_bytes(),
            ]),
        )],
    )
    .unwrap();
    nsi.connect("attr", None, "quad", "geometryattributes")
        .unwrap();
    nsi.connect("where", None, "attr", "surfaceshader").unwrap();

    let mut session = Session::new(nsi, &dso).expect("a render");
    session.wait();
    let (_, _, pixels) = session.render().snapshot().expect("a frame");

    let red = |row: usize| pixels[(row * width + width / 2) * 4];

    // The quad covers roughly the middle half of the frame, so row 16
    // is well inside its upper half and row 24 is its centre.
    let upper = red(16);
    let centre = red(24);

    assert!(
        upper > centre + 0.1,
        "object space should have rotated with the object: {upper} at \
         the top of the quad against {centre} at its centre. Equal \
         means the shader was handed render space, where `abs(P.x)` is \
         near zero all the way up the middle."
    );
}

/// **`T6.6`.** An instanced scene renders — two copies of one
/// prototype, in two places.
///
/// The mapping was asserted as text (`flush::tests`); this is what it
/// looks like. It is also the only thing that can catch a prototype
/// drawn once at the origin instead of twice where its matrices put
/// it, which is what the backend did before instancing was mapped at
/// all.
#[test]
fn an_instanced_scene_renders_its_copies() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (width, height) = (64usize, 48usize);
    let mut nsi = scene(width as i32, height as i32);

    // The scene's own quad is in the way; hide it by detaching, which
    // is now a visibility change rather than a removal.
    nsi.disconnect("quad", None, ".root", "objects").unwrap();

    // A prototype, placed twice.
    nsi.create("proto", "mesh").unwrap();
    nsi.set_attribute(
        "proto",
        vec![
            arg("nvertices", Type::I32, OwnedData::I32(vec![4])),
            arg("P.indices", Type::I32, OwnedData::I32(vec![0, 1, 2, 3])),
            arg(
                "P",
                Type::Point,
                OwnedData::F32(vec![
                    -1.0, -1.0, 0.0, 1.0, -1.0, 0.0, 1.0, 1.0, 0.0, -1.0, 1.0,
                    0.0,
                ]),
            ),
        ],
    )
    .unwrap();

    nsi.create("inst", "instances").unwrap();
    nsi.connect("inst", None, ".root", "objects").unwrap();
    nsi.connect("proto", None, "inst", "sourcemodels").unwrap();

    let mut matrices = instance_at(-2.0, -8.0);
    matrices.extend(instance_at(2.0, -8.0));
    nsi.set_attribute(
        "inst",
        vec![arg(
            "transformationmatrices",
            Type::MatrixF64,
            OwnedData::F64(matrices),
        )],
    )
    .unwrap();

    let mut session = Session::new(nsi, &dso).expect("a render");
    session.wait();
    let pixels = session.render().snapshot().expect("a frame").2;

    // At z = -8 with a 45-degree vertical field of view, x = ±2 lands
    // around a quarter and three quarters of the way across, and the
    // centre column falls between the two copies.
    let left = column(&pixels, width, height, width / 4);
    let centre = column(&pixels, width, height, width / 2);
    let right = column(&pixels, width, height, width * 3 / 4);

    assert!(
        left > 0.0 && right > 0.0,
        "both instances should be drawn: left {left}, right {right}"
    );
    assert!(
        centre < left * 0.5 && centre < right * 0.5,
        "the gap between the two instances should be darker than \
         either: left {left}, centre {centre}, right {right}"
    );
}

/// **`T6.2`.** A prototype's own transform is applied exactly once.
///
/// MoonRay generates a referenced geometry at **identity** and reads
/// its `node_xform` back separately, gated on `use_reference_xforms`
/// (`rt/GeometryManager.cc`). So the prototype's own chain can be
/// dropped or applied twice, and both look like a plausible render of
/// something. Only measuring where the copies land settles it.
///
/// The prototype sits one unit right of its instancer's origin, and
/// the instances are placed at -3 and +3. Applied once, the left copy
/// is centred on -2; dropped, on -3; doubled, on -1. Those are three
/// distinguishable places in the frame.
#[test]
fn a_prototypes_own_transform_is_applied_once() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (width, height) = (64usize, 48usize);
    let mut nsi = scene(width as i32, height as i32);
    nsi.disconnect("quad", None, ".root", "objects").unwrap();

    nsi.create("proto", "mesh").unwrap();
    nsi.set_attribute(
        "proto",
        vec![
            arg("nvertices", Type::I32, OwnedData::I32(vec![4])),
            arg("P.indices", Type::I32, OwnedData::I32(vec![0, 1, 2, 3])),
            arg(
                "P",
                Type::Point,
                OwnedData::F32(vec![
                    -1.0, -1.0, 0.0, 1.0, -1.0, 0.0, 1.0, 1.0, 0.0, -1.0, 1.0,
                    0.0,
                ]),
            ),
        ],
    )
    .unwrap();

    nsi.create("inst", "instances").unwrap();
    nsi.connect("inst", None, ".root", "objects").unwrap();

    // The prototype's own transform, below the instancer.
    nsi.create("proto_xform", "transform").unwrap();
    nsi.set_attribute(
        "proto_xform",
        vec![arg(
            "transformationmatrix",
            Type::MatrixF64,
            OwnedData::F64(instance_at(1.0, 0.0)),
        )],
    )
    .unwrap();
    nsi.connect("proto_xform", None, "inst", "sourcemodels")
        .unwrap();
    nsi.connect("proto", None, "proto_xform", "objects")
        .unwrap();

    let mut matrices = instance_at(-3.0, -8.0);
    matrices.extend(instance_at(3.0, -8.0));
    nsi.set_attribute(
        "inst",
        vec![arg(
            "transformationmatrices",
            Type::MatrixF64,
            OwnedData::F64(matrices),
        )],
    )
    .unwrap();

    let mut session = Session::new(nsi, &dso).expect("a render");
    session.wait();
    let pixels = session.render().snapshot().expect("a frame").2;

    let at = |x: usize| column(&pixels, width, height, x);
    // Centred on -2 spans roughly columns 10 to 25.
    assert!(at(14) > 0.0 && at(22) > 0.0, "the left copy is missing");
    assert!(
        at(5) == 0.0,
        "light at column 5 means the prototype's transform was \
         *dropped* — the copy is centred on -3, not -2: {}",
        at(5)
    );
    assert!(
        at(30) == 0.0,
        "light at column 30 means the prototype's transform was applied \
         *twice* — the copy is centred on -1, not -2: {}",
        at(30)
    );
}

/// **`T2.3`, rendered.** A deforming mesh smears.
///
/// The emission is asserted as text in `flush::tests`; this is that it
/// reaches the image. A smear leaves partially covered columns where a
/// sharp edge leaves none, which is the same measure the transform-blur
/// test uses.
#[test]
fn a_deforming_mesh_renders_blurred() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (width, height) = (64usize, 48usize);

    // The same quad, once static and once deforming across the shutter.
    let quad_at = |x: f32| {
        vec![
            x - 1.0,
            -1.0,
            -6.0,
            x + 1.0,
            -1.0,
            -6.0,
            x + 1.0,
            1.0,
            -6.0,
            x - 1.0,
            1.0,
            -6.0,
        ]
    };

    let frame_of = |deforming: bool| {
        let mut nsi = scene(width as i32, height as i32);
        nsi.disconnect("quad", None, ".root", "objects").unwrap();

        nsi.create("shape", "mesh").unwrap();
        nsi.set_attribute(
            "shape",
            vec![
                arg("nvertices", Type::I32, OwnedData::I32(vec![4])),
                arg("P.indices", Type::I32, OwnedData::I32(vec![0, 1, 2, 3])),
            ],
        )
        .unwrap();
        nsi.connect("shape", None, ".root", "objects").unwrap();

        if deforming {
            for (time, x) in [(0.0, -1.0f32), (1.0, 1.0f32)] {
                nsi.set_attribute_at_time(
                    "shape",
                    time,
                    vec![arg("P", Type::Point, OwnedData::F32(quad_at(x)))],
                )
                .unwrap();
            }
        } else {
            nsi.set_attribute(
                "shape",
                vec![arg("P", Type::Point, OwnedData::F32(quad_at(-1.0)))],
            )
            .unwrap();
        }

        let mut session = Session::new(nsi, &dso).expect("a render");
        session.wait();
        session.render().snapshot().expect("a frame").2
    };

    // Columns that are lit but not fully lit — the signature of an edge
    // that moved across them.
    let partial = |pixels: &[f32]| {
        let brightest = (0..width)
            .map(|x| column(pixels, width, height, x))
            .fold(0.0f32, f32::max);
        (0..width)
            .filter(|x| {
                let light = column(pixels, width, height, *x);
                light > brightest * 0.05 && light < brightest * 0.95
            })
            .count()
    };

    let sharp = frame_of(false);
    let blurred = frame_of(true);

    assert!(
        partial(&blurred) > partial(&sharp),
        "a deforming quad should leave more partially covered columns \
         than a static one: {} sharp, {} blurred",
        partial(&sharp),
        partial(&blurred)
    );
}

/// **`T6.4`.** Instancers nest.
///
/// ɴsɪ connects an `instances` node to another's `sourcemodels`, and
/// MoonRay's `fillGenerateList` walks `references` recursively — so
/// the nesting works through the same mechanism that stops a prototype
/// drawing on its own, with nothing extra to map.
///
/// Two inner copies placed by two outer instances is four shapes from
/// one mesh, which is the memory win the whole mapping exists for.
#[test]
fn instancers_nest() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (width, height) = (96usize, 48usize);
    let mut nsi = scene(width as i32, height as i32);
    nsi.disconnect("quad", None, ".root", "objects").unwrap();

    // One small prototype.
    nsi.create("proto", "mesh").unwrap();
    nsi.set_attribute(
        "proto",
        vec![
            arg("nvertices", Type::I32, OwnedData::I32(vec![4])),
            arg("P.indices", Type::I32, OwnedData::I32(vec![0, 1, 2, 3])),
            arg(
                "P",
                Type::Point,
                OwnedData::F32(vec![
                    -0.4, -0.4, 0.0, 0.4, -0.4, 0.0, 0.4, 0.4, 0.0, -0.4, 0.4,
                    0.0,
                ]),
            ),
        ],
    )
    .unwrap();

    // Inner instancer: two copies, close together.
    nsi.create("inner", "instances").unwrap();
    nsi.connect("proto", None, "inner", "sourcemodels").unwrap();
    let mut inner = instance_at(-0.7, 0.0);
    inner.extend(instance_at(0.7, 0.0));
    nsi.set_attribute(
        "inner",
        vec![arg(
            "transformationmatrices",
            Type::MatrixF64,
            OwnedData::F64(inner),
        )],
    )
    .unwrap();

    // Outer instancer: two copies of the *inner instancer*, far apart.
    nsi.create("outer", "instances").unwrap();
    nsi.connect("outer", None, ".root", "objects").unwrap();
    nsi.connect("inner", None, "outer", "sourcemodels").unwrap();
    let mut outer = instance_at(-3.0, -8.0);
    outer.extend(instance_at(3.0, -8.0));
    nsi.set_attribute(
        "outer",
        vec![arg(
            "transformationmatrices",
            Type::MatrixF64,
            OwnedData::F64(outer),
        )],
    )
    .unwrap();

    let mut session = Session::new(nsi, &dso).expect("a render");
    session.wait();
    let pixels = session.render().snapshot().expect("a frame").2;

    // Four shapes: two clusters of two. Count the runs of lit columns —
    // one nesting level would give two, and none would give one.
    let lit: Vec<bool> = (0..width)
        .map(|x| column(&pixels, width, height, x) > 0.0)
        .collect();
    let clusters = lit
        .iter()
        .enumerate()
        .filter(|(x, on)| **on && (*x == 0 || !lit[x - 1]))
        .count();

    assert_eq!(
        clusters, 4,
        "two outer instances of a two-instance inner instancer is four \
         shapes; {clusters} run(s) of lit columns were found, which \
         means the nesting collapsed"
    );
}

/// **`T3.2`.** Subdivision reaches the limit surface, not the cage.
///
/// `is_subd` being set is asserted as text elsewhere. This is that
/// MoonRay *acts* on it: a cube's Catmull-Clark limit surface rounds
/// inward toward a sphere, so the same cage covers measurably fewer
/// pixels subdivided than as a polygon mesh.
///
/// A **cube**, not a flat grid. The first version of this used a
/// planar 2x2 grid and both renders covered exactly 3598 pixels — a
/// planar cage subdivides to itself, and with sharp boundaries the
/// outline is preserved exactly. The subject has to be closed and
/// non-planar for the limit surface to differ at the silhouette.
///
/// The failure this catches is the quiet one — a subdivision surface
/// rendered as its faceted cage is a perfectly good render of the
/// wrong thing, and it is what this backend did before
/// `subdivision.scheme` was understood to be an attribute rather than
/// a node type.
#[test]
fn subdivision_reaches_the_limit_surface() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (width, height) = (96usize, 96usize);

    // A coarse cage: the coarser it is, the further the limit surface
    // pulls in from it, and the clearer the difference.
    let cage = |subdivided: bool| {
        let mut nsi = scene(width as i32, height as i32);
        nsi.disconnect("quad", None, ".root", "objects").unwrap();

        nsi.create("shape", "mesh").unwrap();
        // A cube: closed, so the limit surface has no boundary to
        // pin it to the cage, and it rounds toward a sphere.
        let c = 1.2f32;
        let z = -6.0f32;
        let mut attributes = vec![
            arg(
                "nvertices",
                Type::I32,
                OwnedData::I32(vec![4, 4, 4, 4, 4, 4]),
            ),
            arg(
                "P.indices",
                Type::I32,
                OwnedData::I32(vec![
                    0, 1, 2, 3, // back
                    4, 7, 6, 5, // front
                    0, 4, 5, 1, // bottom
                    3, 2, 6, 7, // top
                    0, 3, 7, 4, // left
                    1, 5, 6, 2, // right
                ]),
            ),
            arg(
                "P",
                Type::Point,
                OwnedData::F32(vec![
                    -c,
                    -c,
                    z - c,
                    c,
                    -c,
                    z - c,
                    c,
                    c,
                    z - c,
                    -c,
                    c,
                    z - c,
                    -c,
                    -c,
                    z + c,
                    c,
                    -c,
                    z + c,
                    c,
                    c,
                    z + c,
                    -c,
                    c,
                    z + c,
                ]),
            ),
        ];
        if subdivided {
            attributes.push(OwnedArgument::new(
                "subdivision.scheme",
                Type::String,
                1,
                0,
                OwnedData::String(vec![b"catmull-clark".to_vec()]),
            ));
        }
        nsi.set_attribute("shape", attributes).unwrap();
        nsi.connect("shape", None, ".root", "objects").unwrap();

        let mut session = Session::new(nsi, &dso).expect("a render");
        session.wait();
        session.render().snapshot().expect("a frame").2
    };

    let covered =
        |pixels: &[f32]| pixels.chunks_exact(4).filter(|p| p[0] > 0.0).count();

    let polygon = covered(&cage(false));
    let subdivided = covered(&cage(true));

    assert!(polygon > 0, "the polygon cage should render at all");
    assert!(
        subdivided < polygon,
        "a Catmull-Clark limit surface pulls in from its cage, so it \
         must cover fewer pixels. Equal coverage means the cage was \
         rendered and `is_subd` was ignored: {polygon} polygon, \
         {subdivided} subdivided"
    );
    assert!(
        subdivided > polygon / 3,
        "it should round in, not vanish: {polygon} polygon, \
         {subdivided} subdivided"
    );
}

/// **`T6.3`, rendered.** A moving instancer smears.
///
/// The velocities are asserted as numbers in `flush::tests`; this is
/// that they reach the image. `xform_list` carries no timesteps, so
/// this is MoonRay's `position + velocity * dt` path rather than the
/// `blur(a, b)` one every other moving thing uses — a different
/// mechanism, and worth seeing work.
#[test]
fn a_moving_instancer_renders_blurred() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (width, height) = (96usize, 48usize);

    let frame_of = |moving: bool| {
        let mut nsi = scene(width as i32, height as i32);
        nsi.disconnect("quad", None, ".root", "objects").unwrap();

        nsi.create("proto", "mesh").unwrap();
        nsi.set_attribute(
            "proto",
            vec![
                arg("nvertices", Type::I32, OwnedData::I32(vec![4])),
                arg("P.indices", Type::I32, OwnedData::I32(vec![0, 1, 2, 3])),
                arg(
                    "P",
                    Type::Point,
                    OwnedData::F32(vec![
                        -0.5, -0.5, 0.0, 0.5, -0.5, 0.0, 0.5, 0.5, 0.0, -0.5,
                        0.5, 0.0,
                    ]),
                ),
            ],
        )
        .unwrap();

        nsi.create("inst", "instances").unwrap();
        nsi.connect("inst", None, ".root", "objects").unwrap();
        nsi.connect("proto", None, "inst", "sourcemodels").unwrap();

        if moving {
            for (time, x) in [(0.0, -1.5), (1.0, 1.5)] {
                nsi.set_attribute_at_time(
                    "inst",
                    time,
                    vec![arg(
                        "transformationmatrices",
                        Type::MatrixF64,
                        OwnedData::F64(instance_at(x, -8.0)),
                    )],
                )
                .unwrap();
            }
        } else {
            nsi.set_attribute(
                "inst",
                vec![arg(
                    "transformationmatrices",
                    Type::MatrixF64,
                    OwnedData::F64(instance_at(-1.5, -8.0)),
                )],
            )
            .unwrap();
        }

        let mut session = Session::new(nsi, &dso).expect("a render");
        session.wait();
        session.render().snapshot().expect("a frame").2
    };

    // Columns lit but not fully lit: the signature of an edge that
    // swept across them.
    let partial = |pixels: &[f32]| {
        let brightest = (0..width)
            .map(|x| column(pixels, width, height, x))
            .fold(0.0f32, f32::max);
        (0..width)
            .filter(|x| {
                let light = column(pixels, width, height, *x);
                light > brightest * 0.05 && light < brightest * 0.95
            })
            .count()
    };

    let still = frame_of(false);
    let moving = frame_of(true);

    assert!(
        partial(&moving) > partial(&still),
        "a moving instancer should leave more partially covered \
         columns than a still one: {} still, {} moving",
        partial(&still),
        partial(&moving)
    );
}

/// **`T5.3a`.** A delta snapshot names the rectangle that changed, and
/// its pixels agree with a full snapshot of the same frame.
///
/// The agreement is the whole test. `snapshotDelta` does "no resize, no
/// extrapolation and no untiling" and its buffer is *not normalized by
/// weight*, so the shim undoes the tiling and divides each pixel by its
/// own sample count. Both of those have wrong versions that look
/// plausible — a mis-untiled frame is scrambled, an unnormalised one is
/// merely darker — and comparing against `snapshot`, which MoonRay
/// normalises and untiles itself, catches either.
#[test]
fn a_delta_snapshot_agrees_with_a_full_one() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (width, height) = (64usize, 48usize);
    let session = Session::new(scene(width as i32, height as i32), &dso)
        .expect("a render");

    // Converge, but do not stop: a stopped frame has nothing to
    // snapshot a delta against.
    while !session.render().frame_complete() {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let (_, _, full) = session.render().snapshot().expect("a full frame");
    let delta = session
        .render()
        .snapshot_delta()
        .expect("a delta")
        .expect("the first delta covers the whole frame");

    // Nothing has been snapshotted as a delta before, so everything is
    // new.
    assert_eq!(
        (delta.x, delta.y, delta.width, delta.height),
        (0, 0, width as u32, height as u32),
        "the first delta should cover the frame"
    );
    assert_eq!(delta.pixels.len(), width * height * 4);

    // The comparison. A tolerance, not equality: the two snapshots are
    // taken a moment apart from a live renderer, and MoonRay's own
    // normalisation is not bit-identical to dividing by the weight.
    let mut worst = 0.0f32;
    for (a, b) in full.iter().zip(&delta.pixels) {
        worst = worst.max((a - b).abs());
    }
    assert!(
        worst < 0.01,
        "a delta snapshot must agree with a full one; the worst \
         channel differs by {worst}. A scrambled frame means the \
         untiling is wrong, and a uniformly darker one means the \
         weight normalisation is."
    );

    // And it is not comparing two black frames.
    assert!(
        full.iter().any(|value| *value > 0.0),
        "the frame is black, so the comparison proves nothing"
    );

    // A second delta with nothing new between reports nothing.
    let again = session.render().snapshot_delta().expect("a second delta");
    assert!(
        again.is_none(),
        "a converged frame that nobody rendered into should have no \
         changed pixels: {again:?}"
    );

    let _ = session.render().stop();
}

/// **A MaterialX closure becomes a MoonRay lobe.**
///
/// The MaterialX closures are a second, parallel vocabulary in OSL:
/// `oren_nayar_diffuse_bsdf` carries its own albedo rather than being
/// multiplied by one, and its parameters sit at offsets this crate
/// declares in `register_closures`. A wrong offset does not fail --
/// `as<MxDiffuseParams>()` reads whatever is there -- so the albedo is
/// asserted per channel, exactly as the classic-closure test does, and
/// for the same reason.
///
/// Needs the crate built with `$OSL_ROOT`.
#[cfg(osl)]
#[test]
fn a_materialx_closure_renders() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let directory = scratch("nsi-moonray-osl-materialx");
    std::fs::create_dir_all(&directory).expect("a writable directory");
    let source = directory.join("mx.osl");
    // No `* tint` and no `diffuse()`: the closure is the whole shader,
    // so what reaches the frame can only have come through
    // `MxDiffuseParams`.
    std::fs::write(
        &source,
        "surface mx(color tint = color(1, 1, 1))\n\
         {\n    Ci = oren_nayar_diffuse_bsdf(N, tint, 0.0);\n}\n",
    )
    .expect("the shader is written");

    let oslc = std::path::Path::new(env!("OSL_ROOT")).join("bin/oslc");
    let compiled = std::process::Command::new(&oslc)
        .arg("-o")
        .arg(directory.join("mx.oso"))
        .arg(&source)
        .output()
        .expect("oslc runs");
    assert!(
        compiled.status.success(),
        "oslc failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let (width, height) = (64usize, 48usize);
    let mut nsi = scene(width as i32, height as i32);

    nsi.create("attr", "attributes").unwrap();
    nsi.create("mx", "shader").unwrap();
    nsi.set_attribute(
        "mx",
        vec![
            arg(
                "shaderfilename",
                Type::String,
                OwnedData::String(vec![
                    directory
                        .join("mx.oso")
                        .to_string_lossy()
                        .into_owned()
                        .into_bytes(),
                ]),
            ),
            arg("tint", Type::Color, OwnedData::F32(vec![0.05, 0.7, 0.6])),
        ],
    )
    .unwrap();
    nsi.connect("attr", None, "quad", "geometryattributes")
        .unwrap();
    nsi.connect("mx", None, "attr", "surfaceshader").unwrap();

    let mut session = Session::new(nsi, &dso).expect("a render");
    session.wait();
    let (_, _, pixels) = session.render().snapshot().expect("a frame");

    let centre = ((height / 2) * width + width / 2) * 4;
    let (red, green, blue) =
        (pixels[centre], pixels[centre + 1], pixels[centre + 2]);

    assert!(
        green > 0.0,
        "the MaterialX closure should have shaded something -- black \
         means it was never registered, so OSL dropped it: {red} \
         {green} {blue}"
    );
    // The same triple the classic-closure test uses, and the same
    // three questions of it: an albedo read from the wrong offset
    // fails at least one.
    assert!(
        green > red * 5.0,
        "green should dominate red: {red} {green} {blue}"
    );
    assert!(
        blue > red * 5.0,
        "blue should dominate red: {red} {green} {blue}"
    );
    assert!(
        green > blue,
        "green should exceed blue, as `tint` says: {red} {green} {blue}"
    );
}

/// **An OSL displacement moves the surface.**
///
/// A displacement is the one shader binding with no substitute: it
/// changes the *shape*, so a stand-in that shades plausibly and leaves
/// the vertices alone is not an approximation of it. What is asserted
/// is therefore the silhouette -- the covered area of the frame -- and
/// not the colour.
///
/// The quad is pushed half a unit along its normal, towards the camera,
/// which makes it cover more of the frame. Measured against
/// MoonRay's own `NormalDisplacement` at the same height first, so the
/// number below is what the renderer does rather than what this test
/// hopes for.
///
/// Needs the crate built with `$OSL_ROOT`.
#[cfg(osl)]
#[test]
fn an_osl_displacement_displaces() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let directory = scratch("nsi-moonray-osl-displace");
    std::fs::create_dir_all(&directory).expect("a writable directory");
    let source = directory.join("push.osl");
    std::fs::write(
        &source,
        "displacement push(float amount = 0.5)\n\
         {\n    P = P + amount * normalize(N);\n}\n",
    )
    .expect("the shader is written");

    let oslc = std::path::Path::new(env!("OSL_ROOT")).join("bin/oslc");
    let compiled = std::process::Command::new(&oslc)
        .arg("-o")
        .arg(directory.join("push.oso"))
        .arg(&source)
        .output()
        .expect("oslc runs");
    assert!(
        compiled.status.success(),
        "oslc failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    // The same scene twice: once as it is, once with the displacement
    // bound. Anything else that changed the coverage would change both.
    let (width, height) = (64usize, 48usize);
    let covered = |displaced: bool| {
        let mut nsi = scene(width as i32, height as i32);
        nsi.create("attr", "attributes").unwrap();

        if displaced {
            nsi.create("push", "shader").unwrap();
            nsi.set_attribute(
                "push",
                vec![arg(
                    "shaderfilename",
                    Type::String,
                    OwnedData::String(vec![
                        directory
                            .join("push.oso")
                            .to_string_lossy()
                            .into_owned()
                            .into_bytes(),
                    ]),
                )],
            )
            .unwrap();
            nsi.connect("push", None, "attr", "displacementshader")
                .unwrap();
        }

        nsi.connect("attr", None, "quad", "geometryattributes")
            .unwrap();

        let mut session = Session::new(nsi, &dso).expect("a render");
        session.wait();
        let (_, _, pixels) = session.render().snapshot().expect("a frame");

        // Alpha, which is coverage and nothing else.
        pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[3] > 0.5)
            .count()
    };

    let plain = covered(false);
    let pushed = covered(true);

    assert!(plain > 0, "the undisplaced quad is not in frame at all");
    assert!(
        pushed > plain + plain / 10,
        "pushing the quad half a unit towards the camera should have \
         made it visibly larger: {plain} pixels became {pushed}. Equal \
         means the displacement never reached MoonRay -- an unbound \
         `OslDisplacement` is silent, because the layer's displacement \
         column is optional."
    );
}

/// **`transparent()` becomes presence.**
///
/// OSL's straight-through transmission has no MoonRay lobe: MoonRay
/// expresses it as *presence*, a scalar on its own function evaluated
/// before shading. So the material runs the network a second time for
/// it -- and only for a group OSL's optimizer says may emit the
/// closure, which is what keeps every other shader from paying.
///
/// Alpha is what is asserted, because presence is coverage: the quad's
/// colour also drops, but that would drop just as well if the shader
/// simply shaded darker.
///
/// Needs the crate built with `$OSL_ROOT`.
#[cfg(osl)]
#[test]
fn transparent_becomes_presence() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let directory = scratch("nsi-moonray-osl-presence");
    std::fs::create_dir_all(&directory).expect("a writable directory");
    let source = directory.join("see.osl");
    std::fs::write(
        &source,
        "surface see(float amount = 0)\n\
         {\n    Ci = amount * transparent() + (1 - amount) * diffuse(N);\n}\n",
    )
    .expect("the shader is written");

    let oslc = std::path::Path::new(env!("OSL_ROOT")).join("bin/oslc");
    let compiled = std::process::Command::new(&oslc)
        .arg("-o")
        .arg(directory.join("see.oso"))
        .arg(&source)
        .output()
        .expect("oslc runs");
    assert!(
        compiled.status.success(),
        "oslc failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let (width, height) = (64usize, 48usize);
    let alpha = |amount: f32| {
        let mut nsi = scene(width as i32, height as i32);
        nsi.create("attr", "attributes").unwrap();
        nsi.create("see", "shader").unwrap();
        nsi.set_attribute(
            "see",
            vec![
                arg(
                    "shaderfilename",
                    Type::String,
                    OwnedData::String(vec![
                        directory
                            .join("see.oso")
                            .to_string_lossy()
                            .into_owned()
                            .into_bytes(),
                    ]),
                ),
                arg("amount", Type::F32, OwnedData::F32(vec![amount])),
            ],
        )
        .unwrap();
        nsi.connect("attr", None, "quad", "geometryattributes")
            .unwrap();
        nsi.connect("see", None, "attr", "surfaceshader").unwrap();

        let mut session = Session::new(nsi, &dso).expect("a render");
        session.wait();
        let (_, _, pixels) = session.render().snapshot().expect("a frame");

        pixels.chunks_exact(4).map(|pixel| pixel[3]).sum::<f32>()
    };

    let opaque = alpha(0.0);
    let half = alpha(0.5);

    assert!(opaque > 0.0, "the opaque quad is not in frame at all");
    // Half the coverage, within what a stochastic presence and the
    // quad's antialiased edge leave: the two are 2:1, not equal.
    assert!(
        half < opaque * 0.6 && half > opaque * 0.4,
        "half a unit of `transparent()` should have halved the \
         coverage: {opaque} became {half}. Unchanged means the closure \
         never reached presence -- the material renders the same \
         picture either way, only more of it."
    );
}

/// **`st` reaches an OSL shader as `u` and `v`.**
///
/// The whole chain, and each link is one that fails quietly: ɴsɪ's `st`
/// expanded to MoonRay's per-face-vertex `uv_list`, carried into the
/// mesh's primitive attributes, read back by the intersection as `St`,
/// and handed to OSL as `u` and `v`.
///
/// **The values are scaled, not merely present.** Without any `uv_list`
/// MoonRay parametrises the face itself, which for a quad is also
/// 0..1 -- so a test asserting that `u` varies passes with `st`
/// carried nowhere at all. Measured first: half the UVs, half the
/// gradient.
///
/// Needs the crate built with `$OSL_ROOT`.
#[cfg(osl)]
#[test]
fn an_nsi_st_reaches_osl() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let directory = scratch("nsi-moonray-osl-st");
    std::fs::create_dir_all(&directory).expect("a writable directory");
    let source = directory.join("uvshow.osl");
    std::fs::write(
        &source,
        "surface uvshow()\n\
         {\n    Ci = color(u, v, 0) * diffuse(N);\n}\n",
    )
    .expect("the shader is written");

    let oslc = std::path::Path::new(env!("OSL_ROOT")).join("bin/oslc");
    let compiled = std::process::Command::new(&oslc)
        .arg("-o")
        .arg(directory.join("uvshow.oso"))
        .arg(&source)
        .output()
        .expect("oslc runs");
    assert!(
        compiled.status.success(),
        "oslc failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let (width, height) = (64usize, 48usize);
    // The brightest red anywhere, which is the largest `u` the shader
    // saw -- scaled by the light, but by the same factor either way.
    let brightest = |scale: f32| {
        let mut nsi = scene(width as i32, height as i32);
        nsi.set_attribute(
            "quad",
            vec![array_arg(
                "st",
                Type::F32,
                2,
                OwnedData::F32(vec![
                    0.0, 0.0, scale, 0.0, scale, scale, 0.0, scale,
                ]),
            )],
        )
        .unwrap();

        nsi.create("attr", "attributes").unwrap();
        nsi.create("uvshow", "shader").unwrap();
        nsi.set_attribute(
            "uvshow",
            vec![arg(
                "shaderfilename",
                Type::String,
                OwnedData::String(vec![
                    directory
                        .join("uvshow.oso")
                        .to_string_lossy()
                        .into_owned()
                        .into_bytes(),
                ]),
            )],
        )
        .unwrap();
        nsi.connect("attr", None, "quad", "geometryattributes")
            .unwrap();
        nsi.connect("uvshow", None, "attr", "surfaceshader")
            .unwrap();

        let mut session = Session::new(nsi, &dso).expect("a render");
        session.wait();
        let (_, _, pixels) = session.render().snapshot().expect("a frame");

        pixels
            .chunks_exact(4)
            .map(|pixel| pixel[0])
            .fold(0.0f32, f32::max)
    };

    let full = brightest(1.0);
    let half = brightest(0.5);

    assert!(full > 0.1, "`u` never varied: {full}");
    // Half the UVs, half the gradient. Equal means `st` was dropped and
    // MoonRay's own parametrisation -- also 0..1 on a quad -- is what
    // the shader read.
    assert!(
        half < full * 0.6 && half > full * 0.4,
        "halving `st` should have halved what the shader read as `u`: \
         {full} became {half}"
    );
}

/// **An ɴsɪ primitive variable reaches an OSL `getattribute()`.**
///
/// The whole chain again, and a longer one: an attribute on the ɴsɪ
/// `mesh` that this backend has never heard of, expanded to
/// face-varying, written as a MoonRay `UserData` in the mesh's
/// `primitive_attributes`, requested from MoonRay because *OSL* said
/// the group reads it, attached to the intersection, and read back
/// through `RendererServices::get_attribute`.
///
/// The shader's own default is blue and the attribute is red, so what
/// the frame shows says which of the two the shader got. A link
/// missing anywhere in the chain renders the default, silently.
///
/// Needs the crate built with `$OSL_ROOT`.
#[cfg(osl)]
#[test]
fn an_nsi_primitive_variable_reaches_osl() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let directory = scratch("nsi-moonray-osl-primvar");
    std::fs::create_dir_all(&directory).expect("a writable directory");
    let source = directory.join("attrshow.osl");
    std::fs::write(
        &source,
        "surface attrshow()\n\
         {\n    color tint = color(0, 0, 1);\n\
         \x20   getattribute(\"mytint\", tint);\n\
         \x20   Ci = tint * diffuse(N);\n}\n",
    )
    .expect("the shader is written");

    let oslc = std::path::Path::new(env!("OSL_ROOT")).join("bin/oslc");
    let compiled = std::process::Command::new(&oslc)
        .arg("-o")
        .arg(directory.join("attrshow.oso"))
        .arg(&source)
        .output()
        .expect("oslc runs");
    assert!(
        compiled.status.success(),
        "oslc failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let (width, height) = (64usize, 48usize);
    let mut nsi = scene(width as i32, height as i32);

    // Per vertex, so the expansion has to index it the way `P` is
    // indexed rather than copy it across.
    nsi.set_attribute(
        "quad",
        vec![arg(
            "mytint",
            Type::Color,
            OwnedData::F32(vec![
                0.9, 0.1, 0.05, 0.9, 0.1, 0.05, 0.9, 0.1, 0.05, 0.9, 0.1, 0.05,
            ]),
        )],
    )
    .unwrap();

    nsi.create("attr", "attributes").unwrap();
    nsi.create("attrshow", "shader").unwrap();
    nsi.set_attribute(
        "attrshow",
        vec![arg(
            "shaderfilename",
            Type::String,
            OwnedData::String(vec![
                directory
                    .join("attrshow.oso")
                    .to_string_lossy()
                    .into_owned()
                    .into_bytes(),
            ]),
        )],
    )
    .unwrap();
    nsi.connect("attr", None, "quad", "geometryattributes")
        .unwrap();
    nsi.connect("attrshow", None, "attr", "surfaceshader")
        .unwrap();

    let mut session = Session::new(nsi, &dso).expect("a render");
    session.wait();
    let (_, _, pixels) = session.render().snapshot().expect("a frame");

    let centre = ((height / 2) * width + width / 2) * 4;
    let (red, green, blue) =
        (pixels[centre], pixels[centre + 1], pixels[centre + 2]);

    assert!(
        red > blue * 5.0,
        "the shader should have read `mytint` off the geometry -- red, \
         not the blue it defaults to: {red} {green} {blue}"
    );
    assert!(
        red > green * 5.0,
        "and red should dominate green, as `mytint` says: {red} {green} \
         {blue}"
    );
}

/// **A depth AOV reaches the written file, as its own channel.**
///
/// The flush's job is `result = "depth"` on a second `RenderOutput`;
/// MoonRay's is the rest. Checked by reading the channel back and
/// asserting its *values* -- the quad sits five units in front of the
/// camera, so a depth channel that is there but empty, or that is a
/// copy of the beauty, fails.
#[test]
fn a_depth_output_layer_is_written() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let directory = scratch("nsi-moonray-inprocess");
    std::fs::create_dir_all(&directory).expect("a writable directory");
    let image = directory.join("depth.exr");
    let _ = std::fs::remove_file(&image);

    let (width, height) = (64i32, 48i32);
    let mut nsi = scene(width, height);
    nsi.set_attribute(
        "driver",
        vec![OwnedArgument::new(
            "imagefilename",
            Type::String,
            1,
            0,
            OwnedData::String(vec![
                image.to_string_lossy().as_bytes().to_vec(),
            ]),
        )],
    )
    .unwrap();

    nsi.create("depth", "outputlayer").unwrap();
    nsi.set_attribute(
        "depth",
        vec![
            arg(
                "variablesource",
                Type::String,
                OwnedData::String(vec![b"builtin".to_vec()]),
            ),
            arg(
                "variablename",
                Type::String,
                OwnedData::String(vec![b"z".to_vec()]),
            ),
            arg(
                "layername",
                Type::String,
                OwnedData::String(vec![b"Z".to_vec()]),
            ),
        ],
    )
    .unwrap();
    nsi.connect("depth", None, "screen", "outputlayers")
        .unwrap();
    nsi.connect("driver", None, "depth", "outputdrivers")
        .unwrap();

    let mut session = Session::new(nsi, &dso).expect("a render");
    session.wait();
    drop(session);

    assert!(image.exists(), "no image at {}", image.display());

    use exr::prelude::{ReadChannels, ReadLayers};
    let read = exr::prelude::read()
        .no_deep_data()
        .largest_resolution_level()
        .all_channels()
        .first_valid_layer()
        .all_attributes()
        .from_file(&image)
        .expect("the written image reads back");

    let layer = &read.layer_data;
    let depth = layer
        .channel_data
        .list
        .iter()
        .find(|channel| channel.name.to_string() == "Z")
        .unwrap_or_else(|| {
            let names: Vec<String> = layer
                .channel_data
                .list
                .iter()
                .map(|channel| channel.name.to_string())
                .collect();
            panic!("no Z channel; the file has {names:?}")
        });

    // The quad is at z = -5 in a camera at the origin looking down -z,
    // so every pixel it covers is about five units away and the rest is
    // the background. `4.0` and `6.0` bracket the first without
    // admitting the second.
    let hits = (0..layer.size.width() * layer.size.height())
        .map(|i| depth.sample_data.value_by_flat_index(i).to_f32())
        .filter(|value| (4.0..6.0).contains(value))
        .count();

    assert!(
        hits > 400,
        "the depth channel should read about five over the quad, which \
         covers most of the frame; {hits} pixels of {} did",
        layer.size.width() * layer.size.height()
    );
}

/// Where 3Delight's compiled shaders live, if this machine has them.
///
/// `$NSI_MOONRAY_3DELIGHT_OSL` points at 3Delight's `osl` directory --
/// `.../3delight/Linux-x86_64/osl`. Unlike `$NSI_MOONRAY_DSO` this is
/// another vendor's product rather than something everyone building
/// this crate has, so the test that needs it says why it did nothing
/// rather than failing.
#[cfg(osl)]
fn three_delight_shaders() -> Option<std::path::PathBuf> {
    let path = std::path::PathBuf::from(
        std::env::var("NSI_MOONRAY_3DELIGHT_OSL").ok()?,
    );
    path.join("dlPrincipled.oso").exists().then_some(path)
}

/// **A shader 3Delight ships, rendered by MoonRay.**
///
/// The strongest test of the OSL path there is, and the reason is that
/// there is no source: `dlPrincipled.oso` is a compiled artefact of
/// another renderer's shader library, built by a different version of
/// `oslc`, using closures OSL does not declare. Nothing here was
/// written with MoonRay in mind and nothing about it can be adjusted to
/// make it pass.
///
/// Three parameters, three different paths through the closure walk,
/// each asserted against arithmetic rather than against "something
/// happened":
///
/// - `i_color` alone is diffuse, and comes back as the colour scaled by
///   the light.
/// - `metallic` with a low roughness is `microfacet`, and comes back as
///   a white highlight from the white environment.
/// - `incandescence` is `emission`, and *adds* to the diffuse -- so the
///   expected value is the sum of the two, not either.
///
/// It found two real bugs. The `subsurface` registration declared five
/// formal parameters where OSL declares four, which shifted every
/// keyword argument by one and segfaulted inside OSL's code generator;
/// and `layer_closures`, `outputvariable` and `outputconstant` -- which
/// every 3Delight shader builds its `Ci` out of -- were not registered
/// at all.
#[cfg(osl)]
#[test]
fn a_3delight_shader_renders() {
    use nsi_moonray::session::Session;

    let Some(shaders) = three_delight_shaders() else {
        eprintln!(
            "skipped: set $NSI_MOONRAY_3DELIGHT_OSL to 3Delight's `osl` \
             directory to run this"
        );
        return;
    };
    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (width, height) = (64usize, 48usize);
    let brightest = |parameters: Vec<OwnedArgument>| {
        let mut nsi = scene(width as i32, height as i32);
        nsi.create("attr", "attributes").unwrap();
        nsi.create("principled", "shader").unwrap();

        let mut arguments = vec![arg(
            "shaderfilename",
            Type::String,
            OwnedData::String(vec![
                shaders
                    .join("dlPrincipled.oso")
                    .to_string_lossy()
                    .into_owned()
                    .into_bytes(),
            ]),
        )];
        arguments.extend(parameters);
        nsi.set_attribute("principled", arguments).unwrap();

        nsi.connect("attr", None, "quad", "geometryattributes")
            .unwrap();
        nsi.connect("principled", None, "attr", "surfaceshader")
            .unwrap();

        let mut session = Session::new(nsi, &dso).expect("a render");
        session.wait();
        let (_, _, pixels) = session.render().snapshot().expect("a frame");

        let channel = |offset: usize| {
            pixels
                .chunks_exact(4)
                .map(|pixel| pixel[offset])
                .fold(0.0f32, f32::max)
        };
        [channel(0), channel(1), channel(2)]
    };

    let colour =
        |values: Vec<f32>| arg("i_color", Type::Color, OwnedData::F32(values));

    // Diffuse. `i_color` is 0.1, 0.8, 0.2 -- green dominant, red
    // lowest, and nothing like the grey a default surface would give.
    let diffuse = brightest(vec![colour(vec![0.1, 0.8, 0.2])]);
    assert!(
        diffuse[1] > diffuse[0] * 5.0 && diffuse[1] > diffuse[2] * 3.0,
        "`i_color` should have shaded the surface: {diffuse:?}"
    );

    // Metal, and a *gold* one -- red dominant, where the diffuse was
    // green dominant, so a walk that ignored the parameters could not
    // satisfy both.
    //
    // What this guards is the conductor path. 3Delight passes a
    // conductor's complex index of refraction as the `realeta` and
    // `complexeta` keywords on `microfacet` rather than by tinting the
    // closure weight, so a renderer that drops them renders every metal
    // *white*: this came back 1.005 in all three channels before those
    // keywords were registered, which is a mirror, not gold.
    let metal = brightest(vec![
        colour(vec![0.95, 0.75, 0.35]),
        arg("metallic", Type::F32, OwnedData::F32(vec![1.0])),
        arg("roughness", Type::F32, OwnedData::F32(vec![0.15])),
    ]);
    assert!(
        metal[0] > metal[1] && metal[1] > metal[2] * 1.8,
        "a gold conductor should keep its tint -- red over green over \
         blue. Three equal channels mean `realeta` and `complexeta` were \
         dropped and the metal is a mirror: {metal:?}"
    );

    // Emission, which *adds* to the diffuse rather than replacing it.
    // 0.9 * 3 on top of the diffuse red, and 0.1 * 3 on top of the
    // diffuse blue -- so the expected values are arithmetic, not a
    // direction.
    let emissive = brightest(vec![
        colour(vec![0.1, 0.8, 0.2]),
        arg(
            "incandescence",
            Type::Color,
            OwnedData::F32(vec![0.9, 0.2, 0.1]),
        ),
        arg(
            "incandescence_intensity",
            Type::F32,
            OwnedData::F32(vec![3.0]),
        ),
    ]);
    let added = [0.9 * 3.0, 0.2 * 3.0, 0.1 * 3.0];
    for channel in 0..3 {
        let expected = added[channel] + diffuse[channel];
        assert!(
            (emissive[channel] - expected).abs() < 0.05,
            "`incandescence` should add to what the surface already \
             shades: channel {channel} of {emissive:?} should be about \
             {expected}, which is {} on top of the diffuse {}",
            added[channel],
            diffuse[channel]
        );
    }
}

/// **A lobe label reaches a named AOV, end to end.**
///
/// The longest chain in this backend, and every link is one that fails
/// silently:
///
/// `diffuse(N, "label", "diffuse")` in an OSL shader → the `"label"`
/// keyword parameter, which the *renderer* registers rather than OSL →
/// `label_index` against the vocabulary `attributes.cc` declares as the
/// scene class's `labels` → MoonRay reading that array back at render
/// prep and matching it against the AOV schema → an output layer whose
/// light-path expression names the label → a channel in the file.
///
/// The shader emits two labelled lobes and the AOVs ask for one each,
/// so a chain that labelled nothing gives two black channels and one
/// that labelled everything the same gives two identical ones. The
/// assertion is that the diffuse channel is green and the specular one
/// is not.
///
/// Needs the crate built with `$OSL_ROOT`.
#[cfg(osl)]
#[test]
fn a_lobe_label_reaches_a_named_aov() {
    use nsi_moonray::session::Session;

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let directory = scratch("nsi-moonray-osl-label");
    std::fs::create_dir_all(&directory).expect("a writable directory");
    let image = directory.join("labels.exr");
    let _ = std::fs::remove_file(&image);

    let source = directory.join("labelled.osl");
    std::fs::write(
        &source,
        "surface labelled()\n\
         {\n\
         \x20   Ci = color(0.05, 0.8, 0.1) * diffuse(N, \"label\", \"diffuse\")\n\
         \x20      + color(0.8, 0.05, 0.05)\n\
         \x20        * microfacet(\"ggx\", N, vector(0), 0.2, 0.2, 1.5, 0,\n\
         \x20                     \"label\", \"specular\");\n}\n",
    )
    .expect("the shader is written");

    let oslc = std::path::Path::new(env!("OSL_ROOT")).join("bin/oslc");
    let compiled = std::process::Command::new(&oslc)
        .arg("-o")
        .arg(directory.join("labelled.oso"))
        .arg(&source)
        .output()
        .expect("oslc runs");
    assert!(
        compiled.status.success(),
        "oslc failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let (width, height) = (64i32, 48i32);
    let mut nsi = scene(width, height);
    nsi.set_attribute(
        "driver",
        vec![OwnedArgument::new(
            "imagefilename",
            Type::String,
            1,
            0,
            OwnedData::String(vec![
                image.to_string_lossy().as_bytes().to_vec(),
            ]),
        )],
    )
    .unwrap();

    nsi.create("attr", "attributes").unwrap();
    nsi.create("labelled", "shader").unwrap();
    nsi.set_attribute(
        "labelled",
        vec![arg(
            "shaderfilename",
            Type::String,
            OwnedData::String(vec![
                directory
                    .join("labelled.oso")
                    .to_string_lossy()
                    .into_owned()
                    .into_bytes(),
            ]),
        )],
    )
    .unwrap();
    nsi.connect("attr", None, "quad", "geometryattributes")
        .unwrap();
    nsi.connect("labelled", None, "attr", "surfaceshader")
        .unwrap();

    // One layer a lobe. `reflection` is 3Delight's name for the
    // specular one, so this also exercises the vocabulary reconciliation
    // rather than only the pass-through name.
    for (handle, variable) in [("diff", "diffuse"), ("spec", "reflection")] {
        nsi.create(handle, "outputlayer").unwrap();
        nsi.set_attribute(
            handle,
            vec![
                arg(
                    "variablesource",
                    Type::String,
                    OwnedData::String(vec![b"shader".to_vec()]),
                ),
                arg(
                    "variablename",
                    Type::String,
                    OwnedData::String(vec![variable.as_bytes().to_vec()]),
                ),
                arg(
                    "layername",
                    Type::String,
                    OwnedData::String(vec![handle.as_bytes().to_vec()]),
                ),
            ],
        )
        .unwrap();
        nsi.connect(handle, None, "screen", "outputlayers").unwrap();
        nsi.connect("driver", None, handle, "outputdrivers")
            .unwrap();
    }

    let mut session = Session::new(nsi, &dso).expect("a render");
    session.wait();
    drop(session);

    assert!(image.exists(), "no image at {}", image.display());

    use exr::prelude::{ReadChannels, ReadLayers};
    let read = exr::prelude::read()
        .no_deep_data()
        .largest_resolution_level()
        .all_channels()
        .first_valid_layer()
        .all_attributes()
        .from_file(&image)
        .expect("the written image reads back");

    let layer = &read.layer_data;
    let names: Vec<String> = layer
        .channel_data
        .list
        .iter()
        .map(|channel| channel.name.to_string())
        .collect();

    let brightest = |channel: &str| {
        let found = layer
            .channel_data
            .list
            .iter()
            .find(|c| c.name.to_string() == channel)
            .unwrap_or_else(|| panic!("no {channel} channel; found {names:?}"));
        (0..layer.size.width() * layer.size.height())
            .map(|i| found.sample_data.value_by_flat_index(i).to_f32())
            .fold(0.0f32, f32::max)
    };

    // Green in the diffuse layer, red in the specular one -- which is
    // how the shader coloured them, and the only way to tell "the
    // labels were carried" from "both layers got the beauty".
    let (diffuse_green, diffuse_red) =
        (brightest("diff.G"), brightest("diff.R"));
    let (specular_red, specular_green) =
        (brightest("spec.R"), brightest("spec.G"));

    assert!(
        diffuse_green > 0.05,
        "the diffuse layer is black, so the label never reached the AOV: \
         channels are {names:?}"
    );
    assert!(
        diffuse_green > diffuse_red * 5.0,
        "the diffuse layer should be the green lobe alone: {diffuse_red} \
         red against {diffuse_green} green"
    );
    assert!(
        specular_red > specular_green * 3.0,
        "the specular layer should be the red lobe alone: {specular_red} \
         red against {specular_green} green"
    );
}

/// **An OSL volume shader shades the volume.**
///
/// Without one, MoonRay renders the density grid through its own
/// `VdbVolume` and the result is a plausible grey puff -- which is why
/// this asserts *colour* rather than coverage. The shader returns a
/// strongly green `anisotropic_vdf`, so a green cast is the only thing
/// that distinguishes a shaded volume from the stock one, and coverage
/// alone would pass either way.
#[cfg(osl)]
#[test]
fn an_osl_volume_shader_shades_the_volume() {
    use nsi_moonray::session::Session;

    let Some(vdb) = vdb_file() else {
        eprintln!("skipped: run `just assets` for the sample volume");
        return;
    };
    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let directory = scratch("nsi-moonray-osl-volume");
    let source = directory.join("green_smoke.osl");
    std::fs::write(
        &source,
        "volume green_smoke()\n\
         {\n\
         \x20   Ci = anisotropic_vdf(color(0.05, 0.9, 0.05),\n\
         \x20                        color(1.0, 1.0, 1.0), 0.0);\n}\n",
    )
    .expect("the shader is written");

    let oslc = std::path::Path::new(env!("OSL_ROOT")).join("bin/oslc");
    let compiled = std::process::Command::new(&oslc)
        .arg("-o")
        .arg(directory.join("green_smoke.oso"))
        .arg(&source)
        .output()
        .expect("oslc runs");
    assert!(
        compiled.status.success(),
        "oslc failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let (width, height) = (96usize, 72usize);
    let mut nsi = scene(width as i32, height as i32);
    nsi.disconnect("quad", None, ".root", "objects").unwrap();

    nsi.create("smoke", "volume").unwrap();
    nsi.set_attribute(
        "smoke",
        vec![
            arg(
                "vdbfilename",
                Type::String,
                OwnedData::String(vec![
                    vdb.to_string_lossy().into_owned().into_bytes(),
                ]),
            ),
            arg(
                "densitygrid",
                Type::String,
                OwnedData::String(vec![b"density".to_vec()]),
            ),
        ],
    )
    .unwrap();
    nsi.connect("smoke", None, ".root", "objects").unwrap();

    // The volume shader, bound the way the interface binds one.
    nsi.create("attr", "attributes").unwrap();
    nsi.create("green", "shader").unwrap();
    nsi.set_attribute(
        "green",
        vec![arg(
            "shaderfilename",
            Type::String,
            OwnedData::String(vec![
                directory
                    .join("green_smoke.oso")
                    .to_string_lossy()
                    .into_owned()
                    .into_bytes(),
            ]),
        )],
    )
    .unwrap();
    nsi.connect("attr", None, "smoke", "geometryattributes")
        .unwrap();
    nsi.connect("green", None, "attr", "volumeshader").unwrap();

    // The same framing as `a_volume_renders`; see `vdb_file`.
    nsi.set_attribute(
        "cam",
        vec![arg("fov", Type::F32, OwnedData::F32(vec![60.0]))],
    )
    .unwrap();
    nsi.create("xform", "transform").unwrap();
    nsi.set_attribute(
        "xform",
        vec![arg(
            "transformationmatrix",
            Type::MatrixF64,
            OwnedData::F64(vec![
                1.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                0.0, 37.0, 100.0, 1.0,
            ]),
        )],
    )
    .unwrap();
    nsi.disconnect("cam", None, ".root", "objects").unwrap();
    nsi.connect("xform", None, ".root", "objects").unwrap();
    nsi.connect("cam", None, "xform", "objects").unwrap();

    let mut session = Session::new(nsi, &dso).expect("a render");
    session.wait();
    let (_, _, pixels) = session.render().snapshot().expect("a frame");

    let mut green_over_red = 0usize;
    let mut covered = 0usize;
    for pixel in pixels.chunks_exact(4) {
        if pixel[3] <= 0.01 {
            continue;
        }
        covered += 1;
        if pixel[1] > pixel[0] * 1.5 {
            green_over_red += 1;
        }
    }

    assert!(covered > 0, "the volume rendered nothing at all");
    assert!(
        green_over_red * 2 > covered,
        "only {green_over_red} of {covered} covered pixels are green. \
         The stock `VdbVolume` renders this grid grey, so a volume that \
         is not green is one the OSL shader never shaded"
    );
}

/// Where an OpenVDB file to render lives, if this machine has one.
///
/// **`fire.vdb` from the OpenVDB sample models**, which `just assets`
/// downloads. It is an asset rather than something everyone building
/// this crate has, so the tests that need it say why they did nothing
/// rather than failing.
///
/// The framing below is measured off *that* file rather than taken on
/// faith, which is what the previous version of this did and what kept
/// the volume path from ever being exercised: `vdb_print` gives a
/// voxel size of 0.244 and an index-to-world translation of
/// `(-19.1, -8.9, -18.7)` over active bounds `(0,5,0)-(160,368,152)`,
/// so the flame occupies roughly `x -19..20`, `y -8..81`, `z -19..18`.
/// A tall, narrow plume centred near `y = 37` and **not** on the
/// origin, which is why a camera pointed at the origin saw almost
/// nothing.
fn vdb_file() -> Option<std::path::PathBuf> {
    let path = std::path::PathBuf::from(std::env::var("NSI_MOONRAY_VDB").ok()?);
    path.exists().then_some(path)
}

/// **A `volume` node renders.**
///
/// The interface's volume node is OpenVDB and nothing else, and
/// MoonRay's only volume geometry reads exactly that, so this is one of
/// the closer mappings here — and it has one trap. A volume is shaded
/// through the `Layer`'s *volume shader* column, not its material
/// column, and a row with the wrong one renders **nothing**: no
/// warning, no geometry, just the background.
///
/// So what is asserted is coverage. The camera is placed to look at the
/// grid's own bounds, which the test reads off nothing — it takes them
/// on faith from `$NSI_MOONRAY_VDB` being the asset it names — so it
/// asks only that a good fraction of the frame stopped being background.
#[test]
fn a_volume_renders() {
    use nsi_moonray::session::Session;

    let Some(vdb) = vdb_file() else {
        eprintln!(
            "skipped: set $NSI_MOONRAY_VDB to an OpenVDB file with a \
             `density` grid to run this"
        );
        return;
    };
    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (width, height) = (96usize, 72usize);
    let mut nsi = scene(width as i32, height as i32);

    // The quad the fixture builds would sit in front of the volume, so
    // it goes.
    nsi.disconnect("quad", None, ".root", "objects").unwrap();

    nsi.create("smoke", "volume").unwrap();
    nsi.set_attribute(
        "smoke",
        vec![
            arg(
                "vdbfilename",
                Type::String,
                OwnedData::String(vec![
                    vdb.to_string_lossy().into_owned().into_bytes(),
                ]),
            ),
            arg(
                "densitygrid",
                Type::String,
                OwnedData::String(vec![b"density".to_vec()]),
            ),
        ],
    )
    .unwrap();
    nsi.connect("smoke", None, ".root", "objects").unwrap();

    // Framed for the plume: 60 degrees at 100 units gives a frame
    // about 115 units tall against a flame about 89 tall.
    nsi.set_attribute(
        "cam",
        vec![arg("fov", Type::F32, OwnedData::F32(vec![60.0]))],
    )
    .unwrap();
    nsi.create("xform", "transform").unwrap();
    nsi.set_attribute(
        "xform",
        vec![arg(
            "transformationmatrix",
            Type::MatrixF64,
            OwnedData::F64(vec![
                1.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                // Up at the middle of the plume, back far enough to
                // hold it.
                0.0, 37.0, 100.0, 1.0,
            ]),
        )],
    )
    .unwrap();
    nsi.disconnect("cam", None, ".root", "objects").unwrap();
    nsi.connect("xform", None, ".root", "objects").unwrap();
    nsi.connect("cam", None, "xform", "objects").unwrap();

    let mut session = Session::new(nsi, &dso).expect("a render");
    session.wait();
    let (_, _, pixels) = session.render().snapshot().expect("a frame");

    let covered = pixels
        .chunks_exact(4)
        .filter(|pixel| pixel[3] > 0.01)
        .count();

    assert!(
        covered > pixels.len() / 4 / 20,
        "the volume covered {covered} pixels of {}. Nothing at all means \
         the layer row was given a material instead of a volume shader, \
         which MoonRay renders as no geometry rather than as an error",
        pixels.len() / 4
    );
}

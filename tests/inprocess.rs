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

use nsi_intermediate::{OwnedArg, OwnedData, Scene};
use nsi_moonray::{apply::apply, flush::flush, rdl2::Render};
use nsi_trait::Type;

fn arg(name: &str, type_tag: Type, data: OwnedData) -> OwnedArg {
    OwnedArg::new(name, type_tag, 1, 0, data)
}

fn dso_path() -> Option<String> {
    std::env::var("NSI_MOONRAY_DSO").ok()
}

/// One renderer per process, so one test at a time.
///
/// Not a test-harness nicety: MoonRay's driver state is global, and two
/// live `RenderContext`s abort in the allocator. `Render::new` refuses
/// the second, so without this the tests would race for which one gets
/// `None`.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    use nsi_intermediate::HostPtr;
    use nsi_moonray::stream::{Stopped, stream};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };

    let buckets = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&buckets);
    let last = Arc::new(Mutex::new(Vec::<f32>::new()));
    let seen = Arc::clone(&last);

    let write = WriteCallback::<f32>::new(
        move |_name,
              _width,
              _height,
              _x0,
              _x1,
              _y0,
              _y1,
              _format: &PixelFormat,
              pixels: &[f32]| {
            counted.fetch_add(1, Ordering::SeqCst);
            *seen.lock().expect("not poisoned") = pixels.to_vec();
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
        vec![OwnedArg::new(
            "callback.write",
            Type::Reference,
            1,
            0,
            OwnedData::Reference(vec![HostPtr(write.to_ptr())]),
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
    // Six on the machine this was written on: the loop polls every
    // 50ms and this frame converges in about a third of a second. The
    // *count* is not asserted because it is a property of the host, not
    // of the code -- a fast enough machine could finish inside one
    // poll. That progressive delivery happens at all is what
    // `a_frame_can_be_snapshotted_while_it_converges` pins down.
    let count = buckets.load(Ordering::SeqCst);
    assert!(count > 0, "the closure received nothing");

    let pixels = last.lock().expect("not poisoned");
    assert_eq!(pixels.len(), (width * height * 4) as usize);

    let centre =
        ((height as usize / 2) * width as usize + width as usize / 2) * 4;
    assert!(
        pixels[centre..centre + 3].iter().any(|value| *value > 0.0),
        "the closure received a black centre of frame"
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
    use nsi_intermediate::HostPtr;
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
        vec![OwnedArg::new(
            "callback.write",
            Type::Reference,
            1,
            0,
            OwnedData::Reference(vec![HostPtr(write.to_ptr())]),
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
    use nsi_intermediate::HostPtr;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    let Some(dso) = dso_path() else {
        panic!("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso");
    };
    // SAFETY: single-threaded here, under the renderer lock.
    unsafe { std::env::set_var("NSI_MOONRAY_DSO", &dso) };

    let buckets = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&buckets);
    let last = Arc::new(Mutex::new(Vec::<f32>::new()));
    let seen = Arc::clone(&last);

    let write = WriteCallback::<f32>::new(
        move |_name,
              _w,
              _h,
              _x0,
              _x1,
              _y0,
              _y1,
              _format: &PixelFormat,
              pixels: &[f32]| {
            counted.fetch_add(1, Ordering::SeqCst);
            *seen.lock().expect("not poisoned") = pixels.to_vec();
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
        vec![OwnedArg::new(
            "callback.write",
            Type::Reference,
            1,
            0,
            OwnedData::Reference(vec![HostPtr(write.to_ptr())]),
        )],
    )
    .unwrap();

    // Drive the C entry point over a context holding this scene, the
    // way `nsi-ffi-wrap` drives a renderer it loaded.
    assert!(
        nsi_moonray::capi::render_in_process(&nsi),
        "the linked renderer should have taken the scene"
    );

    assert!(
        buckets.load(Ordering::SeqCst) > 0,
        "the C API delivered no pixels"
    );

    let pixels = last.lock().expect("not poisoned");
    assert_eq!(pixels.len(), (width * height * 4) as usize);

    let centre =
        ((height as usize / 2) * width as usize + width as usize / 2) * 4;
    assert!(
        pixels[centre..centre + 3].iter().any(|value| *value > 0.0),
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

    let directory = std::env::temp_dir().join("nsi-moonray-inprocess");
    std::fs::create_dir_all(&directory).expect("a writable directory");
    let image = directory.join("batch.exr");
    let _ = std::fs::remove_file(&image);

    let (width, height) = (64i32, 48i32);
    let mut nsi = scene(width, height);
    // No callbacks on the driver: this is a batch render.
    nsi.set_attribute(
        "driver",
        vec![OwnedArg::new(
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
            attributes.push(OwnedArg::new(
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

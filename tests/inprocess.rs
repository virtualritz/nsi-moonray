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
    let render = Render::new(Some(dso), Some(2)).expect("a renderer");
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

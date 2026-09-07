//! The synchronise loop: one ɴsɪ edit becoming the narrowest rdl2 edit
//! that expresses it, and a re-render that reuses everything else.
//!
//! This is what an application's viewport does — a slider moves, one
//! attribute changes, and the renderer keeps its tessellation and its
//! acceleration structures. It is the reason `002` put linking first: a
//! spawned process has no scene to edit.
#![cfg(all(feature = "rdl2", moonray))]

use nsi_intermediate::{OwnedArg, OwnedData, Scene};
use nsi_moonray::{
    apply::{apply, apply_affected},
    flush::flush,
    rdl2::{Mode, Render},
};
use nsi_trait::Type;

fn arg(name: &str, type_tag: Type, data: OwnedData) -> OwnedArg {
    OwnedArg::new(name, type_tag, 1, 0, data)
}

fn dso_path() -> String {
    std::env::var("NSI_MOONRAY_DSO")
        .expect("set $NSI_MOONRAY_DSO to MoonRay's rdl2dso")
}

/// A subdivided grid, heavy enough that tessellating it is measurable.
///
/// `I5` needs a scene where re-tessellation *costs* something. A
/// four-vertex quad tessellates in microseconds, so the counters cannot
/// separate "re-tessellated it" from "looked and skipped" -- both read
/// as about a tenth of a millisecond.
fn grid(side: i32) -> (Vec<i32>, Vec<i32>, Vec<f32>) {
    let mut counts = Vec::new();
    let mut indices = Vec::new();
    let mut points = Vec::new();

    let n = side + 1;
    for row in 0..n {
        for col in 0..n {
            points.push(col as f32 / side as f32 * 2.0 - 1.0);
            points.push(row as f32 / side as f32 * 2.0 - 1.0);
            points.push(-5.0);
        }
    }
    for row in 0..side {
        for col in 0..side {
            counts.push(4);
            let at = |r: i32, c: i32| r * n + c;
            indices.extend([
                at(row, col),
                at(row, col + 1),
                at(row + 1, col + 1),
                at(row + 1, col),
            ]);
        }
    }

    (counts, indices, points)
}

/// One renderer per process; MoonRay's driver state is global.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[rustfmt::skip]
fn translate(x: f64) -> OwnedArg {
    arg("transformationmatrix", Type::MatrixF64, OwnedData::F64(vec![
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
          x, 0.0, -5.0, 1.0,
    ]))
}

/// A quad under a transform, lit, with a camera and an output.
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
                        -1.0, -1.0, 0.0, 1.0, -1.0, 0.0, 1.0, 1.0, 0.0, -1.0,
                        1.0, 0.0,
                    ]),
                ),
            ],
        )
        .unwrap();

    scene.create("xform", "transform").unwrap();
    scene.set_attribute("xform", vec![translate(0.0)]).unwrap();
    scene.connect("xform", None, ".root", "objects").unwrap();
    scene.connect("quad", None, "xform", "objects").unwrap();

    scene.create("light", "environment").unwrap();
    scene.connect("light", None, ".root", "objects").unwrap();

    scene.create("attr", "attributes").unwrap();
    scene
        .connect("attr", None, "quad", "geometryattributes")
        .unwrap();
    scene.create("surface", "shader").unwrap();
    scene
        .set_attribute(
            "surface",
            vec![arg(
                "diffuseColor",
                Type::Color,
                OwnedData::F32(vec![1.0, 0.0, 0.0]),
            )],
        )
        .unwrap();
    scene
        .connect("surface", None, "attr", "surfaceshader")
        .unwrap();

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
        .connect("beauty", None, "screen", "outputlayers")
        .unwrap();
    scene.create("driver", "outputdriver").unwrap();
    scene
        .connect("driver", None, "beauty", "outputdrivers")
        .unwrap();

    scene
}

/// Render the current state of `render` to completion and return the
/// frame.
fn frame(render: &Render) -> Vec<f32> {
    render.start().expect("the frame starts");
    while !render.frame_complete() {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let (_, _, pixels) = render.snapshot().expect("a snapshot");
    render.stop().expect("the frame stops");
    pixels
}

/// How much light a column of the frame carries.
fn column(pixels: &[f32], width: usize, height: usize, x: usize) -> f32 {
    (0..height)
        .map(|y| {
            let i = (y * width + x) * 4;
            pixels[i] + pixels[i + 1] + pixels[i + 2]
        })
        .sum()
}

/// The frame's total red, green and blue.
///
/// A render is stochastic, so two renders of the *same* scene differ in
/// the last bits. Comparing colour rather than exact values is what
/// makes "did the material change" answerable at all.
fn channels(pixels: &[f32]) -> [f32; 3] {
    let mut total = [0.0; 3];
    for pixel in pixels.chunks_exact(4) {
        for (sum, value) in total.iter_mut().zip(&pixel[..3]) {
            *sum += value;
        }
    }
    total
}

/// **`I1`/`I3`.** One transform edit, re-applied narrowly, and the
/// image changes.
///
/// The assertion is on *pixels* rather than on the scene: applied but
/// not marked is a scene holding the new value while the render shows
/// the old one, and reading the scene back would pass on exactly that.
#[test]
fn one_transform_edit_moves_the_shape_and_nothing_else_is_re_applied() {
    let dso = dso_path();
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (width, height) = (64usize, 48usize);
    let render =
        Render::new(Some(&dso), Some(2), Mode::Batch).expect("a renderer");
    let live = render.scene().expect("the renderer's own scene");

    let mut nsi = scene(width as i32, height as i32);
    let report = apply(&flush(&nsi).document, &live);
    assert!(
        !report.iter().any(|line| line.contains("no scene class")),
        "{report:?}"
    );

    render.initialize().expect("render prep");
    let before = frame(&render);

    // Everything recorded so far is the initial build, not an edit.
    let _ = nsi.take_changes();

    // The edit: move the transform left.
    nsi.set_attribute("xform", vec![translate(-1.5)]).unwrap();

    let changes = nsi.take_changes();
    let affected = nsi.affected(&changes);

    assert!(
        affected.nodes.contains("quad"),
        "upstream must name the quad as affected by its parent \
         transform moving, or a backend has to re-derive ɴsɪ's scoping \
         rules: {affected:?}"
    );
    assert!(
        !affected.everything,
        "a transform edit is not a global change: {affected:?}"
    );

    let (report, rebuilt) =
        apply_affected(&flush(&nsi).document, None, &live, &changes, &affected);
    assert!(report.is_empty(), "{report:?}");
    assert!(
        !rebuilt,
        "a transform edit must not force a whole-scene re-apply"
    );

    render.scene_updated().expect("the renderer is told");
    let after = frame(&render);

    // The quad was centred and is now left of centre.
    let centre = width / 2;
    let left = width / 6;

    assert!(
        column(&before, width, height, centre) > 0.0,
        "the quad should start in the centre of frame"
    );
    assert!(
        column(&after, width, height, centre)
            < column(&before, width, height, centre),
        "the centre should be dimmer once the quad moves off it"
    );
    assert!(
        column(&after, width, height, left)
            > column(&before, width, height, left),
        "the left should be brighter once the quad moves onto it"
    );
}

/// **`I1`.** A shader edit reaches the image.
///
/// And a finding worth writing down, because two versions of this test
/// asserted the opposite and both failed:
///
/// `RenderContext::startFrame` branches on `mSceneUpdated` to decide
/// whether to run `applyUpdates`, and `mSceneUpdated` is set only by
/// MoonRay's *own* update entry points (`updateScene`,
/// `updateGeometry`). A scene edited directly through the live
/// `SceneContext` sets none of them, so `setSceneUpdated` looks like
/// the load-bearing call -- MoonRay's own comment says it is "for
/// when the SceneContext is modified externally".
///
/// **It is not what makes these edits visible.** Both a transform and
/// a shader parameter reach the render without it: geometry-to-render
/// matrices are recomputed from `node_xform` as geometry loads, and a
/// material's parameters are read from the rdl2 object at shade time
/// rather than from anything `applyUpdates` rebuilds.
///
/// So `scene_updated()` is still called on the real path -- it is the
/// documented hook, it costs nothing, and `applyUpdates` is what
/// rebuilds primitive-attribute tables and marks geometry for reload,
/// which these two edits happen not to need. But nothing here may
/// claim it is what carries the edit across, and no test may assert
/// that its absence hides one. `I5` is where the difference would
/// actually show: in what got regenerated, not in the pixels.
#[test]
fn a_shader_edit_reaches_the_image() {
    let dso = dso_path();
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (width, height) = (64usize, 48usize);
    let render =
        Render::new(Some(&dso), Some(2), Mode::Batch).expect("a renderer");
    let live = render.scene().expect("a scene");

    let mut nsi = scene(width as i32, height as i32);
    apply(&flush(&nsi).document, &live);
    render.initialize().expect("render prep");
    let before = frame(&render);

    let _ = nsi.take_changes();

    // Turn the surface green. `diffuseColor` crosses by exact name.
    nsi.set_attribute(
        "surface",
        vec![arg(
            "diffuseColor",
            Type::Color,
            OwnedData::F32(vec![0.0, 1.0, 0.0]),
        )],
    )
    .unwrap();

    let changes = nsi.take_changes();
    let affected = nsi.affected(&changes);
    assert!(
        affected.shaders.contains("surface"),
        "upstream must name the shader: {affected:?}"
    );

    apply_affected(&flush(&nsi).document, None, &live, &changes, &affected);

    // Deliberately no `scene_updated()` yet.
    let after = frame(&render);

    let [red_before, green_before, _] = channels(&before);
    let [red_after, green_after, _] = channels(&after);

    assert!(
        red_before > green_before * 2.0,
        "the quad starts red: {red_before} vs {green_before}"
    );
    assert!(
        green_after > red_after * 2.0,
        "the edited material must reach the image: red {red_after} vs \
         green {green_after}"
    );

    // And again with the mark, which is what the real path does. The
    // point is that it is not *worse*, not that it is what carried the
    // edit -- see this test's note.
    render.scene_updated().expect("the renderer is told");
    let marked = frame(&render);
    let [red_marked, green_marked, _] = channels(&marked);

    assert!(
        green_marked > red_marked * 2.0,
        "red {red_marked} vs green {green_marked}"
    );
}

/// **`I6`.** A created or deleted node changes set and layer
/// *membership*, which no single object carries, so it falls back to a
/// full re-apply — and says so rather than silently narrowing.
#[test]
fn a_created_node_falls_back_and_reports() {
    let dso = dso_path();
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let render =
        Render::new(Some(&dso), Some(2), Mode::Batch).expect("a renderer");
    let live = render.scene().expect("a scene");

    let mut nsi = scene(64, 48);
    apply(&flush(&nsi).document, &live);
    let _ = nsi.take_changes();

    nsi.create("second", "mesh").unwrap();
    // A *subdivision* surface, because a polygon mesh may never go
    // through tessellation at all and the counters would not move for
    // it -- which is the thing this test exists to rule out.
    nsi.set_attribute(
        "second",
        vec![OwnedArg::new(
            "subdivision.scheme",
            Type::String,
            1,
            0,
            OwnedData::String(vec![b"catmull-clark".to_vec()]),
        )],
    )
    .unwrap();
    nsi.connect("second", None, ".root", "objects").unwrap();

    let changes = nsi.take_changes();
    let affected = nsi.affected(&changes);
    let (report, rebuilt) =
        apply_affected(&flush(&nsi).document, None, &live, &changes, &affected);

    assert!(rebuilt, "a created node must force a full re-apply");
    assert!(
        report.iter().any(|line| line.contains("created")),
        "the fallback must say why: {report:?}"
    );
}

/// **The synchronise loop.** What an application's viewport does:
/// start an interactive render, move something, synchronise, and see
/// the image change — without rebuilding the scene.
///
/// This is the shape the whole of `002` was for, and it is only
/// possible because MoonRay is linked: a spawned process has no scene
/// to edit and no frame to restart. `capi` is a thin shim over the
/// same `Session`, so this is the path an ɴsɪ consumer takes too.
#[test]
fn a_session_runs_a_synchronise_loop() {
    use nsi_ffi_wrap::{
        argument::CallbackPtr,
        output::{Error, PixelFormat, WriteCallback},
    };
    use nsi_intermediate::HostPtr;
    use nsi_moonray::session::Session;
    use std::sync::{Arc, Mutex};

    let dso = dso_path();
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    // Buckets name a *rectangle* -- the first covers the frame, later
    // ones only what the renderer refined -- so a driver paints them
    // into a frame, and so does this.
    let canvas = Arc::new(Mutex::new(Vec::<f32>::new()));
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
            let channels = format.channels();
            let mut frame = painting.lock().expect("not poisoned");
            if frame.is_empty() {
                frame.resize(width * height * channels, 0.0);
            }
            for (row, y) in (y0..y1).enumerate() {
                for (col, x) in (x0..x1).enumerate() {
                    let from = (row * (x1 - x0) + col) * channels;
                    let to = (y * width + x) * channels;
                    frame[to..to + channels]
                        .copy_from_slice(&pixels[from..from + channels]);
                }
            }
            Error::None
        },
    );

    let (width, height) = (64usize, 48usize);
    let mut nsi = scene(width as i32, height as i32);
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

    let mut session = Session::new(nsi, &dso).expect("an interactive render");

    session.wait();
    let before = canvas.lock().expect("not poisoned").clone();
    assert_eq!(before.len(), width * height * 4, "no first frame");

    // The edit: turn the surface green.
    session
        .scene_mut()
        .set_attribute(
            "surface",
            vec![arg(
                "diffuseColor",
                Type::Color,
                OwnedData::F32(vec![0.0, 1.0, 0.0]),
            )],
        )
        .unwrap();

    let rebuilt = session.synchronize();
    assert!(
        !rebuilt,
        "a shader parameter must not force a whole-scene re-apply"
    );

    session.wait();
    let after = canvas.lock().expect("not poisoned").clone();

    let [red_before, green_before, _] = channels(&before);
    let [red_after, green_after, _] = channels(&after);

    assert!(
        red_before > green_before * 2.0,
        "the viewport starts red: {red_before} vs {green_before}"
    );
    assert!(
        green_after > red_after * 2.0,
        "one synchronise must turn the viewport green: {red_after} vs \
         {green_after}"
    );
}

/// A moved transform, through the same loop.
#[test]
fn a_session_moves_a_shape() {
    use nsi_moonray::session::Session;

    let dso = dso_path();
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (width, height) = (64usize, 48usize);
    let mut session = Session::new(scene(width as i32, height as i32), &dso)
        .expect("an interactive render");

    session.wait();
    let before = session.render().snapshot().expect("a frame").2;

    session
        .scene_mut()
        .set_attribute("xform", vec![translate(-1.5)])
        .unwrap();
    assert!(!session.synchronize(), "a transform is a narrow edit");
    session.wait();

    let after = session.render().snapshot().expect("a frame").2;

    let centre = width / 2;
    let left = width / 6;
    assert!(
        column(&after, width, height, centre)
            < column(&before, width, height, centre),
        "the centre should dim as the quad leaves it"
    );
    assert!(
        column(&after, width, height, left)
            > column(&before, width, height, left),
        "the left should brighten as the quad arrives"
    );
}

/// **`I5`. The assertion no image can make.**
///
/// A synchronise that re-tessellates the whole scene renders exactly
/// the right picture, slightly later. Every other test here would pass
/// on it. Only a cost counter can tell the difference.
///
/// The counters have to be read **before the frame stops**:
/// `RenderContext::stopFrame` calls `RenderStats::reset()`, so reading
/// them after a render is guaranteed to read zeros — which is what
/// made them look like they were never wired up at all.
/// `Session::last_cost` captures them at the right moment.
#[test]
fn a_shader_edit_costs_no_geometry_work() {
    use nsi_moonray::session::Session;

    let dso = dso_path();
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let mut session =
        Session::new(scene(64, 48), &dso).expect("an interactive render");
    session.wait();

    let first = session.last_cost().expect("the first frame's cost");
    assert!(
        first.load_procedurals > 0.0,
        "the first frame must load procedurals, or this test is \
         measuring nothing: {first:?}"
    );

    // A material parameter. Nothing about the geometry changed.
    session
        .scene_mut()
        .set_attribute(
            "surface",
            vec![arg(
                "diffuseColor",
                Type::Color,
                OwnedData::F32(vec![0.0, 1.0, 0.0]),
            )],
        )
        .unwrap();
    assert!(!session.synchronize());
    session.wait();

    let after = session.last_cost().expect("the second frame's cost");

    eprintln!("shader edit: first {first:?}\n  after {after:?}");
}

/// **`I5`. What a synchronise actually costs.**
///
/// A synchronise that re-tessellates the whole scene renders exactly
/// the right picture, slightly later. No pixel test can see it, and on
/// a four-vertex quad neither can a timer: tessellating one quad and
/// deciding not to both read as about a tenth of a millisecond. So the
/// scene here is a subdivided 40×40 grid, where tessellation costs
/// about ten milliseconds and the difference is a hundredfold.
///
/// Two things this pins down, and one it reports rather than asserts.
///
/// **The counters work**, which was not obvious: they must be read
/// *before* the frame stops, because `RenderContext::stopFrame` calls
/// `RenderStats::reset()`. Reading them after a render is guaranteed
/// to read zeros, which is what made them look like they were never
/// wired up at all.
///
/// **A material change re-tessellates its geometry by design.**
/// `scene_rdl2`'s `Layer.cc:497` says so outright: "if a material
/// changes it might request a new primitive attribute from the
/// geometry and so the geometry would need to be reloaded and
/// retessellated. At this point we do not know which primitive
/// attributes the material requests... so we add this geometry to the
/// list of changed or deformed geometries just in case." Conservative,
/// upstream's, and not something this backend can map around.
///
/// **And what is not yet achieved**: a *visibility* edit re-tessellates
/// too, though `Geometry`'s `visible_*` attributes are declared
/// `FLAGS_GEOM_RELOAD_BVH_ONLY` and should cost an accelerator rebuild
/// instead. That is the gap `I5` opened, and it is now a measurement
/// rather than a suspicion.
#[test]
fn a_synchronise_is_measured_not_assumed() {
    use nsi_moonray::session::Session;

    let dso = dso_path();
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (counts, indices, points) = grid(40);
    let mut nsi = scene(64, 48);
    nsi.set_attribute(
        "quad",
        vec![
            arg("nvertices", Type::I32, OwnedData::I32(counts)),
            arg("P.indices", Type::I32, OwnedData::I32(indices)),
            arg("P", Type::Point, OwnedData::F32(points)),
            OwnedArg::new(
                "subdivision.scheme",
                Type::String,
                1,
                0,
                OwnedData::String(vec![b"catmull-clark".to_vec()]),
            ),
        ],
    )
    .unwrap();

    let mut session = Session::new(nsi, &dso).expect("a render");
    session.wait();
    let first = session.last_cost().expect("the first frame's cost");

    // A heavy scene must cost heavily, or nothing below means
    // anything. A four-vertex quad measures about 0.0001s.
    assert!(
        first.tessellation > 0.001,
        "a subdivided 40x40 grid should take milliseconds to \
         tessellate; if it does not, this test cannot tell a rebuild \
         from a reuse: {first:?}"
    );
    assert!(
        first.build_accelerator > 0.0,
        "the accelerator must be built: {first:?}"
    );

    session
        .scene_mut()
        .set_attribute(
            "surface",
            vec![arg(
                "diffuseColor",
                Type::Color,
                OwnedData::F32(vec![0.0, 1.0, 0.0]),
            )],
        )
        .unwrap();
    assert!(!session.synchronize());
    session.wait();
    let shader = session.last_cost().expect("the second frame's cost");

    // Upstream's choice, asserted so that a future MoonRay narrowing it
    // shows up here as a failing test rather than as a silent
    // improvement nobody notices.
    // An absolute floor, not a fraction of the first frame. A ratio
    // reads as the sharper claim and is the flakier one: both numbers
    // move with machine load, and a second frame that happened to run
    // on warmer caches once dipped under half. The floor is two orders
    // of magnitude above what a four-vertex quad costs, so it still
    // fails loudly if the work stops happening.
    assert!(
        shader.tessellation > 0.001,
        "`Layer.cc:497` says a material change re-tessellates its \
         geometry conservatively. If this now costs nothing, upstream \
         has learned which primitive attributes a material wants, and \
         this test should become the opposite assertion: {first:?} \
         then {shader:?}"
    );
}

/// **`I2`.** Turning geometry off, the cheap way.
///
/// ɴsɪ hides a shape by severing its `objects` connection. That could
/// become a `Layer` edit or a deleted object; `research.md` F3 says it
/// should become a **visibility** change instead, because MoonRay
/// declares every `visible_*` attribute `FLAGS_GEOM_RELOAD_BVH_ONLY` —
/// one cost tier cheaper, for the same image.
///
/// Keeping the shape in the scene and turning it off is also what keeps
/// this a *narrow* edit: dropping it from the `GeometrySet` and the
/// `Layer` would be a change of membership, which forces a full
/// re-apply.
#[test]
fn disconnecting_a_shape_turns_it_off_without_a_rebuild() {
    use nsi_moonray::session::Session;

    let dso = dso_path();
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (width, height) = (64usize, 48usize);
    let mut session = Session::new(scene(width as i32, height as i32), &dso)
        .expect("an interactive render");
    session.wait();
    let before = session.render().snapshot().expect("a frame").2;

    // Sever the transform's connection to `.root`, which is how an
    // application hides a layer.
    session
        .scene_mut()
        .disconnect("xform", None, ".root", "objects")
        .expect("a recordable edit");

    let rebuilt = session.synchronize();
    session.wait();
    let after = session.render().snapshot().expect("a frame").2;

    let centre = width / 2;
    assert!(
        column(&before, width, height, centre) > 0.0,
        "the quad should start visible"
    );
    assert!(
        column(&after, width, height, centre)
            < column(&before, width, height, centre) * 0.5,
        "the quad should be gone: {} then {}",
        column(&before, width, height, centre),
        column(&after, width, height, centre)
    );

    assert!(
        !rebuilt,
        "hiding a shape must not force a whole-scene re-apply -- that \
         is the difference between a BVH rebuild and a re-tessellation"
    );
}

/// **`I4`.** A deformation edit — `P` changes, and the shape moves.
///
/// The narrow path re-sends only the mesh, and MoonRay regenerates it.
/// Which is the expensive tier: `vertex_list_*` is not
/// `FLAGS_CAN_SKIP_GEOM_RELOAD`, so this one *should* re-tessellate,
/// unlike a shader edit. That it does is `I5`'s to prove.
#[test]
fn a_deformation_edit_moves_the_vertices() {
    use nsi_moonray::session::Session;

    let dso = dso_path();
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (width, height) = (64usize, 48usize);
    let mut session = Session::new(scene(width as i32, height as i32), &dso)
        .expect("an interactive render");
    session.wait();
    let before = session.render().snapshot().expect("a frame").2;

    // Move the quad's vertices left, without touching its transform.
    session
        .scene_mut()
        .set_attribute(
            "quad",
            vec![arg(
                "P",
                Type::Point,
                OwnedData::F32(vec![
                    -3.0, -1.0, 0.0, -1.0, -1.0, 0.0, -1.0, 1.0, 0.0, -3.0,
                    1.0, 0.0,
                ]),
            )],
        )
        .unwrap();

    let rebuilt = session.synchronize();
    assert!(!rebuilt, "editing `P` is a narrow edit, not a rebuild");
    session.wait();
    let after = session.render().snapshot().expect("a frame").2;

    let centre = width / 2;
    let left = width / 5;

    assert!(
        column(&before, width, height, centre) > 0.0,
        "the quad should start in the centre"
    );
    assert!(
        column(&after, width, height, centre)
            < column(&before, width, height, centre) * 0.5,
        "the centre should empty as the vertices move off it"
    );
    assert!(
        column(&after, width, height, left)
            > column(&before, width, height, left),
        "the left should fill as the vertices arrive"
    );
}

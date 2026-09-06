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
        .set_attribute("quad", vec![
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
        ])
        .unwrap();

    scene.create("xform", "transform").unwrap();
    scene.set_attribute("xform", vec![translate(0.0)]).unwrap();
    scene.connect("xform", None, ".root", "objects").unwrap();
    scene.connect("quad", None, "xform", "objects").unwrap();

    scene.create("light", "environment").unwrap();
    scene.connect("light", None, ".root", "objects").unwrap();

    scene.create("attr", "attributes").unwrap();
    scene.connect("attr", None, "quad", "geometryattributes").unwrap();
    scene.create("surface", "shader").unwrap();
    scene
        .set_attribute("surface", vec![arg(
            "diffuseColor",
            Type::Color,
            OwnedData::F32(vec![1.0, 0.0, 0.0]),
        )])
        .unwrap();
    scene.connect("surface", None, "attr", "surfaceshader").unwrap();

    scene.create("cam", "perspectivecamera").unwrap();
    scene
        .set_attribute("cam", vec![arg(
            "fov",
            Type::F32,
            OwnedData::F32(vec![45.0]),
        )])
        .unwrap();
    scene.connect("cam", None, ".root", "objects").unwrap();

    scene.create("screen", "screen").unwrap();
    scene
        .set_attribute("screen", vec![arg(
            "resolution",
            Type::I32,
            OwnedData::I32(vec![width, height]),
        )])
        .unwrap();
    scene.connect("screen", None, "cam", "screens").unwrap();

    scene.create("beauty", "outputlayer").unwrap();
    scene.connect("beauty", None, "screen", "outputlayers").unwrap();
    scene.create("driver", "outputdriver").unwrap();
    scene.connect("driver", None, "beauty", "outputdrivers").unwrap();

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
    let render = Render::new(Some(&dso), Some(2), Mode::Batch)
        .expect("a renderer");
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
        apply_affected(&flush(&nsi).document, &live, &changes, &affected);
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

/// **What `scene_updated` is actually load-bearing for.**
///
/// `RenderContext::startFrame` branches three ways: the first frame
/// builds everything; `mSceneUpdated` runs `applyUpdates`, which calls
/// `update()` on every `SceneObject` and rebuilds the attribute tables
/// for shaders the layer says changed; neither means reuse everything.
///
/// So a **shader** edit needs the mark. This asserts the negative --
/// that without it the frame is unchanged -- because that is the shape
/// of the bug: applied but not marked has no symptom except a stale
/// image, and reading the scene back would show the new value and pass.
///
/// A *transform* edit, by contrast, reaches the image without the mark:
/// `GeometryManager` recomputes geometry-to-render matrices from
/// `node_xform` as it loads. Asserting the negative on a transform was
/// the first version of this test, and it failed -- which is how the
/// distinction was found rather than assumed.
#[test]
fn a_shader_edit_that_is_not_marked_renders_the_old_scene() {
    let dso = dso_path();
    let _guard = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|error| error.into_inner());

    let (width, height) = (64usize, 48usize);
    let render = Render::new(Some(&dso), Some(2), Mode::Batch)
        .expect("a renderer");
    let live = render.scene().expect("a scene");

    let mut nsi = scene(width as i32, height as i32);
    apply(&flush(&nsi).document, &live);
    render.initialize().expect("render prep");
    let before = frame(&render);

    let _ = nsi.take_changes();

    // Turn the surface green. `diffuseColor` crosses by exact name.
    nsi.set_attribute("surface", vec![arg(
        "diffuseColor",
        Type::Color,
        OwnedData::F32(vec![0.0, 1.0, 0.0]),
    )])
    .unwrap();

    let changes = nsi.take_changes();
    let affected = nsi.affected(&changes);
    assert!(
        affected.shaders.contains("surface"),
        "upstream must name the shader: {affected:?}"
    );

    apply_affected(&flush(&nsi).document, &live, &changes, &affected);

    // Deliberately no `scene_updated()`.
    let after = frame(&render);

    let [red_before, green_before, _] = channels(&before);
    let [red_after, green_after, _] = channels(&after);

    assert!(
        red_before > green_before * 2.0,
        "the quad starts red: {red_before} vs {green_before}"
    );
    assert!(
        red_after > green_after * 2.0,
        "without `scene_updated` MoonRay skips `applyUpdates`, so the \
         shader's new value never reaches the render and the quad is \
         still red. If this ever fails, `scene_updated` has stopped \
         being load-bearing for shaders and the incremental path needs \
         re-checking. red {red_after} vs green {green_after}"
    );

    // And with the mark, it does reach it.
    render.scene_updated().expect("the renderer is told");
    let marked = frame(&render);
    let [red_marked, green_marked, _] = channels(&marked);

    assert!(
        green_marked > red_marked * 2.0,
        "with `scene_updated` the shader edit must reach the image: red \
         {red_marked} vs green {green_marked}"
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

    let render = Render::new(Some(&dso), Some(2), Mode::Batch)
        .expect("a renderer");
    let live = render.scene().expect("a scene");

    let mut nsi = scene(64, 48);
    apply(&flush(&nsi).document, &live);
    let _ = nsi.take_changes();

    nsi.create("second", "mesh").unwrap();
    nsi.connect("second", None, ".root", "objects").unwrap();

    let changes = nsi.take_changes();
    let affected = nsi.affected(&changes);
    let (report, rebuilt) =
        apply_affected(&flush(&nsi).document, &live, &changes, &affected);

    assert!(rebuilt, "a created node must force a full re-apply");
    assert!(
        report.iter().any(|line| line.contains("created")),
        "the fallback must say why: {report:?}"
    );
}

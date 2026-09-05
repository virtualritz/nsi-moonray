//! A flushed scene, through the real renderer.
//!
//! Everything else in this repository checks the scene as *text* — the
//! oracle proves the syntax, and rdl2's own reader proves it parses.
//! Neither shows MoonRay accepting the file, resolving the DSO classes
//! it names and producing an image, and a scene can be perfectly
//! well-formed and still name a class or attribute the renderer does
//! not have.
//!
//! **Skipped when MoonRay is not installed**, which is the normal case:
//! building it is heavy and nothing else here needs it. Set `$MOONRAY`,
//! `$MOONRAY_ROOT`, or put `moonray` on `$PATH` to run it.

use nsi_intermediate::{OwnedArg, OwnedData, Scene};
use nsi_moonray::{flush::flush, render::Render};
use nsi_trait::Type;
use std::{fs, path::PathBuf};

fn arg(name: &str, type_tag: Type, data: OwnedData) -> OwnedArg {
    OwnedArg {
        name: name.to_string(),
        type_tag,
        array_length: 1,
        flags: 0,
        data,
    }
}

/// A unit quad in the XY plane at `z`, as an ɴsɪ mesh.
fn quad(scene: &mut Scene, handle: &str, z: f32) {
    scene.create(handle, "mesh");
    scene.set_attribute(
        handle,
        vec![
            arg("nvertices", Type::I32, OwnedData::I32(vec![4])),
            arg("P.indices", Type::I32, OwnedData::I32(vec![0, 1, 2, 3])),
            arg(
                "P",
                Type::Point,
                OwnedData::F32(vec![
                    -1.0, -1.0, z, 1.0, -1.0, z, 1.0, 1.0, z, -1.0, 1.0, z,
                ]),
            ),
        ],
    );
}

/// A `shader` carrying one colour, bound to `geometry` through an
/// `attributes` node -- the two-hop ɴsɪ routing that
/// `nsi-intermediate` dissolves.
fn shade(scene: &mut Scene, geometry: &str, colour: [f32; 3]) {
    let shader = format!("{geometry}_shader");
    let attributes = format!("{geometry}_attributes");

    scene.create(&shader, "shader");
    scene.set_attribute(
        &shader,
        vec![arg(
            "diffuseColor",
            Type::Color,
            OwnedData::F32(colour.to_vec()),
        )],
    );
    scene.create(&attributes, "attributes");
    scene
        .connect(&shader, None, &attributes, "surfaceshader")
        .expect("known attribute");
    scene
        .connect(&attributes, None, geometry, "geometryattributes")
        .expect("known attribute");
}

/// A translation, as ɴsɪ stores it: row-major, translation in the last
/// row.
fn translate(x: f64, y: f64, z: f64) -> OwnedArg {
    #[rustfmt::skip]
    let matrix = vec![
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
          x,   y,   z, 1.0,
    ];
    arg(
        "transformationmatrix",
        Type::MatrixF64,
        OwnedData::F64(matrix),
    )
}

/// The camera, screen and light every scene here needs.
fn viewing(scene: &mut Scene, width: i32, height: i32) {
    scene.create("cam", "perspectivecamera");
    scene.set_attribute(
        "cam",
        vec![arg("fov", Type::F32, OwnedData::F32(vec![45.0]))],
    );

    scene.create("screen", "screen");
    scene.set_attribute(
        "screen",
        vec![arg(
            "resolution",
            Type::I32,
            OwnedData::I32(vec![width, height]),
        )],
    );
    scene
        .connect("screen", None, "cam", "screens")
        .expect("known attribute");

    scene.create("env", "environment");
}

/// Render a scene and read the image back as RGB rows.
fn render(name: &str, scene: &Scene, size: (usize, usize)) -> Image {
    let directory = std::env::temp_dir().join("nsi-moonray-render");
    fs::create_dir_all(&directory).expect("a writable temporary directory");
    let scene_file = directory.join(format!("{name}.rdla"));
    let image = directory.join(format!("{name}.exr"));
    let _ = fs::remove_file(&image);

    let flushed = flush(scene);
    fs::write(&scene_file, flushed.to_rdla()).expect("writing the scene");

    let mut job = Render::new(&scene_file);
    job.image = Some(image.clone());
    job.threads = Some(2);
    job.run().unwrap_or_else(|error| {
        panic!("rendering {}: {error}", scene_file.display())
    });

    Image::read(&image, size)
}

/// A rendered image, kept as RGB triples.
struct Image {
    pixels: Vec<[f32; 3]>,
    width: usize,
}

impl Image {
    fn read(path: &PathBuf, (width, height): (usize, usize)) -> Self {
        let image = exr::prelude::read_first_rgba_layer_from_file(
            path,
            move |_, _| vec![[0.0f32; 3]; width * height],
            move |pixels: &mut Vec<[f32; 3]>,
                  position,
                  (red, green, blue, _): (f32, f32, f32, f32)| {
                pixels[position.y() * width + position.x()] =
                    [red, green, blue];
            },
        )
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()));

        Self {
            pixels: image.layer_data.channel_data.pixels,
            width,
        }
    }

    fn at(&self, x: usize, y: usize) -> [f32; 3] {
        self.pixels[y * self.width + x]
    }

    fn brightest(&self) -> f32 {
        self.pixels.iter().flatten().copied().fold(0.0f32, f32::max)
    }
}

/// A triangle, a camera, a screen and an output.
fn scene() -> Scene {
    let mut scene = Scene::default();

    scene.create("tri", "mesh");
    scene.set_attribute(
        "tri",
        vec![
            arg("nvertices", Type::I32, OwnedData::I32(vec![3])),
            arg("P.indices", Type::I32, OwnedData::I32(vec![0, 1, 2])),
            arg(
                "P",
                Type::Point,
                OwnedData::F32(vec![
                    -1.0, -1.0, -5.0, 1.0, -1.0, -5.0, 0.0, 1.0, -5.0,
                ]),
            ),
        ],
    );
    scene
        .connect("tri", None, ".root", "objects")
        .expect("known attribute");

    // Without a light the render is black, and a black image would
    // still pass this test -- but it would be a worse smoke test, and
    // an `environment` is one node.
    viewing(&mut scene, 64, 48);

    scene
}

#[test]
fn moonray_renders_what_the_flush_writes() {
    let Ok(binary) = nsi_moonray::render::binary() else {
        eprintln!(
            "skipped: no `moonray` binary. Set $MOONRAY, $MOONRAY_ROOT, or \
             put it on $PATH."
        );
        return;
    };

    let directory = std::env::temp_dir().join("nsi-moonray-render");
    fs::create_dir_all(&directory).expect("a writable temporary directory");
    let scene_file = directory.join("triangle.rdla");
    let image: PathBuf = directory.join("triangle.exr");
    let _ = fs::remove_file(&image);

    let flushed = flush(&scene());
    fs::write(&scene_file, flushed.to_rdla()).expect("writing the scene");

    let mut job = Render::new(&scene_file);
    job.image = Some(image.clone());
    job.threads = Some(2);

    job.run().unwrap_or_else(|error| {
        panic!(
            "{} rendering {}: {error}",
            binary.display(),
            scene_file.display()
        )
    });

    let brightest = Image::read(&image, (64, 48)).brightest();
    assert!(
        brightest > 0.0,
        "{} is black: the scene rendered nothing. A .rdla can be \
         well-formed, parse, and still put no geometry in the image -- \
         MoonRay skips a layer row whose material column is undef(), \
         which is why the flush assigns a default surface.",
        image.display()
    );
}

/// **The inherited top risk**: two shapes, two materials, each correct.
///
/// A misclassified ɴsɪ connection does not error. It renders — with
/// the materials on the wrong shapes — so nothing short of looking at
/// the pixels catches it. The quads are placed left and right of centre
/// and coloured red and green, and the test reads the two halves back.
#[test]
fn two_materials_land_on_the_right_two_shapes() {
    if nsi_moonray::render::binary().is_err() {
        eprintln!("skipped: no `moonray` binary");
        return;
    }

    let mut scene = Scene::default();
    let (width, height) = (64usize, 48usize);
    viewing(&mut scene, width as i32, height as i32);

    for (handle, x, colour) in [
        ("left", -1.6, [1.0, 0.0, 0.0]),
        ("right", 1.6, [0.0, 1.0, 0.0]),
    ] {
        quad(&mut scene, handle, 0.0);
        shade(&mut scene, handle, colour);

        let transform = format!("{handle}_xform");
        scene.create(&transform, "transform");
        scene.set_attribute(&transform, vec![translate(x, 0.0, -6.0)]);
        scene
            .connect(handle, None, &transform, "objects")
            .expect("known attribute");
        scene
            .connect(&transform, None, ".root", "objects")
            .expect("known attribute");
    }

    let image = render("two_materials", &scene, (width, height));

    // A quarter and three quarters across, mid-height: the middle of
    // each quad.
    let left = image.at(width / 4, height / 2);
    let right = image.at(3 * width / 4, height / 2);

    assert!(
        left[0] > left[1] * 4.0 && left[0] > 0.0,
        "the left shape should be red, and is {left:?}"
    );
    assert!(
        right[1] > right[0] * 4.0 && right[1] > 0.0,
        "the right shape should be green, and is {right:?}"
    );
}

/// A translated mesh renders where the transform puts it.
///
/// Emitting the right matrix and the shape landing in the right place
/// are different claims, and only the second one is what a user sees.
#[test]
fn a_transform_moves_the_shape() {
    if nsi_moonray::render::binary().is_err() {
        eprintln!("skipped: no `moonray` binary");
        return;
    }

    let (width, height) = (64usize, 48usize);

    let mut centred = Scene::default();
    viewing(&mut centred, width as i32, height as i32);
    quad(&mut centred, "quad", -6.0);
    centred
        .connect("quad", None, ".root", "objects")
        .expect("known attribute");
    let before = render("untranslated", &centred, (width, height));

    let mut moved = Scene::default();
    viewing(&mut moved, width as i32, height as i32);
    quad(&mut moved, "quad", 0.0);
    moved.create("xform", "transform");
    // Two units left, and back to the same depth.
    moved.set_attribute("xform", vec![translate(-2.0, 0.0, -6.0)]);
    moved
        .connect("quad", None, "xform", "objects")
        .expect("known attribute");
    moved
        .connect("xform", None, ".root", "objects")
        .expect("known attribute");
    let after = render("translated", &moved, (width, height));

    assert!(before.brightest() > 0.0, "the untranslated quad is missing");
    assert!(after.brightest() > 0.0, "the translated quad is missing");

    // Centre of frame: covered before the translation, empty after it.
    let centre_before = before.at(width / 2, height / 2);
    let centre_after = after.at(width / 2, height / 2);
    assert!(
        centre_before[0] > 0.0,
        "the untranslated quad should cover the centre, and the pixel is \
         {centre_before:?}"
    );
    assert!(
        centre_after == [0.0, 0.0, 0.0],
        "the quad should have moved off the centre, and the pixel is \
         {centre_after:?}"
    );

    // Left of frame: empty before, covered after.
    let left_before = before.at(width / 8, height / 2);
    let left_after = after.at(width / 8, height / 2);
    assert!(
        left_before == [0.0, 0.0, 0.0],
        "nothing should be at the left before the translation, and the \
         pixel is {left_before:?}"
    );
    assert!(
        left_after[0] > 0.0,
        "the quad should have moved left, and the pixel is {left_after:?}"
    );
}

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
    OwnedArg::new(name, type_tag, 1, 0, data)
}

/// A unit quad in the XY plane at `z`, as an ɴsɪ mesh.
fn quad(scene: &mut Scene, handle: &str, z: f32) {
    scene.create(handle, "mesh").expect("a recordable edit");
    scene
        .set_attribute(
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
        )
        .expect("a recordable edit");
}

/// A `shader` carrying one colour, bound to `geometry` through an
/// `attributes` node -- the two-hop ɴsɪ routing that
/// `nsi-intermediate` dissolves.
fn shade(scene: &mut Scene, geometry: &str, colour: [f32; 3]) {
    let shader = format!("{geometry}_shader");
    let attributes = format!("{geometry}_attributes");

    scene.create(&shader, "shader").expect("a recordable edit");
    scene
        .set_attribute(
            &shader,
            vec![arg(
                "diffuseColor",
                Type::Color,
                OwnedData::F32(colour.to_vec()),
            )],
        )
        .expect("a recordable edit");
    scene
        .create(&attributes, "attributes")
        .expect("a recordable edit");
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
    scene
        .create("cam", "perspectivecamera")
        .expect("a recordable edit");
    scene
        .set_attribute(
            "cam",
            vec![arg("fov", Type::F32, OwnedData::F32(vec![45.0]))],
        )
        .expect("a recordable edit");

    scene.create("screen", "screen").expect("a recordable edit");
    scene
        .set_attribute(
            "screen",
            vec![arg(
                "resolution",
                Type::I32,
                OwnedData::I32(vec![width, height]),
            )],
        )
        .expect("a recordable edit");
    scene
        .connect("screen", None, "cam", "screens")
        .expect("known attribute");

    scene
        .create("env", "environment")
        .expect("a recordable edit");
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

    scene.create("tri", "mesh").expect("a recordable edit");
    scene
        .set_attribute(
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
        )
        .expect("a recordable edit");
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
        scene
            .create(&transform, "transform")
            .expect("a recordable edit");
        scene
            .set_attribute(&transform, vec![translate(x, 0.0, -6.0)])
            .expect("a recordable edit");
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
    moved
        .create("xform", "transform")
        .expect("a recordable edit");
    // Two units left, and back to the same depth.
    moved
        .set_attribute("xform", vec![translate(-2.0, 0.0, -6.0)])
        .expect("a recordable edit");
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

/// A moving transform renders blurred.
///
/// The capability that distinguishes this backend from the Mitsuba
/// one, which cannot blur at all — and the assertion has to be about
/// pixels, because emitting `blur(a, b)` and MoonRay acting on it are
/// different claims. A blurred edge is a partially covered pixel where
/// a sharp one is either covered or not.
#[test]
fn a_moving_shape_renders_blurred() {
    if nsi_moonray::render::binary().is_err() {
        eprintln!("skipped: no `moonray` binary");
        return;
    }

    let (width, height) = (64usize, 48usize);

    let mut still = Scene::default();
    viewing(&mut still, width as i32, height as i32);
    quad(&mut still, "quad", -6.0);
    still
        .connect("quad", None, ".root", "objects")
        .expect("known attribute");
    shade(&mut still, "quad", [1.0, 1.0, 1.0]);
    let sharp = render("sharp", &still, (width, height));

    let mut moving = Scene::default();
    viewing(&mut moving, width as i32, height as i32);
    quad(&mut moving, "quad", 0.0);
    shade(&mut moving, "quad", [1.0, 1.0, 1.0]);
    moving.create("xf", "transform").expect("a fresh handle");
    for (time, x) in [(0.0, -1.5), (1.0, 1.5)] {
        moving
            .set_attribute_at_time("xf", time, vec![translate(x, 0.0, -6.0)])
            .expect("a recordable edit");
    }
    moving
        .connect("quad", None, "xf", "objects")
        .expect("known attribute");
    moving
        .connect("xf", None, ".root", "objects")
        .expect("known attribute");
    let blurred = render("blurred", &moving, (width, height));

    // A sharp vertical edge lands between two pixel columns; a blurred
    // one smears across many. Counting columns that are neither empty
    // nor fully lit along the middle row is the difference.
    let partial = |image: &Image| {
        (0..width)
            .filter(|x| {
                let value = image.at(*x, height / 2)[0];
                value > 0.001 && value < 0.9 * image.brightest()
            })
            .count()
    };

    let (sharp_edge, blurred_edge) = (partial(&sharp), partial(&blurred));
    assert!(
        blurred_edge > sharp_edge + 2,
        "the moving quad should smear across more columns than the still \
         one: {blurred_edge} against {sharp_edge}"
    );
}

/// The whole path an application with a viewport uses: ɴsɪ calls in,
/// pixels back to its own closure.
///
/// Not `.rdla` inspected, not a file checked afterwards — the closure
/// the application wrote receives a bucket. That is the claim; the
/// route the pixels take to get there is this backend's business and
/// changes when the renderer runs in process.
#[test]
fn an_applications_callback_receives_the_rendered_pixels() {
    if nsi_moonray::render::binary().is_err() {
        eprintln!("skipped: no `moonray` binary");
        return;
    }

    use nsi_ffi_wrap::{
        argument::CallbackPtr,
        output::{Error, PixelFormat, WriteCallback},
    };
    use nsi_intermediate::HostPtr;
    use std::sync::{Arc, Mutex};

    // What the closure saw: the pixels, and the shape it was told they
    // are in. The channel count is not four -- MoonRay writes the ɴsɪ
    // output layer's own channels, `Ci.R`, `Ci.G`, `Ci.B` for a beauty
    // pass with no alpha -- and a delivery that assumed RGBA would hand
    // the application a buffer of the wrong width.
    #[derive(Default)]
    struct Delivered {
        pixels: Vec<f32>,
        width: usize,
        height: usize,
        channels: usize,
    }

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

    let (width, height) = (32usize, 24usize);
    let directory = std::env::temp_dir().join("nsi-moonray-render");
    fs::create_dir_all(&directory).expect("a writable temporary directory");
    let image = directory.join("callback.exr");
    let _ = fs::remove_file(&image);

    let mut scene = Scene::default();
    viewing(&mut scene, width as i32, height as i32);
    quad(&mut scene, "quad", -6.0);
    scene
        .connect("quad", None, ".root", "objects")
        .expect("known attribute");
    shade(&mut scene, "quad", [1.0, 1.0, 1.0]);

    scene
        .create("beauty", "outputlayer")
        .expect("a fresh handle");
    scene
        .set_attribute(
            "beauty",
            vec![arg(
                "variablename",
                Type::String,
                OwnedData::String(vec![b"Ci".to_vec()]),
            )],
        )
        .expect("a recordable edit");
    scene
        .connect("beauty", None, "screen", "outputlayers")
        .expect("known attribute");

    scene
        .create("driver", "outputdriver")
        .expect("a fresh handle");
    scene
        .set_attribute(
            "driver",
            vec![
                arg(
                    "imagefilename",
                    Type::String,
                    OwnedData::String(vec![
                        image.to_string_lossy().as_bytes().to_vec(),
                    ]),
                ),
                arg(
                    "callback.write",
                    Type::Reference,
                    OwnedData::Reference(vec![HostPtr(write.to_ptr())]),
                ),
            ],
        )
        .expect("a recordable edit");
    scene
        .connect("driver", None, "beauty", "outputdrivers")
        .expect("known attribute");

    // Render it the way the C API does, then deliver.
    let scene_file = directory.join("callback.rdla");
    let flushed = flush(&scene);
    fs::write(&scene_file, flushed.to_rdla()).expect("writing the scene");
    let mut job = Render::new(&scene_file);
    job.threads = Some(2);
    job.run().expect("the render runs");

    let callbacks = nsi_moonray::display::Callbacks::of(&scene, "driver")
        .expect("the callback was recorded");
    nsi_moonray::display::deliver_file(&callbacks, "driver", &image)
        .expect("the image is delivered");

    let delivered = received.lock().expect("not poisoned");

    assert_eq!((delivered.width, delivered.height), (width, height));
    assert!(
        delivered.channels > 0,
        "the closure was told the frame has no channels"
    );
    assert_eq!(
        delivered.pixels.len(),
        width * height * delivered.channels,
        "the closure should receive the whole frame, at the channel count \
         it was handed"
    );
    assert!(
        delivered.pixels.iter().any(|value| *value > 0.0),
        "the closure received a black frame"
    );
}

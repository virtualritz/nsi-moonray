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

use nsi_moonray::{flush::flush, render::Render};
use std::{fs, path::PathBuf};

/// A triangle, a camera, a screen and an output.
fn scene() -> nsi_intermediate::Scene {
    use nsi_intermediate::{OwnedArg, OwnedData, Scene};
    use nsi_trait::Type;

    fn arg(name: &str, type_tag: Type, data: OwnedData) -> OwnedArg {
        OwnedArg {
            name: name.to_string(),
            type_tag,
            array_length: 1,
            flags: 0,
            data,
        }
    }

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

    scene.create("cam", "perspectivecamera");
    scene.set_attribute(
        "cam",
        vec![arg("fov", Type::F32, OwnedData::F32(vec![45.0]))],
    );

    scene.create("screen", "screen");
    scene.set_attribute(
        "screen",
        vec![arg("resolution", Type::I32, OwnedData::I32(vec![64, 48]))],
    );
    scene
        .connect("screen", None, "cam", "screens")
        .expect("known attribute");

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

    let written = fs::metadata(&image)
        .unwrap_or_else(|error| panic!("{}: {error}", image.display()));
    assert!(written.len() > 0, "{} is empty", image.display());
}

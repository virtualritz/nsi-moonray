//! `mrr` takes a `.nsi` stream, not only a `.rdla`.
//!
//! `T4.3`. The parser is upstream's (`nsi-parse`) and it drives
//! `nsi_trait::Nsi`, which `nsi_intermediate::Recorder` implements — so
//! an ɴsɪ stream feeds the same `Scene` the C entry points record into,
//! and the flush that follows is the one every other test exercises.
//! There is nothing here but the wiring, which is the point.

use std::path::PathBuf;

/// The smallest ɴsɪ stream that is a scene.
fn stream(image: &std::path::Path) -> String {
    format!(
        r#"Create "cam" "perspectivecamera"
SetAttribute "cam" "fov" "float" 1 45
Connect "cam" "" ".root" "objects"
Create "screen" "screen"
SetAttribute "screen" "resolution" "int[2]" 1 [64 48]
Connect "screen" "" "cam" "screens"
Create "light" "environment"
Connect "light" "" ".root" "objects"
Create "quad" "mesh"
SetAttribute "quad" "nvertices" "int" 1 4
SetAttribute "quad" "P.indices" "int" 4 [0 1 2 3]
SetAttribute "quad" "P" "point" 4 [-1 -1 -5  1 -1 -5  1 1 -5  -1 1 -5]
Connect "quad" "" ".root" "objects"
Create "beauty" "outputlayer"
SetAttribute "beauty" "variablename" "string" 1 "Ci"
Connect "beauty" "" "screen" "outputlayers"
Create "driver" "outputdriver"
SetAttribute "driver" "imagefilename" "string" 1 "{}"
Connect "driver" "" "beauty" "outputdrivers"
"#,
        image.display()
    )
}

fn mrr() -> PathBuf {
    // The test binary sits beside the ones cargo built for this crate.
    let mut path = std::env::current_exe().expect("a test binary path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join("mrr")
}

/// An ɴsɪ stream becomes a `.rdla`, and the `.rdla` is what MoonRay is
/// told to render.
///
/// `--print` rather than a render, so this runs on a host with no
/// MoonRay: what is being checked is the translation and the wiring,
/// and the rendering half has its own tests.
#[test]
fn an_nsi_stream_is_flushed_before_rendering() {
    let directory = std::env::temp_dir().join("nsi-moonray-nsi-input");
    std::fs::create_dir_all(&directory).expect("a writable directory");
    let scene = directory.join("triangle.nsi");
    let image = directory.join("triangle.exr");
    let rdla = directory.join("triangle.rdla");
    let _ = std::fs::remove_file(&rdla);

    std::fs::write(&scene, stream(&image)).expect("the stream is written");

    let mrr = mrr();
    if !mrr.exists() {
        eprintln!("skipped: no `mrr` at {}", mrr.display());
        return;
    }

    let output = std::process::Command::new(&mrr)
        .arg(&scene)
        .arg("--print")
        .output()
        .expect("mrr runs");

    assert!(
        output.status.success(),
        "mrr failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // The scene handed to MoonRay is the flushed `.rdla`, written
    // beside the stream — which is also what someone debugging the
    // translation wants to look at.
    let command = String::from_utf8_lossy(&output.stdout);
    assert!(
        command.contains("triangle.rdla"),
        "MoonRay should be given the flushed scene: {command}"
    );

    let written = std::fs::read_to_string(&rdla).expect("the flush ran");
    assert!(
        written.contains("RdlMeshGeometry(\"quad\")"),
        "the mesh should have crossed\n{written}"
    );
    assert!(
        written.contains("PerspectiveCamera(\"cam\")"),
        "the camera should have crossed\n{written}"
    );
    assert!(
        written.contains("[\"image_width\"] = 64"),
        "the screen's resolution should have crossed\n{written}"
    );
}

/// A `.rdla` is handed over as it stands, whatever it is called.
///
/// Told apart by *content*: a file named `.nsi` that is really `.rdla`
/// is a thing that happens, and guessing from the name would fail with
/// a parse error about the wrong format.
#[test]
fn an_rdla_is_not_parsed_as_nsi() {
    let directory = std::env::temp_dir().join("nsi-moonray-nsi-input");
    std::fs::create_dir_all(&directory).expect("a writable directory");
    // Deliberately misnamed.
    let scene = directory.join("actually-rdla.nsi");
    std::fs::write(
        &scene,
        "SceneVariables {\n    [\"image_width\"] = 64,\n}\n",
    )
    .expect("written");

    let mrr = mrr();
    if !mrr.exists() {
        eprintln!("skipped: no `mrr`");
        return;
    }

    let output = std::process::Command::new(&mrr)
        .arg(&scene)
        .arg("--print")
        .output()
        .expect("mrr runs");

    assert!(
        output.status.success(),
        "an `.rdla` named `.nsi` must be passed through, not parsed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("actually-rdla.nsi"),
        "it should be handed over under its own name: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

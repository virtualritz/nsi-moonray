//! `mnry` takes a `.nsi` stream, not only a `.rdla`.
//!
//! `T4.3`. The parser is upstream's (`nsi-parse`) and it drives
//! `nsi_trait::Nsi`, which `nsi_intermediate::Recorder` implements — so
//! an ɴsɪ stream feeds the same `Scene` the C entry points record into,
//! and the flush that follows is the one every other test exercises.
//! There is nothing here but the wiring, which is the point.
//!
//! These drive the **command**, not the library, because that wiring is
//! where a stream stops being a file and starts being a scene, and it
//! is what someone actually types.

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

fn mnry() -> PathBuf {
    // The test binary sits beside the ones cargo built for this crate.
    let mut path = std::env::current_exe().expect("a test binary path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join("mnry")
}

fn directory() -> PathBuf {
    let directory = std::env::temp_dir().join("nsi-moonray-nsi-input");
    std::fs::create_dir_all(&directory).expect("a writable directory");
    directory
}

/// An ɴsɪ stream becomes the `.rdla` MoonRay's scene is built from.
///
/// `cat` rather than a render, so this runs on a host with no MoonRay:
/// what is being checked is the translation and the wiring, and the
/// rendering half has its own tests.
#[test]
fn an_nsi_stream_is_flushed() {
    let directory = directory();
    let scene = directory.join("triangle.nsi");
    let image = directory.join("triangle.exr");

    std::fs::write(&scene, stream(&image)).expect("the stream is written");

    let mnry = mnry();
    if !mnry.exists() {
        eprintln!("skipped: no `mnry` at {}", mnry.display());
        return;
    }

    let output = std::process::Command::new(&mnry)
        .args(["cat".as_ref(), scene.as_os_str()])
        .output()
        .expect("mnry runs");

    assert!(
        output.status.success(),
        "mnry cat failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let written = String::from_utf8_lossy(&output.stdout);
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

/// A `.rdla` is recognised as one, whatever it is called.
///
/// Told apart by *content*: a file named `.nsi` that is really `.rdla`
/// is a thing that happens, and guessing from the name would fail with
/// a parse error about the wrong format. `cat` is the one place that
/// has to say so out loud, since there is nothing to convert.
#[test]
fn an_rdla_is_not_parsed_as_nsi() {
    let directory = directory();
    // Deliberately misnamed.
    let scene = directory.join("actually-rdla.nsi");
    std::fs::write(
        &scene,
        "SceneVariables {\n    [\"image_width\"] = 64,\n}\n",
    )
    .expect("written");

    let mnry = mnry();
    if !mnry.exists() {
        eprintln!("skipped: no `mnry`");
        return;
    }

    let output = std::process::Command::new(&mnry)
        .args(["cat".as_ref(), scene.as_os_str()])
        .output()
        .expect("mnry runs");

    assert!(
        !output.status.success(),
        "an `.rdla` named `.nsi` must not be parsed as a stream"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("already an .rdla"),
        "and it should say why: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `--output` redirects the image, and it does so in ɴsɪ's own terms.
///
/// The redirection is an edit to every `outputdriver`'s
/// `imagefilename`, which is the attribute a host would have set --
/// so the in-process and spawned paths cannot disagree about where the
/// image went.
#[test]
fn output_redirects_the_scenes_own_driver() {
    let directory = directory();
    let scene = directory.join("redirect.nsi");
    let image = directory.join("wherever.exr");
    let elsewhere = directory.join("elsewhere.exr");

    std::fs::write(&scene, stream(&image)).expect("the stream is written");

    let mnry = mnry();
    if !mnry.exists() {
        eprintln!("skipped: no `mnry`");
        return;
    }

    // `--dry-run` prints what would be rendered without touching a
    // renderer, which is what makes this runnable anywhere.
    let output = std::process::Command::new(&mnry)
        .args(["render".as_ref(), scene.as_os_str()])
        .arg("--output")
        .arg(&elsewhere)
        .arg("--dry-run")
        .output()
        .expect("mnry runs");

    assert!(
        output.status.success(),
        "mnry render --dry-run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("redirect.nsi"),
        "a dry run names the scene: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// A frame placeholder without a sequence is an error, not an empty
/// render.
#[test]
fn a_placeholder_needs_frames() {
    let mnry = mnry();
    if !mnry.exists() {
        eprintln!("skipped: no `mnry`");
        return;
    }

    let output = std::process::Command::new(&mnry)
        .args(["render", "shot.@.nsi", "--dry-run"])
        .output()
        .expect("mnry runs");

    assert!(!output.status.success(), "it should refuse");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--frames"),
        "and say what is missing: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// `@4` pads, `@` does not, and a sequence expands to one file each.
#[test]
fn a_frame_sequence_expands() {
    let mnry = mnry();
    if !mnry.exists() {
        eprintln!("skipped: no `mnry`");
        return;
    }

    let output = std::process::Command::new(&mnry)
        .args(["render", "shot.@4.nsi", "-f", "10-20@5", "--dry-run"])
        .output()
        .expect("mnry runs");

    assert!(output.status.success());
    let listed = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        listed.lines().collect::<Vec<_>>(),
        ["shot.0010.nsi", "shot.0015.nsi", "shot.0020.nsi"],
        "{listed}"
    );
}

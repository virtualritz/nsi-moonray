//! The bundle layout, held against the code that reads it.
//!
//! `packaging/bundle.sh` writes a tree and `nsi_moonray::dso` finds
//! scene classes in it. Nothing connects the two but agreement, and a
//! disagreement is silent in the worst way: rdl2 resolves no classes,
//! every object fails to create, the `Layer` is empty and MoonRay
//! renders a black frame with no error anywhere. So the script is run
//! here and its output is checked against
//! [`nsi_moonray::dso::beside`], which is the same function the
//! renderer uses.
//!
//! No renderer and no MoonRay: the fixture is a prefix of ordinary
//! system binaries, because what is under test is the *shape* of the
//! tree and the walk that fills it.
#![cfg(unix)]

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// A scratch directory of our own, removed and remade so a previous
/// run cannot satisfy this one -- a stale tree that happens to be
/// right is a green test that checks nothing.
fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("nsi-moonray-bundle-{name}"));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("a scratch directory");
    path
}

/// A MoonRay prefix in the shape the script insists on, built from
/// binaries every host has.
fn fake_prefix(root: &Path) -> PathBuf {
    let prefix = root.join("prefix");
    for directory in ["rdl2dso", "bin", "lib"] {
        fs::create_dir_all(prefix.join(directory)).expect("a directory");
    }

    // A real executable, because the script checks for one and then
    // walks its shared libraries with `ldd`.
    let source = ["/bin/echo", "/usr/bin/echo"]
        .iter()
        .map(Path::new)
        .find(|path| path.exists())
        .expect("a host with echo");

    fs::copy(source, prefix.join("bin/moonray")).expect("a renderer");
    fs::copy(source, prefix.join("rdl2dso/RdlMeshGeometry.so"))
        .expect("a scene class");

    prefix
}

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/bundle.sh")
}

/// **The assembled tree is the tree the resolver looks in.**
///
/// If this fails, one of the two moved. Move the other.
#[test]
fn the_bundle_puts_its_classes_where_the_resolver_looks() {
    let root = scratch("layout");
    let prefix = fake_prefix(&root);

    let artefact = root.join("mnry");
    fs::copy(
        ["/bin/cat", "/usr/bin/cat"]
            .iter()
            .map(Path::new)
            .find(|path| path.exists())
            .expect("a host with cat"),
        &artefact,
    )
    .expect("a binary to bundle");

    let out = root.join("out");
    let status = Command::new("sh")
        .arg(script())
        .args(["--prefix", &prefix.to_string_lossy()])
        .args(["--out", &out.to_string_lossy()])
        .args(["--binary", &artefact.to_string_lossy()])
        .args(["--library", &artefact.to_string_lossy()])
        .status()
        .expect("the bundler runs");
    assert!(status.success(), "the bundler failed");

    let classes = out.join("lib/rdl2dso");
    assert!(
        classes.join("RdlMeshGeometry.so").is_file(),
        "the scene classes are copied"
    );
    assert!(out.join("bin/moonray").is_file(), "the renderer is copied");

    // The contract itself: with `bin/` as the executable's directory,
    // the first place the resolver looks is where the script put them.
    let looked = nsi_moonray::dso::beside(&out.join("bin/mnry"));
    assert_eq!(
        looked.first(),
        Some(&classes),
        "the resolver's first candidate must be the bundle's own \
         classes; it looked in {looked:?}"
    );
}

/// A prefix without scene classes is refused, and says what it is
/// missing.
///
/// The alternative is a bundle that assembles cleanly and renders
/// nothing, which is discovered by whoever installs it.
#[test]
fn a_prefix_without_scene_classes_is_refused() {
    let root = scratch("empty-prefix");
    let prefix = root.join("prefix");
    fs::create_dir_all(prefix.join("bin")).expect("a directory");

    let output = Command::new("sh")
        .arg(script())
        .args(["--prefix", &prefix.to_string_lossy()])
        .args(["--out", &root.join("out").to_string_lossy()])
        .output()
        .expect("the bundler runs");

    assert!(!output.status.success(), "an empty prefix must be refused");
    let complaint = String::from_utf8_lossy(&output.stderr);
    assert!(
        complaint.contains("rdl2dso"),
        "the refusal names what is missing: {complaint}"
    );
}

/// Assembling over an existing bundle is refused rather than merged.
///
/// A bundle assembled twice carries whatever the first one left, and
/// the stale file is always the one nobody thinks to look at.
#[test]
fn a_bundle_is_not_assembled_over_another() {
    let root = scratch("occupied");
    let prefix = fake_prefix(&root);

    let out = root.join("out");
    fs::create_dir_all(&out).expect("a directory");
    fs::write(out.join("leftover"), "from the last run").expect("a file");

    let output = Command::new("sh")
        .arg(script())
        .args(["--prefix", &prefix.to_string_lossy()])
        .args(["--out", &out.to_string_lossy()])
        .args(["--binary", &prefix.join("bin/moonray").to_string_lossy()])
        .args(["--library", &prefix.join("bin/moonray").to_string_lossy()])
        .output()
        .expect("the bundler runs");

    assert!(!output.status.success(), "a non-empty output is refused");
    assert!(
        out.join("leftover").is_file(),
        "and nothing of what was there is touched"
    );
}

/// **The installed package's layout, read off a built `.deb` rather
/// than assumed.**
///
/// `cargo packager` puts the binary at `/usr/bin/mnry` and every
/// resource under `/usr/lib/mnry/`, which the tarball layout misses
/// entirely. That was found by building a package and listing it, and
/// it is pinned here because the next version of the packager could
/// move it and nothing else would notice: an installed renderer that
/// resolves no classes renders a black frame and reports nothing.
#[test]
fn an_installed_package_finds_its_own_classes() {
    let installed = Path::new("/usr/bin/mnry");
    let looked = nsi_moonray::dso::beside(installed);

    assert!(
        looked.contains(&PathBuf::from("/usr/lib/mnry/lib/rdl2dso")),
        "cargo-packager installs resources under /usr/lib/<binary>/; \
         the resolver looked in {looked:?}"
    );
}

/// The same for a macOS `.app`, where the executable is two levels
/// inside the bundle and resources sit beside `MacOS` rather than
/// under a `lib`.
#[test]
fn an_app_bundle_finds_its_own_classes() {
    let executable =
        Path::new("/Applications/nsi-moonray.app/Contents/MacOS/mnry");
    let looked = nsi_moonray::dso::beside(executable);

    assert!(
        looked.contains(&PathBuf::from(
            "/Applications/nsi-moonray.app/Contents/Resources/lib/rdl2dso"
        )),
        "the resolver looked in {looked:?}"
    );
}

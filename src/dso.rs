//! Where MoonRay's scene classes are, without being told.
//!
//! Every object in a scene is a class loaded from MoonRay's `rdl2dso`
//! directory. Without it rdl2 resolves nothing, and the failure is the
//! quiet kind this backend keeps running into: no class means no
//! object, no object means an empty `Layer`, and an empty `Layer`
//! renders a black frame with no error anywhere.
//!
//! So the directory has to be found rather than demanded.
//! `--dso-path` and `$NSI_MOONRAY_DSO` stay, and stay first -- a build
//! from source, a second MoonRay, a bisect all need to win over
//! whatever is installed -- but an ordinary install is found without
//! either.
//!
//! # The order, and why it is that order
//!
//! 1. **What the caller said.** `--dso-path`, then `$NSI_MOONRAY_DSO`,
//!    then `$MOONRAY_ROOT/rdl2dso`. An explicit answer is never
//!    second-guessed, and a wrong one is reported rather than silently
//!    replaced by a working install -- "it renders, but not with the
//!    build I pointed at" is a worse afternoon than "it did not
//!    render".
//! 2. **Beside the executable.** `../lib/rdl2dso` and `./rdl2dso`
//!    relative to the running binary. This is what makes a bundle work
//!    from wherever it was unpacked, with no environment at all, and
//!    it comes before the system locations so a portable copy is not
//!    quietly shadowed by an older system-wide one.
//! 3. **The platform's own places**, per-user before system-wide,
//!    because a user who installed their own MoonRay meant it.
//!
//! # Not a fallback to "nothing"
//!
//! [`find`] returning `None` is a scene with no classes, which is not
//! a state worth rendering from. The caller reports it with
//! [`searched`], which lists every directory that was tried -- a path
//! that is *almost* right is the common case, and it is invisible
//! unless the list is printed.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// The directory name MoonRay installs its scene classes into.
///
/// rdl2's own, not this backend's: `moonray`'s `-dso_path` and
/// `RDL2_DSO_PATH` both name a directory of `.so` files that were
/// built by `rdl2_dso_...` and are looked up by class name.
const RDL2DSO: &str = "rdl2dso";

/// The first candidate that is a directory.
///
/// `explicit` is `--dso-path`, and wins outright: see the module
/// documentation for why it is not merely first.
pub fn resolve(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return path.is_dir().then(|| path.to_path_buf());
    }
    find()
}

/// The first place MoonRay's classes actually are.
pub fn find() -> Option<PathBuf> {
    candidates().into_iter().find(|path| path.is_dir())
}

/// Every directory [`find`] would try, in order, whether or not it
/// exists.
///
/// For the report when nothing was found. A list of five plausible
/// paths is what turns "MoonRay is not installed" into "MoonRay is
/// installed one directory up from where this looked".
pub fn searched() -> Vec<PathBuf> {
    candidates()
}

/// The candidates, from the real environment.
fn candidates() -> Vec<PathBuf> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));

    candidates_from(|name: &str| std::env::var_os(name), exe_dir.as_deref())
}

/// The candidates, from an environment given rather than read.
///
/// Split out so the order can be tested. `std::env::set_var` is unsafe
/// in this edition and races every other test in the process, which is
/// exactly the shape of thing that makes a suite flaky under one
/// runner and fine under another.
fn candidates_from<F>(var: F, exe_dir: Option<&Path>) -> Vec<PathBuf>
where
    F: Fn(&str) -> Option<OsString>,
{
    let mut paths = Vec::new();

    // 1. What the caller said.
    if let Some(dir) = var("NSI_MOONRAY_DSO") {
        paths.push(PathBuf::from(dir));
    }
    if let Some(root) = var("MOONRAY_ROOT") {
        paths.push(PathBuf::from(root).join(RDL2DSO));
    }

    // 2. Beside the executable, so a bundle needs no environment.
    //    `../lib/rdl2dso` is the installed layout -- `bin/mnry` beside
    //    `lib/rdl2dso` -- and `./rdl2dso` is the flat one a zip gets
    //    unpacked into.
    if let Some(dir) = exe_dir {
        if let Some(prefix) = dir.parent() {
            paths.push(prefix.join("lib").join(RDL2DSO));
        }
        paths.push(dir.join(RDL2DSO));
    }

    // 3. The platform's own places.
    paths.extend(platform(&var));

    paths
}

/// Per-user first, then system-wide.
#[cfg(target_os = "macos")]
fn platform<F>(var: &F) -> Vec<PathBuf>
where
    F: Fn(&str) -> Option<OsString>,
{
    let mut paths = Vec::new();
    if let Some(home) = var("HOME") {
        paths.push(
            PathBuf::from(home)
                .join("Library/Application Support/MoonRay")
                .join(RDL2DSO),
        );
    }
    paths.push(PathBuf::from(
        "/Library/Application Support/MoonRay/rdl2dso",
    ));
    paths.push(PathBuf::from("/opt/moonray").join(RDL2DSO));
    paths
}

/// Per-user first, then system-wide.
#[cfg(target_os = "windows")]
fn platform<F>(var: &F) -> Vec<PathBuf>
where
    F: Fn(&str) -> Option<OsString>,
{
    let mut paths = Vec::new();
    for name in ["LOCALAPPDATA", "APPDATA", "ProgramFiles"] {
        if let Some(base) = var(name) {
            paths.push(PathBuf::from(base).join("MoonRay").join(RDL2DSO));
        }
    }
    paths
}

/// Per-user first, then system-wide.
///
/// XDG for the user directory, because that is where a desktop Linux
/// puts application data and `$XDG_DATA_HOME` is the documented way to
/// move it. The system entries are the three prefixes a package,
/// a `make install` and a vendor tarball respectively land in.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform<F>(var: &F) -> Vec<PathBuf>
where
    F: Fn(&str) -> Option<OsString>,
{
    let mut paths = Vec::new();

    if let Some(data) = var("XDG_DATA_HOME") {
        paths.push(PathBuf::from(data).join("moonray").join(RDL2DSO));
    } else if let Some(home) = var("HOME") {
        paths.push(
            PathBuf::from(home)
                .join(".local/share/moonray")
                .join(RDL2DSO),
        );
    }

    for prefix in [
        "/usr/local/share/moonray",
        "/usr/share/moonray",
        "/opt/moonray",
    ] {
        paths.push(PathBuf::from(prefix).join(RDL2DSO));
    }

    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An environment built from pairs, so a test says exactly what is
    /// set and nothing else is.
    fn env<'a>(
        pairs: &'a [(&'a str, &'a str)],
    ) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(*value))
        }
    }

    /// **What the caller said comes first, and in that order.**
    ///
    /// A build from source has to beat an install, or a bisect renders
    /// with the wrong renderer and says nothing.
    #[test]
    fn the_environment_beats_every_installed_location() {
        let paths = candidates_from(
            env(&[
                ("NSI_MOONRAY_DSO", "/scratch/dso"),
                ("MOONRAY_ROOT", "/scratch/install"),
                ("HOME", "/home/someone"),
            ]),
            Some(Path::new("/opt/bundle/bin")),
        );

        assert_eq!(paths[0], PathBuf::from("/scratch/dso"));
        assert_eq!(paths[1], PathBuf::from("/scratch/install/rdl2dso"));
    }

    /// **A bundle finds itself.**
    ///
    /// `bin/mnry` beside `lib/rdl2dso` is the installed layout, and it
    /// has to work with no environment at all -- that is the whole
    /// point of shipping one.
    #[test]
    fn a_bundle_is_found_beside_its_own_binary() {
        let paths =
            candidates_from(env(&[]), Some(Path::new("/opt/moonray-1.0/bin")));

        assert_eq!(paths[0], PathBuf::from("/opt/moonray-1.0/lib/rdl2dso"));
        assert_eq!(paths[1], PathBuf::from("/opt/moonray-1.0/bin/rdl2dso"));
    }

    /// The bundle comes before anything system-wide, so a portable
    /// copy is not shadowed by an older install.
    #[test]
    fn the_bundle_beats_the_system() {
        let paths = candidates_from(
            env(&[("HOME", "/home/someone")]),
            Some(Path::new("/media/stick/moonray/bin")),
        );

        let bundle = paths
            .iter()
            .position(|path| path.starts_with("/media/stick"))
            .expect("the bundle is a candidate");
        let system = paths
            .iter()
            .position(|path| path.starts_with("/usr"))
            .expect("a system location is a candidate");

        assert!(bundle < system, "{paths:?}");
    }

    /// Nothing set and nowhere to look is an empty answer rather than
    /// a guess: `find` reports, and `searched` is what it reports.
    #[test]
    fn every_candidate_is_absolute() {
        let paths = candidates_from(
            env(&[("HOME", "/home/someone"), ("LOCALAPPDATA", "C:\\Users\\x")]),
            None,
        );

        assert!(!paths.is_empty());
        for path in &paths {
            assert!(
                path.file_name() == Some(std::ffi::OsStr::new(RDL2DSO)),
                "every candidate names the dso directory: {path:?}"
            );
        }
    }

    /// An explicit path that is not there is *not* replaced by a
    /// working install. Rendering with a renderer the caller did not
    /// ask for is worse than not rendering.
    #[test]
    fn an_explicit_path_that_is_missing_resolves_to_nothing() {
        assert_eq!(resolve(Some(Path::new("/nonexistent/rdl2dso"))), None);
    }
}

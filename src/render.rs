//! Handing a scene to MoonRay.
//!
//! MoonRay ships its own renderer binary, and it already takes the
//! format this crate writes: `moonray -in scene.rdla -out image.exr`.
//! The flags are read from
//! `moonray/lib/rendering/rndr/RenderOptions.cc`, where `-in` collects
//! scene files and `-out` overrides `SceneVariables`' output file.
//!
//! So *building* MoonRay is heavy and *running* it is not: whoever
//! renders needs the binary installed, and nothing in this crate needs
//! it to produce a scene. That split is deliberate -- it is also what
//! lets the emitter be tested with no renderer anywhere.
//!
//! Linking `libmoonray` instead of spawning its CLI is the other half
//! of this, and the half progressive rendering needs, since a spawned
//! batch render cannot stream samples back. That is `TN.1`.

use std::{
    env,
    ffi::OsString,
    fmt, io,
    path::{Path, PathBuf},
    process::Command,
};

/// The renderer binary's name.
const MOONRAY: &str = "moonray";

/// Where to look for it, beyond `PATH`.
const ROOT_VARIABLES: [&str; 2] = ["MOONRAY_ROOT", "REZ_MOONRAY_ROOT"];

/// What to render, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Render {
    /// The `.rdla` (or `.rdlb`) scene.
    pub scene: PathBuf,
    /// Where the image goes. `None` leaves the scene's own
    /// `SceneVariables` output file alone.
    pub image: Option<PathBuf>,
    /// Render threads. `None` lets MoonRay decide.
    pub threads: Option<usize>,
    /// Extra `RDL2_DSO_PATH` entries, for a MoonRay whose DSOs are not
    /// where its binary expects them.
    pub dso_path: Option<PathBuf>,
    /// Anything else, passed through untouched.
    pub extra: Vec<OsString>,
}

impl Render {
    pub fn new(scene: impl Into<PathBuf>) -> Self {
        Self {
            scene: scene.into(),
            image: None,
            threads: None,
            dso_path: None,
            extra: Vec::new(),
        }
    }

    /// The command that would be run, without running it.
    ///
    /// Separated so the argument list can be tested on a machine with
    /// no MoonRay on it, which is every machine this crate is developed
    /// on so far.
    pub fn command(&self, binary: &Path) -> Command {
        let mut command = Command::new(binary);
        command.arg("-in").arg(&self.scene);

        if let Some(image) = &self.image {
            command.arg("-out").arg(image);
        }

        if let Some(threads) = self.threads {
            command.arg("-threads").arg(threads.to_string());
        }

        if let Some(dso_path) = &self.dso_path {
            command.arg("-dso_path").arg(dso_path);
        }

        command.args(&self.extra);
        command
    }

    /// Run it, and wait.
    pub fn run(&self) -> Result<(), Error> {
        let binary = binary()?;
        let status = self
            .command(&binary)
            .status()
            .map_err(|error| Error::Spawn { binary, error })?;

        if status.success() {
            return Ok(());
        }

        Err(Error::Failed(status.code()))
    }
}

/// Find MoonRay's renderer binary.
///
/// `$MOONRAY` names it outright; `$MOONRAY_ROOT` and `$REZ_MOONRAY_ROOT`
/// name an install whose `bin/` holds it; otherwise it has to be on
/// `PATH`.
pub fn binary() -> Result<PathBuf, Error> {
    if let Some(path) = env::var_os("MOONRAY") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(Error::NotFound(Some(path)));
    }

    for variable in ROOT_VARIABLES {
        if let Some(root) = env::var_os(variable) {
            let path = Path::new(&root).join("bin").join(MOONRAY);
            if path.is_file() {
                return Ok(path);
            }
        }
    }

    if let Some(paths) = env::var_os("PATH") {
        for directory in env::split_paths(&paths) {
            let path = directory.join(MOONRAY);
            if path.is_file() {
                return Ok(path);
            }
        }
    }

    Err(Error::NotFound(None))
}

/// Why a render did not happen.
#[derive(Debug)]
pub enum Error {
    /// No `moonray` binary. The scene is still written -- ɴsɪ always
    /// returns an image, and a missing renderer is a deployment
    /// problem, not a reason to lose the scene.
    NotFound(Option<PathBuf>),
    Spawn {
        binary: PathBuf,
        error: io::Error,
    },
    /// MoonRay ran and exited non-zero.
    Failed(Option<i32>),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(Some(path)) => write!(
                f,
                "$MOONRAY points at {}, which is not a file",
                path.display()
            ),
            Self::NotFound(None) => f.write_str(
                "no `moonray` binary: set $MOONRAY to it, $MOONRAY_ROOT to \
                 an install, or put it on $PATH",
            ),
            Self::Spawn { binary, error } => {
                write!(f, "cannot run {}: {error}", binary.display())
            }
            Self::Failed(Some(code)) => write!(f, "moonray exited with {code}"),
            Self::Failed(None) => f.write_str("moonray was killed by a signal"),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(render: &Render) -> Vec<String> {
        render
            .command(Path::new("/opt/moonray/bin/moonray"))
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn a_scene_and_an_image() {
        let mut render = Render::new("/tmp/scene.rdla");
        render.image = Some(PathBuf::from("/tmp/image.exr"));

        assert_eq!(
            arguments(&render),
            ["-in", "/tmp/scene.rdla", "-out", "/tmp/image.exr"]
        );
    }

    /// Without an image, MoonRay writes wherever the scene's own
    /// `SceneVariables` say -- overriding that with a default would
    /// quietly ignore what the ɴsɪ output driver asked for.
    #[test]
    fn no_image_means_no_out_flag() {
        assert_eq!(
            arguments(&Render::new("/tmp/scene.rdla")),
            ["-in", "/tmp/scene.rdla"]
        );
    }

    #[test]
    fn threads_and_extra_arguments_come_through() {
        let mut render = Render::new("/tmp/scene.rdla");
        render.threads = Some(4);
        render.extra = vec![OsString::from("-info")];

        assert_eq!(
            arguments(&render),
            ["-in", "/tmp/scene.rdla", "-threads", "4", "-info"]
        );
    }
}

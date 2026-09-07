//! `mrr` — render a scene with MoonRay.
//!
//! ```text
//! mrr scene.rdla -o image.exr
//! mrr scene.rdla --print          # what would be run, and nothing else
//! ```
//!
//! ```text
//! mrr scene.nsi -o image.exr      # ɴsɪ in, flushed on the way past
//! ```
//!
//! Two kinds of input, told apart by looking rather than by extension:
//! an `.rdla` goes to MoonRay as it stands, and a `.nsi` stream is
//! parsed, recorded and flushed first. The parser is upstream's
//! (`nsi-parse`) and it drives `nsi_trait::Nsi`, which
//! `nsi_intermediate::Recorder` implements -- so there is nothing to
//! write here but the wiring.

use nsi_moonray::{
    flush::{Purpose, flush_for},
    render::{self, Render},
};
use std::{env, ffi::OsString, path::PathBuf, process::ExitCode};

const USAGE: &str = "\
usage: mrr <scene.nsi|scene.rdla> [-o <image.exr>] [-t <threads>]
           [--dso-path <dir>] [--print] [-- <moonray arguments>...]

  An ɴsɪ stream is flushed to `.rdla` on the way past; an `.rdla` goes
  to MoonRay as it stands. Which it is comes from the content, not the
  name.

  -o, --output     where the image goes; without it, the scene's own
                   output file stands
  -t, --threads    render threads; without it, MoonRay decides
      --dso-path   extra RDL2 DSO directory
      --print      print the MoonRay command and exit
";

fn main() -> ExitCode {
    let mut arguments = env::args_os().skip(1);
    let mut scene: Option<PathBuf> = None;
    let mut image: Option<PathBuf> = None;
    let mut threads: Option<usize> = None;
    let mut dso_path: Option<PathBuf> = None;
    let mut print = false;
    let mut extra: Vec<OsString> = Vec::new();

    while let Some(argument) = arguments.next() {
        match argument.to_string_lossy().as_ref() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--print" => print = true,
            "-o" | "--output" => match arguments.next() {
                Some(value) => image = Some(PathBuf::from(value)),
                None => return fail("-o wants a path"),
            },
            "-t" | "--threads" => match arguments.next() {
                Some(value) => match value.to_string_lossy().parse() {
                    Ok(count) => threads = Some(count),
                    Err(_) => return fail("-t wants a number"),
                },
                None => return fail("-t wants a number"),
            },
            "--dso-path" => match arguments.next() {
                Some(value) => dso_path = Some(PathBuf::from(value)),
                None => return fail("--dso-path wants a directory"),
            },
            // Everything after `--` is MoonRay's.
            "--" => extra.extend(arguments.by_ref()),
            other if other.starts_with('-') => {
                return fail(&format!("unknown option {other:?}"));
            }
            _ if scene.is_none() => scene = Some(PathBuf::from(argument)),
            other => return fail(&format!("unexpected argument {other:?}")),
        }
    }

    let Some(scene) = scene else {
        eprint!("{USAGE}");
        return ExitCode::FAILURE;
    };

    // ɴsɪ in, `.rdla` out, and only then a render.
    let scene = match prepared(&scene) {
        Ok(path) => path,
        Err(message) => return fail(&message),
    };

    let mut job = Render::new(scene);
    job.image = image;
    job.threads = threads;
    job.dso_path = dso_path;
    job.extra = extra;

    if print {
        let binary = match render::binary() {
            Ok(binary) => binary,
            // Printing the command is useful precisely when MoonRay is
            // not installed, so a missing binary is not fatal here.
            Err(_) => PathBuf::from("moonray"),
        };

        let command = job.command(&binary);
        let mut line = command.get_program().to_string_lossy().into_owned();
        for argument in command.get_args() {
            line.push(' ');
            line.push_str(&argument.to_string_lossy());
        }
        println!("{line}");
        return ExitCode::SUCCESS;
    }

    match job.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(&error.to_string()),
    }
}

/// The `.rdla` MoonRay will be given.
///
/// An `.rdla` is handed straight over. An ɴsɪ stream is parsed into a
/// `Scene`, flushed, and written beside itself -- `scene.nsi` becomes
/// `scene.rdla`, which is also what someone debugging the translation
/// wants to look at.
///
/// **Told apart by content, not by extension.** A file named `.nsi`
/// that is really `.rdla` is a thing that happens, and guessing from
/// the name would fail with a parse error about the wrong format.
fn prepared(scene: &PathBuf) -> Result<PathBuf, String> {
    let bytes = std::fs::read(scene)
        .map_err(|error| format!("reading {}: {error}", scene.display()))?;

    if !is_nsi(&bytes) {
        return Ok(scene.clone());
    }

    let recorder = nsi_intermediate::Recorder::new();
    nsi_parse::parse_compressed(&bytes, &recorder)
        .map_err(|error| format!("{}: {error}", scene.display()))?;

    // Rendered once, so geometry ɴsɪ hid is left out rather than
    // tessellated and never drawn.
    let flushed = flush_for(&recorder.into_scene(), Purpose::Batch);
    for limitation in &flushed.limitations {
        eprintln!("mrr: {limitation}");
    }

    let out = scene.with_extension("rdla");
    std::fs::write(&out, flushed.to_rdla())
        .map_err(|error| format!("writing {}: {error}", out.display()))?;

    Ok(out)
}

/// Whether these bytes are an ɴsɪ stream rather than an `.rdla`.
///
/// An `.rdla` is Lua, and every scene this crate or rdl2 writes opens
/// with a class name and a brace -- `SceneVariables {`,
/// `RdlMeshGeometry("x") {`. An ɴsɪ stream opens with one of its own
/// verbs, and a compressed one opens with a magic number. Those are
/// three disjoint shapes, so this needs no cleverness.
fn is_nsi(bytes: &[u8]) -> bool {
    // gzip, and 3Delight writes these.
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return true;
    }
    // A binary ɴsɪ stream, which `nsi-parse` refuses with a message
    // rather than misreading -- so it is still better sent there.
    if bytes.starts_with(&[0xCC, 0x00]) {
        return true;
    }

    // Otherwise the first word decides.
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("--"))
        .is_some_and(|line| {
            [
                "Create",
                "Delete",
                "SetAttribute",
                "SetAttributeAtTime",
                "DeleteAttribute",
                "Connect",
                "Disconnect",
                "Evaluate",
                "RenderControl",
            ]
            .iter()
            .any(|verb| line.starts_with(verb))
        })
}

fn fail(message: &str) -> ExitCode {
    eprintln!("mrr: {message}");
    ExitCode::FAILURE
}

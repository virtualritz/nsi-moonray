//! `mrr` — render a scene with MoonRay.
//!
//! ```text
//! mrr scene.rdla -o image.exr
//! mrr scene.rdla --print          # what would be run, and nothing else
//! ```
//!
//! Today the input is `.rdla`, which this crate writes and MoonRay's own
//! binary reads. Taking a `.nsi` stream directly needs a parser for that
//! format, and there is none: `nsi-intermediate` writes streams and does
//! not read them. That parser belongs upstream, next to the writer, so
//! both backends get it -- see `specs/001-moonray-backend/tasks.md`.

use nsi_moonray::render::{self, Render};
use std::{env, ffi::OsString, path::PathBuf, process::ExitCode};

const USAGE: &str = "\
usage: mrr <scene.rdla> [-o <image.exr>] [-t <threads>] [--dso-path <dir>]
           [--print] [-- <moonray arguments>...]

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

fn fail(message: &str) -> ExitCode {
    eprintln!("mrr: {message}");
    ExitCode::FAILURE
}

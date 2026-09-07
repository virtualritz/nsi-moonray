//! `mnry` — render ɴsɪ with MoonRay.
//!
//! ```text
//! mnry render scene.nsi                    # in process, if built with `rdl2`
//! mnry render 'shot.@4.nsi' -f 1-48        # a sequence
//! mnry cat scene.nsi -l                    # what MoonRay would be given
//! mnry watch /spool -r                     # render what lands there
//! ```
//!
//! Modelled on `rdl`, the `renderdl` replacement in
//! `virtualritz/delight-helpers`, so the two commands take the same
//! shape. What differs is what is underneath: `rdl` drives 3Delight
//! through the ɴsɪ C API, and this parses the same streams into
//! `nsi-intermediate` and builds MoonRay's scene from them directly.

use anyhow::{Result, anyhow};
use clap::CommandFactory;
use clap_complete::{
    generate,
    shells::{Bash, Elvish, Fish, PowerShell, Zsh},
};
use std::io;

mod cli;
mod frames;
mod render;
mod scene;
mod watch;

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("mnry: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let arguments = cli::parse();

    match &arguments.command {
        cli::Command::Render(args) => render::render(args, arguments.verbose),
        cli::Command::Cat(args) => cat(args),
        cli::Command::Watch(args) => watch::watch(args, arguments.verbose),
        cli::Command::GenerateCompletions { shell } => completions(shell),
    }
}

/// `mnry cat` — the `.rdla` MoonRay would be handed.
///
/// The most useful thing this command does is answer "what did my ɴsɪ
/// scene become?" without rendering it, which is the question every
/// translation bug starts as.
fn cat(args: &cli::Cat) -> Result<()> {
    let recorded = match scene::read(&args.file)? {
        scene::Input::Rdla => {
            return Err(anyhow!(
                "{} is already an .rdla; there is nothing to convert",
                args.file.display()
            ));
        }
        scene::Input::Nsi(recorded) => *recorded,
    };

    let flushed = scene::flush(&recorded, args.interactive);

    if args.limitations {
        for limitation in &flushed.limitations {
            eprintln!("mnry: {limitation}");
        }
    }

    match &args.output {
        Some(path) => std::fs::write(path, flushed.document.to_rdla())
            .map_err(|error| anyhow!("writing {}: {error}", path.display())),
        None => {
            use std::io::Write as _;
            io::stdout()
                .write_all(flushed.document.to_rdla().as_bytes())
                .map_err(Into::into)
        }
    }
}

fn completions(shell: &str) -> Result<()> {
    let mut command = cli::Cli::command();
    let mut out = io::stdout();

    match shell {
        "bash" => generate(Bash, &mut command, "mnry", &mut out),
        "elvish" => generate(Elvish, &mut command, "mnry", &mut out),
        "fish" => generate(Fish, &mut command, "mnry", &mut out),
        "powershell" => generate(PowerShell, &mut command, "mnry", &mut out),
        "zsh" => generate(Zsh, &mut command, "mnry", &mut out),
        other => return Err(anyhow!("unsupported shell {other:?}")),
    }

    Ok(())
}

//! `mnry render`.
//!
//! # Two paths, and the reason there are two
//!
//! With the `rdl2` feature this **links MoonRay and renders in this
//! process**: the scene is built into a live `SceneContext`, the frame
//! converges behind a `RenderContext`, and nothing is written to disk
//! on the way. That is the path this backend exists for -- a spawned
//! batch process has no scene to edit and no frame to snapshot, so it
//! forecloses interactive updates, progressive delivery and concurrent
//! rendering together.
//!
//! Without the feature, or for an `.rdla` input -- which only rdl2's
//! own reader can parse -- it falls back to writing the scene out and
//! running the `moonray` binary. That is a real fallback, not a
//! pretence: it renders the same image, later, and says which path it
//! took when asked.

use crate::{cli, frames, scene};
use anyhow::{Result, anyhow};
use std::path::{Path, PathBuf};

pub fn render(args: &cli::Render, verbose: u8) -> Result<()> {
    let files = frames::expand(&args.file, args.frames.as_deref())?;

    if files.is_empty() {
        return Err(anyhow!("nothing to render"));
    }

    let mut failed = 0usize;
    for file in &files {
        if args.dry_run {
            println!("{file}");
            continue;
        }

        if let Err(error) = one(Path::new(file), args, verbose) {
            eprintln!("mnry: {error:#}");
            failed += 1;
        }
    }

    if failed > 0 {
        return Err(anyhow!("{failed} of {} scene(s) failed", files.len()));
    }

    Ok(())
}

fn one(file: &Path, args: &cli::Render, verbose: u8) -> Result<()> {
    if verbose > 0 {
        eprintln!("mnry: rendering {}", file.display());
    }

    let mut recorded = match scene::read(file)? {
        scene::Input::Rdla => {
            // rdl2's `AsciiReader` is the only thing that parses this,
            // and it is not on this side of the shim.
            if verbose > 0 {
                eprintln!(
                    "mnry: {} is an .rdla, so MoonRay's own binary reads it",
                    file.display()
                );
            }
            return spawn(file.to_path_buf(), args);
        }
        scene::Input::Nsi(recorded) => *recorded,
    };

    if let Some(image) = &args.output {
        let redirected = scene::redirect_output(&mut recorded, image)?;
        if redirected > 1 {
            eprintln!(
                "mnry: {redirected} output drivers now write to {}; \
                 --output names one file and the scene wanted several",
                image.display()
            );
        }
    }

    in_process(recorded, file, args, verbose)
}

#[cfg(all(feature = "rdl2", moonray))]
fn in_process(
    recorded: nsi_intermediate::Scene,
    file: &Path,
    args: &cli::Render,
    verbose: u8,
) -> Result<()> {
    use nsi_moonray::session::Session;

    // `--dso-path` is an override, not the only way to be told. An
    // ordinary install is found where the platform puts one, and a
    // bundle is found beside this binary -- `nsi_moonray::dso` has the
    // order and the reasoning.
    let Some(dso) =
        nsi_moonray::dso::resolve(args.renderer.dso_path.as_deref())
    else {
        return Err(anyhow!(
            "MoonRay's rdl2dso directory was not found, and without it no \
             scene class resolves -- the render would come out empty \
             rather than fail. Name it with --dso-path or \
             $NSI_MOONRAY_DSO. Looked in:\n{}",
            nsi_moonray::dso::searched()
                .iter()
                .map(|path| format!("  {}", path.display()))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    };

    if verbose > 0 && args.renderer.dso_path.is_none() {
        eprintln!("mnry: scene classes from {}", dso.display());
    }

    if args.renderer.threads.is_some() && verbose > 0 {
        eprintln!(
            "mnry: --threads is not carried into an in-process render yet; \
             MoonRay uses every core"
        );
    }

    let started = std::time::Instant::now();
    let mut session = Session::new(recorded, &dso.to_string_lossy())
        .ok_or_else(|| {
            anyhow!("MoonRay refused {}; see the report above", file.display())
        })?;

    if args.progress {
        while !session.render().frame_complete() {
            std::thread::sleep(std::time::Duration::from_millis(200));
            eprint!(".");
        }
        eprintln!();
    }

    session.wait();

    if args.statistics > 0 {
        eprintln!("mnry: {:.3}s wall clock", started.elapsed().as_secs_f64());
    }
    if args.statistics > 1
        && let Some(cost) = session.last_cost()
    {
        eprintln!(
            "mnry: {:.3}s tessellating {} primitive(s), {:.3}s building \
             the accelerator, {:.3}s loading procedurals, {:.3}s \
             rebuilding geometry",
            cost.tessellation,
            cost.primitives_tessellated,
            cost.build_accelerator,
            cost.load_procedurals,
            cost.rebuild_geometry
        );
    }

    Ok(())
}

#[cfg(not(all(feature = "rdl2", moonray)))]
fn in_process(
    recorded: nsi_intermediate::Scene,
    file: &Path,
    args: &cli::Render,
    verbose: u8,
) -> Result<()> {
    // No linked renderer here, so the scene goes out as a file and
    // MoonRay's own binary reads it back. Said plainly rather than
    // implied: the two paths have different capabilities, and someone
    // wondering why their viewport does not update should be able to
    // find out which one they are on.
    if verbose > 0 {
        eprintln!(
            "mnry: built without the `rdl2` feature, so {} is written out \
             and the `moonray` binary renders it",
            file.display()
        );
    }

    let flushed = scene::flush(&recorded, false);
    for limitation in &flushed.limitations {
        eprintln!("mnry: {limitation}");
    }

    let out = file.with_extension("rdla");
    std::fs::write(&out, flushed.document.to_rdla())
        .map_err(|error| anyhow!("writing {}: {error}", out.display()))?;

    spawn(out, args)
}

/// Hand a scene file to the `moonray` binary.
fn spawn(scene_file: PathBuf, args: &cli::Render) -> Result<()> {
    let mut job = nsi_moonray::render::Render::new(scene_file);
    job.image.clone_from(&args.output);
    job.threads = args.renderer.threads;
    // Same resolution as the in-process path, so a spawned render and
    // a linked one load the same classes.
    job.dso_path = nsi_moonray::dso::resolve(args.renderer.dso_path.as_deref());

    job.run().map_err(|error| anyhow!("{error}"))
}

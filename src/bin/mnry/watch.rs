//! `mnry watch`.
//!
//! Watch folders and render what lands in them. The use is a farm
//! spool or a DCC's export directory: point this at it, leave it
//! running, and every scene written there renders.
//!
//! A file appears before it is finished being written, so a create
//! event is not a render cue. What is: the writer closing it. `notify`
//! reports that as an access-close-write event on Linux, and as a
//! modify elsewhere -- so a short settle is applied either way rather
//! than trusting one platform's event to mean the same as another's.

use crate::{cli, render};
use anyhow::{Context as _, Result, anyhow};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

/// How long a file has to sit unchanged before it is considered
/// written.
const SETTLE: Duration = Duration::from_millis(500);

pub fn watch(args: &cli::Watch, verbose: u8) -> Result<()> {
    if args.folder.is_empty() {
        return Err(anyhow!("no folder to watch"));
    }

    let (sender, receiver) = mpsc::channel();
    let mut watcher =
        notify::recommended_watcher(move |event: notify::Result<Event>| {
            if let Ok(event) = event {
                let _ = sender.send(event);
            }
        })
        .context("starting a file watcher")?;

    let mode = if args.recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };

    for folder in &args.folder {
        watcher
            .watch(folder, mode)
            .with_context(|| format!("watching {}", folder.display()))?;
        eprintln!("mnry: watching {}", folder.display());
    }

    // Paths seen but not yet settled, and when they were last touched.
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();

    loop {
        match receiver.recv_timeout(SETTLE) {
            Ok(event) => {
                if matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(_)
                ) {
                    for path in event.paths {
                        if interesting(&path) {
                            pending.insert(path, Instant::now());
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            // The watcher is gone, so there is nothing left to wait
            // for.
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }

        let now = Instant::now();
        let ready: Vec<PathBuf> = pending
            .iter()
            .filter(|(_, seen)| now.duration_since(**seen) >= SETTLE)
            .map(|(path, _)| path.clone())
            .collect();

        for path in ready {
            pending.remove(&path);

            let job = cli::Render {
                file: vec![path.to_string_lossy().into_owned()],
                renderer: args.renderer.clone(),
                frames: None,
                output: None,
                progress: false,
                statistics: 0,
                dry_run: false,
            };

            if let Err(error) = render::render(&job, verbose) {
                eprintln!("mnry: {error:#}");
            }
        }
    }
}

/// Whether a path is worth rendering.
///
/// By extension here, unlike everywhere else in this command, because
/// a watcher sees every temporary file a writer makes and opening each
/// one to look inside would be the expensive way to ignore it. A file
/// that passes this is still identified by content before it renders.
fn interesting(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(extension, "nsi" | "nsia" | "rdla" | "gz")
        })
}

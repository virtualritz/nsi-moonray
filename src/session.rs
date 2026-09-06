//! An interactive render: a scene you keep editing, and a renderer that
//! keeps what it already built.
//!
//! This is the loop an application's viewport runs. A slider moves, one
//! ɴsɪ attribute changes, [`Session::synchronize`] re-sends only what
//! depended on it, and MoonRay reuses its tessellation and its
//! acceleration structures for everything else. Reuse is the entire
//! point: a viewport that rebuilt the scene per edit would be a batch
//! renderer with extra steps.
//!
//! It exists as a Rust type rather than living inside the C entry
//! points because the logic is worth testing directly, and because an
//! application embedding this crate as a library wants it without going
//! through `dlopen` and `NSIParam_t`. [`crate::capi`] is a thin shim
//! over this.
//!
//! # One at a time
//!
//! MoonRay's driver state is global, so only one [`Session`] can exist
//! per process (`002` `research.md` F4). Drop one before making the
//! next.

use crate::{
    apply::{apply, apply_affected},
    display::Callbacks,
    document::Document,
    flush::{Flushed, flush},
    rdl2::{Mode, Render},
    stream::{Stopped, stream},
};
use nsi_intermediate::Scene;

/// A live render over a scene that can still be edited.
pub struct Session {
    scene: Scene,
    render: Render,
    /// The document last applied to the renderer.
    ///
    /// What MoonRay is holding, so a synchronise can send only the
    /// attributes that actually differ. Without it the narrow path is
    /// narrow in *objects* only, and re-sending a mesh's
    /// `vertex_list_0` regenerates its geometry whether or not the
    /// vertices moved.
    applied: Document,
    /// What the last completed frame cost.
    ///
    /// Captured **before** the frame is stopped, because
    /// `RenderContext::stopFrame` calls `RenderStats::reset()`
    /// (`RenderContext.cc:1208`) and zeroes every timer. Reading the
    /// counters after a render is therefore guaranteed to read zeros,
    /// which is what made them look like they were never wired up at
    /// all (`002` `research.md` F8).
    last_cost: Option<crate::rdl2::Cost>,
}

impl Session {
    /// Build the scene into a renderer and start the first frame.
    ///
    /// Returns as soon as render prep is done -- the frame converges
    /// behind it, which is what makes this a viewport rather than a
    /// batch.
    ///
    /// `None` when there is no renderer to be had: `dso_path` not
    /// naming MoonRay's `rdl2dso`, a session already alive in this
    /// process, or a scene MoonRay's render prep refuses. Each is
    /// reported on the way out.
    pub fn new(scene: Scene, dso_path: &str) -> Option<Self> {
        let render = Render::new(Some(dso_path), None, Mode::Progressive)?;
        let mut session = Self {
            scene,
            render,
            applied: Document::default(),
            last_cost: None,
        };

        let flushed = flush(&session.scene);
        session.report(&flushed);

        let live = session.render.scene()?;
        for line in apply(&flushed.document, &live) {
            eprintln!("nsi-moonray: {line}");
        }
        session.applied = flushed.document.clone();

        if let Err(error) = session.render.initialize() {
            eprintln!(
                "nsi-moonray: render prep failed ({})",
                session.render.error().unwrap_or_else(|| error.to_string())
            );
            return None;
        }
        if let Err(error) = session.render.start() {
            eprintln!("nsi-moonray: the frame did not start: {error}");
            return None;
        }

        // Everything up to here *built* the scene rather than editing
        // it. Leaving it in the journal would make the first
        // synchronise re-apply the whole thing -- correct, and exactly
        // the rebuild this exists to avoid.
        let _ = session.scene.take_changes();

        Some(session)
    }

    /// The scene, to edit.
    ///
    /// Edits are recorded in `nsi-intermediate`'s journal and reach the
    /// renderer at the next [`Session::synchronize`], not before: the
    /// frame has to be stopped before the scene it is rendering can be
    /// touched.
    pub fn scene_mut(&mut self) -> &mut Scene {
        &mut self.scene
    }

    /// The scene, to read.
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// End the session and take the scene back.
    ///
    /// Dropping the [`Render`] with it is what frees MoonRay's global
    /// driver state for the next session; it allows only one at a
    /// time.
    pub fn into_scene(self) -> Scene {
        self.scene
    }

    /// The renderer, for snapshots and progress.
    pub fn render(&self) -> &Render {
        &self.render
    }

    /// What the last completed frame cost.
    ///
    /// The only way to tell an incremental update from a rebuild: a
    /// synchronise that re-tessellates everything renders exactly the
    /// right image, slightly later, and no pixel test can see the
    /// difference.
    ///
    /// `None` before the first [`Session::wait`]. The counters must be
    /// read before the frame stops -- `stopFrame` resets them -- which
    /// is why this is recorded here rather than left to the caller.
    pub fn last_cost(&self) -> Option<crate::rdl2::Cost> {
        self.last_cost
    }

    /// Read the cost, then stop the frame. Order matters.
    fn stop_and_record(&mut self) {
        self.last_cost = self.render.cost().ok();
        let _ = self.render.stop();
    }

    /// Apply the edits made since the last call, and re-render.
    ///
    /// Returns whether the edit forced a whole-scene re-apply. That is
    /// worth acting on rather than ignoring: a rebuild renders
    /// correctly and slowly, which is invisible in the image and shows
    /// up only as time.
    pub fn synchronize(&mut self) -> bool {
        // MoonRay asserts on a scene changed mid-render, so the frame
        // comes down first.
        let _ = self.render.stop();

        let changes = self.scene.take_changes();
        let affected = self.scene.affected(&changes);
        let flushed = flush(&self.scene);
        self.report(&flushed);

        let Some(live) = self.render.scene() else {
            eprintln!("nsi-moonray: the renderer has no scene context");
            return false;
        };

        let (report, rebuilt) = apply_affected(
            &flushed.document,
            Some(&self.applied),
            &live,
            &changes,
            &affected,
        );
        for line in report {
            eprintln!("nsi-moonray: {line}");
        }
        self.applied = flushed.document.clone();

        let _ = self.render.scene_updated();
        if let Err(error) = self.render.start() {
            eprintln!("nsi-moonray: the frame did not restart: {error}");
        }

        rebuilt
    }

    /// Block until the frame is done, delivering it to the driver's
    /// callbacks as it converges.
    ///
    /// A scene whose output driver carries no callbacks is a batch
    /// render: it converges and then writes the files the scene names,
    /// through MoonRay's own output machinery.
    pub fn wait(&mut self) -> Option<Stopped> {
        let driver = self
            .scene
            .nodes()
            .filter(|(_, node)| node.node_type == "outputdriver")
            .find_map(|(handle, _)| {
                Some((handle.clone(), Callbacks::of(&self.scene, handle)?))
            });

        match driver {
            Some((handle, callbacks)) => {
                match stream(&self.render, &callbacks, &handle, None) {
                    Ok(stopped) => Some(stopped),
                    Err(error) => {
                        eprintln!("nsi-moonray: {handle:?} streaming: {error}");
                        None
                    }
                }
            }
            // No callbacks: a batch render, whose outputs are files.
            None => {
                while !self.render.frame_complete() {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                self.stop_and_record();
                if let Err(error) = self.render.write() {
                    eprintln!(
                        "nsi-moonray: the image was not written ({}): {error}",
                        self.render
                            .error()
                            .unwrap_or_else(|| "no detail".to_string())
                    );
                }
                Some(Stopped::Complete)
            }
        }
    }

    /// Report a flush's limitations, and dump the scene if asked.
    fn report(&self, flushed: &Flushed) {
        for limitation in &flushed.limitations {
            eprintln!("nsi-moonray: {limitation}");
        }

        // `.rdla` is a dump now, not the transport -- but a dump you can
        // ask for. It has to be the scene that actually rendered, or it
        // answers the wrong question.
        if let Some(path) = std::env::var_os("NSI_MOONRAY_SCENE")
            && let Err(error) = std::fs::write(&path, flushed.to_rdla())
        {
            eprintln!(
                "nsi-moonray: cannot write the scene dump {}: {error}",
                std::path::Path::new(&path).display()
            );
        }
    }
}

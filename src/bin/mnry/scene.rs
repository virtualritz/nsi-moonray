//! Getting from a file on disk to something MoonRay can render.
//!
//! Two kinds of input, **told apart by content rather than by
//! extension**: an `.rdla` goes to MoonRay as it stands, and an ɴsɪ
//! stream is parsed, recorded and flushed first. A file named `.nsi`
//! that is really an `.rdla` is a thing that happens, and guessing from
//! the name fails with a parse error about the wrong format.

use anyhow::{Context as _, Result};
use nsi_intermediate::{OwnedArgument, OwnedData, Recorder, Scene};
use nsi_moonray::flush::{Flushed, Purpose, flush_for};
use nsi_trait::Type;
use std::path::Path;

/// What a scene file turned out to be.
pub enum Input {
    /// An ɴsɪ stream, recorded and ready to flush or to render.
    Nsi(Box<Scene>),
    /// An `.rdla`, which only MoonRay's own reader can take.
    Rdla,
}

/// Read a scene file and, if it is ɴsɪ, record it.
pub fn read(path: &Path) -> Result<Input> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("reading {}", path.display()))?;

    if !is_nsi(&bytes) {
        return Ok(Input::Rdla);
    }

    let recorder = Recorder::new();
    nsi_parse::parse_compressed(&bytes, &recorder)
        .map_err(|error| anyhow::anyhow!("{}: {error}", path.display()))?;

    Ok(Input::Nsi(Box::new(recorder.into_scene())))
}

/// Point every ɴsɪ output driver at one file.
///
/// Done on the **ɴsɪ scene** rather than on the flushed document,
/// because that is where it means something: an output driver's
/// `imagefilename` is the ɴsɪ attribute a host would have set, and
/// changing it here keeps the in-process and spawned paths saying the
/// same thing.
///
/// Returns how many drivers were redirected; more than one is worth
/// reporting, since they all now write to the same place.
pub fn redirect_output(scene: &mut Scene, image: &Path) -> Result<usize> {
    let drivers: Vec<String> = scene
        .nodes()
        .filter(|(_, node)| node.node_type() == "outputdriver")
        .map(|(handle, _)| handle.to_owned())
        .collect();

    let name = image.to_string_lossy().into_owned();
    for driver in &drivers {
        scene
            .set_attribute(
                driver,
                vec![OwnedArgument::new(
                    "imagefilename",
                    Type::String,
                    1,
                    0,
                    OwnedData::String(vec![name.clone().into_bytes()]),
                )],
            )
            .map_err(|error| {
                anyhow::anyhow!("redirecting {driver:?}: {error}")
            })?;
    }

    Ok(drivers.len())
}

/// Flush a recorded scene, and say what did not survive the trip.
pub fn flush(scene: &Scene, interactive: bool) -> Flushed {
    flush_for(
        scene,
        if interactive {
            Purpose::Interactive
        } else {
            Purpose::Batch
        },
    )
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

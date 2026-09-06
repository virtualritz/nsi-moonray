//! An [ɴsɪ](https://nsi.readthedocs.io/) backend on
//! [MoonRay](https://github.com/OpenMoonRay/moonray), DreamWorks
//! Animation's production renderer.
//!
//! This crate owns **only the flush**. Recording an ɴsɪ scene,
//! classifying its connections and resolving ɴsɪ's graph semantics all
//! happen upstream in [`nsi-intermediate`], and are shared with the
//! Mitsuba backend.
//!
//! # What it emits
//!
//! `.rdla`, MoonRay's ASCII scene format. The alternative — an
//! `extern "C"` shim over `scene_rdl2` — is where this has to end up
//! for MoonRay's progressive render modes, which `.rdla` cannot reach
//! because it is a batch authoring path. The reasoning behind starting
//! with `.rdla` is in `specs/001-moonray-backend/research.md`; the
//! short of it is that the format is small, fully captured by an
//! oracle, and checkable end to end today, whereas the shim buys
//! nothing until a machine can build the renderer.
//!
//! Everything about the format was read out of scenes written by rdl2's
//! own `AsciiWriter`, never inferred. See
//! `specs/001-moonray-backend/oracle/`.
//!
//! # Layers
//!
//! - [`value`] — how rdl2 prints each attribute type.
//! - [`document`] — the file: objects, sets and the `Layer` table.
//!
//! - [`flush`] — turning a [`nsi_intermediate::Scene`] into one of those
//!   documents.
//! - [`render`] — handing the result to MoonRay's own renderer binary,
//!   which is what the `mrr` command does.
//! - [`display`] — pixels back out, through ɴsɪ's display-driver ABI.
//!   MoonRay has none, so this calls the driver rather than being
//!   called by one.
//! - [`capi`] — the ɴsɪ C entry points, so this builds as a `cdylib`
//!   that an existing ɴsɪ consumer can load in place of 3Delight.
//!
//! # Building it
//!
//! `nsi-intermediate` is overlaid from a sibling checkout: clone
//! <https://github.com/virtualritz/nsi> next to this repository. It is
//! unpublished, and a git dependency on that workspace makes Cargo
//! fetch its private `.blueprints` submodule. See `Cargo.toml`.
//!
//! [`nsi-intermediate`]: https://github.com/virtualritz/nsi

// Applying a `Document` to a live scene, rather than writing it out.
// Behind the `rdl2` feature: it needs `scene_rdl2` installed, while
// the emitter, the oracle and the flush are all checked without a
// renderer and stay that way.
#[cfg(feature = "rdl2")]
pub mod apply;
pub mod capi;
pub mod display;
pub mod document;
pub mod flush;
#[cfg(feature = "rdl2")]
pub mod rdl2;
pub mod render;
pub mod value;

pub use document::{Assignment, Body, Document, Object};
pub use flush::{Flushed, flush};
pub use value::{Reference, Value};

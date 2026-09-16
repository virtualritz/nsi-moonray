//! Matte, plastic, glass, metal and emissive, through both renderers.
//!
//! **One scene, two renderers, two images.** The scene is built once
//! through `nsi::Context` and rendered twice, differing only in the
//! `"renderer"` argument. Every sphere wears the same 3Delight shader,
//! `dlPrincipled`, at five parameter sets -- so what the two images
//! disagree about is the renderer, not the description.
//!
//! That is the whole point of a shared interface, and it is also the
//! only honest way to compare: a scene written twice is two scenes.
//!
//! # Reading the result
//!
//! The emissive sphere is the interesting one. ɴsɪ has no light nodes,
//! so an emitter is geometry wearing a shader that emits, and what the
//! two renderers then do with it differs:
//!
//! - **3Delight** samples emissive geometry directly, so it lights the
//!   row and appears in the metal sphere's reflection.
//! - **MoonRay** has no next-event estimation toward emissive geometry.
//!   `dlPrincipled` both emits *and* shades, so this backend keeps it a
//!   surface rather than turning it into a `MeshLight` -- a shader that
//!   does both has no faithful mapping, and a metal object rendering as
//!   a featureless emitter is the more visibly wrong of the two. Its
//!   emission is then hit-only: seen where a ray lands on it, lighting
//!   little, and noisy.
//!
//! Needs `$DELIGHT` for the shaders and `$NSI_MOONRAY`/a linked backend
//! for the second render; skipped without either.

#![cfg(all(feature = "rdl2", moonray, feature = "backend-registry"))]

use nsi_ffi_wrap as nsi;
use std::{num::NonZeroUsize, sync::Arc};

const PAIR: NonZeroUsize = match NonZeroUsize::new(2) {
    Some(two) => two,
    None => unreachable!(),
};

/// A UV sphere, built through the interface.
fn sphere(context: &nsi::Context, handle: &str, centre: [f32; 3], radius: f32) {
    let (segments, rings) = (64usize, 32usize);
    let mut positions: Vec<[f32; 3]> = Vec::new();
    for ring in 0..=rings {
        let theta = std::f32::consts::PI * ring as f32 / rings as f32;
        for segment in 0..segments {
            let phi = std::f32::consts::TAU * segment as f32 / segments as f32;
            positions.push([
                centre[0] + radius * theta.sin() * phi.cos(),
                centre[1] + radius * theta.cos(),
                centre[2] + radius * theta.sin() * phi.sin(),
            ]);
        }
    }

    let at = |ring: usize, segment: usize| -> i32 {
        (ring * segments + segment % segments) as i32
    };
    let mut counts = Vec::new();
    let mut indices = Vec::new();
    for ring in 0..rings {
        for segment in 0..segments {
            counts.push(4);
            indices.extend_from_slice(&[
                at(ring, segment),
                at(ring, segment + 1),
                at(ring + 1, segment + 1),
                at(ring + 1, segment),
            ]);
        }
    }

    context.create(handle, nsi::node::MESH, None);
    context.set_attribute(
        handle,
        &[
            nsi::i32_slice!("nvertices", &counts),
            nsi::i32_slice!("P.indices", &indices),
            nsi::point_slice!("P", &positions),
        ],
    );
    context.connect(handle, None, nsi::ROOT, "objects", None);
}

/// Bind `dlPrincipled` with one look's parameters.
fn look<'a>(
    context: &nsi::Context<'a>,
    geometry: &str,
    shader_file: &str,
    parameters: &nsi::ArgSlice<'_, 'a>,
) {
    let shader = format!("{geometry}_shader");
    let attributes = format!("{geometry}_attributes");

    context.create(&shader, nsi::node::SHADER, None);
    let mut all: nsi::ArgVec<'_, 'a> =
        vec![nsi::string!("shaderfilename", shader_file)];
    all.extend(parameters.iter().cloned());
    context.set_attribute(&shader, &all);

    context.create(&attributes, nsi::node::ATTRIBUTES, None);
    context.connect(&attributes, None, geometry, "geometryattributes", None);
    context.connect(&shader, None, &attributes, "surfaceshader", None);
}

/// Build the row. Identical for both renderers, by construction.
///
/// `camera_height` and `camera_pitch_degrees` (positive tilts down)
/// are parameters rather than the fixed `1.1` and `0.0` they replace,
/// because **a level camera puts the horizon at the exact vertical
/// centre of frame no matter how high it sits** -- height alone never
/// hides the sky over an unbounded-looking floor; only pitch does, by
/// moving the frustum's top edge below the horizon's fixed 0-degree
/// elevation. The roughness sweep keeps the original framing, since
/// its disc coordinates for measurement are pixel positions computed
/// against it; the five-look row asks for a different one.
fn stage<'a>(
    context: &nsi::Context<'a>,
    principled: &'a str,
    environment: &'a str,
    image: &str,
    shading_samples: i32,
    stats: &str,
    camera_height: f64,
    camera_pitch_degrees: f64,
) {
    let (width, height) = (960i32, 320i32);

    let pitch = camera_pitch_degrees.to_radians();
    let (sin, cos) = (pitch.sin(), pitch.cos());
    context.create("camxf", nsi::node::TRANSFORM, None);
    context.set_attribute(
        "camxf",
        &[nsi::matrix_f64!(
            "transformationmatrix",
            &[
                1.0,
                0.0,
                0.0,
                0.0, //
                0.0,
                cos,
                -sin,
                0.0, //
                0.0,
                sin,
                cos,
                0.0, //
                0.0,
                camera_height,
                11.0,
                1.0f64,
            ]
        )],
    );
    context.connect("camxf", None, nsi::ROOT, "objects", None);

    context.create("cam", nsi::node::PERSPECTIVE_CAMERA, None);
    context.set_attribute("cam", &[nsi::f32!("fov", 40.0)]);
    context.connect("cam", None, "camxf", "objects", None);

    context.create("screen", nsi::node::SCREEN, None);
    context.set_attribute(
        "screen",
        &[nsi::i32_slice!("resolution", &[width, height]).array_len(PAIR)],
    );
    context.connect("screen", None, "cam", "screens", None);

    // **`oversampling` set explicitly, because leaving it unset is not
    // neutral.** The comment this replaced claimed only 3Delight reads
    // it -- wrong about this backend's own code: `pixel_samples()` in
    // `flush.rs` reads `screen`'s `oversampling` and forwards its
    // square root to MoonRay's `pixel_samples` for exactly this
    // reason, ɴsɪ separating AA from shading quality on both sides.
    //
    // Unlike `quality.shadingsamples`, the specification names no
    // default for `oversampling` at all (nsi.readthedocs.io's `screen`
    // page: an empty default column) -- so `flush.rs` forwarding
    // nothing when it is unset is correct, not a gap to close the way
    // the ray depths and shading samples were. There is no spec number
    // to carry across.
    //
    // But a scene built to *compare* two renderers cannot leave either
    // to a number the specification declines to pin down: absent it,
    // each renderer reaches for its own house default, and those have
    // no reason to agree. MoonRay's is `pixel_samples = 8`
    // (`SceneVariables.cc`'s declared default), sixty-four actual
    // camera rays per pixel -- most of why the earlier sweep took 22
    // minutes and came out smoother than 3Delight's, whose own unset
    // default is far lower, which is why *its* image is the noisy one.
    // Two unrelated defaults were being compared and called a result.
    // 16 gives 3Delight sixteen direct camera rays and MoonRay
    // `round(sqrt(16)) = 4`, i.e. sixteen actual -- matched, and cheap
    // enough for a confirmation render.
    context.set_attribute("screen", &[nsi::i32!("oversampling", 16)]);

    // `quality.shadingsamples` is the interface's own control and
    // crosses to `SceneVariables` on the MoonRay side, so asking for a
    // number here asks *both* for it.
    //
    // The interface's default is 1, which is honest and unreadable: at
    // one sample the noise is louder than anything the image is trying
    // to show. Everything *else* on `.global` is left unset on purpose,
    // so the ray depths come from the defaults this backend forwards
    // rather than from a number written here.
    //
    // Taken as a parameter, not hardcoded: the roughness sweep and the
    // five-look row want different budgets. The sweep is read as a
    // measurement and stays cheap; the row is read as an image and can
    // afford to sit at a render for a while.
    context.set_attribute(
        nsi::node::GLOBAL,
        &[nsi::i32!("quality.shadingsamples", shading_samples)],
    );

    // **`quality.causticsamples`, 3Delight-only.** MoonRay has no
    // equivalent control surface for caustic photon density -- its own
    // "caustic" is an eye-caustic BRDF and a path flag, not this
    // mechanism -- so this is inert there and only 3Delight reads it.
    // Set unconditionally rather than only for the looks scene: the
    // roughness sweep has no glass to focus anything through, so it
    // costs that render nothing either way.
    context.set_attribute(
        nsi::node::GLOBAL,
        &[nsi::i32!("quality.causticsamples", 64)],
    );

    // **CPU time per phase, not wall clock.** 3Delight writes proper
    // JSON when the name ends `.json` -- `render_options`,
    // `profiling.timings` per task, `system_time`, `cpu_usage` --
    // undocumented but confirmed by rendering and reading the file.
    // MoonRay's own `stats_file` (`SceneVariables.cc`): "the filename
    // to write the rendering statistics to in CSV format", forwarded
    // by `with_globals` in `flush.rs`. Different formats because
    // neither renderer was asked to match the other's, but both name
    // real per-phase CPU time, which a wall-clock reading from outside
    // the process cannot separate from time lost to another renderer
    // sharing the machine -- the actual question after tonight.
    context.set_attribute(
        nsi::node::GLOBAL,
        &[nsi::string!("statistics.filename", stats)],
    );

    // A dim sky, so the emitter is the brightest thing and the metal
    // has something other than white to reflect.
    context.create("env", nsi::node::ENVIRONMENT, None);
    context.connect("env", None, nsi::ROOT, "objects", None);
    context.create("env_shader", nsi::node::SHADER, None);
    context.set_attribute(
        "env_shader",
        &[
            nsi::string!("shaderfilename", environment),
            nsi::color!("Cs", &[0.55, 0.60, 0.70]),
            nsi::f32!("intensity", 0.5),
        ],
    );
    context.create("env_attributes", nsi::node::ATTRIBUTES, None);
    context.connect("env_attributes", None, "env", "geometryattributes", None);
    context.connect(
        "env_shader",
        None,
        "env_attributes",
        "surfaceshader",
        None,
    );

    // **A key light, because the sky alone cannot show roughness.**
    //
    // A uniform environment is a furnace test: an energy-conserving
    // BRDF integrates a constant radiance field to the same value no
    // matter how it is distributed across the hemisphere, so a matte
    // and a mirror sphere come out the same brightness and a roughness
    // sweep against the sky alone has nothing to sweep. What varies
    // with roughness is the *shape* of a bright, small, angularly
    // compact source -- its reflection blurs from a point to a blob --
    // and that needs a source smaller than the sky.
    //
    // Built the same way every emitter in ɴsɪ is: geometry wearing a
    // shader that emits, so this is the same mechanism the "emissive"
    // look below demonstrates, not a second one to explain.
    sphere(context, "key", [3.4, 6.0, 1.5], 0.5);
    look(
        context,
        "key",
        principled,
        &[
            nsi::color!("i_color", &[0.0, 0.0, 0.0]),
            nsi::color!("incandescence", &[1.0, 0.96, 0.9]),
            // Bright enough to read as the key against the sky, not so
            // bright that Russian roulette keeps every path alive near
            // it: throughput near a source this hot survives roulette
            // far more often, and with the deeper ray counts this
            // backend now forwards to match ɴsɪ's own defaults, that
            // turned a five-sphere test into a half-hour one. 8 matches
            // what the "emissive" look below already uses.
            nsi::f32!("incandescence_intensity", 8.0),
        ],
    );
    caustics(context, "key", &["emit"]);

    // The floor.
    context.create("floor", nsi::node::MESH, None);
    context.set_attribute(
        "floor",
        &[
            nsi::i32!("nvertices", 4),
            nsi::i32_slice!("P.indices", &[0, 1, 2, 3]),
            nsi::point_slice!(
                "P",
                &[
                    [-60.0f32, -1.0, -60.0],
                    [60.0, -1.0, -60.0],
                    [60.0, -1.0, 60.0],
                    [-60.0, -1.0, 60.0],
                ]
            ),
        ],
    );
    context.connect("floor", None, nsi::ROOT, "objects", None);
    look(
        context,
        "floor",
        principled,
        &[
            nsi::color!("i_color", &[0.22, 0.22, 0.24]),
            nsi::f32!("roughness", 0.225),
        ],
    );
    caustics(context, "floor", &["receive"]);

    context.create("beauty", nsi::node::OUTPUT_LAYER, None);
    context.set_attribute(
        "beauty",
        &[
            nsi::string!("variablename", "Ci"),
            nsi::string!("scalarformat", "float"),
        ],
    );
    context.connect("beauty", None, "screen", "outputlayers", None);

    context.create("driver", nsi::node::OUTPUT_DRIVER, None);
    context.set_attribute(
        "driver",
        &[
            nsi::string!("drivername", "exr"),
            nsi::string!("imagefilename", image),
        ],
    );
    context.connect("driver", None, "beauty", "outputdrivers", None);
}

/// **A roughness sweep, to settle what the looks only hint at.**
///
/// The comparison above shows the plastic sphere glossier in 3Delight
/// and the floor glossier in MoonRay, which is the wrong shape for a
/// simple unit mismatch: a roughness read as alpha, or alpha read as
/// roughness, moves every surface the same way. Something else is
/// going on, and five spheres wearing five different looks cannot say
/// what.
///
/// So: one look, one parameter, swept. Identical geometry, identical
/// lighting, only `roughness` changing across the row. Whatever the two
/// renderers disagree about is then a function of that one number, and
/// the measured highlight tells which way.

/// The five looks, left to right.
/// **Caustics are three separate opt-ins, all off by default.**
///
/// `caustics.cast`, `caustics.receive` and `caustics.emit` live on the
/// `attributes` node, not on the shader -- `dlPrincipled` exposes none
/// of them (only `dlConstant` does, as a convenience passthrough), so
/// setting them has to reach the attributes node directly rather than
/// go through `look`'s shader parameters. Confirmed rather than
/// guessed: `dlConstant.oso`'s own metadata names `caustics.emit` and
/// defaults it to `0`, and 3Delight's Maya integration groups the same
/// three names under a "Caustics" section nothing turns on by hand.
/// Geometry with none of the three set casts, receives and emits
/// nothing extra -- which is why the key light's caustic through the
/// glass ball was invisible until all three were set: the light never
/// emitted into caustic paths, the glass never cast them, and the
/// floor never received them.
fn caustics(context: &nsi::Context, geometry: &str, roles: &[&str]) {
    let attributes = format!("{geometry}_attributes");
    // `nsi::i32!` needs a string *literal* or a `const` path for the
    // name, and `caustics.{role}` is neither -- so this builds the
    // argument the macro would, by hand, off the owned `String`.
    let names: Vec<String> = roles
        .iter()
        .map(|role| format!("caustics.{role}"))
        .collect();
    let args: Vec<_> = names
        .iter()
        .map(|name| {
            nsi::Arg::new(name.as_str(), nsi::ArgData::from(nsi::I32::new(1)))
        })
        .collect();
    context.set_attribute(&attributes, &args);
}

fn looks<'a>(context: &nsi::Context<'a>, principled: &'a str) {
    // Matte, plastic, glass, metal, emissive -- left to right.
    let spacing = 2.3f32;
    let first = -2.0 * spacing;
    let place = |slot: usize| [first + slot as f32 * spacing, 0.0, 0.0];

    sphere(context, "matte", place(0), 1.0);
    look(
        context,
        "matte",
        principled,
        &[
            nsi::color!("i_color", &[0.62, 0.24, 0.22]),
            nsi::f32!("roughness", 1.0),
            nsi::f32!("specular_level", 0.0),
        ],
    );
    caustics(context, "matte", &["receive"]);

    sphere(context, "plastic", place(1), 1.0);
    look(
        context,
        "plastic",
        principled,
        &[
            nsi::color!("i_color", &[0.2, 0.42, 0.66]),
            nsi::f32!("roughness", 0.18),
            nsi::f32!("specular_level", 0.6),
        ],
    );
    caustics(context, "plastic", &["receive"]);

    sphere(context, "glass", place(2), 1.0);
    look(
        context,
        "glass",
        principled,
        &[
            nsi::color!("i_color", &[1.0, 1.0, 1.0]),
            nsi::f32!("refract_weight", 1.0),
            nsi::f32!("refract_ior", 1.5),
            nsi::f32!("roughness", 0.0),
        ],
    );
    // The glass ball is the focusing element: it casts the caustic
    // rather than receiving one, which is the same "one role per
    // object" split every renderer with this feature makes.
    caustics(context, "glass", &["cast"]);

    sphere(context, "metal", place(3), 1.0);
    look(
        context,
        "metal",
        principled,
        &[
            nsi::color!("i_color", &[0.91, 0.87, 0.78]),
            nsi::f32!("metallic", 1.0),
            nsi::f32!("roughness", 0.06),
        ],
    );
    caustics(context, "metal", &["receive"]);

    sphere(context, "emissive", place(4), 1.0);
    look(
        context,
        "emissive",
        principled,
        &[
            nsi::color!("i_color", &[0.05, 0.05, 0.05]),
            nsi::color!("incandescence", &[1.0, 0.72, 0.36]),
            nsi::f32!("incandescence_intensity", 8.0),
        ],
    );
    caustics(context, "emissive", &["receive"]);
}

#[test]
fn a_roughness_sweep_through_both_renderers() {
    let Ok(delight) = std::env::var("DELIGHT") else {
        eprintln!("skipped: no $DELIGHT");
        return;
    };
    let principled = format!("{delight}/osl/dlPrincipled.oso");
    if !std::path::Path::new(&principled).is_file() {
        eprintln!("skipped: {principled} is not there");
        return;
    }

    // **Not covered by the `#[ctor]` in `src/linked.rs`.** This test
    // binary references nothing else from that module, so nothing
    // forces the linker to pull its object file out of the `rlib`
    // archive and the constructor never runs -- a real Rust/linker
    // limitation, not an oversight; see that module's doc comment for
    // why. Calling `register` here is the reliable path regardless.
    nsi::backend::register("moonray", Arc::new(nsi_moonray::MoonRay));

    let environment = std::path::Path::new(env!("NSI_MOONRAY_SHADERS"))
        .join("moonrayEnvironment.oso")
        .to_string_lossy()
        .into_owned();
    let directory = std::env::temp_dir().join("nsi-moonray-roughness");
    std::fs::create_dir_all(&directory).expect("a writable directory");

    const SWEEP: [f32; 5] = [0.05, 0.15, 0.30, 0.50, 0.80];

    // Cheap: this render is read as numbers, not a picture, and every
    // extra minute is a minute spent waiting to know if a fix worked.
    const SHADING_SAMPLES: i32 = 32;

    for renderer in ["3delight", "moonray"] {
        let image = directory.join(format!("{renderer}.exr"));
        let _ = std::fs::remove_file(&image);
        // JSON for 3Delight, CSV for MoonRay -- each renderer's own
        // native format, not a shared one; see `stage`'s doc comment.
        let stats_extension = if renderer == "3delight" {
            "json"
        } else {
            "csv"
        };
        let stats = directory.join(format!("{renderer}.{stats_extension}"));
        let _ = std::fs::remove_file(&stats);

        {
            let context =
                nsi::Context::new(Some(&[nsi::string!("renderer", renderer)]))
                    .unwrap_or_else(|| panic!("{renderer} did not load"));

            stage(
                &context,
                &principled,
                &environment,
                image.to_string_lossy().as_ref(),
                SHADING_SAMPLES,
                stats.to_string_lossy().as_ref(),
                1.1,
                0.0,
            );

            let spacing = 2.3f32;
            let first = -2.0 * spacing;
            for (slot, roughness) in SWEEP.into_iter().enumerate() {
                let handle = format!("ball{slot}");
                sphere(
                    &context,
                    &handle,
                    [first + slot as f32 * spacing, 0.0, 0.0],
                    1.0,
                );
                look(
                    &context,
                    &handle,
                    &principled,
                    &[
                        nsi::color!("i_color", &[0.8, 0.8, 0.8]),
                        nsi::f32!("roughness", roughness),
                        nsi::f32!("specular_level", 0.5),
                    ],
                );
            }

            context.render_control(nsi::Action::Start, None);
            context.render_control(nsi::Action::Wait, None);
            context.render_control(nsi::Action::Stop, None);
        }

        eprintln!(
            "{renderer}: {} ({})",
            image.display(),
            if image.is_file() {
                "written"
            } else {
                "MISSING"
            }
        );
        eprintln!(
            "{renderer}: stats {} ({})",
            stats.display(),
            if stats.is_file() {
                "written"
            } else {
                "MISSING"
            }
        );
    }
}

#[test]
fn a_row_of_looks_through_both_renderers() {
    let Ok(delight) = std::env::var("DELIGHT") else {
        eprintln!("skipped: no $DELIGHT, so no shaders to share");
        return;
    };
    let principled = format!("{delight}/osl/dlPrincipled.oso");
    if !std::path::Path::new(&principled).is_file() {
        eprintln!("skipped: {principled} is not there");
        return;
    }

    // **Not covered by the `#[ctor]` in `src/linked.rs`.** This test
    // binary references nothing else from that module, so nothing
    // forces the linker to pull its object file out of the `rlib`
    // archive and the constructor never runs -- a real Rust/linker
    // limitation, not an oversight; see that module's doc comment for
    // why. Calling `register` here is the reliable path regardless.
    nsi::backend::register("moonray", Arc::new(nsi_moonray::MoonRay));

    // **MoonRay's own environment, asked for by name.**
    //
    // `EnvLight` is a light class with nothing an `OslMap` could bind
    // to, so an environment's OSL never executes on that side. Rather
    // than let a built-in stand in silently for whatever shader the
    // scene named -- which made the dome a stop bright with a black
    // sky, and swamped everything this image is about -- the scene
    // names the stub this crate ships for exactly that class.
    //
    // 3Delight runs it, because it is a real OSL shader that emits a
    // constant. MoonRay maps its parameters onto `EnvLight` one for
    // one. Both therefore stand on the same ground, and the comparison
    // is about the surfaces.
    let environment = std::path::Path::new(env!("NSI_MOONRAY_SHADERS"))
        .join("moonrayEnvironment.oso")
        .to_string_lossy()
        .into_owned();

    let directory = std::env::temp_dir().join("nsi-moonray-shaderballs");
    std::fs::create_dir_all(&directory).expect("a writable directory");

    // This render is read as an image, not a measurement, and can
    // afford to sit for it.
    const SHADING_SAMPLES: i32 = 256;

    for renderer in ["3delight", "moonray"] {
        let image = directory.join(format!("{renderer}.exr"));
        let _ = std::fs::remove_file(&image);
        // JSON for 3Delight, CSV for MoonRay -- each renderer's own
        // native format; see `stage`'s doc comment.
        let stats_extension = if renderer == "3delight" {
            "json"
        } else {
            "csv"
        };
        let stats = directory.join(format!("{renderer}.{stats_extension}"));
        let _ = std::fs::remove_file(&stats);

        {
            let context =
                nsi::Context::new(Some(&[nsi::string!("renderer", renderer)]))
                    .unwrap_or_else(|| panic!("{renderer} did not load"));

            stage(
                &context,
                &principled,
                &environment,
                image.to_string_lossy().as_ref(),
                SHADING_SAMPLES,
                stats.to_string_lossy().as_ref(),
                // Raised and pitched down 25 degrees: the frustum's
                // vertical half-angle is 20 (`fov` 40), so its top edge
                // sits 5 degrees below the horizon's fixed 0-degree
                // elevation -- the sky is out of frame with a small
                // margin, not balanced exactly on the edge.
                4.0,
                25.0,
            );
            looks(&context, &principled);
            // **A batch render: start, wait, stop.** No
            // `synchronize` -- that is the interactive loop's verb,
            // and a render driven through it is a different thing
            // being measured. `stop` after the wait so the frame is
            // finished rather than merely left.
            context.render_control(nsi::Action::Start, None);
            context.render_control(nsi::Action::Wait, None);
            context.render_control(nsi::Action::Stop, None);
        }

        eprintln!(
            "{renderer}: {} ({})",
            image.display(),
            if image.is_file() {
                "written"
            } else {
                "MISSING"
            }
        );
        eprintln!(
            "{renderer}: stats {} ({})",
            stats.display(),
            if stats.is_file() {
                "written"
            } else {
                "MISSING"
            }
        );
    }
}

/// Dumps `examples/shaderballs.nsi`: the five-look scene, as ɴsɪ's own
/// text, with no renderer in the loop.
///
/// **`type="apistream"` is not a substitute for a renderer -- it *is*
/// one, in the sense the interface cares about.** `NSIBegin` takes it
/// alongside `streamfilename` and `streamformat="nsi"` to open a
/// context that writes every subsequent call as ɴsɪ's own ASCII
/// syntax rather than rendering it -- documented at
/// <https://nsi.readthedocs.io/en/latest/c-api.html>. So this calls
/// exactly `stage`, `looks` and the same `render_control` sequence the
/// 3Delight and MoonRay contexts get, through a context that happens
/// to write rather than shade: the file is the same scene those two
/// render, not a hand-abridged stand-in for it.
///
/// `#[ignore]`d: this writes into the source tree, which a `cargo
/// test` run has no business doing on its own, and the file only
/// needs regenerating when the scene changes. Run it explicitly:
/// `cargo test --features rdl2,backend-registry --test shaderballs
/// dump_shaderballs_nsi -- --ignored`.
#[test]
#[ignore]
fn dump_shaderballs_nsi() {
    let delight = std::env::var("DELIGHT").expect("$DELIGHT");
    let principled = format!("{delight}/osl/dlPrincipled.oso");
    assert!(
        std::path::Path::new(&principled).is_file(),
        "{principled} is not there"
    );

    let environment = std::path::Path::new(env!("NSI_MOONRAY_SHADERS"))
        .join("moonrayEnvironment.oso")
        .to_string_lossy()
        .into_owned();

    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("shaderballs.nsi");

    let context = nsi::Context::new(Some(&[
        nsi::string!("type", "apistream"),
        nsi::string!("streamfilename", out.to_string_lossy().as_ref()),
        nsi::string!("streamformat", "nsi"),
    ]))
    .expect("an apistream context");

    stage(
        &context,
        &principled,
        &environment,
        // The stream records whatever `outputdriver` a real render
        // would write to; naming the same `.exr` a batch run produces
        // keeps the dump usable as-is rather than pointing at a file
        // this run never creates.
        "shaderballs.exr",
        256,
        "shaderballs.csv",
        4.0,
        25.0,
    );
    looks(&context, &principled);
    context.render_control(nsi::Action::Start, None);
    context.render_control(nsi::Action::Wait, None);
    context.render_control(nsi::Action::Stop, None);
    drop(context);

    eprintln!(
        "shaderballs.nsi: {} ({})",
        out.display(),
        if out.is_file() { "written" } else { "MISSING" }
    );
}

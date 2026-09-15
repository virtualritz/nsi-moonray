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

#![cfg(all(feature = "rdl2", moonray, feature = "linked-route"))]

use nsi_ffi_wrap as nsi;
use std::num::NonZeroUsize;

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
fn stage<'a>(
    context: &nsi::Context<'a>,
    principled: &'a str,
    environment: &'a str,
    image: &str,
) {
    let (width, height) = (960i32, 320i32);

    context.create("camxf", nsi::node::TRANSFORM, None);
    context.set_attribute(
        "camxf",
        &[nsi::matrix_f64!(
            "transformationmatrix",
            &[
                1.0, 0.0, 0.0, 0.0, //
                0.0, 1.0, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                0.0, 1.1, 11.0, 1.0f64,
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

    // **Samples on `.global`, because both renderers read it.**
    //
    // `quality.shadingsamples` is the interface's own control and
    // crosses to `SceneVariables` on the MoonRay side, so asking for a
    // number here asks *both* for it. That is the opposite of
    // `screen.oversampling`, which only 3Delight reads -- setting that
    // would hand one renderer a larger budget than the other and call
    // the result a comparison, so it is deliberately absent.
    //
    // The interface's default is 1, which is honest and unreadable: at
    // one sample the noise is louder than anything the image is trying
    // to show. Everything *else* on `.global` is left unset on purpose,
    // so the ray depths come from the defaults this backend forwards
    // rather than from a number written here.
    context.set_attribute(
        nsi::node::GLOBAL,
        &[nsi::i32!("quality.shadingsamples", 32)],
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
            nsi::f32!("roughness", 0.45),
        ],
    );

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

    nsi::backend::register(
        "moonray",
        std::sync::Arc::new(nsi_moonray::MoonRay),
    );

    let environment = std::path::Path::new(env!("NSI_MOONRAY_SHADERS"))
        .join("moonrayEnvironment.oso")
        .to_string_lossy()
        .into_owned();
    let directory = std::env::temp_dir().join("nsi-moonray-roughness");
    std::fs::create_dir_all(&directory).expect("a writable directory");

    const SWEEP: [f32; 5] = [0.05, 0.15, 0.30, 0.50, 0.80];

    for renderer in ["3delight", "moonray"] {
        let image = directory.join(format!("{renderer}.exr"));
        let _ = std::fs::remove_file(&image);

        {
            let context =
                nsi::Context::new(Some(&[nsi::string!("renderer", renderer)]))
                    .unwrap_or_else(|| panic!("{renderer} did not load"));

            stage(
                &context,
                &principled,
                &environment,
                image.to_string_lossy().as_ref(),
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

    for renderer in ["3delight", "moonray"] {
        let image = directory.join(format!("{renderer}.exr"));
        let _ = std::fs::remove_file(&image);

        {
            let context =
                nsi::Context::new(Some(&[nsi::string!("renderer", renderer)]))
                    .unwrap_or_else(|| panic!("{renderer} did not load"));

            stage(
                &context,
                &principled,
                &environment,
                image.to_string_lossy().as_ref(),
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
    }
}

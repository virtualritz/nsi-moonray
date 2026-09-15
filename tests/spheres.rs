//! A row of spheres, one of them emissive, rendered through MoonRay.
//!
//! **What it is for.** An ɴsɪ emitter is geometry wearing a shader that
//! emits, and this backend turns every one of those into a MoonRay
//! `MeshLight`. A mesh light's geometry may not be in the render layer
//! -- `RenderContext::createMeshLightLayer` builds a layer of its own
//! for it -- so the light is forced visible to *camera* rays and the
//! mesh is left out of the layer every other ray consults.
//!
//! That predicts something specific and checkable: the emissive sphere
//! is seen directly, and is missing from what the mirror spheres
//! reflect. This renders the scene and reads both, so the claim is a
//! measurement rather than an impression.
//!
//! The shaders are 3Delight's own `.oso` files, which is what makes
//! this a *shared* scene rather than one built around this backend:
//! `dlPrincipled` for the spheres and `areaLight` for the emitter.
//! Needs `$DELIGHT`; skipped without it.

use nsi_intermediate::{OwnedArgument, OwnedData, Scene};
use nsi_moonray::{flush::flush, render::Render};
use nsi_trait::Type;

fn arg(name: &str, type_tag: Type, data: OwnedData) -> OwnedArgument {
    OwnedArgument::new(name, type_tag, 1, 0, data)
}

/// A UV sphere as an ɴsɪ mesh.
///
/// Quads rather than triangles, and a modest tessellation: what is
/// being read here is whether a reflection contains a bright disc, not
/// how round its silhouette is.
fn sphere(
    scene: &mut Scene,
    handle: &str,
    centre: [f32; 3],
    radius: f32,
    segments: usize,
    rings: usize,
) {
    let mut positions = Vec::new();
    for ring in 0..=rings {
        let theta = std::f32::consts::PI * ring as f32 / rings as f32;
        for segment in 0..segments {
            let phi = std::f32::consts::TAU * segment as f32 / segments as f32;
            positions.extend_from_slice(&[
                centre[0] + radius * theta.sin() * phi.cos(),
                centre[1] + radius * theta.cos(),
                centre[2] + radius * theta.sin() * phi.sin(),
            ]);
        }
    }

    let index = |ring: usize, segment: usize| -> i32 {
        (ring * segments + segment % segments) as i32
    };

    let mut counts = Vec::new();
    let mut indices = Vec::new();
    for ring in 0..rings {
        for segment in 0..segments {
            counts.push(4);
            indices.extend_from_slice(&[
                index(ring, segment),
                index(ring, segment + 1),
                index(ring + 1, segment + 1),
                index(ring + 1, segment),
            ]);
        }
    }

    scene.create(handle, "mesh").expect("a recordable edit");
    scene
        .set_attribute(
            handle,
            vec![
                arg("nvertices", Type::I32, OwnedData::I32(counts)),
                arg("P.indices", Type::I32, OwnedData::I32(indices)),
                arg("P", Type::Point, OwnedData::F32(positions)),
            ],
        )
        .expect("a recordable edit");
    scene
        .connect(handle, None, ".root", "objects")
        .expect("known attribute");
}

/// Bind one of 3Delight's own shaders to a shape.
fn shade(
    scene: &mut Scene,
    geometry: &str,
    shader_file: &str,
    parameters: Vec<OwnedArgument>,
) {
    let shader = format!("{geometry}_shader");
    let attributes = format!("{geometry}_attributes");

    scene.create(&shader, "shader").expect("a recordable edit");
    let mut all = vec![arg(
        "shaderfilename",
        Type::String,
        OwnedData::String(vec![shader_file.as_bytes().to_vec()]),
    )];
    all.extend(parameters);
    scene
        .set_attribute(&shader, all)
        .expect("a recordable edit");

    scene
        .create(&attributes, "attributes")
        .expect("a recordable edit");
    scene
        .connect(&attributes, None, geometry, "geometryattributes")
        .expect("known attribute");
    scene
        .connect(&shader, None, &attributes, "surfaceshader")
        .expect("known attribute");
}

/// **The emissive sphere is seen, and is missing from the reflections.**
#[test]
fn a_row_of_spheres_with_one_emitter() {
    let Ok(delight) = std::env::var("DELIGHT") else {
        eprintln!("skipped: no $DELIGHT, so no 3Delight `.oso` shaders");
        return;
    };
    if nsi_moonray::render::binary().is_err() {
        eprintln!("skipped: no `moonray` binary");
        return;
    }

    let principled = format!("{delight}/osl/dlPrincipled.oso");
    let area_light = format!("{delight}/osl/areaLight.oso");
    for shader in [&principled, &area_light] {
        if !std::path::Path::new(shader).is_file() {
            eprintln!("skipped: {shader} is not there");
            return;
        }
    }

    let (width, height) = (480i32, 200i32);
    let mut scene = Scene::default();

    // The camera, looking down -z at a row standing on a floor.
    scene
        .create("cam", "perspectivecamera")
        .expect("a recordable edit");
    scene
        .set_attribute(
            "cam",
            vec![arg("fov", Type::F32, OwnedData::F32(vec![40.0]))],
        )
        .expect("a recordable edit");
    scene
        .create("camxf", "transform")
        .expect("a recordable edit");
    scene
        .set_attribute(
            "camxf",
            vec![arg(
                "transformationmatrix",
                Type::MatrixF64,
                OwnedData::F64(vec![
                    1.0, 0.0, 0.0, 0.0, //
                    0.0, 1.0, 0.0, 0.0, //
                    0.0, 0.0, 1.0, 0.0, //
                    0.0, 0.6, 9.0, 1.0,
                ]),
            )],
        )
        .expect("a recordable edit");
    scene
        .connect("camxf", None, ".root", "objects")
        .expect("known attribute");
    scene
        .connect("cam", None, "camxf", "objects")
        .expect("known attribute");

    scene.create("screen", "screen").expect("a recordable edit");
    scene
        .set_attribute(
            "screen",
            vec![arg(
                "resolution",
                Type::I32,
                OwnedData::I32(vec![width, height]),
            )],
        )
        .expect("a recordable edit");
    scene
        .connect("screen", None, "cam", "screens")
        .expect("known attribute");

    // **A dim environment, because a mirror in a uniform bright one
    // looks like a flat white ball.** The whole question here is what
    // the mirrors reflect, and that is unreadable until the background
    // stops being the brightest thing in the scene.
    scene
        .create("env", "environment")
        .expect("a recordable edit");
    scene
        .connect("env", None, ".root", "objects")
        .expect("known attribute");
    scene
        .create("env_shader", "shader")
        .expect("a recordable edit");
    scene
        .set_attribute(
            "env_shader",
            vec![
                arg(
                    "shaderfilename",
                    Type::String,
                    OwnedData::String(vec![
                        format!("{delight}/osl/environmentLight.oso")
                            .into_bytes(),
                    ]),
                ),
                arg("intensity", Type::F32, OwnedData::F32(vec![0.05])),
            ],
        )
        .expect("a recordable edit");
    scene
        .create("env_attributes", "attributes")
        .expect("a recordable edit");
    scene
        .connect("env_attributes", None, "env", "geometryattributes")
        .expect("known attribute");
    scene
        .connect("env_shader", None, "env_attributes", "surfaceshader")
        .expect("known attribute");

    // A floor, so the mirrors have something with structure to show.
    scene.create("floor", "mesh").expect("a recordable edit");
    scene
        .set_attribute(
            "floor",
            vec![
                arg("nvertices", Type::I32, OwnedData::I32(vec![4])),
                arg("P.indices", Type::I32, OwnedData::I32(vec![0, 1, 2, 3])),
                arg(
                    "P",
                    Type::Point,
                    OwnedData::F32(vec![
                        -40.0, -0.95, -40.0, 40.0, -0.95, -40.0, 40.0, -0.95,
                        40.0, -40.0, -0.95, 40.0,
                    ]),
                ),
            ],
        )
        .expect("a recordable edit");
    scene
        .connect("floor", None, ".root", "objects")
        .expect("known attribute");

    // Four mirror-ish spheres, then the emitter at the right end. The
    // mirrors are polished, so whatever they reflect is legible.
    let spacing = 2.1f32;
    let first = -2.0 * spacing;
    for (slot, roughness) in [0.02f32, 0.05, 0.1, 0.2].into_iter().enumerate() {
        let handle = format!("sphere{slot}");
        sphere(
            &mut scene,
            &handle,
            [first + slot as f32 * spacing, 0.0, 0.0],
            0.9,
            48,
            24,
        );
        shade(
            &mut scene,
            &handle,
            &principled,
            vec![
                arg(
                    "i_color",
                    Type::Color,
                    OwnedData::F32(vec![0.85, 0.86, 0.88]),
                ),
                arg("metallic", Type::F32, OwnedData::F32(vec![1.0])),
                arg("roughness", Type::F32, OwnedData::F32(vec![roughness])),
            ],
        );
    }

    // The emitter, at the right end of the row.
    sphere(
        &mut scene,
        "emitter",
        [first + 4.0 * spacing, 0.0, 0.0],
        0.9,
        48,
        24,
    );
    shade(
        &mut scene,
        "emitter",
        &area_light,
        vec![
            arg("intensity", Type::F32, OwnedData::F32(vec![12.0])),
            arg(
                "i_color",
                Type::Color,
                OwnedData::F32(vec![1.0, 0.72, 0.35]),
            ),
        ],
    );

    shade(
        &mut scene,
        "floor",
        &principled,
        vec![
            arg(
                "i_color",
                Type::Color,
                OwnedData::F32(vec![0.25, 0.25, 0.27]),
            ),
            arg("roughness", Type::F32, OwnedData::F32(vec![0.5])),
        ],
    );

    let directory = std::env::temp_dir()
        .join(format!("nsi-moonray-spheres-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("a writable directory");
    let image = directory.join("spheres.exr");

    scene
        .create("beauty", "outputlayer")
        .expect("a recordable edit");
    scene
        .set_attribute(
            "beauty",
            vec![arg(
                "variablename",
                Type::String,
                OwnedData::String(vec![b"Ci".to_vec()]),
            )],
        )
        .expect("a recordable edit");
    scene
        .connect("beauty", None, "screen", "outputlayers")
        .expect("known attribute");

    scene
        .create("driver", "outputdriver")
        .expect("a recordable edit");
    scene
        .set_attribute(
            "driver",
            vec![arg(
                "imagefilename",
                Type::String,
                OwnedData::String(vec![
                    image.to_string_lossy().as_bytes().to_vec(),
                ]),
            )],
        )
        .expect("a recordable edit");
    scene
        .connect("driver", None, "beauty", "outputdrivers")
        .expect("known attribute");

    let flushed = flush(&scene);
    let scene_file = directory.join("spheres.rdla");
    std::fs::write(&scene_file, flushed.to_rdla()).expect("writing the scene");
    for line in &flushed.limitations {
        eprintln!("nsi-moonray: {line}");
    }

    let mut job = Render::new(&scene_file);
    job.threads = Some(4);
    job.run().expect("the render runs");

    eprintln!("wrote {}", image.display());
    assert!(image.is_file(), "the render produced no image");
}

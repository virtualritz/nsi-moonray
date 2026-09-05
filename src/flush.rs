//! Turning a recorded ɴsɪ scene into an `.rdla` document.
//!
//! This is the flush, and it is the only thing this repository owns.
//! Everything it consumes has already been resolved by
//! [`nsi_intermediate`]: transform chains are composed, `attributes`
//! nodes are dissolved into bindings, and the output chain is collapsed
//! into camera-and-screen pairs. None of that logic is re-derived here.
//!
//! # ɴsɪ always returns an image
//!
//! Nothing in this module refuses a scene. What MoonRay cannot carry is
//! recorded in [`Flushed::limitations`] and the rest is emitted, because
//! a render farm depends on a frame coming back. That is also why an
//! unmapped shader leaves the material column `undef()` rather than
//! aborting: MoonRay renders it with its default material, and the
//! limitation says so.

use crate::{
    document::{Assignment, Body, Document, Object},
    value::{Reference, Value},
};
use nsi_intermediate::{IDENTITY, OwnedData, Scene};

/// MoonRay's mesh geometry, whose DSO is `moonray/dso/geometry/RdlMesh`.
const MESH: &str = "RdlMeshGeometry";

/// MoonRay's perspective camera DSO.
const PERSPECTIVE_CAMERA: &str = "PerspectiveCamera";

/// MoonRay's environment light DSO.
const ENVIRONMENT_LIGHT: &str = "EnvLight";

/// The material given to geometry with no ɴsɪ shader bound.
///
/// Not a nicety: MoonRay does not render a `Layer` row whose material
/// column is `undef()` at all. Verified against the renderer -- the
/// same triangle is absent from the image without a material and
/// present with one.
const DEFAULT_MATERIAL: &str = "/nsi/default_material";

/// The set every light lands in.
///
/// A `Layer` row with no light set is lit by nothing, so this is
/// referenced from every assignment rather than being decoration.
const LIGHT_SET: &str = "/nsi/lights";

/// The material every ɴsɪ shader becomes.
///
/// MoonRay has no OSL (`research.md` F6), so an ɴsɪ shader cannot be
/// run as written and there is nothing to translate it into
/// mechanically. `UsdPreviewSurface` is stock MoonRay's general-purpose
/// PBR surface -- diffuse colour, metallic, roughness, IOR, opacity,
/// emission -- and standing it in at its defaults is what keeps a
/// shaded scene from rendering as MoonRay's untextured default.
const MATERIAL: &str = "UsdPreviewSurface";

/// `PerspectiveCamera`'s default film-back width in millimetres, which
/// the focal length is derived against.
const FILM_WIDTH_APERTURE: f32 = 24.0;

/// An emitted scene, and what did not survive the crossing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Flushed {
    pub document: Document,
    /// One line per thing MoonRay cannot represent. Never empty when
    /// something was dropped or substituted, and never a reason to
    /// refuse the scene.
    pub limitations: Vec<String>,
}

impl Flushed {
    /// The `.rdla` file.
    pub fn to_rdla(&self) -> String {
        self.document.to_rdla()
    }
}

/// Flush a recorded scene.
pub fn flush(scene: &Scene) -> Flushed {
    let mut flushed = Flushed::default();

    // `SceneVariables` is written first, as rdl2's own writer does, but
    // what goes in it -- the camera, the layer, the resolution -- is
    // only known once the rest has been walked. So it is built last and
    // inserted at the front.
    let mut variables = Object::scene_variables();
    let mut geometries = Vec::new();
    let mut lights = Vec::new();
    // Geometry and its material, if it has one. The rows themselves are
    // built after the walk, because every one of them references the
    // light set and that is not known until the last node is seen.
    let mut bindings: Vec<(String, Option<Reference>)> = Vec::new();
    let mut objects = Vec::new();

    let resolution = resolution(scene);

    for (handle, node) in &scene.nodes {
        match node.node_type.as_str() {
            "mesh" | "subdivisionmesh" => {
                let subdivision = node.node_type == "subdivisionmesh";
                objects.push(mesh(scene, handle, subdivision, &mut flushed));
                geometries.push(Reference::new(MESH, handle));

                // Every mesh gets a row, bound or not: MoonRay renders
                // what the `Layer` names, so geometry left out of it is
                // simply absent from the image.
                bindings.push((handle.clone(), material(scene, handle)));
            }

            "perspectivecamera" => {
                objects.push(camera(scene, handle, resolution, &mut flushed));
            }

            "environment" => {
                objects.push(environment(scene, handle, &mut flushed));
                lights.push(Reference::new(ENVIRONMENT_LIGHT, handle));
            }

            // Resolved away upstream, or carried by another node.
            "transform" | "attributes" | "screen" | "root" => {}

            "outputdriver" | "outputlayer" => {}

            "shader" => {
                // Substituted at its defaults, not translated: an ɴsɪ
                // shader is an OSL shader, and MoonRay runs none.
                objects.push(Object::new(MATERIAL, handle));
                flushed.limitations.push(format!(
                    "shader {handle:?} is an OSL shader, which MoonRay \
                     cannot run; a default {MATERIAL} stands in and none \
                     of the shader's parameters are carried over"
                ));
            }

            other => flushed.limitations.push(format!(
                "node {handle:?} of type {other:?} has no MoonRay mapping \
                 and was skipped"
            )),
        }

        if !node.time_attrs.is_empty() {
            // `nsi-intermediate` resolves static transforms only, so
            // motion samples cannot be honoured yet. Reporting a sharp
            // render is the contract; flattening it silently is not.
            flushed.limitations.push(format!(
                "node {handle:?} carries {} motion sample(s), which are \
                 not resolved upstream yet; it renders sharp",
                node.time_attrs.len()
            ));
        }
    }

    for output in scene.render_outputs() {
        for layer in &output.layers {
            objects.push(render_output(scene, &layer.handle, &layer.drivers));
        }

        variables = variables
            .set("camera", Value::Object(camera_reference(&output.camera)));
    }

    let light_set = if lights.is_empty() {
        // Nothing lights the scene. Say so: a correct scene that
        // renders black looks like a bug in this backend, and this is
        // the one line that says it is not.
        flushed.limitations.push(
            "no ɴsɪ node became a MoonRay light, so the scene renders \
             black; only `environment` maps to one so far"
                .to_string(),
        );
        None
    } else {
        objects.push(Object {
            class: "LightSet".to_string(),
            name: Some(LIGHT_SET.to_string()),
            body: Body::Set(lights),
        });
        Some(Reference::new("LightSet", LIGHT_SET))
    };

    let mut unshaded = 0;
    let assignments = bindings
        .into_iter()
        .map(|(handle, material)| {
            let material = material.unwrap_or_else(|| {
                unshaded += 1;
                Reference::new(MATERIAL, DEFAULT_MATERIAL)
            });

            Assignment::new(
                Reference::new(MESH, &handle),
                Some(material),
                light_set.clone(),
            )
        })
        .collect();

    if unshaded > 0 {
        objects.push(Object::new(MATERIAL, DEFAULT_MATERIAL));
        flushed.limitations.push(format!(
            "{unshaded} shape(s) had no ɴsɪ shader bound and were given a \
             default {MATERIAL}; MoonRay does not render geometry whose \
             layer assignment has no material"
        ));
    }

    if !geometries.is_empty() {
        objects.push(Object {
            class: "GeometrySet".to_string(),
            name: Some("/nsi/geometries".to_string()),
            body: Body::Set(geometries),
        });
    }

    objects.push(Object {
        class: "Layer".to_string(),
        name: Some("/nsi/layer".to_string()),
        body: Body::Layer(assignments),
    });

    variables = variables
        .set(
            "layer",
            Value::Object(Reference::new("Layer", "/nsi/layer")),
        )
        .set("image_width", Value::Int(resolution.0))
        .set("image_height", Value::Int(resolution.1));

    flushed.document.push(variables);
    for object in objects {
        flushed.document.push(object);
    }

    flushed
}

/// One `RdlMeshGeometry`, with its world transform baked in.
fn mesh(
    scene: &Scene,
    handle: &str,
    subdivision: bool,
    flushed: &mut Flushed,
) -> Object {
    let node = &scene.nodes[handle];

    let mut object = Object::new(MESH, handle);

    let transform = scene.world_transform(handle);
    if transform != IDENTITY {
        object = object.set("node_xform", Value::Mat4d(transform));
    }

    match node.attrs.get("nvertices").map(|arg| &arg.data) {
        Some(OwnedData::I32(counts)) => {
            object = object.set(
                "face_vertex_count",
                Value::Vector(counts.iter().map(|c| Value::Int(*c)).collect()),
            );
        }
        _ => flushed
            .limitations
            .push(format!("mesh {handle:?} has no \"nvertices\"")),
    }

    match node.attrs.get("P.indices").map(|arg| &arg.data) {
        Some(OwnedData::I32(indices)) => {
            object = object.set(
                "vertices_by_index",
                Value::Vector(indices.iter().map(|i| Value::Int(*i)).collect()),
            );
        }
        _ => flushed
            .limitations
            .push(format!("mesh {handle:?} has no \"P.indices\"")),
    }

    match node.attrs.get("P").map(|arg| &arg.data) {
        Some(OwnedData::F32(points)) if points.len() % 3 == 0 => {
            object = object.set(
                "vertex_list_0",
                Value::Vector(
                    points
                        .chunks_exact(3)
                        .map(|p| Value::Vec3f([p[0], p[1], p[2]]))
                        .collect(),
                ),
            );
        }
        _ => flushed
            .limitations
            .push(format!("mesh {handle:?} has no float \"P\"")),
    }

    // `is_subd` defaults to *true*, so an ɴsɪ `mesh` has to say
    // otherwise or it renders as a subdivision surface.
    object.set("is_subd", Value::Bool(subdivision))
}

/// The material bound to one piece of geometry, if any.
///
/// The shader itself is substituted where it is declared; here it only
/// has to be pointed at. An `attributes` node carrying nothing but
/// visibility has no shader, and that row's material column stays
/// `undef()`.
fn material(scene: &Scene, handle: &str) -> Option<Reference> {
    scene
        .geometry_binding(handle)?
        .surface_shader
        .as_deref()
        .map(|shader| Reference::new(MATERIAL, shader))
}

/// One `EnvLight`.
///
/// ɴsɪ puts the environment's *look* in an OSL shader hanging off an
/// `attributes` node, and MoonRay cannot run it, so what crosses is the
/// light itself at its defaults -- white, intensity 1 -- and its
/// transform. That is enough to light a scene, which is the point.
fn environment(scene: &Scene, handle: &str, flushed: &mut Flushed) -> Object {
    let mut object = Object::new(ENVIRONMENT_LIGHT, handle);

    let transform = scene.world_transform(handle);
    if transform != IDENTITY {
        object = object.set("node_xform", Value::Mat4d(transform));
    }

    if scene
        .geometry_binding(handle)
        .and_then(|binding| binding.surface_shader)
        .is_some()
    {
        flushed.limitations.push(format!(
            "environment {handle:?} carries a shader, which MoonRay \
             cannot run; the light is white at intensity 1 and any \
             environment texture is lost"
        ));
    }

    object
}

/// One `PerspectiveCamera`.
fn camera(
    scene: &Scene,
    handle: &str,
    resolution: (i32, i32),
    flushed: &mut Flushed,
) -> Object {
    let node = &scene.nodes[handle];

    let mut object = Object::new(PERSPECTIVE_CAMERA, handle);

    let transform = scene.world_transform(handle);
    if transform != IDENTITY {
        object = object.set("node_xform", Value::Mat4d(transform));
    }

    match node.attrs.get("fov").map(|arg| &arg.data) {
        Some(OwnedData::F32(values)) if !values.is_empty() => {
            object =
                object.set("focal", Value::Float(focal(values[0], resolution)));
        }
        Some(OwnedData::F64(values)) if !values.is_empty() => {
            object = object.set(
                "focal",
                Value::Float(focal(values[0] as f32, resolution)),
            );
        }
        _ => flushed.limitations.push(format!(
            "camera {handle:?} has no \"fov\"; MoonRay's default focal \
             length is used"
        )),
    }

    object
}

/// ɴsɪ's vertical field of view, in degrees, as MoonRay's focal length
/// in millimetres.
///
/// `PerspectiveCamera::computeProjectionMatrix` scales the aperture
/// window by `near / focal` and divides the vertical extent by the pixel
/// aspect ratio, so with a square pixel the vertical half-angle is
/// `atan(halfFilmWidth * height / width / focal)`. Inverting that gives
/// the focal length below.
///
/// ɴsɪ's `fov` is read as **vertical**, which is how
/// `nsi_toolbelt::look_at_bounding_box_perspective_camera` uses it.
/// Unverified against a 3Delight render; see `contracts/flush.md`.
fn focal(fov_degrees: f32, resolution: (i32, i32)) -> f32 {
    let aspect = resolution.1 as f32 / resolution.0 as f32;
    let half = (fov_degrees.to_radians() * 0.5).tan();

    FILM_WIDTH_APERTURE * 0.5 * aspect / half
}

/// One `RenderOutput` per ɴsɪ output layer.
fn render_output(scene: &Scene, layer: &str, drivers: &[String]) -> Object {
    let mut object = Object::new("RenderOutput", layer);

    if let Some(node) = scene.nodes.get(layer)
        && let Some(OwnedData::String(names)) =
            node.attrs.get("variablename").map(|arg| &arg.data)
        && let Some(name) = names.first()
    {
        object = object.set("channel_name", Value::String(name.clone()));
    }

    // A layer may fan out to several drivers. rdl2 writes one file per
    // `RenderOutput`, so the first one names the file and the rest are
    // reported by the caller's limitations if this ever grows to handle
    // them.
    if let Some(driver) = drivers.first()
        && let Some(node) = scene.nodes.get(driver)
        && let Some(OwnedData::String(files)) =
            node.attrs.get("imagefilename").map(|arg| &arg.data)
        && let Some(file) = files.first()
    {
        object = object.set("file_name", Value::String(file.clone()));
    }

    object
}

/// The image resolution, from the first screen that carries one.
///
/// rdl2's own defaults, 1920x1080, when the scene does not say --
/// `SceneVariables::sImageWidth` and `sImageHeight`.
fn resolution(scene: &Scene) -> (i32, i32) {
    for output in scene.render_outputs() {
        if let Some(node) = scene.nodes.get(&output.screen)
            && let Some(OwnedData::I32(values)) =
                node.attrs.get("resolution").map(|arg| &arg.data)
            && values.len() >= 2
        {
            return (values[0], values[1]);
        }
    }

    (1920, 1080)
}

fn camera_reference(handle: &str) -> Reference {
    Reference::new(PERSPECTIVE_CAMERA, handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nsi_intermediate::OwnedArg;
    use nsi_trait::Type;

    fn arg(name: &str, type_tag: Type, data: OwnedData) -> OwnedArg {
        OwnedArg {
            name: name.to_string(),
            type_tag,
            array_length: 1,
            flags: 0,
            data,
        }
    }

    /// A triangle, a camera, a screen and an output -- the smallest
    /// scene that is a scene.
    fn triangle() -> Scene {
        let mut scene = Scene::default();

        scene.create("tri", "mesh");
        scene.set_attribute(
            "tri",
            vec![
                arg("nvertices", Type::I32, OwnedData::I32(vec![3])),
                arg("P.indices", Type::I32, OwnedData::I32(vec![0, 1, 2])),
                arg(
                    "P",
                    Type::Point,
                    OwnedData::F32(vec![
                        0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                    ]),
                ),
            ],
        );
        scene.connect("tri", None, ".root", "objects").unwrap();

        scene.create("cam", "perspectivecamera");
        scene.set_attribute(
            "cam",
            vec![arg("fov", Type::F32, OwnedData::F32(vec![45.0]))],
        );

        scene.create("screen", "screen");
        scene.set_attribute(
            "screen",
            vec![arg("resolution", Type::I32, OwnedData::I32(vec![320, 240]))],
        );
        scene.connect("screen", None, "cam", "screens").unwrap();

        scene.create("beauty", "outputlayer");
        scene.set_attribute(
            "beauty",
            vec![arg(
                "variablename",
                Type::String,
                OwnedData::String(vec!["Ci".to_string()]),
            )],
        );
        scene
            .connect("beauty", None, "screen", "outputlayers")
            .unwrap();

        scene.create("driver", "outputdriver");
        scene.set_attribute(
            "driver",
            vec![arg(
                "imagefilename",
                Type::String,
                OwnedData::String(vec!["beauty.exr".to_string()]),
            )],
        );
        scene
            .connect("driver", None, "beauty", "outputdrivers")
            .unwrap();

        scene
    }

    #[test]
    fn a_triangle_becomes_a_mesh_a_camera_and_an_output() {
        let flushed = flush(&triangle());
        let rdla = flushed.to_rdla();

        assert!(rdla.contains("RdlMeshGeometry(\"tri\") {"), "{rdla}");
        assert!(rdla.contains("[\"face_vertex_count\"] = { 3},"), "{rdla}");
        assert!(
            rdla.contains("[\"vertices_by_index\"] = { 0, 1, 2},"),
            "{rdla}"
        );
        assert!(
            rdla.contains(
                "[\"vertex_list_0\"] = { Vec3(0, 0, 0), Vec3(1, 0, 0), \
                 Vec3(0, 1, 0)},"
            ),
            "{rdla}"
        );
        assert!(rdla.contains("[\"image_width\"] = 320,"), "{rdla}");
        assert!(rdla.contains("[\"file_name\"] = \"beauty.exr\","), "{rdla}");
        assert!(rdla.contains("[\"channel_name\"] = \"Ci\","), "{rdla}");
        // Two things are missing from this scene and the flush names
        // both: it has no light, and its shape has no shader. A
        // correct scene that renders black otherwise looks like a bug
        // in this backend.
        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("renders black")),
            "{:?}",
            flushed.limitations
        );
        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("no ɴsɪ shader bound")),
            "{:?}",
            flushed.limitations
        );
    }

    /// `is_subd` defaults to true in MoonRay, so an ɴsɪ `mesh` that does
    /// not set it false renders as a subdivision surface.
    #[test]
    fn a_mesh_says_it_is_not_a_subdivision_surface() {
        let rdla = flush(&triangle()).to_rdla();
        assert!(rdla.contains("[\"is_subd\"] = false,"), "{rdla}");
    }

    #[test]
    fn a_subdivisionmesh_says_it_is_one() {
        let mut scene = triangle();
        scene.create("tri", "subdivisionmesh");

        let rdla = flush(&scene).to_rdla();
        assert!(rdla.contains("[\"is_subd\"] = true,"), "{rdla}");
    }

    /// The world transform is composed upstream and lands in
    /// `node_xform`.
    #[test]
    fn geometry_carries_its_world_transform() {
        let mut scene = triangle();

        scene.create("xf", "transform");
        #[rustfmt::skip]
        let matrix = vec![
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            1.0, 2.0, 3.0, 1.0,
        ];
        scene.set_attribute(
            "xf",
            vec![arg(
                "transformationmatrix",
                Type::MatrixF64,
                OwnedData::F64(matrix),
            )],
        );
        // The mesh hangs off the transform, not off the root: ɴsɪ's
        // transform chain is a chain, and a shape connected straight to
        // `.root` as well would stop the walk at the root.
        scene.disconnect("tri", None, ".root", "objects").unwrap();
        scene.connect("tri", None, "xf", "objects").unwrap();
        scene.connect("xf", None, ".root", "objects").unwrap();

        let rdla = flush(&scene).to_rdla();
        assert!(
            rdla.contains(
                "[\"node_xform\"] = Mat4(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, \
                 1, 2, 3, 1),"
            ),
            "{rdla}"
        );
    }

    /// A bound shader becomes MoonRay's stock PBR surface at its
    /// defaults, and the substitution is reported rather than passed
    /// off as a translation.
    #[test]
    fn a_bound_shader_becomes_the_default_surface() {
        let mut scene = triangle();

        scene.create("attr", "attributes");
        scene.create("shader", "shader");
        scene
            .connect("attr", None, "tri", "geometryattributes")
            .unwrap();
        scene
            .connect("shader", None, "attr", "surfaceshader")
            .unwrap();

        let flushed = flush(&scene);
        let rdla = flushed.to_rdla();

        // The material is the stand-in surface, and the row points at
        // it rather than leaving the shape unshaded.
        assert!(rdla.contains("UsdPreviewSurface(\"shader\") {"), "{rdla}");
        assert!(
            rdla.contains(
                "{RdlMeshGeometry(\"tri\"), \"\", \
                 UsdPreviewSurface(\"shader\"), undef()"
            ),
            "{rdla}"
        );
        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("none of the shader's parameters")),
            "{:?}",
            flushed.limitations
        );
    }

    /// Motion samples are reported as unhonoured rather than flattened
    /// into a sharp render without a word.
    #[test]
    fn motion_samples_are_reported() {
        let mut scene = triangle();
        scene.set_attribute_at_time(
            "tri",
            0.0,
            vec![arg("P", Type::Point, OwnedData::F32(vec![0.0, 0.0, 0.0]))],
        );

        let flushed = flush(&scene);
        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("renders sharp")),
            "{:?}",
            flushed.limitations
        );
    }

    /// 45 degrees vertical on a 4:3 frame: `24 / 2 * 0.75 / tan(22.5°)`.
    #[test]
    fn fov_becomes_a_focal_length() {
        let millimetres = focal(45.0, (320, 240));
        assert!((millimetres - 21.7279).abs() < 1e-3, "{millimetres} mm");
    }

    /// An `environment` becomes an `EnvLight`, and every assignment
    /// points at the set holding it -- a `Layer` row with no light set
    /// is lit by nothing.
    #[test]
    fn an_environment_lights_the_scene() {
        let mut scene = triangle();
        scene.create("env", "environment");

        let flushed = flush(&scene);
        let rdla = flushed.to_rdla();

        assert!(rdla.contains("EnvLight(\"env\") {"), "{rdla}");
        assert!(
            rdla.contains(
                "LightSet(\"/nsi/lights\") {\n    EnvLight(\"env\"),"
            ),
            "{rdla}"
        );
        assert!(
            rdla.contains(
                "{RdlMeshGeometry(\"tri\"), \"\", \
                 UsdPreviewSurface(\"/nsi/default_material\"), \
                 LightSet(\"/nsi/lights\")"
            ),
            "{rdla}"
        );
        assert!(
            !flushed
                .limitations
                .iter()
                .any(|line| line.contains("renders black")),
            "{:?}",
            flushed.limitations
        );
    }

    /// Geometry with nothing bound to it still gets a `Layer` row, and
    /// a material.
    ///
    /// Both halves were learned from the renderer rather than reasoned
    /// out: MoonRay renders what the layer names, and it skips a row
    /// whose material column is `undef()`. Either mistake produces a
    /// black image from a scene that looks entirely correct.
    #[test]
    fn unbound_geometry_gets_a_row_and_a_default_material() {
        let flushed = flush(&triangle());
        let rdla = flushed.to_rdla();

        assert!(
            rdla.contains(
                "{RdlMeshGeometry(\"tri\"), \"\", \
                 UsdPreviewSurface(\"/nsi/default_material\")"
            ),
            "{rdla}"
        );
        assert!(
            rdla.contains("UsdPreviewSurface(\"/nsi/default_material\") {"),
            "{rdla}"
        );
        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("no ɴsɪ shader bound")),
            "{:?}",
            flushed.limitations
        );
    }
}

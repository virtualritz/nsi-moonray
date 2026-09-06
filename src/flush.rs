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
use nsi_intermediate::{IDENTITY, Node, OwnedData, Scene};
use nsi_trait::Type;

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

    for (handle, node) in scene.nodes() {
        match node.node_type.as_str() {
            "mesh" | "subdivisionmesh" => {
                objects.push(mesh(scene, handle, &mut flushed));
                geometries.push(Reference::new(MESH, handle));

                // Every mesh gets a row, bound or not: MoonRay renders
                // what the `Layer` names, so geometry left out of it is
                // simply absent from the image.
                bindings.push((
                    handle.clone(),
                    material(scene, handle, &mut flushed),
                ));
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
                objects.push(shader(scene, handle, &mut flushed));
            }

            other => flushed.limitations.push(format!(
                "node {handle:?} of type {other:?} has no MoonRay mapping \
                 and was skipped"
            )),
        }
    }

    for output in scene.render_outputs() {
        for layer in &output.layers {
            objects.push(render_output(scene, &layer.handle, &layer.drivers));
        }

        // `SceneVariables`' own output file defaults to `scene.exr` in
        // the working directory, and MoonRay writes it whether or not a
        // `RenderOutput` names a file. Pointing it at the ɴsɪ output
        // driver's file is what keeps a stray `scene.exr` from
        // appearing next to whatever ran the render.
        if let Some(file) = output
            .layers
            .iter()
            .flat_map(|layer| layer.drivers.iter())
            .find_map(|driver| image_file(scene, driver))
        {
            variables = variables.set("output_file", Value::String(file));
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

/// Put an object's resolved world transform on it, as `node_xform`.
///
/// `world_transform` refuses rather than guesses: a cycle, a node that
/// never reaches `.root`, and a *prototype* under an `instances` node,
/// which has one matrix per instance and none of its own. Each of those
/// is reported and the object is left where it is, because ɴsɪ always
/// returns an image.
fn with_transform(
    object: Object,
    scene: &Scene,
    handle: &str,
    flushed: &mut Flushed,
) -> Object {
    // Motion first: a moving object's static transform is only one of
    // its samples, and taking it would be the silent flattening this
    // backend exists to avoid.
    let times = scene.motion_times(handle).unwrap_or_default();
    if times.len() >= 2 {
        return match blurred_transform(scene, handle, &times, flushed) {
            Some(object_with_blur) => {
                object.set("node_xform", object_with_blur)
            }
            None => object,
        };
    }

    match scene.world_transform(handle) {
        Ok(transform) if transform == IDENTITY => object,
        Ok(transform) => object.set("node_xform", Value::Mat4d(transform)),
        Err(error) => {
            flushed.limitations.push(format!(
                "{handle:?} has no single world transform ({error}); it is \
                 left untransformed"
            ));
            object
        }
    }
}

/// A moving transform, as rdl2's two-sample `blur(a, b)`.
///
/// **rdl2 has exactly two timesteps.** ɴsɪ has as many as the scene
/// sets, so anything past the first and last is dropped -- reported,
/// never silently. Upstream interpolates the way 3Delight does
/// (element-wise, holding the ends), so the two samples asked for here
/// are the renderer's own answer rather than this backend's arithmetic.
fn blurred_transform(
    scene: &Scene,
    handle: &str,
    times: &[f64],
    flushed: &mut Flushed,
) -> Option<Value> {
    let (first, last) = (times[0], times[times.len() - 1]);

    let begin = match scene.world_transform_interpolated_at(handle, first) {
        Ok(matrix) => matrix,
        Err(error) => {
            flushed.limitations.push(format!(
                "{handle:?} is motion sampled but has no transform at \
                 {first} ({error}); it renders sharp"
            ));
            return None;
        }
    };
    let end = match scene.world_transform_interpolated_at(handle, last) {
        Ok(matrix) => matrix,
        Err(error) => {
            flushed.limitations.push(format!(
                "{handle:?} is motion sampled but has no transform at \
                 {last} ({error}); it renders sharp"
            ));
            return None;
        }
    };

    if times.len() > 2 {
        flushed.limitations.push(format!(
            "{handle:?} carries {} motion samples and MoonRay takes two; \
             the transform is blurred between {first} and {last} and the \
             {} in between are dropped",
            times.len(),
            times.len() - 2
        ));
    }

    if begin == end && begin == IDENTITY {
        return None;
    }

    Some(Value::Blur(
        Box::new(Value::Mat4d(begin)),
        Box::new(Value::Mat4d(end)),
    ))
}

/// One `RdlMeshGeometry`, with its world transform baked in.
fn mesh(scene: &Scene, handle: &str, flushed: &mut Flushed) -> Object {
    let Some(node) = scene.node(handle) else {
        return Object::new(MESH, handle);
    };

    let mut object = Object::new(MESH, handle);

    object = with_transform(object, scene, handle, flushed);

    match node.effective("nvertices").map(|arg| &arg.data) {
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

    match node.effective("P.indices").map(|arg| &arg.data) {
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

    match node.effective("P").map(|arg| &arg.data) {
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

    // ɴsɪ marks a subdivision surface with an *attribute* on a `mesh`,
    // not with a node type: `subdivision.scheme`. Keying off the type
    // alone renders every subdivision surface faceted -- and silently,
    // since a polygon mesh of the same cage is a perfectly good render
    // of the wrong thing.
    // An ɴsɪ string is bytes, not `String`: the C API was handed
    // whatever the host had, and a file name need not be UTF-8. Only
    // the values compared against a known spelling are read as text.
    let scheme = match node.effective("subdivision.scheme").map(|a| &a.data) {
        Some(OwnedData::String(values)) => values
            .first()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned()),
        _ => None,
    };
    let subdivision = scheme.is_some() || node.node_type == "subdivisionmesh";

    // `is_subd` defaults to *true* in MoonRay, so a polygon mesh has to
    // say otherwise or it is subdivided anyway.
    object = object.set("is_subd", Value::Bool(subdivision));

    if let Some(scheme) = &scheme {
        // 0 is bilinear and 1 is catclark, per `RdlMesh`'s
        // `subd_scheme` enum. MoonRay has no other schemes.
        match scheme.as_str() {
            "catmull-clark" => {
                object = object.set("subd_scheme", Value::Int(1))
            }
            "bilinear" => object = object.set("subd_scheme", Value::Int(0)),
            other => flushed.limitations.push(format!(
                "mesh {handle:?} asks for subdivision scheme {other:?}, \
                 which MoonRay does not have; Catmull-Clark is used"
            )),
        }
    }

    if subdivision {
        object = creases(object, node);
    }

    // ɴsɪ says which way faces wind; MoonRay calls the same thing
    // orientation, where 1 is left-handed. Getting it backwards turns
    // every generated normal inside out.
    if let Some(OwnedData::I32(values)) =
        node.effective("clockwisewinding").map(|arg| &arg.data)
        && values.first().is_some_and(|winding| *winding != 0)
    {
        object = object.set("orientation", Value::Int(1));
    }

    object
}

/// Subdivision creases and corners, which ɴsɪ carries as four parallel
/// attributes and MoonRay as four of its own.
///
/// `subdivision.creasevertices` is a flat list of vertex index *pairs*,
/// one edge each, and `subd_crease_indices` is the same shape -- so this
/// is a rename rather than a conversion.
fn creases(mut object: Object, node: &Node) -> Object {
    for (from, to) in [
        ("subdivision.creasevertices", "subd_crease_indices"),
        ("subdivision.cornervertices", "subd_corner_indices"),
    ] {
        if let Some(OwnedData::I32(indices)) =
            node.effective(from).map(|arg| &arg.data)
        {
            object = object.set(
                to,
                Value::Vector(indices.iter().map(|i| Value::Int(*i)).collect()),
            );
        }
    }

    for (from, to) in [
        ("subdivision.creasesharpness", "subd_crease_sharpnesses"),
        ("subdivision.cornersharpness", "subd_corner_sharpnesses"),
    ] {
        if let Some(OwnedData::F32(values)) =
            node.effective(from).map(|arg| &arg.data)
        {
            object = object.set(
                to,
                Value::Vector(
                    values.iter().map(|v| Value::Float(*v)).collect(),
                ),
            );
        }
    }

    object
}

/// The material bound to one piece of geometry, if any.
///
/// The shader itself is substituted where it is declared; here it only
/// has to be pointed at. An `attributes` node carrying nothing but
/// visibility has no shader, and that row's material column stays
/// `undef()`.
fn material(
    scene: &Scene,
    handle: &str,
    flushed: &mut Flushed,
) -> Option<Reference> {
    match scene.geometry_binding(handle) {
        Ok(binding) => binding?
            .surface_shader
            .as_deref()
            .map(|shader| Reference::new(MATERIAL, shader)),
        Err(error) => {
            flushed.limitations.push(format!(
                "{handle:?} has no single material binding ({error}); it \
                 renders with the default surface"
            ));
            None
        }
    }
}

/// The parameters carried from an ɴsɪ shader into the substitute
/// surface, paired with the `UsdPreviewSurface` attribute each feeds.
///
/// Matched by **exact name only**. An ɴsɪ shader is an OSL shader and
/// its parameter names are whatever its author chose, so anything
/// cleverer than this is guesswork -- and a guessed name table that
/// silently maps the wrong parameter is worse than carrying nothing,
/// because the render looks plausible. Everything not on this list is
/// reported by name rather than dropped quietly.
const CARRIED: [(&str, &str); 6] = [
    ("diffuseColor", "diffuseColor"),
    ("emissiveColor", "emissiveColor"),
    ("roughness", "roughness"),
    ("metallic", "metallic"),
    ("ior", "ior"),
    ("opacity", "opacity"),
];

/// One ɴsɪ shader, as MoonRay's stock PBR surface.
///
/// MoonRay runs no OSL (`research.md` F6), so the shader itself cannot
/// cross. What crosses is a `UsdPreviewSurface` carrying the handful of
/// parameters that share a name with one of its attributes.
fn shader(scene: &Scene, handle: &str, flushed: &mut Flushed) -> Object {
    let mut object = Object::new(MATERIAL, handle);
    let Some(node) = scene.node(handle) else {
        return object;
    };

    let mut carried = Vec::new();
    for (from, to) in CARRIED {
        let Some(arg) = node.effective(from) else {
            continue;
        };

        let value = match &arg.data {
            OwnedData::F32(values)
                if arg.type_tag == Type::Color && values.len() >= 3 =>
            {
                Some(Value::Rgb([values[0], values[1], values[2]]))
            }
            OwnedData::F32(values) if values.len() == 1 => {
                Some(Value::Float(values[0]))
            }
            OwnedData::F64(values) if values.len() == 1 => {
                Some(Value::Float(values[0] as f32))
            }
            _ => None,
        };

        if let Some(value) = value {
            object = object.set(to, value);
            carried.push(from);
        }
    }

    let dropped: Vec<&str> = node
        .attrs
        .keys()
        .map(String::as_str)
        .filter(|name| !carried.contains(name))
        .collect();

    if dropped.is_empty() {
        flushed.limitations.push(format!(
            "shader {handle:?} is an OSL shader, which MoonRay cannot \
             run; a {MATERIAL} stands in for it"
        ));
    } else {
        flushed.limitations.push(format!(
            "shader {handle:?} is an OSL shader, which MoonRay cannot \
             run; a {MATERIAL} stands in for it and these parameters are \
             not carried: {}",
            dropped.join(", ")
        ));
    }

    object
}

/// One `EnvLight`.
///
/// ɴsɪ puts the environment's *look* in an OSL shader hanging off an
/// `attributes` node, and MoonRay cannot run it, so what crosses is the
/// light itself at its defaults -- white, intensity 1 -- and its
/// transform. That is enough to light a scene, which is the point.
fn environment(scene: &Scene, handle: &str, flushed: &mut Flushed) -> Object {
    let mut object = Object::new(ENVIRONMENT_LIGHT, handle);

    object = with_transform(object, scene, handle, flushed);

    if scene
        .geometry_binding(handle)
        .ok()
        .flatten()
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
    let Some(node) = scene.node(handle) else {
        return Object::new(PERSPECTIVE_CAMERA, handle);
    };

    let mut object = Object::new(PERSPECTIVE_CAMERA, handle);

    object = with_transform(object, scene, handle, flushed);

    match node.effective("fov").map(|arg| &arg.data) {
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

    if let Some(node) = scene.node(layer)
        && let Some(OwnedData::String(names)) =
            node.effective("variablename").map(|arg| &arg.data)
        && let Some(name) = names.first()
    {
        object = object.set(
            "channel_name",
            Value::String(String::from_utf8_lossy(name).into_owned()),
        );
    }

    // A layer may fan out to several drivers. rdl2 writes one file per
    // `RenderOutput`, so the first one names the file and the rest are
    // reported by the caller's limitations if this ever grows to handle
    // them.
    if let Some(driver) = drivers.first()
        && let Some(file) = image_file(scene, driver)
    {
        object = object.set("file_name", Value::String(file));
    }

    object
}

/// The file an ɴsɪ output driver writes to.
pub(crate) fn image_file(scene: &Scene, driver: &str) -> Option<String> {
    match &scene.node(driver)?.effective("imagefilename")?.data {
        OwnedData::String(files) => files
            .first()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned()),
        _ => None,
    }
}

/// The image resolution, from the first screen that carries one.
///
/// rdl2's own defaults, 1920x1080, when the scene does not say --
/// `SceneVariables::sImageWidth` and `sImageHeight`.
fn resolution(scene: &Scene) -> (i32, i32) {
    for output in scene.render_outputs() {
        if let Some(node) = scene.node(&output.screen)
            && let Some(OwnedData::I32(values)) =
                node.effective("resolution").map(|arg| &arg.data)
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

    /// A translation along X, as ɴsɪ stores a matrix: row-major, with
    /// the translation in the last row.
    fn translation(x: f64) -> OwnedArg {
        #[rustfmt::skip]
        let matrix = vec![
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
              x, 0.0, 0.0, 1.0,
        ];
        arg(
            "transformationmatrix",
            Type::MatrixF64,
            OwnedData::F64(matrix),
        )
    }

    fn arg(name: &str, type_tag: Type, data: OwnedData) -> OwnedArg {
        OwnedArg::new(name, type_tag, 1, 0, data)
    }

    /// A triangle, a camera, a screen and an output -- the smallest
    /// scene that is a scene.
    fn triangle() -> Scene {
        let mut scene = Scene::default();

        scene.create("tri", "mesh").expect("a recordable edit");
        scene
            .set_attribute(
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
            )
            .expect("a recordable edit");
        scene.connect("tri", None, ".root", "objects").unwrap();

        scene
            .create("cam", "perspectivecamera")
            .expect("a recordable edit");
        scene
            .set_attribute(
                "cam",
                vec![arg("fov", Type::F32, OwnedData::F32(vec![45.0]))],
            )
            .expect("a recordable edit");

        scene.create("screen", "screen").expect("a recordable edit");
        scene
            .set_attribute(
                "screen",
                vec![arg(
                    "resolution",
                    Type::I32,
                    OwnedData::I32(vec![320, 240]),
                )],
            )
            .expect("a recordable edit");
        scene.connect("screen", None, "cam", "screens").unwrap();

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
            .unwrap();

        scene
            .create("driver", "outputdriver")
            .expect("a recordable edit");
        scene
            .set_attribute(
                "driver",
                vec![arg(
                    "imagefilename",
                    Type::String,
                    OwnedData::String(vec![b"beauty.exr".to_vec()]),
                )],
            )
            .expect("a recordable edit");
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
        let mut scene = Scene::default();
        scene
            .create("subd", "subdivisionmesh")
            .expect("a fresh handle");

        let rdla = flush(&scene).to_rdla();
        assert!(rdla.contains("RdlMeshGeometry(\"subd\") {"), "{rdla}");
        assert!(rdla.contains("[\"is_subd\"] = true,"), "{rdla}");
    }

    /// ɴsɪ marks a subdivision surface with an **attribute on a mesh**,
    /// not a node type — `subdivision.scheme`. Keying off the type
    /// alone renders every subdivision surface as its faceted cage,
    /// which looks like a plausible render of the wrong thing.
    #[test]
    fn subdivision_scheme_on_a_mesh_makes_it_a_subdivision_surface() {
        let mut scene = triangle();
        scene
            .set_attribute(
                "tri",
                vec![arg(
                    "subdivision.scheme",
                    Type::String,
                    OwnedData::String(vec![b"catmull-clark".to_vec()]),
                )],
            )
            .expect("a recordable edit");

        let rdla = flush(&scene).to_rdla();
        assert!(rdla.contains("[\"is_subd\"] = true,"), "{rdla}");
        assert!(rdla.contains("[\"subd_scheme\"] = 1,"), "{rdla}");
    }

    /// Creases and corners are four parallel ɴsɪ attributes and four
    /// MoonRay ones of the same shape: index pairs and a sharpness
    /// each.
    #[test]
    fn creases_and_corners_cross() {
        let mut scene = triangle();
        scene
            .set_attribute(
                "tri",
                vec![
                    arg(
                        "subdivision.scheme",
                        Type::String,
                        OwnedData::String(vec![b"catmull-clark".to_vec()]),
                    ),
                    arg(
                        "subdivision.creasevertices",
                        Type::I32,
                        OwnedData::I32(vec![0, 1, 1, 2]),
                    ),
                    arg(
                        "subdivision.creasesharpness",
                        Type::F32,
                        OwnedData::F32(vec![2.5, 2.5]),
                    ),
                    arg(
                        "subdivision.cornervertices",
                        Type::I32,
                        OwnedData::I32(vec![2]),
                    ),
                    arg(
                        "subdivision.cornersharpness",
                        Type::F32,
                        OwnedData::F32(vec![10.0]),
                    ),
                ],
            )
            .expect("a recordable edit");

        let rdla = flush(&scene).to_rdla();
        assert!(
            rdla.contains("[\"subd_crease_indices\"] = { 0, 1, 1, 2},"),
            "{rdla}"
        );
        assert!(
            rdla.contains("[\"subd_crease_sharpnesses\"] = { 2.5, 2.5},"),
            "{rdla}"
        );
        assert!(rdla.contains("[\"subd_corner_indices\"] = { 2},"), "{rdla}");
        assert!(
            rdla.contains("[\"subd_corner_sharpnesses\"] = { 10},"),
            "{rdla}"
        );
    }

    /// ɴsɪ's winding is MoonRay's orientation, and getting it backwards
    /// turns every generated normal inside out.
    #[test]
    fn clockwise_winding_is_left_handed_orientation() {
        let mut scene = triangle();
        scene
            .set_attribute(
                "tri",
                vec![arg(
                    "clockwisewinding",
                    Type::I32,
                    OwnedData::I32(vec![1]),
                )],
            )
            .expect("a recordable edit");

        let rdla = flush(&scene).to_rdla();
        assert!(rdla.contains("[\"orientation\"] = 1,"), "{rdla}");
    }

    /// The world transform is composed upstream and lands in
    /// `node_xform`.
    #[test]
    fn geometry_carries_its_world_transform() {
        let mut scene = triangle();

        scene.create("xf", "transform").expect("a recordable edit");
        #[rustfmt::skip]
        let matrix = vec![
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            1.0, 2.0, 3.0, 1.0,
        ];
        scene
            .set_attribute(
                "xf",
                vec![arg(
                    "transformationmatrix",
                    Type::MatrixF64,
                    OwnedData::F64(matrix),
                )],
            )
            .expect("a recordable edit");
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

        scene
            .create("attr", "attributes")
            .expect("a recordable edit");
        scene.create("shader", "shader").expect("a recordable edit");
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
                .any(|line| line.contains("stands in for it")),
            "{:?}",
            flushed.limitations
        );
    }

    /// A moving transform becomes rdl2's two-sample `blur(a, b)`.
    ///
    /// This is the capability that distinguishes this backend from the
    /// Mitsuba one, which cannot blur at all.
    #[test]
    fn a_moving_transform_blurs() {
        let mut scene = triangle();
        scene
            .disconnect("tri", None, ".root", "objects")
            .expect("connected");
        scene.create("xf", "transform").expect("a fresh handle");
        scene
            .connect("tri", None, "xf", "objects")
            .expect("known attribute");
        scene
            .connect("xf", None, ".root", "objects")
            .expect("known attribute");

        for (time, x) in [(0.0, 0.0), (1.0, 5.0)] {
            scene
                .set_attribute_at_time("xf", time, vec![translation(x)])
                .expect("a recordable edit");
        }

        let rdla = flush(&scene).to_rdla();
        assert!(
            rdla.contains(
                "[\"node_xform\"] = blur(Mat4(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, \
                 1, 0, 0, 0, 0, 1), Mat4(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, \
                 0, 5, 0, 0, 1))"
            ),
            "{rdla}"
        );
    }

    /// rdl2 has exactly two timesteps. ɴsɪ has as many as the scene
    /// sets, and dropping the rest quietly is the failure this reports.
    #[test]
    fn more_than_two_motion_samples_are_reported() {
        let mut scene = triangle();
        scene
            .disconnect("tri", None, ".root", "objects")
            .expect("connected");
        scene.create("xf", "transform").expect("a fresh handle");
        scene
            .connect("tri", None, "xf", "objects")
            .expect("known attribute");
        scene
            .connect("xf", None, ".root", "objects")
            .expect("known attribute");

        for (time, x) in [(0.0, 0.0), (0.5, 1.0), (1.0, 5.0)] {
            scene
                .set_attribute_at_time("xf", time, vec![translation(x)])
                .expect("a recordable edit");
        }

        let flushed = flush(&scene);
        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("MoonRay takes two")),
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
        scene
            .create("env", "environment")
            .expect("a recordable edit");

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

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

/// MoonRay's instancer, whose DSO is
/// `moonray/dso/geometry/RdlInstancerGeometry`.
///
/// Instancing is native on both sides and neither needs persuading
/// (`research.md` F9): this declares `references`, `xform_list`,
/// `ref_indices` and nesting to five levels, and `nsi-intermediate`
/// already resolves ɴsɪ's `instances` node into that exact shape.
/// Expanding instances into separate objects here would throw away the
/// memory win that is the whole point of both.
const INSTANCER: &str = "RdlInstancerGeometry";

/// `RdlInstancerGeometry`'s `method` for reading whole matrices.
///
/// `0` takes decomposed `positions`/`orientations`/`scales`; `2` takes
/// `xform_list`. ɴsɪ hands over 4x4s, so decomposing them here only to
/// have MoonRay recompose them would be a lossy round trip for nothing.
const XFORM_LIST: i32 = 2;

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

/// The camera a scene without one gets.
///
/// Not a nicety either. MoonRay's `RenderContext::initialize` does
/// `initActiveCamera(getActiveCameras()[0])`, and `getActiveCameras`
/// returns an **empty vector** for a scene holding no camera --
/// indexing which is undefined behaviour, not the `KeyError` its own
/// `catch` is waiting for. The process dies with a SIGSEGV, and in a
/// renderer loaded by `dlopen` it takes the host application with it
/// (`002` `research.md` F5).
///
/// Emitting one is the better answer than refusing the scene, and it
/// is the one MoonRay itself intends: `getActiveCameras` already falls
/// back to whichever camera was created first when the scene variables
/// name none. It only has nothing to fall back *to*. So: ɴsɪ always
/// returns an image, and a scene with no camera gets a view from the
/// origin down `-Z` rather than a crash. The substitution is reported.
const DEFAULT_CAMERA: &str = "/nsi/default_camera";

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

/// Every way MoonRay can see a piece of geometry.
///
/// Read from `scene_rdl2/lib/scene/rdl2/Geometry.cc` rather than
/// guessed: setting only `visible_in_camera` leaves a shape casting
/// shadows and appearing in reflections, which looks like a lighting
/// bug rather than a visibility one.
///
/// All nine are declared `FLAGS_GEOM_RELOAD_BVH_ONLY` (`002`
/// `research.md` F3), which is the point -- turning geometry off this
/// way costs an accelerator rebuild rather than a re-tessellation.
const VISIBILITY: [&str; 9] = [
    "visible_in_camera",
    "visible_shadow",
    "visible_diffuse_reflection",
    "visible_diffuse_transmission",
    "visible_glossy_reflection",
    "visible_glossy_transmission",
    "visible_mirror_reflection",
    "visible_mirror_transmission",
    "visible_volume",
];

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
    // The rdl2 *class* travels with the handle. A `Layer` row names an
    // object by class and name both, so assuming `RdlMeshGeometry` for
    // every row puts an instancer in the layer under a class it does
    // not have -- a row that looks right and names nothing.
    let mut bindings: Vec<(&'static str, String, Option<Reference>)> =
        Vec::new();
    let mut objects = Vec::new();
    // Handles of the instancers seen, so a prototype can be told from
    // an ordinary shape after the walk -- a prototype is drawn by its
    // instancer and must not also be drawn on its own.
    let mut instancers: Vec<String> = Vec::new();
    // A scene with none gets one, because MoonRay crashes rather than
    // complains. See `DEFAULT_CAMERA`.
    let mut cameras = 0usize;

    let resolution = resolution(scene);
    // One interval for the whole scene: MoonRay has two global
    // timesteps, not two per object. See `shutter`.
    let shutter = shutter(scene);

    // Which instancer, if any, places each prototype. Built before the
    // walk because node order says nothing: a prototype is commonly
    // recorded before the `instances` node that places it, and a
    // prototype's transform has to be resolved *relative to* its
    // instancer rather than to the world.
    let prototypes = prototypes(scene);

    for (handle, node) in scene.nodes() {
        match node.node_type.as_str() {
            "mesh" | "subdivisionmesh" => {
                let shape = mesh(
                    scene,
                    handle,
                    prototypes.get(handle).map(String::as_str),
                    shutter,
                    &mut flushed,
                );
                // A prototype does not reach `.root` and is not
                // detached: its instancer is what places it.
                let placed = prototypes.contains_key(handle);
                objects.push(if !placed && detached(scene, handle) {
                    hidden(shape)
                } else {
                    shape
                });
                geometries.push(Reference::new(MESH, handle));

                // Every mesh gets a row, bound or not: MoonRay renders
                // what the `Layer` names, so geometry left out of it is
                // simply absent from the image.
                bindings.push((
                    MESH,
                    handle.clone(),
                    material(scene, handle, &mut flushed),
                ));
            }

            "perspectivecamera" => {
                objects.push(camera(
                    scene,
                    handle,
                    resolution,
                    shutter,
                    &mut flushed,
                ));
                cameras += 1;
            }

            "environment" => {
                objects.push(environment(scene, handle, shutter, &mut flushed));
                lights.push(Reference::new(ENVIRONMENT_LIGHT, handle));
            }

            "instances" => {
                if let Some(object) =
                    instancer(scene, handle, shutter, &mut flushed)
                {
                    objects.push(if detached(scene, handle) {
                        hidden(object)
                    } else {
                        object
                    });
                    geometries.push(Reference::new(INSTANCER, handle));
                    // An instancer needs a `Layer` row like any other
                    // geometry: MoonRay renders what the `Layer` names,
                    // and a row with no material is skipped outright.
                    // Its material is the one bound to the instancer,
                    // if any -- the prototypes carry their own.
                    bindings.push((
                        INSTANCER,
                        handle.clone(),
                        material(scene, handle, &mut flushed),
                    ));
                    instancers.push(handle.clone());
                }
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

    if cameras == 0 {
        // At the origin looking down `-Z`, which is where ɴsɪ's own
        // identity transform points a camera, with a plain 45-degree
        // field of view. Arbitrary, and said so rather than implied.
        objects.push(
            Object::new(PERSPECTIVE_CAMERA, DEFAULT_CAMERA)
                .set("focal", Value::Float(focal(45.0, resolution))),
        );
        variables = variables.set(
            "camera",
            Value::Object(Reference::new(PERSPECTIVE_CAMERA, DEFAULT_CAMERA)),
        );
        flushed.limitations.push(format!(
            "no ɴsɪ camera reached the scene, so a default \
             {PERSPECTIVE_CAMERA} at the origin looking down -Z was \
             added; MoonRay reads cameras[0] of an empty list and \
             would otherwise crash rather than report"
        ));
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
        .map(|(class, handle, material)| {
            let material = material.unwrap_or_else(|| {
                unshaded += 1;
                Reference::new(MATERIAL, DEFAULT_MATERIAL)
            });

            Assignment::new(
                Reference::new(class, &handle),
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

    if let Some([open, close]) = shutter {
        // The two timesteps every `blur(a, b)` in this scene is
        // evaluated at. Without this MoonRay keeps its `{-1, 0}`
        // default and blurs over an interval that has nothing to do
        // with the one the values were sampled at -- which still
        // *looks* like motion blur, of the wrong length.
        variables = variables.set(
            "motion_steps",
            Value::Vector(vec![
                Value::Float(open as f32),
                Value::Float(close as f32),
            ]),
        );
    }

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

/// Which instancer places each prototype.
///
/// A prototype shared by two instancers is ambiguous -- upstream says
/// so, and there is no single relative transform for it -- so the
/// first instancer wins and the collision is reported by
/// [`with_prototype_transform`], which is where the transform it
/// affects is chosen.
fn prototypes(scene: &Scene) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();

    for (handle, node) in scene.nodes() {
        if node.node_type == "instances" {
            for source in scene.instance_sources(handle) {
                // The connection may name a transform above the
                // geometry; the prototype is what is under it.
                if let Some(geometry) = prototype_geometry(scene, &source) {
                    map.entry(geometry).or_insert_with(|| handle.clone());
                }
            }
        }
    }

    map
}

/// A prototype's transform, resolved against its instancer.
///
/// `world_transform` *refuses* for a prototype
/// ([`ResolveError::Instanced`]) and is right to: the `instances` node
/// holds one matrix per instance and none that belongs to the
/// prototype, so any single world transform would put every instance
/// in one place. What the prototype does have is the chain from itself
/// up to the instancer, which is what MoonRay applies through
/// `use_reference_xforms`.
fn with_prototype_transform(
    object: Object,
    scene: &Scene,
    handle: &str,
    instancer: &str,
    flushed: &mut Flushed,
) -> Object {
    match scene.relative_transform(handle, instancer) {
        Ok(transform) if transform == IDENTITY => object,
        Ok(transform) => object.set("node_xform", Value::Mat4d(transform)),
        Err(error) => {
            flushed.limitations.push(format!(
                "prototype {handle:?} of instancer {instancer:?} was left \
                 at identity: {error}"
            ));
            object
        }
    }
}

/// The geometry an `instances` node's `sourcemodels` connection names.
///
/// **A `sourcemodels` edge need not point at geometry.** ɴsɪ connects
/// the *model root*, which is commonly a `transform` with the geometry
/// under it -- that is how a prototype gets a placement of its own
/// relative to the instancer. MoonRay's `references` takes `Geometry`
/// objects, so the transform has to be descended through.
///
/// Getting this wrong is quiet: `references` names an object that does
/// not exist, the whole attribute fails to set, and **nothing renders**
/// while the scene itself is perfectly valid.
///
/// The transform on the way down is not lost -- it comes back as
/// `relative_transform(geometry, instancer)` in
/// [`with_prototype_transform`], which composes the whole chain.
///
/// Returns `None` for a subtree holding no geometry, and the *first*
/// geometry for one holding several: MoonRay's `references` is one
/// `Geometry` per entry, and a subtree of many would need a group it
/// has no way to express. The caller reports both.
fn prototype_geometry(scene: &Scene, source: &str) -> Option<String> {
    const GEOMETRY: [&str; 3] = ["mesh", "subdivisionmesh", "instances"];

    let node = scene.node(source)?;
    if GEOMETRY.contains(&node.node_type.as_str()) {
        return Some(source.to_string());
    }

    // Breadth first, so the shallowest geometry wins and the answer
    // does not depend on how deep an unrelated branch goes.
    let mut queue = std::collections::VecDeque::from([source.to_string()]);
    let mut seen = std::collections::HashSet::new();

    while let Some(handle) = queue.pop_front() {
        if !seen.insert(handle.clone()) {
            continue;
        }
        for edge in scene.edges_to_attr(&handle, "objects") {
            let Some(child) = scene.node(&edge.from) else {
                continue;
            };
            if GEOMETRY.contains(&child.node_type.as_str()) {
                return Some(edge.from.clone());
            }
            queue.push_back(edge.from.clone());
        }
    }

    None
}

/// The rdl2 class an ɴsɪ geometry node becomes.
///
/// An instancing prototype can itself be an instancer -- ɴsɪ nests
/// `instances` under `instances`, and MoonRay's `instance_level` goes
/// to four -- so a reference is not always a mesh.
fn geometry_class(scene: &Scene, handle: &str) -> &'static str {
    match scene.node(handle).map(|node| node.node_type.as_str()) {
        Some("instances") => INSTANCER,
        _ => MESH,
    }
}

/// An ɴsɪ `instances` node as a `RdlInstancerGeometry`.
///
/// Both sides model instancing directly and upstream has already done
/// the resolving (`research.md` F9), so this is a transcription rather
/// than a translation: `instance_sources` are the `references`,
/// `Instance::transform` fills `xform_list`, and `Instance::source`
/// fills `ref_indices`.
///
/// # What MoonRay does by itself, and what it does not
///
/// Read from `rt/GeometryManager.cc`, because both halves are the kind
/// of thing a careful guess gets backwards:
///
/// - **A prototype is not drawn on its own, automatically.**
///   `fillGenerateList` walks `references` recursively and everything
///   it reaches below the top level is generated in *local* space and
///   promoted to a shared primitive: "this geometry won't show". So a
///   prototype needs no excluding from the `GeometrySet`, and the
///   recursion is also what makes nesting work without help.
/// - **A prototype still needs its `Layer` row.** The shaders the
///   instanced copies use are looked up through the layer's
///   `GeometryToRootShadersMap` and hoisted onto the instancer; a
///   prototype missing from the layer takes the "referenced geometry
///   has no shaders" path and its copies render unshaded. Excluding
///   the prototype to stop it drawing twice -- the obvious move -- is
///   exactly how to lose the material while the image still appears.
///
/// Because the referenced geometry is generated at identity, its own
/// transform reaches the instance only through `use_reference_xforms`,
/// which is why that is set here alongside the prototype's
/// `relative_transform`.
fn instancer(
    scene: &Scene,
    handle: &str,
    shutter: Option<[f64; 2]>,
    flushed: &mut Flushed,
) -> Option<Object> {
    let sources = scene.instance_sources(handle);
    if sources.is_empty() {
        flushed.limitations.push(format!(
            "instancer {handle:?} has no `sourcemodels` connected and \
             places nothing"
        ));
        return None;
    }

    // A moving instancer is ɴsɪ-legal and 3Delight renders it, but
    // `xform_list` is one list and rdl2 has two timesteps. Take the
    // shutter-open sample and *say so*: a crowd frozen at t0 is a
    // reduction, and reporting it is what separates this from the
    // silent flattening the backend exists to avoid.
    let instances = match scene.instance_transforms(handle) {
        Ok(instances) => instances,
        Err(nsi_intermediate::ResolveError::MotionSampledTransform {
            ..
        }) => {
            let time = scene
                .motion_times(handle)
                .unwrap_or_default()
                .first()
                .copied()
                .unwrap_or(0.0);
            flushed.limitations.push(format!(
                "instancer {handle:?} moves; its instance transforms were \
                 taken at time {time} because `xform_list` is a single \
                 list. Per-instance motion blur needs `velocities`"
            ));
            match scene.instance_transforms_at(handle, time) {
                Ok(instances) => instances,
                Err(error) => {
                    flushed.limitations.push(format!(
                        "instancer {handle:?} places nothing: {error}"
                    ));
                    return None;
                }
            }
        }
        Err(error) => {
            flushed
                .limitations
                .push(format!("instancer {handle:?} places nothing: {error}"));
            return None;
        }
    };

    if instances.is_empty() {
        flushed.limitations.push(format!(
            "instancer {handle:?} has prototypes but no instance \
             transforms, so it places nothing"
        ));
        return None;
    }

    // Each `sourcemodels` connection resolved to the geometry it
    // names, which may be under a transform.
    let mut prototypes = Vec::with_capacity(sources.len());
    for source in &sources {
        match prototype_geometry(scene, source) {
            Some(geometry) => prototypes.push(geometry),
            None => flushed.limitations.push(format!(
                "instancer {handle:?} has a `sourcemodels` connection to                  {source:?}, which holds no geometry; that prototype                  places nothing"
            )),
        }
    }

    if prototypes.len() != sources.len() {
        // The indices upstream resolved are positions in `sources`, and
        // dropping one would silently renumber every instance after it
        // onto the wrong prototype.
        flushed.limitations.push(format!(
            "instancer {handle:?} places nothing: {} of its {}              prototypes hold no geometry, and dropping one would              renumber the rest onto the wrong models",
            sources.len() - prototypes.len(),
            sources.len()
        ));
        return None;
    }

    let references = Value::Vector(
        prototypes
            .iter()
            .map(|geometry| {
                Value::Object(Reference::new(
                    geometry_class(scene, geometry),
                    geometry,
                ))
            })
            .collect(),
    );

    let object = Object::new(INSTANCER, handle)
        .set("references", references)
        .set("method", Value::Int(XFORM_LIST))
        .set(
            "xform_list",
            Value::Vector(
                instances
                    .iter()
                    .map(|instance| Value::Mat4d(instance.transform))
                    .collect(),
            ),
        )
        // Written even when every instance draws source 0, where
        // MoonRay would default to it: the list is what says the
        // pairing was resolved rather than assumed, and upstream
        // resolves it against each connection's `index` attribute
        // rather than against connection order.
        .set(
            "ref_indices",
            Value::Vector(
                instances
                    .iter()
                    .map(|instance| Value::Int(instance.source as i32))
                    .collect(),
            ),
        )
        // The prototype is generated at identity, so without this its
        // own transform below the instancer is simply lost.
        .set("use_reference_xforms", Value::Bool(true));

    Some(with_transform(object, scene, handle, shutter, flushed))
}

/// Put an object's resolved world transform on it, as `node_xform`.
///
/// `world_transform` refuses rather than guesses: a cycle, a node that
/// never reaches `.root`, and a *prototype* under an `instances` node,
/// which has one matrix per instance and none of its own. Each of those
/// is reported and the object is left where it is, because ɴsɪ always
/// returns an image.
/// The one interval every blurred attribute is sampled over.
///
/// **MoonRay has two global timesteps, not two per object.** Every
/// `blur(a, b)` in the scene is evaluated at the same pair, so
/// sampling each node over *its own* recorded times renders a shape
/// that moved between `t=10` and `t=11` as though it had moved during
/// another shape's shutter. Two objects moving over different ranges
/// come out with the same smear, which looks like motion blur working.
///
/// ɴsɪ's answer is the camera's `shutterrange`. Without one, the union
/// of every recorded motion time is the honest fallback: it covers all
/// the motion the scene describes, and upstream's
/// `world_transform_interpolated_at` holds the ends outside a node's
/// own samples, so a node that stopped moving early stays still for
/// the rest of the shutter rather than being extrapolated.
///
/// `None` when nothing moves, which is the ordinary case.
fn shutter(scene: &Scene) -> Option<[f64; 2]> {
    for (handle, node) in scene.nodes() {
        if node.node_type != "perspectivecamera" {
            continue;
        }
        if let Some(OwnedData::F64(values)) =
            node.effective("shutterrange").map(|arg| &arg.data)
            && values.len() >= 2
            && values[0] < values[1]
        {
            return Some([values[0], values[1]]);
        }
        let _ = handle;
    }

    // No shutter: take everything that moves.
    let mut span: Option<[f64; 2]> = None;
    let mut widen = |times: &[f64]| {
        if times.len() < 2 {
            return;
        }
        let (first, last) = (times[0], times[times.len() - 1]);
        span = Some(match span {
            Some([low, high]) => [low.min(first), high.max(last)],
            None => [first, last],
        });
    };

    for (handle, _) in scene.nodes() {
        widen(&scene.motion_times(handle).unwrap_or_default());
        if let Ok(samples) = scene.attribute_samples(handle, "P") {
            let times: Vec<f64> =
                samples.iter().map(|(time, _)| *time).collect();
            widen(&times);
        }
    }

    span
}

/// A mesh's two deformation samples, if it has any.
///
/// **`P` sampled over time is deformation blur.** rdl2 carries it as
/// two separate attributes -- `vertex_list_0` and `vertex_list_1` --
/// rather than as a `blur()` pair, which is why this returns them
/// separately instead of going through [`Value::Blur`].
///
/// `None` for a mesh whose `P` was set once, which is the ordinary
/// case: there is nothing to blur and `vertex_list_1` is left unset.
///
/// More than two samples cannot be carried -- rdl2 has exactly two
/// timesteps -- so the first and last are taken and the reduction is
/// *reported*. Keeping the ends rather than the first two is what
/// preserves the extent of the motion, which is what a smear looks
/// like; quietly keeping the first two would shorten every blur in the
/// scene and look like a shutter setting.
fn deformation(
    scene: &Scene,
    handle: &str,
    shutter: Option<[f64; 2]>,
    flushed: &mut Flushed,
) -> Option<(Value, Value)> {
    let samples = scene.attribute_samples(handle, "P").ok()?;

    let recorded: Vec<(f64, &[f32])> = samples
        .iter()
        .filter_map(|(time, argument)| match &argument.data {
            OwnedData::F32(values) if values.len() % 3 == 0 => {
                Some((*time, values.as_slice()))
            }
            _ => None,
        })
        .collect();

    if recorded.len() < 2 {
        return None;
    }

    // A mesh whose vertex count changes between samples is not
    // deforming, and nothing can interpolate it.
    let width = recorded[0].1.len();
    if let Some((time, other)) =
        recorded.iter().find(|(_, values)| values.len() != width)
    {
        flushed.limitations.push(format!(
            "mesh {handle:?} has {} vertices at time {} and {} at \
             {time}; that is not deformation and cannot be blurred, so \
             the first sample is used",
            width / 3,
            recorded[0].0,
            other.len() / 3
        ));
        let first = points_of(recorded[0].1);
        return Some((first.clone(), first));
    }

    // The *scene's* interval, for the reason transforms use it too:
    // MoonRay evaluates one global pair of timesteps, so a mesh
    // sampled over its own range deforms during somebody else's
    // shutter.
    let [open, close] =
        shutter.unwrap_or([recorded[0].0, recorded[recorded.len() - 1].0]);

    if recorded.len() > 2 {
        flushed.limitations.push(format!(
            "mesh {handle:?} has {} motion samples on \"P\"; rdl2 has two \
             timesteps, so it was resampled to the shutter and the \
             intermediate shapes were lost",
            recorded.len()
        ));
    }

    Some((
        points_of(&sampled_at(&recorded, open)),
        points_of(&sampled_at(&recorded, close)),
    ))
}

/// A flat `P` buffer as a vector of points.
fn points_of(values: &[f32]) -> Value {
    Value::Vector(
        values
            .chunks_exact(3)
            .map(|p| Value::Vec3f([p[0], p[1], p[2]]))
            .collect(),
    )
}

/// `P` at an arbitrary time, interpolated element-wise between the
/// bracketing samples and **held** outside them.
///
/// The same policy upstream documents for transforms
/// (`world_transform_interpolated_at`), applied to vertices for the
/// same reason: with one global pair of timesteps, a mesh whose
/// samples do not reach the shutter's ends has to answer for them
/// somehow, and holding is the answer that does not invent motion. A
/// mesh that stopped deforming early stays put for the rest of the
/// shutter rather than being flung onwards by extrapolation.
fn sampled_at(recorded: &[(f64, &[f32])], time: f64) -> Vec<f32> {
    let first = recorded[0];
    let last = recorded[recorded.len() - 1];

    if time <= first.0 {
        return first.1.to_vec();
    }
    if time >= last.0 {
        return last.1.to_vec();
    }

    let after = recorded
        .iter()
        .position(|(sample, _)| *sample >= time)
        .unwrap_or(recorded.len() - 1)
        .max(1);
    let (before_time, before) = recorded[after - 1];
    let (after_time, values) = recorded[after];

    let span = after_time - before_time;
    // Two samples recorded at the same time: the later one wins, which
    // is what a repeated `SetAttributeAtTime` means.
    if span <= 0.0 {
        return values.to_vec();
    }
    let alpha = ((time - before_time) / span) as f32;

    before
        .iter()
        .zip(values)
        .map(|(a, b)| a + (b - a) * alpha)
        .collect()
}

/// Turn a shape off, every way MoonRay can see one.
///
/// **ɴsɪ turns geometry off by severing its `objects` connection**, and
/// that is what an application does when a layer is hidden. The flush
/// walks every recorded node rather than only the reachable ones, so
/// without this a detached shape keeps rendering -- and at identity,
/// since `world_transform` refuses for it. It is the quietest kind of
/// wrong: the scene is correct, the render succeeds, and a shape that
/// should be gone is sitting at the origin.
///
/// Turned off rather than left out on purpose (`002` `research.md`
/// F3). Omitting it would make an interactive disconnect a change of
/// *membership* -- the `GeometrySet` and the `Layer` -- which is a
/// structural edit and forces a full re-apply. Writing the visibility
/// flags instead keeps the scene's shape constant, so the same edit is
/// nine attribute writes on an object MoonRay already has, and costs
/// an accelerator rebuild rather than a re-tessellation.
///
/// The cost of that choice is a first flush that hands MoonRay
/// geometry it will never draw. `T7.1`.
fn hidden(object: Object) -> Object {
    VISIBILITY.iter().fold(object, |object, attribute| {
        object.set(*attribute, Value::Bool(false))
    })
}

/// Whether a node reaches `.root`, and so is in the scene at all.
///
/// A prototype under an `instances` node is *in* the scene without
/// reaching `.root` directly -- upstream answers `Instanced` rather
/// than `Detached` for it, which is the distinction that keeps a crowd
/// from being hidden wholesale.
fn detached(scene: &Scene, handle: &str) -> bool {
    matches!(
        scene.world_transform(handle),
        Err(nsi_intermediate::ResolveError::Detached { .. })
    )
}

fn with_transform(
    object: Object,
    scene: &Scene,
    handle: &str,
    shutter: Option<[f64; 2]>,
    flushed: &mut Flushed,
) -> Object {
    // Motion first: a moving object's static transform is only one of
    // its samples, and taking it would be the silent flattening this
    // backend exists to avoid.
    let times = scene.motion_times(handle).unwrap_or_default();
    if times.len() >= 2
        && let Some(shutter) = shutter
    {
        return match blurred_transform(scene, handle, &times, shutter, flushed)
        {
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
    shutter: [f64; 2],
    flushed: &mut Flushed,
) -> Option<Value> {
    // The *scene's* interval, not this node's. MoonRay evaluates every
    // `blur(a, b)` at one global pair of timesteps, so a node sampled
    // over its own range would be rendered as though it moved during
    // somebody else's shutter. Upstream holds the ends outside a
    // node's own samples, so one that stops moving early stays still.
    let [first, last] = shutter;

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
fn mesh(
    scene: &Scene,
    handle: &str,
    prototype_of: Option<&str>,
    shutter: Option<[f64; 2]>,
    flushed: &mut Flushed,
) -> Object {
    let Some(node) = scene.node(handle) else {
        return Object::new(MESH, handle);
    };

    let mut object = Object::new(MESH, handle);

    object = match prototype_of {
        Some(instancer) => {
            with_prototype_transform(object, scene, handle, instancer, flushed)
        }
        None => with_transform(object, scene, handle, shutter, flushed),
    };

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

    // Deformation blur first: `P` sampled over time is `vertex_list_0`
    // and `vertex_list_1`, and reading the static value of a moving
    // mesh would be the silent flattening this backend exists to
    // avoid. `T2.3`.
    let deformed = deformation(scene, handle, shutter, flushed);
    if let Some((begin, end)) = deformed {
        object = object.set("vertex_list_0", begin).set("vertex_list_1", end);
    } else {
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
    }

    // ɴsɪ marks a subdivision surface with an *attribute* on a `mesh`,
    // not with a node type: `subdivision.scheme`. Keying off the type
    // alone renders every subdivision surface faceted -- and silently,
    // since a polygon mesh of the same cage is a perfectly good render
    // of the wrong thing.
    // An ɴsɪ string is recorded as bytes even though the spec calls
    // it UTF-8, so reading one as text is a lossy conversion by
    // choice: a malformed name should render with a mangled name, not
    // fail to render.
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
fn environment(
    scene: &Scene,
    handle: &str,
    shutter: Option<[f64; 2]>,
    flushed: &mut Flushed,
) -> Object {
    let mut object = Object::new(ENVIRONMENT_LIGHT, handle);

    object = with_transform(object, scene, handle, shutter, flushed);

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
    shutter: Option<[f64; 2]>,
    flushed: &mut Flushed,
) -> Object {
    let Some(node) = scene.node(handle) else {
        return Object::new(PERSPECTIVE_CAMERA, handle);
    };

    let mut object = Object::new(PERSPECTIVE_CAMERA, handle);

    object = with_transform(object, scene, handle, shutter, flushed);

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

    /// One instance matrix, translated along X.
    fn instance_matrix(x: f64) -> Vec<f64> {
        #[rustfmt::skip]
        let matrix = vec![
            1.0, 0.0, 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
              x, 0.0, 0.0, 1.0,
        ];
        matrix
    }

    /// **Every blurred value is sampled over one interval.**
    ///
    /// MoonRay has two *global* timesteps, not two per object. A node
    /// sampled over its own range would be rendered as though it had
    /// moved during another node's shutter, and two objects moving
    /// over different ranges would come out with the same smear --
    /// which looks like motion blur working.
    #[test]
    fn two_objects_share_one_shutter() {
        let mut scene = triangle();

        // One moves over [0, 1].
        scene
            .create("early", "transform")
            .expect("a recordable edit");
        scene.connect("early", None, ".root", "objects").unwrap();
        for (time, x) in [(0.0, 0.0), (1.0, 2.0)] {
            scene
                .set_attribute_at_time("early", time, vec![translation(x)])
                .expect("a recordable edit");
        }
        scene.create("a", "mesh").expect("a recordable edit");
        scene.connect("a", None, "early", "objects").unwrap();

        // The other over [2, 4], which no shared pair of timesteps
        // could describe if each were sampled over its own range.
        scene
            .create("late", "transform")
            .expect("a recordable edit");
        scene.connect("late", None, ".root", "objects").unwrap();
        for (time, x) in [(2.0, 10.0), (4.0, 12.0)] {
            scene
                .set_attribute_at_time("late", time, vec![translation(x)])
                .expect("a recordable edit");
        }
        scene.create("b", "mesh").expect("a recordable edit");
        scene.connect("b", None, "late", "objects").unwrap();

        let rdla = flush(&scene).to_rdla();

        // The union is [0, 4], and MoonRay is told so.
        assert!(
            rdla.contains("[\"motion_steps\"] = { 0, 4}"),
            "the scene's timesteps must span all of its motion\n{rdla}"
        );

        // `b` is held at its ends outside its own samples, so at t=0 it
        // is already at 10 and at t=4 it is at 12 -- not extrapolated
        // backwards to somewhere it never was.
        assert!(
            rdla.contains("blur(Mat4(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 10, 0, 0, 1), Mat4(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 12, 0, 0, 1))"),
            "the later-moving shape is held at its ends, not \
             extrapolated\n{rdla}"
        );
    }

    /// A camera's `shutterrange` decides the interval when it has one.
    #[test]
    fn a_shutter_range_beats_the_union() {
        let mut scene = triangle();
        scene
            .set_attribute(
                "cam",
                vec![arg(
                    "shutterrange",
                    Type::F64,
                    OwnedData::F64(vec![0.25, 0.75]),
                )],
            )
            .expect("a recordable edit");

        scene.create("xf", "transform").expect("a recordable edit");
        scene.connect("xf", None, ".root", "objects").unwrap();
        for (time, x) in [(0.0, 0.0), (1.0, 4.0)] {
            scene
                .set_attribute_at_time("xf", time, vec![translation(x)])
                .expect("a recordable edit");
        }
        scene.create("m", "mesh").expect("a recordable edit");
        scene.connect("m", None, "xf", "objects").unwrap();

        let rdla = flush(&scene).to_rdla();

        assert!(
            rdla.contains("[\"motion_steps\"] = { 0.25, 0.75}"),
            "the camera's shutter decides\n{rdla}"
        );
        // A quarter and three quarters of the way along a 0->4 move.
        assert!(
            rdla.contains(
                "Mat4(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1)"
            ) && rdla.contains(
                "Mat4(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 3, 0, 0, 1)"
            ),
            "the transform must be interpolated to the shutter's \
             ends\n{rdla}"
        );
    }

    /// **`T2.3`.** `P` sampled over time becomes two vertex lists.
    ///
    /// rdl2 carries deformation as `vertex_list_0` and
    /// `vertex_list_1`, not as a `blur()` pair -- the oracle's
    /// `blur(a, b)` form is for scalars and matrices.
    #[test]
    fn a_deforming_mesh_gets_two_vertex_lists() {
        let mut scene = triangle();
        for (time, y) in [(0.0, 1.0f32), (1.0, 3.0f32)] {
            scene
                .set_attribute_at_time(
                    "tri",
                    time,
                    vec![arg(
                        "P",
                        Type::Point,
                        OwnedData::F32(vec![
                            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, y, 0.0,
                        ]),
                    )],
                )
                .expect("a recordable edit");
        }

        let rdla = flush(&scene).to_rdla();

        assert!(
            rdla.contains("[\"vertex_list_0\"] = { Vec3(0, 0, 0), Vec3(1, 0, 0), Vec3(0, 1, 0)}"),
            "{rdla}"
        );
        assert!(
            rdla.contains("[\"vertex_list_1\"] = { Vec3(0, 0, 0), Vec3(1, 0, 0), Vec3(0, 3, 0)}"),
            "the second sample must be its own attribute\n{rdla}"
        );
    }

    /// Deformation is resampled onto the scene's shutter, like every
    /// other blurred value.
    ///
    /// A mesh sampled over `[0, 1]` in a scene whose shutter is
    /// `[0.25, 0.75]` must hand MoonRay the shape at those two times,
    /// not at its own — there is one global pair of timesteps and
    /// every blurred value is read at them.
    #[test]
    fn deformation_is_resampled_onto_the_shutter() {
        let mut scene = triangle();
        scene
            .set_attribute(
                "cam",
                vec![arg(
                    "shutterrange",
                    Type::F64,
                    OwnedData::F64(vec![0.25, 0.75]),
                )],
            )
            .expect("a recordable edit");

        // One vertex travels from y=0 to y=4 across [0, 1].
        for (time, y) in [(0.0, 0.0f32), (1.0, 4.0f32)] {
            scene
                .set_attribute_at_time(
                    "tri",
                    time,
                    vec![arg(
                        "P",
                        Type::Point,
                        OwnedData::F32(vec![
                            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, y, 0.0,
                        ]),
                    )],
                )
                .expect("a recordable edit");
        }

        let rdla = flush(&scene).to_rdla();

        // A quarter and three quarters along, so y = 1 and y = 3.
        assert!(
            rdla.contains("[\"vertex_list_0\"] = { Vec3(0, 0, 0), Vec3(1, 0, 0), Vec3(0, 1, 0)}"),
            "the shutter opens a quarter of the way along\n{rdla}"
        );
        assert!(
            rdla.contains("[\"vertex_list_1\"] = { Vec3(0, 0, 0), Vec3(1, 0, 0), Vec3(0, 3, 0)}"),
            "and closes three quarters along\n{rdla}"
        );
    }

    /// A mesh whose `P` was set once has no second list.
    #[test]
    fn a_static_mesh_has_one_vertex_list() {
        let rdla = flush(&triangle()).to_rdla();
        assert!(rdla.contains("vertex_list_0"), "{rdla}");
        assert!(!rdla.contains("vertex_list_1"), "{rdla}");
    }

    /// More than two samples is a reduction, and it is reported.
    #[test]
    fn more_than_two_deformation_samples_are_reported() {
        let mut scene = triangle();
        for (time, y) in [(0.0, 1.0f32), (0.5, 2.0), (1.0, 3.0)] {
            scene
                .set_attribute_at_time(
                    "tri",
                    time,
                    vec![arg(
                        "P",
                        Type::Point,
                        OwnedData::F32(vec![
                            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, y, 0.0,
                        ]),
                    )],
                )
                .expect("a recordable edit");
        }

        let flushed = flush(&scene);

        // The *ends*, so the extent of the motion survives. Keeping
        // the first two would shorten every blur in the scene.
        assert!(
            flushed.to_rdla().contains("Vec3(0, 3, 0)"),
            "{}",
            flushed.to_rdla()
        );
        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("3 motion samples")),
            "{:?}",
            flushed.limitations
        );
    }

    /// A changing vertex count is not deformation, and MoonRay cannot
    /// interpolate it.
    #[test]
    fn a_changing_vertex_count_is_reported_not_blurred() {
        let mut scene = triangle();
        scene
            .set_attribute_at_time(
                "tri",
                0.0,
                vec![arg(
                    "P",
                    Type::Point,
                    OwnedData::F32(vec![
                        0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                    ]),
                )],
            )
            .expect("a recordable edit");
        scene
            .set_attribute_at_time(
                "tri",
                1.0,
                vec![arg(
                    "P",
                    Type::Point,
                    OwnedData::F32(vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
                )],
            )
            .expect("a recordable edit");

        let flushed = flush(&scene);

        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("not deformation")),
            "{:?}",
            flushed.limitations
        );
    }

    /// **A shape disconnected from `.root` is not in the scene.**
    ///
    /// ɴsɪ's way of turning geometry off is to sever its `objects`
    /// connection, and it is what an application does when a layer is
    /// hidden. The flush walks every recorded node rather than only
    /// the reachable ones, so without this a detached shape keeps
    /// rendering -- and at identity, since `world_transform` refuses
    /// for it.
    #[test]
    fn a_detached_shape_is_not_rendered() {
        let mut scene = triangle();
        scene
            .disconnect("tri", None, ".root", "objects")
            .expect("a recordable edit");

        let flushed = flush(&scene);
        let rdla = flushed.to_rdla();

        // It keeps its row and its place in the set, and is turned
        // *off*: that keeps the scene's shape constant, so an
        // interactive disconnect is an attribute edit rather than a
        // change of membership. `002` `research.md` F3.
        assert!(rdla.contains("[\"visible_in_camera\"] = false"), "{rdla}");
        assert!(rdla.contains("[\"visible_shadow\"] = false"), "{rdla}");
        assert!(
            rdla.contains("[\"visible_mirror_reflection\"] = false"),
            "every way of seeing it must be off, or it casts shadows \
             and appears in reflections -- which reads as a lighting \
             bug, not a visibility one\n{rdla}"
        );
    }

    /// A shape that *is* connected is not turned off.
    #[test]
    fn a_connected_shape_keeps_its_visibility() {
        let rdla = flush(&triangle()).to_rdla();
        assert!(!rdla.contains("visible_in_camera"), "{rdla}");
    }

    /// A scene with no camera still gets one.
    ///
    /// Not cosmetic: MoonRay's `initialize` indexes `[0]` of an empty
    /// camera list, which is undefined behaviour rather than the error
    /// its own `catch` expects, and the process dies. A camera-less
    /// ɴsɪ scene is legal to record, so this is the difference between
    /// an image with a note and a crashed host application.
    #[test]
    fn a_scene_with_no_camera_gets_a_default_one() {
        let mut scene = Scene::default();
        scene.create("tri", "mesh").expect("a recordable edit");
        scene.connect("tri", None, ".root", "objects").unwrap();

        let flushed = flush(&scene);
        let rdla = flushed.to_rdla();

        assert!(
            rdla.contains("PerspectiveCamera(\"/nsi/default_camera\")"),
            "{rdla}"
        );
        assert!(
            rdla.contains(
                "[\"camera\"] = PerspectiveCamera(\"/nsi/default_camera\")"
            ),
            "the scene variables must name it, or MoonRay has nothing to \
             fall back to\n{rdla}"
        );
        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("no ɴsɪ camera")),
            "the substitution must be reported: {:?}",
            flushed.limitations
        );
    }

    /// A scene that has a camera does not get a second one.
    #[test]
    fn a_scene_with_a_camera_keeps_only_its_own() {
        let rdla = flush(&triangle()).to_rdla();

        assert!(!rdla.contains("/nsi/default_camera"), "{rdla}");
        assert!(rdla.contains("PerspectiveCamera(\"cam\")"), "{rdla}");
    }

    /// A prototype mesh under an instancer that places it twice.
    fn two_instances() -> Scene {
        let mut scene = triangle();

        scene
            .create("inst", "instances")
            .expect("a recordable edit");
        scene.connect("inst", None, ".root", "objects").unwrap();

        scene.create("proto", "mesh").expect("a recordable edit");
        scene
            .set_attribute(
                "proto",
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
        scene
            .connect("proto", None, "inst", "sourcemodels")
            .unwrap();

        let mut matrices = instance_matrix(10.0);
        matrices.extend(instance_matrix(20.0));
        scene
            .set_attribute(
                "inst",
                vec![arg(
                    "transformationmatrices",
                    Type::MatrixF64,
                    OwnedData::F64(matrices),
                )],
            )
            .expect("a recordable edit");

        scene
    }

    /// The mapping: `sourcemodels` to `references`, one matrix per
    /// instance to `xform_list`, and the method that reads it.
    #[test]
    fn an_instances_node_becomes_an_instancer() {
        let flushed = flush(&two_instances());
        let rdla = flushed.to_rdla();

        assert!(rdla.contains("RdlInstancerGeometry(\"inst\") {"), "{rdla}");
        assert!(
            rdla.contains("[\"references\"] = { RdlMeshGeometry(\"proto\")}"),
            "{rdla}"
        );
        // `method` 2 is "xform list"; 0 would read
        // positions/orientations/scales, which are not written.
        assert!(rdla.contains("[\"method\"] = 2"), "{rdla}");
        assert!(rdla.contains("[\"xform_list\"] = { Mat4("), "{rdla}");
        assert!(rdla.contains("[\"use_reference_xforms\"] = true"), "{rdla}");
    }

    /// **`T6.5`.** The point of instancing is that the prototype exists
    /// once. A flattened scene renders the same image, so an image test
    /// cannot catch this and only counting can.
    #[test]
    fn a_prototype_is_referenced_once_not_expanded() {
        let flushed = flush(&two_instances());
        let rdla = flushed.to_rdla();

        let meshes = rdla.matches("RdlMeshGeometry(\"proto\") {").count();
        assert_eq!(
            meshes, 1,
            "the prototype must be declared once however many instances \
             draw it -- expanding it throws away the whole point.\n{rdla}"
        );

        // Two instances, one prototype: the matrices live in the
        // instancer, not in two copies of the mesh.
        let instancers =
            rdla.matches("RdlInstancerGeometry(\"inst\") {").count();
        assert_eq!(instancers, 1, "{rdla}");
    }

    /// A prototype needs its `Layer` row like any other geometry.
    ///
    /// MoonRay looks the instanced copies' shaders up through the
    /// layer's `GeometryToRootShadersMap` and hoists them onto the
    /// instancer (`rt/GeometryManager.cc`), so a prototype left out of
    /// the layer renders unshaded -- while `fillGenerateList` is what
    /// stops it *also* drawing on its own, with no help from here.
    #[test]
    fn a_prototype_keeps_its_layer_row() {
        let rdla = flush(&two_instances()).to_rdla();

        assert!(
            rdla.contains("RdlMeshGeometry(\"proto\")")
                && rdla.contains("Layer(\"/nsi/layer\")"),
            "{rdla}"
        );
        // The instancer is named in the layer too, or MoonRay renders
        // nothing it places -- **by class and name**. Asserting only
        // that the handle appears is what let a row reading
        // `RdlMeshGeometry("inst")` through: it looks right, and names
        // an object that does not exist.
        let layer = rdla
            .split("Layer(\"/nsi/layer\") {")
            .nth(1)
            .expect("a layer");
        assert!(
            layer.contains("RdlInstancerGeometry(\"inst\")"),
            "the instancer's row must name its own class\n{layer}"
        );
        assert!(layer.contains("RdlMeshGeometry(\"proto\")"), "{layer}");
    }

    /// An instancer with prototypes but no matrices places nothing, and
    /// says so rather than emitting an instancer that draws air.
    #[test]
    fn an_instancer_with_no_matrices_is_reported() {
        let mut scene = triangle();
        scene
            .create("inst", "instances")
            .expect("a recordable edit");
        scene.connect("inst", None, ".root", "objects").unwrap();
        scene.create("proto", "mesh").expect("a recordable edit");
        scene
            .connect("proto", None, "inst", "sourcemodels")
            .unwrap();

        let flushed = flush(&scene);

        assert!(
            !flushed.to_rdla().contains("RdlInstancerGeometry"),
            "{}",
            flushed.to_rdla()
        );
        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("places nothing")),
            "{:?}",
            flushed.limitations
        );
    }
}

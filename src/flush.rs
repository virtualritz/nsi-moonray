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
    name::Name,
    value::{Reference, Value},
};
use nsi_intermediate::{
    EdgeKind, IDENTITY, Node, OwnedArgument, OwnedData, Scene,
};
use nsi_trait::Type;
use std::collections::HashSet;

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

/// The camera classes, by the node type each comes from.
///
/// Four of the interface's five have a MoonRay counterpart and the
/// mapping is a rename; the fifth, `cylindricalcamera`, has none.
/// Getting this wrong is the quiet kind of wrong -- a scene shot
/// through an orthographic camera rendered in perspective is a
/// perfectly good image of the wrong thing -- which is why the class
/// travels with the handle rather than being assumed at the reference.
const CAMERAS: [(&str, &str); 4] = [
    ("perspectivecamera", PERSPECTIVE_CAMERA),
    ("orthographiccamera", "OrthographicCamera"),
    ("fisheyecamera", "FisheyeCamera"),
    ("sphericalcamera", "SphericalCamera"),
];

/// Whether a node type is one of the interface's cameras.
///
/// The specification is explicit that "all camera nodes share a set of
/// common attributes", `shutterrange` among them, so anything reading
/// one has to accept every camera type rather than the perspective one
/// -- an orthographic shot whose shutter is ignored still blurs, over
/// whatever interval the scene's motion happens to span.
///
/// `cylindricalcamera` counts here even though [`CAMERAS`] has no class
/// for it. The shutter is a property of the scene, and it stays right
/// when the projection is the thing MoonRay cannot do.
fn is_camera(node_type: &str) -> bool {
    node_type == "cylindricalcamera"
        || CAMERAS.iter().any(|(nsi, _)| *nsi == node_type)
}

/// The MoonRay class one camera node becomes.
fn camera_class(scene: &Scene, handle: &str) -> &'static str {
    scene
        .node(handle)
        .and_then(|node| {
            CAMERAS
                .iter()
                .find(|(nsi, _)| *nsi == node.node_type())
                .map(|(_, class)| *class)
        })
        .unwrap_or(PERSPECTIVE_CAMERA)
}

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

/// The material an ɴsɪ shader becomes when OSL is running.
///
/// Built by this repository -- `dso/osl/` -- rather than shipped with
/// MoonRay, so it has to be on MoonRay's DSO path for a scene naming
/// it to load at all.
const OSL_MATERIAL: &str = "Osl";

/// The root shader an ɴsɪ `displacementshader` becomes.
///
/// Built by this repository beside the material, from the same shading
/// system: the group is identical, the *usage* is not. There is no
/// substitute for it, so without OSL a displacement is reported and
/// dropped -- moving vertices is not something a stand-in surface can
/// approximate.
const OSL_DISPLACEMENT: &str = "OslDisplacement";

/// The class one ɴsɪ primitive variable becomes.
///
/// MoonRay declares a geometry's attributes statically like everything
/// else in rdl2, so an attribute nobody knew about in advance travels
/// as a `UserData` object in the mesh's `primitive_attributes`.
const USER_DATA: &str = "UserData";

/// What a `volume` node becomes.
///
/// The interface's `volume` node is *defined* as OpenVDB -- a file and
/// a set of named grids, and nothing else -- and `VdbGeometry` is
/// MoonRay's only volume geometry, so this is one of the closer
/// mappings in this backend.
const VOLUME: &str = "VdbGeometry";

/// The volume shader a `VdbGeometry` is rendered with.
///
/// MoonRay's stock one, standing in for the OSL volume shader the
/// interface binds through `volumeshader` -- which needs a
/// `VolumeShader` root of its own, whose four separate virtuals
/// (extinction, albedo, emission, anisotropy) do not fit OSL's one
/// execution. A `Layer` row with no volume shader renders nothing at
/// all, so the substitute is what makes a volume appear.
const VOLUME_SHADER: &str = "VdbVolume";

/// The one `VdbVolume` every volume in the scene is rendered with.
const DEFAULT_VOLUME_SHADER: &str = "/nsi/volume_shader";

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

/// The frame rate written into the scene.
///
/// rdl2's own default, and written rather than relied on: MoonRay
/// converts a per-instance velocity to a position offset with
/// `dt = (motionStep - evaluationFrame) / fps`, so an instancer's
/// velocities are only correct if this backend and the renderer agree
/// on `fps`. Emitting it makes them agree by construction instead of
/// by assumption.
const FPS: f32 = 24.0;

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
/// What the flushed scene is for.
///
/// The one thing it decides is what happens to geometry that does not
/// reach `.root` -- which ɴsɪ uses to mean "hidden", and which a
/// viewport toggles constantly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Purpose {
    /// A scene that will be edited: hidden geometry is **kept**, with
    /// its visibility off.
    ///
    /// That keeps the scene's shape constant, so hiding and showing a
    /// shape is nine attribute writes on an object MoonRay already
    /// has -- an accelerator rebuild rather than a re-tessellation
    /// (`002` `research.md` F3) -- instead of a change of set and
    /// layer membership, which is structural and forces a full
    /// re-apply.
    #[default]
    Interactive,
    /// A scene that will be rendered once: hidden geometry is **left
    /// out**.
    ///
    /// Nothing will toggle it, so carrying it costs a tessellation and
    /// a place in the accelerator for something that will never be
    /// drawn.
    Batch,
}

/// How an ɴsɪ shader crosses.
///
/// ɴsɪ *is* OSL, so the honest answer is [`Shading::Osl`] and the
/// other one is a stand-in. Which is the default depends on whether
/// this crate was built with an OSL to build the `Osl` material DSO
/// against -- `$OSL_ROOT`, the way `$MOONRAY_ROOT` is for the renderer
/// -- because a scene naming a class the renderer cannot load renders
/// nothing at all.
///
/// It is a *choice* rather than a `cfg` at the point of use, because
/// the flush is a pure transformation: a scene dumped by `mnry cat` on
/// a machine with no OSL and rendered on a farm that has one should
/// say `Osl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shading {
    /// The ɴsɪ shader network becomes an `Osl` material carrying an OSL
    /// group specification, and MoonRay runs it. `src/osl.rs`.
    #[cfg_attr(osl, default)]
    Osl,
    /// Every shader becomes a `UsdPreviewSurface` carrying the
    /// parameters that shader is known to have. What this crate did
    /// before OSL, and what a build without one still does.
    #[cfg_attr(not(osl), default)]
    Substitute,
}

pub fn flush(scene: &Scene) -> Flushed {
    flush_for(scene, Purpose::default())
}

/// Flush a recorded scene for a particular [`Purpose`].
pub fn flush_for(scene: &Scene, purpose: Purpose) -> Flushed {
    flush_with(scene, purpose, Shading::default())
}

/// Flush a recorded scene, saying both what it is for and how its
/// shaders should cross.
pub fn flush_with(
    scene: &Scene,
    purpose: Purpose,
    shading: Shading,
) -> Flushed {
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
    // Handles borrow from the scene now rather than being copied:
    // upstream interns them, and a flush that cloned every one back
    // into a `String` would hand that saving straight back.
    let mut bindings: Vec<(
        &'static str,
        &str,
        Option<Reference>,
        Option<Reference>,
    )> = Vec::new();
    let mut objects = Vec::new();
    // Handles of the instancers seen, so a prototype can be told from
    // an ordinary shape after the walk -- a prototype is drawn by its
    // instancer and must not also be drawn on its own.
    let mut instancers: Vec<&str> = Vec::new();
    // Handles of the volumes seen, so their layer rows can be given a
    // volume shader rather than a material after the walk.
    let mut volumes: Vec<&str> = Vec::new();
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
    // Which shaders are bound in which slot. Built from the edges
    // rather than per geometry because the *class* a shader node
    // becomes is a property of the shader, and the node walk reaches it
    // in whatever order the scene was recorded in.
    let (surfaces, displaces) = shader_roles(scene);

    for (handle, node) in scene.nodes() {
        match node.node_type() {
            "mesh" | "subdivisionmesh" => {
                // ɴsɪ has no light nodes: a mesh wearing an emitter
                // *is* the light (`LIGHTS`). Checked before anything
                // else, because a light is not also a shape.
                if let Some(shader) = surface_shader(scene, handle)
                    && let Some(class) = light_class(scene, &shader)
                {
                    let mut emitter = light(
                        scene,
                        handle,
                        &shader,
                        class,
                        shutter,
                        &mut flushed,
                    );
                    let dark = detached(scene, handle);
                    if dark {
                        emitter = switched_off(emitter);
                    }
                    lights.push(Reference::new(class, light_handle(handle)));

                    if class == MESH_LIGHT && !dark {
                        // A `MeshLight` reads its shape from a
                        // `Geometry` that must **not** be in the main
                        // `Layer`: `RenderContext::createMeshLightLayer`
                        // builds a layer of its own for it and warns
                        // and skips the light otherwise. So the mesh is
                        // emitted, and left out of both the layer and
                        // the geometry set.
                        let (shape, data) = mesh(
                            scene,
                            handle,
                            prototypes.get(handle).copied(),
                            shutter,
                            &mut flushed,
                        );
                        objects.extend(data);
                        objects.push(shape);
                        flushed.limitations.push(format!(
                            "{handle:?} is a {MESH_LIGHT}'s geometry and so \
                             is not in the render layer, which MoonRay \
                             refuses; the light is forced visible in \
                             camera instead, so it is seen as well as \
                             sampled, but it cannot also wear a material"
                        ));
                    }

                    objects.push(emitter);
                    continue;
                }

                let (shape, data) = mesh(
                    scene,
                    handle,
                    prototypes.get(handle).copied(),
                    shutter,
                    &mut flushed,
                );
                objects.extend(data);
                // A prototype does not reach `.root` and is not
                // detached: its instancer is what places it.
                let placed = prototypes.contains_key(handle);
                if !placed && detached(scene, handle) {
                    if purpose == Purpose::Batch {
                        // Nothing will show it again, so it is not
                        // worth tessellating.
                        continue;
                    }
                    objects.push(hidden(shape));
                } else {
                    objects.push(shape);
                }
                geometries.push(Reference::new(MESH, handle));

                // Every mesh gets a row, bound or not: MoonRay renders
                // what the `Layer` names, so geometry left out of it is
                // simply absent from the image.
                bindings.push((
                    MESH,
                    handle,
                    material(scene, handle, shading, &mut flushed),
                    displacement(scene, handle, shading, &mut flushed),
                ));
            }

            "volume" => {
                objects.push(volume(scene, handle, shutter, &mut flushed));
                geometries.push(Reference::new(VOLUME, handle));
                // A volume's row carries a *volume shader* rather than
                // a material, and the two columns are not
                // interchangeable: MoonRay reads the volume through the
                // sixth and would render nothing from the third.
                bindings.push((VOLUME, handle, None, None));
                report_volume_shader(scene, handle, &mut flushed);
                volumes.push(handle);
            }

            "vdbparticles" => flushed.limitations.push(format!(
                "{handle:?} is a `vdbparticles` node; MoonRay has no \
                 point-cloud geometry that reads an OpenVDB \
                 `PointDataGrid`, and it was skipped"
            )),

            "cylindricalcamera" => flushed.limitations.push(format!(
                "{handle:?} is a `cylindricalcamera`; MoonRay has no \
                 cylindrical projection, and the camera was skipped"
            )),

            "perspectivecamera" | "orthographiccamera" | "fisheyecamera"
            | "sphericalcamera" => {
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
                let light = environment(scene, handle, shutter, &mut flushed);
                // A light disconnected from `.root` lights nothing.
                // Left in its set and switched off, for the same reason
                // a detached shape is left in the layer and hidden.
                if detached(scene, handle) {
                    if purpose == Purpose::Batch {
                        continue;
                    }
                    objects.push(switched_off(light));
                } else {
                    objects.push(light);
                }
                lights.push(Reference::new(ENVIRONMENT_LIGHT, handle));
            }

            "instances" => {
                if let Some(object) =
                    instancer(scene, handle, shutter, &mut flushed)
                {
                    if detached(scene, handle) {
                        if purpose == Purpose::Batch {
                            continue;
                        }
                        objects.push(hidden(object));
                    } else {
                        objects.push(object);
                    }
                    geometries.push(Reference::new(INSTANCER, handle));
                    // An instancer needs a `Layer` row like any other
                    // geometry: MoonRay renders what the `Layer` names,
                    // and a row with no material is skipped outright.
                    // Its material is the one bound to the instancer,
                    // if any -- the prototypes carry their own.
                    bindings.push((
                        INSTANCER,
                        handle,
                        material(scene, handle, shading, &mut flushed),
                        displacement(scene, handle, shading, &mut flushed),
                    ));
                    instancers.push(handle);
                }
            }

            // Resolved away upstream, or carried by another node.
            "transform" | "attributes" | "screen" | "root" => {}

            "outputdriver" | "outputlayer" => {}

            "shader" => {
                // rdl2 names are unique across classes -- creating a
                // second object under a name another class already
                // holds is an error, not a shadowing -- so a shader
                // becomes *one* object and its binding decides which.
                if shading == Shading::Osl && displaces.contains(&handle) {
                    if surfaces.contains(&handle) {
                        flushed.limitations.push(format!(
                            "shader {handle:?} is bound as both a surface                              and a displacement shader; it crossed as the                              surface, because a MoonRay object has one                              class and one name"
                        ));
                        objects.push(shader(
                            scene,
                            handle,
                            shading,
                            &mut flushed,
                        ));
                    } else {
                        objects.push(osl_displacement(
                            scene,
                            handle,
                            &mut flushed,
                        ));
                    }
                }
                // An emitter is carried by the light it makes, not by
                // a stand-in surface nothing references. With OSL
                // running the question is answered by *executing* the
                // shader rather than by recognising its name, so the
                // check only applies to the substitute.
                else if shading == Shading::Osl
                    || light_class(scene, handle).is_none()
                {
                    objects.push(shader(scene, handle, shading, &mut flushed));
                }
            }

            other => flushed.limitations.push(format!(
                "node {handle:?} of type {other:?} has no MoonRay mapping \
                 and was skipped"
            )),
        }
    }

    for output in scene.render_outputs() {
        for layer in &output.layers {
            objects.push(render_output(
                scene,
                &layer.handle,
                &layer.drivers,
                &mut flushed,
            ));
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

        variables = variables.set(
            "camera",
            Value::Object(camera_reference(scene, &output.camera)),
        );
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
             black; an `environment` node becomes one, as does geometry \
             wearing an emitter this backend recognises by name"
                .to_string(),
        );
        None
    } else {
        objects.push(Object {
            class: Name::new("LightSet"),
            name: Some(Name::new(LIGHT_SET)),
            body: Body::Set(lights),
        });
        Some(Reference::new("LightSet", LIGHT_SET))
    };

    let mut unshaded = 0;
    let mut volumes_shaded = false;
    let assignments = bindings
        .into_iter()
        .map(|(class, handle, material, displacement)| {
            // **A volume's row is shaded through the sixth column, not
            // the third.** MoonRay reads a volume through its
            // `VolumeShader` and a material there does nothing; the row
            // still needs one, or the volume renders as nothing at all.
            if class == VOLUME {
                volumes_shaded = true;
                return Assignment {
                    volume_shader: Some(Reference::new(
                        VOLUME_SHADER,
                        DEFAULT_VOLUME_SHADER,
                    )),
                    ..Assignment::new(
                        Reference::new(class, handle),
                        None,
                        light_set.clone(),
                    )
                };
            }

            let material = material.unwrap_or_else(|| {
                unshaded += 1;
                Reference::new(MATERIAL, DEFAULT_MATERIAL)
            });

            Assignment {
                displacement,
                ..Assignment::new(
                    Reference::new(class, handle),
                    Some(material),
                    light_set.clone(),
                )
            }
        })
        .collect();

    if volumes_shaded {
        objects.push(Object::new(VOLUME_SHADER, DEFAULT_VOLUME_SHADER));
    }

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
            class: Name::new("GeometrySet"),
            name: Some(Name::new("/nsi/geometries")),
            body: Body::Set(geometries),
        });
    }

    objects.push(Object {
        class: Name::new("Layer"),
        name: Some(Name::new("/nsi/layer")),
        body: Body::Layer(assignments),
    });

    if let Some([open, close]) = shutter {
        // See `FPS`: the instancer velocity conversion depends on this
        // and on `motion_steps` being the two below.
        variables = variables.set("fps", Value::Float(FPS));
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
fn prototypes(scene: &Scene) -> std::collections::HashMap<String, &str> {
    let mut map = std::collections::HashMap::new();

    for (handle, node) in scene.nodes() {
        if node.node_type() == "instances" {
            for source in scene.instance_sources(handle) {
                // The connection may name a transform above the
                // geometry; the prototype is what is under it.
                if let Some(geometry) = prototype_geometry(scene, &source) {
                    map.entry(geometry).or_insert(handle);
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
    if GEOMETRY.contains(&node.node_type()) {
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
        for edge in scene.edges_to_attribute(&handle, "objects") {
            let Some(child) = scene.node(edge.from()) else {
                continue;
            };
            if GEOMETRY.contains(&child.node_type()) {
                return Some(edge.from().to_owned());
            }
            queue.push_back(edge.from().to_owned());
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
    match scene.node(handle).map(|node| node.node_type()) {
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

    // A moving instancer, or a still one. `xform_list` is **not
    // blurrable** -- declared with no flags, and `FLAGS_BLURRABLE` is
    // what carries two timesteps (`research.md` F10) -- so ɴsɪ's
    // sampled `transformationmatrices` cannot cross as a `blur()`
    // pair. MoonRay's route is `velocities`, one vector per instance,
    // applied as `position + velocity * dt`.
    let (instances, velocities) = match scene.instance_transforms(handle) {
        Ok(instances) => (instances, None),

        Err(nsi_intermediate::ResolveError::MotionSampledTransform {
            ..
        }) => {
            let [open, close] = shutter.unwrap_or([0.0, 1.0]);

            let mut at =
                |time: f64| match scene.instance_transforms_at(handle, time) {
                    Ok(placed) => Some(placed),
                    Err(error) => {
                        flushed.limitations.push(format!(
                            "instancer {handle:?} places nothing at {time}: \
                         {error}"
                        ));
                        None
                    }
                };

            let (begin, end) = (at(open)?, at(close)?);

            if begin.len() != end.len() {
                flushed.limitations.push(format!(
                    "instancer {handle:?} places {} instances at the \
                     shutter's open and {} at its close; that cannot be \
                     blurred, so the open sample is used",
                    begin.len(),
                    end.len()
                ));
                (begin, None)
            } else {
                (begin.clone(), Some(velocity(&begin, &end, open, close)))
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

    if let Some(velocities) = velocities {
        // `evaluation_frame` is the time `xform_list` describes, so
        // MoonRay's `dt` is zero there and the open-shutter positions
        // are used as written.
        let object = object
            .set(
                "evaluation_frame",
                Value::Float(shutter.map_or(0.0, |[open, _]| open as f32)),
            )
            .set("velocities", Value::Vector(velocities));

        flushed.limitations.push(format!(
            "instancer {handle:?} moves; only its **translation** is \
             blurred. `xform_list` cannot carry two timesteps, so \
             rotation and scale across the shutter would need the \
             decomposed form and `use_rotation_motion_blur`"
        ));

        return Some(with_transform(object, scene, handle, shutter, flushed));
    }

    Some(with_transform(object, scene, handle, shutter, flushed))
}

/// Per-instance velocity, in units per second.
///
/// MoonRay applies it as `position + velocity * dt` with
/// `dt = (motionStep - evaluationFrame) / fps`
/// (`InstanceProceduralLeaf.cc:344`), so with `motion_steps` at the
/// shutter's ends and `evaluation_frame` at its open, landing on the
/// close-shutter position needs
///
/// ```text
/// velocity = delta * fps / (close - open)
/// ```
///
/// `fps` is in it and does not cancel -- an earlier note said it did.
/// That is harmless because this backend *writes* `fps` (see [`FPS`]),
/// so the two agree by construction rather than by assumption.
fn velocity(
    begin: &[nsi_intermediate::Instance],
    end: &[nsi_intermediate::Instance],
    open: f64,
    close: f64,
) -> Vec<Value> {
    let span = close - open;
    // A zero-length shutter is not motion; a velocity of zero renders
    // the open sample sharp, which is the honest answer.
    let scale = if span > 0.0 {
        f64::from(FPS) / span
    } else {
        0.0
    };

    begin
        .iter()
        .zip(end)
        .map(|(from, to)| {
            // The translation is the last row: ɴsɪ uses RenderMan's
            // row-vector convention, so a point is a row and the
            // offset lives at 12, 13, 14.
            Value::Vec3f([
                ((to.transform[12] - from.transform[12]) * scale) as f32,
                ((to.transform[13] - from.transform[13]) * scale) as f32,
                ((to.transform[14] - from.transform[14]) * scale) as f32,
            ])
        })
        .collect()
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
        if !is_camera(node.node_type()) {
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

        // The three attributes this backend blurs. `motion_times`
        // walks the *transform chain*, so it does not see a mesh
        // deforming in place or an instancer whose own matrices are
        // sampled -- and missing either would leave those blurred over
        // a shutter that does not cover them.
        for attribute in ["P", "transformationmatrices"] {
            if let Ok(samples) = scene.attribute_samples(handle, attribute) {
                let times: Vec<f64> =
                    samples.iter().map(|(time, _)| *time).collect();
                widen(&times);
            }
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
            .as_chunks::<3>()
            .0
            .iter()
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

/// A light that is not in the scene, switched off rather than left out.
///
/// `Light.cc` declares `on` on the base class, so this is one attribute
/// on an object that stays in its `LightSet` -- the same reasoning as
/// `hidden`: dropping it instead would make an interactive disconnect a
/// change of *set membership*, which forces a whole-scene re-apply
/// (`002` `research.md` F3).
fn switched_off(object: Object) -> Object {
    object.set("on", Value::Bool(false))
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
) -> (Object, Vec<Object>) {
    let Some(node) = scene.node(handle) else {
        return (Object::new(MESH, handle), Vec::new());
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
                            .as_chunks::<3>()
                            .0
                            .iter()
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
    let subdivision = scheme.is_some() || node.node_type() == "subdivisionmesh";

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

    let mut data = Vec::new();
    object =
        primitive_variables(object, scene, node, handle, &mut data, flushed);

    (object, data)
}

/// ɴsɪ's `st` and `N`, as MoonRay's `uv_list` and `normal_list`.
///
/// Both of MoonRay's are **per face-vertex** -- its own comment says so
/// -- and ɴsɪ's may be given in any of four interpolations. So this is
/// an expansion, not a rename: whatever ɴsɪ gave is written out once
/// per face-vertex, in the same order as `vertices_by_index`.
///
/// Measured before implementing: `uv_list` is honoured, and halving it
/// halves what an OSL shader reads as `u` and `v`. Without it MoonRay
/// parametrises the face itself, which for a quad is also 0..1 -- so a
/// test that does not *change* the values proves nothing.
fn primitive_variables(
    mut object: Object,
    scene: &Scene,
    node: &Node,
    handle: &str,
    data: &mut Vec<Object>,
    flushed: &mut Flushed,
) -> Object {
    if let Some(values) = expanded(scene, handle, "st", 2, flushed) {
        object = object.set(
            "uv_list",
            Value::Vector(
                values
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|st| Value::Vec2f([st[0], st[1]]))
                    .collect(),
            ),
        );
    }

    if let Some(values) = expanded(scene, handle, "N", 3, flushed) {
        object = object.set(
            "normal_list",
            Value::Vector(
                values
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|n| Value::Vec3f([n[0], n[1], n[2]]))
                    .collect(),
            ),
        );
    }

    // Everything else the mesh carries, as `UserData` -- which is how
    // an OSL shader's `getattribute("name", value)` is answered, and
    // the only route MoonRay has for an attribute nobody declared in
    // advance.
    let mut references = Vec::new();
    for (name, argument) in node.attributes() {
        if STRUCTURE.contains(&name) || name.ends_with(".indices") {
            continue;
        }

        let Some(user_data) = user_data(scene, name, argument, handle, flushed)
        else {
            continue;
        };
        references.push(Value::Object(Reference::new(
            USER_DATA,
            user_data_name(handle, name),
        )));
        data.push(user_data);
    }

    if !references.is_empty() {
        object = object.set("primitive_attributes", Value::Vector(references));
    }

    object
}

/// The `mesh` attributes that describe the mesh rather than decorate
/// it.
///
/// Everything else on the node is a primitive variable and crosses as
/// one. Listed rather than inferred: an attribute this backend does
/// not know is far more likely to be a shader's than to be a mesh
/// attribute nobody implemented, and carrying it costs a `UserData` a
/// shader may ignore -- while dropping it costs the look.
const STRUCTURE: [&str; 15] = [
    "P",
    "N",
    "st",
    "nvertices",
    "nholes",
    "clockwisewinding",
    "referencetime",
    "quadraticmotion",
    "outlinecreasethreshold",
    "subdivision.scheme",
    "subdivision.cornervertices",
    "subdivision.cornersharpness",
    "subdivision.creasevertices",
    "subdivision.creasesharpness",
    "subdivision.smoothcreasecorners",
];

/// A `UserData` object's rdl2 name.
///
/// The mesh's handle and the attribute's, which is unique because
/// handles are and a mesh carries each attribute once.
fn user_data_name(handle: &str, name: &str) -> String {
    format!("{handle}/{name}")
}

/// One ɴsɪ primitive variable, as a `UserData`.
///
/// Face-varying, always: `expanded` has already put every
/// interpolation into that one order, and a rate MoonRay has to guess
/// at is a rate it can guess wrong.
fn user_data(
    scene: &Scene,
    name: &str,
    argument: &OwnedArgument,
    handle: &str,
    flushed: &mut Flushed,
) -> Option<Object> {
    let (key, values, components): (&str, &str, usize) = match argument.type_tag
    {
        Type::Color => ("color_key", "color_values_0", 3),
        Type::Point | Type::Vector | Type::Normal => {
            ("vec3f_key", "vec3f_values_0", 3)
        }
        Type::F32 if argument.array_length == 2 => {
            ("vec2f_key", "vec2f_values_0", 2)
        }
        Type::F32 => ("float_key", "float_values_0", 1),
        _ => {
            flushed.limitations.push(format!(
                    "mesh {handle:?} carries {name:?}, whose ɴsɪ type has no                      MoonRay `UserData` counterpart; it was not carried"
                ));
            return None;
        }
    };

    let expanded = expanded(scene, handle, name, components, flushed)?;

    let vector = Value::Vector(match components {
        3 if argument.type_tag == Type::Color => expanded
            .as_chunks::<3>()
            .0
            .iter()
            .map(|v| Value::Rgb([v[0], v[1], v[2]]))
            .collect(),
        3 => expanded
            .as_chunks::<3>()
            .0
            .iter()
            .map(|v| Value::Vec3f([v[0], v[1], v[2]]))
            .collect(),
        2 => expanded
            .as_chunks::<2>()
            .0
            .iter()
            .map(|v| Value::Vec2f([v[0], v[1]]))
            .collect(),
        _ => expanded.into_iter().map(Value::Float).collect(),
    });

    Some(
        Object::new(USER_DATA, user_data_name(handle, name))
            .set(key, Value::String(name.to_owned()))
            .set(values, vector)
            // 6 is "face varying" in `UserData`'s own `rate` enum. Its
            // default, "auto", guesses from the count -- and on a mesh
            // where the counts coincide it can guess wrong.
            .set("rate", Value::Int(6)),
    )
}

/// One ɴsɪ primitive variable, expanded to one float per face-vertex.
///
/// The *interpolation* is `nsi-intermediate`'s to resolve -- `.indices`
/// first, then the `per_vertex` and `per_face` flags, then the counts,
/// and a refusal where those disagree. What is left here is the part
/// that is MoonRay's: reading the floats out in the order it wants
/// them, which is one per face-vertex in face order.
fn expanded(
    scene: &Scene,
    handle: &str,
    name: &str,
    components: usize,
    flushed: &mut Flushed,
) -> Option<Vec<f32>> {
    let variable = match scene.primitive_variable(handle, name) {
        Ok(variable) => variable?,
        Err(error) => {
            // **The refusal is the point.** Four values on a mesh with
            // four faces *and* four vertices is two different meshes
            // depending which reading is taken, and ɴsɪ's own answer is
            // the `per_face`/`per_vertex` flag -- so an unflagged one is
            // reported rather than guessed. An earlier version of this
            // code guessed, and guessed uniform.
            flushed.limitations.push(format!(
                "mesh {handle:?}: {name:?} was not carried ({error})"
            ));
            return None;
        }
    };

    let OwnedData::F32(values) = &variable.values().data else {
        flushed.limitations.push(format!(
            "mesh {handle:?} has a {name:?} that is not float data; it was \
             not carried"
        ));
        return None;
    };

    let count = values.len() / components;
    let mut out = Vec::with_capacity(variable.face_vertex_count() * components);
    for index in variable.face_varying_indices() {
        if index >= count {
            flushed.limitations.push(format!(
                "mesh {handle:?} indexes {name:?} out of range; it was not \
                 carried"
            ));
            return None;
        }
        let start = index * components;
        out.extend_from_slice(&values[start..start + components]);
    }

    Some(out)
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
    shading: Shading,
    flushed: &mut Flushed,
) -> Option<Reference> {
    // A `Layer` row names an object by class *and* name, so the class
    // has to be the one the shader actually became. Assuming
    // `UsdPreviewSurface` here while the shader emitted an `Osl`
    // yields a row that reads perfectly and points at nothing --
    // which MoonRay renders as no material at all, and skips.
    let class = |shader: &str| match shading {
        Shading::Osl if crate::osl::is_runnable(scene, shader) => OSL_MATERIAL,
        _ => MATERIAL,
    };

    match scene.geometry_binding(handle) {
        Ok(binding) => binding?
            .surface_shader
            .as_deref()
            .map(|shader| Reference::new(class(shader), shader)),
        Err(error) => {
            flushed.limitations.push(format!(
                "{handle:?} has no single material binding ({error}); it \
                 renders with the default surface"
            ));
            None
        }
    }
}

/// The shader handles bound in each shader slot: surfaces, then
/// displacements.
///
/// Read off the edges rather than resolved per geometry, and
/// deliberately: this decides what *class* a shader node becomes, which
/// has to be the same answer everywhere the shader is named. An edge
/// that loses ɴsɪ's precedence rule to another still leaves an object
/// nothing references, which costs a few lines of `.rdla`; getting the
/// class wrong costs the surface.
fn shader_roles(scene: &Scene) -> (HashSet<&str>, HashSet<&str>) {
    let mut surfaces = HashSet::new();
    let mut displaces = HashSet::new();

    for edge in scene.edges() {
        match edge.kind {
            EdgeKind::SurfaceShader => {
                surfaces.insert(edge.from());
            }
            EdgeKind::DisplacementShader => {
                displaces.insert(edge.from());
            }
            _ => {}
        }
    }

    (surfaces, displaces)
}

/// The displacement bound to one piece of geometry, if any.
///
/// There is no substitute for a displacement the way `UsdPreviewSurface`
/// substitutes for a surface: a displacement *moves vertices*, and a
/// stand-in that does not move them renders a different shape. So
/// without OSL -- or with a shader OSL cannot load -- it is reported and
/// the geometry keeps its own silhouette.
fn displacement(
    scene: &Scene,
    handle: &str,
    shading: Shading,
    flushed: &mut Flushed,
) -> Option<Reference> {
    let shader = scene
        .geometry_binding(handle)
        .ok()
        .flatten()?
        .displacement_shader?;

    if shading != Shading::Osl || !crate::osl::is_runnable(scene, &shader) {
        flushed.limitations.push(format!(
            "{handle:?} has displacement shader {shader:?} bound, which \
             needs OSL; the geometry is not displaced"
        ));
        return None;
    }

    Some(Reference::new(OSL_DISPLACEMENT, shader))
}

/// Say that a bound `volumeshader` is not the one being run.
///
/// The interface binds a volume shader through the `attributes` node
/// the way it binds a surface or a displacement, and upstream resolves
/// it. Nothing here can run it: MoonRay reads a volume through a
/// `VolumeShader` root, whose interface is four separate virtuals --
/// `extinct`, `albedo`, `emission` and `anisotropy`, each asked
/// independently and at its own time -- against OSL's one execution
/// producing one closure tree. There is no `OslVolume` to bind, so
/// every volume renders with the stock `VdbVolume`.
///
/// **Reported rather than dropped, because the volume still appears.**
/// A missing surface shader shows up as an untextured shape; a missing
/// *volume* shader shows up as a perfectly plausible puff of the
/// density grid, with none of the shader's extinction, colour or
/// emission, and nothing about the image says so.
fn report_volume_shader(scene: &Scene, handle: &str, flushed: &mut Flushed) {
    let Some(shader) = scene
        .geometry_binding(handle)
        .ok()
        .flatten()
        .and_then(|binding| binding.volume_shader)
    else {
        return;
    };

    flushed.limitations.push(format!(
        "{handle:?} has volume shader {shader:?} bound; MoonRay reads a \
         volume through a `VolumeShader` root and this backend has no OSL \
         one, so the volume renders with the stock `{VOLUME_SHADER}` and \
         the shader's extinction, albedo, emission and anisotropy are lost"
    ));
}

/// The parameters carried from an ɴsɪ shader into the substitute
/// surface, paired with the `UsdPreviewSurface` attribute each feeds.
///
/// An ɴsɪ shader is an OSL shader: it names a compiled shader in
/// `shaderfilename` and carries whatever parameters *that* shader
/// declares. So there is no ɴsɪ spelling of "roughness" to look up, and
/// inventing one is the failure this table exists to avoid -- a wrong
/// name renders plausibly and silently.
///
/// What there is, instead, is a short list of shaders in practical use,
/// shipped compiled with 3Delight. `.oso` is a text format, so each row
/// below was **read** off one, with `tools/probe/parameters.sh`, rather
/// than guessed; `research.md` F11 has the table and where it came
/// from. `UsdPreviewSurface`'s own names are the last row, and stand in
/// for a shader this list does not know: matching them by exact name is
/// the behaviour that was here before the probe existed.
///
/// Everything not carried is reported by name rather than dropped
/// quietly.
const PARAMETERS: [(&str, &[(&str, &str)]); 7] = [
    (
        "dlPrincipled",
        &[
            ("i_color", "diffuseColor"),
            ("incandescence", "emissiveColor"),
            ("roughness", "roughness"),
            ("metallic", "metallic"),
            ("refract_ior", "ior"),
            ("opacity", "opacity"),
        ],
    ),
    (
        "dlStandard",
        &[
            ("base_color", "diffuseColor"),
            ("emission_color", "emissiveColor"),
            ("specular_roughness", "roughness"),
            ("metalness", "metallic"),
            ("specular_IOR", "ior"),
            ("opacity", "opacity"),
        ],
    ),
    (
        "openPBRSurface",
        &[
            ("baseColor", "diffuseColor"),
            ("emissionColor", "emissiveColor"),
            ("specularRoughness", "roughness"),
            ("baseMetalness", "metallic"),
            ("specularIOR", "ior"),
            ("geometryOpacity", "opacity"),
        ],
    ),
    (
        // Always metal, so `metallic` is 1 rather than read.
        "dlMetal",
        &[
            ("i_color", "diffuseColor"),
            ("roughness", "roughness"),
            ("opacity", "opacity"),
        ],
    ),
    (
        "dlGlass",
        &[
            ("i_color", "diffuseColor"),
            ("incandescence", "emissiveColor"),
            ("refract_roughness", "roughness"),
            ("refract_ior", "ior"),
        ],
    ),
    (
        "dlPrelit",
        &[
            ("i_color", "diffuseColor"),
            ("i_incandescence", "emissiveColor"),
        ],
    ),
    (
        "UsdPreviewSurface",
        &[
            ("diffuseColor", "diffuseColor"),
            ("emissiveColor", "emissiveColor"),
            ("roughness", "roughness"),
            ("metallic", "metallic"),
            ("ior", "ior"),
            ("opacity", "opacity"),
        ],
    ),
];

/// What `PARAMETERS` says to carry for a shader.
///
/// `shaderfilename` is a path to a compiled shader, so what identifies
/// it is the file stem: `dlPrincipled`, `/opt/3delight/osl/dlPrincipled`
/// and `dlPrincipled.oso` are the same shader. A shader the table does
/// not know falls back to `UsdPreviewSurface`'s own names, matched
/// exactly -- which is right as often as the two happen to agree, and
/// wrong in no case that carrying nothing would have got right.
fn parameters(node: &Node) -> &'static [(&'static str, &'static str)] {
    let fallback = PARAMETERS[PARAMETERS.len() - 1].1;

    let Some(stem) = shader_stem(node) else {
        return fallback;
    };

    PARAMETERS
        .iter()
        .find(|(shader, _)| *shader == stem)
        .map_or(fallback, |(_, carried)| *carried)
}

/// One ɴsɪ shader, as MoonRay's stock PBR surface.
///
/// MoonRay runs no OSL (`research.md` F6), so the shader itself cannot
/// cross. What crosses is a `UsdPreviewSurface` carrying the parameters
/// `PARAMETERS` knows how to name for this shader.
fn shader(
    scene: &Scene,
    handle: &str,
    shading: Shading,
    flushed: &mut Flushed,
) -> Object {
    // A shader with nothing for OSL to load would shade black, and ɴsɪ
    // always returns an image -- so it falls back to the substitute
    // for that one shader rather than losing the surface. Reported
    // either way.
    if shading == Shading::Osl && crate::osl::is_runnable(scene, handle) {
        return osl_shader(scene, handle, flushed);
    }

    let mut object = Object::new(MATERIAL, handle);
    let Some(node) = scene.node(handle) else {
        return object;
    };

    let mut carried = Vec::new();
    for (from, to) in parameters(node) {
        let (from, to) = (*from, *to);
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

    // `shaderfilename` names the shader rather than parametrising it,
    // and reporting it as a lost parameter would be noise in every
    // message.
    if shading == Shading::Osl {
        flushed.limitations.push(format!(
            "shader {handle:?} names no \"shaderfilename\", so there is \
             nothing for OSL to run; a {MATERIAL} stands in for it"
        ));
    }

    let dropped: Vec<&str> = node
        .attributes()
        .map(|(name, _)| name)
        .filter(|name| *name != "shaderfilename" && !carried.contains(name))
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

/// The ɴsɪ shaders that turn geometry into a light, and what each
/// becomes in MoonRay.
///
/// ɴsɪ has **no light nodes**. Section 4.5 of the specification: "There
/// are no special light source nodes in ɴsɪ ... Any scene geometry can
/// become a light source if its surface shader produces an
/// `emission()` closure." An area light is a mesh wearing an emitter; a
/// spot light is "an epsilon sized geometry (a small disk, a particle,
/// etc.)" whose shader shapes the emission into a cone.
///
/// So recognising a light means knowing what a shader *does*, and
/// MoonRay runs no OSL (`research.md` F6). There is no attribute to
/// read. What is readable is the shader's name, and the emitters in
/// practical use are a short list: 3Delight ships six, and the
/// specification's own listings 4.2 and 4.3 are two more of the same
/// shape. Like `PARAMETERS`, this table was read off the shipped
/// `.oso` files with `tools/probe/parameters.sh` rather than guessed.
///
/// A shader not on this list leaves its geometry as geometry, wearing
/// a substitute surface. That is the safe direction to be wrong in: a
/// mesh that should have been a light renders dark and visible, which
/// looks like what it is, whereas a mesh silently promoted to a light
/// disappears from the frame.
const LIGHTS: [(&str, &str); 6] = [
    // 3Delight's own, all sharing `i_color`, `intensity`, `exposure`.
    ("areaLight", MESH_LIGHT),
    ("pointLight", SPHERE_LIGHT),
    ("spotLight", SPOT_LIGHT),
    ("distantLight", DISTANT_LIGHT),
    ("directionalLight", DISTANT_LIGHT),
    // The specification's own listing 4.2, which every hand-written
    // ɴsɪ scene uses. Its parameters are `power` and `Cs`.
    ("emitter", MESH_LIGHT),
];

/// `Light::visible_in_camera`'s "force on".
///
/// `Light.cc` declares it enumerable: 0 is off, 1 on, and 2 -- the
/// default -- reads `SceneVariables::lights_visible_in_camera`.
const VISIBLE_IN_CAMERA_ON: i32 = 1;

/// MoonRay's light DSOs, for the ɴsɪ emitters above.
const MESH_LIGHT: &str = "MeshLight";
const SPHERE_LIGHT: &str = "SphereLight";
const SPOT_LIGHT: &str = "SpotLight";
const DISTANT_LIGHT: &str = "DistantLight";

/// What MoonRay light, if any, an ɴsɪ shader makes of its geometry.
fn light_class(scene: &Scene, shader: &str) -> Option<&'static str> {
    let stem = shader_stem(scene.node(shader)?)?;

    LIGHTS
        .iter()
        .find(|(name, _)| *name == stem)
        .map(|(_, class)| *class)
}

/// The shader an ɴsɪ node's geometry wears, if it wears one.
fn surface_shader(scene: &Scene, handle: &str) -> Option<String> {
    scene
        .geometry_binding(handle)
        .ok()
        .flatten()
        .and_then(|binding| binding.surface_shader)
}

/// One ɴsɪ light: geometry whose shader emits.
///
/// The name is derived rather than the geometry's own, because a
/// `MeshLight` and the mesh it points at are two objects and rdl2 names
/// them apart.
fn light_handle(handle: &str) -> String {
    format!("{handle}/light")
}

/// One ɴsɪ emitter as a MoonRay light.
///
/// `Light.cc` declares `color`, `intensity` and `exposure` on the base
/// class, and every one of 3Delight's light shaders declares
/// `i_color`, `intensity` and `exposure`. That correspondence is
/// one-to-one and is the reason this mapping is a table rather than an
/// interpretation.
fn light(
    scene: &Scene,
    handle: &str,
    shader: &str,
    class: &'static str,
    shutter: Option<[f64; 2]>,
    flushed: &mut Flushed,
) -> Object {
    let mut object = Object::new(class, light_handle(handle));

    // The light stands where the geometry stands.
    object = with_transform(object, scene, handle, shutter, flushed);

    let Some(node) = scene.node(shader) else {
        return object;
    };

    let mut carried = vec!["shaderfilename"];

    // `emitter` (listing 4.2) spells the same two things differently.
    let (colour, strength) = match shader_stem(node).as_deref() {
        Some("emitter") => ("Cs", "power"),
        _ => ("i_color", "intensity"),
    };

    if let Some(rgb) = colour_of(node, colour) {
        object = object.set("color", Value::Rgb(rgb));
        carried.push(colour);
    }
    for (from, to) in [(strength, "intensity"), ("exposure", "exposure")] {
        if let Some(value) = scalar_of(node, from) {
            object = object.set(to, Value::Float(value));
            carried.push(from);
        }
    }

    match class {
        MESH_LIGHT => {
            // The light *is* the mesh, so it points back at it.
            //
            // And it is seen as well as sampled. In ɴsɪ an emissive
            // mesh is ordinary geometry: a camera ray hits it and sees
            // it glow. A `MeshLight`'s geometry cannot be in the render
            // layer (`research.md` F12), so that path is closed -- but
            // `MeshLight::intersect` ray-traces the *real mesh* through
            // an Embree scene of its own, and `Scene::updateActiveLights`
            // puts a bounded light into the camera-visible set when this
            // is on. So one object is both seen and sampled, which is
            // what ɴsɪ means.
            //
            // Forced on rather than left at the default, which defers to
            // `SceneVariables::lights_visible_in_camera` -- a scene-wide
            // switch that would make an ɴsɪ emitter appear or vanish for
            // a reason nothing in the ɴsɪ scene said.
            object = object
                .set("geometry", Value::Object(Reference::new(MESH, handle)))
                .set("visible_in_camera", Value::Int(VISIBLE_IN_CAMERA_ON));
        }
        SPOT_LIGHT => {
            let (outer, inner) = cone(node);
            object = object
                .set("outer_cone_angle", Value::Float(outer))
                .set("inner_cone_angle", Value::Float(inner));
            carried.extend(["coneAngle", "penumbraAngle"]);
        }
        _ => {}
    }

    let dropped: Vec<&str> = node
        .attributes()
        .map(|(name, _)| name)
        .filter(|name| !carried.contains(name))
        .collect();

    if dropped.is_empty() {
        flushed.limitations.push(format!(
            "{handle:?} wears the ɴsɪ emitter {shader:?} and became a \
             {class}; its emission is MoonRay's rather than the OSL \
             closure's, so the two renderers agree on where the light is \
             and not on its photometry"
        ));
    } else {
        flushed.limitations.push(format!(
            "{handle:?} wears the ɴsɪ emitter {shader:?} and became a \
             {class}; its emission is MoonRay's rather than the OSL \
             closure's, and these parameters are not carried: {}",
            dropped.join(", ")
        ));
    }

    object
}

/// A spot light's two cone angles, in degrees, from ɴsɪ's `coneAngle`
/// and `penumbraAngle`.
///
/// Both of MoonRay's are documented in `SpotLight/attributes.cc` as
/// "a full angle, measured from one side to the other". ɴsɪ's
/// `coneAngle` is full as well -- the specification's listing 4.3
/// halves it before comparing cosines -- and `penumbraAngle` is added
/// to that *half* angle, so it counts double here:
///
/// ```text
/// coslimit = cos(coneAngle / 2)                 the hard edge
/// cospen   = cos(coneAngle / 2 + penumbraAngle) the soft one
/// smoothstep(min, max, ...)                     either way round
/// ```
///
/// A negative penumbra puts the soft edge inside the cone, which is
/// why the two are split by sign rather than by name.
fn cone(node: &Node) -> (f32, f32) {
    let angle = scalar_of(node, "coneAngle").unwrap_or(40.0);
    let penumbra = scalar_of(node, "penumbraAngle").unwrap_or(0.0);

    let outer = (angle + 2.0 * penumbra.max(0.0)).clamp(0.0, 180.0);
    let inner = (angle + 2.0 * penumbra.min(0.0)).clamp(0.0, outer);

    (outer, inner)
}

/// A shader's identity: the stem of `shaderfilename`.
///
/// `dlPrincipled`, `/opt/3delight/osl/dlPrincipled` and
/// `dlPrincipled.oso` are the same shader.
fn shader_stem(node: &Node) -> Option<String> {
    let Some(OwnedData::String(names)) =
        node.effective("shaderfilename").map(|arg| &arg.data)
    else {
        return None;
    };

    let name = String::from_utf8_lossy(names.first()?).into_owned();
    let after_slash = name.rsplit(['/', '\\']).next().unwrap_or_default();

    Some(
        after_slash
            .strip_suffix(".oso")
            .unwrap_or(after_slash)
            .to_owned(),
    )
}

/// One colour parameter, whatever width it was recorded at.
fn colour_of(node: &Node, name: &str) -> Option<[f32; 3]> {
    match &node.effective(name)?.data {
        OwnedData::F32(values) if values.len() >= 3 => {
            Some([values[0], values[1], values[2]])
        }
        OwnedData::F64(values) if values.len() >= 3 => {
            Some([values[0] as f32, values[1] as f32, values[2] as f32])
        }
        _ => None,
    }
}

/// One scalar parameter, whatever precision it was recorded at.
fn scalar_of(node: &Node, name: &str) -> Option<f32> {
    match &node.effective(name)?.data {
        OwnedData::F32(values) => values.first().copied(),
        OwnedData::F64(values) => values.first().map(|value| *value as f32),
        OwnedData::I32(values) => values.first().map(|value| *value as f32),
        _ => None,
    }
}

/// One ɴsɪ shader network, as MoonRay's `Osl` material.
///
/// Nothing here decides what the shader *means*: the network crosses
/// as an OSL group specification and MoonRay runs it. Which is the
/// point -- with OSL running, "does this shader emit?" is answered by
/// executing it rather than by recognising a name, and `PARAMETERS`
/// and `LIGHTS` become fallbacks rather than the mapping.
fn osl_shader(scene: &Scene, handle: &str, flushed: &mut Flushed) -> Object {
    let group = crate::osl::group(scene, handle);

    for line in &group.dropped {
        flushed
            .limitations
            .push(format!("shader {handle:?}: {line} was not carried"));
    }

    let mut object = Object::new(OSL_MATERIAL, handle)
        .set("group_name", Value::String(handle.to_owned()))
        .set("group_spec", Value::String(group.spec));

    // OSL resolves a shader by name against a search path, so a scene
    // that spelled its shaders absolutely has to have the directory
    // lifted out of the name -- which `osl::search_path` does, from
    // the root shader. A network whose layers live in different
    // directories needs more than one, and the material takes one; the
    // rest come from `$OSL_SHADER_PATH`.
    if let Some(path) = crate::osl::search_path(scene, handle) {
        object = object.set("search_path", Value::String(path));
    }

    object
}

/// One ɴsɪ shader network, as MoonRay's `OslDisplacement`.
///
/// The same group specification a material would carry: OSL decides
/// what a shader may do from the *usage* it is compiled into a group
/// with, and that is the root shader's business, not the flush's.
fn osl_displacement(
    scene: &Scene,
    handle: &str,
    flushed: &mut Flushed,
) -> Object {
    let mut object = osl_shader(scene, handle, flushed);
    object.class = Name::new(OSL_DISPLACEMENT);
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
    let class = camera_class(scene, handle);
    let Some(node) = scene.node(handle) else {
        return Object::new(class, handle);
    };

    let mut object = Object::new(class, handle);

    object = with_transform(object, scene, handle, shutter, flushed);
    object = clipping(object, node, handle, flushed);

    let degrees = match node.effective("fov").map(|arg| &arg.data) {
        Some(OwnedData::F32(values)) => values.first().copied(),
        Some(OwnedData::F64(values)) => values.first().map(|v| *v as f32),
        _ => None,
    };

    match class {
        // A perspective camera's field of view is a focal length in
        // MoonRay, against a fixed film aperture. See `focal`.
        PERSPECTIVE_CAMERA => match degrees {
            Some(degrees) => {
                object = object
                    .set("focal", Value::Float(focal(degrees, resolution)));
                report_perspective_window(
                    screen_window(scene, resolution),
                    resolution,
                    handle,
                    flushed,
                );
            }
            None => flushed.limitations.push(format!(
                "camera {handle:?} has no \"fov\"; MoonRay's default focal \
                 length is used"
            )),
        },

        // A fisheye's is an angle on both sides, so it crosses as
        // itself.
        "FisheyeCamera" => {
            if let Some(degrees) = degrees {
                object = object.set("fov", Value::Float(degrees));
            }
            object = fisheye_mapping(object, node, handle, flushed);
        }

        // **An orthographic camera's extent is the screen window.**
        // The interface gives it no `fov`, and MoonRay's default
        // aperture is 24 world units, so without this the framing is
        // wrong by whatever the scene's scale happens to be.
        "OrthographicCamera" => {
            object = orthographic_extent(
                object,
                screen_window(scene, resolution),
                resolution,
                handle,
                flushed,
            );
        }

        // A spherical camera sees everything; there is no extent to
        // carry.
        _ => {}
    }

    object
}

/// The interface's fisheye mappings, as MoonRay's `mapping` enum.
///
/// Three of the four are the same idea under the same name. The fourth
/// is not: `equisolidangle` is MoonRay's `equisolid angle`, with a
/// space, and a name it does not know leaves the enum at its default
/// rather than failing -- so an unrecognised one is reported.
fn fisheye_mapping(
    object: Object,
    node: &Node,
    handle: &str,
    flushed: &mut Flushed,
) -> Object {
    let Some(OwnedData::String(values)) =
        node.effective("mapping").map(|arg| &arg.data)
    else {
        return object;
    };
    let Some(name) = values.first() else {
        return object;
    };
    let name = String::from_utf8_lossy(name).into_owned();

    let mapping = match name.as_str() {
        "equidistant" => "equidistant",
        "equisolidangle" => "equisolid angle",
        "orthographic" => "orthographic",
        "stereographic" => "stereographic",
        other => {
            flushed.limitations.push(format!(
                "camera {handle:?} asks for fisheye mapping {other:?}, \
                 which MoonRay does not have; its default is used"
            ));
            return object;
        }
    };

    object.set("mapping", Value::String(mapping.to_owned()))
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
/// ɴsɪ's `fov` is **vertical**, measured against 3Delight rather than
/// inferred: a quad of half-extent 1 one unit in front of the camera,
/// at `fov` 90 on a 400x200 frame, fills all 200 rows and 200 of the
/// 400 columns. `tools/probe/framing.nsi` is the probe and
/// `research.md` F11 the write-up.
fn focal(fov_degrees: f32, resolution: (i32, i32)) -> f32 {
    let aspect = resolution.1 as f32 / resolution.0 as f32;
    let half = (fov_degrees.to_radians() * 0.5).tan();

    FILM_WIDTH_APERTURE * 0.5 * aspect / half
}

/// One `volume` node, as a `VdbGeometry`.
///
/// The interface's `volume` node is OpenVDB and nothing else: a file
/// and a set of named grids. MoonRay reads two of those grids --
/// density and emission -- and has no notion of the rest, so what does
/// not cross is named.
fn volume(
    scene: &Scene,
    handle: &str,
    shutter: Option<[f64; 2]>,
    flushed: &mut Flushed,
) -> Object {
    let mut object = Object::new(VOLUME, handle);
    object = with_transform(object, scene, handle, shutter, flushed);

    let Some(node) = scene.node(handle) else {
        return object;
    };

    let text = |name: &str| match node.effective(name).map(|arg| &arg.data) {
        Some(OwnedData::String(values)) => values
            .first()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned()),
        _ => None,
    };

    match text("vdbfilename") {
        Some(file) => object = object.set("model", Value::String(file)),
        None => flushed.limitations.push(format!(
            "volume {handle:?} names no \"vdbfilename\"; there is nothing \
             to read"
        )),
    }

    for (from, to) in [
        ("densitygrid", "density_grid"),
        ("velocitygrid", "velocity_grid"),
    ] {
        if let Some(grid) = text(from) {
            object = object.set(to, Value::String(grid));
        }
    }

    // **MoonRay's emission grid must be RGB.** Measured: a float grid
    // named here is refused at render prep -- "is not an RGB grid" --
    // and the whole volume then renders as nothing. So it is carried,
    // because a colour grid is exactly what it wants, and the shape of
    // the failure is said rather than discovered.
    if let Some(grid) = text("emissiongrid") {
        object = object.set("emission_grid", Value::String(grid.clone()));
        flushed.limitations.push(format!(
            "volume {handle:?} names emission grid {grid:?}; MoonRay reads \
             only an RGB grid there and refuses a scalar one, which stops \
             the volume rendering at all"
        ));
    }

    if let Some(OwnedData::F64(values)) =
        node.effective("velocityscale").map(|arg| &arg.data)
        && let Some(scale) = values.first()
    {
        object = object.set("velocity_scale", Value::Float(*scale as f32));
    }

    for name in [
        "colorgrid",
        "emissionintensitygrid",
        "temperaturegrid",
        "velocityreferencetime",
    ] {
        if node.effective(name).is_some() {
            flushed.limitations.push(format!(
                "volume {handle:?} sets {name:?}, which MoonRay's \
                 `VdbGeometry` has no counterpart for; it reads a density \
                 grid and an emission grid and nothing else"
            ));
        }
    }

    object
}

/// One `RenderOutput` per ɴsɪ output layer.
fn render_output(
    scene: &Scene,
    layer: &str,
    drivers: &[String],
    flushed: &mut Flushed,
) -> Object {
    let mut object = Object::new("RenderOutput", layer);

    let node = scene.node(layer);
    let text = |name: &str| {
        node.and_then(|node| node.effective(name))
            .and_then(|argument| match &argument.data {
                OwnedData::String(values) => values
                    .first()
                    .map(|bytes| String::from_utf8_lossy(bytes).into_owned()),
                _ => None,
            })
    };

    let variable = text("variablename");
    // `layername` "will be name of the layer as written by the output
    // driver", which is `channel_name`. Without one the variable's own
    // name is what a driver would call it.
    if let Some(name) = text("layername").or_else(|| variable.clone()) {
        object = object.set("channel_name", Value::String(name));
    }

    // ɴsɪ defaults `variablesource` to `shader`, and `Ci` there is the
    // beauty -- which is `RenderOutput`'s own default, so the common
    // layer sets nothing.
    let source = text("variablesource").unwrap_or_else(|| "shader".into());
    // ɴsɪ's own default is `color`, and it is what MoonRay has to be
    // told for a primitive attribute -- rdl2 declares the channel count
    // statically and will not work it out from the data.
    let kind = text("layertype").unwrap_or_else(|| "color".into());
    object =
        result(object, &source, variable.as_deref(), &kind, layer, flushed);

    // `scalarformat` is ɴsɪ's quantization; MoonRay's `channel_format`
    // has two of the eight. The integer formats have no counterpart at
    // all -- rdl2 writes float or half EXR -- so they are reported
    // rather than silently rounded to one.
    match text("scalarformat").as_deref() {
        None | Some("half") => {}
        Some("float") => {
            object = object.set("channel_format", Value::String("float".into()))
        }
        Some(other) => flushed.limitations.push(format!(
            "output layer {layer:?} asks for {other:?} scalars, which              MoonRay's `RenderOutput` cannot encode; half float is written"
        )),
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

/// What an output layer asks for, as MoonRay's `result` and whatever
/// that result needs beside it.
///
/// ɴsɪ says where a variable comes from with `variablesource` and names
/// it with `variablename`; MoonRay has an enum and a second attribute
/// per case. The two do not cover each other, and what does not cross
/// is named rather than defaulted to the beauty -- an AOV that renders
/// the beauty under another name is worse than a missing one.
fn result(
    object: Object,
    source: &str,
    variable: Option<&str>,
    kind: &str,
    layer: &str,
    flushed: &mut Flushed,
) -> Object {
    let unmapped = |flushed: &mut Flushed, why: &str| {
        flushed.limitations.push(format!(
            "output layer {layer:?} asks for {why}, which MoonRay's              `RenderOutput` has no result for; the layer renders the beauty"
        ));
    };

    match (source, variable) {
        // The beauty, however it was spelled.
        ("shader", None | Some("Ci")) => object,

        ("builtin", Some(name)) => match name {
            "alpha" => object.set("result", Value::String("alpha".into())),
            // ɴsɪ's `z` is camera-space depth, which is exactly what
            // MoonRay's `depth` result is.
            "z" => object.set("result", Value::String("depth".into())),
            _ => match state_variable(name) {
                Some(variable) => object
                    .set("result", Value::String("state variable".into()))
                    .set("state_variable", Value::String(variable.into())),
                None => {
                    unmapped(flushed, &format!("built-in {name:?}"));
                    object
                }
            },
        },

        // A primitive variable, straight off the geometry.
        ("attribute", Some(name)) => {
            let object = object
                .set("result", Value::String("primitive attribute".into()))
                .set("primitive_attribute", Value::String(name.to_owned()));
            match kind {
                "scalar" => object.set(
                    "primitive_attribute_type",
                    Value::String("FLOAT".into()),
                ),
                "vector" => object.set(
                    "primitive_attribute_type",
                    Value::String("VEC3F".into()),
                ),
                // `color` is ɴsɪ's default and MoonRay's; `quad` has no
                // counterpart, and a fourth component nobody asked for
                // is not something to invent.
                "quad" => {
                    flushed.limitations.push(format!(
                        "output layer {layer:?} asks for a `quad` layer \
                         type; MoonRay's primitive attributes are one, two \
                         or three components, and the layer is written as \
                         a colour"
                    ));
                    object
                }
                _ => object,
            }
        }

        // A shader output: the radiance of one part of the surface.
        //
        // MoonRay spells that as a *light* AOV -- a light-path
        // expression -- rather than a material AOV, which names a
        // lobe's properties (its albedo, its roughness) rather than
        // what it contributed. `C<..'diffuse'>L` is every path that
        // scattered off a lobe labelled `diffuse`, by any event, and
        // reached a light.
        //
        // **The label is bare, with no material name in front of it.**
        // MoonRay registers a lobe label as `<material label>.<lobe>`
        // when the material carries a label and as the lobe alone when
        // it does not -- measured -- so leaving the material unlabelled
        // is what lets one output layer name a lobe across every shader
        // in the scene, which is what a shader AOV means.
        ("shader", Some(name)) => match lobe_label(name) {
            Some(label) => object
                .set("result", Value::String("light aov".into()))
                .set("lpe", Value::String(format!("C<..'{label}'>L"))),
            None => {
                unmapped(
                    flushed,
                    &format!(
                        "shader output {name:?} -- it is not one of the \
                         lobe labels the `Osl` material can name"
                    ),
                );
                object
            }
        },

        _ => {
            unmapped(
                flushed,
                &format!("variable source {source:?} with no variable name"),
            );
            object
        }
    }
}

/// One shader-AOV name, as the lobe label the `Osl` material sets.
///
/// **This table and `aov_label` in `dso/osl/Osl.cc` are one contract.**
/// The flush writes a light-path expression naming a label; the
/// material is what puts that label on the lobe. They are in different
/// languages and cannot share the table, so they have to be changed
/// together -- and a disagreement renders the AOV black rather than
/// failing.
///
/// The left column is 3Delight's `outputvariable` vocabulary, read off
/// the shaders it ships. The right is MoonRay's, from `labels[]` in
/// `dso/osl/attributes.cc`. A name already in the right column is taken
/// as itself, since the interface does not mandate 3Delight's spelling.
const SHADER_AOVS: [(&str, &str); 8] = [
    ("diffuse", "diffuse"),
    ("reflection", "specular"),
    ("refraction", "transmission"),
    ("subsurface", "subsurface"),
    ("sheen", "sheen"),
    ("coating", "coat"),
    ("incandescence", "emission"),
    ("hair", "hair"),
];

fn lobe_label(name: &str) -> Option<&'static str> {
    SHADER_AOVS
        .iter()
        .find(|(aov, _)| *aov == name)
        .or_else(|| SHADER_AOVS.iter().find(|(_, label)| *label == name))
        .map(|(_, label)| *label)
}

/// One ɴsɪ built-in variable, as MoonRay's `state_variable` enum.
///
/// ɴsɪ spells a space into the name -- `"P.world"`, `"N.camera"` --
/// and MoonRay has one entry per quantity in *render* space plus a
/// world-space position. Only the pairs that mean the same thing are
/// here: `"N.world"` has no entry, and answering it with `N` would be
/// a normal in the wrong space, which shades plausibly and wrongly.
fn state_variable(name: &str) -> Option<&'static str> {
    Some(match name {
        "P.world" => "Wp",
        "P.camera" => "P",
        "N.camera" => "N",
        "Ng.camera" => "Ng",
        "st" => "St",
        "motionvector" => "motionvec",
        _ => return None,
    })
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

/// The screen window, as the interface's four numbers.
///
/// `[left, bottom, right, top]` in screen space. The specification's
/// default is `[-f, -1], [f, 1]` for `f = xres/yres`, and it says so
/// rather than leaving it to the renderer -- so a scene that sets
/// nothing still has a definite one, and it is the one computed here.
fn screen_window(scene: &Scene, resolution: (i32, i32)) -> [f64; 4] {
    for output in scene.render_outputs() {
        if let Some(node) = scene.node(&output.screen)
            && let Some(argument) = node.effective("screenwindow")
        {
            let values: Vec<f64> = match &argument.data {
                OwnedData::F64(values) => values.clone(),
                OwnedData::F32(values) => {
                    values.iter().map(|value| *value as f64).collect()
                }
                _ => continue,
            };
            if values.len() >= 4 {
                return [values[0], values[1], values[2], values[3]];
            }
        }
    }

    let aspect = f64::from(resolution.0) / f64::from(resolution.1.max(1));
    [-aspect, -1.0, aspect, 1.0]
}

/// The near and far clipping planes.
///
/// `clippingrange` is one of the attributes the specification gives
/// *all* camera nodes, and MoonRay's `Camera` base class declares
/// `near` and `far` for the same thing -- so this is a rename.
///
/// It matters more than a rename usually does. MoonRay's defaults are
/// `near` 1 and `far` 10000, in world units, and a scene modelled in
/// centimetres or millimetres puts its whole subject inside the near
/// plane. Nothing is reported when that happens: the geometry is
/// simply not there, and the image is empty in the way this backend
/// keeps running into.
fn clipping(
    object: Object,
    node: &Node,
    handle: &str,
    flushed: &mut Flushed,
) -> Object {
    let values: Vec<f64> =
        match node.effective("clippingrange").map(|arg| &arg.data) {
            Some(OwnedData::F64(values)) => values.clone(),
            Some(OwnedData::F32(values)) => {
                values.iter().map(|value| *value as f64).collect()
            }
            _ => return object,
        };

    if values.len() < 2 {
        flushed.limitations.push(format!(
            "camera {handle:?} has a \"clippingrange\" of {} value(s) \
             rather than two; MoonRay's own near and far are used",
            values.len()
        ));
        return object;
    }

    let (near, far) = (values[0], values[1]);

    // A near plane at or behind the camera is not a clipping range,
    // and rdl2 clamps it to 0.01 without saying so -- which renders a
    // plausible image of a scene that was asking for something else.
    //
    // Spelled out rather than negated, so that NaN -- which fails
    // every comparison and would slip through a `<=` -- is refused
    // along with the rest.
    let usable =
        near.is_finite() && far.is_finite() && near > 0.0 && far > near;
    if !usable {
        flushed.limitations.push(format!(
            "camera {handle:?} has a \"clippingrange\" of \
             [{near}, {far}], which is not a near plane in front of a \
             far one; MoonRay's own near and far are used"
        ));
        return object;
    }

    object
        .set("near", Value::Float(near as f32))
        .set("far", Value::Float(far as f32))
}

/// Say when a perspective camera's screen window is not the one its
/// `fov` describes.
///
/// The interface's two settings work together: `fov` gives the angle
/// and `screenwindow` the rectangle it covers. MoonRay has only the
/// first -- `focal`, against a fixed film back -- so a scene that
/// narrows or shifts the window gets a frame `fov` alone decides.
///
/// **Reported rather than approximated.** A window scaled uniformly
/// could be folded into the focal length, but one that is not, or one
/// that is off centre, could not, and a rule that silently handles
/// some framings and not others is worse than one that handles none
/// and says which.
fn report_perspective_window(
    window: [f64; 4],
    resolution: (i32, i32),
    handle: &str,
    flushed: &mut Flushed,
) {
    let aspect = f64::from(resolution.0) / f64::from(resolution.1.max(1));
    let default = [-aspect, -1.0, aspect, 1.0];

    // Generous, because the default is computed from integers and a
    // scene that writes it out by hand should not be reported.
    let differs = window
        .iter()
        .zip(default.iter())
        .any(|(had, want)| (had - want).abs() > 1e-6 * want.abs().max(1.0));

    if differs {
        let [left, bottom, right, top] = window;
        flushed.limitations.push(format!(
            "camera {handle:?} sets a screen window \
             [{left}, {bottom}, {right}, {top}] rather than the \
             default for its frame; MoonRay has no screen window and \
             the framing follows \"fov\" alone"
        ));
    }
}

/// An orthographic camera's extent, which is the screen window and
/// nothing else.
///
/// **MoonRay has no screen-window attribute at all.**
/// `ProjectiveCamera::updateImpl` builds its normalised window from the
/// aperture viewport alone -- `[-1, -h/w, 1, h/w]`, every time -- so
/// there is nothing to carry `screenwindow` into directly. What the
/// orthographic projection multiplies that window by is
/// `film_width_aperture`, which is therefore the *width of the screen
/// window in world units*.
///
/// For a perspective camera the same scale is absorbed by the focal
/// length, which is why this matters here and nowhere else. And it
/// matters a lot: `film_width_aperture` defaults to **24**, so an
/// orthographic camera framing a unit-sized subject renders it about a
/// twelfth of the frame wide unless this is set. That reads as an
/// empty image rather than as a framing error.
fn orthographic_extent(
    object: Object,
    window: [f64; 4],
    resolution: (i32, i32),
    handle: &str,
    flushed: &mut Flushed,
) -> Object {
    let [left, bottom, right, top] = window;
    let width = right - left;
    let height = top - bottom;

    if width <= 0.0 || height <= 0.0 {
        flushed.limitations.push(format!(
            "camera {handle:?} has an empty screen window; MoonRay's \
             default aperture is used"
        ));
        return object;
    }

    // The vertical extent is not a separate attribute: MoonRay derives
    // it from the aperture's aspect ratio. A screen window shaped
    // differently from the image cannot be carried, and squashing it
    // silently would be a plausible render of the wrong framing.
    let image = f64::from(resolution.0) / f64::from(resolution.1.max(1));
    let asked = width / height;
    if (asked - image).abs() > 1e-6 * image.max(1.0) {
        flushed.limitations.push(format!(
            "camera {handle:?} has a screen window {asked:.4} wide for \
             every unit high while the image is {image:.4}; MoonRay \
             takes its vertical extent from the image, so the width is \
             carried and the height follows the frame"
        ));
    }

    let object = object.set("film_width_aperture", Value::Float(width as f32));

    // A window that is not centred becomes a film offset, in the same
    // world units.
    let (x, y) = ((left + right) / 2.0, (bottom + top) / 2.0);
    let object = if x != 0.0 {
        object.set("horizontal_film_offset", Value::Float(x as f32))
    } else {
        object
    };
    if y != 0.0 {
        object.set("vertical_film_offset", Value::Float(y as f32))
    } else {
        object
    }
}

fn camera_reference(scene: &Scene, handle: &str) -> Reference {
    Reference::new(camera_class(scene, handle), handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nsi_intermediate::OwnedArgument;
    use nsi_trait::Type;

    /// A translation along X, as ɴsɪ stores a matrix: row-major, with
    /// the translation in the last row.
    fn translation(x: f64) -> OwnedArgument {
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

    fn arg(name: &str, type_tag: Type, data: OwnedData) -> OwnedArgument {
        OwnedArgument::new(name, type_tag, 1, 0, data)
    }

    /// An argument whose values are fixed-size arrays -- how ɴsɪ spells a
    /// UV set, `float[2]`. The array length is what tells one value from
    /// two, and so what tells a per-vertex variable from a face-varying
    /// one.
    fn array_arg(
        name: &str,
        type_tag: Type,
        length: usize,
        data: OwnedData,
    ) -> OwnedArgument {
        OwnedArgument::new(name, type_tag, length, 0, data)
    }

    /// The triangle, wearing a named shader.
    fn emissive(shader: &str, parameters: &[OwnedArgument]) -> Scene {
        let mut scene = triangle();

        scene
            .create("attr", "attributes")
            .expect("a recordable edit");
        scene.create("emit", "shader").expect("a recordable edit");

        let mut attributes = vec![arg(
            "shaderfilename",
            Type::String,
            OwnedData::String(vec![shader.as_bytes().to_vec()]),
        )];
        attributes.extend(parameters.iter().cloned());
        scene
            .set_attribute("emit", attributes)
            .expect("a recordable edit");

        scene
            .connect("attr", None, "tri", "geometryattributes")
            .unwrap();
        scene
            .connect("emit", None, "attr", "surfaceshader")
            .unwrap();

        scene
    }

    /// A triangle, a camera, a screen and an output -- the smallest
    /// scene that is a scene.
    /// Two faces sharing an edge, so per-vertex and face-varying are
    /// different lengths and the expansion has something to do.
    fn two_quads() -> Scene {
        let mut scene = Scene::default();

        scene.create("mesh", "mesh").expect("a recordable edit");
        scene
            .set_attribute(
                "mesh",
                vec![
                    arg("nvertices", Type::I32, OwnedData::I32(vec![4, 4])),
                    arg(
                        "P.indices",
                        Type::I32,
                        OwnedData::I32(vec![0, 1, 4, 3, 1, 2, 5, 4]),
                    ),
                    arg(
                        "P",
                        Type::Point,
                        OwnedData::F32(vec![
                            0.0, 0.0, 0.0, // 0
                            1.0, 0.0, 0.0, // 1
                            2.0, 0.0, 0.0, // 2
                            0.0, 1.0, 0.0, // 3
                            1.0, 1.0, 0.0, // 4
                            2.0, 1.0, 0.0, // 5
                        ]),
                    ),
                ],
            )
            .expect("a recordable edit");
        scene.connect("mesh", None, ".root", "objects").unwrap();

        scene
    }

    fn uvs(rdla: &str) -> String {
        rdla.split("[\"uv_list\"] = ")
            .nth(1)
            .map(|rest| rest.split('}').next().unwrap_or_default().to_string())
            .unwrap_or_default()
    }

    /// **A per-vertex `st` is indexed the way `P` is.**
    ///
    /// Six values for eight face-vertices: the shared edge's two
    /// vertices are written twice, which is exactly what MoonRay's
    /// per-face-vertex `uv_list` wants and what a rename would get
    /// wrong.
    #[test]
    fn a_per_vertex_st_is_expanded_by_the_vertex_indices() {
        let mut scene = two_quads();
        scene
            .set_attribute(
                "mesh",
                vec![array_arg(
                    "st",
                    Type::F32,
                    2,
                    OwnedData::F32(vec![
                        0.0, 0.0, 0.5, 0.0, 1.0, 0.0, // the bottom row
                        0.0, 1.0, 0.5, 1.0, 1.0, 1.0, // the top row
                    ]),
                )],
            )
            .expect("a recordable edit");

        let rdla = flush(&scene).to_rdla();
        let uvs = uvs(&rdla);

        // Eight, in `P.indices` order: 0 1 4 3 1 2 5 4.
        assert_eq!(uvs.matches("Vec2(").count(), 8, "{rdla}");
        assert!(
            uvs.contains(
                "Vec2(0, 0), Vec2(0.5, 0), Vec2(0.5, 1), Vec2(0, 1), \
                 Vec2(0.5, 0), Vec2(1, 0), Vec2(1, 1), Vec2(0.5, 1)"
            ),
            "{uvs}"
        );
    }

    /// `st.indices` is ɴsɪ's own indirect lookup, and it wins over any
    /// inference from the length.
    #[test]
    fn an_indexed_st_is_looked_up() {
        let mut scene = two_quads();
        scene
            .set_attribute(
                "mesh",
                vec![
                    array_arg(
                        "st",
                        Type::F32,
                        2,
                        OwnedData::F32(vec![0.0, 0.0, 1.0, 1.0]),
                    ),
                    arg(
                        "st.indices",
                        Type::I32,
                        OwnedData::I32(vec![0, 1, 0, 1, 1, 0, 1, 0]),
                    ),
                ],
            )
            .expect("a recordable edit");

        let uvs = uvs(&flush(&scene).to_rdla());

        assert!(
            uvs.contains(
                "Vec2(0, 0), Vec2(1, 1), Vec2(0, 0), Vec2(1, 1), \
                 Vec2(1, 1), Vec2(0, 0), Vec2(1, 1), Vec2(0, 0)"
            ),
            "{uvs}"
        );
    }

    /// **A variable the count cannot decide is refused, not guessed.**
    ///
    /// A tetrahedron has four faces and four vertices, so a four-value
    /// variable on one is per-face or per-vertex depending on nothing
    /// the count can see -- and the two are different meshes. ɴsɪ's
    /// answer is the `per_face`/`per_vertex` flag, and `nsi-intermediate`
    /// refuses an unflagged one rather than picking.
    ///
    /// This backend used to pick, and picked uniform.
    #[test]
    fn a_variable_the_count_cannot_decide_is_reported() {
        let mut scene = Scene::default();
        scene.create("tet", "mesh").expect("a recordable edit");
        scene
            .set_attribute(
                "tet",
                vec![
                    arg("nvertices", Type::I32, OwnedData::I32(vec![3; 4])),
                    arg(
                        "P.indices",
                        Type::I32,
                        OwnedData::I32(vec![
                            0, 1, 2, 0, 2, 3, 0, 3, 1, 1, 3, 2,
                        ]),
                    ),
                    arg(
                        "P",
                        Type::Point,
                        OwnedData::F32(vec![
                            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, //
                            0.0, 1.0, 0.0, 0.0, 0.0, 1.0,
                        ]),
                    ),
                    // Four values, four faces, four vertices, and no
                    // flag to say which.
                    arg(
                        "heat",
                        Type::F32,
                        OwnedData::F32(vec![0.1, 0.2, 0.3, 0.4]),
                    ),
                ],
            )
            .expect("a recordable edit");
        scene.connect("tet", None, ".root", "objects").unwrap();

        let flushed = flush(&scene);

        assert!(!flushed.to_rdla().contains("heat"), "{}", flushed.to_rdla());
        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("heat")
                    && line.contains("not carried")),
            "{:?}",
            flushed.limitations
        );
    }

    /// A length that means nothing is said rather than reshaped into
    /// something plausible.
    #[test]
    fn an_st_of_no_recognisable_length_is_reported() {
        let mut scene = two_quads();
        scene
            .set_attribute(
                "mesh",
                vec![array_arg(
                    "st",
                    Type::F32,
                    2,
                    OwnedData::F32(vec![0.0, 0.0, 1.0, 1.0, 0.5, 0.5]),
                )],
            )
            .expect("a recordable edit");

        let flushed = flush(&scene);

        assert!(
            !flushed.to_rdla().contains("uv_list"),
            "{}",
            flushed.to_rdla()
        );
        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("was not carried")),
            "{:?}",
            flushed.limitations
        );
    }

    /// `N` travels the same road, into MoonRay's own per-face-vertex
    /// list.
    #[test]
    fn a_per_vertex_normal_becomes_a_normal_list() {
        let mut scene = two_quads();
        scene
            .set_attribute(
                "mesh",
                vec![arg(
                    "N",
                    Type::Normal,
                    OwnedData::F32(vec![
                        0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, //
                        0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0,
                    ]),
                )],
            )
            .expect("a recordable edit");

        let rdla = flush(&scene).to_rdla();

        assert!(rdla.contains("[\"normal_list\"] = {"), "{rdla}");
        // Eight, one a face-vertex, and the top row's `(0, 1, 0)` where
        // `P.indices` names vertices 3, 4 and 5.
        let normals = rdla
            .split("[\"normal_list\"] = ")
            .nth(1)
            .and_then(|rest| rest.split('}').next())
            .unwrap_or_default();
        assert_eq!(normals.matches("Vec3(").count(), 8, "{rdla}");
        assert_eq!(normals.matches("Vec3(0, 1, 0)").count(), 4, "{normals}");
    }

    /// **An attribute nobody declared crosses as `UserData`.**
    ///
    /// rdl2 declares a geometry's attributes statically, so this is the
    /// only route MoonRay has for one it could not know about -- and it
    /// is what answers an OSL shader's `getattribute()`.
    #[test]
    fn an_unknown_mesh_attribute_becomes_user_data() {
        let mut scene = two_quads();
        scene
            .set_attribute(
                "mesh",
                vec![arg(
                    "mytint",
                    Type::Color,
                    OwnedData::F32(vec![
                        1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, //
                        0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0,
                    ]),
                )],
            )
            .expect("a recordable edit");

        let rdla = flush(&scene).to_rdla();

        assert!(rdla.contains("UserData(\"mesh/mytint\") {"), "{rdla}");
        assert!(rdla.contains("[\"color_key\"] = \"mytint\""), "{rdla}");
        // Face-varying, always: `rate` 6. "Auto" would guess from the
        // count, and on a mesh where the counts coincide it can guess
        // wrong.
        assert!(rdla.contains("[\"rate\"] = 6,"), "{rdla}");
        // Expanded to eight, by `P.indices`, like everything else.
        let values = rdla
            .split("[\"color_values_0\"] = ")
            .nth(1)
            .and_then(|rest| rest.split('}').next())
            .unwrap_or_default();
        assert_eq!(values.matches("Rgb(").count(), 8, "{rdla}");
        // And the mesh points at it.
        assert!(
            rdla.contains(
                "[\"primitive_attributes\"] = { UserData(\"mesh/mytint\")}"
            ),
            "{rdla}"
        );
    }

    /// A mesh attribute this backend reads itself is not *also* a
    /// primitive variable: `st` is `uv_list`, and a `UserData` beside
    /// it would be the same values twice under two names.
    #[test]
    fn a_structural_attribute_is_not_user_data() {
        let mut scene = two_quads();
        scene
            .set_attribute(
                "mesh",
                vec![array_arg(
                    "st",
                    Type::F32,
                    2,
                    OwnedData::F32(vec![
                        0.0, 0.0, 0.5, 0.0, 1.0, 0.0, 0.0, 1.0, 0.5, 1.0, 1.0,
                        1.0,
                    ]),
                )],
            )
            .expect("a recordable edit");

        let rdla = flush(&scene).to_rdla();

        assert!(rdla.contains("uv_list"), "{rdla}");
        assert!(!rdla.contains("UserData"), "{rdla}");
    }

    /// An output layer with an ɴsɪ handle, a variable source and a
    /// name, connected to the scene's screen.
    fn output_layer(scene: &mut Scene, handle: &str, args: Vec<OwnedArgument>) {
        scene
            .create(handle, "outputlayer")
            .expect("a recordable edit");
        scene
            .set_attribute(handle, args)
            .expect("a recordable edit");
        scene
            .connect(handle, None, "screen", "outputlayers")
            .unwrap();
    }

    fn output(rdla: &str, handle: &str) -> String {
        rdla.split(&format!("RenderOutput(\"{handle}\") {{"))
            .nth(1)
            .and_then(|rest| rest.split('}').next())
            .unwrap_or_default()
            .to_string()
    }

    /// **`variablesource "builtin"` names a MoonRay result.**
    ///
    /// ɴsɪ's `z` is camera-space depth, which is what MoonRay's `depth`
    /// result is; `P.world` is a state variable, and MoonRay spells the
    /// world-space one `Wp`.
    #[test]
    fn a_builtin_output_layer_becomes_a_result() {
        let mut scene = triangle();
        output_layer(
            &mut scene,
            "z",
            vec![
                arg(
                    "variablesource",
                    Type::String,
                    OwnedData::String(vec![b"builtin".to_vec()]),
                ),
                arg(
                    "variablename",
                    Type::String,
                    OwnedData::String(vec![b"z".to_vec()]),
                ),
            ],
        );
        output_layer(
            &mut scene,
            "position",
            vec![
                arg(
                    "variablesource",
                    Type::String,
                    OwnedData::String(vec![b"builtin".to_vec()]),
                ),
                arg(
                    "variablename",
                    Type::String,
                    OwnedData::String(vec![b"P.world".to_vec()]),
                ),
            ],
        );

        let rdla = flush(&scene).to_rdla();

        assert!(
            output(&rdla, "z").contains("[\"result\"] = \"depth\""),
            "{rdla}"
        );
        let position = output(&rdla, "position");
        assert!(
            position.contains("[\"result\"] = \"state variable\""),
            "{position}"
        );
        assert!(
            position.contains("[\"state_variable\"] = \"Wp\""),
            "{position}"
        );
    }

    /// `variablesource "attribute"` reads a primitive variable, and
    /// MoonRay has to be told how many components it has -- rdl2 will
    /// not work it out from the data.
    #[test]
    fn an_attribute_output_layer_names_its_primitive_variable() {
        let mut scene = triangle();
        output_layer(
            &mut scene,
            "tint",
            vec![
                arg(
                    "variablesource",
                    Type::String,
                    OwnedData::String(vec![b"attribute".to_vec()]),
                ),
                arg(
                    "variablename",
                    Type::String,
                    OwnedData::String(vec![b"mytint".to_vec()]),
                ),
                arg(
                    "layertype",
                    Type::String,
                    OwnedData::String(vec![b"scalar".to_vec()]),
                ),
            ],
        );

        let tint = output(&flush(&scene).to_rdla(), "tint");

        assert!(
            tint.contains("[\"result\"] = \"primitive attribute\""),
            "{tint}"
        );
        assert!(
            tint.contains("[\"primitive_attribute\"] = \"mytint\""),
            "{tint}"
        );
        assert!(
            tint.contains("[\"primitive_attribute_type\"] = \"FLOAT\""),
            "{tint}"
        );
    }

    /// **A layer MoonRay has no result for is named, not defaulted.**
    ///
    /// Every unmapped case falls through to the beauty, because that is
    /// `RenderOutput`'s default and there is nothing else to be. An AOV
    /// that renders the beauty under another name is worse than a
    /// missing one, so it is said.
    #[test]
    fn an_output_layer_with_no_moonray_result_is_reported() {
        let mut scene = triangle();
        output_layer(
            &mut scene,
            "normal",
            vec![
                arg(
                    "variablesource",
                    Type::String,
                    OwnedData::String(vec![b"builtin".to_vec()]),
                ),
                // World-space normals. MoonRay's `N` is render space,
                // and answering with it would be the wrong space.
                arg(
                    "variablename",
                    Type::String,
                    OwnedData::String(vec![b"N.world".to_vec()]),
                ),
            ],
        );

        let flushed = flush(&scene);

        assert!(
            !output(&flushed.to_rdla(), "normal").contains("result"),
            "{}",
            flushed.to_rdla()
        );
        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("N.world")),
            "{:?}",
            flushed.limitations
        );
    }

    /// A shader AOV becomes a light-path expression naming the lobe.
    ///
    /// Not a *material* AOV: those name a lobe's properties -- its
    /// albedo, its roughness -- rather than what it contributed, and
    /// what an output layer with `variablesource "shader"` asks for is
    /// the contribution.
    #[test]
    fn a_shader_output_layer_becomes_a_light_path_expression() {
        let mut scene = triangle();
        output_layer(
            &mut scene,
            "spec",
            vec![
                arg(
                    "variablesource",
                    Type::String,
                    OwnedData::String(vec![b"shader".to_vec()]),
                ),
                // 3Delight's name for it. MoonRay's lobe label is
                // `specular`, and the two have to be reconciled or the
                // AOV renders black.
                arg(
                    "variablename",
                    Type::String,
                    OwnedData::String(vec![b"reflection".to_vec()]),
                ),
            ],
        );

        let spec = output(&flush(&scene).to_rdla(), "spec");

        assert!(spec.contains("[\"result\"] = \"light aov\""), "{spec}");
        assert!(spec.contains("[\"lpe\"] = \"C<..'specular'>L\""), "{spec}");
    }

    /// `Ci` is the beauty, which is `RenderOutput`'s own default -- so
    /// the commonest layer of all sets nothing.
    #[test]
    fn a_ci_output_layer_is_the_beauty() {
        let mut scene = triangle();
        output_layer(
            &mut scene,
            "beauty",
            vec![arg(
                "variablename",
                Type::String,
                OwnedData::String(vec![b"Ci".to_vec()]),
            )],
        );

        let beauty = output(&flush(&scene).to_rdla(), "beauty");

        assert!(!beauty.contains("result"), "{beauty}");
        assert!(!beauty.contains("lpe"), "{beauty}");
    }

    /// **A bound `volumeshader` does not cross, and says so.**
    ///
    /// The interface binds one through the `attributes` node the way it
    /// binds a surface or a displacement, and every volume here is
    /// rendered with MoonRay's stock `VdbVolume` instead. Silence would
    /// be the wrong answer twice over: the volume *appears*, so nothing
    /// looks broken, and what it looks like is the density grid with
    /// none of the shader's extinction, albedo, emission or
    /// anisotropy -- a plausible puff of smoke that is not the one the
    /// scene describes.
    #[test]
    fn a_bound_volume_shader_is_reported() {
        let mut scene = Scene::default();
        scene.create("smoke", "volume").expect("a recordable edit");
        scene
            .set_attribute(
                "smoke",
                vec![arg(
                    "vdbfilename",
                    Type::String,
                    OwnedData::String(vec![b"/tmp/explosion.vdb".to_vec()]),
                )],
            )
            .expect("a recordable edit");
        scene.connect("smoke", None, ".root", "objects").unwrap();

        scene
            .create("attr", "attributes")
            .expect("a recordable edit");
        scene.create("vol", "shader").expect("a recordable edit");
        scene
            .set_attribute(
                "vol",
                vec![arg(
                    "shaderfilename",
                    Type::String,
                    OwnedData::String(vec![
                        b"/opt/3delight/osl/dlAtmosphere.oso".to_vec(),
                    ]),
                )],
            )
            .expect("a recordable edit");
        scene
            .connect("attr", None, "smoke", "geometryattributes")
            .unwrap();
        scene.connect("vol", None, "attr", "volumeshader").unwrap();

        // With OSL running, because the point is that even then there
        // is no `VolumeShader` root to run it in.
        let flushed = flush_with(&scene, Purpose::default(), Shading::Osl);

        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("vol") && line.contains("volume")),
            "a bound volume shader must be reported\n{:?}",
            flushed.limitations
        );
        // And the volume still renders, through the stock shader.
        assert!(
            flushed.to_rdla().contains("VdbVolume("),
            "{}",
            flushed.to_rdla()
        );
    }

    /// **A `volume` node becomes a `VdbGeometry`.**
    ///
    /// The interface's volume node is OpenVDB and nothing else -- a
    /// file and named grids -- and MoonRay's only volume geometry reads
    /// exactly that, so the two meet with almost no translation. What
    /// does not cross is named: MoonRay reads a density grid and an
    /// emission grid, and has no notion of colour, temperature or
    /// emission intensity.
    #[test]
    fn a_volume_becomes_vdb_geometry() {
        let mut scene = Scene::default();
        scene.create("smoke", "volume").expect("a recordable edit");
        scene
            .set_attribute(
                "smoke",
                vec![
                    arg(
                        "vdbfilename",
                        Type::String,
                        OwnedData::String(vec![b"/tmp/explosion.vdb".to_vec()]),
                    ),
                    arg(
                        "densitygrid",
                        Type::String,
                        OwnedData::String(vec![b"density".to_vec()]),
                    ),
                    arg(
                        "temperaturegrid",
                        Type::String,
                        OwnedData::String(vec![b"temperature".to_vec()]),
                    ),
                ],
            )
            .expect("a recordable edit");
        scene.connect("smoke", None, ".root", "objects").unwrap();

        let flushed = flush(&scene);
        let rdla = flushed.to_rdla();

        assert!(rdla.contains("VdbGeometry(\"smoke\") {"), "{rdla}");
        assert!(
            rdla.contains("[\"model\"] = \"/tmp/explosion.vdb\""),
            "{rdla}"
        );
        assert!(rdla.contains("[\"density_grid\"] = \"density\""), "{rdla}");

        // The sixth column, not the third: a material there does
        // nothing and MoonRay renders the volume as nothing at all.
        assert!(
            rdla.contains(
                "{VdbGeometry(\"smoke\"), \"\", undef(), undef(), undef(), \
                 VdbVolume(\"/nsi/volume_shader\")"
            ),
            "{rdla}"
        );

        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("temperaturegrid")),
            "{:?}",
            flushed.limitations
        );
    }

    /// **Each camera node becomes its own MoonRay class.**
    ///
    /// The class has to travel with the handle: a scene shot through an
    /// orthographic camera and rendered in perspective is a perfectly
    /// good image of the wrong thing, and `SceneVariables` points at
    /// the camera by class *and* name, so getting it wrong there points
    /// at nothing at all.
    #[test]
    fn each_camera_node_becomes_its_own_class() {
        for (node_type, class) in [
            ("orthographiccamera", "OrthographicCamera"),
            ("fisheyecamera", "FisheyeCamera"),
            ("sphericalcamera", "SphericalCamera"),
        ] {
            let mut scene = triangle();
            // `triangle` brings a perspective camera; this replaces the
            // one the screen is connected to.
            scene.delete("cam").expect("a recordable edit");
            scene.create("cam", node_type).expect("a recordable edit");
            scene.connect("cam", None, ".root", "objects").unwrap();
            scene.connect("screen", None, "cam", "screens").unwrap();

            let rdla = flush(&scene).to_rdla();

            assert!(rdla.contains(&format!("{class}(\"cam\") {{")), "{rdla}");
            // And `SceneVariables` names it by that class.
            assert!(
                rdla.contains(&format!("[\"camera\"] = {class}(\"cam\")")),
                "{rdla}"
            );
        }
    }

    /// A fisheye's `fov` is an angle on both sides, so it crosses as
    /// itself rather than as a focal length -- and its mapping is the
    /// same idea under a slightly different spelling.
    #[test]
    fn a_fisheye_carries_its_field_of_view_and_mapping() {
        let mut scene = triangle();
        scene.delete("cam").expect("a recordable edit");
        scene
            .create("cam", "fisheyecamera")
            .expect("a recordable edit");
        scene
            .set_attribute(
                "cam",
                vec![
                    arg("fov", Type::F32, OwnedData::F32(vec![180.0])),
                    arg(
                        "mapping",
                        Type::String,
                        OwnedData::String(vec![b"equisolidangle".to_vec()]),
                    ),
                ],
            )
            .expect("a recordable edit");
        scene.connect("cam", None, ".root", "objects").unwrap();
        scene.connect("screen", None, "cam", "screens").unwrap();

        let rdla = flush(&scene).to_rdla();

        assert!(rdla.contains("[\"fov\"] = 180"), "{rdla}");
        // MoonRay spells it with a space.
        assert!(
            rdla.contains("[\"mapping\"] = \"equisolid angle\""),
            "{rdla}"
        );
        // Not a focal length: that is the perspective camera's answer.
        assert!(!rdla.contains("focal"), "{rdla}");
    }

    /// MoonRay has no cylindrical projection, and a camera that quietly
    /// became a perspective one would render a plausible wrong image.
    #[test]
    fn a_cylindrical_camera_is_reported() {
        let mut scene = triangle();
        scene.delete("cam").expect("a recordable edit");
        scene
            .create("cam", "cylindricalcamera")
            .expect("a recordable edit");
        scene.connect("cam", None, ".root", "objects").unwrap();

        let flushed = flush(&scene);

        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("cylindricalcamera")),
            "{:?}",
            flushed.limitations
        );
    }

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

        // Explicitly the substitute: this test is about what
        // `PARAMETERS` carries, and the default flips with
        // `$OSL_ROOT` at build time.
        let flushed =
            flush_with(&scene, Purpose::default(), Shading::Substitute);
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

    /// A shader names an OSL shader, and its parameters are that
    /// shader's, so the table is keyed on `shaderfilename`.
    ///
    /// `dlPrincipled` is 3Delight's supershader. Its base colour is
    /// `i_color` and its index of refraction is `refract_ior`, neither
    /// of which shares a name with anything on `UsdPreviewSurface`.
    /// Read off the shipped `.oso`; see `research.md` F11.
    #[test]
    fn a_known_shader_is_carried_by_its_own_parameter_names() {
        let mut scene = triangle();

        scene
            .create("attr", "attributes")
            .expect("a recordable edit");
        scene.create("shader", "shader").expect("a recordable edit");
        scene
            .set_attribute(
                "shader",
                vec![
                    arg(
                        "shaderfilename",
                        Type::String,
                        OwnedData::String(vec![b"dlPrincipled".to_vec()]),
                    ),
                    arg(
                        "i_color",
                        Type::Color,
                        OwnedData::F32(vec![0.25, 0.5, 0.75]),
                    ),
                    arg("refract_ior", Type::F32, OwnedData::F32(vec![1.6])),
                    arg("roughness", Type::F32, OwnedData::F32(vec![0.3])),
                ],
            )
            .expect("a recordable edit");
        scene
            .connect("attr", None, "tri", "geometryattributes")
            .unwrap();
        scene
            .connect("shader", None, "attr", "surfaceshader")
            .unwrap();

        // Explicitly the substitute: this test is about what
        // `PARAMETERS` carries, and the default flips with
        // `$OSL_ROOT` at build time.
        let flushed =
            flush_with(&scene, Purpose::default(), Shading::Substitute);
        let rdla = flushed.to_rdla();

        assert!(
            rdla.contains("[\"diffuseColor\"] = Rgb(0.25, 0.5, 0.75)"),
            "{rdla}"
        );
        assert!(rdla.contains("[\"ior\"] = 1.6"), "{rdla}");
        assert!(rdla.contains("[\"roughness\"] = 0.3"), "{rdla}");

        // The shader's own name is not a lost parameter.
        assert!(
            !flushed
                .limitations
                .iter()
                .any(|line| line.contains("shaderfilename")),
            "{:?}",
            flushed.limitations
        );
    }

    /// A shader the table does not know keeps the old behaviour:
    /// `UsdPreviewSurface`'s own names, matched exactly.
    #[test]
    fn an_unknown_shader_falls_back_to_exact_names() {
        let mut scene = triangle();

        scene
            .create("attr", "attributes")
            .expect("a recordable edit");
        scene.create("shader", "shader").expect("a recordable edit");
        scene
            .set_attribute(
                "shader",
                vec![
                    arg(
                        "shaderfilename",
                        Type::String,
                        OwnedData::String(vec![
                            b"/some/studio/osl/houseShader.oso".to_vec(),
                        ]),
                    ),
                    arg(
                        "diffuseColor",
                        Type::Color,
                        OwnedData::F32(vec![1.0, 0.0, 0.0]),
                    ),
                    arg("gloopiness", Type::F32, OwnedData::F32(vec![7.0])),
                ],
            )
            .expect("a recordable edit");
        scene
            .connect("attr", None, "tri", "geometryattributes")
            .unwrap();
        scene
            .connect("shader", None, "attr", "surfaceshader")
            .unwrap();

        // Explicitly the substitute: this test is about what
        // `PARAMETERS` carries, and the default flips with
        // `$OSL_ROOT` at build time.
        let flushed =
            flush_with(&scene, Purpose::default(), Shading::Substitute);
        let rdla = flushed.to_rdla();

        assert!(rdla.contains("[\"diffuseColor\"] = Rgb(1, 0, 0)"), "{rdla}");
        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("gloopiness")),
            "{:?}",
            flushed.limitations
        );
    }

    /// The shader is identified by the file stem, so a path and an
    /// extension do not hide it.
    #[test]
    fn a_shader_path_still_names_its_shader() {
        let mut scene = Scene::default();
        scene.create("shader", "shader").expect("a recordable edit");
        scene
            .set_attribute(
                "shader",
                vec![arg(
                    "shaderfilename",
                    Type::String,
                    OwnedData::String(vec![
                        b"/opt/3delight/osl/openPBRSurface.oso".to_vec(),
                    ]),
                )],
            )
            .expect("a recordable edit");

        let carried = parameters(scene.node("shader").expect("the node"));

        assert!(
            carried.iter().any(|(from, to)| *from == "baseColor"
                && *to == "diffuseColor"),
            "{carried:?}"
        );
    }

    /// Geometry wearing an emitter is a light, and stops being a
    /// shape.
    ///
    /// ɴsɪ has no light nodes at all (specification 4.5), so this is
    /// the only way an area light can arrive. The mesh is still
    /// emitted, because a `MeshLight` points at it, but it is out of
    /// the `Layer` -- `RenderContext::createMeshLightLayer` warns and
    /// skips a light whose geometry is in the main layer.
    #[test]
    fn a_mesh_wearing_an_emitter_becomes_a_mesh_light() {
        let scene = emissive("areaLight", &[]);

        let flushed = flush(&scene);
        let rdla = flushed.to_rdla();

        assert!(rdla.contains("MeshLight(\"tri/light\") {"), "{rdla}");
        assert!(
            rdla.contains("[\"geometry\"] = RdlMeshGeometry(\"tri\")"),
            "{rdla}"
        );
        // Seen as well as sampled. `MeshLight::intersect` ray-traces
        // the real mesh, so this is the whole of what ɴsɪ means by an
        // emissive mesh being ordinary geometry.
        assert!(rdla.contains("[\"visible_in_camera\"] = 1"), "{rdla}");
        assert!(
            rdla.contains(
                "LightSet(\"/nsi/lights\") {\n    MeshLight(\"tri/light\"),"
            ),
            "{rdla}"
        );

        // The mesh exists, and is in neither the layer nor the
        // geometry set.
        assert!(rdla.contains("RdlMeshGeometry(\"tri\") {"), "{rdla}");
        assert!(!rdla.contains("{RdlMeshGeometry(\"tri\"), \"\","), "{rdla}");
        assert!(
            !rdla.contains("GeometrySet(\"/nsi/geometries\") {\n    RdlMeshGeometry(\"tri\")"),
            "{rdla}"
        );

        // And the difference from ɴsɪ is said out loud.
        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("cannot also wear a material")),
            "{:?}",
            flushed.limitations
        );
    }

    /// The three parameters every 3Delight light shader declares are
    /// the three MoonRay's `Light` base class declares.
    #[test]
    fn a_lights_colour_and_intensity_cross() {
        let scene = emissive(
            "pointLight",
            &[
                arg(
                    "i_color",
                    Type::Color,
                    OwnedData::F32(vec![1.0, 0.5, 0.0]),
                ),
                arg("intensity", Type::F32, OwnedData::F32(vec![7.0])),
                arg("exposure", Type::F32, OwnedData::F32(vec![2.0])),
            ],
        );

        let rdla = flush(&scene).to_rdla();

        assert!(rdla.contains("SphereLight(\"tri/light\") {"), "{rdla}");
        assert!(rdla.contains("[\"color\"] = Rgb(1, 0.5, 0)"), "{rdla}");
        assert!(rdla.contains("[\"intensity\"] = 7"), "{rdla}");
        assert!(rdla.contains("[\"exposure\"] = 2"), "{rdla}");
    }

    /// A spot's cone angles, derived from the specification's own
    /// listing 4.3 rather than assumed.
    ///
    /// Both sides are full angles, and ɴsɪ's `penumbraAngle` is added
    /// to the *half* angle, so it counts double.
    #[test]
    fn a_spots_penumbra_widens_the_outer_cone_twice_over() {
        let scene = emissive(
            "spotLight",
            &[
                arg("coneAngle", Type::F32, OwnedData::F32(vec![40.0])),
                arg("penumbraAngle", Type::F32, OwnedData::F32(vec![5.0])),
            ],
        );

        let rdla = flush(&scene).to_rdla();

        assert!(rdla.contains("SpotLight(\"tri/light\") {"), "{rdla}");
        assert!(rdla.contains("[\"outer_cone_angle\"] = 50"), "{rdla}");
        assert!(rdla.contains("[\"inner_cone_angle\"] = 40"), "{rdla}");
    }

    /// A negative penumbra softens inward, so the outer cone is the
    /// one that stays put.
    #[test]
    fn a_negative_penumbra_narrows_the_inner_cone() {
        let scene = emissive(
            "spotLight",
            &[
                arg("coneAngle", Type::F32, OwnedData::F32(vec![60.0])),
                arg("penumbraAngle", Type::F32, OwnedData::F32(vec![-10.0])),
            ],
        );

        let rdla = flush(&scene).to_rdla();

        assert!(rdla.contains("[\"outer_cone_angle\"] = 60"), "{rdla}");
        assert!(rdla.contains("[\"inner_cone_angle\"] = 40"), "{rdla}");
    }

    /// The specification's own emitter spells the same two things
    /// `Cs` and `power`.
    #[test]
    fn the_specifications_emitter_is_recognised_too() {
        let scene = emissive(
            "emitter",
            &[
                arg("Cs", Type::Color, OwnedData::F32(vec![0.0, 1.0, 0.0])),
                arg("power", Type::F32, OwnedData::F32(vec![100.0])),
            ],
        );

        let rdla = flush(&scene).to_rdla();

        assert!(rdla.contains("MeshLight(\"tri/light\") {"), "{rdla}");
        assert!(rdla.contains("[\"color\"] = Rgb(0, 1, 0)"), "{rdla}");
        assert!(rdla.contains("[\"intensity\"] = 100"), "{rdla}");
    }

    /// A shader the table does not know leaves its geometry a shape.
    ///
    /// The safe direction to be wrong in: a mesh that should have been
    /// a light renders dark and visible, which looks like what it is.
    /// A mesh silently promoted to a light disappears.
    #[test]
    fn an_unknown_shader_leaves_its_geometry_a_shape() {
        let scene = emissive("houseEmitter", &[]);

        let flushed = flush(&scene);
        let rdla = flushed.to_rdla();

        assert!(!rdla.contains("MeshLight"), "{rdla}");
        assert!(rdla.contains("{RdlMeshGeometry(\"tri\"), \"\","), "{rdla}");
        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("renders black")),
            "{:?}",
            flushed.limitations
        );
    }

    /// A light disconnected from `.root` lights nothing.
    ///
    /// Switched off rather than left out, so that reconnecting it is an
    /// attribute edit rather than a change of set membership -- which
    /// would force a whole-scene re-apply.
    #[test]
    fn a_detached_light_is_switched_off() {
        let mut scene = triangle();
        scene
            .create("env", "environment")
            .expect("a recordable edit");
        scene.connect("env", None, ".root", "objects").unwrap();

        let connected = flush(&scene).to_rdla();
        assert!(connected.contains("EnvLight(\"env\") {\n}"), "{connected}");

        scene.disconnect("env", None, ".root", "objects").unwrap();
        let detached = flush(&scene).to_rdla();

        assert!(
            detached.contains("EnvLight(\"env\") {\n    [\"on\"] = false,"),
            "{detached}"
        );
        // Still in the set: membership is what a narrow update cannot
        // change.
        assert!(
            detached.contains(
                "LightSet(\"/nsi/lights\") {\n    EnvLight(\"env\"),"
            ),
            "{detached}"
        );
    }

    /// With OSL, an ɴsɪ shader crosses as itself.
    ///
    /// Not a substitute carrying the parameters this backend happens
    /// to recognise: the network becomes an OSL group specification
    /// and MoonRay runs it, so a parameter no table knows about
    /// arrives anyway.
    /// **A displacement shader becomes an `OslDisplacement`, not an
    /// `Osl`.**
    ///
    /// rdl2 refuses a second object under a name another class already
    /// holds -- measured, and it is a hard error at scene load, not a
    /// warning -- so which class a shader node becomes is decided by
    /// what it is bound to and there is exactly one answer per handle.
    #[test]
    fn an_nsi_displacement_shader_becomes_a_displacement() {
        let mut scene = triangle();
        scene
            .create("attr", "attributes")
            .expect("a recordable edit");
        scene.create("surf", "shader").expect("a recordable edit");
        scene.create("disp", "shader").expect("a recordable edit");
        for (handle, file) in [
            ("surf", "/opt/3delight/osl/dlPrincipled.oso"),
            ("disp", "/opt/3delight/osl/push.oso"),
        ] {
            scene
                .set_attribute(
                    handle,
                    vec![arg(
                        "shaderfilename",
                        Type::String,
                        OwnedData::String(vec![file.as_bytes().to_vec()]),
                    )],
                )
                .expect("a recordable edit");
        }
        scene
            .connect("attr", None, "tri", "geometryattributes")
            .unwrap();
        scene
            .connect("surf", None, "attr", "surfaceshader")
            .unwrap();
        scene
            .connect("disp", None, "attr", "displacementshader")
            .unwrap();

        let flushed = flush_with(&scene, Purpose::default(), Shading::Osl);
        let rdla = flushed.to_rdla();

        assert!(rdla.contains("OslDisplacement(\"disp\") {"), "{rdla}");
        // And not *also* as a material, which would not load.
        assert!(!rdla.contains("Osl(\"disp\") {"), "{rdla}");
        assert!(rdla.contains("shader push disp ;"), "{rdla}");
        // The fifth column of the layer row: geometry, part, material,
        // light set -- which this fixture has none of -- displacement.
        assert!(
            rdla.contains(
                "{RdlMeshGeometry(\"tri\"), \"\", Osl(\"surf\"), undef(), \
                 OslDisplacement(\"disp\")"
            ),
            "{rdla}"
        );
    }

    /// Without OSL there is nothing to run a displacement with, and no
    /// stand-in that moves vertices -- so it is said rather than
    /// silently dropped.
    #[test]
    fn a_displacement_without_osl_is_reported() {
        let mut scene = triangle();
        scene
            .create("attr", "attributes")
            .expect("a recordable edit");
        scene.create("disp", "shader").expect("a recordable edit");
        scene
            .set_attribute(
                "disp",
                vec![arg(
                    "shaderfilename",
                    Type::String,
                    OwnedData::String(vec![b"/opt/osl/push.oso".to_vec()]),
                )],
            )
            .expect("a recordable edit");
        scene
            .connect("attr", None, "tri", "geometryattributes")
            .unwrap();
        scene
            .connect("disp", None, "attr", "displacementshader")
            .unwrap();

        let flushed =
            flush_with(&scene, Purpose::default(), Shading::Substitute);

        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("is not displaced")),
            "{:?}",
            flushed.limitations
        );
        assert!(
            !flushed.to_rdla().contains("OslDisplacement"),
            "{}",
            flushed.to_rdla()
        );
    }

    #[test]
    fn an_nsi_shader_becomes_an_osl_material() {
        let mut scene = triangle();
        scene
            .create("attr", "attributes")
            .expect("a recordable edit");
        scene.create("shader", "shader").expect("a recordable edit");
        scene
            .set_attribute(
                "shader",
                vec![
                    arg(
                        "shaderfilename",
                        Type::String,
                        OwnedData::String(vec![
                            b"/opt/3delight/osl/dlPrincipled.oso".to_vec(),
                        ]),
                    ),
                    arg(
                        "i_color",
                        Type::Color,
                        OwnedData::F32(vec![0.1, 0.8, 0.2]),
                    ),
                    // A parameter `PARAMETERS` has never heard of, which
                    // the substitute would report as lost.
                    arg("sss_anisotropy", Type::F32, OwnedData::F32(vec![0.4])),
                ],
            )
            .expect("a recordable edit");
        scene
            .connect("attr", None, "tri", "geometryattributes")
            .unwrap();
        scene
            .connect("shader", None, "attr", "surfaceshader")
            .unwrap();

        let flushed = flush_with(&scene, Purpose::default(), Shading::Osl);
        let rdla = flushed.to_rdla();

        assert!(rdla.contains("Osl(\"shader\") {"), "{rdla}");
        assert!(rdla.contains("shader dlPrincipled shader ;"), "{rdla}");
        assert!(rdla.contains("param color i_color 0.1 0.8 0.2 ;"), "{rdla}");
        // The one the table does not know, carried all the same.
        assert!(rdla.contains("param float sss_anisotropy 0.4 ;"), "{rdla}");
        // The directory came off the shader's name, because OSL
        // resolves by name against a search path.
        assert!(
            rdla.contains("[\"search_path\"] = \"/opt/3delight/osl\""),
            "{rdla}"
        );
        // And the layer is what the row points at.
        assert!(
            rdla.contains("{RdlMeshGeometry(\"tri\"), \"\", Osl(\"shader\")"),
            "{rdla}"
        );
        assert!(
            !flushed
                .limitations
                .iter()
                .any(|line| line.contains("stands in for it")),
            "nothing stands in when OSL runs: {:?}",
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
        scene.connect("env", None, ".root", "objects").unwrap();

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

    /// **`clippingrange` becomes `near` and `far`, on every camera
    /// type.**
    ///
    /// MoonRay defaults to a near plane at 1 world unit, so a scene
    /// modelled in centimetres puts its whole subject inside it and
    /// renders nothing, with no error.
    #[test]
    fn a_clipping_range_becomes_near_and_far() {
        for node_type in ["perspectivecamera", "orthographiccamera"] {
            let mut scene = triangle();
            scene.delete("cam").expect("a recordable edit");
            scene.create("cam", node_type).expect("a recordable edit");
            scene.connect("cam", None, ".root", "objects").unwrap();
            scene.connect("screen", None, "cam", "screens").unwrap();
            scene
                .set_attribute(
                    "cam",
                    vec![
                        arg("fov", Type::F32, OwnedData::F32(vec![45.0])),
                        arg(
                            "clippingrange",
                            Type::F64,
                            OwnedData::F64(vec![0.01, 250.0]),
                        ),
                    ],
                )
                .expect("a recordable edit");

            let rdla = flush(&scene).to_rdla();

            assert!(
                rdla.contains("[\"near\"] = 0.00999999978,"),
                "{node_type}\n{rdla}"
            );
            assert!(rdla.contains("[\"far\"] = 250,"), "{node_type}\n{rdla}");
        }
    }

    /// A range that is not a near plane in front of a far one is
    /// reported: rdl2 clamps silently, and a clamped plane renders a
    /// plausible image of a scene nobody described.
    #[test]
    fn a_clipping_range_that_is_not_one_is_reported() {
        let mut scene = triangle();
        scene
            .set_attribute(
                "cam",
                vec![arg(
                    "clippingrange",
                    Type::F64,
                    OwnedData::F64(vec![0.0, -1.0]),
                )],
            )
            .expect("a recordable edit");

        let flushed = flush(&scene);

        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("\"cam\"")
                    && line.contains("clippingrange")),
            "{:?}",
            flushed.limitations
        );
        assert!(!flushed.to_rdla().contains("[\"near\"]"), "nothing is set");
    }

    /// A perspective camera's screen window is not carried, and the
    /// scene is told rather than left with a frame it did not ask for.
    #[test]
    fn a_perspective_screen_window_is_reported() {
        let mut scene = triangle();
        scene
            .set_attribute(
                "screen",
                vec![arg(
                    "screenwindow",
                    Type::F64,
                    OwnedData::F64(vec![-0.5, -0.5, 0.5, 0.5]),
                )],
            )
            .expect("a recordable edit");

        let flushed = flush(&scene);

        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("\"cam\"")
                    && line.contains("screen window")),
            "{:?}",
            flushed.limitations
        );
    }

    /// The default window is not a change, and is not reported. The
    /// specification computes it from the frame aspect ratio, so a
    /// scene that writes it out explicitly says nothing new.
    #[test]
    fn the_default_perspective_screen_window_is_not_reported() {
        let mut scene = triangle();
        // 320x240, so f = 4/3.
        scene
            .set_attribute(
                "screen",
                vec![arg(
                    "screenwindow",
                    Type::F64,
                    OwnedData::F64(vec![-4.0 / 3.0, -1.0, 4.0 / 3.0, 1.0]),
                )],
            )
            .expect("a recordable edit");

        let flushed = flush(&scene);

        assert!(
            !flushed
                .limitations
                .iter()
                .any(|line| line.contains("screen window")),
            "{:?}",
            flushed.limitations
        );
    }

    /// **An orthographic camera's extent comes from the screen
    /// window, and without it the framing is wrong by the scene's
    /// scale.**
    ///
    /// MoonRay builds its projection window from the aperture viewport
    /// alone and has no screen-window attribute, so `screenwindow`
    /// lands on `film_width_aperture` -- which defaults to 24 world
    /// units. A scene framing a unit-sized subject therefore rendered
    /// it a twelfth of the frame wide, which reads as an empty image.
    #[test]
    fn an_orthographic_camera_takes_its_extent_from_the_screen_window() {
        let mut scene = triangle();
        scene.delete("cam").expect("a recordable edit");
        scene
            .create("cam", "orthographiccamera")
            .expect("a recordable edit");
        scene.connect("cam", None, ".root", "objects").unwrap();
        scene.connect("screen", None, "cam", "screens").unwrap();
        // 320x240, so a window four wide and three high matches the
        // frame and nothing is reported.
        scene
            .set_attribute(
                "screen",
                vec![arg(
                    "screenwindow",
                    Type::F64,
                    OwnedData::F64(vec![-2.0, -1.5, 2.0, 1.5]),
                )],
            )
            .expect("a recordable edit");

        let flushed = flush(&scene);
        let rdla = flushed.to_rdla();

        assert!(
            rdla.contains("[\"film_width_aperture\"] = 4,"),
            "the window's width in world units\n{rdla}"
        );
        assert!(
            !rdla.contains("horizontal_film_offset"),
            "a centred window needs no offset\n{rdla}"
        );
        // Quoted, because a handle is quoted in a report and
        // "became" contains "cam".
        assert!(
            flushed
                .limitations
                .iter()
                .all(|line| !line.contains("\"cam\"")),
            "a window matching the frame is carried without comment: \
             {:?}",
            flushed.limitations
        );
    }

    /// The interface's default screen window is `[-f, -1], [f, 1]` for
    /// `f = xres/yres`, and it is what an orthographic camera gets when
    /// the scene sets none -- **not** MoonRay's 24.
    #[test]
    fn an_orthographic_camera_without_a_screen_window_uses_the_default() {
        let mut scene = triangle();
        scene.delete("cam").expect("a recordable edit");
        scene
            .create("cam", "orthographiccamera")
            .expect("a recordable edit");
        scene.connect("cam", None, ".root", "objects").unwrap();
        scene.connect("screen", None, "cam", "screens").unwrap();

        let rdla = flush(&scene).to_rdla();

        // 320/240 is 4/3, so the default window is 8/3 wide.
        assert!(
            rdla.contains("[\"film_width_aperture\"] = 2.66666675,"),
            "twice the frame aspect ratio, printed the way rdl2 prints \
             a float\n{rdla}"
        );
    }

    /// A window shaped differently from the image cannot be carried:
    /// MoonRay takes the vertical extent from the frame. Reported,
    /// because the alternative is a plausible render of the wrong
    /// framing.
    #[test]
    fn a_screen_window_that_does_not_match_the_frame_is_reported() {
        let mut scene = triangle();
        scene.delete("cam").expect("a recordable edit");
        scene
            .create("cam", "orthographiccamera")
            .expect("a recordable edit");
        scene.connect("cam", None, ".root", "objects").unwrap();
        scene.connect("screen", None, "cam", "screens").unwrap();
        // Square, on a 4:3 frame.
        scene
            .set_attribute(
                "screen",
                vec![arg(
                    "screenwindow",
                    Type::F64,
                    OwnedData::F64(vec![-1.0, -1.0, 1.0, 1.0]),
                )],
            )
            .expect("a recordable edit");

        let flushed = flush(&scene);

        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("cam") && line.contains("wide")),
            "{:?}",
            flushed.limitations
        );
        // The width is still carried; only the height cannot be.
        assert!(
            flushed.to_rdla().contains("[\"film_width_aperture\"] = 2,"),
            "{}",
            flushed.to_rdla()
        );
    }

    /// An off-centre window becomes a film offset, in the same world
    /// units.
    #[test]
    fn an_off_centre_screen_window_becomes_a_film_offset() {
        let mut scene = triangle();
        scene.delete("cam").expect("a recordable edit");
        scene
            .create("cam", "orthographiccamera")
            .expect("a recordable edit");
        scene.connect("cam", None, ".root", "objects").unwrap();
        scene.connect("screen", None, "cam", "screens").unwrap();
        scene
            .set_attribute(
                "screen",
                vec![arg(
                    "screenwindow",
                    Type::F64,
                    OwnedData::F64(vec![3.0, -1.5, 7.0, 1.5]),
                )],
            )
            .expect("a recordable edit");

        let rdla = flush(&scene).to_rdla();

        assert!(rdla.contains("[\"film_width_aperture\"] = 4,"), "{rdla}");
        assert!(
            rdla.contains("[\"horizontal_film_offset\"] = 5,"),
            "the centre of the window\n{rdla}"
        );
    }

    /// **Every camera node carries `shutterrange`, not just the
    /// perspective one.**
    ///
    /// The specification says so in as many words: "All camera nodes
    /// share a set of common attributes", and `shutterrange` is the
    /// second of them. Reading it off `perspectivecamera` alone leaves
    /// an orthographic shot falling back to the union of every motion
    /// time in the scene -- which is a *plausible* blur over the wrong
    /// interval, with nothing said anywhere.
    #[test]
    fn any_camera_node_carries_the_shutter_range() {
        for node_type in
            ["orthographiccamera", "fisheyecamera", "sphericalcamera"]
        {
            let mut scene = triangle();
            // `triangle` brings a perspective camera; this replaces the
            // one the screen is connected to.
            scene.delete("cam").expect("a recordable edit");
            scene.create("cam", node_type).expect("a recordable edit");
            scene.connect("cam", None, ".root", "objects").unwrap();
            scene.connect("screen", None, "cam", "screens").unwrap();
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
                "a {node_type} shutter decides the interval\n{rdla}"
            );
            // A quarter and three quarters of the way along a 0->4
            // move. Without the camera's range the union 0->1 is used,
            // and these become 0 and 4.
            assert!(
                rdla.contains(
                    "Mat4(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1)"
                ) && rdla.contains(
                    "Mat4(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 3, 0, 0, 1)"
                ),
                "a {node_type} shutter interpolates the transform to its \
                 ends\n{rdla}"
            );
        }
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

    /// A batch flush leaves hidden geometry out entirely.
    ///
    /// Nothing will show it again, so carrying it costs a
    /// tessellation and a place in the accelerator for something that
    /// will never be drawn. `T7.1`.
    #[test]
    fn a_batch_flush_omits_a_detached_shape() {
        let mut scene = triangle();
        scene
            .disconnect("tri", None, ".root", "objects")
            .expect("a recordable edit");

        let rdla = flush_for(&scene, Purpose::Batch).to_rdla();

        assert!(!rdla.contains("RdlMeshGeometry(\"tri\")"), "{rdla}");
        assert!(!rdla.contains("visible_in_camera"), "{rdla}");

        // And it is gone from the layer, not merely from the objects.
        let layer = rdla
            .split("Layer(\"/nsi/layer\") {")
            .nth(1)
            .expect("a layer");
        assert!(!layer.contains("\"tri\""), "{layer}");
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

    /// **`T6.3`.** A moving instancer blurs through `velocities`.
    ///
    /// `xform_list` carries no timesteps, so ɴsɪ's sampled
    /// `transformationmatrices` cannot cross as a `blur()` pair.
    /// MoonRay's route is a per-instance velocity, applied as
    /// `position + velocity * dt` with
    /// `dt = (motionStep - evaluationFrame) / fps` -- so the magnitude
    /// is `delta * fps / (close - open)`, and getting it wrong is a
    /// smear of the wrong length, which reads as a shutter setting.
    #[test]
    fn a_moving_instancer_gets_velocities() {
        let mut scene = triangle();
        scene
            .create("inst", "instances")
            .expect("a recordable edit");
        scene.connect("inst", None, ".root", "objects").unwrap();
        scene.create("proto", "mesh").expect("a recordable edit");
        scene
            .connect("proto", None, "inst", "sourcemodels")
            .unwrap();

        // One instance, travelling 6 units along X across [0, 1].
        for (time, x) in [(0.0, 0.0), (1.0, 6.0)] {
            scene
                .set_attribute_at_time(
                    "inst",
                    time,
                    vec![arg(
                        "transformationmatrices",
                        Type::MatrixF64,
                        OwnedData::F64(instance_matrix(x)),
                    )],
                )
                .expect("a recordable edit");
        }

        let flushed = flush(&scene);
        let rdla = flushed.to_rdla();

        // The shutter is [0, 1] and `fps` is 24, so 6 units across it
        // is 144 units per second.
        assert!(
            rdla.contains("[\"velocities\"] = { Vec3(144, 0, 0)}"),
            "delta * fps / (close - open) = 6 * 24 / 1\n{rdla}"
        );
        // `xform_list` is the shutter-open placement, and
        // `evaluation_frame` says so, making MoonRay's `dt` zero there.
        assert!(rdla.contains("[\"evaluation_frame\"] = 0"), "{rdla}");
        assert!(
            rdla.contains("[\"xform_list\"] = { Mat4(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)}"),
            "the open-shutter placement\n{rdla}"
        );
        // And the scene agrees about both numbers the maths used.
        assert!(rdla.contains("[\"fps\"] = 24"), "{rdla}");
        assert!(rdla.contains("[\"motion_steps\"] = { 0, 1}"), "{rdla}");

        assert!(
            flushed
                .limitations
                .iter()
                .any(|line| line.contains("only its **translation**")),
            "rotation and scale are not blurred, and that is reported: \
             {:?}",
            flushed.limitations
        );
    }

    /// A still instancer gets no velocities at all.
    #[test]
    fn a_still_instancer_has_no_velocities() {
        let rdla = flush(&two_instances()).to_rdla();
        assert!(!rdla.contains("velocities"), "{rdla}");
        assert!(!rdla.contains("evaluation_frame"), "{rdla}");
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

//! Replay a [`Document`] into a live `scene_rdl2` scene.
//!
//! The same structure the `.rdla` writer consumes, applied to objects
//! instead of written to a file. That is the whole reason the flush
//! produces a `Document` rather than text: **`.rdla` is now a dump**
//! -- `--print`, a bug report, an oracle diff -- and this is the
//! transport.
//!
//! The oracle tests keep covering the text path, and go on being worth
//! running: they check the *values* this backend computes without
//! needing a renderer, which is what lets the transport change
//! underneath them.
//!
//! # Nothing here refuses a scene
//!
//! Every failure becomes a line in the returned report and the walk
//! carries on. A scene with one unmappable attribute renders without
//! it; a render farm depends on that.

use crate::{
    document::{Body, Document, Object as Described},
    rdl2::{Context, Error, Object, Timestep},
    value::{Reference, Value},
};
use std::collections::HashMap;

/// `SceneVariables` has no name, and rdl2 reaches it by class alone.
const SCENE_VARIABLES: &str = "SceneVariables";

/// Apply only the objects an edit touched.
///
/// The synchronise loop's half of `apply`. One ɴsɪ attribute changing
/// should cost one rdl2 attribute write, not a scene rebuild --
/// MoonRay reuses tessellation and acceleration structures for
/// anything it was not told changed, and that reuse is the entire
/// point of an interactive render.
///
/// `affected` comes from `Scene::affected(&scene.take_changes())`
/// upstream, which answers "given this node changed, whose resolved
/// answers depended on it" by walking the graph *down*. Working that
/// out here would mean re-deriving ɴsɪ's scoping rules in a backend,
/// which is exactly the duplication `nsi-intermediate` exists to
/// prevent.
///
/// # When it does everything anyway
///
/// - `Affected::everything`, which upstream sets for an edit to
///   `.root` or `.global`. It does not fill `nodes` in that case, since
///   the answer is the whole scene.
/// - A node created or deleted. The `Layer`, the `GeometrySet` and the
///   `LightSet` are *membership*, and a member appearing or vanishing
///   changes them rather than any one object. Narrowing that is
///   `I6`'s business; getting it wrong renders a shape that should be
///   gone, or loses one that should be there.
///
/// Both are reported, because a full rebuild that renders correctly
/// and slowly is invisible in the image and shows up only as time.
pub fn apply_affected(
    document: &Document,
    context: &Context,
    changes: &nsi_intermediate::Changes,
    affected: &nsi_intermediate::Affected,
) -> (Vec<String>, bool) {
    if affected.everything {
        let mut report = apply(document, context);
        report.push(
            "an attribute on `.root` or `.global` changed, so the whole              scene was re-applied"
                .to_string(),
        );
        return (report, true);
    }

    if !changes.created.is_empty() || !changes.deleted.is_empty() {
        let mut report = apply(document, context);
        report.push(format!(
            "{} node(s) created and {} deleted, which changes set \
             and layer membership, so the whole scene was \
             re-applied",
            changes.created.len(),
            changes.deleted.len()
        ));
        return (report, true);
    }

    // The narrow path: only the objects whose handles upstream named.
    // `Object::name` is the ɴsɪ handle, which is what makes this a
    // filter rather than a second mapping.
    let touched = |described: &Described| {
        described.name.as_ref().is_some_and(|name| {
            // `Affected` borrows from the scene now rather than
            // owning copies of every handle, so these are `&str`.
            affected.nodes.contains(name.as_str())
                || affected.shaders.contains(name.as_str())
        })
    };

    let narrowed = Document {
        objects: document
            .objects
            .iter()
            .filter(|described| touched(described))
            .cloned()
            .collect(),
    };

    if narrowed.objects.is_empty() {
        return (Vec::new(), false);
    }

    (apply(&narrowed, context), false)
}

/// Apply a document, reporting what would not go across.
///
/// Two passes, and the reason is references: an attribute can name an
/// object the document declares later, so every object is created
/// before any attribute is set. A single pass would resolve a forward
/// reference to nothing and write `undef()` -- which, for a `Layer`
/// row's material, means MoonRay silently does not render the shape.
pub fn apply(document: &Document, context: &Context) -> Vec<String> {
    let mut report = Vec::new();
    let mut objects: HashMap<(String, String), Object<'_>> = HashMap::new();

    // Pass one: every object exists.
    for described in &document.objects {
        let name = object_name(described);
        let key = (described.class.clone(), name.clone());

        match context.object(&described.class, &name) {
            Some(object) => {
                objects.insert(key, object);
            }
            None => report.push(format!(
                "no scene class {:?} for {name:?}{}",
                described.class,
                context
                    .error()
                    .map(|error| format!(": {error}"))
                    .unwrap_or_default()
            )),
        }
    }

    // Pass two: the contents.
    for described in &document.objects {
        let name = object_name(described);
        let Some(object) =
            objects.get(&(described.class.clone(), name.clone()))
        else {
            continue;
        };

        let update = match object.update() {
            Ok(update) => update,
            Err(error) => {
                report.push(format!("{name:?}: {error}"));
                continue;
            }
        };

        match &described.body {
            Body::Attributes(attributes) => {
                for (attribute, value) in attributes {
                    if let Err(error) = set(
                        &update,
                        attribute,
                        value,
                        Timestep::WholeShutter,
                        &objects,
                        &mut report,
                    ) {
                        report.push(format!(
                            "{name:?} attribute {attribute:?}: {error}"
                        ));
                    }
                }
            }

            Body::Set(members) => {
                for member in members {
                    match resolve(member, &objects) {
                        Some(resolved) => {
                            if let Err(error) = update.add(resolved) {
                                report.push(format!(
                                    "{name:?}: {member} not added: {error}"
                                ));
                            }
                        }
                        None => report.push(format!(
                            "{name:?}: {member} is not in the document"
                        )),
                    }
                }
            }

            Body::Layer(assignments) => {
                for assignment in assignments {
                    let Some(geometry) = assignment
                        .geometry
                        .as_ref()
                        .and_then(|reference| resolve(reference, &objects))
                    else {
                        report.push(format!(
                            "{name:?}: a row names no geometry and was \
                             skipped"
                        ));
                        continue;
                    };

                    // A row whose material is `undef()` is *skipped* by
                    // MoonRay -- the shape is absent from the image, not
                    // merely unshaded -- so an unresolved material here
                    // is reported rather than passed through quietly.
                    let material = optional(
                        &assignment.material,
                        &objects,
                        &name,
                        "material",
                        &mut report,
                    );
                    let light_set = optional(
                        &assignment.light_set,
                        &objects,
                        &name,
                        "light set",
                        &mut report,
                    );

                    if let Err(error) = update.assign(
                        geometry,
                        &assignment.part,
                        material,
                        light_set,
                    ) {
                        report.push(format!(
                            "{name:?}: row not assigned: {error}"
                        ));
                    }
                }
            }
        }
    }

    report
}

/// rdl2 reaches `SceneVariables` by class; everything else by name.
fn object_name(described: &Described) -> String {
    described
        .name
        .clone()
        .unwrap_or_else(|| SCENE_VARIABLES.to_string())
}

fn resolve<'a>(
    reference: &Reference,
    objects: &HashMap<(String, String), Object<'a>>,
) -> Option<Object<'a>> {
    objects
        .get(&(reference.class.clone(), reference.name.clone()))
        .copied()
}

/// A column that may legitimately be absent, but must not be *silently*
/// absent when the document named something.
fn optional<'a>(
    reference: &Option<Reference>,
    objects: &HashMap<(String, String), Object<'a>>,
    layer: &str,
    what: &str,
    report: &mut Vec<String>,
) -> Option<Object<'a>> {
    let reference = reference.as_ref()?;
    let resolved = resolve(reference, objects);

    if resolved.is_none() {
        report.push(format!(
            "{layer:?}: {what} {reference} is not in the document, so the \
             row was left without one"
        ));
    }

    resolved
}

/// One attribute.
fn set(
    update: &crate::rdl2::Update<'_>,
    attribute: &str,
    value: &Value,
    timestep: Timestep,
    objects: &HashMap<(String, String), Object<'_>>,
    report: &mut Vec<String>,
) -> Result<(), Error> {
    match value {
        Value::Bool(value) => update.set_bool(attribute, *value, timestep),
        Value::Int(value) => update.set_int(attribute, *value, timestep),
        Value::Long(value) => update.set_long(attribute, *value, timestep),
        Value::Float(value) => update.set_float(attribute, *value, timestep),
        Value::Double(value) => update.set_double(attribute, *value, timestep),
        Value::String(value) => update.set_string(attribute, value, timestep),

        Value::Rgb(value) => update.set_rgb(attribute, value, timestep),
        Value::Rgba(value) => update.set_rgba(attribute, value, timestep),
        Value::Vec2f(value) => update.set_vec2f(attribute, value, timestep),
        Value::Vec2d(value) => update.set_vec2d(attribute, value, timestep),
        Value::Vec3f(value) => update.set_vec3f(attribute, value, timestep),
        Value::Vec3d(value) => update.set_vec3d(attribute, value, timestep),
        Value::Vec4f(value) => update.set_vec4f(attribute, value, timestep),
        Value::Vec4d(value) => update.set_vec4d(attribute, value, timestep),
        Value::Mat4f(value) => update.set_mat4f(attribute, value, timestep),
        Value::Mat4d(value) => update.set_mat4d(attribute, value, timestep),

        Value::Object(reference) => {
            let resolved = resolve(reference, objects);
            if resolved.is_none() {
                report.push(format!(
                    "attribute {attribute:?} names {reference}, which is \
                     not in the document"
                ));
            }
            update.set_object(attribute, resolved, timestep)
        }
        Value::Undef => update.set_object(attribute, None, timestep),

        // rdl2's two timesteps, which is exactly what `Blur` carries --
        // the emitter refuses to build one from more than two samples,
        // so there is nothing to flatten here.
        Value::Blur(begin, end) => {
            set(update, attribute, begin, Timestep::Begin, objects, report)?;
            set(update, attribute, end, Timestep::End, objects, report)
        }

        // A binding keeps its own value alongside, which the format
        // oracle settled: writing only the binding loses the fallback
        // the shader reads when nothing is connected.
        Value::Bind(target, value) => {
            set(update, attribute, value, timestep, objects, report)?;
            let resolved = resolve(target, objects);
            if resolved.is_none() {
                report.push(format!(
                    "attribute {attribute:?} is bound to {target}, which \
                     is not in the document"
                ));
            }
            update.set_binding(attribute, resolved)
        }

        Value::Vector(values) => vector(update, attribute, values, objects),
    }
}

/// A vector attribute, typed from its first element.
///
/// rdl2 has a distinct type per element type and the document does not
/// carry one for an empty list -- so an empty vector is left alone
/// rather than guessed at, which matches rdl2's own default.
fn vector(
    update: &crate::rdl2::Update<'_>,
    attribute: &str,
    values: &[Value],
    objects: &HashMap<(String, String), Object<'_>>,
) -> Result<(), Error> {
    let Some(first) = values.first() else {
        return Ok(());
    };

    /// Collect a flat buffer, refusing a list whose elements are not
    /// all the same shape -- which would otherwise write a buffer of
    /// the right length and the wrong contents.
    macro_rules! flat {
        ($variant:path, $components:literal) => {{
            let mut flat = Vec::with_capacity(values.len() * $components);
            for value in values {
                match value {
                    $variant(components) => flat.extend_from_slice(components),
                    _ => return Err(Error::TypeMismatch),
                }
            }
            flat
        }};
    }

    macro_rules! scalars {
        ($variant:path) => {{
            let mut flat = Vec::with_capacity(values.len());
            for value in values {
                match value {
                    $variant(scalar) => flat.push(*scalar),
                    _ => return Err(Error::TypeMismatch),
                }
            }
            flat
        }};
    }

    match first {
        Value::Int(_) => {
            update.set_int_vector(attribute, &scalars!(Value::Int))
        }
        Value::Long(_) => {
            update.set_long_vector(attribute, &scalars!(Value::Long))
        }
        Value::Float(_) => {
            update.set_float_vector(attribute, &scalars!(Value::Float))
        }
        Value::Double(_) => {
            update.set_double_vector(attribute, &scalars!(Value::Double))
        }
        Value::Rgb(_) => {
            update.set_rgb_vector(attribute, &flat!(Value::Rgb, 3))
        }
        Value::Vec2f(_) => {
            update.set_vec2f_vector(attribute, &flat!(Value::Vec2f, 2))
        }
        Value::Vec3f(_) => {
            update.set_vec3f_vector(attribute, &flat!(Value::Vec3f, 3))
        }
        Value::Vec3d(_) => {
            update.set_vec3d_vector(attribute, &flat!(Value::Vec3d, 3))
        }
        Value::Vec4f(_) => {
            update.set_vec4f_vector(attribute, &flat!(Value::Vec4f, 4))
        }
        Value::Mat4f(_) => {
            update.set_mat4f_vector(attribute, &flat!(Value::Mat4f, 16))
        }
        Value::Mat4d(_) => {
            update.set_mat4d_vector(attribute, &flat!(Value::Mat4d, 16))
        }

        Value::String(_) => {
            let mut strings = Vec::with_capacity(values.len());
            for value in values {
                match value {
                    Value::String(string) => strings.push(string.as_str()),
                    _ => return Err(Error::TypeMismatch),
                }
            }
            update.set_string_vector(attribute, &strings)
        }

        Value::Object(_) => {
            let mut resolved = Vec::with_capacity(values.len());
            for value in values {
                match value {
                    Value::Object(reference) => {
                        // A dangling reference in a set would silently
                        // shorten it, which for an instancer's
                        // `references` means instances drawing the
                        // wrong prototype.
                        resolved.push(
                            resolve(reference, objects)
                                .ok_or(Error::BadArgument)?,
                        );
                    }
                    _ => return Err(Error::TypeMismatch),
                }
            }
            update.set_object_vector(attribute, &resolved)
        }

        _ => Err(Error::TypeMismatch),
    }
}

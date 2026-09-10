//! An ɴsɪ shader network, as an OSL group specification.
//!
//! ɴsɪ *is* OSL. A `shader` node names a compiled `.oso` and carries
//! that shader's parameters; a named shader port is a connection
//! between two of them. There is nothing to translate -- the network
//! only has to be handed to OSL in the form OSL reads.
//!
//! # Why a string
//!
//! rdl2 declares a scene class's attributes **once**, statically, so
//! one `Osl` material class serving every OSL shader in existence
//! cannot have an attribute per parameter. OSL already solved this:
//! `ShadingSystem::ShaderGroupBegin` takes a *group specification* --
//! layers, parameter values and connections, as text.
//!
//! ```text
//! param color Cs 0.1 0.8 0.2 ;
//! param float roughness 0.25 ;
//! shader red layer1 ;
//! connect layer1.Cout layer2.Cs ;
//! ```
//!
//! So the whole network crosses in one `String` attribute, and this
//! module is a **text transformation** -- which is why it is tested
//! against strings and needs no renderer, exactly as the `.rdla`
//! emitter is. `specs/003-osl/research.md` O6.
//!
//! # What it does not do
//!
//! Nothing here decides what a shader *means*. That is the point: with
//! OSL running, the question "does this shader emit?" is answered by
//! executing it, not by recognising a name.

use nsi_intermediate::{EdgeKind, OwnedData, Scene};
use nsi_trait::Type;
use oslquery_petite::OslQuery;
use std::collections::HashSet;

/// Where `.oso` files are looked for when a shader names no directory.
///
/// The same variable OSL's own `ShadingSystem` reads, so a scene that
/// works with `oslc` and `oslinfo` works here.
const SHADER_PATH: &str = "OSL_SHADER_PATH";

/// A shader network, ready for `ShaderGroupBegin`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Group {
    /// The specification text, on **one line**.
    ///
    /// It travels as an rdl2 `String` attribute, which is one Lua
    /// string, and a raw newline inside one is a syntax error. OSL
    /// treats all whitespace alike, so a space costs nothing.
    pub spec: String,
    /// The ɴsɪ handles that became layers, in the order they are
    /// declared -- which is dependency order, because OSL binds a
    /// `connect` only once both layers exist.
    pub layers: Vec<String>,
    /// What could not be carried, by name. Reported rather than
    /// dropped: a parameter that silently does not arrive renders a
    /// plausible picture of the shader's default.
    pub dropped: Vec<String>,
}

/// Build the group specification for a shader network rooted at
/// `handle`.
///
/// The root is the shader an `attributes` node binds as
/// `surfaceshader`; everything reachable from it through incoming
/// connections is a layer of the same group.
pub fn group(scene: &Scene, root: &str) -> Group {
    let mut group = Group::default();
    let mut declared = HashSet::new();

    // Layers first, deepest first: OSL applies each `param` to the
    // *next* `shader` line, and a `connect` needs both its layers
    // declared already.
    declare(scene, root, &mut group, &mut declared);

    // Then the wiring. Collected after every layer exists, because a
    // connection naming a layer that has not been declared is an error
    // rather than a forward reference.
    for layer in &group.layers.clone() {
        for edge in scene.edges_to(layer) {
            if !group.layers.iter().any(|name| name == edge.from()) {
                continue;
            }
            // Upstream already classified this. A *named* source port
            // is what makes a connection a shader-network edge -- ɴsɪ
            // says so and `EdgeKind::classify` implements it -- so
            // anything else between two shaders is a node-level
            // connection and not wiring.
            let EdgeKind::ShaderNetwork { from_port, to_port } = &edge.kind
            else {
                group.dropped.push(format!(
                    "{}->{}: a connection between two shaders that names no \
                     output port, so it wires nothing",
                    edge.from(),
                    edge.to()
                ));
                continue;
            };
            // **This one is fatal.** A parameter OSL does not
            // recognise is a warning; a connection from something that
            // is not an output is
            //
            //     ERROR: ConnectShaders: "nope" is not a parameter or
            //            global of layer "a"
            //     ERROR: ShaderGroupBegin: error parsing group description
            //
            // and the group does not exist -- so the surface shades
            // nothing at all. Dropping the connection loses one input;
            // keeping it loses the shader.
            if let Some(source) = open(scene, edge.from())
                && !source
                    .param_by_name(from_port)
                    .is_some_and(|parameter| parameter.is_output())
            {
                group.dropped.push(format!(
                    "{}.{from_port}: not an output of that shader, so the \
                     connection to {}.{to_port} was dropped",
                    edge.from(),
                    edge.to()
                ));
                continue;
            }
            if let Some(destination) = open(scene, edge.to())
                && destination.param_by_name(to_port).is_none()
            {
                group.dropped.push(format!(
                    "{}.{to_port}: no such parameter, so the connection \
                     from {}.{from_port} was dropped",
                    edge.to(),
                    edge.from()
                ));
                continue;
            }

            group.spec.push_str(&format!(
                "connect {}.{from_port} {}.{to_port} ; ",
                layer_name(edge.from()),
                layer_name(edge.to())
            ));
        }
    }

    group
}

/// Declare one shader and everything it reads, deepest first.
fn declare(
    scene: &Scene,
    handle: &str,
    group: &mut Group,
    declared: &mut HashSet<String>,
) {
    if !declared.insert(handle.to_owned()) {
        return;
    }

    let Some(node) = scene.node(handle) else {
        return;
    };
    if node.node_type() != "shader" {
        return;
    }

    // Inputs before this layer, so `connect` always names two layers
    // that exist.
    for edge in scene.edges_to(handle) {
        declare(scene, edge.from(), group, declared);
    }

    let Some(shader) = shader_name(scene, handle) else {
        group.dropped.push(format!(
            "{handle}: a shader node with no \"shaderfilename\""
        ));
        return;
    };
    // What the shader actually declares, if the `.oso` can be found.
    //
    // **Three mistakes, three different silences.** Measured against
    // OSL 1.13 rather than assumed:
    //
    // | Spec says | OSL does |
    // | --- | --- |
    // | a parameter the shader lacks | warns on its error handler, shades on |
    // | a value for an *output* | **accepts it, ignores it, says nothing** |
    // | a connection from a non-output | **refuses the whole group** |
    //
    // The middle one is the reason this check earns its keep: nothing
    // anywhere reports it, and the shader renders its default. The
    // first is a warning that names the parameter but not the ɴsɪ node
    // that asked for it, on a handler this backend does not own. The
    // last is checked where the connections are emitted.
    //
    // A shader that cannot be found is not an error here. It may be on
    // the renderer's search path and not on ours -- the flush is a
    // pure transformation and may run on a different machine -- so
    // what cannot be checked is emitted unchecked.
    let declared = open(scene, handle);

    for (name, argument) in node.attributes() {
        if name == "shaderfilename" {
            continue;
        }

        if let Some(query) = &declared {
            match query.param_by_name(name) {
                None => {
                    group.dropped.push(format!(
                        "{handle}.{name}: {shader} declares no such \
                         parameter"
                    ));
                    continue;
                }
                Some(parameter) if parameter.is_output() => {
                    group.dropped.push(format!(
                        "{handle}.{name}: {shader} declares it an output, \
                         which cannot be set"
                    ));
                    continue;
                }
                Some(_) => {}
            }
        }

        match parameter(name, argument.type_tag, &argument.data) {
            Some(line) => group.spec.push_str(&line),
            None => group.dropped.push(format!(
                "{handle}.{name}: no OSL spelling for its ɴsɪ type"
            )),
        }
    }

    group
        .spec
        .push_str(&format!("shader {shader} {} ;\n", layer_name(handle)));
    group.layers.push(handle.to_owned());
}

/// One `param` line, or `None` for a type OSL's specification syntax
/// has no spelling for.
fn parameter(name: &str, type_tag: Type, data: &OwnedData) -> Option<String> {
    let (osl_type, values) = match (type_tag, data) {
        (Type::F32, OwnedData::F32(values)) => {
            ("float", values.iter().map(number).collect::<Vec<_>>())
        }
        (Type::F64, OwnedData::F64(values)) => (
            "float",
            values
                .iter()
                .map(|value| number(&(*value as f32)))
                .collect(),
        ),
        (Type::I32, OwnedData::I32(values)) => (
            "int",
            values.iter().map(|value| value.to_string()).collect(),
        ),
        (Type::Color, OwnedData::F32(values)) => {
            ("color", values.iter().map(number).collect())
        }
        (Type::Point, OwnedData::F32(values)) => {
            ("point", values.iter().map(number).collect())
        }
        (Type::Vector, OwnedData::F32(values)) => {
            ("vector", values.iter().map(number).collect())
        }
        (Type::Normal, OwnedData::F32(values)) => {
            ("normal", values.iter().map(number).collect())
        }
        (Type::MatrixF32, OwnedData::F32(values)) => {
            ("matrix", values.iter().map(number).collect())
        }
        (Type::MatrixF64, OwnedData::F64(values)) => (
            "matrix",
            values
                .iter()
                .map(|value| number(&(*value as f32)))
                .collect(),
        ),
        (Type::String, OwnedData::String(values)) => {
            ("string", values.iter().map(|value| quoted(value)).collect())
        }
        // `Reference` is a host pointer, which cannot cross into a
        // scene description at all; anything else is a mismatch
        // between the tag and the data and is upstream's to explain.
        _ => return None,
    };

    if values.is_empty() {
        return None;
    }

    Some(format!("param {osl_type} {name} {} ; ", values.join(" ")))
}

/// A float, printed so OSL's parser reads back what ɴsɪ recorded.
///
/// `{}` on an `f32` prints the shortest string that round-trips, which
/// is what is wanted -- but it prints an integral value without a
/// decimal point, and OSL's group parser reads `1` as an int and
/// refuses to bind it to a float parameter.
fn number(value: &f32) -> String {
    let printed = format!("{value}");
    if printed.contains(['.', 'e', 'E', 'n', 'i']) {
        printed
    } else {
        format!("{printed}.0")
    }
}

/// A string parameter, quoted for the specification syntax.
fn quoted(value: &[u8]) -> String {
    let text = String::from_utf8_lossy(value);
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// The shader an ɴsɪ node names, as OSL wants it: the file stem.
///
/// OSL resolves a shader by name against its search path and appends
/// `.oso` itself, so a path or an extension has to come off -- and the
/// directory is what `search_path` on the material carries.
fn shader_name(scene: &Scene, handle: &str) -> Option<String> {
    let node = scene.node(handle)?;
    let OwnedData::String(names) = &node.effective("shaderfilename")?.data
    else {
        return None;
    };

    let name = String::from_utf8_lossy(names.first()?).into_owned();
    let after_slash = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let stem = after_slash.strip_suffix(".oso").unwrap_or(after_slash);

    (!stem.is_empty()).then(|| stem.to_owned())
}

/// The declarations of the shader an ɴsɪ node names, if they can be
/// found.
///
/// `shaderfilename` may be a path or a bare name; `oslquery-petite`
/// resolves both, appending `.oso` and walking a search path exactly
/// as OSL does. `$OSL_SHADER_PATH` is the same variable OSL's own
/// `ShadingSystem` reads, so a scene that works with `oslc` and
/// `oslinfo` works here.
///
/// `None` is not a failure: the shader may be on the renderer's search
/// path and not on this machine's, since the flush is a pure
/// transformation and may run somewhere else entirely. What cannot be
/// checked is emitted unchecked.
fn open(scene: &Scene, handle: &str) -> Option<OslQuery> {
    let node = scene.node(handle)?;
    let OwnedData::String(names) = &node.effective("shaderfilename")?.data
    else {
        return None;
    };

    let name = String::from_utf8_lossy(names.first()?).into_owned();
    let search = std::env::var(SHADER_PATH).unwrap_or_default();

    OslQuery::open_with_searchpath(&name, &search).ok()
}

/// Whether a compiled shader produces an `emission()` closure.
///
/// **The interface has no light nodes.** Section 4.5 says a light is
/// geometry whose surface shader emits, so recognising one means
/// knowing what a shader *does* -- and this backend has been answering
/// that with a table of six names, which makes every other emitter
/// render dark.
///
/// `.oso` is a text format, and OSL compiles a closure call into a
/// string constant naming the closure plus a `closure` instruction
/// using it:
///
/// ```text
/// const   string  $const1 "emission"
///         closure     $tmp1 $const1
/// ```
///
/// So both halves are checked. **A parameter called `emission_color`
/// is not an emitter** -- it compiles to a `param` line and no
/// constant -- and requiring the instruction as well means a shader
/// that merely holds the *word* somewhere is not promoted either.
///
/// That direction matters. A mesh wrongly left as geometry renders
/// dark and visible, which looks like what it is; a mesh wrongly
/// promoted to a light leaves the render layer and **disappears from
/// the frame**, so a false positive is much the worse mistake.
fn closures_of_oso(source: &str) -> Vec<String> {
    // `const  string  $const1  "emission"` -- the closure's name, held
    // as a string constant.
    let mut constants: Vec<(&str, &str)> = Vec::new();
    for line in source.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 4
            && fields[0] == "const"
            && fields[1] == "string"
            && fields[3].starts_with('"')
            && fields[3].ends_with('"')
        {
            constants.push((fields[2], fields[3].trim_matches('"')));
        }
    }

    // `closure  $tmp1 $const1` -- the instruction that builds one.
    let mut built = Vec::new();
    for line in source.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.first() != Some(&"closure") {
            continue;
        }
        for field in &fields {
            if let Some((_, name)) =
                constants.iter().find(|(constant, _)| constant == field)
            {
                built.push((*name).to_owned());
            }
        }
    }

    built.sort_unstable();
    built.dedup();
    built
}

/// Whether a compiled shader builds an `emission()` closure.
fn emits_from_oso(source: &str) -> bool {
    closures_of_oso(source)
        .iter()
        .any(|name| name == "emission")
}

/// Whether a compiled shader builds anything *besides* emission.
///
/// **This is what decides whether a light can also be a surface**, and
/// MoonRay's answer is that it cannot. `RenderContext::createMeshLightLayer`
/// warns and skips the light outright when its geometry is in the
/// render layer:
///
/// ```text
/// "..." cannot be referenced in a MeshLight when it is in
/// "Layer(...)". Please use a different geometry.
/// ```
///
/// So a shader that is ninety percent metal with emissive patches has
/// no faithful mapping: as a light it loses the metal, and as a surface
/// its glow is hit-only and lights nothing. Duplicating the geometry
/// does not rescue it either -- the material's own emission and the
/// light's sampled emission would both contribute and the glow would
/// count twice.
///
/// The surface is kept, because a metal object rendering as a
/// featureless emitter is the more visibly wrong of the two, and the
/// loss is reported.
fn shades_from_oso(source: &str) -> bool {
    closures_of_oso(source)
        .iter()
        .any(|name| name != "emission")
}

/// Whether the shader an ɴsɪ node names emits.
///
/// `None` when the compiled shader cannot be found, which is not a
/// failure: the flush is a pure transformation and may run on a
/// machine that has no shaders at all. What cannot be checked is left
/// to the name table.
pub fn emits(scene: &Scene, handle: &str) -> Option<bool> {
    let path = oso_path(scene, handle)?;
    let source = std::fs::read_to_string(path).ok()?;
    Some(emits_from_oso(&source))
}

/// Whether the shader an ɴsɪ node names also *shades* -- that is,
/// builds any closure besides emission.
///
/// See `shades_from_oso` for why this decides the mapping.
pub fn shades(scene: &Scene, handle: &str) -> Option<bool> {
    let path = oso_path(scene, handle)?;
    let source = std::fs::read_to_string(path).ok()?;
    Some(shades_from_oso(&source))
}

/// Where the compiled shader actually is.
///
/// The same resolution `open` performs -- a name or a path, against
/// OSL's own search path -- but yielding the file rather than its
/// declarations, because what is wanted here is the instruction stream
/// and `oslquery-petite` reads only the interface.
fn oso_path(scene: &Scene, handle: &str) -> Option<std::path::PathBuf> {
    let node = scene.node(handle)?;
    let OwnedData::String(names) = &node.effective("shaderfilename")?.data
    else {
        return None;
    };

    let name = String::from_utf8_lossy(names.first()?).into_owned();
    let with_extension = if name.ends_with(".oso") {
        name.clone()
    } else {
        format!("{name}.oso")
    };

    let direct = std::path::PathBuf::from(&with_extension);
    if direct.is_file() {
        return Some(direct);
    }

    // A bare name, against OSL's own search path.
    let search = std::env::var(SHADER_PATH).unwrap_or_default();
    for directory in search.split(':').filter(|entry| !entry.is_empty()) {
        let candidate = std::path::Path::new(directory).join(&with_extension);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

/// Whether a shader node names a shader OSL could run.
///
/// The one thing that decides whether the network is usable at all: a
/// `shader` node with no `shaderfilename` has nothing for OSL to load,
/// and an `Osl` material built from it would shade black. Cheap enough
/// to ask twice -- once when the material is emitted and once when the
/// `Layer` row that points at it is built -- which is what keeps the
/// two from disagreeing about the class.
pub fn is_runnable(scene: &Scene, handle: &str) -> bool {
    scene
        .node(handle)
        .is_some_and(|node| node.node_type() == "shader")
        && shader_name(scene, handle).is_some()
}

/// The directory an ɴsɪ shader's `.oso` lives in, if it named one.
///
/// OSL takes a search path rather than a filename, so a scene that
/// spells its shaders absolutely has to have the directory lifted out
/// of the name.
pub fn search_path(scene: &Scene, handle: &str) -> Option<String> {
    let node = scene.node(handle)?;
    let OwnedData::String(names) = &node.effective("shaderfilename")?.data
    else {
        return None;
    };

    let name = String::from_utf8_lossy(names.first()?).into_owned();
    let cut = name.rfind(['/', '\\'])?;

    Some(name[..cut].to_owned())
}

/// An ɴsɪ handle as an OSL layer name.
///
/// OSL's group syntax separates a layer from a parameter with a `.`,
/// so a handle carrying one -- and ɴsɪ handles routinely do, being
/// paths -- would make `connect a.b.c d.e` ambiguous.
fn layer_name(handle: &str) -> String {
    handle.replace('.', "_")
}

#[cfg(test)]
mod tests {
    /// **A light is a shader that emits, not a shader with a known
    /// name.** The interface says so in as many words, and a table of
    /// six names answers it for six shaders.
    #[test]
    fn an_emission_closure_is_found_in_a_compiled_shader() {
        // What `oslc` produces for `Ci = emission()`.
        let emitting = "\
surface emits
global\tclosure color\tCi
temp\tclosure color\t$tmp1
const\tstring\t$const1\t\"emission\"
code ___main___
\tclosure\t\t$tmp1 $const1
\tend
";
        assert!(emits_from_oso(emitting));
    }

    /// **A parameter called `emission_color` is not an emitter.**
    ///
    /// It compiles to a `param` line and no string constant, and this
    /// is the direction that matters: a mesh wrongly promoted to a
    /// light leaves the render layer and disappears from the frame,
    /// where one wrongly left as geometry merely renders dark.
    #[test]
    fn a_parameter_named_for_emission_is_not_an_emitter() {
        let plain = "\
surface param
param\tcolor\temission_color\t0 0 0
code ___main___
\tmul\t\tCi $tmp1 emission_color
\tend
";
        assert!(!emits_from_oso(plain));
    }

    /// **Against `oslc`'s own output**, because the three tests above
    /// assert on text this file wrote. A compiler that changes how it
    /// spells a closure would pass all of them and break the feature.
    ///
    /// Needs `$OSL_ROOT`, like the other tests that compile a shader.
    #[cfg(osl)]
    #[test]
    fn oslc_agrees_about_what_emits() {
        let directory = std::env::temp_dir()
            .join(format!("nsi-moonray-emits-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a directory");

        let oslc = std::path::Path::new(env!("OSL_ROOT")).join("bin/oslc");

        for (name, source, expected) in [
            (
                "emits",
                "surface emits() { Ci = emission() * color(1, 1, 1); }",
                true,
            ),
            ("plain", "surface plain() { Ci = diffuse(N); }", false),
            (
                "named",
                "surface named(color emission_color = 0) \
                 { Ci = diffuse(N) * emission_color; }",
                false,
            ),
        ] {
            let osl = directory.join(format!("{name}.osl"));
            let oso = directory.join(format!("{name}.oso"));
            std::fs::write(&osl, source).expect("the shader is written");

            let compiled = std::process::Command::new(&oslc)
                .arg("-o")
                .arg(&oso)
                .arg(&osl)
                .output()
                .expect("oslc runs");
            assert!(
                compiled.status.success(),
                "oslc failed for {name}: {}",
                String::from_utf8_lossy(&compiled.stderr)
            );

            let text = std::fs::read_to_string(&oso).expect("the .oso");
            assert_eq!(
                emits_from_oso(&text),
                expected,
                "{name} was read wrongly"
            );
        }
    }

    /// A shader that merely holds the word, with no closure built from
    /// it, is not an emitter either.
    #[test]
    fn a_string_constant_alone_is_not_an_emitter() {
        let held = "\
surface holds
const\tstring\t$const1\t\"emission\"
code ___main___
\tprintf\t\t$const1
\tend
";
        assert!(!emits_from_oso(held));
    }

    use super::*;
    use nsi_intermediate::OwnedArgument;

    fn argument(name: &str, type_tag: Type, data: OwnedData) -> OwnedArgument {
        OwnedArgument::new(name, type_tag, 1, 0, data)
    }

    fn shader(scene: &mut Scene, handle: &str, file: &str) {
        scene.create(handle, "shader").expect("a recordable edit");
        scene
            .set_attribute(
                handle,
                vec![argument(
                    "shaderfilename",
                    Type::String,
                    OwnedData::String(vec![file.as_bytes().to_vec()]),
                )],
            )
            .expect("a recordable edit");
    }

    /// One shader, with its parameters, is one layer.
    #[test]
    fn a_shader_becomes_a_layer_with_its_parameters() {
        let mut scene = Scene::default();
        shader(&mut scene, "surface", "dlPrincipled");
        scene
            .set_attribute(
                "surface",
                vec![
                    argument(
                        "i_color",
                        Type::Color,
                        OwnedData::F32(vec![0.1, 0.8, 0.2]),
                    ),
                    argument(
                        "roughness",
                        Type::F32,
                        OwnedData::F32(vec![0.25]),
                    ),
                ],
            )
            .expect("a recordable edit");

        let group = group(&scene, "surface");

        assert!(
            group.spec.contains("param color i_color 0.1 0.8 0.2 ;"),
            "{}",
            group.spec
        );
        assert!(
            group.spec.contains("param float roughness 0.25 ;"),
            "{}",
            group.spec
        );
        assert!(
            group.spec.contains("shader dlPrincipled surface ;"),
            "{}",
            group.spec
        );
        assert_eq!(group.layers, ["surface"]);
        assert!(group.dropped.is_empty(), "{:?}", group.dropped);
    }

    /// An integral float keeps its decimal point.
    ///
    /// OSL's group parser reads `1` as an int and refuses to bind it
    /// to a float parameter, so a shader whose roughness happened to
    /// be exactly 1 would fail to load -- and the message names the
    /// parameter, not the number.
    #[test]
    fn an_integral_float_is_still_a_float() {
        let mut scene = Scene::default();
        shader(&mut scene, "s", "test");
        scene
            .set_attribute(
                "s",
                vec![argument("gain", Type::F32, OwnedData::F32(vec![1.0]))],
            )
            .expect("a recordable edit");

        let group = group(&scene, "s");

        assert!(
            group.spec.contains("param float gain 1.0 ;"),
            "{}",
            group.spec
        );
    }

    /// A named shader port is a `connect`, and the layers it names are
    /// declared before it.
    #[test]
    fn a_named_port_becomes_a_connect() {
        let mut scene = Scene::default();
        shader(&mut scene, "texture", "dlTexture");
        shader(&mut scene, "surface", "dlPrincipled");
        scene
            .connect("texture", Some("outColor"), "surface", "i_color")
            .expect("a recordable edit");

        let group = group(&scene, "surface");

        // Deepest first, so a `connect` never names a layer OSL has
        // not seen.
        assert_eq!(group.layers, ["texture", "surface"]);
        let texture = group.spec.find("shader dlTexture texture ;");
        let surface = group.spec.find("shader dlPrincipled surface ;");
        let connect = group
            .spec
            .find("connect texture.outColor surface.i_color ;");
        assert!(texture < surface, "{}", group.spec);
        assert!(surface < connect, "{}", group.spec);
    }

    /// A handle with a dot cannot be a layer name as it stands.
    #[test]
    fn a_dotted_handle_is_renamed() {
        let mut scene = Scene::default();
        shader(&mut scene, "/set/wall.surface", "dlPrincipled");

        let group = group(&scene, "/set/wall.surface");

        assert!(
            group
                .spec
                .contains("shader dlPrincipled /set/wall_surface ;"),
            "{}",
            group.spec
        );
    }

    /// A shader path yields both the shader name and a search path.
    #[test]
    fn a_path_is_split_into_a_name_and_a_directory() {
        let mut scene = Scene::default();
        shader(&mut scene, "s", "/opt/3delight/osl/dlPrincipled.oso");

        let group = group(&scene, "s");

        assert!(
            group.spec.contains("shader dlPrincipled s ;"),
            "{}",
            group.spec
        );
        assert_eq!(
            search_path(&scene, "s").as_deref(),
            Some("/opt/3delight/osl")
        );
    }

    /// A string parameter is quoted, and a quote inside it escaped.
    #[test]
    fn a_string_parameter_is_quoted() {
        let mut scene = Scene::default();
        shader(&mut scene, "s", "dlTexture");
        scene
            .set_attribute(
                "s",
                vec![argument(
                    "texturename",
                    Type::String,
                    OwnedData::String(vec![
                        br#"/tex/a "quoted" name.tx"#.to_vec(),
                    ]),
                )],
            )
            .expect("a recordable edit");

        let group = group(&scene, "s");

        assert!(
            group.spec.contains(
                r#"param string texturename "/tex/a \"quoted\" name.tx" ;"#
            ),
            "{}",
            group.spec
        );
    }

    /// A parameter the shader does not declare is dropped and named.
    ///
    /// Neither of these is loud in OSL, which is the point. A
    /// parameter the shader lacks draws a warning on OSL's error
    /// handler -- naming the parameter, not the ɴsɪ node that asked
    /// for it, on a handler this backend does not own. A value set on
    /// an **output** draws nothing at all: OSL accepts it, ignores it,
    /// and the shader renders its default.
    ///
    /// Measured against OSL 1.13, not assumed. The third case, a
    /// connection from a non-output, *is* fatal and is checked where
    /// connections are emitted.
    ///
    /// Uses the probe's own compiled shader, so it needs
    /// `tools/osl-probe/build.sh` to have run.
    #[test]
    fn a_parameter_the_shader_does_not_have_is_dropped() {
        let compiled = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tools/osl-probe/build/probe.oso");
        if !compiled.exists() {
            eprintln!(
                "skipped: no {} -- tools/osl-probe/build.sh",
                compiled.display()
            );
            return;
        }

        let mut scene = Scene::default();
        shader(&mut scene, "s", &compiled.to_string_lossy());
        scene
            .set_attribute(
                "s",
                vec![
                    // `probe` declares this one.
                    argument(
                        "Cs",
                        Type::Color,
                        OwnedData::F32(vec![0.1, 0.8, 0.2]),
                    ),
                    // And not this one.
                    argument(
                        "rooughness",
                        Type::F32,
                        OwnedData::F32(vec![0.25]),
                    ),
                    // Nor this: `Cout` is an output, which cannot be
                    // set, and OSL refuses the whole group over it.
                    argument(
                        "Cout",
                        Type::Color,
                        OwnedData::F32(vec![1.0, 0.0, 0.0]),
                    ),
                ],
            )
            .expect("a recordable edit");

        let group = group(&scene, "s");

        // The good one crossed.
        assert!(
            group.spec.contains("param color Cs 0.1 0.8 0.2 ;"),
            "{}",
            group.spec
        );
        // The typo did not, and is named with the shader that refused
        // it.
        assert!(!group.spec.contains("rooughness"), "{}", group.spec);
        assert!(
            group.dropped.iter().any(|line| line.contains("rooughness")
                && line.contains("no such parameter")),
            "{:?}",
            group.dropped
        );
        // And so is the output.
        assert!(!group.spec.contains("param color Cout"), "{}", group.spec);
        assert!(
            group
                .dropped
                .iter()
                .any(|line| line.contains("Cout")
                    && line.contains("an output")),
            "{:?}",
            group.dropped
        );

        // The shader itself still crossed, which is the point: ɴsɪ
        // always returns an image.
        assert_eq!(group.layers, ["s"]);
    }

    /// **The oracle.** What this emits is what OSL parses.
    ///
    /// Every other test here asserts against what this module was
    /// expected to write, which cannot catch a spec that is
    /// well-formed to me and rejected by the parser that has to read
    /// it -- the same trap the `.rdla` oracle exists for. So this
    /// writes a spec covering every type the emitter knows and hands
    /// it to OSL through `tools/osl-probe`, which reports a parse
    /// failure as a non-zero exit.
    ///
    /// Skipped where the probe has not been built, since it needs OSL
    /// and this crate does not.
    #[test]
    fn what_this_emits_is_what_osl_parses() {
        let probe = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tools/osl-probe/build/probe");
        if !probe.exists() {
            eprintln!(
                "skipped: no probe at {} -- tools/osl-probe/build.sh",
                probe.display()
            );
            return;
        }

        let mut scene = Scene::default();
        shader(&mut scene, "tex", "probe");
        shader(&mut scene, "surface", "probe");
        scene
            .set_attribute(
                "surface",
                vec![
                    argument(
                        "Cs",
                        Type::Color,
                        OwnedData::F32(vec![0.1, 0.8, 0.2]),
                    ),
                    argument("power", Type::F32, OwnedData::F32(vec![1.0])),
                    argument(
                        "roughness",
                        Type::F64,
                        OwnedData::F64(vec![0.25]),
                    ),
                ],
            )
            .expect("a recordable edit");
        scene
            .connect("tex", Some("Cout"), "surface", "Cs")
            .expect("a recordable edit");

        let group = group(&scene, "surface");

        let directory = std::env::temp_dir().join("nsi-moonray-osl");
        std::fs::create_dir_all(&directory).expect("a writable directory");
        let spec = directory.join("group.spec");
        std::fs::write(&spec, &group.spec).expect("the spec is written");

        let shaders = probe.parent().expect("the probe's directory");
        let output = std::process::Command::new(&probe)
            .arg(shaders)
            .arg(&spec)
            .output()
            .expect("the probe runs");

        assert!(
            output.status.success(),
            "OSL rejected what this emitted:\n{}\n{}",
            group.spec,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A shader with no `shaderfilename` is reported, not guessed at.
    #[test]
    fn a_shader_with_no_file_is_reported() {
        let mut scene = Scene::default();
        scene.create("s", "shader").expect("a recordable edit");

        let group = group(&scene, "s");

        assert!(group.layers.is_empty(), "{:?}", group.layers);
        assert!(
            group
                .dropped
                .iter()
                .any(|line| line.contains("shaderfilename")),
            "{:?}",
            group.dropped
        );
    }
}

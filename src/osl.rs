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
use std::collections::HashSet;

/// A shader network, ready for `ShaderGroupBegin`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Group {
    /// The specification text.
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
            group.spec.push_str(&format!(
                "connect {}.{from_port} {}.{to_port} ;\n",
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

    for (name, argument) in node.attributes() {
        if name == "shaderfilename" {
            continue;
        }
        match parameter(name, argument.type_tag, &argument.data) {
            Some(line) => group.spec.push_str(&line),
            None => group.dropped.push(format!("{handle}.{name}")),
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

    Some(format!("param {osl_type} {name} {} ;\n", values.join(" ")))
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

//! The transport, end to end: a recorded ɴsɪ scene reaching live rdl2
//! objects with no file in between.
//!
//! Needs the `rdl2` feature and `$SCENE_RDL2_ROOT`. `$NSI_MOONRAY_DSO`
//! points at a directory of scene classes; without it only rdl2's
//! built-in classes resolve, which is enough for what these check.
#![cfg(feature = "rdl2")]

use nsi_moonray::{
    apply::apply,
    document::{Assignment, Body, Document, Object as Described},
    rdl2::Context,
    value::{Reference, Value},
};

fn dso_path() -> Option<String> {
    std::env::var("NSI_MOONRAY_DSO").ok()
}

fn context() -> Context {
    Context::new(dso_path().as_deref()).expect("a scene context")
}

/// The transport works: a document reaches live objects, and rdl2's own
/// writer reads back what was applied.
///
/// Checked against rdl2 rather than against this crate's emitter, which
/// is the point -- the emitter is now one consumer of `Document` and
/// this is the other, so a test that compared them to each other could
/// pass with both wrong.
#[test]
fn a_document_reaches_live_objects() {
    let context = context();

    let mut document = Document::default();
    document.push(
        Described::new("GeometrySet", "/set")
            .set("label", Value::String("unused".to_string())),
    );

    let report = apply(&document, &context);

    // `GeometrySet` has no `label`, so this must come back as a
    // *reported* mapping problem rather than as silence or a panic.
    assert!(
        report.iter().any(|line| line.contains("no such attribute")),
        "an unknown attribute must be reported: {report:?}"
    );

    let out = std::env::temp_dir().join("nsi-moonray-apply.rdla");
    context
        .write_ascii(&out)
        .expect("the live scene writes out");
    let written = std::fs::read_to_string(&out).expect("it was written");

    assert!(
        written.contains("GeometrySet(\"/set\")"),
        "the object reached rdl2\n{written}"
    );
}

/// A set's membership and a `Layer`'s rows are structure, not
/// attributes, and go through their own calls.
#[test]
fn sets_and_layer_rows_apply() {
    let context = context();

    let mut document = Document::default();
    document.push(Described {
        class: "GeometrySet".to_string(),
        name: Some("/set".to_string()),
        body: Body::Set(vec![]),
    });
    document.push(Described {
        class: "Layer".to_string(),
        name: Some("/layer".to_string()),
        body: Body::Layer(vec![]),
    });

    let report = apply(&document, &context);
    assert!(report.is_empty(), "{report:?}");

    let out = std::env::temp_dir().join("nsi-moonray-apply-sets.rdla");
    context.write_ascii(&out).expect("written");
    let written = std::fs::read_to_string(&out).expect("read back");

    assert!(written.contains("GeometrySet(\"/set\")"), "{written}");
    assert!(written.contains("Layer(\"/layer\")"), "{written}");
}

/// A reference to an object the document does not declare must be
/// *reported*, not written as nothing.
///
/// This is the one that matters: a `Layer` row whose material resolves
/// to `undef()` is skipped by MoonRay outright, so the shape vanishes
/// from the image with no error anywhere. Silence here is a black
/// render later.
#[test]
fn a_dangling_reference_is_reported() {
    let context = context();

    let mut document = Document::default();
    document.push(Described {
        class: "GeometrySet".to_string(),
        name: Some("/set".to_string()),
        body: Body::Set(vec![Reference::new("GeometrySet", "/nowhere")]),
    });

    let report = apply(&document, &context);

    assert!(
        report
            .iter()
            .any(|line| line.contains("not in the document")),
        "a dangling member must be reported: {report:?}"
    );
}

/// A `Layer` row naming a geometry the document does not declare is
/// skipped and reported, rather than assigned to nothing.
#[test]
fn a_layer_row_with_no_geometry_is_reported() {
    let context = context();

    let mut document = Document::default();
    document.push(Described {
        class: "Layer".to_string(),
        name: Some("/layer".to_string()),
        body: Body::Layer(vec![Assignment::default()]),
    });

    let report = apply(&document, &context);

    assert!(
        report.iter().any(|line| line.contains("names no geometry")),
        "{report:?}"
    );
}

/// An unknown scene class is the ordinary way a missing DSO shows up,
/// and must name itself rather than failing the whole apply.
#[test]
fn an_unknown_class_is_reported_by_name() {
    let context = context();

    let mut document = Document::default();
    document.push(Described::new("NoSuchClassAnywhere", "/thing"));

    let report = apply(&document, &context);

    assert!(
        report
            .iter()
            .any(|line| line.contains("NoSuchClassAnywhere")),
        "{report:?}"
    );
}

/// **`S5`. The two consumers of `Document`, checked against each
/// other — through rdl2.**
///
/// `apply` and `to_rdla` are two paths out of one structure, and
/// nothing else compares them. A setter that took a different route
/// from the writer — a wrong attribute name, a transposed matrix, a
/// vector element order — would leave both self-consistent and only
/// one correct.
///
/// The comparison is not text against text: rdl2's writer emits every
/// declared attribute of every class, defaults included, while this
/// crate emits only what it set. So the check is that **every
/// attribute this crate writes appears with the same value** in what
/// rdl2 writes after being driven through the shim.
///
/// `SceneVariables` is the subject because it is built into rdl2 —
/// needing no DSO — and declares an attribute of most types.
#[test]
fn applying_a_document_and_dumping_it_agrees_with_the_emitter() {
    use nsi_moonray::{document::Body, value::Value};

    let context = context();

    let mut variables = nsi_moonray::document::Object::scene_variables();
    for (name, value) in [
        ("image_width", Value::Int(640)),
        ("image_height", Value::Int(480)),
        ("output_file", Value::String("beauty.exr".to_string())),
        ("pixel_samples", Value::Int(3)),
        // A float `%g` renders unobviously, which is the case that
        // would catch a formatter disagreeing with rdl2's writer.
        ("scene_scale", Value::Float(0.1)),
        ("fatal_color", Value::Rgb([0.25, 0.5, 0.75])),
        ("sample_clamping_value", Value::Float(12.5)),
    ] {
        variables = variables.set(name, value);
    }

    let mut document = Document::default();
    document.push(variables);

    let report = apply(&document, &context);
    assert!(
        report.is_empty(),
        "every attribute must apply; an unreported failure would make \
         this test vacuous: {report:?}"
    );

    let out = std::env::temp_dir().join("nsi-moonray-roundtrip.rdla");
    context
        .write_ascii(&out)
        .expect("the live scene writes out");
    let written = std::fs::read_to_string(&out).expect("read back");

    // Every attribute the emitter wrote must appear, verbatim, in what
    // rdl2 wrote. `Value`'s formatting is oracle-checked against rdl2's
    // own writer, so agreeing here means the *setter* agrees too.
    for object in &document.objects {
        if let Body::Attributes(attributes) = &object.body {
            for (name, value) in attributes {
                let expected = format!("[\"{name}\"] = {value},");
                assert!(
                    written.contains(&expected),
                    "the applied scene disagrees with the emitter at \
                     {name:?}: expected {expected}\n{written}"
                );
            }
        }
    }
}

/// **`T0.6`.** A real mesh scene applied and read back through rdl2,
/// with **no MoonRay** — only the authoring twins in `tools/twin`.
///
/// Everything else that checks a mesh either reads the emitted text
/// (which proves the emitter agrees with itself) or renders it (which
/// needs a fifty-minute build and five packaging workarounds). This is
/// the middle: rdl2 itself resolving `RdlMeshGeometry`, accepting every
/// attribute the flush sets, and writing them back.
///
/// The twins are built from **MoonRay's own `attributes.cc`**, compiled
/// out of a source checkout rather than copied, so they cannot drift
/// from what the renderer declares. A copy would go stale in exactly
/// the way that renders plausibly and wrongly.
///
/// Skipped where `$NSI_MOONRAY_DSO` names no directory holding them.
#[test]
fn a_mesh_scene_applies_through_the_authoring_twins() {
    use nsi_intermediate::{OwnedArg, Scene};
    use nsi_moonray::flush::flush;
    use nsi_trait::Type;

    let Some(dso) = dso_path() else {
        eprintln!("skipped: $NSI_MOONRAY_DSO names no scene classes");
        return;
    };
    if !std::path::Path::new(&dso)
        .join("RdlMeshGeometry.so")
        .exists()
    {
        eprintln!("skipped: no RdlMeshGeometry.so in {dso}");
        return;
    }

    fn arg(
        name: &str,
        type_tag: Type,
        data: nsi_intermediate::OwnedData,
    ) -> OwnedArg {
        OwnedArg::new(name, type_tag, 1, 0, data)
    }
    use nsi_intermediate::OwnedData;

    let mut scene = Scene::default();
    scene.create("quad", "mesh").unwrap();
    scene
        .set_attribute(
            "quad",
            vec![
                arg("nvertices", Type::I32, OwnedData::I32(vec![4])),
                arg("P.indices", Type::I32, OwnedData::I32(vec![0, 1, 2, 3])),
                arg(
                    "P",
                    Type::Point,
                    OwnedData::F32(vec![
                        -1.0, -1.0, 0.0, 1.0, -1.0, 0.0, 1.0, 1.0, 0.0, -1.0,
                        1.0, 0.0,
                    ]),
                ),
                arg(
                    "subdivision.scheme",
                    Type::String,
                    OwnedData::String(vec![b"catmull-clark".to_vec()]),
                ),
            ],
        )
        .unwrap();
    scene.connect("quad", None, ".root", "objects").unwrap();

    let context = context();
    let flushed = flush(&scene);
    let report = apply(&flushed.document, &context);

    // The point of the twins: every attribute the flush sets is one
    // rdl2 knows, because the declarations are MoonRay's own.
    //
    // `UsdPreviewSurface` is the one class with no twin, and the
    // report naming it is correct rather than a gap in this test: its
    // `attributes.cc` is *generated* from an `.ispc` by MoonRay's own
    // build, so a twin for it would need the thing these exist to
    // avoid needing. Declaring its parameters by hand instead is the
    // copy that drifts, and would be worse than the gap.
    let unexpected: Vec<&String> = report
        .iter()
        .filter(|line| !line.contains("UsdPreviewSurface"))
        .collect();
    assert!(
        unexpected.is_empty(),
        "rdl2 refused something the flush wrote, which means the \
         emitter and MoonRay's declarations disagree: {unexpected:?}"
    );

    let out = std::env::temp_dir().join("nsi-moonray-twin.rdla");
    context
        .write_ascii(&out)
        .expect("the live scene writes out");
    let written = std::fs::read_to_string(&out).expect("read back");

    assert!(
        written.contains("RdlMeshGeometry(\"quad\")"),
        "the mesh should have reached rdl2\n{written}"
    );
    // Values, not just presence: a class that loaded but ignored its
    // attributes would pass the line above.
    assert!(
        written.contains("[\"face_vertex_count\"] = { 4}"),
        "{written}"
    );
    assert!(
        written.contains("[\"vertices_by_index\"] = { 0, 1, 2, 3}"),
        "{written}"
    );
    assert!(written.contains("[\"is_subd\"] = true"), "{written}");

    // **An enumerable `Int` reads back as its enum name.** The flush
    // writes `subd_scheme` as `1`, rdl2 accepts it, and its writer
    // emits `"catclark"`. So a text diff between what this crate
    // writes and what rdl2 writes will differ on every enumerable
    // attribute even when the value is identical — which is worth
    // knowing before treating such a diff as a defect.
    assert!(
        written.contains("[\"subd_scheme\"] = \"catclark\""),
        "the integer this crate wrote should read back as the enum \
         name it means\n{written}"
    );
    assert!(
        written.contains(
            "[\"vertex_list_0\"] = { Vec3(-1, -1, 0), Vec3(1, -1, 0), Vec3(1, 1, 0), Vec3(-1, 1, 0)}"
        ),
        "the vertices should have crossed intact\n{written}"
    );
}

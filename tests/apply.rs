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

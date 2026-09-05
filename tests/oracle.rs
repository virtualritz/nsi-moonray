//! The emitter against the format oracle.
//!
//! Each test rebuilds, by hand, the scene `tools/oracle` built through
//! the real `scene_rdl2`, and asserts this crate writes byte-for-byte
//! what rdl2's own `AsciiWriter` wrote. A guessed emitter passes none
//! of these.
//!
//! The fixtures are read at run time rather than with `include_str!`,
//! so that a packaged crate — which ships `src/` only — still builds.

use nsi_moonray::{Assignment, Body, Document, Object, Reference, Value};
use std::{fs, path::PathBuf};

fn oracle(name: &str) -> String {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "specs",
        "001-moonray-backend",
        "oracle",
        &format!("{name}.rdla"),
    ]
    .iter()
    .collect();

    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// Every `AttributeType`, and how each prints.
#[test]
fn types() {
    let other = Reference::new("ExtensiveObject", "/oracle/other");

    let mut document = Document::default();
    document.push(Object::scene_variables());
    document.push(Object::new("ExtensiveObject", "/oracle/other"));
    document.push(
        Object::new("ExtensiveObject", "/oracle/types")
            .set("bool", Value::Bool(false))
            .set("int", Value::Int(7))
            .set("long", Value::Long(8))
            .set("float", Value::Float(1.5))
            .set("double", Value::Double(2.5))
            .set("string", Value::String("a string".into()))
            .set("rgb", Value::Rgb([0.25, 0.5, 0.75]))
            .set("rgba", Value::Rgba([0.25, 0.5, 0.75, 1.0]))
            .set("vec2f", Value::Vec2f([9.0, 8.0]))
            .set("vec2d", Value::Vec2d([1.0, 2.0]))
            .set("vec3f", Value::Vec3f([9.0, 8.0, 7.0]))
            .set("vec3d", Value::Vec3d([1.0, 2.0, 3.0]))
            .set("vec4f", Value::Vec4f([9.0, 8.0, 7.0, 6.0]))
            .set("vec4d", Value::Vec4d([1.0, 2.0, 3.0, 4.0]))
            .set(
                "mat4f",
                Value::Mat4f([
                    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                    4.0, 5.0, 6.0, 1.0,
                ]),
            )
            .set(
                "mat4d",
                Value::Mat4d([
                    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                    4.0, 5.0, 6.0, 1.0,
                ]),
            )
            .set("scene_object", Value::Object(other.clone()))
            .set(
                "bool_vector",
                Value::Vector(vec![
                    Value::Bool(false),
                    Value::Bool(true),
                    Value::Bool(false),
                ]),
            )
            .set(
                "int_vector",
                Value::Vector(vec![
                    Value::Int(1),
                    Value::Int(2),
                    Value::Int(3),
                ]),
            )
            .set(
                "long_vector",
                Value::Vector(vec![Value::Long(4), Value::Long(5)]),
            )
            .set(
                "float_vector",
                Value::Vector(vec![
                    Value::Float(0.1),
                    Value::Float(1e20),
                    Value::Float(1e-7),
                    Value::Float(-0.0),
                    Value::Float(1234567.0),
                ]),
            )
            .set(
                "double_vector",
                Value::Vector(vec![
                    Value::Double(0.1),
                    Value::Double(1e300),
                    Value::Double(1e-7),
                    Value::Double(-2.5),
                    Value::Double(1234567890123.0),
                ]),
            )
            .set(
                "string_vector",
                Value::Vector(vec![
                    Value::String("one".into()),
                    Value::String("two".into()),
                ]),
            )
            .set(
                "rgb_vector",
                Value::Vector(vec![
                    Value::Rgb([1.0, 0.0, 0.0]),
                    Value::Rgb([0.0, 1.0, 0.0]),
                ]),
            )
            .set(
                "rgba_vector",
                Value::Vector(vec![Value::Rgba([1.0, 0.0, 0.0, 1.0])]),
            )
            .set(
                "vec2f_vector",
                Value::Vector(vec![Value::Vec2f([1.0, 2.0])]),
            )
            .set(
                "vec2d_vector",
                Value::Vector(vec![Value::Vec2d([9.0, 8.0])]),
            )
            .set(
                "vec3f_vector",
                Value::Vector(vec![Value::Vec3f([9.0, 8.0, 7.0])]),
            )
            .set(
                "vec3d_vector",
                Value::Vector(vec![Value::Vec3d([9.0, 8.0, 7.0])]),
            )
            .set(
                "vec4f_vector",
                Value::Vector(vec![Value::Vec4f([1.0, 2.0, 3.0, 4.0])]),
            )
            .set(
                "vec4d_vector",
                Value::Vector(vec![Value::Vec4d([9.0, 8.0, 7.0, 6.0])]),
            )
            .set(
                "mat4f_vector",
                Value::Vector(vec![Value::Mat4f([
                    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                    0.0, 0.0, 0.0, 1.0,
                ])]),
            )
            .set(
                "mat4d_vector",
                Value::Vector(vec![Value::Mat4d([
                    2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0,
                    0.0, 0.0, 0.0, 1.0,
                ])]),
            )
            .set(
                "scene_object_vector",
                Value::Vector(vec![Value::Object(other)]),
            ),
    );

    assert_eq!(document.to_rdla(), oracle("types"));
}

/// A scene shaped like the one this backend has to emit.
#[test]
fn scene() {
    let geometry = Reference::new("FakeTeapot", "/oracle/geometry");
    let material = Reference::new("FakeMaterial", "/oracle/material");
    let light = Reference::new("FakeLight", "/oracle/light");
    let lights = Reference::new("LightSet", "/oracle/lights");
    let layer = Reference::new("Layer", "/oracle/layer");

    let mut document = Document::default();
    document.push(
        Object::scene_variables()
            .set("layer", Value::Object(layer.clone()))
            .set("image_width", Value::Int(320))
            .set("image_height", Value::Int(240)),
    );
    document.push(Object::new("FakeTeapot", "/oracle/geometry").set(
        "node_xform",
        Value::Mat4d([
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0,
            2.0, 3.0, 1.0,
        ]),
    ));
    document.push(Object {
        class: "GeometrySet".into(),
        name: Some("/oracle/geometries".into()),
        body: Body::Set(vec![geometry.clone()]),
    });
    document.push(Object {
        class: "Layer".into(),
        name: Some("/oracle/layer".into()),
        body: Body::Layer(vec![Assignment::new(
            geometry,
            Some(material),
            Some(lights.clone()),
        )]),
    });
    document.push(Object::new("FakeMaterial", "/oracle/material"));
    document.push(Object::new("FakeLight", "/oracle/light"));
    document.push(Object {
        class: "LightSet".into(),
        name: Some("/oracle/lights".into()),
        body: Body::Set(vec![light]),
    });
    document.push(
        Object::new("RenderOutput", "/oracle/beauty")
            .set("file_name", Value::String("beauty.exr".into())),
    );

    assert_eq!(document.to_rdla(), oracle("scene"));
}

/// Motion blur: two samples, and no more than two.
#[test]
fn blur() {
    let mut document = Document::default();
    document.push(Object::scene_variables());
    document.push(Object::new("FakeTeapot", "/oracle/moving").set(
        "node_xform",
        Value::Blur(
            Box::new(Value::Mat4d([
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                0.0, 0.0, 0.0, 1.0,
            ])),
            Box::new(Value::Mat4d([
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0,
                5.0, 0.0, 0.0, 1.0,
            ])),
        ),
    ));
    document.push(
        Object::new("ExtensiveObject", "/oracle/blurred")
            .set(
                "float",
                Value::Blur(
                    Box::new(Value::Float(0.0)),
                    Box::new(Value::Float(1.0)),
                ),
            )
            .set(
                "vec3f",
                Value::Blur(
                    Box::new(Value::Vec3f([0.0, 0.0, 0.0])),
                    Box::new(Value::Vec3f([0.0, 1.0, 0.0])),
                ),
            ),
    );

    assert_eq!(document.to_rdla(), oracle("blur"));
}

/// A bound attribute, where ɴsɪ's named shader ports land.
#[test]
fn binding() {
    let mut document = Document::default();
    document.push(Object::scene_variables());
    document.push(Object::new("FakeMaterial", "/oracle/source"));
    document.push(Object::new("ExtensiveObject", "/oracle/bound").set(
        "string",
        Value::Bind(
            Reference::new("FakeMaterial", "/oracle/source"),
            Box::new(Value::String("pizza".into())),
        ),
    ));

    assert_eq!(document.to_rdla(), oracle("binding"));
}

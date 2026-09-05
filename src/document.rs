//! The shape of an `.rdla` file: a list of scene objects, each of which
//! is a block of attributes, a set's membership, or a `Layer`'s
//! assignment table.
//!
//! Layout follows the oracle exactly — four-space indent, a trailing
//! comma on every entry, one blank line between objects and none after
//! the last — so that what this crate writes can be diffed against what
//! rdl2's own `AsciiWriter` writes for the same scene.

use crate::value::{Reference, Value};
use std::io::{self, Write};

/// rdl2's indent, which is four spaces.
const INDENT: &str = "    ";

/// One row of a `Layer`'s assignment table.
///
/// Nine columns, in the order `AsciiWriter::writeLayer` writes them.
/// Everything past the light set is optional and prints as `undef()`;
/// an ɴsɪ `attributes` node dissolves into the first four.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Assignment {
    pub geometry: Option<Reference>,
    /// The part name. Empty means the whole geometry, which is what an
    /// ɴsɪ scene without face groups produces.
    pub part: String,
    pub material: Option<Reference>,
    pub light_set: Option<Reference>,
    pub displacement: Option<Reference>,
    pub volume_shader: Option<Reference>,
    pub light_filter_set: Option<Reference>,
    pub shadow_set: Option<Reference>,
    pub shadow_receiver_set: Option<Reference>,
}

impl Assignment {
    /// The common case: a whole geometry, a material, and a light set.
    pub fn new(
        geometry: Reference,
        material: Option<Reference>,
        light_set: Option<Reference>,
    ) -> Self {
        Self {
            geometry: Some(geometry),
            material,
            light_set,
            ..Default::default()
        }
    }

    fn write(&self, out: &mut impl Write) -> io::Result<()> {
        let columns = [
            &self.geometry,
            &self.material,
            &self.light_set,
            &self.displacement,
            &self.volume_shader,
            &self.light_filter_set,
            &self.shadow_set,
            &self.shadow_receiver_set,
        ];

        write!(out, "{INDENT}{{")?;
        // The part name sits between the geometry and the material, so
        // the first column is written before the loop.
        write!(out, "{}, \"{}\"", reference(columns[0]), self.part)?;
        for column in &columns[1..] {
            write!(out, ", {}", reference(column))?;
        }
        writeln!(out, "}},")
    }
}

fn reference(reference: &Option<Reference>) -> String {
    match reference {
        Some(reference) => reference.to_string(),
        None => "undef()".to_string(),
    }
}

/// What sits between an object's braces.
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// `["name"] = value,` per attribute, in the order given. rdl2
    /// writes attributes in declaration order, so a backend that wants
    /// a clean diff has to feed them in that order.
    Attributes(Vec<(String, Value)>),
    /// A `GeometrySet`, `LightSet` or any other set: bare references,
    /// one per line.
    Set(Vec<Reference>),
    /// A `Layer`'s assignment table.
    Layer(Vec<Assignment>),
}

/// One `SceneObject` in the file.
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub class: String,
    /// `None` for `SceneVariables`, which is a singleton and is written
    /// without a name or parentheses.
    pub name: Option<String>,
    pub body: Body,
}

impl Object {
    /// An object with attributes.
    pub fn new(class: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            class: class.into(),
            name: Some(name.into()),
            body: Body::Attributes(Vec::new()),
        }
    }

    /// The scene variables, which have no name.
    pub fn scene_variables() -> Self {
        Self {
            class: "SceneVariables".to_string(),
            name: None,
            body: Body::Attributes(Vec::new()),
        }
    }

    /// Append an attribute. Order is preserved.
    pub fn set(mut self, name: impl Into<String>, value: Value) -> Self {
        match &mut self.body {
            Body::Attributes(attributes) => {
                attributes.push((name.into(), value))
            }
            _ => panic!("`set` on an object whose body is not attributes"),
        }
        self
    }

    /// A reference to this object, for use as another's attribute.
    pub fn reference(&self) -> Reference {
        Reference::new(
            self.class.clone(),
            self.name.clone().unwrap_or_default(),
        )
    }

    fn write(&self, out: &mut impl Write) -> io::Result<()> {
        match &self.name {
            Some(name) => writeln!(out, "{}(\"{}\") {{", self.class, name)?,
            None => writeln!(out, "{} {{", self.class)?,
        }

        match &self.body {
            Body::Attributes(attributes) => {
                for (name, value) in attributes {
                    writeln!(out, "{INDENT}[\"{name}\"] = {value},")?;
                }
            }
            Body::Set(members) => {
                for member in members {
                    writeln!(out, "{INDENT}{member},")?;
                }
            }
            Body::Layer(assignments) => {
                for assignment in assignments {
                    assignment.write(out)?;
                }
            }
        }

        writeln!(out, "}}")
    }
}

/// A whole `.rdla` file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    pub objects: Vec<Object>,
}

impl Document {
    pub fn push(&mut self, object: Object) -> Reference {
        let reference = object.reference();
        self.objects.push(object);
        reference
    }

    /// Write the file. Objects are separated by a blank line, with none
    /// after the last.
    pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
        for (index, object) in self.objects.iter().enumerate() {
            if index > 0 {
                writeln!(out)?;
            }
            object.write(out)?;
        }
        Ok(())
    }

    /// The file as a string.
    pub fn to_rdla(&self) -> String {
        let mut buffer = Vec::new();
        self.write(&mut buffer)
            .expect("writing to a `Vec` cannot fail");
        String::from_utf8(buffer).expect("`.rdla` is written as UTF-8")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_variables_are_written_without_a_name() {
        let mut document = Document::default();
        document.push(Object::scene_variables());
        assert_eq!(document.to_rdla(), "SceneVariables {\n}\n");
    }

    #[test]
    fn objects_are_separated_by_one_blank_line() {
        let mut document = Document::default();
        document.push(Object::scene_variables());
        document.push(Object::new("FakeMaterial", "/m"));
        assert_eq!(
            document.to_rdla(),
            "SceneVariables {\n}\n\nFakeMaterial(\"/m\") {\n}\n"
        );
    }

    #[test]
    fn a_set_writes_bare_references() {
        let mut document = Document::default();
        document.push(Object {
            class: "GeometrySet".to_string(),
            name: Some("/gs".to_string()),
            body: Body::Set(vec![Reference::new("RdlMeshGeometry", "/mesh")]),
        });
        assert_eq!(
            document.to_rdla(),
            "GeometrySet(\"/gs\") {\n    RdlMeshGeometry(\"/mesh\"),\n}\n"
        );
    }

    /// Nine columns, and the unassigned ones are `undef()` rather than
    /// being left out.
    #[test]
    fn a_layer_row_has_nine_columns() {
        let mut document = Document::default();
        document.push(Object {
            class: "Layer".to_string(),
            name: Some("/layer".to_string()),
            body: Body::Layer(vec![Assignment::new(
                Reference::new("RdlMeshGeometry", "/mesh"),
                Some(Reference::new("DwaBaseMaterial", "/material")),
                None,
            )]),
        });
        assert_eq!(
            document.to_rdla(),
            "Layer(\"/layer\") {\n    {RdlMeshGeometry(\"/mesh\"), \"\", \
             DwaBaseMaterial(\"/material\"), undef(), undef(), undef(), \
             undef(), undef(), undef()},\n}\n"
        );
    }
}

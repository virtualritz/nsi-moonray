//! `.rdla` values, and how rdl2 prints them.
//!
//! Every rule here was read out of a scene written by rdl2's own
//! `AsciiWriter` — see `specs/001-moonray-backend/oracle/`. None of it
//! is inferred from the format's Lua-ness, because a plausible guess
//! and the real format differ in at least four places: `Vec2`/`Vec3`
//! carry no precision suffix even when the attribute is double, a null
//! object reference is `undef()` rather than `nil`, a bound attribute
//! keeps its own value alongside the binding, and numbers print through
//! C++'s `%g` with `max_digits10`, which is neither Rust's `{}` nor its
//! `{:?}`.

use std::fmt;

/// Significant digits rdl2 prints a `Float` with:
/// `std::numeric_limits<float>::max_digits10`.
const FLOAT_DIGITS: usize = 9;

/// Significant digits rdl2 prints a `Double` with:
/// `std::numeric_limits<double>::max_digits10`.
const DOUBLE_DIGITS: usize = 17;

/// A reference to another `SceneObject`, which `.rdla` spells as the
/// class name applied to the object's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub class: String,
    pub name: String,
}

impl Reference {
    pub fn new(class: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            class: class.into(),
            name: name.into(),
        }
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The name is quoted, and rdl2 does not escape it -- a scene
        // object name carrying a quote would produce a broken file
        // there too.
        write!(f, "{}(\"{}\")", self.class, self.name)
    }
}

/// One attribute value.
///
/// The variants mirror rdl2's `AttributeType`, with `Vector` standing
/// in for all of its `*Vector` types: rdl2 prints a vector the same way
/// whatever its element type is.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    Rgb([f32; 3]),
    Rgba([f32; 4]),
    Vec2f([f32; 2]),
    Vec2d([f64; 2]),
    Vec3f([f32; 3]),
    Vec3d([f64; 3]),
    Vec4f([f32; 4]),
    Vec4d([f64; 4]),
    /// Row-major, as ɴsɪ's `transformationmatrix` is and as rdl2 prints
    /// it.
    Mat4f([f32; 16]),
    Mat4d([f64; 16]),
    /// A `SceneObject*` attribute pointing at another object.
    Object(Reference),
    /// A `SceneObject*` attribute pointing at nothing.
    Undef,
    Vector(Vec<Value>),
    /// Two motion samples. rdl2 has exactly two timesteps,
    /// `TIMESTEP_BEGIN` and `TIMESTEP_END`, so a scene carrying more
    /// than two ɴsɪ motion samples cannot be represented and the
    /// backend must say so rather than quietly dropping the rest.
    Blur(Box<Value>, Box<Value>),
    /// An attribute bound to another object — where ɴsɪ's named shader
    /// ports land. The bound-to value is still written.
    Bind(Reference, Box<Value>),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(value) => write!(f, "{value}"),
            Self::Int(value) => write!(f, "{value}"),
            Self::Long(value) => write!(f, "{value}"),
            Self::Float(value) => f.write_str(&float(*value)),
            Self::Double(value) => f.write_str(&double(*value)),
            Self::String(value) => write!(f, "\"{value}\""),
            Self::Rgb([r, g, b]) => {
                write!(f, "Rgb({}, {}, {})", float(*r), float(*g), float(*b))
            }
            Self::Rgba([r, g, b, a]) => write!(
                f,
                "Rgba({}, {}, {}, {})",
                float(*r),
                float(*g),
                float(*b),
                float(*a)
            ),
            // No precision suffix on the constructor: rdl2 writes
            // `Vec2(...)` for both `Vec2f` and `Vec2d`.
            Self::Vec2f(values) => tuple(f, "Vec2", values, float),
            Self::Vec2d(values) => tuple(f, "Vec2", values, double),
            Self::Vec3f(values) => tuple(f, "Vec3", values, float),
            Self::Vec3d(values) => tuple(f, "Vec3", values, double),
            Self::Vec4f(values) => tuple(f, "Vec4", values, float),
            Self::Vec4d(values) => tuple(f, "Vec4", values, double),
            Self::Mat4f(values) => tuple(f, "Mat4", values, float),
            Self::Mat4d(values) => tuple(f, "Mat4", values, double),
            Self::Object(reference) => write!(f, "{reference}"),
            Self::Undef => f.write_str("undef()"),
            Self::Vector(values) => {
                // rdl2 writes a space after the opening brace and none
                // before the closing one: `{ 1, 2, 3}`.
                f.write_str("{")?;
                for (index, value) in values.iter().enumerate() {
                    if index == 0 {
                        write!(f, " {value}")?;
                    } else {
                        write!(f, ", {value}")?;
                    }
                }
                f.write_str("}")
            }
            Self::Blur(begin, end) => write!(f, "blur({begin}, {end})"),
            Self::Bind(source, value) => write!(f, "bind({source}, {value})"),
        }
    }
}

/// Write `Name(a, b, ...)` for a fixed-size tuple type.
fn tuple<T: Copy>(
    f: &mut fmt::Formatter<'_>,
    name: &str,
    values: &[T],
    format: fn(T) -> String,
) -> fmt::Result {
    f.write_str(name)?;
    f.write_str("(")?;
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            f.write_str(", ")?;
        }
        f.write_str(&format(*value))?;
    }
    f.write_str(")")
}

/// Print an `f32` the way rdl2 does.
pub fn float(value: f32) -> String {
    general(value as f64, FLOAT_DIGITS)
}

/// Print an `f64` the way rdl2 does.
pub fn double(value: f64) -> String {
    general(value, DOUBLE_DIGITS)
}

/// C++'s default `ostream` float format, which is `printf`'s `%g` at
/// the given precision.
///
/// Reimplemented rather than approximated because the oracle pins the
/// output exactly: `0.1f` is `0.100000001`, `1e20f` is
/// `1.00000002e+20`, `-0.0f` is `-0`, and `1234567.0f` is `1234567`.
/// Rust's own `{}` prints the shortest round-tripping form and agrees
/// with none of those.
fn general(value: f64, precision: usize) -> String {
    if value.is_nan() {
        // What C++ prints. A scene carrying one is broken input, and
        // the resulting file will not load -- the backend reports that
        // rather than silently substituting a number.
        return if value.is_sign_negative() {
            "-nan".to_string()
        } else {
            "nan".to_string()
        };
    }

    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-inf".to_string()
        } else {
            "inf".to_string()
        };
    }

    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0".to_string()
        } else {
            "0".to_string()
        };
    }

    // Round first, then decide the notation from the *rounded*
    // exponent: 9.9999e-1 at three digits is 1e0, and choosing the
    // notation before rounding would put it in the wrong branch.
    let scientific = format!("{:.*e}", precision - 1, value);
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("Rust's `e` format always emits an exponent");
    let exponent: i32 = exponent.parse().expect("a decimal exponent");

    if exponent < -4 || exponent >= precision as i32 {
        let mantissa = trim(mantissa);
        // C++ pads the exponent to two digits and always signs it.
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!("{mantissa}e{sign}{:02}", exponent.abs());
    }

    let decimals = (precision as i32 - 1 - exponent).max(0) as usize;
    trim(&format!("{value:.decimals$}")).to_string()
}

/// Drop a fractional part's trailing zeros, and the point with them.
fn trim(text: &str) -> &str {
    if !text.contains('.') {
        return text;
    }

    text.trim_end_matches('0').trim_end_matches('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every one of these is a value read back out of
    /// `oracle/types.rdla`, which rdl2's own writer produced.
    #[test]
    fn floats_print_the_way_rdl2_prints_them() {
        assert_eq!(float(0.1), "0.100000001");
        assert_eq!(float(1e20), "1.00000002e+20");
        assert_eq!(float(1e-7), "1.00000001e-07");
        assert_eq!(float(-0.0), "-0");
        assert_eq!(float(1234567.0), "1234567");
        assert_eq!(float(1.5), "1.5");
        assert_eq!(float(0.25), "0.25");
        assert_eq!(float(1.0), "1");
    }

    #[test]
    fn doubles_print_the_way_rdl2_prints_them() {
        assert_eq!(double(0.1), "0.10000000000000001");
        assert_eq!(double(1e300), "1.0000000000000001e+300");
        assert_eq!(double(1e-7), "9.9999999999999995e-08");
        assert_eq!(double(-2.5), "-2.5");
        assert_eq!(double(1234567890123.0), "1234567890123");
    }

    #[test]
    fn tuples_carry_no_precision_suffix() {
        assert_eq!(Value::Vec2d([1.0, 2.0]).to_string(), "Vec2(1, 2)");
        assert_eq!(Value::Vec3f([9.0, 8.0, 7.0]).to_string(), "Vec3(9, 8, 7)");
        assert_eq!(
            Value::Rgba([0.25, 0.5, 0.75, 1.0]).to_string(),
            "Rgba(0.25, 0.5, 0.75, 1)"
        );
    }

    #[test]
    fn a_vector_has_a_leading_space_and_no_trailing_one() {
        let value =
            Value::Vector(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert_eq!(value.to_string(), "{ 1, 2, 3}");
    }

    #[test]
    fn an_empty_vector_is_two_braces() {
        assert_eq!(Value::Vector(vec![]).to_string(), "{}");
    }

    #[test]
    fn a_null_object_reference_is_undef() {
        assert_eq!(Value::Undef.to_string(), "undef()");
    }

    #[test]
    fn blur_takes_exactly_two_samples() {
        let value = Value::Blur(
            Box::new(Value::Float(0.0)),
            Box::new(Value::Float(1.0)),
        );
        assert_eq!(value.to_string(), "blur(0, 1)");
    }

    /// A binding keeps the attribute's own value, which is not obvious
    /// and would have been left out of a guessed emitter.
    #[test]
    fn a_binding_keeps_the_bound_attributes_value() {
        let value = Value::Bind(
            Reference::new("FakeMaterial", "/oracle/source"),
            Box::new(Value::String("pizza".to_string())),
        );
        assert_eq!(
            value.to_string(),
            "bind(FakeMaterial(\"/oracle/source\"), \"pizza\")"
        );
    }
}

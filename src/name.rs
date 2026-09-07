//! A short, repeated string: a class name, an ɴsɪ handle, an attribute
//! name.
//!
//! # Why this is not a `String`
//!
//! A flushed [`Document`](crate::Document) is *mostly* these. Every
//! object carries its class and its name; every [`Reference`] carries
//! both again, and a `Layer` row holds up to nine references; every
//! attribute carries its name. So a shape with a material and a light
//! set spells its own handle five or six times over.
//!
//! Measured, on a 100 001-node scene: the document cost 109 MB against
//! the scene's 103 MB -- *more than the scene it came from* -- and it
//! did not shrink at all when upstream stopped duplicating handles.
//! It is resident for the life of an interactive session too, since
//! `apply_affected` diffs the previous document against the new one
//! between frames. `research.md` F14; `tools/footprint` is the
//! measurement.
//!
//! # What it is
//!
//! With `interned_handles` -- on by default -- a [`Ustr`], eight
//! bytes, pointing into the same global table upstream already
//! populated with every one of these handles. Looking one up is free
//! and storing it costs nothing beyond the pointer.
//!
//! Without it, a `Box<str>`: sixteen bytes and an exact-size
//! allocation rather than a `String`'s twenty-four and its spare
//! capacity. Not free, but not a global table either.
//!
//! The API is the same in both, so nothing downstream has to know
//! which it got. It derefs to `str`, compares against `&str`, and
//! prints and debug-prints exactly as a `String` does -- that last one
//! is load-bearing, because handles reach users through `{handle:?}`
//! in limitation messages.
//!
//! # What is *not* a `Name`
//!
//! [`Value::String`](crate::Value::String): file paths, channel names,
//! anything an ɴsɪ scene set as a string attribute. Those are neither
//! short nor repeated, and interning them would put unbounded,
//! never-freed strings in a global table -- which is a leak rather
//! than a saving.
//!
//! [`Reference`]: crate::Reference
//! [`Ustr`]: https://docs.rs/ustr/

use std::{
    borrow::Borrow,
    fmt,
    hash::{Hash, Hasher},
    ops::Deref,
};

#[cfg(feature = "interned_handles")]
type Storage = ustr::Ustr;

#[cfg(not(feature = "interned_handles"))]
type Storage = Box<str>;

/// A class name, an ɴsɪ handle, or an attribute name.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Name(Storage);

/// Hashes as its **text**, not as its storage.
///
/// `Borrow<str>` is a promise that a `Name` and the `str` it borrows
/// as hash alike, and `Ustr`'s own `Hash` is a precomputed hash of the
/// pointer -- faster, and not that. Deriving it made
/// `HashMap<Name, _>::get("quad")` return `None` for a key that was
/// there, silently, which the test below caught and a caller would
/// not have.
impl Hash for Name {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl Name {
    pub fn new(text: &str) -> Self {
        #[cfg(feature = "interned_handles")]
        return Self(ustr::Ustr::from(text));

        #[cfg(not(feature = "interned_handles"))]
        return Self(text.into());
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for Name {
    fn default() -> Self {
        Self::new("")
    }
}

impl Deref for Name {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Name {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for Name {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Debug-prints as a quoted string, exactly as `String` does.
///
/// Not cosmetic: handles reach users through `{handle:?}` in the
/// limitation messages this backend reports, and a wrapper's derived
/// `Debug` would put `Name("…")` in every one of them.
impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl From<&str> for Name {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl From<&String> for Name {
    fn from(text: &String) -> Self {
        Self::new(text)
    }
}

impl From<String> for Name {
    fn from(text: String) -> Self {
        Self::new(&text)
    }
}

impl From<&Name> for Name {
    fn from(name: &Name) -> Self {
        name.clone()
    }
}

impl From<Name> for String {
    fn from(name: Name) -> Self {
        name.as_str().to_owned()
    }
}

impl PartialEq<str> for Name {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for Name {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<Name> for str {
    fn eq(&self, other: &Name) -> bool {
        self == other.as_str()
    }
}

impl PartialEq<Name> for &str {
    fn eq(&self, other: &Name) -> bool {
        *self == other.as_str()
    }
}

impl PartialEq<String> for Name {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// It prints as its text, and debug-prints quoted -- which is what
    /// every limitation message this backend reports depends on.
    #[test]
    fn a_name_prints_the_way_a_string_does() {
        let name = Name::new("/set/wall");

        assert_eq!(name.to_string(), "/set/wall");
        assert_eq!(format!("{name:?}"), "\"/set/wall\"");
        assert_eq!(
            format!("{:?}", "/set/wall".to_string()),
            format!("{name:?}")
        );
    }

    /// It compares against `&str` both ways round, so call sites read
    /// as they did when this was a `String`.
    #[test]
    fn a_name_compares_against_a_str() {
        let name = Name::new("mesh");

        assert!(name == "mesh");
        assert!("mesh" == name);
        assert_ne!(name, Name::new("transform"));
    }

    /// It borrows as `str`, which is what lets a `HashMap<Name, _>` be
    /// probed with one.
    #[test]
    fn a_name_keys_a_map_probed_by_str() {
        let mut map = std::collections::HashMap::new();
        map.insert(Name::new("quad"), 1);

        assert_eq!(map.get("quad"), Some(&1));
    }
}

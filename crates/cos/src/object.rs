//! The [`Object`] enum and its compound types ([`Dict`], [`Stream`],
//! [`Reference`]).

use crate::name::Name;
use crate::string::PdfString;

/// An indirect reference: "object `number` generation `generation` R".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Reference {
    pub number: u32,
    pub generation: u16,
}

impl Reference {
    /// A reference with generation 0 (the common case for freshly written
    /// files).
    pub fn new(number: u32) -> Self {
        Reference {
            number,
            generation: 0,
        }
    }
}

/// A PDF dictionary. Entries are kept in insertion order so output is
/// deterministic (important for golden-file and round-trip tests).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dict {
    entries: Vec<(Name, Object)>,
}

impl Dict {
    /// An empty dictionary.
    pub fn new() -> Self {
        Dict::default()
    }

    /// Insert or replace `key`. Returns `self` for builder-style chaining.
    pub fn set(&mut self, key: impl Into<Name>, value: impl Into<Object>) -> &mut Self {
        let key = key.into();
        let value = value.into();
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
        } else {
            self.entries.push((key, value));
        }
        self
    }

    /// Builder variant of [`Dict::set`] that consumes and returns `self`.
    #[must_use]
    pub fn with(mut self, key: impl Into<Name>, value: impl Into<Object>) -> Self {
        self.set(key, value);
        self
    }

    /// Look up a value by name.
    pub fn get(&self, key: &str) -> Option<&Object> {
        self.entries
            .iter()
            .find(|(k, _)| k.as_str() == key)
            .map(|(_, v)| v)
    }

    /// True if `key` is present.
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Remove `key`, returning its value if it was present. Preserves the
    /// insertion order of the remaining entries.
    pub fn remove(&mut self, key: &str) -> Option<Object> {
        let pos = self.entries.iter().position(|(k, _)| k.as_str() == key)?;
        Some(self.entries.remove(pos).1)
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if there are no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over entries in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&Name, &Object)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }
}

/// A stream object: a dictionary followed by raw byte data.
///
/// On serialization, `/Length` is filled in automatically as a direct
/// integer when absent. To use an *indirect* length, set `/Length` to a
/// [`Reference`] in the dict before serializing and it will be preserved.
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub dict: Dict,
    pub data: Vec<u8>,
}

impl Stream {
    /// A stream with the given data and an empty dictionary.
    pub fn new(data: impl Into<Vec<u8>>) -> Self {
        Stream {
            dict: Dict::new(),
            data: data.into(),
        }
    }

    /// A stream with both dictionary and data supplied.
    pub fn with_dict(dict: Dict, data: impl Into<Vec<u8>>) -> Self {
        Stream {
            dict,
            data: data.into(),
        }
    }
}

/// Any COS object.
#[derive(Debug, Clone, PartialEq)]
pub enum Object {
    Null,
    Bool(bool),
    Integer(i64),
    Real(f64),
    Name(Name),
    String(PdfString),
    Array(Vec<Object>),
    Dict(Dict),
    Stream(Stream),
    Reference(Reference),
}

// ---- Ergonomic conversions so builders read nicely -------------------------

impl From<bool> for Object {
    fn from(v: bool) -> Self {
        Object::Bool(v)
    }
}

macro_rules! from_int {
    ($($t:ty),*) => {$(
        impl From<$t> for Object {
            fn from(v: $t) -> Self {
                Object::Integer(v as i64)
            }
        }
    )*};
}
from_int!(i8, i16, i32, i64, u8, u16, u32, usize);

impl From<f32> for Object {
    fn from(v: f32) -> Self {
        Object::Real(v as f64)
    }
}

impl From<f64> for Object {
    fn from(v: f64) -> Self {
        Object::Real(v)
    }
}

impl From<Name> for Object {
    fn from(v: Name) -> Self {
        Object::Name(v)
    }
}

impl From<PdfString> for Object {
    fn from(v: PdfString) -> Self {
        Object::String(v)
    }
}

impl From<Vec<Object>> for Object {
    fn from(v: Vec<Object>) -> Self {
        Object::Array(v)
    }
}

impl From<Dict> for Object {
    fn from(v: Dict) -> Self {
        Object::Dict(v)
    }
}

impl From<Stream> for Object {
    fn from(v: Stream) -> Self {
        Object::Stream(v)
    }
}

impl From<Reference> for Object {
    fn from(v: Reference) -> Self {
        Object::Reference(v)
    }
}

impl Object {
    /// Convenience constructor for a name object.
    pub fn name(s: impl Into<String>) -> Self {
        Object::Name(Name::new(s))
    }

    /// Convenience constructor for an indirect reference (generation 0).
    pub fn reference(number: u32) -> Self {
        Object::Reference(Reference::new(number))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dict_set_replaces_in_place_keeping_order() {
        let mut d = Dict::new();
        d.set("A", 1).set("B", 2).set("A", 3);
        assert_eq!(d.len(), 2);
        assert_eq!(d.get("A"), Some(&Object::Integer(3)));
        let keys: Vec<_> = d.iter().map(|(k, _)| k.as_str().to_owned()).collect();
        assert_eq!(keys, vec!["A", "B"]);
    }

    #[test]
    fn conversions() {
        assert_eq!(Object::from(true), Object::Bool(true));
        assert_eq!(Object::from(7u32), Object::Integer(7));
        assert_eq!(Object::from(1.5f64), Object::Real(1.5));
        assert_eq!(Object::reference(4), Object::Reference(Reference::new(4)));
    }
}

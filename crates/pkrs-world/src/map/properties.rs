//! Engine-owned custom properties attached to maps, layers, tiles and objects (§6.2).

use std::collections::BTreeMap;

/// One custom property value, mirroring the Tiled property types the engine understands.
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyValue {
    /// `bool` property.
    Bool(bool),
    /// `int` property.
    Int(i64),
    /// `float` property.
    Float(f64),
    /// `string` property.
    String(String),
    /// `file` property: a path relative to the file that declared it.
    File(String),
    /// `color` property as `[r, g, b, a]`.
    Color([u8; 4]),
    /// `object` property: id of another object on the same map (`0` = unset).
    Object(u32),
    /// `list` property.
    List(Vec<PropertyValue>),
    /// `class` property: a named type with nested properties.
    Class {
        /// Class (custom type) name.
        class: String,
        /// Nested properties.
        properties: Properties,
    },
}

impl PropertyValue {
    /// Human-readable type name used in convention errors.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::String(_) => "string",
            Self::File(_) => "file",
            Self::Color(_) => "color",
            Self::Object(_) => "object",
            Self::List(_) => "list",
            Self::Class { .. } => "class",
        }
    }
}

/// Ordered property map (deterministic iteration for tests and `pkrs check` output).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Properties(BTreeMap<String, PropertyValue>);

impl Properties {
    /// Empty property set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or replaces `key`.
    pub fn insert(&mut self, key: impl Into<String>, value: PropertyValue) {
        self.0.insert(key.into(), value);
    }

    /// Builder-style [`insert`](Self::insert).
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: PropertyValue) -> Self {
        self.insert(key, value);
        self
    }

    /// Raw value for `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&PropertyValue> {
        self.0.get(key)
    }

    /// `true` when `key` is present.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    /// Number of properties.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// `true` when there are no properties.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterates `(key, value)` in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &PropertyValue)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Typed lookup: `Ok(None)` if absent, `Err(found_type)` if present with another type.
    pub fn bool(&self, key: &str) -> Result<Option<bool>, &'static str> {
        match self.get(key) {
            None => Ok(None),
            Some(PropertyValue::Bool(b)) => Ok(Some(*b)),
            Some(other) => Err(other.type_name()),
        }
    }

    /// Typed lookup for integers (`Ok(None)` if absent, `Err(found_type)` on type mismatch).
    pub fn int(&self, key: &str) -> Result<Option<i64>, &'static str> {
        match self.get(key) {
            None => Ok(None),
            Some(PropertyValue::Int(i)) => Ok(Some(*i)),
            Some(other) => Err(other.type_name()),
        }
    }

    /// Typed lookup for strings; `file` properties are accepted as strings too.
    pub fn string(&self, key: &str) -> Result<Option<&str>, &'static str> {
        match self.get(key) {
            None => Ok(None),
            Some(PropertyValue::String(s) | PropertyValue::File(s)) => Ok(Some(s.as_str())),
            Some(other) => Err(other.type_name()),
        }
    }
}

impl FromIterator<(String, PropertyValue)> for Properties {
    fn from_iter<I: IntoIterator<Item = (String, PropertyValue)>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_lookups_report_mismatches() {
        let props = Properties::new()
            .with("solid", PropertyValue::Bool(true))
            .with("rate", PropertyValue::Int(20))
            .with("script", PropertyValue::String("maps/a.lua:f".into()))
            .with("img", PropertyValue::File("a.png".into()));
        assert_eq!(props.bool("solid"), Ok(Some(true)));
        assert_eq!(props.bool("missing"), Ok(None));
        assert_eq!(props.bool("rate"), Err("int"));
        assert_eq!(props.int("rate"), Ok(Some(20)));
        assert_eq!(props.string("script"), Ok(Some("maps/a.lua:f")));
        assert_eq!(props.string("img"), Ok(Some("a.png")));
        assert_eq!(props.string("solid"), Err("bool"));
    }

    #[test]
    fn iteration_is_key_ordered() {
        let props: Properties = [
            ("b".to_string(), PropertyValue::Int(2)),
            ("a".to_string(), PropertyValue::Int(1)),
        ]
        .into_iter()
        .collect();
        let keys: Vec<_> = props.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["a", "b"]);
    }
}

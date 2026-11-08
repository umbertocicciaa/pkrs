//! [`Value`]: the save model's own dynamic value type (§12).
//!
//! Script variables (`game.set_var`, `map.set_var`) and plugin save data are stored as
//! `Value`s so the save model never depends on the script engine or on a codec crate (§4.5).
//! There is no `nil`: storing `nil` removes the entry instead.
//!
//! A whole save is also a `Value` tree while it is migrated (§12): migrations read and rewrite
//! it with the helpers below ([`get`](Value::get), [`get_mut`](Value::get_mut),
//! [`as_map_mut`](Value::as_map_mut), …) before it is turned into typed [`SaveData`].
//!
//! [`SaveData`]: crate::SaveData

use core::fmt;
use std::collections::BTreeMap;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, Serializer};

/// A dynamically typed value stored in a save.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// Boolean.
    Bool(bool),
    /// Integer.
    Int(i64),
    /// Float.
    Float(f64),
    /// UTF-8 string.
    String(String),
    /// Ordered list.
    List(Vec<Value>),
    /// String-keyed table.
    Map(BTreeMap<String, Value>),
}

impl Value {
    /// An empty [`Map`](Self::Map).
    #[must_use]
    pub fn map() -> Self {
        Self::Map(BTreeMap::new())
    }

    /// Integer value.
    #[must_use]
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// Float value (integers are widened).
    #[must_use]
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            Self::Int(i) => Some(*i as f64),
            _ => None,
        }
    }

    /// Boolean value.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// String value.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// List items.
    #[must_use]
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Self::List(items) => Some(items),
            _ => None,
        }
    }

    /// Table entries.
    #[must_use]
    pub fn as_map(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Self::Map(map) => Some(map),
            _ => None,
        }
    }

    /// Table entries, mutably.
    pub fn as_map_mut(&mut self) -> Option<&mut BTreeMap<String, Value>> {
        match self {
            Self::Map(map) => Some(map),
            _ => None,
        }
    }

    /// Entry `key` of a table (`None` for a missing key or a value that is not a table).
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_map()?.get(key)
    }

    /// Entry `key` of a table, mutably.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.as_map_mut()?.get_mut(key)
    }

    /// The value at the dotted `path` (`"header.format_version"`).
    #[must_use]
    pub fn pointer(&self, path: &str) -> Option<&Value> {
        path.split('.').try_fold(self, |v, key| v.get(key))
    }

    /// The value at the dotted `path`, mutably.
    pub fn pointer_mut(&mut self, path: &str) -> Option<&mut Value> {
        path.split('.').try_fold(self, |v, key| v.get_mut(key))
    }

    /// Sets entry `key` of a table; returns the previous value. Does nothing (and returns
    /// `None`) when `self` is not a table.
    pub fn insert(&mut self, key: &str, value: Value) -> Option<Value> {
        self.as_map_mut()?.insert(key.to_owned(), value)
    }

    /// Removes entry `key` of a table and returns it.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.as_map_mut()?.remove(key)
    }

    /// Short name of the value's kind, for error messages.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::String(_) => "string",
            Self::List(_) => "list",
            Self::Map(_) => "map",
        }
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Self::Int(i)
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Self {
        Self::Float(f)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Self::String(s.to_owned())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<Vec<Value>> for Value {
    fn from(items: Vec<Value>) -> Self {
        Self::List(items)
    }
}

impl From<BTreeMap<String, Value>> for Value {
    fn from(map: BTreeMap<String, Value>) -> Self {
        Self::Map(map)
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Bool(b) => serializer.serialize_bool(*b),
            Self::Int(i) => serializer.serialize_i64(*i),
            Self::Float(f) => serializer.serialize_f64(*f),
            Self::String(s) => serializer.serialize_str(s),
            Self::List(items) => items.serialize(serializer),
            Self::Map(map) => map.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a bool, number, string, list or map")
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Int(v))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        // Bit-cast like the typed save serializer does for `u64` fields (RNG seeds).
        Ok(Value::Int(v as i64))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        Ok(Value::Float(v))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_char<E: de::Error>(self, v: char) -> Result<Value, E> {
        Ok(Value::String(v.to_string()))
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Value::List(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Value, A::Error> {
        let mut map = BTreeMap::new();
        while let Some((key, value)) = access.next_entry::<String, Value>()? {
            map.insert(key, value);
        }
        Ok(Value::Map(map))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_helpers_read_and_rewrite_trees() {
        let mut v = Value::map();
        v.insert("header", Value::map());
        v.pointer_mut("header")
            .unwrap()
            .insert("format_version", Value::Int(1));
        assert_eq!(v.pointer("header.format_version"), Some(&Value::Int(1)));
        assert_eq!(v.pointer("header.missing"), None);
        assert_eq!(v.pointer("header.format_version.deeper"), None);
        let old = v.pointer_mut("header").unwrap().remove("format_version");
        assert_eq!(old, Some(Value::Int(1)));
        assert_eq!(Value::Int(3).insert("x", Value::Int(1)), None);
        assert_eq!(Value::Int(3).as_float(), Some(3.0));
        assert_eq!(Value::from(vec![Value::from(true)]).kind(), "list");
    }
}

//! [`Value`]: the save model's own dynamic value type (§12).
//!
//! Script variables (`game.set_var`, `map.set_var`) and plugin save data are stored as
//! `Value`s so the save model never depends on the script engine or on a codec crate (§4.5).
//! There is no `nil`: storing `nil` removes the entry instead.

use std::collections::BTreeMap;

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
    /// Integer value.
    #[must_use]
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
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

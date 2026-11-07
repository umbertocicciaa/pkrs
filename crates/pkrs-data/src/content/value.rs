//! [`ContentValue`]: a dynamic value (e.g. a Lua table) that content records deserialize
//! from, so `data.register_move { … }` uses the same schemas as the RON files.
//!
//! Enum values are written as strings (`category = "special"`), enum variants with data as a
//! one-entry table (`residual = { max_hp_fraction = { denom = 8 } }`), tuples as lists, and
//! an empty table is accepted where a list is expected.

use core::fmt;
use std::collections::BTreeMap;

use serde::de::value::{MapDeserializer, SeqDeserializer};
use serde::de::{self, DeserializeSeed, IntoDeserializer, Visitor};

/// A dynamic content value.
#[derive(Clone, Debug, PartialEq)]
pub enum ContentValue {
    /// No value (`nil`).
    Unit,
    /// Boolean.
    Bool(bool),
    /// Integer.
    Int(i64),
    /// Float.
    Float(f64),
    /// String.
    Str(String),
    /// Sequence.
    List(Vec<ContentValue>),
    /// Table with string keys.
    Map(BTreeMap<String, ContentValue>),
}

/// Why a [`ContentValue`] did not match a schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentValueError(String);

impl fmt::Display for ContentValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ContentValueError {}

impl de::Error for ContentValueError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self(msg.to_string())
    }
}

impl<'de> IntoDeserializer<'de, ContentValueError> for ContentValue {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}

type Res<T> = Result<T, ContentValueError>;

fn visit_list<'de, V: Visitor<'de>>(items: Vec<ContentValue>, visitor: V) -> Res<V::Value> {
    let mut seq = SeqDeserializer::new(items.into_iter());
    let value = visitor.visit_seq(&mut seq)?;
    seq.end()?;
    Ok(value)
}

impl<'de> de::Deserializer<'de> for ContentValue {
    type Error = ContentValueError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Res<V::Value> {
        match self {
            Self::Unit => visitor.visit_unit(),
            Self::Bool(b) => visitor.visit_bool(b),
            Self::Int(i) => visitor.visit_i64(i),
            Self::Float(f) => visitor.visit_f64(f),
            Self::Str(s) => visitor.visit_string(s),
            Self::List(items) => visit_list(items, visitor),
            Self::Map(map) => {
                let mut access = MapDeserializer::new(map.into_iter());
                let value = visitor.visit_map(&mut access)?;
                access.end()?;
                Ok(value)
            }
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Res<V::Value> {
        match self {
            Self::Unit => visitor.visit_none(),
            other => visitor.visit_some(other),
        }
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Res<V::Value> {
        match self {
            Self::Map(map) if map.is_empty() => visit_list(Vec::new(), visitor),
            other => other.deserialize_any(visitor),
        }
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Res<V::Value> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Res<V::Value> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Res<V::Value> {
        match self {
            Self::Str(s) => {
                visitor.visit_enum(IntoDeserializer::<ContentValueError>::into_deserializer(s))
            }
            Self::Map(map) if map.len() == 1 => {
                let Some((variant, value)) = map.into_iter().next() else {
                    return Err(de::Error::custom(format!("expected a {name} value")));
                };
                visitor.visit_enum(EnumValue { variant, value })
            }
            other => Err(de::Error::custom(format!(
                "expected a {name} (a string, or a table with one entry), got {}",
                other.type_name()
            ))),
        }
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
        unit unit_struct tuple_struct map struct identifier ignored_any
    }
}

impl ContentValue {
    fn type_name(&self) -> &'static str {
        match self {
            Self::Unit => "nil",
            Self::Bool(_) => "a boolean",
            Self::Int(_) | Self::Float(_) => "a number",
            Self::Str(_) => "a string",
            Self::List(_) => "a list",
            Self::Map(_) => "a table",
        }
    }
}

struct EnumValue {
    variant: String,
    value: ContentValue,
}

impl<'de> de::EnumAccess<'de> for EnumValue {
    type Error = ContentValueError;
    type Variant = VariantValue;

    fn variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Res<(S::Value, VariantValue)> {
        let variant = seed.deserialize(
            IntoDeserializer::<ContentValueError>::into_deserializer(self.variant),
        )?;
        Ok((variant, VariantValue(self.value)))
    }
}

struct VariantValue(ContentValue);

impl<'de> de::VariantAccess<'de> for VariantValue {
    type Error = ContentValueError;

    fn unit_variant(self) -> Res<()> {
        match self.0 {
            ContentValue::Unit => Ok(()),
            ContentValue::Map(m) if m.is_empty() => Ok(()),
            other => Err(de::Error::custom(format!(
                "unit variant takes no value, got {}",
                other.type_name()
            ))),
        }
    }

    fn newtype_variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Res<S::Value> {
        seed.deserialize(self.0)
    }

    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Res<V::Value> {
        de::Deserializer::deserialize_seq(self.0, visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Res<V::Value> {
        de::Deserializer::deserialize_any(self.0, visitor)
    }
}

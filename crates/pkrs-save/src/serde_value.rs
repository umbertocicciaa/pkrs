//! Typed save structs ⇄ [`Value`] trees, through `serde` (§12).
//!
//! Saves are read as a `Value` tree first, so engine and game migrations can rewrite them
//! before typed deserialization; writing goes the other way. The mapping:
//!
//! - structs and maps become [`Value::Map`]s (map keys must be strings, integers or bools);
//!   a field whose value is `None` (or `()`) is left out, and a missing field reads as `None`
//!   or its `#[serde(default)]`;
//! - sequences and tuples become [`Value::List`]s;
//! - unit enum variants become their name as a [`Value::String`]; other variants become a
//!   one-entry map `{ "Variant": payload }`;
//! - `u64` is stored bit-cast in [`Value::Int`] (RNG seeds use the full range) and read back
//!   the same way; other integers must fit their type.
//!
//! An empty list and an empty map read as each other, since scripts cannot tell them apart.

use core::fmt;
use std::collections::BTreeMap;

use serde::de::value::{MapAccessDeserializer, MapDeserializer, SeqDeserializer};
use serde::de::{self, DeserializeOwned, IntoDeserializer, Visitor};
use serde::ser::{self, Serialize};

use crate::error::{Result, SaveError};
use crate::value::Value;

/// Converts a typed value into a [`Value`] tree.
pub fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<Value> {
    value
        .serialize(ValueSerializer)
        .map_err(|e| SaveError::Schema(e.0))?
        .ok_or_else(|| SaveError::Schema("value has no saved form (nil)".into()))
}

/// Converts a [`Value`] tree into a typed value.
pub fn from_value<T: DeserializeOwned>(value: Value) -> Result<T> {
    T::deserialize(value).map_err(|e| SaveError::Schema(e.0))
}

/// Error of the value serializer and deserializer.
#[derive(Debug)]
pub struct ValueError(String);

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ValueError {}

impl ser::Error for ValueError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self(msg.to_string())
    }
}

impl de::Error for ValueError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self(msg.to_string())
    }
}

type SerResult = core::result::Result<Option<Value>, ValueError>;

/// Serializes into `Option<Value>`; `None` means "no value" (a `None` option or `()`).
struct ValueSerializer;

fn present<T: Serialize + ?Sized>(
    value: &T,
    what: &str,
) -> core::result::Result<Value, ValueError> {
    value
        .serialize(ValueSerializer)?
        .ok_or_else(|| ValueError(format!("{what} cannot be nil")))
}

fn key_string(key: Value) -> core::result::Result<String, ValueError> {
    match key {
        Value::String(s) => Ok(s),
        Value::Int(i) => Ok(i.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        other => Err(ValueError(format!(
            "map keys must be strings, got a {}",
            other.kind()
        ))),
    }
}

impl ser::Serializer for ValueSerializer {
    type Ok = Option<Value>;
    type Error = ValueError;
    type SerializeSeq = SeqSer;
    type SerializeTuple = SeqSer;
    type SerializeTupleStruct = SeqSer;
    type SerializeTupleVariant = VariantSeqSer;
    type SerializeMap = MapSer;
    type SerializeStruct = MapSer;
    type SerializeStructVariant = VariantMapSer;

    fn serialize_bool(self, v: bool) -> SerResult {
        Ok(Some(Value::Bool(v)))
    }
    fn serialize_i8(self, v: i8) -> SerResult {
        Ok(Some(Value::Int(v.into())))
    }
    fn serialize_i16(self, v: i16) -> SerResult {
        Ok(Some(Value::Int(v.into())))
    }
    fn serialize_i32(self, v: i32) -> SerResult {
        Ok(Some(Value::Int(v.into())))
    }
    fn serialize_i64(self, v: i64) -> SerResult {
        Ok(Some(Value::Int(v)))
    }
    fn serialize_u8(self, v: u8) -> SerResult {
        Ok(Some(Value::Int(v.into())))
    }
    fn serialize_u16(self, v: u16) -> SerResult {
        Ok(Some(Value::Int(v.into())))
    }
    fn serialize_u32(self, v: u32) -> SerResult {
        Ok(Some(Value::Int(v.into())))
    }
    fn serialize_u64(self, v: u64) -> SerResult {
        // Bit-cast: read back by `deserialize_u64`.
        Ok(Some(Value::Int(v as i64)))
    }
    fn serialize_f32(self, v: f32) -> SerResult {
        Ok(Some(Value::Float(v.into())))
    }
    fn serialize_f64(self, v: f64) -> SerResult {
        Ok(Some(Value::Float(v)))
    }
    fn serialize_char(self, v: char) -> SerResult {
        Ok(Some(Value::String(v.to_string())))
    }
    fn serialize_str(self, v: &str) -> SerResult {
        Ok(Some(Value::String(v.to_owned())))
    }
    fn serialize_bytes(self, v: &[u8]) -> SerResult {
        Ok(Some(Value::List(
            v.iter().map(|b| Value::Int((*b).into())).collect(),
        )))
    }
    fn serialize_none(self) -> SerResult {
        Ok(None)
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> SerResult {
        value.serialize(self)
    }
    fn serialize_unit(self) -> SerResult {
        Ok(None)
    }
    fn serialize_unit_struct(self, _name: &'static str) -> SerResult {
        Ok(None)
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> SerResult {
        Ok(Some(Value::String(variant.to_owned())))
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> SerResult {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> SerResult {
        let inner = present(value, variant)?;
        Ok(Some(Value::Map(BTreeMap::from([(
            variant.to_owned(),
            inner,
        )]))))
    }
    fn serialize_seq(self, len: Option<usize>) -> core::result::Result<SeqSer, ValueError> {
        Ok(SeqSer(Vec::with_capacity(len.unwrap_or(0))))
    }
    fn serialize_tuple(self, len: usize) -> core::result::Result<SeqSer, ValueError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> core::result::Result<SeqSer, ValueError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> core::result::Result<VariantSeqSer, ValueError> {
        Ok(VariantSeqSer(variant, SeqSer(Vec::with_capacity(len))))
    }
    fn serialize_map(self, _len: Option<usize>) -> core::result::Result<MapSer, ValueError> {
        Ok(MapSer::default())
    }
    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> core::result::Result<MapSer, ValueError> {
        Ok(MapSer::default())
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> core::result::Result<VariantMapSer, ValueError> {
        Ok(VariantMapSer(variant, MapSer::default()))
    }
}

struct SeqSer(Vec<Value>);

impl SeqSer {
    fn push<T: Serialize + ?Sized>(&mut self, value: &T) -> core::result::Result<(), ValueError> {
        self.0.push(present(value, "a list item")?);
        Ok(())
    }
}

impl ser::SerializeSeq for SeqSer {
    type Ok = Option<Value>;
    type Error = ValueError;
    fn serialize_element<T: Serialize + ?Sized>(
        &mut self,
        value: &T,
    ) -> core::result::Result<(), ValueError> {
        self.push(value)
    }
    fn end(self) -> SerResult {
        Ok(Some(Value::List(self.0)))
    }
}

impl ser::SerializeTuple for SeqSer {
    type Ok = Option<Value>;
    type Error = ValueError;
    fn serialize_element<T: Serialize + ?Sized>(
        &mut self,
        value: &T,
    ) -> core::result::Result<(), ValueError> {
        self.push(value)
    }
    fn end(self) -> SerResult {
        Ok(Some(Value::List(self.0)))
    }
}

impl ser::SerializeTupleStruct for SeqSer {
    type Ok = Option<Value>;
    type Error = ValueError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        value: &T,
    ) -> core::result::Result<(), ValueError> {
        self.push(value)
    }
    fn end(self) -> SerResult {
        Ok(Some(Value::List(self.0)))
    }
}

struct VariantSeqSer(&'static str, SeqSer);

impl ser::SerializeTupleVariant for VariantSeqSer {
    type Ok = Option<Value>;
    type Error = ValueError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        value: &T,
    ) -> core::result::Result<(), ValueError> {
        self.1.push(value)
    }
    fn end(self) -> SerResult {
        Ok(Some(Value::Map(BTreeMap::from([(
            self.0.to_owned(),
            Value::List(self.1.0),
        )]))))
    }
}

#[derive(Default)]
struct MapSer {
    map: BTreeMap<String, Value>,
    key: Option<String>,
}

impl MapSer {
    fn field<T: Serialize + ?Sized>(
        &mut self,
        key: String,
        value: &T,
    ) -> core::result::Result<(), ValueError> {
        if let Some(value) = value.serialize(ValueSerializer)? {
            self.map.insert(key, value);
        }
        Ok(())
    }
}

impl ser::SerializeMap for MapSer {
    type Ok = Option<Value>;
    type Error = ValueError;
    fn serialize_key<T: Serialize + ?Sized>(
        &mut self,
        key: &T,
    ) -> core::result::Result<(), ValueError> {
        self.key = Some(key_string(present(key, "a map key")?)?);
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(
        &mut self,
        value: &T,
    ) -> core::result::Result<(), ValueError> {
        let key = self
            .key
            .take()
            .ok_or_else(|| ValueError("map value without a key".into()))?;
        self.field(key, value)
    }
    fn end(self) -> SerResult {
        Ok(Some(Value::Map(self.map)))
    }
}

impl ser::SerializeStruct for MapSer {
    type Ok = Option<Value>;
    type Error = ValueError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> core::result::Result<(), ValueError> {
        self.field(key.to_owned(), value)
    }
    fn end(self) -> SerResult {
        Ok(Some(Value::Map(self.map)))
    }
}

struct VariantMapSer(&'static str, MapSer);

impl ser::SerializeStructVariant for VariantMapSer {
    type Ok = Option<Value>;
    type Error = ValueError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> core::result::Result<(), ValueError> {
        self.1.field(key.to_owned(), value)
    }
    fn end(self) -> SerResult {
        Ok(Some(Value::Map(BTreeMap::from([(
            self.0.to_owned(),
            Value::Map(self.1.map),
        )]))))
    }
}

impl IntoDeserializer<'_, ValueError> for Value {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self {
        self
    }
}

fn visit_list<'de, V: Visitor<'de>>(
    items: Vec<Value>,
    visitor: V,
) -> core::result::Result<V::Value, ValueError> {
    let mut seq = SeqDeserializer::new(items.into_iter());
    let value = visitor.visit_seq(&mut seq)?;
    seq.end()?;
    Ok(value)
}

fn visit_table<'de, V: Visitor<'de>>(
    map: BTreeMap<String, Value>,
    visitor: V,
) -> core::result::Result<V::Value, ValueError> {
    let mut access = MapDeserializer::new(map.into_iter());
    let value = visitor.visit_map(&mut access)?;
    access.end()?;
    Ok(value)
}

impl<'de> de::Deserializer<'de> for Value {
    type Error = ValueError;

    fn deserialize_any<V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> core::result::Result<V::Value, ValueError> {
        match self {
            Value::Bool(b) => visitor.visit_bool(b),
            Value::Int(i) => visitor.visit_i64(i),
            Value::Float(f) => visitor.visit_f64(f),
            Value::String(s) => visitor.visit_string(s),
            Value::List(items) => visit_list(items, visitor),
            Value::Map(map) => visit_table(map, visitor),
        }
    }

    fn deserialize_u64<V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> core::result::Result<V::Value, ValueError> {
        match self {
            Value::Int(i) => visitor.visit_u64(i as u64),
            other => other.deserialize_any(visitor),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> core::result::Result<V::Value, ValueError> {
        visitor.visit_some(self)
    }

    fn deserialize_unit<V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> core::result::Result<V::Value, ValueError> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> core::result::Result<V::Value, ValueError> {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> core::result::Result<V::Value, ValueError> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> core::result::Result<V::Value, ValueError> {
        match self {
            Value::Map(map) if map.is_empty() => visit_list(Vec::new(), visitor),
            other => other.deserialize_any(visitor),
        }
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> core::result::Result<V::Value, ValueError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> core::result::Result<V::Value, ValueError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> core::result::Result<V::Value, ValueError> {
        match self {
            Value::List(items) if items.is_empty() => visit_table(BTreeMap::new(), visitor),
            other => other.deserialize_any(visitor),
        }
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> core::result::Result<V::Value, ValueError> {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> core::result::Result<V::Value, ValueError> {
        match self {
            Value::String(variant) => visitor.visit_enum(variant.into_deserializer()),
            Value::Map(map) if map.len() == 1 => visitor.visit_enum(MapAccessDeserializer::new(
                MapDeserializer::new(map.into_iter()),
            )),
            other => Err(ValueError(format!(
                "expected a `{name}` variant (a string or a one-entry map), got a {}",
                other.kind()
            ))),
        }
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(
        self,
        visitor: V,
    ) -> core::result::Result<V::Value, ValueError> {
        visitor.visit_unit()
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u128 f32 f64 char str string bytes byte_buf
        identifier
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    enum Shape {
        Dot,
        Circle(f64),
        Rect { w: u32, h: u32 },
        Pair(i32, i32),
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        name: String,
        seed: u64,
        level: u8,
        maybe: Option<i32>,
        shapes: Vec<Shape>,
        tags: BTreeMap<String, bool>,
        pos: (i32, i32),
    }

    #[test]
    fn typed_values_round_trip_through_value_trees() {
        let sample = Sample {
            name: "ann".into(),
            seed: u64::MAX - 3,
            level: 7,
            maybe: None,
            shapes: vec![
                Shape::Dot,
                Shape::Circle(1.5),
                Shape::Rect { w: 2, h: 3 },
                Shape::Pair(-1, 4),
            ],
            tags: BTreeMap::from([("a".into(), true)]),
            pos: (3, -4),
        };
        let value = to_value(&sample).unwrap();
        assert_eq!(value.get("maybe"), None, "None fields are left out");
        assert_eq!(
            value
                .get("shapes")
                .and_then(|s| s.as_list())
                .map(|s| s[0].clone()),
            Some(Value::from("Dot"))
        );
        let back: Sample = from_value(value).unwrap();
        assert_eq!(back, sample);
    }

    #[test]
    fn empty_tables_read_as_lists_or_maps() {
        let list: Vec<i32> = from_value(Value::map()).unwrap();
        assert!(list.is_empty());
        let map: BTreeMap<String, i32> = from_value(Value::List(Vec::new())).unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn schema_mismatches_are_errors() {
        let err = from_value::<u8>(Value::Int(300)).unwrap_err();
        assert!(matches!(err, SaveError::Schema(_)), "{err}");
        let err = from_value::<Sample>(Value::Int(1)).unwrap_err();
        assert!(err.to_string().contains("schema"), "{err}");
        assert!(to_value(&None::<i32>).is_err());
        assert!(to_value(&vec![None::<i32>]).is_err());
        let int_keys = BTreeMap::from([(1, 2)]);
        assert_eq!(
            to_value(&int_keys).unwrap(),
            Value::Map(BTreeMap::from([("1".into(), Value::Int(2))]))
        );
    }

    #[test]
    fn floats_accept_integers() {
        let f: f64 = from_value(Value::Int(2)).unwrap();
        assert_eq!(f, 2.0);
    }
}

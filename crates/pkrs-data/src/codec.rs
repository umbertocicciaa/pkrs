//! [`RonCodec`]: the only module of `pkrs-data` that uses the `ron` crate (§4.5).
//!
//! Every content file is decoded here into the crate's typed schema structs; `ron` types
//! never leave this module. Parse errors become [`DataError::Parse`] with the file and the
//! line/column RON reports (§8.2 stage 1).
//!
//! The decoder enables RON's `implicit_some` extension, so optional fields may be written
//! as `field: value` as well as `field: Some(value)`.

use core::fmt;

use serde::de::{self, DeserializeOwned, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};

use crate::error::{DataError, Result};

/// RON decoder for content files.
#[derive(Clone, Copy, Debug, Default)]
pub struct RonCodec;

/// A record decoded from a content file, with its `override` flag (§8.1, §9.4).
#[derive(Clone, Debug, PartialEq)]
pub struct Decoded<T> {
    /// The record.
    pub value: T,
    /// The record declared `override: true`.
    pub override_existing: bool,
}

impl RonCodec {
    /// Decodes one RON document of type `T`. `file` names the source in errors.
    pub fn decode<T: DeserializeOwned>(source: &str, file: &str) -> Result<T> {
        ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .from_str(source)
            .map_err(|e| DataError::Parse {
                file: file.into(),
                message: e.to_string(),
            })
    }

    /// Decodes a file holding **one record or a list of records** (§8.1). Each record may
    /// set `override: true`; the flag is returned next to the record. Struct names
    /// (`Species(…)`, `Move(…)`) are optional and checked by the schema.
    pub fn decode_records<T: DeserializeOwned>(
        source: &str,
        file: &str,
    ) -> Result<Vec<Decoded<T>>> {
        match Self::decode::<Shape>(source, file) {
            Ok(Shape::List(flags)) => {
                let values: Vec<T> = Self::decode(source, file)?;
                Ok(values
                    .into_iter()
                    .zip(flags)
                    .map(|(value, override_existing)| Decoded {
                        value,
                        override_existing,
                    })
                    .collect())
            }
            Ok(Shape::One(override_existing)) => Ok(vec![Decoded {
                value: Self::decode(source, file)?,
                override_existing,
            }]),
            // The schema decoder gives the more useful message; when it accepts the document,
            // the shape scan failed on the `override` flag itself.
            Err(shape_err) => match Self::decode::<T>(source, file) {
                Ok(_) => Err(shape_err),
                Err(schema_err) => Err(schema_err),
            },
        }
    }

    /// [`decode_records`](Self::decode_records) without the `override` flags.
    pub fn decode_one_or_many<T: DeserializeOwned>(source: &str, file: &str) -> Result<Vec<T>> {
        Ok(Self::decode_records(source, file)?
            .into_iter()
            .map(|d| d.value)
            .collect())
    }
}

/// Top-level shape of a records file: a list (with each element's `override` flag) or one
/// record (with its flag).
enum Shape {
    List(Vec<bool>),
    One(bool),
}

impl<'de> de::Deserialize<'de> for Shape {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> core::result::Result<Self, D::Error> {
        struct ShapeVisitor;

        impl<'de> Visitor<'de> for ShapeVisitor {
            type Value = Shape;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a record or a list of records")
            }

            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> core::result::Result<Shape, A::Error> {
                let mut flags = Vec::new();
                while let Some(OverrideFlag(flag)) = seq.next_element()? {
                    flags.push(flag);
                }
                Ok(Shape::List(flags))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> core::result::Result<Shape, A::Error> {
                Ok(Shape::One(scan_override(map)?))
            }
        }

        deserializer.deserialize_any(ShapeVisitor)
    }
}

/// The `override` flag of one record (`false` when absent or not a record).
struct OverrideFlag(bool);

impl<'de> de::Deserialize<'de> for OverrideFlag {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> core::result::Result<Self, D::Error> {
        struct FlagVisitor;

        impl<'de> Visitor<'de> for FlagVisitor {
            type Value = OverrideFlag;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a record")
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                map: A,
            ) -> core::result::Result<OverrideFlag, A::Error> {
                Ok(OverrideFlag(scan_override(map)?))
            }

            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> core::result::Result<OverrideFlag, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(OverrideFlag(false))
            }

            fn visit_str<E: de::Error>(self, _: &str) -> core::result::Result<OverrideFlag, E> {
                Ok(OverrideFlag(false))
            }

            fn visit_unit<E: de::Error>(self) -> core::result::Result<OverrideFlag, E> {
                Ok(OverrideFlag(false))
            }
        }

        deserializer.deserialize_any(FlagVisitor)
    }
}

/// Reads every entry of a record, returning its `override: <bool>` value.
fn scan_override<'de, A: MapAccess<'de>>(mut map: A) -> core::result::Result<bool, A::Error> {
    let mut flag = false;
    while let Some(Key(key)) = map.next_key()? {
        if key.as_deref() == Some("override") {
            flag = map.next_value::<bool>()?;
        } else {
            map.next_value::<IgnoredAny>()?;
        }
    }
    Ok(flag)
}

/// A map key: its text when it is a string or identifier.
struct Key(Option<String>);

impl<'de> de::Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> core::result::Result<Self, D::Error> {
        struct KeyVisitor;

        impl<'de> Visitor<'de> for KeyVisitor {
            type Value = Key;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a key")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> core::result::Result<Key, E> {
                Ok(Key(Some(v.to_owned())))
            }

            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> core::result::Result<Key, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Key(None))
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> core::result::Result<Key, A::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(Key(None))
            }

            fn visit_i64<E: de::Error>(self, _: i64) -> core::result::Result<Key, E> {
                Ok(Key(None))
            }

            fn visit_u64<E: de::Error>(self, _: u64) -> core::result::Result<Key, E> {
                Ok(Key(None))
            }

            fn visit_bool<E: de::Error>(self, _: bool) -> core::result::Result<Key, E> {
                Ok(Key(None))
            }
        }

        deserializer.deserialize_any(KeyVisitor)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::collections::BTreeMap;

    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Deserialize, PartialEq)]
    #[serde(rename = "Thing")]
    struct Thing {
        id: String,
        #[serde(default)]
        size: Option<u8>,
    }

    fn records(source: &str) -> Vec<(String, bool)> {
        RonCodec::decode_records::<Thing>(source, "t.ron")
            .unwrap()
            .into_iter()
            .map(|d| (d.value.id, d.override_existing))
            .collect()
    }

    #[test]
    fn one_record_or_a_list_named_or_not() {
        assert_eq!(records(r#"(id: "a")"#), [("a".into(), false)]);
        assert_eq!(
            records(r#"Thing(id: "a", override: true)"#),
            [("a".into(), true)]
        );
        assert_eq!(
            records(
                r#"// comment first
                [Thing(id: "a"), (id: "b", override: true, size: 3), (override: false, id: "c")]"#
            ),
            [("a".into(), false), ("b".into(), true), ("c".into(), false)]
        );
        assert!(records("[]").is_empty());
    }

    #[test]
    fn implicit_some_and_wrong_names() {
        let t: Thing = RonCodec::decode(r#"(id: "a", size: 4)"#, "t.ron").unwrap();
        assert_eq!(t.size, Some(4));
        let t: Thing = RonCodec::decode(r#"(id: "a", size: Some(4))"#, "t.ron").unwrap();
        assert_eq!(t.size, Some(4));
        assert!(RonCodec::decode_records::<Thing>(r#"Other(id: "a")"#, "t.ron").is_err());
    }

    #[test]
    fn errors_name_the_file_and_position() {
        let err = RonCodec::decode_records::<Thing>("(id: )", "data/x/t.ron").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.starts_with("failed to parse `data/x/t.ron`: 1:"),
            "{msg}"
        );
        let err = RonCodec::decode_records::<Thing>("(size: 1)", "t.ron").unwrap_err();
        assert!(err.to_string().contains("id"), "{err}");
        let err = RonCodec::decode_records::<Thing>(r#"(id: "a", override: 3)"#, "t.ron");
        assert!(err.is_err());
    }

    #[test]
    fn maps_with_any_keys_decode() {
        let map: BTreeMap<String, String> =
            RonCodec::decode(r#"{ "a.b": "x", "c": "y" }"#, "t.ron").unwrap();
        assert_eq!(map.len(), 2);
    }
}

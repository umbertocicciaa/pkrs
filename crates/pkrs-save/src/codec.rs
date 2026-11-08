//! [`RonCodec`]: the only module of `pkrs-save` that uses the `ron` crate (§4.5, §12).
//!
//! Saves are pretty-printed RON documents of a [`Value`] tree: tables are RON maps with string
//! keys, lists are RON lists. `ron` types never leave this module; errors become
//! [`SaveError::Codec`].

use crate::error::{Result, SaveError};
use crate::value::Value;

/// RON encoder/decoder for save files.
#[derive(Clone, Copy, Debug, Default)]
pub struct RonCodec;

impl RonCodec {
    /// Pretty-printed RON text of `value`.
    pub fn encode(value: &Value) -> Result<String> {
        let config = ron::ser::PrettyConfig::new()
            .indentor("    ".to_owned())
            .struct_names(false);
        let mut text = ron::ser::to_string_pretty(value, config)
            .map_err(|e| SaveError::Codec(e.to_string()))?;
        text.push('\n');
        Ok(text)
    }

    /// Parses RON `text` into a [`Value`] tree. `file` names the source in errors.
    pub fn decode(text: &str, file: &str) -> Result<Value> {
        ron::Options::default()
            .from_str::<Value>(text)
            .map_err(|e| SaveError::Codec(format!("{file}: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn values_round_trip_through_pretty_ron() {
        let value = Value::Map(BTreeMap::from([
            ("flag".into(), Value::Bool(true)),
            ("int".into(), Value::Int(-7)),
            ("whole".into(), Value::Float(1.0)),
            ("float".into(), Value::Float(0.25)),
            ("text".into(), Value::from("say \"hi\"\n")),
            (
                "list".into(),
                Value::List(vec![Value::Int(1), Value::map(), Value::List(Vec::new())]),
            ),
            ("big".into(), Value::Int(i64::MIN)),
        ]));
        let text = RonCodec::encode(&value).unwrap();
        assert!(text.contains('\n'), "pretty-printed: {text}");
        assert_eq!(RonCodec::decode(&text, "t.sav").unwrap(), value);
    }

    #[test]
    fn invalid_text_is_a_codec_error_naming_the_file() {
        let err = RonCodec::decode("{ \"a\": ", "slot1.sav").unwrap_err();
        assert!(
            matches!(err, SaveError::Codec(ref m) if m.contains("slot1.sav")),
            "{err}"
        );
    }
}

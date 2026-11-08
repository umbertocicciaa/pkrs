//! The only module of `pkrs-audio` that uses the `ron` crate (§4.5): decodes the audio
//! manifest. RON's `implicit_some` extension is on, so optional fields may be written as
//! `loop_end: 123` as well as `loop_end: Some(123)`.

use serde::de::DeserializeOwned;

use crate::error::{AudioError, Result};

/// Decodes one RON document; `file` names the source in errors.
pub(crate) fn decode<T: DeserializeOwned>(source: &str, file: &str) -> Result<T> {
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(source)
        .map_err(|e| AudioError::Manifest {
            file: file.to_owned(),
            message: e.to_string(),
        })
}

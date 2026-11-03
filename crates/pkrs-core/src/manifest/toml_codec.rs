//! `toml` adapter — the only module that imports `toml` (§4.5).
//!
//! Deserializes `pkrs.toml` into the private raw model, attaching the failing field path
//! (via `serde_path_to_error`) and the source line/column to any error.

use super::error::{Location, ManifestIssue};
use super::raw::RawManifest;

/// Parses manifest source text into the raw (unvalidated) model.
pub(super) fn parse(src: &str) -> Result<RawManifest, ManifestIssue> {
    let de = toml::Deserializer::parse(src).map_err(|e| issue(src, String::new(), &e))?;
    serde_path_to_error::deserialize(de).map_err(|e| {
        let path = e.path().to_string();
        // `serde_path_to_error` renders the root as ".".
        let path = if path == "." { String::new() } else { path };
        issue(src, path, e.inner())
    })
}

fn issue(src: &str, path: String, err: &toml::de::Error) -> ManifestIssue {
    ManifestIssue {
        path,
        message: err.message().trim().to_owned(),
        location: err
            .span()
            .map(|span| Location::from_offset(src, span.start)),
    }
}

//! `pak.ron` codec: the only module of `pkrs-assets` that uses the `ron` crate (§4.5).

use super::{PakError, PakIndex};

/// Pretty-printed RON text of `index` (`pak.ron`).
pub fn index_to_ron(index: &PakIndex) -> Result<String, PakError> {
    let config = ron::ser::PrettyConfig::new()
        .indentor("    ".to_owned())
        .struct_names(true);
    let mut text =
        ron::ser::to_string_pretty(index, config).map_err(|e| PakError::Index(e.to_string()))?;
    text.push('\n');
    Ok(text)
}

/// Parses `pak.ron` text. Only parses: call [`PakIndex::validate`] before trusting it.
pub fn index_from_ron(text: &str) -> Result<PakIndex, PakError> {
    ron::Options::default()
        .from_str::<PakIndex>(text)
        .map_err(|e| PakError::Index(e.to_string()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::super::tests::sample_index;
    use super::*;

    #[test]
    fn index_round_trips_through_ron() {
        let index = sample_index();
        let text = index_to_ron(&index).unwrap();
        assert!(text.starts_with("PakIndex("), "{text}");
        assert!(text.contains("game_id: \"com.example.town\""), "{text}");
        let back = index_from_ron(&text).unwrap();
        assert_eq!(back, index);
        back.validate().unwrap();
    }

    #[test]
    fn malformed_index_is_an_index_error() {
        let err = index_from_ron("PakIndex(format: 1").unwrap_err();
        assert!(matches!(err, PakError::Index(_)), "{err}");
    }
}

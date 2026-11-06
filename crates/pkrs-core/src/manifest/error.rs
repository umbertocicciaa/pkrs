//! Manifest errors: every problem carries the dotted field path it refers to.

use core::fmt;
use std::path::PathBuf;

use thiserror::Error;

/// A 1-based line/column position in the manifest source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Location {
    /// 1-based line.
    pub line: usize,
    /// 1-based column (in characters).
    pub column: usize,
}

impl Location {
    /// Converts a byte offset in `src` to a line/column position.
    pub(crate) fn from_offset(src: &str, offset: usize) -> Self {
        let offset = offset.min(src.len());
        // Snap to a char boundary so slicing cannot panic on multi-byte text.
        let offset = (0..=offset)
            .rev()
            .find(|&i| src.is_char_boundary(i))
            .unwrap_or(0);
        let before = &src[..offset];
        let line = before.matches('\n').count() + 1;
        let line_start = before.rfind('\n').map_or(0, |i| i + 1);
        let column = before[line_start..].chars().count() + 1;
        Self { line, column }
    }
}

/// One problem found in `pkrs.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestIssue {
    /// Dotted field path such as `display.scale` or `plugins.enabled[1]`; empty for the whole file.
    pub path: String,
    /// Human-readable description.
    pub message: String,
    /// Source position, when known (parse errors only).
    pub location: Option<Location>,
}

impl ManifestIssue {
    /// Creates an issue for `path` without a source position.
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
            location: None,
        }
    }
}

impl fmt::Display for ManifestIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(loc) = self.location {
            write!(f, "{}:{}: ", loc.line, loc.column)?;
        }
        if !self.path.is_empty() {
            write!(f, "`{}`: ", self.path)?;
        }
        f.write_str(&self.message)
    }
}

/// Errors from loading or validating `pkrs.toml`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ManifestError {
    /// The manifest file could not be read.
    #[error("cannot read {}: {source}", path.display())]
    Io {
        /// The file that failed to read.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The file is not valid TOML, or a value has the wrong type / an unknown key.
    #[error("pkrs.toml: {0}")]
    Parse(ManifestIssue),

    /// The file parsed but failed validation; all problems are reported at once.
    #[error("pkrs.toml is invalid:{}", IssueList(.0))]
    Invalid(Vec<ManifestIssue>),
}

impl ManifestError {
    /// All issues carried by this error (empty for I/O errors).
    pub fn issues(&self) -> &[ManifestIssue] {
        match self {
            Self::Io { .. } => &[],
            Self::Parse(issue) => core::slice::from_ref(issue),
            Self::Invalid(issues) => issues,
        }
    }
}

/// Errors from loading or validating a plugin's `plugin.toml` (§9.4).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PluginManifestError {
    /// The file is not valid TOML, or a value has the wrong type / an unknown key.
    #[error("plugin.toml: {0}")]
    Parse(ManifestIssue),

    /// The file parsed but failed validation; all problems are reported at once.
    #[error("plugin.toml is invalid:{}", IssueList(.0))]
    Invalid(Vec<ManifestIssue>),
}

impl PluginManifestError {
    /// All issues carried by this error.
    pub fn issues(&self) -> &[ManifestIssue] {
        match self {
            Self::Parse(issue) => core::slice::from_ref(issue),
            Self::Invalid(issues) => issues,
        }
    }
}

struct IssueList<'a>(&'a [ManifestIssue]);

impl fmt::Display for IssueList<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for issue in self.0 {
            write!(f, "\n  - {issue}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_counts_lines_and_chars() {
        let src = "a = 1\nbé = 2\n";
        assert_eq!(
            Location::from_offset(src, 0),
            Location { line: 1, column: 1 }
        );
        // Offset of `=` on line 2: "a = 1\n" (6 bytes) + "bé " (4 bytes).
        assert_eq!(
            Location::from_offset(src, 10),
            Location { line: 2, column: 4 }
        );
        // Inside the multi-byte `é` snaps back to its start; past the end clamps.
        assert_eq!(
            Location::from_offset(src, 8),
            Location { line: 2, column: 2 }
        );
        assert_eq!(Location::from_offset(src, 999).line, 3);
    }

    #[test]
    fn invalid_lists_every_issue() {
        let err = ManifestError::Invalid(vec![
            ManifestIssue::new("game.id", "must not be empty"),
            ManifestIssue::new("display.scale", "must be between 1 and 16"),
        ]);
        assert_eq!(
            err.to_string(),
            "pkrs.toml is invalid:\n  - `game.id`: must not be empty\n  - `display.scale`: must be between 1 and 16"
        );
        assert_eq!(err.issues().len(), 2);
    }

    #[test]
    fn parse_issue_shows_location() {
        let err = ManifestError::Parse(ManifestIssue {
            path: "display.scale".into(),
            message: "invalid type: string \"3\", expected i64".into(),
            location: Some(Location { line: 4, column: 9 }),
        });
        assert_eq!(
            err.to_string(),
            "pkrs.toml: 4:9: `display.scale`: invalid type: string \"3\", expected i64"
        );
    }
}

//! Project templates embedded in the CLI (`pkrs new`, §2.4).
//!
//! Templates live under `crates/pkrs-cli/templates/<name>/` and are embedded with
//! `include_dir`, so `pkrs new` works offline. Text files (`.toml`, `.md`, `.lua`, `.txt`,
//! `_gitignore`) have `{{name}}`, `{{id}}` and `{{engine_version}}` substituted; everything else
//! is copied byte for byte. A file named `_gitignore` is written as `.gitignore`.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use include_dir::{Dir, DirEntry, include_dir};

static ONE_SHOT: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/templates/one-shot");

/// Available templates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Template {
    /// One small map and the player; no battle data (§2.4).
    OneShot,
    /// Two maps, battles and a plugin (§2.4; ships with M8).
    FullGame,
}

impl Template {
    /// CLI spelling.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::OneShot => "one-shot",
            Self::FullGame => "full-game",
        }
    }

    fn dir(self) -> Option<&'static Dir<'static>> {
        match self {
            Self::OneShot => Some(&ONE_SHOT),
            Self::FullGame => None,
        }
    }
}

/// Why scaffolding failed.
#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    /// The project name cannot be used.
    #[error(
        "invalid project name `{0}`: use 1-64 ASCII letters, digits, spaces, `_` or `-`, starting with a letter or digit"
    )]
    InvalidName(String),
    /// The destination exists and is not empty.
    #[error("`{0}` already exists and is not empty")]
    DestinationNotEmpty(PathBuf),
    /// The template is not shipped yet.
    #[error("the `{0}` template is not available yet (it ships with the sample projects, M8)")]
    Unavailable(&'static str),
    /// Writing a file failed.
    #[error("cannot write `{path}`: {source}")]
    Io {
        /// File or directory.
        path: PathBuf,
        /// I/O error.
        #[source]
        source: std::io::Error,
    },
}

/// Values substituted into template text files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateVars {
    /// `game.name` (the name given to `pkrs new`).
    pub name: String,
    /// `game.id` (`com.example.<slug>`).
    pub id: String,
    /// `engine.version`, pinned to this CLI's version.
    pub engine_version: String,
}

impl TemplateVars {
    /// Variables for a project called `name`.
    pub fn for_project(name: &str) -> Result<Self, TemplateError> {
        if !valid_name(name) {
            return Err(TemplateError::InvalidName(name.to_owned()));
        }
        Ok(Self {
            name: name.to_owned(),
            id: format!("com.example.{}", slug(name)),
            engine_version: pinned_engine_version().to_owned(),
        })
    }
}

impl fmt::Display for TemplateVars {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}, engine {})",
            self.name, self.id, self.engine_version
        )
    }
}

/// The `engine.version` written into new projects: this CLI's own version (§2.3).
#[must_use]
pub fn pinned_engine_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '-'))
        && !name.ends_with(' ')
}

/// `My Game_2` → `my-game_2` (a valid `game.id` segment).
#[must_use]
pub fn slug(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            ' ' => '-',
            c => c.to_ascii_lowercase(),
        })
        .collect()
}

/// Substitutes `{{name}}`, `{{id}}` and `{{engine_version}}`.
#[must_use]
pub fn render(text: &str, vars: &TemplateVars) -> String {
    text.replace("{{name}}", &vars.name)
        .replace("{{id}}", &vars.id)
        .replace("{{engine_version}}", &vars.engine_version)
}

fn is_text(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n == "_gitignore")
        || path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e, "toml" | "md" | "lua" | "txt"))
}

fn output_path(rel: &Path) -> PathBuf {
    match rel.file_name() {
        Some(n) if n == "_gitignore" => rel.with_file_name(".gitignore"),
        _ => rel.to_path_buf(),
    }
}

/// Writes `template` into `dest` (created if missing; must be empty if it exists). Returns
/// the files written, relative to `dest`, in a stable order.
pub fn scaffold(
    template: Template,
    dest: &Path,
    vars: &TemplateVars,
) -> Result<Vec<PathBuf>, TemplateError> {
    let dir = template
        .dir()
        .ok_or(TemplateError::Unavailable(template.name()))?;
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |source| TemplateError::Io { path, source }
    };
    if dest.exists() {
        let mut entries = fs::read_dir(dest).map_err(io(dest))?;
        if entries.next().is_some() {
            return Err(TemplateError::DestinationNotEmpty(dest.to_path_buf()));
        }
    }
    fs::create_dir_all(dest).map_err(io(dest))?;

    let mut files = Vec::new();
    collect(dir, &mut files);
    files.sort_by(|a, b| a.path().cmp(b.path()));
    let mut written = Vec::with_capacity(files.len());
    for file in files {
        let rel = output_path(file.path());
        let target = dest.join(&rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(io(parent))?;
        }
        let bytes = if is_text(file.path()) {
            match file.contents_utf8() {
                Some(text) => render(text, vars).into_bytes(),
                None => file.contents().to_vec(),
            }
        } else {
            file.contents().to_vec()
        };
        fs::write(&target, bytes).map_err(io(&target))?;
        written.push(rel);
    }
    Ok(written)
}

fn collect<'a>(dir: &'a Dir<'a>, out: &mut Vec<&'a include_dir::File<'a>>) {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(d) => collect(d, out),
            DirEntry::File(f) => out.push(f),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_slugs() {
        for ok in ["demo", "Lighthouse Tale", "my_game-2", "A"] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in [
            "",
            " lead",
            "trail ",
            "a/b",
            "dots.no",
            "../x",
            "é",
            &"x".repeat(65),
        ] {
            assert!(!valid_name(bad), "{bad}");
        }
        assert_eq!(slug("Lighthouse Tale"), "lighthouse-tale");
        assert_eq!(
            TemplateVars::for_project("Lighthouse Tale").unwrap().id,
            "com.example.lighthouse-tale"
        );
    }

    #[test]
    fn render_substitutes_every_placeholder() {
        let vars = TemplateVars {
            name: "Demo".into(),
            id: "com.example.demo".into(),
            engine_version: "1.2.3".into(),
        };
        assert_eq!(
            render("{{name}}|{{id}}|{{engine_version}}|{{name}}", &vars),
            "Demo|com.example.demo|1.2.3|Demo"
        );
    }

    #[test]
    fn version_is_pinned_to_the_cli() {
        let vars = TemplateVars::for_project("demo").unwrap();
        assert_eq!(vars.engine_version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn rendered_manifest_is_valid() {
        let vars = TemplateVars::for_project("Lighthouse Tale").unwrap();
        let src = ONE_SHOT
            .get_file("pkrs.toml")
            .and_then(|f| f.contents_utf8())
            .unwrap();
        let manifest = pkrs_core::ProjectManifest::from_toml_str(&render(src, &vars)).unwrap();
        assert_eq!(manifest.game.name, "Lighthouse Tale");
        assert_eq!(manifest.game.id, "com.example.lighthouse-tale");
        assert_eq!(manifest.start.map, "start");
        assert!(!manifest.features.battles, "one-shot ships no battle data");
        let engine = pkrs_core::Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
        assert!(manifest.engine.accepts(&engine));
    }

    #[test]
    fn template_has_map_tileset_player_and_licenses() {
        for path in [
            "pkrs.toml",
            "_gitignore",
            "LICENSES.md",
            "maps/start.tmx",
            "maps/dest.tmx",
            "maps/north.tmx",
            "maps/world.ron",
            "tilesets/overworld.tsx",
            "tilesets/overworld.png",
            "sprites/player.png",
        ] {
            assert!(ONE_SHOT.get_file(path).is_some(), "{path}");
        }
        let gitignore = ONE_SHOT
            .get_file("_gitignore")
            .and_then(|f| f.contents_utf8())
            .unwrap();
        assert!(gitignore.lines().any(|l| l == "saves/"));
    }

    #[test]
    fn full_game_is_not_available_yet() {
        let vars = TemplateVars::for_project("x").unwrap();
        let dest = std::env::temp_dir().join("pkrs-template-unavailable");
        assert!(matches!(
            scaffold(Template::FullGame, &dest, &vars),
            Err(TemplateError::Unavailable("full-game"))
        ));
    }
}

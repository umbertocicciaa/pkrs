//! `pkrs upgrade` (§2.3, §2.5): the only command that edits `pkrs.toml`.
//!
//! It bumps `engine.version` to an exact pin, runs the project migration registry (renamed
//! manifest keys) and prints a changelog. Deprecated scripting calls are listed for manual
//! attention; scripts are not rewritten.
//!
//! Every migration is idempotent: a second upgrade of an already current project does not
//! change a byte of `pkrs.toml`.

use std::fmt;
use std::path::{Path, PathBuf};

use pkrs_core::{ProjectManifest, Version};
use pkrs_script::{FoundDeprecation, find_deprecations};

/// Why `pkrs upgrade` could not run. The manifest is left untouched.
#[derive(Debug, thiserror::Error)]
pub enum UpgradeError {
    /// `pkrs.toml` could not be read.
    #[error("cannot read {}: {source}", path.display())]
    Read {
        /// The file that failed to read.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// `pkrs.toml` could not be written.
    #[error("cannot write {}: {source}", path.display())]
    Write {
        /// The file that failed to write.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// `[engine] version` is missing.
    #[error("pkrs.toml has no [engine] version string")]
    MissingVersion,
    /// `[engine] version` is not a quoted string.
    #[error("engine.version must be a quoted string")]
    VersionNotString,
    /// `--to` is not a full `major.minor.patch` version.
    #[error("--to `{input}` is not a full version ({reason})")]
    BadTarget {
        /// The rejected text.
        input: String,
        /// Why it was rejected.
        reason: String,
    },
    /// The project is already pinned to a newer exact version than `--to`.
    #[error("refusing to downgrade engine.version from {from} to {to}")]
    Downgrade {
        /// The pin in the manifest.
        from: String,
        /// The requested pin.
        to: String,
    },
    /// Migrations produced a manifest the loader rejects. Nothing was written.
    #[error("migrated pkrs.toml is not valid and was not written: {0}")]
    InvalidManifest(pkrs_core::ManifestError),
}

/// One migration the registry applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppliedMigration {
    /// Stable id (`window-to-display`).
    pub id: &'static str,
    /// Changelog line.
    pub summary: &'static str,
}

/// A deprecated call `pkrs upgrade` will not rewrite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManualAttention {
    /// Project-relative path (`/`-separated).
    pub path: String,
    /// The call and where it is.
    pub hit: FoundDeprecation,
}

/// What `pkrs upgrade` did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpgradeReport {
    /// `engine.version` before the pin.
    pub from_version: String,
    /// Exact version now pinned.
    pub to_version: String,
    /// Migrations that changed the manifest, in registry order.
    pub applied: Vec<AppliedMigration>,
    /// Deprecated scripting calls, in file order.
    pub deprecations: Vec<ManualAttention>,
    /// `true` when `pkrs.toml` was written.
    pub wrote_manifest: bool,
}

impl fmt::Display for UpgradeReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "engine.version: {} -> {}",
            self.from_version, self.to_version
        )?;
        writeln!(f, "migrations:")?;
        if self.applied.is_empty() {
            writeln!(f, "  (none)")?;
        } else {
            for step in &self.applied {
                writeln!(f, "  - {}: {}", step.id, step.summary)?;
            }
        }
        writeln!(f, "manual attention:")?;
        if self.deprecations.is_empty() {
            writeln!(f, "  (none)")?;
        } else {
            for item in &self.deprecations {
                writeln!(
                    f,
                    "  - {}:{}:{}: {}",
                    item.path,
                    item.hit.span.line,
                    item.hit.span.column,
                    item.hit.message()
                )?;
            }
        }
        if self.wrote_manifest {
            writeln!(f, "wrote pkrs.toml")?;
        } else {
            writeln!(f, "pkrs.toml unchanged")?;
        }
        Ok(())
    }
}

/// `--to`, or this CLI's own version when `to` is `None`.
pub fn resolve_target(to: Option<&str>) -> Result<Version, UpgradeError> {
    let raw = to.unwrap_or(env!("CARGO_PKG_VERSION"));
    Version::parse(raw).map_err(|err| UpgradeError::BadTarget {
        input: raw.to_owned(),
        reason: err.to_string(),
    })
}

/// Upgrades the project at `dir` to `target` (§2.5).
pub fn upgrade_project(dir: &Path, target: &Version) -> Result<UpgradeReport, UpgradeError> {
    let path = dir.join(ProjectManifest::FILE_NAME);
    let original = std::fs::read_to_string(&path).map_err(|source| UpgradeError::Read {
        path: path.clone(),
        source,
    })?;
    let upgraded = upgrade_manifest(&original, target)?;
    let changed = upgraded.text != original;
    let api = if changed {
        let manifest = ProjectManifest::from_toml_str(&upgraded.text)
            .map_err(UpgradeError::InvalidManifest)?;
        write_manifest(dir, &upgraded.text)?;
        manifest.engine.api
    } else {
        ProjectManifest::from_toml_str(&original)
            .map(|manifest| manifest.engine.api)
            .unwrap_or(1)
    };
    Ok(UpgradeReport {
        from_version: upgraded.from_version,
        to_version: target.to_string(),
        applied: upgraded.applied,
        deprecations: scan_deprecations(dir, api),
        wrote_manifest: changed,
    })
}

#[derive(Debug)]
struct ManifestUpgrade {
    text: String,
    from_version: String,
    applied: Vec<AppliedMigration>,
}

/// Rewrites manifest text. Does not touch the filesystem.
fn upgrade_manifest(src: &str, target: &Version) -> Result<ManifestUpgrade, UpgradeError> {
    let from_version = engine_version(src)?;
    if let Ok(current) = Version::parse(&from_version)
        && current > *target
    {
        return Err(UpgradeError::Downgrade {
            from: from_version,
            to: target.to_string(),
        });
    }
    let (text, applied) = apply_migrations(src);
    let text = match pin_engine_version(&text, target)? {
        Some(pinned) => pinned,
        None => text,
    };
    Ok(ManifestUpgrade {
        text,
        from_version,
        applied,
    })
}

struct Step {
    id: &'static str,
    summary: &'static str,
    apply: fn(&str) -> Option<String>,
}

/// Draft manifest keys the current schema replaced (§2.2, §2.5).
const STEPS: &[Step] = &[
    Step {
        id: "window-to-display",
        summary: "renamed [window] to [display]",
        apply: rename_window,
    },
    Step {
        id: "start-pos-to-position",
        summary: "renamed start.pos to start.position",
        apply: rename_start_pos,
    },
    Step {
        id: "engine-api-level-to-api",
        summary: "renamed engine.api_level to engine.api",
        apply: rename_api_level,
    },
];

fn apply_migrations(src: &str) -> (String, Vec<AppliedMigration>) {
    let mut text = src.to_owned();
    let mut applied = Vec::new();
    for step in STEPS {
        if let Some(next) = (step.apply)(&text) {
            text = next;
            applied.push(AppliedMigration {
                id: step.id,
                summary: step.summary,
            });
        }
    }
    (text, applied)
}

fn rename_window(src: &str) -> Option<String> {
    rename_table(src, "window", "display")
}

fn rename_start_pos(src: &str) -> Option<String> {
    rename_key(src, "start", "pos", "position")
}

fn rename_api_level(src: &str) -> Option<String> {
    rename_key(src, "engine", "api_level", "api")
}

fn rename_table(src: &str, from: &str, to: &str) -> Option<String> {
    let mut changed = false;
    let mut out = String::with_capacity(src.len());
    for line in lines_with_ends(src) {
        if let Some(new_line) = rewrite_header(line, from, to) {
            changed = true;
            out.push_str(&new_line);
        } else {
            out.push_str(line);
        }
    }
    changed.then_some(out)
}

fn rename_key(src: &str, table: &str, from: &str, to: &str) -> Option<String> {
    rewrite_in_table(src, table, |line| rewrite_key_line(line, from, to))
}

fn pin_engine_version(src: &str, target: &Version) -> Result<Option<String>, UpgradeError> {
    let target = target.to_string();
    if engine_version(src)? == target {
        return Ok(None);
    }
    rewrite_in_table(src, "engine", |line| rewrite_version_line(line, &target))
        .ok_or(UpgradeError::MissingVersion)
        .map(Some)
}

fn rewrite_in_table(
    src: &str,
    table: &str,
    edit: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    let mut changed = false;
    let mut in_table = false;
    let mut out = String::with_capacity(src.len());
    for line in lines_with_ends(src) {
        if let Some(header) = header(line) {
            in_table = matches!(header, Header::Simple(name) if name == table);
            out.push_str(line);
            continue;
        }
        if in_table && let Some(new_line) = edit(line) {
            changed = true;
            out.push_str(&new_line);
            continue;
        }
        out.push_str(line);
    }
    changed.then_some(out)
}

enum Header<'a> {
    Simple(&'a str),
    Other,
}

fn header(line: &str) -> Option<Header<'_>> {
    let (bare, _) = split_ending(line);
    let (code, _) = split_comment(bare);
    let code = code.trim();
    if !code.starts_with('[') || !code.ends_with(']') {
        return None;
    }
    let inner = code.strip_prefix('[')?.strip_suffix(']')?.trim();
    if inner.starts_with('[') {
        return Some(Header::Other);
    }
    match bare_ident(inner) {
        Some(name) => Some(Header::Simple(name)),
        None => Some(Header::Other),
    }
}

fn rewrite_header(line: &str, from: &str, to: &str) -> Option<String> {
    let (bare, nl) = split_ending(line);
    let (code, comment) = split_comment(bare);
    let trimmed = code.trim();
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?.trim();
    if bare_ident(inner)? != from {
        return None;
    }
    let start = code.find('[')?;
    let end = code.rfind(']')? + 1;
    Some(format!(
        "{}[{to}]{}{comment}{nl}",
        &code[..start],
        &code[end..]
    ))
}

fn rewrite_key_line(line: &str, from: &str, to: &str) -> Option<String> {
    let (bare, nl) = split_ending(line);
    let (code, comment) = split_comment(bare);
    let eq = code.find('=')?;
    if bare_ident(code[..eq].trim())? != from {
        return None;
    }
    let key_raw = code[..eq].trim();
    let start = code.find(key_raw)?;
    let end = start + key_raw.len();
    Some(format!(
        "{}{to}{}{comment}{nl}",
        &code[..start],
        &code[end..]
    ))
}

fn rewrite_version_line(line: &str, target: &str) -> Option<String> {
    let (bare, nl) = split_ending(line);
    let (code, comment) = split_comment(bare);
    let eq = code.find('=')?;
    if bare_ident(code[..eq].trim())? != "version" {
        return None;
    }
    let value_part = &code[eq + 1..];
    let pad = value_part.len() - value_part.trim_start().len();
    let value = value_part.trim_start();
    let quote = value.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = value.get(quote.len_utf8()..)?;
    let end = rest.find(quote)?;
    if &rest[..end] == target {
        return None;
    }
    let after = quote.len_utf8() + end + quote.len_utf8();
    Some(format!(
        "{}={}{quote}{target}{quote}{}{comment}{nl}",
        &code[..eq],
        &value_part[..pad],
        &value[after..]
    ))
}

enum VersionLiteral {
    Absent,
    String(String),
    NotString,
}

fn engine_version(src: &str) -> Result<String, UpgradeError> {
    let mut in_engine = false;
    let mut saw_unquoted = false;
    for line in lines_with_ends(src) {
        if let Some(header) = header(line) {
            in_engine = matches!(header, Header::Simple(name) if name == "engine");
            continue;
        }
        if !in_engine {
            continue;
        }
        match version_literal(line) {
            VersionLiteral::Absent => {}
            VersionLiteral::String(value) => return Ok(value),
            VersionLiteral::NotString => saw_unquoted = true,
        }
    }
    if saw_unquoted {
        Err(UpgradeError::VersionNotString)
    } else {
        Err(UpgradeError::MissingVersion)
    }
}

fn version_literal(line: &str) -> VersionLiteral {
    let (bare, _) = split_ending(line);
    let (code, _) = split_comment(bare);
    let Some(eq) = code.find('=') else {
        return VersionLiteral::Absent;
    };
    if bare_ident(code[..eq].trim()) != Some("version") {
        return VersionLiteral::Absent;
    }
    let value = code[eq + 1..].trim();
    let Some(quote) = value.chars().next() else {
        return VersionLiteral::NotString;
    };
    if quote != '"' && quote != '\'' {
        return VersionLiteral::NotString;
    }
    let rest = &value[quote.len_utf8()..];
    match rest.find(quote) {
        Some(end) => VersionLiteral::String(rest[..end].to_owned()),
        None => VersionLiteral::NotString,
    }
}

fn bare_ident(raw: &str) -> Option<&str> {
    let raw = raw.trim();
    let inner = if let Some(rest) = raw.strip_prefix('"') {
        rest.strip_suffix('"')?
    } else if let Some(rest) = raw.strip_prefix('\'') {
        rest.strip_suffix('\'')?
    } else {
        raw
    };
    if !inner.is_empty()
        && inner
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Some(inner)
    } else {
        None
    }
}

fn lines_with_ends(src: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut rest = src;
    while let Some(i) = rest.find('\n') {
        let (line, tail) = rest.split_at(i + 1);
        lines.push(line);
        rest = tail;
    }
    if !rest.is_empty() {
        lines.push(rest);
    }
    lines
}

fn split_ending(line: &str) -> (&str, &str) {
    if let Some(stripped) = line.strip_suffix("\r\n") {
        (stripped, "\r\n")
    } else if let Some(stripped) = line.strip_suffix('\n') {
        (stripped, "\n")
    } else {
        (line, "")
    }
}

/// Splits a TOML comment (`#`) that is not inside quotes. The comment keeps its `#`.
fn split_comment(bare: &str) -> (&str, &str) {
    let mut quote: Option<char> = None;
    let bytes = bare.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bare[i..].chars().next().unwrap_or('\0');
        let len = c.len_utf8();
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '\\' => {
                i += len;
                if let Some(next) = bare[i..].chars().next() {
                    i += next.len_utf8();
                }
                continue;
            }
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '#' => return (&bare[..i], &bare[i..]),
            None => {}
        }
        i += len;
    }
    (bare, "")
}

fn write_manifest(dir: &Path, text: &str) -> Result<(), UpgradeError> {
    let path = dir.join(ProjectManifest::FILE_NAME);
    let tmp = dir.join("pkrs.toml.tmp");
    std::fs::write(&tmp, text).map_err(|source| UpgradeError::Write {
        path: tmp.clone(),
        source,
    })?;
    if let Err(source) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(UpgradeError::Write { path, source });
    }
    Ok(())
}

fn scan_deprecations(dir: &Path, api: u32) -> Vec<ManualAttention> {
    let mut files = Vec::new();
    collect_lua(&dir.join("scripts"), 0, false, &mut files);
    collect_lua(&dir.join("plugins"), 0, true, &mut files);
    files.sort();
    let mut hits = Vec::new();
    for file in files {
        let Ok(source) = std::fs::read_to_string(&file) else {
            continue;
        };
        for hit in find_deprecations(&source, api) {
            hits.push(ManualAttention {
                path: relative_path(dir, &file),
                hit,
            });
        }
    }
    hits
}

fn collect_lua(dir: &Path, depth: usize, skip_plugin_assets: bool, out: &mut Vec<PathBuf>) {
    if depth > 16 || !dir.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if kind.is_dir() {
            if skip_plugin_assets && depth == 1 && (name == "assets" || name == "data") {
                continue;
            }
            collect_lua(&path, depth + 1, skip_plugin_assets, out);
        } else if kind.is_file() && name.ends_with(".lua") {
            out.push(path);
        }
    }
}

fn relative_path(project: &Path, file: &Path) -> String {
    file.strip_prefix(project)
        .unwrap_or(file)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = "\
# draft
[game]
name = \"Old Tale\"
id = \"com.example.old\"
version = \"0.1.0\"

[engine]
version = \"0.0.0\"
api_level = 1

[window]
width = 240

[start]
map = \"start\"
pos = [2, 3]
facing = \"down\"
";

    fn target(major: u64, minor: u64, patch: u64) -> Version {
        Version::new(major, minor, patch)
    }

    #[test]
    fn each_migration_renames_only_its_key_and_is_idempotent() {
        let window = "[game]\nx = 1\n# [window]\n[window]\nwidth = 1\n[other]\nwidth = 2\n";
        let once = rename_window(window).expect("renames");
        assert!(once.contains("[display]\n"));
        assert!(once.contains("# [window]\n"));
        assert!(!once.contains("\n[window]\n"));
        assert!(rename_window(&once).is_none());

        let pos = "[other]\npos = [1, 1]\n[start]\npos = [2, 3]\nposition = [0, 0]\n[start.extra]\npos = [9, 9]\n";
        let once = rename_start_pos(pos).expect("renames");
        assert!(once.contains("[other]\npos = [1, 1]\n"));
        assert!(once.contains("[start]\nposition = [2, 3]\n"));
        assert!(once.contains("[start.extra]\npos = [9, 9]\n"));
        assert!(rename_start_pos(&once).is_none());

        let api = "[game]\napi_level = 2\n[engine]\napi_level = 1\napi = 1\n";
        let once = rename_api_level(api).expect("renames");
        assert!(once.contains("[game]\napi_level = 2\n"));
        assert!(once.contains("[engine]\napi = 1\napi = 1\n"));
        assert!(rename_api_level(&once).is_none());
    }

    #[test]
    fn crlf_and_quoted_keys_survive_a_rename() {
        let src = "[window]\r\n\"pos\" = 1\r\n";
        let table = rename_window(src).unwrap();
        assert_eq!(table, "[display]\r\n\"pos\" = 1\r\n");
        let keyed = "[start]\r\n\"pos\" = [1, 2]\r\n";
        assert_eq!(
            rename_start_pos(keyed).unwrap(),
            "[start]\r\nposition = [1, 2]\r\n"
        );
    }

    #[test]
    fn upgrade_applies_every_step_then_is_idempotent() {
        let to = target(0, 0, 0);
        let once = upgrade_manifest(OLD, &to).unwrap();
        assert_eq!(once.from_version, "0.0.0");
        assert_eq!(
            once.applied.iter().map(|step| step.id).collect::<Vec<_>>(),
            [
                "window-to-display",
                "start-pos-to-position",
                "engine-api-level-to-api"
            ]
        );
        let text = &once.text;
        assert!(text.contains("[display]\n"), "{text}");
        assert!(text.contains("position = [2, 3]\n"), "{text}");
        assert!(text.contains("api = 1\n"), "{text}");
        assert!(text.contains("version = \"0.1.0\"\n"), "game version stays");
        assert!(!text.contains("api_level"));
        assert!(!text.contains("[window]"));
        let parsed = ProjectManifest::from_toml_str(text).expect("valid");
        assert!(parsed.check_engine(&to, 1..=1).is_ok());

        let twice = upgrade_manifest(text, &to).unwrap();
        assert_eq!(twice.text, *text);
        assert!(twice.applied.is_empty());
    }

    #[test]
    fn pin_replaces_a_requirement_and_leaves_the_game_version() {
        let src = "[game]\nversion = \"0.1.0\"  # game\n[engine]\nversion = \"0.3\"\napi = 1\n";
        let upgraded = upgrade_manifest(src, &target(0, 3, 1)).unwrap();
        assert!(upgraded.applied.is_empty());
        assert!(
            upgraded.text.contains("version = \"0.1.0\"  # game\n"),
            "{}",
            upgraded.text
        );
        assert!(
            upgraded.text.contains("version = \"0.3.1\"\n"),
            "{}",
            upgraded.text
        );
        let again = upgrade_manifest(&upgraded.text, &target(0, 3, 1)).unwrap();
        assert_eq!(again.text, upgraded.text);
    }

    #[test]
    fn an_exact_newer_pin_is_a_downgrade() {
        let src = "[engine]\nversion = \"1.0.0\"\napi = 1\n";
        let err = upgrade_manifest(src, &target(0, 0, 0)).unwrap_err();
        assert!(matches!(err, UpgradeError::Downgrade { .. }), "{err}");
    }

    #[test]
    fn missing_or_unquoted_version_is_an_error() {
        let missing = upgrade_manifest("[game]\nname = \"T\"\n", &target(0, 0, 0)).unwrap_err();
        assert!(matches!(missing, UpgradeError::MissingVersion));
        let bare = upgrade_manifest("[engine]\nversion = 1\n", &target(0, 0, 0)).unwrap_err();
        assert!(matches!(bare, UpgradeError::VersionNotString));
    }

    #[test]
    fn resolve_target_defaults_to_this_cli() {
        let version = resolve_target(None).unwrap();
        assert_eq!(version.to_string(), env!("CARGO_PKG_VERSION"));
        assert!(resolve_target(Some("0.3")).is_err());
        assert_eq!(resolve_target(Some("0.3.1")).unwrap().to_string(), "0.3.1");
    }

    #[test]
    fn a_current_project_is_not_rewritten_and_deprecations_are_listed() {
        let dir = temp_dir("current");
        std::fs::write(
            dir.join("pkrs.toml"),
            "[game]\nname = \"T\"\nid = \"com.example.t\"\n\
             [engine]\nversion = \"0.0.0\"\napi = 1\n\
             [start]\nmap = \"start\"\nposition = [0, 0]\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::write(dir.join("scripts/main.lua"), "audio.jingle('fanfare')\n").unwrap();
        std::fs::create_dir_all(dir.join("plugins/mud/assets")).unwrap();
        std::fs::write(
            dir.join("plugins/mud/assets/skip.lua"),
            "audio.jingle('hidden')\n",
        )
        .unwrap();
        let before = std::fs::read(dir.join("pkrs.toml")).unwrap();
        let report = upgrade_project(&dir, &target(0, 0, 0)).unwrap();
        assert!(!report.wrote_manifest);
        assert!(report.applied.is_empty());
        assert_eq!(std::fs::read(dir.join("pkrs.toml")).unwrap(), before);
        assert_eq!(report.deprecations.len(), 1);
        assert_eq!(report.deprecations[0].path, "scripts/main.lua");
        let text = report.to_string();
        assert!(text.contains("pkrs.toml unchanged"), "{text}");
        assert!(text.contains("audio.jingle"), "{text}");
        assert!(!text.contains("hidden"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_invalid_migration_is_not_written() {
        let dir = temp_dir("invalid");
        let original = "[engine]\nversion = \"0.0.0\"\napi_level = 1\n[window]\nwidth = 1\n";
        std::fs::write(dir.join("pkrs.toml"), original).unwrap();
        let err = upgrade_project(&dir, &target(0, 0, 0)).unwrap_err();
        assert!(matches!(err, UpgradeError::InvalidManifest(_)), "{err}");
        assert_eq!(
            std::fs::read_to_string(dir.join("pkrs.toml")).unwrap(),
            original
        );
        assert!(!dir.join("pkrs.toml.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("pkrs-upgrade-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}

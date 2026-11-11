//! Deprecated scripting calls (§9.7).
//!
//! Compatibility shims stay callable. [`find_deprecations`] is what `pkrs upgrade` lists as
//! manual attention, and `pkrs check` reports the same hits as `script::deprecated` warnings.
//! A call is reported when the project's `engine.api` is at least the level that deprecated it.

use pkrs_core::SourceSpan;

use crate::validate::LuaFacts;

/// One deprecated `table.name(` call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeprecatedCall {
    /// Module name (`audio`).
    pub table: &'static str,
    /// Deprecated field (`jingle`).
    pub name: &'static str,
    /// First API level at which this call is deprecated.
    pub deprecated_in: u32,
    /// The call to write instead (`audio.play_jingle`).
    pub replacement: &'static str,
}

/// Shims the engine still runs, and that `pkrs check` / `pkrs upgrade` report (§2.5, §9.7).
pub const DEPRECATED_CALLS: &[DeprecatedCall] = &[
    DeprecatedCall {
        table: "audio",
        name: "jingle",
        deprecated_in: 1,
        replacement: "audio.play_jingle",
    },
    DeprecatedCall {
        table: "map",
        name: "register_terrain",
        deprecated_in: 1,
        replacement: "world.register_terrain",
    },
];

/// One deprecated call found in a source file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoundDeprecation {
    /// Module name.
    pub table: &'static str,
    /// Deprecated field.
    pub name: &'static str,
    /// First API level at which this call is deprecated.
    pub deprecated_in: u32,
    /// The call to write instead.
    pub replacement: &'static str,
    /// Span of the field name.
    pub span: SourceSpan,
}

impl FoundDeprecation {
    /// The warning text, shared by `pkrs check` and the upgrade changelog.
    #[must_use]
    pub fn message(&self) -> String {
        format!(
            "`{}.{}` is deprecated since API {}; use `{}`",
            self.table, self.name, self.deprecated_in, self.replacement
        )
    }
}

/// Deprecated calls in `source` at scripting API level `api`, using [`DEPRECATED_CALLS`].
#[must_use]
pub fn find_deprecations(source: &str, api: u32) -> Vec<FoundDeprecation> {
    deprecations_in(&LuaFacts::scan(source), source, api)
}

/// Like [`find_deprecations`], over facts already scanned for this file.
#[must_use]
pub fn deprecations_in(facts: &LuaFacts, source: &str, api: u32) -> Vec<FoundDeprecation> {
    scan_catalog(facts, source, api, DEPRECATED_CALLS)
}

/// Deprecated calls in `source` against `catalog` (tests, and callers with an extra list).
#[must_use]
pub fn scan_deprecations(
    source: &str,
    api: u32,
    catalog: &[DeprecatedCall],
) -> Vec<FoundDeprecation> {
    scan_catalog(&LuaFacts::scan(source), source, api, catalog)
}

fn scan_catalog(
    facts: &LuaFacts,
    source: &str,
    api: u32,
    catalog: &[DeprecatedCall],
) -> Vec<FoundDeprecation> {
    let mut hits = Vec::new();
    for call in facts.member_calls() {
        let Some(entry) = catalog.iter().find(|entry| {
            entry.table == call.table && entry.name == call.name && api >= entry.deprecated_in
        }) else {
            continue;
        };
        hits.push(FoundDeprecation {
            table: entry.table,
            name: entry.name,
            deprecated_in: entry.deprecated_in,
            replacement: entry.replacement,
            span: SourceSpan::from_byte_range(source, call.name_range),
        });
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    const LATER: &[DeprecatedCall] = &[DeprecatedCall {
        table: "game",
        name: "old_flag",
        deprecated_in: 2,
        replacement: "game.flag",
    }];

    #[test]
    fn reports_catalogued_calls_and_skips_comments_and_strings() {
        let src = "\
audio.play_jingle('ok')
-- audio.jingle('no')
local s = 'audio.jingle'
audio.jingle('fanfare')
map.register_terrain('ice', function(ctx) end)
function map.register_terrain() end
";
        let hits = find_deprecations(src, 1);
        let names: Vec<&str> = hits.iter().map(|h| h.name).collect();
        assert_eq!(names, ["jingle", "register_terrain"]);
        assert_eq!(hits[0].span.line, 4);
        assert!(
            hits[0].message().contains("audio.play_jingle"),
            "{}",
            hits[0].message()
        );
        assert!(hits[1].message().contains("world.register_terrain"));
        assert!(
            find_deprecations(src, 0).is_empty(),
            "api 0 is before every entry"
        );
    }

    #[test]
    fn a_call_is_silent_until_its_deprecated_level() {
        let src = "game.old_flag('met')\n";
        assert!(scan_deprecations(src, 1, LATER).is_empty());
        let hits = scan_deprecations(src, 2, LATER);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].replacement, "game.flag");
    }
}

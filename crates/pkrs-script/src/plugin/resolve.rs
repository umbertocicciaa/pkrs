//! Validation and deterministic load order of the enabled plugin set (§9.4 steps 1–3, §9.7).
//!
//! Pure: works on already-parsed manifests, so it is unit-testable without files or Lua.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;

use pkrs_core::PluginManifest;

use super::error::{PluginError, PluginErrors};

/// The enabled plugins in resolved load order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PluginPlan {
    plugins: Vec<PluginManifest>,
}

impl PluginPlan {
    /// Manifests in load order.
    #[must_use]
    pub fn plugins(&self) -> &[PluginManifest] {
        &self.plugins
    }

    /// Plugin ids in load order.
    #[must_use]
    pub fn ids(&self) -> Vec<&str> {
        self.plugins.iter().map(PluginManifest::id).collect()
    }

    /// `true` when no plugin is enabled.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }
}

/// Validates the plugins named in `enabled` against the `available` (discovered) manifests and
/// sorts them into load order.
///
/// Checks, all reported at once: every enabled plugin exists; its API level is in
/// `supported_api`; every dependency is installed, enabled and satisfies its semver range; no
/// two enabled plugins conflict; `dependencies` + `load.after` have no cycle. `load.after`
/// entries that are not enabled are ignored (they are only hints).
///
/// The order is a topological sort in which a plugin loads after its dependencies and its
/// enabled `load.after` entries. Among plugins that are ready at the same time, the one listed
/// earlier in `enabled` goes first, then the smaller id — so the same set always yields the
/// same order.
pub fn resolve_load_order(
    available: &[PluginManifest],
    enabled: &[String],
    supported_api: RangeInclusive<u32>,
) -> Result<PluginPlan, PluginErrors> {
    let mut errors = Vec::new();

    let mut by_id: BTreeMap<&str, &PluginManifest> = BTreeMap::new();
    let mut duplicates = BTreeSet::new();
    for manifest in available {
        if by_id.insert(manifest.id(), manifest).is_some() {
            duplicates.insert(manifest.id());
        }
    }
    errors.extend(duplicates.into_iter().map(|id| PluginError::Duplicate {
        plugin: id.to_owned(),
    }));

    // The enabled set, keyed for tie-breaking: (position in `enabled`, id).
    let mut position: BTreeMap<&str, usize> = BTreeMap::new();
    let mut nodes: Vec<&PluginManifest> = Vec::new();
    for (index, id) in enabled.iter().enumerate() {
        if position.contains_key(id.as_str()) {
            continue; // `pkrs.toml` validation already rejects duplicates.
        }
        match by_id.get(id.as_str()) {
            Some(manifest) => {
                position.insert(id.as_str(), index);
                nodes.push(manifest);
            }
            None => errors.push(PluginError::NotFound {
                plugin: id.clone(),
                index,
            }),
        }
    }

    for manifest in &nodes {
        let id = manifest.id();
        let api = manifest.plugin.api;
        if !supported_api.contains(&api) {
            errors.push(PluginError::UnsupportedApi {
                plugin: id.to_owned(),
                api,
                min: *supported_api.start(),
                max: *supported_api.end(),
            });
        }
        for (dep, requirement) in &manifest.dependencies {
            match by_id.get(dep.as_str()) {
                None => errors.push(PluginError::MissingDependency {
                    plugin: id.to_owned(),
                    dependency: dep.clone(),
                    requirement: requirement.clone(),
                }),
                Some(_) if !position.contains_key(dep.as_str()) => {
                    errors.push(PluginError::DependencyNotEnabled {
                        plugin: id.to_owned(),
                        dependency: dep.clone(),
                    });
                }
                Some(found) if !requirement.matches(&found.plugin.version) => {
                    errors.push(PluginError::VersionMismatch {
                        plugin: id.to_owned(),
                        dependency: dep.clone(),
                        requirement: requirement.clone(),
                        found: found.plugin.version.clone(),
                    });
                }
                Some(_) => {}
            }
        }
    }

    // Conflicts: one error per unordered pair, attributed to the first declaring plugin.
    let mut conflict_pairs = BTreeSet::new();
    for manifest in &nodes {
        for other in &manifest.load.conflicts {
            if position.contains_key(other.as_str()) {
                let pair = ordered_pair(manifest.id(), other);
                if conflict_pairs.insert(pair) {
                    errors.push(PluginError::Conflict {
                        plugin: manifest.id().to_owned(),
                        other: other.clone(),
                    });
                }
            }
        }
    }

    let order = match toposort(&nodes, &position) {
        Ok(order) => order,
        Err(cycles) => {
            errors.extend(cycles.into_iter().map(|cycle| PluginError::Cycle { cycle }));
            Vec::new()
        }
    };

    if errors.is_empty() {
        Ok(PluginPlan {
            plugins: order.into_iter().map(|m| (*m).clone()).collect(),
        })
    } else {
        Err(PluginErrors(errors))
    }
}

fn ordered_pair<'a>(a: &'a str, b: &'a str) -> (&'a str, &'a str) {
    if a <= b { (a, b) } else { (b, a) }
}

type Key<'a> = (usize, &'a str);

/// Kahn's algorithm with a `(enabled position, id)` priority queue. On failure, returns every
/// cycle found among the unsorted plugins (each starting at its smallest key).
fn toposort<'a>(
    nodes: &[&'a PluginManifest],
    position: &BTreeMap<&'a str, usize>,
) -> Result<Vec<&'a PluginManifest>, Vec<Vec<String>>> {
    let key = |id: &'a str| -> Key<'a> { (position.get(id).copied().unwrap_or(usize::MAX), id) };
    let manifest_of: BTreeMap<Key<'a>, &'a PluginManifest> =
        nodes.iter().map(|m| (key(m.id()), *m)).collect();

    // `before[v]`: enabled plugins that must load before `v`.
    let mut before: BTreeMap<Key<'a>, BTreeSet<Key<'a>>> = BTreeMap::new();
    for manifest in nodes {
        let preds: BTreeSet<Key<'a>> = manifest
            .dependencies
            .keys()
            .chain(manifest.load.after.iter())
            .filter(|id| position.contains_key(id.as_str()))
            .map(|id| key(id.as_str()))
            .collect();
        before.insert(key(manifest.id()), preds);
    }
    let mut after: BTreeMap<Key<'a>, Vec<Key<'a>>> = BTreeMap::new();
    for (node, preds) in &before {
        for pred in preds {
            after.entry(*pred).or_default().push(*node);
        }
    }

    let mut pending: BTreeMap<Key<'a>, usize> =
        before.iter().map(|(k, preds)| (*k, preds.len())).collect();
    let mut ready: BTreeSet<Key<'a>> = pending
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(k, _)| *k)
        .collect();
    let mut order = Vec::with_capacity(nodes.len());
    while let Some(next) = ready.pop_first() {
        pending.remove(&next);
        if let Some(manifest) = manifest_of.get(&next) {
            order.push(*manifest);
        }
        for succ in after.get(&next).into_iter().flatten() {
            if let Some(n) = pending.get_mut(succ) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    ready.insert(*succ);
                }
            }
        }
    }
    if pending.is_empty() {
        return Ok(order);
    }

    // Every unsorted plugin has an unsorted predecessor, so walking predecessors from any of
    // them must reach a cycle.
    let mut cycles = Vec::new();
    let mut visited: BTreeSet<Key<'a>> = BTreeSet::new();
    for start in pending.keys() {
        if visited.contains(start) {
            continue;
        }
        let mut path: Vec<Key<'a>> = Vec::new();
        let mut current = *start;
        loop {
            if let Some(at) = path.iter().position(|k| *k == current) {
                cycles.push(cycle_ids(&path[at..]));
                break;
            }
            if visited.contains(&current) {
                break; // Leads into a cycle already reported.
            }
            visited.insert(current);
            path.push(current);
            let pred = before
                .get(&current)
                .and_then(|preds| preds.iter().find(|p| pending.contains_key(*p)));
            match pred {
                Some(pred) => current = *pred,
                None => break, // Unreachable: unsorted nodes always have an unsorted predecessor.
            }
        }
    }
    Err(cycles)
}

/// `path` lists a cycle walking *predecessors*; returns it in load direction (each plugin
/// must load before the next), rotated to start at its smallest key, closed with the start id.
fn cycle_ids(path: &[Key<'_>]) -> Vec<String> {
    let mut forward: Vec<Key<'_>> = path.iter().rev().copied().collect();
    if let Some(min_at) = forward
        .iter()
        .enumerate()
        .min_by_key(|(_, k)| **k)
        .map(|(i, _)| i)
    {
        forward.rotate_left(min_at);
    }
    let mut ids: Vec<String> = forward.iter().map(|(_, id)| (*id).to_owned()).collect();
    if let Some(first) = ids.first().cloned() {
        ids.push(first);
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `plugin(id, version, api, &[(dep, req)], &[after], &[conflicts])`.
    fn plugin(
        id: &str,
        version: &str,
        api: u32,
        deps: &[(&str, &str)],
        after: &[&str],
        conflicts: &[&str],
    ) -> PluginManifest {
        let list = |items: &[&str]| {
            items
                .iter()
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let deps: String = deps
            .iter()
            .map(|(d, r)| format!("\"{d}\" = \"{r}\"\n"))
            .collect();
        let src = format!(
            "[plugin]\nid = \"{id}\"\nname = \"{id}\"\nversion = \"{version}\"\napi = {api}\n\
             [dependencies]\n{deps}\
             [load]\nafter = [{}]\nconflicts = [{}]\n",
            list(after),
            list(conflicts)
        );
        PluginManifest::from_toml_str(&src).unwrap()
    }

    fn simple(id: &str) -> PluginManifest {
        plugin(id, "1.0.0", 1, &[], &[], &[])
    }

    fn enabled(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| (*s).to_owned()).collect()
    }

    fn order(available: &[PluginManifest], ids: &[&str]) -> Vec<String> {
        resolve_load_order(available, &enabled(ids), 1..=1)
            .unwrap()
            .ids()
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    fn errors(available: &[PluginManifest], ids: &[&str]) -> Vec<String> {
        resolve_load_order(available, &enabled(ids), 1..=1)
            .unwrap_err()
            .errors()
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn independent_plugins_keep_enabled_order() {
        let all = [simple("c"), simple("a"), simple("b")];
        assert_eq!(order(&all, &["b", "c", "a"]), ["b", "c", "a"]);
        assert_eq!(order(&all, &["a", "b", "c"]), ["a", "b", "c"]);
    }

    #[test]
    fn dependencies_load_first() {
        let all = [
            plugin("ui-kit", "1.0.0", 1, &[], &[], &[]),
            plugin(
                "resonance",
                "1.2.0",
                1,
                &[("status-pack", "^1.0")],
                &[],
                &[],
            ),
            plugin("status-pack", "1.4.2", 1, &[("ui-kit", "1")], &[], &[]),
        ];
        assert_eq!(
            order(&all, &["resonance", "status-pack", "ui-kit"]),
            ["ui-kit", "status-pack", "resonance"]
        );
    }

    #[test]
    fn load_after_orders_only_enabled_plugins() {
        let all = [
            plugin("a", "1.0.0", 1, &[], &["b", "not-installed"], &[]),
            simple("b"),
            simple("c"),
        ];
        assert_eq!(order(&all, &["a", "b"]), ["b", "a"]);
        // `b` not enabled: the hint is ignored rather than an error.
        assert_eq!(order(&all, &["a", "c"]), ["a", "c"]);
    }

    #[test]
    fn ties_break_by_enabled_order_then_id() {
        // `d` depends on both `b` and `a`; once `z` is placed, `a` and `b` are tied and the
        // one earlier in `enabled` wins, regardless of id or manifest order.
        let all = [
            plugin("d", "1.0.0", 1, &[("a", "1"), ("b", "1")], &[], &[]),
            simple("b"),
            simple("a"),
            simple("z"),
        ];
        assert_eq!(order(&all, &["z", "d", "b", "a"]), ["z", "b", "a", "d"]);
        assert_eq!(order(&all, &["d", "a", "b", "z"]), ["a", "b", "d", "z"]);
    }

    #[test]
    fn same_set_always_yields_the_same_order() {
        let mut all = vec![
            plugin("e", "1.0.0", 1, &[("c", "1")], &["a"], &[]),
            plugin("c", "1.0.0", 1, &[], &["b"], &[]),
            simple("a"),
            simple("b"),
            plugin("d", "1.0.0", 1, &[("a", "1")], &[], &[]),
        ];
        let ids = ["e", "d", "c", "b", "a"];
        let expected = order(&all, &ids);
        assert_eq!(expected, ["b", "c", "a", "e", "d"]);
        for _ in 0..all.len() {
            all.rotate_left(1);
            assert_eq!(order(&all, &ids), expected);
        }
    }

    #[test]
    fn id_breaks_ties_between_plugins_outside_enabled() {
        let a = simple("a");
        let b = simple("b");
        let position = BTreeMap::new();
        let sorted = toposort(&[&b, &a], &position).unwrap();
        let ids: Vec<&str> = sorted.iter().map(|m| m.id()).collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn dependency_cycle_is_an_error() {
        let all = [
            plugin("a", "1.0.0", 1, &[("c", "1")], &[], &[]),
            plugin("b", "1.0.0", 1, &[("a", "1")], &[], &[]),
            plugin("c", "1.0.0", 1, &[("b", "1")], &[], &[]),
            plugin("x", "1.0.0", 1, &[("a", "1")], &[], &[]),
            simple("ok"),
        ];
        assert_eq!(
            errors(&all, &["ok", "x", "a", "b", "c"]),
            ["plugin load order has a cycle: a -> b -> c -> a"]
        );
    }

    #[test]
    fn load_after_can_close_a_cycle() {
        let all = [
            plugin("a", "1.0.0", 1, &[("b", "1")], &[], &[]),
            plugin("b", "1.0.0", 1, &[], &["a"], &[]),
            plugin("p", "1.0.0", 1, &[], &["q"], &[]),
            plugin("q", "1.0.0", 1, &[], &["p"], &[]),
        ];
        assert_eq!(
            errors(&all, &["a", "b", "q", "p"]),
            [
                "plugin load order has a cycle: a -> b -> a",
                "plugin load order has a cycle: q -> p -> q",
            ]
        );
    }

    #[test]
    fn conflicts_are_reported_once_per_pair() {
        let all = [
            plugin("overdrive", "1.0.0", 1, &[], &[], &["resonance"]),
            plugin("resonance", "1.0.0", 1, &[], &[], &["overdrive", "absent"]),
        ];
        assert_eq!(
            errors(&all, &["resonance", "overdrive"]),
            ["plugin `resonance` conflicts with `overdrive`; they cannot both be enabled"]
        );
        // Conflicting with a plugin that is not enabled is fine.
        assert_eq!(order(&all, &["resonance"]), ["resonance"]);
    }

    #[test]
    fn missing_plugins_and_dependencies() {
        let all = [
            plugin("a", "1.0.0", 1, &[("ghost", "^1.0")], &[], &[]),
            plugin("b", "1.0.0", 1, &[("c", "1")], &[], &[]),
            simple("c"),
        ];
        assert_eq!(
            errors(&all, &["a", "b", "nowhere"]),
            [
                "`plugins.enabled[2]`: plugin `nowhere` is enabled but `plugins/nowhere/plugin.toml` does not exist",
                "plugin `a` depends on `ghost` (^1.0), which is not installed",
                "plugin `b` depends on `c`, which is installed but not enabled in pkrs.toml",
            ]
        );
    }

    #[test]
    fn semver_ranges_are_checked() {
        let lib = |v: &str| plugin("lib", v, 1, &[], &[], &[]);
        let user = |req: &str| plugin("user", "1.0.0", 1, &[("lib", req)], &[], &[]);
        let ok = |v: &str, req: &str| {
            resolve_load_order(&[lib(v), user(req)], &enabled(&["user", "lib"]), 1..=1).is_ok()
        };
        assert!(ok("1.4.2", "^1.0"));
        assert!(ok("1.0.0", "1"));
        assert!(!ok("2.0.0", "^1.0"));
        assert!(!ok("0.9.9", "^1.0"));
        assert!(ok("0.3.7", "0.3"));
        assert!(!ok("0.4.0", "0.3"));
        assert!(ok("1.4.9", ">=1.2, <1.5"));
        assert!(!ok("1.5.0", ">=1.2, <1.5"));
        assert_eq!(
            errors(&[lib("2.1.0"), user("^1.0")], &["user", "lib"]),
            ["plugin `user` requires `lib` ^1.0, but version 2.1.0 is installed"]
        );
    }

    #[test]
    fn api_levels_must_be_supported() {
        let all = [
            plugin("old", "1.0.0", 1, &[], &[], &[]),
            plugin("new", "1.0.0", 3, &[], &[], &[]),
        ];
        assert_eq!(
            errors(&all, &["old", "new"]),
            ["plugin `new` targets API level 3, but this engine supports 1..=1"]
        );
        let plan = resolve_load_order(&all, &enabled(&["old", "new"]), 1..=3).unwrap();
        assert_eq!(plan.ids(), ["old", "new"]);
        let err = resolve_load_order(&all, &enabled(&["old"]), 2..=3).unwrap_err();
        assert!(matches!(
            err.errors(),
            [PluginError::UnsupportedApi {
                api: 1,
                min: 2,
                max: 3,
                ..
            }]
        ));
    }

    #[test]
    fn duplicate_manifests_and_every_error_at_once() {
        let all = [
            simple("a"),
            simple("a"),
            plugin("b", "1.0.0", 9, &[("a", "^2")], &[], &["c"]),
            simple("c"),
        ];
        let err = resolve_load_order(&all, &enabled(&["a", "b", "c"]), 1..=1).unwrap_err();
        assert_eq!(err.errors().len(), 4, "{err}");
        assert!(
            err.to_string()
                .starts_with("plugins cannot be loaded:\n  - ")
        );
    }

    #[test]
    fn nothing_enabled_is_an_empty_plan() {
        let plan = resolve_load_order(&[simple("a")], &[], 1..=1).unwrap();
        assert!(plan.is_empty());
    }
}

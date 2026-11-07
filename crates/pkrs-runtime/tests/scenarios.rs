//! Integration (§15.2): runs every `tests/*.lua` scenario of every fixture project under
//! `tests/fixtures/projects/` headless, with scripted input, and fails listing each failure.
//! The acceptance scenario "talk to NPC -> dialogue -> choice -> back to overworld" also runs
//! as its own test.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use pkrs_runtime::project::{Project, ProjectOptions};
use pkrs_runtime::test_harness::{
    ScenarioOptions, ScenarioOutcome, discover_scenarios, run_project_scenarios, run_scenario,
    run_scenario_source,
};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/projects")
}

fn town() -> PathBuf {
    fixture_root().join("scenario-town")
}

fn open(dir: &Path) -> Project {
    Project::open(&ProjectOptions {
        dir: dir.to_path_buf(),
        ..ProjectOptions::default()
    })
    .unwrap()
}

/// Fixture projects that ship scenarios.
fn projects_with_scenarios() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(fixture_root())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| !discover_scenarios(p).unwrap().is_empty())
        .collect();
    dirs.sort();
    dirs
}

#[test]
fn every_fixture_scenario_passes() {
    let projects = projects_with_scenarios();
    assert!(projects.contains(&town()), "{projects:?}");
    let mut ran = 0;
    let mut failures = Vec::new();
    for dir in projects {
        let reports =
            run_project_scenarios(&open(&dir), &dir, &ScenarioOptions::default()).unwrap();
        for report in reports {
            println!("{}: {report}", dir.display());
            ran += 1;
            if !report.passed() {
                failures.push(format!("{}: {report}", dir.display()));
            }
        }
    }
    assert!(ran >= 4, "expected the scenario-town suite, ran {ran}");
    assert!(
        failures.is_empty(),
        "{} scenario(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn acceptance_talk_to_npc_dialogue_choice_back_to_overworld() {
    let dir = town();
    let report = run_scenario(
        &open(&dir),
        &dir.join("tests/talk_to_npc.lua"),
        "tests/talk_to_npc.lua",
        &ScenarioOptions::default(),
    )
    .unwrap();
    assert!(report.passed(), "{report}");
}

fn battle_town() -> PathBuf {
    fixture_root().join("battle-town")
}

#[test]
fn acceptance_trainer_battle_win_and_back_to_overworld() {
    let dir = battle_town();
    let run = || {
        run_scenario(
            &open(&dir),
            &dir.join("tests/trainer_battle.lua"),
            "tests/trainer_battle.lua",
            &ScenarioOptions::default(),
        )
        .unwrap()
    };
    let (a, b) = (run(), run());
    assert!(a.passed(), "{a}");
    assert_eq!(a, b, "battle scenarios replay identically");
}

#[test]
fn battle_state_and_queries_are_visible_to_scenarios() {
    let source = r#"
        walk("up", 2)
        press("confirm")
        advance_dialogue()
        wait_for_state("Battle")
        expect_state("Battle")
        expect(battle().menu == nil, "events play back before the first menu")
        advance_battle()
        expect_eq(battle().opponent.hp, battle().opponent.max_hp)
        choose(1)
        choose(1)
        expect_dialogue("Emberkit used Ember!")
        local full = battle().opponent.max_hp
        repeat
          if dialogue() ~= nil then press("confirm") else wait_ticks(1) end
        until battle().opponent.hp < full
        expect(dialogue() == nil, "the HP bar animates without a text box")
        local first = battle().opponent.hp
        wait_ticks(1)
        expect(battle().opponent.hp < first, "the bar keeps moving tick by tick")
    "#;
    let report = run_scenario_source(
        &open(&battle_town()),
        "tests/x.lua",
        source,
        &ScenarioOptions::default(),
    )
    .unwrap();
    assert!(report.passed(), "{report}");
}

#[test]
fn scenario_runs_are_deterministic() {
    let dir = town();
    let run = || {
        run_scenario(
            &open(&dir),
            &dir.join("tests/warp.lua"),
            "tests/warp.lua",
            &ScenarioOptions::default(),
        )
        .unwrap()
    };
    let (a, b) = (run(), run());
    assert!(a.passed(), "{a}");
    assert_eq!(a, b);
}

#[test]
fn failed_expectation_reports_location_tick_and_values() {
    let source = "expect_state('Overworld')\npress('confirm')\nexpect_state('Dialogue')\n";
    let report = run_scenario_source(
        &open(&town()),
        "tests/x.lua",
        source,
        &ScenarioOptions::default(),
    )
    .unwrap();
    let failure = report.failure().expect("must fail");
    assert_eq!(failure.check, "expect_state");
    assert_eq!(
        failure.location.as_ref().unwrap().to_string(),
        "tests/x.lua:3"
    );
    assert_eq!(failure.tick, 2);
    assert_eq!(failure.expected.as_deref(), Some("Dialogue"));
    assert_eq!(failure.actual.as_deref(), Some("Overworld"));
    assert!(
        report.to_string().starts_with(
            "FAIL tests/x.lua (2 ticks)\n  tests/x.lua:3: expect_state failed at tick 2"
        ),
        "{report}"
    );
}

#[test]
fn tick_limit_and_load_errors_fail() {
    let options = ScenarioOptions {
        max_ticks: 10,
        ..ScenarioOptions::default()
    };
    let report =
        run_scenario_source(&open(&town()), "tests/x.lua", "wait_ticks(11)", &options).unwrap();
    assert_eq!(report.ticks, 10);
    assert!(matches!(&report.outcome, ScenarioOutcome::Failed(f) if f.check == "tick limit"));

    let report = run_scenario_source(
        &open(&town()),
        "tests/x.lua",
        "expect(",
        &ScenarioOptions::default(),
    )
    .unwrap();
    assert!(matches!(&report.outcome, ScenarioOutcome::Failed(f) if f.check == "load"));
}

fn grass_route() -> PathBuf {
    fixture_root().join("grass-route")
}

#[test]
fn acceptance_wild_encounter_flee_and_back_to_overworld() {
    let dir = grass_route();
    let run = || {
        run_scenario(
            &open(&dir),
            &dir.join("tests/wild_encounter.lua"),
            "tests/wild_encounter.lua",
            &ScenarioOptions::default(),
        )
        .unwrap()
    };
    let (a, b) = (run(), run());
    assert!(a.passed(), "{a}");
    assert_eq!(a, b, "encounter scenarios replay identically");
}

#[test]
fn the_step_that_starts_an_encounter_depends_only_on_the_seed() {
    // Pace in the grass until a battle starts, then fail on purpose: the failure reports
    // the tick, which pins the step the encounter happened on.
    let source = r#"
        walk("up", 3)
        local dir = "left"
        while state() ~= "Battle" do
          walk(dir)
          local x = player()
          if x <= 3 then dir = "right" elseif x >= 8 then dir = "left" end
        end
        local x = player()
        fail("encounter at x = " .. x)
    "#;
    let ticks = |seed| {
        let options = ScenarioOptions {
            seed,
            ..ScenarioOptions::default()
        };
        let report =
            run_scenario_source(&open(&grass_route()), "tests/x.lua", source, &options).unwrap();
        let failure = report.failure().expect("the scenario fails on purpose");
        assert_eq!(failure.check, "fail", "{report}");
        (failure.tick, failure.to_string())
    };
    assert_eq!(ticks(1), ticks(1));
    let seeds: Vec<_> = (1..=4).map(ticks).collect();
    assert!(
        seeds.iter().any(|t| t.0 != seeds[0].0),
        "different seeds should meet the first creature on different steps: {seeds:?}"
    );
}

#[test]
fn walking_into_a_wall_fails_walk_with_positions() {
    // The guide stands at (5, 4): walking up three tiles stops at (5, 5).
    let report = run_scenario_source(
        &open(&town()),
        "tests/x.lua",
        "walk('up', 3)",
        &ScenarioOptions::default(),
    )
    .unwrap();
    let failure = report.failure().expect("must fail");
    assert_eq!(failure.check, "walk");
    assert_eq!(failure.expected.as_deref(), Some("player at (5, 4)"));
    assert!(
        failure.actual.as_deref().unwrap().starts_with("(5, 5)"),
        "{failure}"
    );
}

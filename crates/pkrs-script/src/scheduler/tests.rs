//! Scheduler tests (§9.3, §15.2): wait resolution, ordering, cancellation, error isolation.

use pkrs_core::Pcg64Rng;

use super::*;
use crate::host::{MemoryScripts, ScriptLimits};
use crate::lua::LuaScriptHost;

const SCRIPT: &str = r#"
log = {}
local function note(s) log[#log + 1] = s end
function log_text() return table.concat(log, ",") end

function wait_and_report(n)
  local start = time.ticks()
  local resumed = time.wait(n)
  return start .. ":" .. resumed .. ":" .. time.ticks()
end

function tagged(tag, n)
  wait(n)
  note(tag)
  wait(n)
  note(tag .. "2")
end

function fail_after(n)
  wait(n)
  error("boom")
end

function fail_now() error("early") end

function instant() return "done" end

function dialogue()
  local answer = coroutine.yield({ wait = "dialogue" })
  return "chose:" .. tostring(answer)
end

function walk(npc)
  coroutine.yield({ wait = "path", npc = npc })
  return npc .. " arrived"
end

function custom()
  local v = coroutine.yield({ wait = "door" })
  return v
end

function fight()
  local r = battle.start_wild("sparkit", 5)
  return r.outcome
end

function bad_wait(n) time.wait(n) end
"#;

fn host() -> LuaScriptHost {
    let mut host = LuaScriptHost::new(
        Box::new(MemoryScripts::new()),
        Box::new(Pcg64Rng::from_seed(1)),
        ScriptLimits::default(),
    )
    .unwrap();
    host.run_source(&ScriptOwner::Game, "scripts/test.lua", SCRIPT)
        .unwrap();
    host
}

fn spawn(
    s: &mut Scheduler,
    h: &mut LuaScriptHost,
    name: &str,
    args: &[ScriptValue],
    kind: TaskKind,
) -> TaskId {
    s.spawn(h, &ScriptOwner::Game, name, args, kind).unwrap().0
}

fn log(h: &mut LuaScriptHost) -> String {
    h.call_function(&ScriptOwner::Game, "log_text", &[])
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

fn finished(report: &StepReport) -> Vec<(TaskId, ScriptValue)> {
    report
        .outcomes
        .iter()
        .filter_map(|(id, o)| match o {
            TaskOutcome::Finished(v) => Some((*id, v.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn wait_30_resumes_after_exactly_30_ticks() {
    let (mut s, mut h) = (Scheduler::new(), host());
    let (id, outcome) = s
        .spawn(
            &mut h,
            &ScriptOwner::Game,
            "wait_and_report",
            &[ScriptValue::Int(30)],
            TaskKind::Background,
        )
        .unwrap();
    assert!(matches!(
        outcome,
        TaskOutcome::Waiting(WaitCondition::Ticks { resume_at: 30 })
    ));
    for tick in 1..30 {
        let report = s.step(&mut h);
        assert_eq!(report.tick, tick);
        assert!(report.outcomes.is_empty(), "resumed early at tick {tick}");
    }
    let report = s.step(&mut h);
    assert_eq!(report.tick, 30);
    assert_eq!(finished(&report), [(id, ScriptValue::from("0:30:30"))]);
    assert!(s.is_empty());
}

#[test]
fn waits_are_relative_to_the_current_tick() {
    let (mut s, mut h) = (Scheduler::new(), host());
    for _ in 0..7 {
        s.step(&mut h);
    }
    spawn(
        &mut s,
        &mut h,
        "wait_and_report",
        &[ScriptValue::Int(3)],
        TaskKind::Background,
    );
    let reports: Vec<StepReport> = (0..3).map(|_| s.step(&mut h)).collect();
    assert!(reports[0].outcomes.is_empty() && reports[1].outcomes.is_empty());
    assert_eq!(finished(&reports[2])[0].1, ScriptValue::from("7:10:10"));
}

#[test]
fn wait_zero_and_default_resume_next_tick() {
    let (mut s, mut h) = (Scheduler::new(), host());
    spawn(
        &mut s,
        &mut h,
        "wait_and_report",
        &[ScriptValue::Int(0)],
        TaskKind::Background,
    );
    spawn(&mut s, &mut h, "wait_and_report", &[], TaskKind::Background);
    let report = s.step(&mut h);
    assert_eq!(finished(&report).len(), 2);
}

#[test]
fn concurrent_tasks_resume_in_creation_order() {
    let (mut s, mut h) = (Scheduler::new(), host());
    // Created C, A, B: same wait, so they must run in exactly that order every time.
    for tag in ["C", "A", "B"] {
        spawn(
            &mut s,
            &mut h,
            "tagged",
            &[ScriptValue::from(tag), ScriptValue::Int(2)],
            TaskKind::Background,
        );
    }
    // A shorter-wait task created last runs first only because its wait ends earlier.
    spawn(
        &mut s,
        &mut h,
        "tagged",
        &[ScriptValue::from("Z"), ScriptValue::Int(1)],
        TaskKind::Background,
    );
    for _ in 0..4 {
        s.step(&mut h);
    }
    assert_eq!(log(&mut h), "Z,C,A,B,Z2,C2,A2,B2");
    assert!(s.is_empty());
}

#[test]
fn interact_and_trigger_lock_the_player_until_they_finish() {
    let (mut s, mut h) = (Scheduler::new(), host());
    spawn(
        &mut s,
        &mut h,
        "wait_and_report",
        &[ScriptValue::Int(5)],
        TaskKind::Background,
    );
    assert!(!s.player_locked(), "background scripts never lock");
    spawn(
        &mut s,
        &mut h,
        "wait_and_report",
        &[ScriptValue::Int(2)],
        TaskKind::Interact,
    );
    spawn(
        &mut s,
        &mut h,
        "wait_and_report",
        &[ScriptValue::Int(3)],
        TaskKind::Trigger,
    );
    assert_eq!(s.player_lock().holder_count(), 2);
    s.step(&mut h);
    s.step(&mut h);
    assert!(s.player_locked(), "the trigger script is still running");
    s.step(&mut h);
    assert!(!s.player_locked());
    assert_eq!(s.len(), 1);
}

#[test]
fn instant_tasks_never_lock() {
    let (mut s, mut h) = (Scheduler::new(), host());
    let (_, outcome) = s
        .spawn(
            &mut h,
            &ScriptOwner::Game,
            "instant",
            &[],
            TaskKind::Interact,
        )
        .unwrap();
    assert!(matches!(outcome, TaskOutcome::Finished(ScriptValue::String(ref s)) if s == "done"));
    assert!(!s.player_locked());
    assert!(s.is_empty());
}

#[test]
fn failing_task_is_aborted_unlocks_and_spares_the_others() {
    let (mut s, mut h) = (Scheduler::new(), host());
    let bad = spawn(
        &mut s,
        &mut h,
        "fail_after",
        &[ScriptValue::Int(1)],
        TaskKind::Interact,
    );
    let good = spawn(
        &mut s,
        &mut h,
        "tagged",
        &[ScriptValue::from("ok"), ScriptValue::Int(1)],
        TaskKind::Background,
    );
    assert!(s.player_locked());
    let report = s.step(&mut h);
    assert_eq!(report.failures(), 1);
    match &report.outcomes[0] {
        (id, TaskOutcome::Failed(err)) => {
            assert_eq!(*id, bad);
            assert!(err.to_string().contains("boom"), "{err}");
        }
        other => panic!("expected failure, got {other:?}"),
    }
    assert!(matches!(report.outcomes[1], (id, TaskOutcome::Waiting(_)) if id == good));
    assert!(!s.player_locked(), "a failed cutscene unlocks the player");
    assert_eq!(h.task_count(), 1);
    s.step(&mut h);
    assert_eq!(log(&mut h), "ok,ok2");
}

#[test]
fn failing_spawn_returns_error_and_leaves_player_unlocked() {
    let (mut s, mut h) = (Scheduler::new(), host());
    let err = s
        .spawn(
            &mut h,
            &ScriptOwner::Game,
            "fail_now",
            &[],
            TaskKind::Trigger,
        )
        .unwrap_err();
    assert!(err.to_string().contains("early"));
    assert!(!s.player_locked());
    assert!(
        s.spawn(&mut h, &ScriptOwner::Game, "nope", &[], TaskKind::Interact)
            .is_err()
    );
}

#[test]
fn bad_wait_arguments_abort_the_task() {
    let (mut s, mut h) = (Scheduler::new(), host());
    for arg in [
        ScriptValue::from("x"),
        ScriptValue::Int(-1),
        ScriptValue::Number(1.5),
    ] {
        assert!(
            s.spawn(
                &mut h,
                &ScriptOwner::Game,
                "bad_wait",
                &[arg],
                TaskKind::Interact
            )
            .is_err()
        );
    }
    assert!(!s.player_locked());
    let err = h
        .call_function(&ScriptOwner::Game, "bad_wait", &[ScriptValue::Int(1)])
        .unwrap_err();
    assert!(err.to_string().contains("script task"), "{err}");
}

#[test]
fn cancellation_drops_the_task_and_its_lock() {
    let (mut s, mut h) = (Scheduler::new(), host());
    let a = spawn(
        &mut s,
        &mut h,
        "tagged",
        &[ScriptValue::from("a"), ScriptValue::Int(1)],
        TaskKind::Interact,
    );
    let b = spawn(
        &mut s,
        &mut h,
        "tagged",
        &[ScriptValue::from("b"), ScriptValue::Int(1)],
        TaskKind::Background,
    );
    assert!(s.cancel(&mut h, a));
    assert!(!s.cancel(&mut h, a), "already cancelled");
    assert!(!s.player_locked());
    assert_eq!(s.task_ids(), [b]);
    assert_eq!(h.task_count(), 1);
    s.step(&mut h);
    assert_eq!(log(&mut h), "b");
    s.cancel_all(&mut h);
    assert!(s.is_empty());
    assert_eq!(h.task_count(), 0);
    s.step(&mut h);
    assert_eq!(log(&mut h), "b");
}

#[test]
fn signals_wake_matching_waits_on_the_next_step() {
    let (mut s, mut h) = (Scheduler::new(), host());
    let d = spawn(&mut s, &mut h, "dialogue", &[], TaskKind::Interact);
    let p1 = spawn(
        &mut s,
        &mut h,
        "walk",
        &[ScriptValue::from("keeper")],
        TaskKind::Background,
    );
    let p2 = spawn(
        &mut s,
        &mut h,
        "walk",
        &[ScriptValue::from("guard")],
        TaskKind::Background,
    );
    assert_eq!(s.wait_of(d), Some(&WaitCondition::DialogueClosed));
    assert!(s.step(&mut h).outcomes.is_empty(), "nothing signalled yet");

    assert_eq!(s.signal(WakeSignal::DialogueClosed(ScriptValue::Int(2))), 1);
    assert_eq!(
        s.signal(WakeSignal::PathFinished {
            npc: "guard".into()
        }),
        1
    );
    assert!(
        s.player_locked(),
        "signals only take effect in the next step"
    );
    let report = s.step(&mut h);
    assert_eq!(
        finished(&report),
        [
            (d, ScriptValue::from("chose:2")),
            (p2, ScriptValue::from("guard arrived"))
        ]
    );
    assert!(!s.player_locked());
    assert_eq!(s.task_ids(), [p1]);
}

#[test]
fn battle_waits_resume_with_the_result() {
    let (mut s, mut h) = (Scheduler::new(), host());
    let (id, outcome) = s
        .spawn(&mut h, &ScriptOwner::Game, "fight", &[], TaskKind::Trigger)
        .unwrap();
    assert!(matches!(
        outcome,
        TaskOutcome::Waiting(WaitCondition::BattleEnded(BattleRequest::Wild {
            level: 5,
            ..
        }))
    ));
    let mut result = std::collections::BTreeMap::new();
    result.insert("outcome".to_string(), ScriptValue::from("win"));
    s.signal(WakeSignal::BattleEnded(ScriptValue::Map(result)));
    assert_eq!(finished(&s.step(&mut h)), [(id, ScriptValue::from("win"))]);
}

#[test]
fn external_waits_resume_through_wake() {
    let (mut s, mut h) = (Scheduler::new(), host());
    let id = spawn(&mut s, &mut h, "custom", &[], TaskKind::Background);
    assert!(matches!(s.wait_of(id), Some(WaitCondition::External(_))));
    assert_eq!(s.describe(id), Some(("custom", TaskKind::Background)));
    assert!(s.step(&mut h).outcomes.is_empty());
    assert!(s.wake(id, ScriptValue::Int(9)));
    assert!(!s.wake(999, ScriptValue::Nil));
    assert_eq!(finished(&s.step(&mut h)), [(id, ScriptValue::Int(9))]);
}

#[test]
fn time_ticks_outside_tasks_reports_the_scheduler_tick() {
    let (mut s, mut h) = (Scheduler::new(), host());
    for _ in 0..4 {
        s.step(&mut h);
    }
    assert_eq!(
        h.run_source(&ScriptOwner::Game, "scripts/t.lua", "return time.ticks()")
            .unwrap(),
        ScriptValue::Int(4)
    );
    assert_eq!(s.tick(), 4);
}

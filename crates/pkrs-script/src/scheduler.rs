//! Coroutine scheduler for event handlers and cutscenes (§9.3, §3.3 step 2).
//!
//! Script tasks ([`ScriptHost::start_task`]) yield a [`WaitRequest`]; the [`Scheduler`] turns it
//! into a [`WaitCondition`] and resumes the task once that condition is met:
//!
//! - **Ticks** (`time.wait(n)` / `wait(n)`): a wait issued at tick `T` resumes in the
//!   [`step`](Scheduler::step) that reaches tick `T + n` (`n = 0` means the next tick). The task
//!   is resumed with the current tick.
//! - **Dialogue closed, path finished, battle ended**: the engine reports these with
//!   [`Scheduler::signal`]; matching tasks resume, with the signal's answer, in the next step.
//! - **Anything else** yielded by a script waits until [`Scheduler::wake`] names it.
//!
//! Every step resumes ready tasks in **creation order**, so concurrent coroutines are
//! deterministic. `interact` and `trigger` tasks lock the player ([`PlayerLock`]) until they
//! finish, fail or are cancelled. A failing task is aborted and logged; it never stops other
//! tasks or the engine (§9.6).

use pkrs_world::PlayerLock;

use crate::error::{Result, ScriptError};
use crate::host::{
    BattleRequest, ScriptHost, ScriptOwner, ScriptValue, TaskId, TaskStatus, WaitRequest,
};

/// Why a task was started; decides whether it locks the player (§9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    /// `interact` handler (NPC, sign, tile): locks the player while it runs.
    Interact,
    /// Trigger-zone script (`trigger_enter`): locks the player while it runs.
    Trigger,
    /// Background script (NPC wander, ambient effects): never locks the player.
    Background,
}

impl TaskKind {
    /// `true` for [`Interact`](Self::Interact) and [`Trigger`](Self::Trigger).
    #[must_use]
    pub const fn locks_player(self) -> bool {
        matches!(self, Self::Interact | Self::Trigger)
    }
}

/// What a suspended task waits for.
#[derive(Clone, Debug, PartialEq)]
pub enum WaitCondition {
    /// Resume in the step that reaches this tick.
    Ticks {
        /// Absolute tick (see [`Scheduler::tick`]).
        resume_at: u64,
    },
    /// Resume after [`WakeSignal::DialogueClosed`].
    DialogueClosed,
    /// Resume after [`WakeSignal::PathFinished`] for `npc`.
    PathFinished {
        /// NPC id.
        npc: String,
    },
    /// Resume after [`WakeSignal::BattleEnded`]; the engine should start `0`.
    BattleEnded(BattleRequest),
    /// Until [`WakeSignal::UiClosed`] for this [`WaitRequest::PushUiScreen`].
    UiPushScreen {
        /// Same id as registration.
        screen: String,
        /// Args from `ui.push_screen` (for debugging).
        args: ScriptValue,
    },
    /// A script-defined request; resume with [`Scheduler::wake`].
    External(ScriptValue),
}

/// An engine event that may satisfy waiting tasks.
#[derive(Clone, Debug, PartialEq)]
pub enum WakeSignal {
    /// The dialogue box closed; `answer` is handed to the task (e.g. the chosen option).
    DialogueClosed(ScriptValue),
    /// NPC `npc` finished its scripted path.
    PathFinished {
        /// NPC id.
        npc: String,
    },
    /// The requested battle ended; resume with [`battle_result_value`](crate::battle_result_value).
    BattleEnded(ScriptValue),
    /// Lua UI screen `screen` was popped; resume the `ui.push_screen` waiter with `result`.
    UiClosed {
        /// Waiting task ([`TaskId`]).
        task: TaskId,
        /// Screen id.
        screen: String,
        /// First return value from `update`, or nil.
        result: ScriptValue,
    },
}

/// What happened to a task when it ran.
#[derive(Debug)]
pub enum TaskOutcome {
    /// Suspended again.
    Waiting(WaitCondition),
    /// Returned its first value; the task is gone.
    Finished(ScriptValue),
    /// Raised an error; the task was aborted (and logged).
    Failed(ScriptError),
}

/// Result of one [`Scheduler::step`].
#[derive(Debug, Default)]
pub struct StepReport {
    /// The tick this step ran for.
    pub tick: u64,
    /// Every task that ran, in the order it ran (creation order).
    pub outcomes: Vec<(TaskId, TaskOutcome)>,
}

impl StepReport {
    /// Number of tasks aborted by an error in this step.
    #[must_use]
    pub fn failures(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|(_, o)| matches!(o, TaskOutcome::Failed(_)))
            .count()
    }
}

/// A Lua UI screen the engine should push this tick (§9.5).
#[derive(Clone, Debug, PartialEq)]
pub struct PendingUiPush {
    /// Script owner that registered the screen.
    pub owner: ScriptOwner,
    /// `ui.register_screen` id.
    pub screen: String,
    /// Second argument to `ui.push_screen`.
    pub args: ScriptValue,
    /// Task waiting for pop (for [`WakeSignal::UiClosed`] routing).
    pub task: TaskId,
}

#[derive(Debug)]
struct Entry {
    id: TaskId,
    owner: ScriptOwner,
    name: String,
    kind: TaskKind,
    wait: WaitCondition,
    /// Set when a signal satisfied `wait`; consumed on resume.
    answer: Option<ScriptValue>,
}

impl Entry {
    fn take_ready(&mut self, tick: u64) -> Option<ScriptValue> {
        match self.wait {
            WaitCondition::Ticks { resume_at } if resume_at <= tick => {
                Some(ScriptValue::Int(i64::try_from(tick).unwrap_or(i64::MAX)))
            }
            WaitCondition::Ticks { .. } => None,
            _ => self.answer.take(),
        }
    }
}

/// Runs script tasks as coroutines resumed when their [`WaitCondition`] is met (§9.3).
///
/// Call [`step`](Self::step) once per simulation tick, in the scripts phase (§3.3 step 2).
#[derive(Debug, Default)]
pub struct Scheduler {
    tick: u64,
    tasks: Vec<Entry>,
    lock: PlayerLock,
    /// UI opens requested by tasks that yielded `ui.push_screen` (consumed by the runtime).
    pending_ui_pushes: Vec<PendingUiPush>,
}

impl Scheduler {
    /// Empty scheduler at tick 0.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes UI push requests queued by script tasks this tick (then clears the queue).
    pub fn drain_pending_ui_pushes(&mut self) -> Vec<PendingUiPush> {
        std::mem::take(&mut self.pending_ui_pushes)
    }

    /// Ticks stepped so far (`time.ticks()`).
    #[must_use]
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// The lock held by running interact/trigger tasks; gate player movement with it.
    #[must_use]
    pub fn player_lock(&self) -> &PlayerLock {
        &self.lock
    }

    /// `true` while an interact or trigger task is running.
    #[must_use]
    pub fn player_locked(&self) -> bool {
        self.lock.is_locked()
    }

    /// Number of suspended tasks.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// `true` when no task is suspended.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Suspended tasks in creation (= resume) order.
    #[must_use]
    pub fn task_ids(&self) -> Vec<TaskId> {
        self.tasks.iter().map(|e| e.id).collect()
    }

    /// What `task` waits for, if it is suspended.
    #[must_use]
    pub fn wait_of(&self, task: TaskId) -> Option<&WaitCondition> {
        self.entry(task).map(|e| &e.wait)
    }

    /// Function name and kind `task` was started with.
    #[must_use]
    pub fn describe(&self, task: TaskId) -> Option<(&str, TaskKind)> {
        self.entry(task).map(|e| (e.name.as_str(), e.kind))
    }

    /// Starts `owner`'s function `name` as a task and runs it until its first wait.
    ///
    /// Returns [`TaskOutcome::Waiting`] or [`TaskOutcome::Finished`]; a task that errors before
    /// its first wait is aborted, logged and returned as `Err` (the player is not left locked).
    pub fn spawn(
        &mut self,
        host: &mut (impl ScriptHost + ?Sized),
        owner: &ScriptOwner,
        name: &str,
        args: &[ScriptValue],
        kind: TaskKind,
    ) -> Result<(TaskId, TaskOutcome)> {
        host.set_tick(self.tick);
        let (id, status) = host.start_task(owner, name, args).inspect_err(|err| {
            tracing::warn!(target: "script", task = name, "script task aborted: {err}");
        })?;
        Ok((id, self.adopt(owner, name, kind, id, status)))
    }

    /// Takes over a task the host already started (e.g. an event listener,
    /// [`ScriptHost::start_listener`]): a waiting task is suspended (and locks the player for
    /// [`TaskKind::Interact`] / [`TaskKind::Trigger`]) exactly as with [`spawn`](Self::spawn).
    pub fn adopt(
        &mut self,
        owner: &ScriptOwner,
        name: &str,
        kind: TaskKind,
        id: TaskId,
        status: TaskStatus,
    ) -> TaskOutcome {
        match status {
            TaskStatus::Waiting(request) => {
                let wait = self.condition(request.clone());
                if let WaitRequest::PushUiScreen {
                    ref screen,
                    args: ref screen_args,
                } = request
                {
                    self.pending_ui_pushes.push(PendingUiPush {
                        owner: owner.clone(),
                        screen: screen.clone(),
                        args: screen_args.clone(),
                        task: id,
                    });
                }
                if kind.locks_player() {
                    self.lock.lock(u64::from(id));
                }
                self.tasks.push(Entry {
                    id,
                    owner: owner.clone(),
                    name: name.to_owned(),
                    kind,
                    wait: wait.clone(),
                    answer: None,
                });
                TaskOutcome::Waiting(wait)
            }
            TaskStatus::Finished(value) => TaskOutcome::Finished(value),
        }
    }

    /// Advances one tick and resumes every task whose condition is met, in creation order.
    pub fn step(&mut self, host: &mut (impl ScriptHost + ?Sized)) -> StepReport {
        self.tick = self.tick.saturating_add(1);
        host.set_tick(self.tick);
        let mut report = StepReport {
            tick: self.tick,
            outcomes: Vec::new(),
        };
        let mut i = 0;
        while i < self.tasks.len() {
            let Some(answer) = self.tasks[i].take_ready(self.tick) else {
                i += 1;
                continue;
            };
            let id = self.tasks[i].id;
            let entry_owner = self.tasks[i].owner.clone();
            match host.resume_task(id, answer) {
                Ok(TaskStatus::Waiting(request)) => {
                    let wait = self.condition(request.clone());
                    if let WaitRequest::PushUiScreen {
                        ref screen,
                        args: ref screen_args,
                    } = request
                    {
                        self.pending_ui_pushes.push(PendingUiPush {
                            owner: entry_owner,
                            screen: screen.clone(),
                            args: screen_args.clone(),
                            task: id,
                        });
                    }
                    self.tasks[i].wait = wait.clone();
                    report.outcomes.push((id, TaskOutcome::Waiting(wait)));
                    i += 1;
                }
                Ok(TaskStatus::Finished(value)) => {
                    self.remove_at(i);
                    report.outcomes.push((id, TaskOutcome::Finished(value)));
                }
                Err(err) => {
                    let entry = self.remove_at(i);
                    tracing::warn!(
                        target: "script",
                        task = entry.name.as_str(),
                        id,
                        "script task aborted: {err}"
                    );
                    report.outcomes.push((id, TaskOutcome::Failed(err)));
                }
            }
        }
        report
    }

    /// Satisfies every task currently waiting for `signal`; they resume in the next
    /// [`step`](Self::step). Returns how many tasks were woken.
    pub fn signal(&mut self, signal: WakeSignal) -> usize {
        let mut woken = 0;
        for entry in self.tasks.iter_mut().filter(|e| e.answer.is_none()) {
            let answer = match (&signal, &entry.wait) {
                (WakeSignal::DialogueClosed(answer), WaitCondition::DialogueClosed) => {
                    answer.clone()
                }
                (WakeSignal::PathFinished { npc }, WaitCondition::PathFinished { npc: want })
                    if npc == want =>
                {
                    ScriptValue::Nil
                }
                (WakeSignal::BattleEnded(result), WaitCondition::BattleEnded(_)) => result.clone(),
                (
                    WakeSignal::UiClosed {
                        task,
                        screen,
                        result,
                    },
                    WaitCondition::UiPushScreen {
                        screen: want,
                        args: _,
                    },
                ) if screen == want && entry.id == *task => result.clone(),
                _ => continue,
            };
            entry.answer = Some(answer);
            woken += 1;
        }
        woken
    }

    /// Resumes `task` with `answer` in the next step, whatever it waits for. Returns `false`
    /// if `task` is not suspended.
    pub fn wake(&mut self, task: TaskId, answer: ScriptValue) -> bool {
        match self.tasks.iter_mut().find(|e| e.id == task) {
            Some(entry) => {
                if let WaitCondition::Ticks { .. } = entry.wait {
                    entry.wait = WaitCondition::External(ScriptValue::Nil);
                }
                entry.answer = Some(answer);
                true
            }
            None => false,
        }
    }

    /// Aborts `task` without resuming it and releases its player lock. Returns `false` if it
    /// is not suspended.
    pub fn cancel(&mut self, host: &mut (impl ScriptHost + ?Sized), task: TaskId) -> bool {
        let Some(index) = self.tasks.iter().position(|e| e.id == task) else {
            return false;
        };
        self.remove_at(index);
        host.cancel_task(task);
        true
    }

    /// Aborts every task (e.g. on map change or game reset) and unlocks the player.
    pub fn cancel_all(&mut self, host: &mut (impl ScriptHost + ?Sized)) {
        for entry in self.tasks.drain(..) {
            host.cancel_task(entry.id);
        }
        self.lock.clear();
    }

    fn entry(&self, task: TaskId) -> Option<&Entry> {
        self.tasks.iter().find(|e| e.id == task)
    }

    fn remove_at(&mut self, index: usize) -> Entry {
        let entry = self.tasks.remove(index);
        self.lock.unlock(u64::from(entry.id));
        entry
    }

    fn condition(&self, request: WaitRequest) -> WaitCondition {
        match request {
            WaitRequest::Ticks(n) => WaitCondition::Ticks {
                resume_at: self.tick.saturating_add(u64::from(n.max(1))),
            },
            WaitRequest::DialogueClosed => WaitCondition::DialogueClosed,
            WaitRequest::PathFinished { npc } => WaitCondition::PathFinished { npc },
            WaitRequest::Battle(request) => WaitCondition::BattleEnded(request),
            WaitRequest::PushUiScreen { screen, args } => {
                WaitCondition::UiPushScreen { screen, args }
            }
            WaitRequest::Other(value) => WaitCondition::External(value),
        }
    }
}

#[cfg(test)]
mod tests;

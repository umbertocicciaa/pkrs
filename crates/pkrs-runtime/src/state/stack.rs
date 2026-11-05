//! Stack storage, deferred transitions, and opaque-aware rendering.

use pkrs_render::Renderer;

use super::{GameState, Transition};
use crate::ctx::RuntimeCtx;

/// Modal game states with §3.3 deferred transitions.
#[derive(Default)]
pub struct StateStack {
    states: Vec<Box<dyn GameState>>,
    pending: Option<Transition>,
    quit_requested: bool,
}

impl StateStack {
    /// Empty stack.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of states currently on the stack.
    #[must_use]
    pub fn len(&self) -> usize {
        self.states.len()
    }

    /// Whether the stack has no states.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }

    /// Id of the top state, if any.
    #[must_use]
    pub fn top_id(&self) -> Option<&'static str> {
        self.states.last().map(|s| s.id())
    }

    /// Whether [`Transition::Quit`] was applied.
    #[must_use]
    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    /// Pushes `state` and runs [`GameState::on_enter`].
    pub fn push(&mut self, ctx: &mut RuntimeCtx, state: Box<dyn GameState>) {
        self.states.push(state);
        if let Some(top) = self.states.last_mut() {
            top.on_enter(ctx);
        }
    }

    /// §3.3 step 3: background updates, then top [`GameState::update`]. Does not apply transitions.
    pub fn run_update(&mut self, ctx: &mut RuntimeCtx) {
        self.pending = None;
        let Some(top_index) = self.states.len().checked_sub(1) else {
            return;
        };

        for state in self.states.iter_mut().take(top_index) {
            if state.wants_background_update() {
                state.update_background(ctx);
            }
        }

        let transition = self.states[top_index].update(ctx);
        if !matches!(transition, Transition::None) {
            self.pending = Some(transition);
        }
    }

    /// §3.3 step 4: apply the transition queued by [`run_update`](Self::run_update).
    pub fn apply_pending(&mut self, ctx: &mut RuntimeCtx) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        match pending {
            Transition::None => {}
            Transition::Push(state) => self.push(ctx, state),
            Transition::Pop => self.pop(ctx),
            Transition::Replace(state) => self.replace(ctx, state),
            Transition::PopTo(id) => self.pop_to(ctx, id),
            Transition::Quit => self.quit_requested = true,
        }
    }

    /// Renders from the nearest opaque state below the top through the top (§4.1).
    pub fn render(&self, renderer: &mut Renderer, alpha: f32) {
        if self.states.is_empty() {
            return;
        }
        let start = self.states.iter().rposition(|s| s.is_opaque()).unwrap_or(0);
        for state in &self.states[start..] {
            state.render(renderer, alpha);
        }
    }

    /// Human-readable stack line (`a > b > c`) for logging and F3.
    #[must_use]
    pub fn format_debug_stack(&self) -> String {
        self.states
            .iter()
            .map(|s| s.id())
            .collect::<Vec<_>>()
            .join(" > ")
    }

    fn pop(&mut self, ctx: &mut RuntimeCtx) {
        if self.states.len() <= 1 {
            tracing::warn!("Pop ignored: cannot pop the last state on the stack");
            return;
        }
        let Some(mut top) = self.states.pop() else {
            return;
        };
        top.on_exit(ctx);
        if let Some(resumed) = self.states.last_mut() {
            resumed.on_resume(ctx);
        }
    }

    fn replace(&mut self, ctx: &mut RuntimeCtx, new_state: Box<dyn GameState>) {
        if let Some(mut old) = self.states.pop() {
            old.on_exit(ctx);
        }
        self.states.push(new_state);
        if let Some(top) = self.states.last_mut() {
            top.on_enter(ctx);
        }
    }

    fn pop_to(&mut self, ctx: &mut RuntimeCtx, target: &'static str) {
        let Some(keep) = self.states.iter().rposition(|s| s.id() == target) else {
            tracing::warn!(
                target,
                stack = %self.format_debug_stack(),
                "PopTo: no such state; ignored"
            );
            return;
        };
        if keep + 1 == self.states.len() {
            return;
        }
        for mut state in self.states.drain(keep + 1..).rev() {
            state.on_exit(ctx);
        }
        if let Some(top) = self.states.last_mut() {
            top.on_resume(ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_platform::DisplaySettings;
    use pkrs_render::{NullRenderer, Renderer, RendererConfig};

    struct RecordingState {
        id: &'static str,
        opaque: bool,
        background: bool,
        log: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
        next: Option<Transition>,
        render_tag: &'static str,
    }

    impl RecordingState {
        fn log_event(log: &std::rc::Rc<std::cell::RefCell<Vec<String>>>, msg: &str) {
            log.borrow_mut().push(msg.to_string());
        }

        fn new(id: &'static str, log: std::rc::Rc<std::cell::RefCell<Vec<String>>>) -> Self {
            Self {
                id,
                opaque: true,
                background: false,
                log,
                next: None,
                render_tag: id,
            }
        }
    }

    impl GameState for RecordingState {
        fn id(&self) -> &'static str {
            self.id
        }

        fn on_enter(&mut self, _: &mut RuntimeCtx) {
            Self::log_event(&self.log, &format!("enter:{}", self.id));
        }

        fn on_exit(&mut self, _: &mut RuntimeCtx) {
            Self::log_event(&self.log, &format!("exit:{}", self.id));
        }

        fn on_resume(&mut self, _: &mut RuntimeCtx) {
            Self::log_event(&self.log, &format!("resume:{}", self.id));
        }

        fn wants_background_update(&self) -> bool {
            self.background
        }

        fn update_background(&mut self, _: &mut RuntimeCtx) {
            Self::log_event(&self.log, &format!("bg:{}", self.id));
        }

        fn update(&mut self, _: &mut RuntimeCtx) -> Transition {
            self.next.take().unwrap_or(Transition::None)
        }

        fn render(&self, _: &mut Renderer, _: f32) {
            Self::log_event(&self.log, &format!("render:{}", self.render_tag));
        }

        fn is_opaque(&self) -> bool {
            self.opaque
        }
    }

    fn tick(stack: &mut StateStack, ctx: &mut RuntimeCtx) {
        stack.run_update(ctx);
        stack.apply_pending(ctx);
    }

    #[test]
    fn push_and_pop_call_enter_exit_and_resume() {
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut ctx = RuntimeCtx;
        let mut stack = StateStack::new();
        stack.push(&mut ctx, Box::new(RecordingState::new("root", log.clone())));

        let mut child = RecordingState::new("child", log.clone());
        child.next = Some(Transition::Pop);
        stack.push(&mut ctx, Box::new(child));
        assert_eq!(log.borrow().as_slice(), &["enter:root", "enter:child"]);

        tick(&mut stack, &mut ctx);
        assert_eq!(
            log.borrow().as_slice(),
            &["enter:root", "enter:child", "exit:child", "resume:root"]
        );
    }

    #[test]
    fn replace_swaps_top_without_resume_below() {
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut ctx = RuntimeCtx;
        let mut stack = StateStack::new();
        stack.push(&mut ctx, Box::new(RecordingState::new("root", log.clone())));
        let mut top = RecordingState::new("menu", log.clone());
        top.next = Some(Transition::Replace(Box::new(RecordingState::new(
            "party",
            log.clone(),
        ))));
        stack.push(&mut ctx, Box::new(top));
        tick(&mut stack, &mut ctx);
        assert_eq!(
            log.borrow().as_slice(),
            &["enter:root", "enter:menu", "exit:menu", "enter:party"]
        );
        assert!(!log.borrow().iter().any(|e| e == "resume:root"));
    }

    #[test]
    fn pop_to_peels_until_target_and_resumes_once() {
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut ctx = RuntimeCtx;
        let mut stack = StateStack::new();
        stack.push(&mut ctx, Box::new(RecordingState::new("root", log.clone())));
        stack.push(&mut ctx, Box::new(RecordingState::new("d", log.clone())));
        let mut driver = RecordingState::new("e", log.clone());
        driver.next = Some(Transition::PopTo("root"));
        stack.push(&mut ctx, Box::new(driver));
        tick(&mut stack, &mut ctx);
        assert_eq!(
            log.borrow().as_slice(),
            &[
                "enter:root",
                "enter:d",
                "enter:e",
                "exit:e",
                "exit:d",
                "resume:root"
            ]
        );
    }

    #[test]
    fn transitions_are_not_applied_mid_update() {
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut ctx = RuntimeCtx;
        let mut top_state = RecordingState::new("only", log.clone());
        top_state.next = Some(Transition::Push(Box::new(RecordingState::new(
            "pushed",
            log.clone(),
        ))));
        let mut stack = StateStack::new();
        stack.push(&mut ctx, Box::new(top_state));

        stack.run_update(&mut ctx);
        assert_eq!(stack.len(), 1);
        assert_eq!(stack.top_id(), Some("only"));
        stack.apply_pending(&mut ctx);
        assert_eq!(stack.len(), 2);
        assert_eq!(stack.top_id(), Some("pushed"));
    }

    #[test]
    fn background_update_only_when_opted_in() {
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut ctx = RuntimeCtx;
        let mut under = RecordingState::new("under", log.clone());
        under.background = true;
        let mut stack = StateStack::new();
        stack.push(&mut ctx, Box::new(under));
        stack.push(&mut ctx, Box::new(RecordingState::new("over", log.clone())));

        stack.run_update(&mut ctx);
        assert!(log.borrow().iter().any(|e| e == "bg:under"));
        log.borrow_mut().clear();

        let mut stack2 = StateStack::new();
        stack2.push(
            &mut ctx,
            Box::new(RecordingState::new("silent", log.clone())),
        );
        stack2.push(&mut ctx, Box::new(RecordingState::new("over", log.clone())));
        stack2.run_update(&mut ctx);
        assert!(!log.borrow().iter().any(|e| e.starts_with("bg:")));
    }

    #[test]
    fn render_walks_to_nearest_opaque_then_upward() {
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut stack = StateStack::new();
        let mut ctx = RuntimeCtx;
        let mut a = RecordingState::new("a", log.clone());
        a.opaque = true;
        let mut b = RecordingState::new("b", log.clone());
        b.opaque = true;
        let mut c = RecordingState::new("c", log.clone());
        c.opaque = false;
        stack.push(&mut ctx, Box::new(a));
        stack.push(&mut ctx, Box::new(b));
        stack.push(&mut ctx, Box::new(c));
        log.borrow_mut().clear();

        let null = NullRenderer::new();
        let mut renderer = Renderer::new(
            Box::new(null),
            DisplaySettings::default(),
            RendererConfig::default(),
        );
        renderer.begin_frame(240, 160);
        stack.render(&mut renderer, 0.0);
        renderer.end_frame();

        assert_eq!(log.borrow().as_slice(), &["render:b", "render:c"]);
    }

    #[test]
    fn debug_stack_lists_bottom_to_top() {
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut stack = StateStack::new();
        let mut ctx = RuntimeCtx;
        stack.push(
            &mut ctx,
            Box::new(RecordingState::new("title", log.clone())),
        );
        stack.push(&mut ctx, Box::new(RecordingState::new("fade", log.clone())));
        assert_eq!(stack.format_debug_stack(), "title > fade");
    }

    #[test]
    fn pop_to_unknown_id_leaves_stack_unchanged() {
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut ctx = RuntimeCtx;
        let mut stack = StateStack::new();
        stack.push(
            &mut ctx,
            Box::new(RecordingState::new("overworld", log.clone())),
        );
        stack.push(&mut ctx, Box::new(RecordingState::new("menu", log.clone())));
        let mut driver = RecordingState::new("dialog", log.clone());
        driver.next = Some(Transition::PopTo("ovreworld"));
        stack.push(&mut ctx, Box::new(driver));
        tick(&mut stack, &mut ctx);
        assert_eq!(stack.len(), 3);
        assert_eq!(stack.format_debug_stack(), "overworld > menu > dialog");
        assert!(!log.borrow().iter().any(|e| e == "exit:overworld"));
    }

    #[test]
    fn pop_to_own_id_does_not_resume() {
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut ctx = RuntimeCtx;
        let mut stack = StateStack::new();
        let mut top = RecordingState::new("a", log.clone());
        top.next = Some(Transition::PopTo("a"));
        stack.push(&mut ctx, Box::new(top));
        tick(&mut stack, &mut ctx);
        assert_eq!(stack.len(), 1);
        assert!(!log.borrow().iter().any(|e| e == "resume:a"));
    }

    #[test]
    fn pop_last_state_is_ignored() {
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut ctx = RuntimeCtx;
        let mut stack = StateStack::new();
        let mut only = RecordingState::new("root", log.clone());
        only.next = Some(Transition::Pop);
        stack.push(&mut ctx, Box::new(only));
        tick(&mut stack, &mut ctx);
        assert_eq!(stack.len(), 1);
        assert_eq!(stack.top_id(), Some("root"));
        assert!(!log.borrow().iter().any(|e| e == "exit:root"));
    }
}

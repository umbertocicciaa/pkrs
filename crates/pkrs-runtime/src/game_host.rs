//! Runtime host: wires the §3.3 tick pipeline to the state stack.

use pkrs_platform::{DisplaySettings, Platform};
use pkrs_render::{NullRenderer, Renderer, RendererConfig};
use pkrs_script::{LuaScriptHost, Scheduler};

use crate::ctx::RuntimeCtx;
use crate::main_loop::SimulationHost;
use crate::state::{LuaUiState, StateStack, TitleState};
use crate::tick::{TickHooks, run_tick};
use crate::ui_assets::build_test_ui_theme;

/// Simulation host with an active [`StateStack`] starting at [`TitleState`].
pub struct RuntimeHost {
    stack: StateStack,
    ctx: RuntimeCtx,
    renderer: Renderer,
}

impl RuntimeHost {
    /// Stack with title state and the given renderer (headless or windowed).
    #[must_use]
    pub fn new(renderer: Renderer) -> Self {
        let ctx = RuntimeCtx::default();
        let stack = StateStack::new();

        Self {
            stack,
            ctx,
            renderer,
        }
    }

    /// Title screen with a [`NullRenderer`] (tests and headless runs).
    #[must_use]
    pub fn with_title() -> Self {
        let mut host = Self::new(Renderer::new(
            Box::new(NullRenderer::new()),
            DisplaySettings::default(),
            RendererConfig::default(),
        ));
        if let Ok(theme) = build_test_ui_theme(host.renderer_mut())
            && let Ok(title) = TitleState::for_stack_demo(theme, host.renderer_mut())
        {
            host.push_state(Box::new(title));
        }
        host
    }

    /// Title screen, headless renderer, and a paired Lua host + scheduler (§9.3, §9.5).
    #[must_use]
    pub fn with_title_and_scripts(script_host: LuaScriptHost, scheduler: Scheduler) -> Self {
        let mut ctx = RuntimeCtx {
            script_host: Some(script_host),
            scheduler: Some(scheduler),
            ..RuntimeCtx::default()
        };
        let mut stack = StateStack::new();
        let mut renderer = Renderer::new(
            Box::new(NullRenderer::new()),
            DisplaySettings::default(),
            RendererConfig::default(),
        );
        if let Ok(theme) = build_test_ui_theme(&mut renderer)
            && let Ok(title) = TitleState::for_stack_demo(theme, &renderer)
        {
            stack.push(&mut ctx, Box::new(title));
        }

        Self {
            stack,
            ctx,
            renderer,
        }
    }

    /// Active state stack (e.g. tests, debug overlay).
    #[must_use]
    pub fn stack(&self) -> &StateStack {
        &self.stack
    }

    /// Mutable renderer (e.g. tests building UI states).
    pub fn renderer_mut(&mut self) -> &mut Renderer {
        &mut self.renderer
    }

    /// Whether [`crate::Transition::Quit`] was applied.
    #[must_use]
    pub fn should_quit(&self) -> bool {
        self.stack.quit_requested()
    }

    /// Mutable simulation context (tests, wiring script tasks and wake signals).
    pub fn runtime_ctx(&mut self) -> &mut RuntimeCtx {
        &mut self.ctx
    }

    /// Pushes `state` using the host context.
    pub fn push_state(&mut self, state: Box<dyn crate::state::GameState>) {
        self.stack.push(&mut self.ctx, state);
    }

    fn render_frame(&mut self, platform: &Platform, alpha: f32) {
        let (w, h) = platform.window_size();
        self.renderer.begin_frame(w, h);
        self.stack.render(&mut self.renderer, alpha);
        self.renderer.end_frame();
    }
}

impl TickHooks for RuntimeHost {
    fn scripts(&mut self, _dt: f64) {
        let wakes = self.ctx.drain_script_wakes();
        if let Some(scheduler) = self.ctx.scheduler.as_mut() {
            for signal in wakes {
                let _ = scheduler.signal(signal);
            }
            if let Some(host) = self.ctx.script_host.as_mut() {
                scheduler.step(host);
            }
        }
    }

    fn state_stack(&mut self, _dt: f64) {
        if let (Some(_host), Some(scheduler)) = (&mut self.ctx.script_host, &mut self.ctx.scheduler)
        {
            let pushes = scheduler.drain_pending_ui_pushes();
            for p in pushes {
                self.stack
                    .push(&mut self.ctx, Box::new(LuaUiState::from_pending(p)));
            }
        }
        self.stack.run_update(&mut self.ctx);
    }

    fn deferred_transitions(&mut self) {
        self.stack.apply_pending(&mut self.ctx);
    }
}

impl SimulationHost for RuntimeHost {
    fn poll_input(&mut self, platform: &Platform) {
        self.ctx.input.poll(platform);
    }

    fn begin_tick(&mut self) {
        self.ctx.input.begin_tick();
    }

    fn simulation_tick(&mut self, dt: f64) {
        run_tick(self, dt);
    }

    fn render(&mut self, platform: &Platform, alpha: f32) {
        self.render_frame(platform, alpha);
    }

    fn should_quit(&self) -> bool {
        self.stack.quit_requested()
    }
}

//! Runtime host: wires the §3.3 tick pipeline to the state stack.

use pkrs_platform::{DisplaySettings, Platform};
use pkrs_render::{NullRenderer, Renderer, RendererConfig};

use crate::ctx::RuntimeCtx;
use crate::main_loop::SimulationHost;
use crate::state::{StateStack, TitleState};
use crate::tick::{TickHooks, run_tick};

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
        let mut ctx = RuntimeCtx::default();
        let mut stack = StateStack::new();
        stack.push(&mut ctx, Box::new(TitleState));
        Self {
            stack,
            ctx,
            renderer,
        }
    }

    /// Title screen with a [`NullRenderer`] (tests and headless runs).
    #[must_use]
    pub fn with_title() -> Self {
        Self::new(Renderer::new(
            Box::new(NullRenderer::new()),
            DisplaySettings::default(),
            RendererConfig::default(),
        ))
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

    /// Whether [`Transition::Quit`] was applied.
    #[must_use]
    pub fn should_quit(&self) -> bool {
        self.stack.quit_requested()
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
    fn state_stack(&mut self, _dt: f64) {
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

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
    renderer: Option<Renderer>,
    quit: bool,
}

impl RuntimeHost {
    /// Title screen on a fresh stack.
    #[must_use]
    pub fn with_title() -> Self {
        Self {
            stack: StateStack::with_initial(Box::new(TitleState)),
            ctx: RuntimeCtx,
            renderer: None,
            quit: false,
        }
    }

    /// Active state stack (e.g. tests, debug overlay).
    #[must_use]
    pub fn stack(&self) -> &StateStack {
        &self.stack
    }

    /// Mutable stack access for scripted tests.
    pub fn stack_mut(&mut self) -> &mut StateStack {
        &mut self.stack
    }

    /// Shared context passed to states each tick.
    pub fn ctx_mut(&mut self) -> &mut RuntimeCtx {
        &mut self.ctx
    }

    /// Whether [`Transition::Quit`] was applied.
    #[must_use]
    pub fn should_quit(&self) -> bool {
        self.quit
    }

    /// Pushes `state` using the host context.
    pub fn push_state(&mut self, state: Box<dyn crate::state::GameState>) {
        self.stack.push(&mut self.ctx, state);
    }

    fn render_frame(&mut self, platform: &Platform, alpha: f32) {
        if self.renderer.is_none() {
            let backend = Box::new(NullRenderer::new());
            self.renderer = Some(Renderer::new(
                backend,
                DisplaySettings::default(),
                RendererConfig::default(),
            ));
        }
        if let Some(renderer) = self.renderer.as_mut() {
            let (w, h) = platform.window_size();
            renderer.begin_frame(w, h);
            self.stack.render(renderer, alpha);
            renderer.end_frame();
        }
    }
}

impl TickHooks for RuntimeHost {
    fn state_stack(&mut self, _dt: f64) {
        self.stack.run_update(&mut self.ctx);
    }

    fn deferred_transitions(&mut self) {
        self.stack.apply_pending(&mut self.ctx);
        if self.stack.quit_requested() {
            self.quit = true;
        }
    }
}

impl SimulationHost for RuntimeHost {
    fn poll_input(&mut self, _platform: &Platform) {}

    fn begin_tick(&mut self) {}

    fn simulation_tick(&mut self, dt: f64) {
        run_tick(self, dt);
    }

    fn render(&mut self, platform: &Platform, alpha: f32) {
        self.render_frame(platform, alpha);
    }
}

//! Title and save UI before the overworld (§4.1, §12).

use pkrs_platform::{Input, InputFacadeConfig, Platform};
use pkrs_render::{NullRenderer, Renderer, RendererConfig};
use pkrs_script::LuaScriptHost;

use super::{Project, ProjectBoot, ProjectError, ProjectHost};
use crate::ctx::{BootAction, RuntimeCtx};
use crate::default_theme::build_default_ui_theme;
use crate::main_loop::SimulationHost;
use crate::state::{StateStack, TitleState};
use crate::tick::{TickHooks, run_tick};
use crate::ui_theme::UiTheme;

/// Runs title / save UI, then the project overworld as one [`SimulationHost`].
pub struct ProjectSessionHost {
    project: Project,
    scripts: Option<LuaScriptHost>,
    ui_theme: UiTheme,
    ui_renderer: Renderer,
    ctx: RuntimeCtx,
    stack: StateStack,
    overworld: Option<ProjectHost>,
    quit: bool,
}

impl ProjectSessionHost {
    /// Opens `project`: title first unless `skip_title` (e.g. `--map` / `--warp`).
    pub fn open(
        project: &Project,
        mut renderer: Renderer,
        scripts: LuaScriptHost,
        skip_title: bool,
    ) -> Result<Self, ProjectError> {
        let input = Input::from_manifest(InputFacadeConfig::default(), &project.manifest.input)
            .unwrap_or_else(|_| Input::new(InputFacadeConfig::default()));
        let ui_theme = load_ui_theme(project, &mut renderer)?;
        let mut session = Self {
            project: project.clone(),
            scripts: Some(scripts),
            ui_theme,
            ui_renderer: renderer,
            ctx: RuntimeCtx {
                input,
                ..RuntimeCtx::default()
            },
            stack: StateStack::new(),
            overworld: None,
            quit: false,
        };
        if skip_title {
            session.boot_overworld(ProjectBoot::Immediate)?;
        } else {
            session.push_title()?;
        }
        Ok(session)
    }

    fn push_title(&mut self) -> Result<(), ProjectError> {
        let title =
            TitleState::for_project(&self.project, self.ui_theme.clone(), &self.ui_renderer)
                .map_err(|err| {
                    ProjectError::Core(pkrs_core::CoreError::InvalidId(err.to_string()))
                })?;
        self.stack.push(&mut self.ctx, Box::new(title));
        Ok(())
    }

    fn boot_overworld(&mut self, boot: ProjectBoot) -> Result<(), ProjectError> {
        let Some(scripts) = self.scripts.take() else {
            return Err(ProjectError::Core(pkrs_core::CoreError::InvalidId(
                "script host already started".into(),
            )));
        };
        let renderer = std::mem::replace(&mut self.ui_renderer, fresh_renderer(&self.project));
        self.overworld = Some(ProjectHost::with_scripts_boot(
            &self.project,
            renderer,
            scripts,
            boot,
        )?);
        self.stack = StateStack::new();
        Ok(())
    }

    fn drain_boot(&mut self) -> Result<(), ProjectError> {
        while let Some(action) = self.ctx.boot_actions.pop_front() {
            match action {
                BootAction::NewGame => {
                    self.boot_overworld(ProjectBoot::AtTitle)?;
                    if let Some(host) = self.overworld.as_mut() {
                        host.begin_new_game()?;
                    }
                }
                BootAction::LoadSlot(slot) => {
                    self.boot_overworld(ProjectBoot::AtTitle)?;
                    if let Some(host) = self.overworld.as_mut() {
                        host.load_from_slot(slot).map_err(|err| {
                            ProjectError::Core(pkrs_core::CoreError::InvalidId(format!(
                                "save load: {err}"
                            )))
                        })?;
                    }
                }
                BootAction::SaveSlot(slot) => {
                    if let Some(host) = self.overworld.as_mut() {
                        let _ = host.save_to_slot(slot);
                    }
                }
            }
        }
        Ok(())
    }

    fn on_title_tick(&mut self) -> Result<(), ProjectError> {
        if self.stack.quit_requested() {
            self.quit = true;
        }
        self.drain_boot()?;
        Ok(())
    }

    /// Overworld host after boot; `None` on the title screen only.
    pub fn overworld_mut(&mut self) -> Option<&mut ProjectHost> {
        self.overworld.as_mut()
    }

    /// Same as [`overworld_mut`](Self::overworld_mut), shared reference.
    #[must_use]
    pub fn overworld(&self) -> Option<&ProjectHost> {
        self.overworld.as_ref()
    }

    /// Mutable overworld (integration tests after Continue / New Game).
    pub fn host_mut(&mut self) -> Option<&mut ProjectHost> {
        self.overworld.as_mut()
    }

    /// Overworld reference (integration tests).
    #[must_use]
    pub fn host(&self) -> Option<&ProjectHost> {
        self.overworld.as_ref()
    }

    /// Simulation ticks on the overworld (0 on the title screen).
    pub fn ticks(&self) -> u64 {
        self.overworld.as_ref().map_or(0, ProjectHost::ticks)
    }

    /// Active built-in screen id (`title`, `save`, …) when the stack is non-empty.
    pub fn front_screen(&self) -> Option<&str> {
        self.stack.top_id()
    }
}

fn load_ui_theme(project: &Project, renderer: &mut Renderer) -> Result<UiTheme, ProjectError> {
    if let Ok(theme) = build_default_ui_theme(renderer) {
        return Ok(theme);
    }
    let mut spare = fresh_renderer(project);
    if let Ok(theme) = build_default_ui_theme(&mut spare) {
        return Ok(theme);
    }
    let mut spare = fresh_renderer(project);
    crate::ui_assets::build_test_ui_theme(&mut spare)
        .map_err(|err| ProjectError::Core(pkrs_core::CoreError::InvalidId(err.to_string())))
}

fn fresh_renderer(project: &Project) -> Renderer {
    Renderer::new(
        Box::new(NullRenderer::new()),
        project.display(),
        RendererConfig::default(),
    )
}

impl TickHooks for ProjectSessionHost {
    fn state_stack(&mut self, _dt: f64) {
        if self.overworld.is_some() && self.stack.is_empty() {
            return;
        }
        self.stack.run_update(&mut self.ctx);
    }

    fn deferred_transitions(&mut self) {
        if self.overworld.is_some() && self.stack.is_empty() {
            return;
        }
        self.stack.apply_pending(&mut self.ctx);
    }
}

impl SimulationHost for ProjectSessionHost {
    fn poll_input(&mut self, platform: &Platform) {
        if let Some(host) = self.overworld.as_mut() {
            host.poll_input(platform);
            if !self.stack.is_empty() {
                self.ctx.input.poll(platform);
            }
        } else {
            self.ctx.input.poll(platform);
        }
    }

    fn begin_tick(&mut self) {
        if let Some(host) = self.overworld.as_mut() {
            host.begin_tick();
            if !self.stack.is_empty() {
                self.ctx.input.begin_tick();
            }
        } else {
            self.ctx.input.begin_tick();
        }
    }

    fn simulation_tick(&mut self, dt: f64) {
        if let Some(host) = self.overworld.as_mut() {
            host.simulation_tick(dt);
            if !self.stack.is_empty() {
                run_tick(self, dt);
                if let Err(err) = self.on_title_tick() {
                    tracing::error!(%err, "title/session boot step failed");
                }
            }
            return;
        }
        run_tick(self, dt);
        if let Err(err) = self.on_title_tick() {
            tracing::error!(%err, "title/session boot step failed");
        }
    }

    fn render(&mut self, platform: &Platform, alpha: f32) {
        let (w, h) = platform.window_size();
        if let Some(host) = self.overworld.as_mut() {
            host.render(platform, alpha);
            if self.stack.is_empty() {
                return;
            }
            let mut overlay = fresh_renderer(&self.project);
            overlay.begin_frame(w, h);
            self.stack.render(&mut overlay, alpha);
            overlay.end_frame();
            return;
        }
        self.ui_renderer.begin_frame(w, h);
        self.stack.render(&mut self.ui_renderer, alpha);
        self.ui_renderer.end_frame();
    }

    fn should_quit(&self) -> bool {
        self.quit || self.stack.quit_requested()
    }
}

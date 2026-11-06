//! Lua-defined UI screen on the stack (`ui.register_screen` / `ui.push_screen`, §9.5).

use pkrs_render::{Color, Layer, Rect, Renderer};
use pkrs_script::{GfxOp, LuaScriptHost, PendingUiPush, ScriptOwner, ScriptValue, TaskId};

use super::{GameState, Transition};
use crate::ctx::RuntimeCtx;

fn parse_ui_transition(v: &ScriptValue) -> Option<(Transition, ScriptValue)> {
    match v {
        ScriptValue::String(s) if s == "pop" => Some((Transition::Pop, ScriptValue::Nil)),
        ScriptValue::Map(m) => {
            if let Some(ScriptValue::String(target)) = m.get("pop_to") {
                let res = m.get("result").cloned().unwrap_or(ScriptValue::Nil);
                return Some((Transition::PopTo(target.to_owned()), res));
            }
            if matches!(m.get("pop"), Some(ScriptValue::Bool(true))) {
                let res = m.get("result").cloned().unwrap_or(ScriptValue::Nil);
                return Some((Transition::Pop, res));
            }
            None
        }
        _ => None,
    }
}

/// Stack state driven by a registered Lua UI handler table.
pub struct LuaUiState {
    owner: ScriptOwner,
    screen_id: String,
    args: ScriptValue,
    waiting_task: TaskId,
    cached_ops: Vec<GfxOp>,
}

impl LuaUiState {
    /// Builds state from a script task that yielded [`WaitRequest::PushUiScreen`](pkrs_script::WaitRequest::PushUiScreen).
    #[must_use]
    pub fn from_pending(p: PendingUiPush) -> Self {
        Self {
            owner: p.owner,
            screen_id: p.screen,
            args: p.args,
            waiting_task: p.task,
            cached_ops: Vec::new(),
        }
    }

    fn refresh_render_cache(&mut self, host: &mut LuaScriptHost) {
        match host.ui_screen_render_ops(&self.owner, &self.screen_id) {
            Ok(ops) => self.cached_ops = ops,
            Err(err) => {
                tracing::error!(target: "script", screen = %self.screen_id, "Lua UI render: {err}");
                self.cached_ops.clear();
            }
        }
    }

    fn wake_on_close(&self, result: ScriptValue, ctx: &mut RuntimeCtx) {
        ctx.queue_script_wake(pkrs_script::WakeSignal::UiClosed {
            task: self.waiting_task,
            screen: self.screen_id.clone(),
            result,
        });
    }
}

impl GameState for LuaUiState {
    fn id(&self) -> &str {
        self.screen_id.as_str()
    }

    fn is_opaque(&self) -> bool {
        true
    }

    fn on_enter(&mut self, ctx: &mut RuntimeCtx) {
        let Some(host) = ctx.script_host.as_mut() else {
            tracing::warn!(
                target: "script",
                screen = %self.screen_id,
                "Lua UI on_enter: no script host"
            );
            return;
        };
        if let Err(err) = host.ui_screen_on_enter(&self.owner, &self.screen_id, self.args.clone()) {
            tracing::error!(target: "script", screen = %self.screen_id, "{err}");
            return;
        }
        self.refresh_render_cache(host);
    }

    fn update(&mut self, ctx: &mut RuntimeCtx) -> Transition {
        let Some(host) = ctx.script_host.as_mut() else {
            return Transition::None;
        };
        if let Err(err) = host.sync_script_input(&self.owner, &ctx.input) {
            tracing::error!(target: "script", screen = %self.screen_id, "sync_script_input: {err}");
            return Transition::None;
        }
        let value = match host.ui_screen_update(&self.owner, &self.screen_id) {
            Ok(v) => v,
            Err(err) => {
                tracing::error!(target: "script", screen = %self.screen_id, "Lua UI update: {err}");
                self.wake_on_close(ScriptValue::Nil, ctx);
                return Transition::Pop;
            }
        };
        if let Some((transition, result)) = parse_ui_transition(&value) {
            match &transition {
                Transition::Pop | Transition::PopTo(_) => self.wake_on_close(result, ctx),
                Transition::None
                | Transition::Push(_)
                | Transition::Replace(_)
                | Transition::Quit => {}
            }
            return transition;
        }
        self.refresh_render_cache(host);
        Transition::None
    }

    fn render(&self, renderer: &mut Renderer, _alpha: f32) {
        for op in &self.cached_ops {
            match op {
                GfxOp::Window { x, y, w, h } => {
                    let dst = Rect::new(*x as f32, *y as f32, *w as f32, *h as f32);
                    let color = Color {
                        r: 48,
                        g: 48,
                        b: 64,
                        a: 220,
                    };
                    renderer.draw_filled_rect(dst, color, Layer::Ui);
                }
                GfxOp::Text { x, y, text: _ } => {
                    let dst = Rect::new(*x as f32, *y as f32, 2.0, 2.0);
                    renderer.draw_filled_rect(dst, Color::WHITE, Layer::Ui);
                }
            }
        }
    }
}

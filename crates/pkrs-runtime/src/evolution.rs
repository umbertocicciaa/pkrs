//! The evolution scene (§8.1 `evolutions`): placeholder art at internal resolution.
//!
//! A dark backdrop with the creature in the middle as a coloured block. While the scene
//! animates, the block flickers between the old and the new species' colours, faster and
//! faster, and a white glow pulses over it; at the end it shows the new species. The
//! post-battle flow (`battle::rewards`) shows the texts around it ("What? … is evolving!",
//! "Congratulations! …") in the bottom text panel.

use pkrs_render::{Color, Layer, Rect, Renderer};

use crate::battle::screen::{TEXT_PANEL_HEIGHT, creature_color};

/// Ticks of the evolution animation (2 s at 60 Hz).
pub const EVOLUTION_TICKS: u32 = 120;

/// Size of the creature block.
const CREATURE_SIZE: f32 = 56.0;

/// A running (or finished) evolution animation of `from` into `to` (display names).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvolutionScene {
    from: String,
    to: String,
    elapsed: u32,
}

impl EvolutionScene {
    /// A scene at its first frame.
    #[must_use]
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            elapsed: 0,
        }
    }

    /// Display name of the creature before evolving.
    #[must_use]
    pub fn from(&self) -> &str {
        &self.from
    }

    /// Display name of the species it evolves into.
    #[must_use]
    pub fn to(&self) -> &str {
        &self.to
    }

    /// Advances one tick; `true` once the animation is over.
    pub fn tick(&mut self) -> bool {
        self.elapsed = (self.elapsed + 1).min(EVOLUTION_TICKS);
        self.is_done()
    }

    /// `true` once the animation is over (the new species is shown).
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.elapsed >= EVOLUTION_TICKS
    }

    /// Jumps to the end (the new species is shown).
    pub fn finish(&mut self) {
        self.elapsed = EVOLUTION_TICKS;
    }

    /// `true` when this frame shows the new species: the two forms alternate with a period
    /// that shrinks from 32 to 4 ticks, then the new form stays.
    #[must_use]
    pub fn shows_new_form(&self) -> bool {
        if self.is_done() {
            return true;
        }
        let left = EVOLUTION_TICKS - self.elapsed;
        let half_period = (left / 8).clamp(2, 16);
        (self.elapsed / half_period) % 2 == 1
    }

    /// Draws the backdrop and the creature (between `begin_frame` and `end_frame`); `view`
    /// is the internal resolution. The text panel is drawn by the caller.
    pub fn render(&self, renderer: &mut Renderer, view: (u32, u32)) {
        let (vw, vh) = (view.0 as f32, view.1 as f32);
        let field_h = (vh - TEXT_PANEL_HEIGHT).max(CREATURE_SIZE);
        renderer.draw_filled_rect(Rect::new(0.0, 0.0, vw, vh), BACKDROP, Layer::Ground);
        let x = (vw - CREATURE_SIZE) / 2.0;
        let y = (field_h - CREATURE_SIZE) / 2.0;
        let name = if self.shows_new_form() {
            &self.to
        } else {
            &self.from
        };
        renderer.draw_filled_rect(
            Rect::new(x - 8.0, y + CREATURE_SIZE - 4.0, CREATURE_SIZE + 16.0, 8.0),
            PLATFORM,
            Layer::GroundDecor,
        );
        renderer.draw_filled_rect(
            Rect::new(x, y, CREATURE_SIZE, CREATURE_SIZE),
            creature_color(name),
            Layer::Entities,
        );
        if !self.is_done() {
            // A glow that pulses every 20 ticks.
            let phase = self.elapsed % 20;
            let alpha = (if phase < 10 { phase } else { 20 - phase }) as u8 * 12;
            renderer.draw_filled_rect(
                Rect::new(x, y, CREATURE_SIZE, CREATURE_SIZE),
                Color {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: alpha,
                },
                Layer::Entities,
            );
        }
    }
}

const BACKDROP: Color = Color {
    r: 24,
    g: 24,
    b: 40,
    a: 255,
};
const PLATFORM: Color = Color {
    r: 64,
    g: 64,
    b: 96,
    a: 255,
};

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_platform::DisplaySettings;
    use pkrs_render::{NullRenderer, RendererConfig};

    #[test]
    fn the_animation_alternates_forms_then_ends_on_the_new_one() {
        let mut scene = EvolutionScene::new("Sproutling", "Bloomling");
        assert_eq!((scene.from(), scene.to()), ("Sproutling", "Bloomling"));
        assert!(!scene.shows_new_form() && !scene.is_done());
        let mut flips = 0;
        let mut last = scene.shows_new_form();
        let mut ticks = 0;
        while !scene.tick() {
            ticks += 1;
            if scene.shows_new_form() != last {
                flips += 1;
                last = !last;
            }
        }
        assert_eq!(ticks + 1, EVOLUTION_TICKS);
        assert!(flips >= 6, "the forms alternate ({flips} flips)");
        assert!(scene.shows_new_form() && scene.is_done());
        assert!(scene.tick(), "stays done");

        let mut skipped = EvolutionScene::new("A", "B");
        skipped.finish();
        assert!(skipped.is_done());
    }

    #[test]
    fn renders_at_internal_resolution() {
        let mut r = Renderer::new(
            Box::new(NullRenderer::new()),
            DisplaySettings::default(),
            RendererConfig::default(),
        );
        let mut scene = EvolutionScene::new("Sproutling", "Bloomling");
        for _ in 0..3 {
            r.begin_frame(240, 160);
            let view = r.view_size();
            scene.render(&mut r, view);
            r.end_frame();
            scene.tick();
        }
    }
}

//! Adapter selection for windowed and headless runs (§4.5, §14.4, §15.2).
//!
//! `pkrs-runtime --headless` must run without a window or GPU so CI can smoke-test the
//! ports/adapters surface on every OS. This module is the single place that decides which
//! adapter backs each port for a given [`RunMode`], so the choice is unit-testable.

use pkrs_assets::Vfs;
use pkrs_audio::{AudioBackend, KiraAudio, NullAudio};
use pkrs_platform::{
    DisplaySettings, FixedPaths, NullPlatform, Platform, PlatformConfig, WindowConfig,
};
use pkrs_render::{DrawLog, NullRenderer, Renderer, RendererConfig};

/// How the runtime was asked to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunMode {
    /// Open a window and drive the GPU (macroquad adapters).
    Windowed,
    /// No window, no GPU: Null adapters only (`--headless`, CI smoke test).
    Headless,
}

impl RunMode {
    /// `Headless` when `headless` is set, otherwise `Windowed`.
    #[must_use]
    pub const fn from_headless_flag(headless: bool) -> Self {
        if headless {
            Self::Headless
        } else {
            Self::Windowed
        }
    }
}

/// Adapter behind the `PlatformBackend` port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformAdapter {
    /// `MacroquadPlatform`: real window, loop and keyboard.
    Macroquad,
    /// `NullPlatform`: scripted frames, no window.
    Null,
}

/// Adapter behind the `RenderBackend` port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderAdapter {
    /// `MacroquadRenderer`: GL render target and sprite batch.
    Macroquad,
    /// `NullRenderer`: records draws for assertions.
    Null,
}

/// Adapter behind the `AudioBackend` port (§11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioAdapter {
    /// `KiraAudio`: real output; falls back to `NullAudio` (logged) without a sound device.
    Kira,
    /// `NullAudio`: records commands, no sound device needed.
    Null,
}

/// Which adapter backs each port for one run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdapterSet {
    /// Window, loop and input.
    pub platform: PlatformAdapter,
    /// Drawing.
    pub renderer: RenderAdapter,
    /// Sound.
    pub audio: AudioAdapter,
}

impl AdapterSet {
    /// The adapters used for `mode`.
    #[must_use]
    pub const fn for_mode(mode: RunMode) -> Self {
        match mode {
            RunMode::Windowed => Self {
                platform: PlatformAdapter::Macroquad,
                renderer: RenderAdapter::Macroquad,
                audio: AudioAdapter::Kira,
            },
            RunMode::Headless => Self {
                platform: PlatformAdapter::Null,
                renderer: RenderAdapter::Null,
                audio: AudioAdapter::Null,
            },
        }
    }

    /// `true` when no adapter needs a window or GPU context.
    #[must_use]
    pub const fn is_headless(&self) -> bool {
        matches!(self.platform, PlatformAdapter::Null)
            && matches!(self.renderer, RenderAdapter::Null)
            && matches!(self.audio, AudioAdapter::Null)
    }
}

/// [`Platform`] facade over [`NullPlatform`], with user paths fixed under `pkrs/`.
#[must_use]
pub fn headless_platform(config: PlatformConfig) -> Platform {
    Platform::new(
        Box::new(NullPlatform::new()),
        Box::new(FixedPaths::under("pkrs")),
        config,
    )
}

/// [`Renderer`] facade over [`NullRenderer`], plus the draw log for assertions.
#[must_use]
pub fn headless_renderer(display: DisplaySettings) -> (Renderer, DrawLog) {
    let null = NullRenderer::new();
    let log = null.draw_log();
    let renderer = Renderer::new(Box::new(null), display, RendererConfig::default());
    (renderer, log)
}

/// The [`AudioBackend`] for `adapter`, reading sounds from `vfs`. `Kira` falls back to
/// [`NullAudio`] (with a warning) when no output device can be opened, so machines and CI
/// runners without sound still run the game.
#[must_use]
pub fn audio_backend(adapter: AudioAdapter, vfs: Box<dyn Vfs>) -> Box<dyn AudioBackend> {
    match adapter {
        AudioAdapter::Kira => KiraAudio::open_or_null(vfs),
        AudioAdapter::Null => Box::new(NullAudio::new()),
    }
}

/// Window settings for a windowed run titled `title`.
#[must_use]
pub fn window_config(title: &str, display: &DisplaySettings, vsync: bool) -> WindowConfig {
    let mut window = WindowConfig::from_display(title, display);
    window.vsync = vsync;
    window
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_wiring_selects_null_adapters() {
        let set = AdapterSet::for_mode(RunMode::from_headless_flag(true));
        assert_eq!(set.platform, PlatformAdapter::Null);
        assert_eq!(set.renderer, RenderAdapter::Null);
        assert_eq!(set.audio, AudioAdapter::Null);
        assert!(set.is_headless());
    }

    #[test]
    fn windowed_wiring_selects_macroquad_adapters() {
        let set = AdapterSet::for_mode(RunMode::from_headless_flag(false));
        assert_eq!(set.platform, PlatformAdapter::Macroquad);
        assert_eq!(set.renderer, RenderAdapter::Macroquad);
        assert_eq!(set.audio, AudioAdapter::Kira);
        assert!(!set.is_headless());
    }

    #[test]
    fn null_audio_adapter_needs_no_device() {
        let backend = audio_backend(AudioAdapter::Null, Box::new(pkrs_assets::MemoryVfs::new()));
        assert_eq!(backend.name(), "null");
    }

    #[test]
    fn headless_renderer_records_into_returned_log() {
        let (mut renderer, log) = headless_renderer(DisplaySettings::default());
        renderer.begin_frame(240, 160);
        renderer.draw_sprite(pkrs_render::SpriteDraw {
            texture: pkrs_render::TextureHandle(1),
            src: pkrs_render::Rect::new(0.0, 0.0, 8.0, 8.0),
            dst: pkrs_render::Rect::new(0.0, 0.0, 8.0, 8.0),
            flip_x: false,
            tint: pkrs_render::Color::WHITE,
            layer: pkrs_render::Layer::Entities,
        });
        renderer.end_frame();
        assert_eq!(log.draws().len(), 1);
    }

    #[test]
    fn headless_platform_needs_no_window() {
        let platform = headless_platform(PlatformConfig::default());
        assert_eq!(platform.frame_count(), 0);
    }
}

//! Window configuration derived from the project's display settings (`pkrs.toml [display]`, §2.2).

/// The `[display]` table of `pkrs.toml`: internal resolution and initial window mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplaySettings {
    /// Internal render width in pixels (default 240).
    pub width: u32,
    /// Internal render height in pixels (default 160).
    pub height: u32,
    /// Initial integer window scale (default 3). `0` is treated as `1`.
    pub scale: u32,
    /// Start in fullscreen.
    pub fullscreen: bool,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            width: 240,
            height: 160,
            scale: 3,
            fullscreen: false,
        }
    }
}

impl From<&pkrs_core::manifest::DisplayConfig> for DisplaySettings {
    fn from(display: &pkrs_core::manifest::DisplayConfig) -> Self {
        Self {
            width: display.width,
            height: display.height,
            scale: display.scale,
            fullscreen: display.fullscreen,
        }
    }
}

impl DisplaySettings {
    /// Window size in physical pixels for windowed mode: internal resolution × scale.
    pub fn window_size(&self) -> (u32, u32) {
        let scale = self.scale.max(1);
        (
            self.width.saturating_mul(scale),
            self.height.saturating_mul(scale),
        )
    }
}

/// Everything a backend needs to open the game window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowConfig {
    /// Window title (usually `game.name`).
    pub title: String,
    /// Initial window width in pixels.
    pub width: u32,
    /// Initial window height in pixels.
    pub height: u32,
    /// Start in fullscreen.
    pub fullscreen: bool,
    /// Whether the user can resize the window (the integer scaler handles any size, §5.1).
    pub resizable: bool,
}

impl WindowConfig {
    /// Builds the window configuration from `pkrs.toml [display]`.
    pub fn from_display(title: impl Into<String>, display: &DisplaySettings) -> Self {
        let (width, height) = display.window_size();
        Self {
            title: title.into(),
            width,
            height,
            fullscreen: display.fullscreen,
            resizable: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_size_is_resolution_times_scale() {
        let display = DisplaySettings::default();
        assert_eq!(display.window_size(), (720, 480));
    }

    #[test]
    fn zero_scale_is_treated_as_one() {
        let display = DisplaySettings {
            scale: 0,
            ..DisplaySettings::default()
        };
        assert_eq!(display.window_size(), (240, 160));
    }

    #[test]
    fn manifest_display_defaults_match() {
        let manifest = pkrs_core::manifest::DisplayConfig::default();
        assert_eq!(DisplaySettings::from(&manifest), DisplaySettings::default());
    }

    #[test]
    fn manifest_display_is_copied() {
        let src = "[game]\nname = \"t\"\nid = \"t\"\n[engine]\nversion = \"0.0\"\napi = 1\n\
                   [display]\nwidth = 320\nheight = 180\nscale = 2\nfullscreen = true\n\
                   [start]\nmap = \"m\"\nposition = [0, 0]\n";
        let manifest = pkrs_core::ProjectManifest::from_toml_str(src).expect("valid");
        let display = DisplaySettings::from(&manifest.display);
        assert_eq!(
            display,
            DisplaySettings {
                width: 320,
                height: 180,
                scale: 2,
                fullscreen: true
            }
        );
    }

    #[test]
    fn from_display_copies_mode_and_size() {
        let display = DisplaySettings {
            width: 320,
            height: 180,
            scale: 2,
            fullscreen: true,
        };
        let window = WindowConfig::from_display("Lighthouse Tale", &display);
        assert_eq!(window.title, "Lighthouse Tale");
        assert_eq!((window.width, window.height), (640, 360));
        assert!(window.fullscreen);
    }
}

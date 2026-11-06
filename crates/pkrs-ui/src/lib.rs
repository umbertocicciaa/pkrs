//! State-stack UI: dialogue, choices, window skins (§4.3, §5.5).

mod choice;
mod codes;
mod dialogue;
mod error;
mod layout;
mod window_skin;

pub use choice::{ChoiceOption, ChoicePollOutcome, ChoiceSession};
pub use codes::{DialogueSegment, expand_dialogue, pause_schedule};
pub use dialogue::{DialogueAdvance, DialoguePage, DialogueSession};
pub use error::{Result, UiError};
pub use layout::{
    colored_line_char_count, draw_colored_lines, flatten_segments, wrap_colored_lines,
};
pub use window_skin::{NineSliceSpec, WindowSkin, draw_more_arrow};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_human_readable() {
        let err: UiError = UiError::InvalidSkin("default".into());
        assert_eq!(err.to_string(), "invalid window skin `default`");
    }

    #[test]
    fn result_alias_defaults_to_crate_error() {
        let ok: Result<u8> = Ok(1);
        assert!(matches!(ok, Ok(1)));
    }
}

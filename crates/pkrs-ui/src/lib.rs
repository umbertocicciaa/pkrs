//! State-stack UI: dialogue, choices, window skins (§4.3, §5.5).

mod bag;
mod choice;
mod codes;
mod dialogue;
mod error;
mod item_rules;
mod keyboard;
mod layout;
mod party_menu;
mod window_skin;

pub use bag::{BagEntry, BagList};
pub use choice::{ChoiceOption, ChoicePollOutcome, ChoiceSession};
pub use codes::{DialogueSegment, expand_dialogue, expand_dialogue_with_pauses, pause_schedule};
pub use dialogue::{DialogueAdvance, DialoguePage, DialogueSession};
pub use error::{Result, UiError};
pub use item_rules::{BuiltinItemEffect, ItemUseVerdict, builtin_effect, field_use_verdict};
pub use keyboard::{KeyCell, KeyboardPollOutcome, NamingKeyboard, ROWS};
pub use layout::{
    colored_line_char_count, draw_colored_lines, flatten_segments, wrap_colored_lines,
};
pub use party_menu::{PartyReorderMode, PartyReorderSession};
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

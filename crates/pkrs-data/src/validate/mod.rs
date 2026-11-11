//! Content validation (§8.2) as vendor-free [`pkrs_core::Diagnostic`]s.
//!
//! - **Stage 1 (parse)** and the load-time **stage 2** checks are the [`crate::DataError`]s of
//!   loading ([`GameDataBuilder::finish`](crate::GameDataBuilder::finish));
//!   [`data_error_diagnostic`] turns each into a diagnostic with the file and, when the
//!   source is at hand, its line and column (RON's position for parse errors, the record or
//!   field for the others).
//! - **Stage 2 (schema)** checks that need the whole record set, and **stage 3
//!   (cross-references)**: every referenced type, ability, move, species, item, status,
//!   weather, AI profile, AI script, sprite path and `text:` key exists
//!   ([`check_game_data`]).
//!
//! Every problem is collected; nothing is fail-fast.

mod diagnostic;
mod locate;
mod refs;
mod schema;

pub use diagnostic::{data_error_diagnostic, data_error_file};
pub use locate::{find_field_span, find_quoted_span, record_offset};
pub use refs::{RefContext, TextLocale, asset_exists, check_game_data, check_text_ref};

#[cfg(test)]
mod tests;

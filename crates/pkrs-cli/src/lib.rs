//! Library half of the `pkrs` CLI: project templates and `pkrs run` plumbing (§2.3, §2.4).
//!
//! The binary (`src/main.rs`) parses arguments; this library holds the logic so it is unit
//! testable and so other workspace tests can scaffold projects.

pub mod run;
pub mod template;

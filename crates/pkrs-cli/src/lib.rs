//! Library half of the `pkrs` CLI: project and plugin templates, `pkrs run` plumbing and
//! `pkrs check` (§2.3, §2.4, §9.4).
//!
//! The binary (`src/main.rs`) parses arguments; this library holds the logic so it is unit
//! testable and so other workspace tests can scaffold projects.

pub mod check;
pub mod plugin;
pub mod report;
pub mod run;
pub mod template;

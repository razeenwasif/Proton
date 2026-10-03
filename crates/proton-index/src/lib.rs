//! The index behind Telescope.
//!
//! One fuzzy finder over everything the workspace holds is the spine of the
//! whole interaction model, so this crate owns the parts that are the same for
//! every picker: what the candidates are, how a query scores against them, and
//! which characters matched so the UI can highlight them.
//!
//! A picker is then just a source of candidates. Files and grep live here;
//! symbols and diagnostics arrive with the LSP client, and artboards, scene
//! objects and cells arrive with their modes — none of them need new matching
//! code.

pub mod files;
pub mod grep;
pub mod match_;

pub use files::{children, Child, FileIndex};
pub use grep::{grep, grep_until, GrepHit};
pub use match_::{Match, Matcher};

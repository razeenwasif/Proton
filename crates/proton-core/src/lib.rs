//! Proton's core.
//!
//! Headless by construction: no windowing, no webview, no pixels. The shell
//! sends [`Command`]s and renders [`Event`]s, and a different front end could
//! be dropped in without touching anything here. See `docs/ARCHITECTURE.md` §1.

pub mod action;
pub mod bus;
pub mod editor;
pub mod key;
pub mod mode;
pub mod vim;
pub mod workspace;

pub use action::{Action, Object, Verb};
pub use bus::{Command, Event};
pub use editor::Editor;
pub use key::Key;
pub use mode::ModeId;
pub use proton_text::{Buffer, Cursor, Edit, Position, Range};
pub use vim::{Outcome, Vim, VimMode};
pub use workspace::Workspace;

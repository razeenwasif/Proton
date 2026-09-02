//! Proton's core.
//!
//! Headless by construction: no windowing, no webview, no pixels. The shell
//! sends [`Command`]s and renders [`Event`]s, and a different front end could
//! be dropped in without touching anything here. See `docs/ARCHITECTURE.md` §1.

pub mod action;
pub mod agent;
pub mod block;
pub mod bus;
pub mod editor;
pub mod ex;
pub mod folder;
pub mod highlights;
pub mod key;
pub mod lsp;
pub mod mode;
pub mod picker;
pub mod session;
pub mod trace;
pub mod vim;
pub mod workspace;

pub use action::{Action, Object, Verb};
pub use agent::{Activity, Agent, Entry, Speaker};
pub use bus::{Command, Event, WorkspaceTab};
pub use editor::Editor;
pub use ex::Ex;
pub use highlights::Highlighters;
pub use key::Key;
pub use lsp::{Completion, Diagnostic, Servers, Severity};
pub use mode::ModeId;
pub use picker::{Picker, PickerItem, PickerKey, PickerKind};
pub use proton_syntax::{Scope, Span};
pub use proton_text::{Buffer, Cursor, Edit, Position, Range};
pub use session::Session;
pub use trace::{StepView, TraceState, Visualizer};
pub use vim::{Outcome, Vim, VimMode};
pub use workspace::Workspace;

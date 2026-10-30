//! Proton's core.
//!
//! Headless by construction: no windowing, no webview, no pixels. The shell
//! sends [`Command`]s and renders [`Event`]s, and a different front end could
//! be dropped in without touching anything here. See `docs/ARCHITECTURE.md` §1.

pub mod action;
pub mod agent;
pub mod block;
pub mod bus;
pub mod chats;
pub mod circuit;
pub mod claude;
pub mod contract;
pub mod data;
pub mod debug;
pub mod editor;
pub mod ex;
pub mod folder;
pub mod formatters;
pub mod git;
pub mod highlights;
pub mod key;
pub mod logic;
pub mod lsp;
pub mod mention;
pub mod mode;
pub mod picker;
pub mod plugins;
pub mod review;
pub mod research;
pub mod session;
pub mod settings;
pub mod sheet;
pub mod split;
pub mod study;
pub mod tasks;
pub mod trace;
pub mod tree;
pub mod typing;
pub mod vim;
pub mod workspace;

pub use action::{Action, Object, Verb};
pub use agent::{Activity, Agent, Entry, Speaker};
pub use bus::{Command, Event, WorkspaceTab};
pub use debug::{Debugger, Phase};
pub use editor::Editor;
pub use ex::Ex;
pub use git::Git;
pub use highlights::Highlighters;
pub use key::Key;
pub use lsp::{Completion, Diagnostic, Servers, Severity};
pub use mode::ModeId;
pub use picker::{Picker, PickerItem, PickerKey, PickerKind};
pub use proton_syntax::{Scope, Span};
pub use proton_claude::on_output as on_claude_output;
pub use proton_term::on_output as on_terminal_output;
pub use proton_text::{Buffer, Cursor, Edit, Position, Range};
pub use session::Session;
pub use split::{Direction, Splits, View};
pub use trace::{StepView, TraceState, Visualizer};
pub use tree::{Tree, TreeRow};
pub use vim::{Outcome, Vim, VimMode};
pub use workspace::Workspace;

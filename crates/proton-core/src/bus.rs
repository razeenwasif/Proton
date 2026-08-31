//! The command/event bus — the seam between the core and whatever draws it.
//!
//! The shell sends [`Command`]s and receives [`Event`]s. Nothing in this module
//! names a widget, a pixel or a DOM node: if a variant here needed to, the
//! design would be wrong and the GPU front end in `docs/ARCHITECTURE.md` would
//! be unreachable.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::action::Action;
use crate::key::Key;
use crate::mode::ModeId;
use crate::vim::VimMode;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Command {
    /// A raw keystroke. The core decides what it means.
    Key {
        key: Key,
    },
    /// An already-resolved action, from a menu, a click, or a replayed macro.
    Perform {
        action: Action,
    },
    SetMode {
        mode: ModeId,
    },
    /// Ask for the current state — used once on connect so the shell can render
    /// without guessing.
    Sync,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    /// The modal state changed. Drives the statusline.
    VimMode {
        mode: VimMode,
        label: String,
    },
    /// A pending command, for `showcmd`. Empty string means "cleared".
    Pending {
        keys: String,
    },
    /// The leader was pressed, or a subsequent key narrowed it.
    /// `None` closes which-key.
    Leader {
        prefix: Option<String>,
    },
    /// The command line opened, changed, or (with `None`) closed.
    Cmdline {
        text: Option<String>,
    },
    /// The command line was submitted. A separate event from `Cmdline` on
    /// purpose: the two carry the same text and mean entirely different
    /// things, and a consumer that cannot tell them apart runs every
    /// keystroke as a command.
    CmdlineRun {
        text: String,
    },
    /// An action resolved and the active mode accepted it.
    Action {
        action: Action,
        description: String,
    },
    /// The active mode has no meaning for this action. Not an error — a
    /// reportable fact, so the statusline can show it.
    Unsupported {
        description: String,
        mode: ModeId,
    },
    /// A literal character was typed in insert mode. M1 turns these into rope
    /// edits; M0 only proves the round trip.
    Input {
        text: String,
    },
    ModeChanged {
        mode: ModeId,
    },
    /// Full state, in reply to [`Command::Sync`].
    #[serde(rename_all = "camelCase")]
    State {
        mode: ModeId,
        vim_mode: VimMode,
        vim_label: String,
    },
}

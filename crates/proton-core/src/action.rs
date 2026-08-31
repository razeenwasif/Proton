//! Resolved actions — the boundary the whole architecture rests on.
//!
//! The modal engine turns keys into an [`Action`], and a mode turns an
//! [`Action`] into an edit. `2dd` resolves to `Delete · Line · 2` here, and
//! Codex splices a rope with it while Video ripple-deletes two clips. Neither
//! mode ever sees a keystroke.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub enum Verb {
    Move,
    Delete,
    Yank,
    Change,
    /// Enter insert mode. The object says where the cursor lands first.
    Insert,
    Paste,
    Undo,
    Redo,
    /// Follow a reference to where it is defined. Not an edit, but it is a
    /// verb the modal engine resolves like any other, which is what lets `gd`
    /// mean the same shape of thing in a mode that has no text in it.
    Goto,
    /// Repeat the last change — vim's `.`.
    Repeat,
}

/// What to do it to. A mode maps these onto its own nouns: `Line` is a line of
/// text in Codex, a layer in Design, a clip in Video.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub enum Object {
    Line,
    LineDown,
    LineUp,
    CharLeft,
    CharRight,
    WordForward,
    WordBackward,
    WordEnd,
    /// `W`, `B`, `E` — every non-blank counts as one class.
    BigWordForward,
    BigWordBackward,
    BigWordEnd,
    LineStart,
    /// `^` — the first non-blank, which is also where `I` inserts.
    FirstNonBlank,
    LineEnd,
    FileStart,
    FileEnd,
    /// The current selection, in visual mode.
    Selection,
    /// Where the symbol under the cursor is defined.
    Definition,
    /// No object — the verb stands alone (`u`, `.`).
    None,
    /// Cursor stays put; used by `i` and `a` to say which side.
    Here,
    After,
    /// Open a new line and insert on it.
    LineBelow,
    LineAbove,

    /* ── text objects ─────────────────────────────────────────────
    These are the reason the engine resolves to objects rather than
    to offsets: `ciw` means the same thing in every mode that has a
    notion of "the thing under the cursor". */
    InnerWord,
    AroundWord,
    InnerBigWord,
    AroundBigWord,
    /// `i(`, `i{`, `i"` … carried by the *opening* delimiter.
    InnerPair {
        open: char,
    },
    AroundPair {
        open: char,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct Action {
    pub verb: Verb,
    pub object: Object,
    /// Always at least 1. `2d3w` multiplies to 6.
    pub count: u32,
    /// The register a yank or delete writes to, from `"a`.
    pub register: Option<String>,
}

impl Verb {
    /// Whether this verb changes the buffer. `.` repeats changes, not motions.
    pub fn modifies(self) -> bool {
        matches!(
            self,
            Verb::Delete | Verb::Change | Verb::Insert | Verb::Paste | Verb::Repeat
        )
    }
}

impl Action {
    pub fn new(verb: Verb, object: Object, count: u32) -> Self {
        Self {
            verb,
            object,
            count: count.max(1),
            register: None,
        }
    }

    /// A mode reports this when it has no meaning for the action, so the
    /// statusline can say so. Silently doing nothing is the failure users
    /// cannot debug.
    pub fn describe(&self) -> String {
        let count = if self.count > 1 {
            format!("{} ", self.count)
        } else {
            String::new()
        };
        format!("{count}{:?} · {:?}", self.verb, self.object)
    }
}

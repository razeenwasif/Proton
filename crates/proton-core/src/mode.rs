//! Workspace modes.
//!
//! A mode is the unit of "which application am I in". M0 carries only the
//! identity and the verb-support question; the four view slots arrive with the
//! mode-plugin work in M6.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::action::{Action, Verb};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum ModeId {
    Chat,
    #[default]
    Codex,
    Design,
    Modeling,
    Game,
    Video,
    Data,
}

impl ModeId {
    /// Ordered as the rail shows them, which is also the `1`–`7` binding.
    pub const ALL: [ModeId; 7] = [
        ModeId::Chat,
        ModeId::Codex,
        ModeId::Design,
        ModeId::Modeling,
        ModeId::Game,
        ModeId::Video,
        ModeId::Data,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            ModeId::Chat => "chat",
            ModeId::Codex => "codex",
            ModeId::Design => "design",
            ModeId::Modeling => "modeling",
            ModeId::Game => "game",
            ModeId::Video => "video",
            ModeId::Data => "data",
        }
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        ModeId::ALL.into_iter().find(|m| m.slug() == s)
    }

    /// `1`–`7`, one-indexed as the user sees them.
    pub fn from_index(n: usize) -> Option<Self> {
        ModeId::ALL.get(n.checked_sub(1)?).copied()
    }

    /// Whether this mode has a meaning for the action.
    ///
    /// A mode that cannot implement a verb must say so — the statusline reports
    /// it, because silently doing nothing is the failure users cannot debug.
    /// Chat is the deliberate outlier: its noun is prose, so the editing verbs
    /// belong to the composer rather than to the mode.
    pub fn supports(self, action: &Action) -> bool {
        match self {
            ModeId::Chat => matches!(action.verb, Verb::Move | Verb::Undo | Verb::Redo),
            _ => true,
        }
    }
}

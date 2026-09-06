//! Workspace modes.
//!
//! A mode is the unit of "which application am I in". This is its identity;
//! its four slots, verb table and document type live in `contract`, which is
//! where the answers to `supports` and `layout` actually come from.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::action::Action;

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
    /// Asked of the mode's own contract rather than answered here, so there is
    /// one verb table and not two that can drift apart.
    pub fn supports(self, action: &Action) -> bool {
        crate::contract::Layout::of(self).supports(action)
    }

    /// The four slots, the verb table and the document — see `contract`.
    pub fn layout(self) -> crate::contract::Layout {
        crate::contract::Layout::of(self)
    }
}

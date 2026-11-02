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
    #[default]
    Codex,
    Data,
    /// A team of AI researchers with a scientist in the loop: co-scientist's
    /// rounds over an investigation's files, and a Lab that runs their code.
    Research,
    /// Notebooks of one's own sources, to study from: a tutor that answers
    /// from them and cites them, and what it makes of them — guides, cards,
    /// quizzes, exams. "Notebook" on the rail; Data's Jupyter notebooks keep
    /// the word in the code.
    Study,
    /// A spreadsheet: workbooks of sheets, formulas worked out as they are
    /// typed, sorting, filters, colour and charts. "Excel" on the rail;
    /// `sheet` in the code.
    Sheet,
    /// Digital logic: gates, flip-flops, memories and buses drawn, run and
    /// checked against test tables. "Circuit" on the rail.
    Circuit,
    /// A logic calculator and puzzle solver: truth tables, normal forms,
    /// models of theories, proofs. "Logic" on the rail.
    Logic,
}

impl ModeId {
    /// Ordered as the keys bind them, `1`–`7`: Data and Research beside
    /// Codex, the three a working day moves between, Notebook after
    /// Research, Excel fifth, Circuit sixth and Logic seventh. (Chat was the
    /// first, until Fairy took conversations over; Design, Model, Game, Video,
    /// Canvas and Diffuse, and LaTeX and the Reader once among them, followed
    /// Notebook until they moved to apps of their own.)
    pub const ALL: [ModeId; 7] = [ModeId::Codex, ModeId::Data, ModeId::Research, ModeId::Study, ModeId::Sheet, ModeId::Circuit, ModeId::Logic];

    pub fn slug(self) -> &'static str {
        match self {
            ModeId::Codex => "codex",
            ModeId::Data => "data",
            ModeId::Research => "research",
            ModeId::Study => "study",
            ModeId::Sheet => "sheet",
            ModeId::Circuit => "circuit",
            ModeId::Logic => "logic",
        }
    }

    /// Its place in `ALL`, from 0.
    pub fn index(self) -> usize {
        ModeId::ALL.iter().position(|m| *m == self).unwrap_or(0)
    }

    /// What the rail calls it.
    pub fn label(self) -> &'static str {
        match self {
            ModeId::Codex => "Codex",
            ModeId::Data => "Data",
            ModeId::Research => "Research",
            ModeId::Study => "Notebook",
            ModeId::Sheet => "Excel",
            ModeId::Circuit => "Circuit",
            ModeId::Logic => "Logic",
        }
    }

    /// Whether text is typed here into the editor's buffers: Codex's.
    pub fn edits_text(self) -> bool {
        matches!(self, ModeId::Codex)
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

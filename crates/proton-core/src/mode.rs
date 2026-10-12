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
    Data,
    /// A team of AI researchers with a scientist in the loop: co-scientist's
    /// rounds over an investigation's files, and a Lab that runs their code.
    Research,
    /// Notebooks of one's own sources, to study from: a tutor that answers
    /// from them and cites them, and what it makes of them — guides, cards,
    /// quizzes, exams. "Notebook" on the rail; Data's Jupyter notebooks keep
    /// the word in the code.
    Study,
    Design,
    Modeling,
    Game,
    Video,
    /// LaTeX, as Overleaf has it: Codex's own editor over the project's
    /// `.tex` files, beside the PDF they make, the build's log and the agent.
    /// "LaTeX" on the rail.
    Tex,
    /// PDFs, read and edited as Acrobat does: pages, comments, forms,
    /// passwords, combining and splitting, OCR — held until saved. The
    /// person's, whichever project is open. "PDF" on the rail.
    Pdf,
}

impl ModeId {
    /// Ordered as the keys bind them, `1`–`9`, `0` for the tenth and `-` for
    /// the eleventh: Data and Research beside Codex, the three a working day
    /// moves between, Notebook after Research, then LaTeX and PDF. The rail
    /// and the title bar draw LaTeX and PDF beside Notebook, the reading and
    /// writing modes together, without moving their keys.
    pub const ALL: [ModeId; 11] = [
        ModeId::Chat,
        ModeId::Codex,
        ModeId::Data,
        ModeId::Research,
        ModeId::Study,
        ModeId::Design,
        ModeId::Modeling,
        ModeId::Game,
        ModeId::Video,
        ModeId::Tex,
        ModeId::Pdf,
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
            ModeId::Research => "research",
            ModeId::Study => "study",
            ModeId::Tex => "tex",
            ModeId::Pdf => "pdf",
        }
    }

    /// Whether text is typed here into the editor's buffers: Codex's, and
    /// LaTeX's, which is Codex's editor beside a PDF.
    pub fn edits_text(self) -> bool {
        matches!(self, ModeId::Codex | ModeId::Tex)
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        ModeId::ALL.into_iter().find(|m| m.slug() == s)
    }

    /// `1`–`11`, one-indexed as the user sees them — `10` is `0` on the keys
    /// and `11` is `-`.
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

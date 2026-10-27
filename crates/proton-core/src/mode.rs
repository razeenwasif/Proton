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
    /// Pixel art and painting: layered pictures drawn with a pencil from a
    /// palette or with pressure-led brushes, and grown with seeded
    /// generators. "Canvas" on the rail; `raster` in the code, where "paint"
    /// and "canvas" already mean drawing a frame.
    Raster,
    /// Image generation by node graph, a small ComfyUI: graphs built here
    /// and run whole on Nexus's image server on forge. "Diffuse" on the rail.
    Diffuse,
}

impl ModeId {
    /// Ordered as the keys bind them, `1`–`9`, `0` for the tenth and `-`
    /// for the eleventh: Data and Research beside Codex, the three a working
    /// day moves between, Notebook after Research, then Canvas, then
    /// Diffuse. The rail and the title bar draw Canvas and Diffuse beside
    /// Game, which they make pictures for — without moving their keys.
    /// (LaTeX and the Reader were the tenth and eleventh until they moved
    /// to Quark.)
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
        ModeId::Raster,
        ModeId::Diffuse,
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
            ModeId::Raster => "raster",
            ModeId::Diffuse => "diffuse",
        }
    }

    /// Its place in `ALL`, from 0.
    pub fn index(self) -> usize {
        ModeId::ALL.iter().position(|m| *m == self).unwrap_or(0)
    }

    /// What the rail calls it.
    pub fn label(self) -> &'static str {
        match self {
            ModeId::Chat => "Chat",
            ModeId::Codex => "Codex",
            ModeId::Data => "Data",
            ModeId::Research => "Research",
            ModeId::Study => "Notebook",
            ModeId::Design => "Design",
            ModeId::Modeling => "Model",
            ModeId::Game => "Game",
            ModeId::Video => "Video",
            ModeId::Raster => "Canvas",
            ModeId::Diffuse => "Diffuse",
        }
    }

    /// Whether text is typed here into the editor's buffers: Codex's.
    pub fn edits_text(self) -> bool {
        matches!(self, ModeId::Codex)
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        ModeId::ALL.into_iter().find(|m| m.slug() == s)
    }

    /// `1`–`11`, one-indexed as the user sees them — `10` is `0` on the
    /// keys and `11` is `-`.
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

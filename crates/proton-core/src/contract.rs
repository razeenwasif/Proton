//! The mode contract: four slots, a verb table, and a document.
//!
//! A mode is the unit of "which application am I in", and the claim the whole
//! project rests on is that eleven of them can share one editor because a mode
//! is a small, closed thing:
//!
//! ```text
//! context pane (left) │ stage (centre) │ dock (right)
//!                     verbs, document
//! ```
//!
//! Data mode is the first that is not a text editor, which is the point of
//! writing this down now rather than later: a contract that only ever
//! described editors would not have been a contract, it would have been the
//! editor with extra words.
//!
//! ## What a slot holds is named, not drawn
//!
//! `proton-core` names no widget, and that rule does not bend here. A [`Slot`]
//! says *what belongs* in a position — the file tree, the editor, a
//! notebook's cells — and the front end knows how to draw each one. A closed
//! enum rather than a string, so a front end that has not implemented a slot
//! fails to compile rather than rendering a blank rectangle.
//!
//! ## Verbs are what a mode can be asked to do
//!
//! `dd` in Codex deletes a line. In Data it deletes a cell. In Chat it does
//! nothing at all, because the noun is prose and the editing verbs belong to
//! the composer. A mode that cannot implement a verb has to say so — the
//! statusline reports it, because silently doing nothing is the failure users
//! cannot debug.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::action::{Action, Verb};
use crate::mode::ModeId;

/// What belongs in one position of a mode's layout.
///
/// Named rather than drawn: this says *what*, and the front end decides how.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum Slot {
    /// Nothing here. A mode with an empty slot gets the space back rather
    /// than an empty panel, which is the difference between a layout and a
    /// frame with a hole in it.
    Empty,
    /// The project's files.
    Explorer,
    /// Text, in panes, with a cursor.
    Editor,
    /// A notebook: cells, their outputs, and what the kernel is doing.
    Cells,
    /// The agent, and what it has staged.
    Agent,
    /// Everything the dock holds today — agent, tasks, debug, git, terminal.
    Panels,
    /// A single table, large enough to read.
    Table,
    /// Layers, for the modes that have them.
    Layers,
    /// Properties of whatever is selected.
    Inspector,
    /// A project's investigations.
    Investigations,
    /// One investigation: its overview, its hypotheses ranked, the round's work.
    Board,
    /// Runs of code, and what they drew.
    Lab,
    /// A notebook's sources, ticked for the tutor to use, and its notes.
    Sources,
    /// The tutor: a conversation grounded in the sources, and what is open
    /// beside it — a source, a note, something made.
    Tutor,
    /// What can be made from the sources, and what has been.
    Studio,
    /// What the source typesets to: the PDF, the build's log, the agent.
    Typeset,
    /// A PDF's pages small, its bookmarks, its comments and what a search
    /// found in it.
    Thumbnails,
    /// A PDF's pages, to read, mark and fill in.
    Pages,
    /// What can be done to a PDF — its pages, its forms, its security, OCR —
    /// and the agent.
    PdfTools,
}

/// How a mode's work persists.
///
/// **Every one of these is a text file that diffs in Git** — but one. That
/// is the load-bearing rule of the project rather than a matter of taste: it
/// is what lets the agent edit a scene or a timeline by writing a patch
/// instead of driving a GUI, and what makes any of it reviewable. The one is
/// the PDF, which is the person's to read and mark rather than the project's
/// to write: it is changed through the core's own operations, each undoable,
/// and saved as an update appended to the file, its first bytes kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum DocumentKind {
    /// `.proton/chats/*.json`
    Conversation,
    /// Whatever the project is written in.
    Source,
    /// `.ipynb` and `.sql`.
    Notebook,
    /// Component source plus `theme.css` tokens.
    Component,
    /// `scene.proton.json`
    Scene,
    /// A game's folder: `game.json`, `world.json`, a `.room.json` per room
    /// and a `.prefab.json` per prefab — Proton's own engine's files.
    Level,
    /// An `.edl` and its sidecar captions.
    Timeline,
    /// `research/<slug>/`: Markdown hypotheses and reviews, a JSON-lines
    /// tournament, the plan and the logs.
    Investigation,
    /// `~/Github/Notebooks/<slug>/`: the notebook, its sources and notes in
    /// JSON and Markdown, its chats, and what the studio made.
    StudyNotebook,
    /// `.tex` and `.bib`: a LaTeX project, whose PDF is built outside it.
    Manuscript,
    /// A `.pdf`, anywhere: the one document that is not text.
    Pdf,
}

/// The whole of a mode, as anything outside it needs to know.
///
/// A struct rather than a trait object: every field is data, none of it needs
/// to run, and a mode that is a value can be compared, sent over the bus and
/// written down in a test. The trait in the design document described the
/// same thing in a language where the answers might have been computed; here
/// they are not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub mode: ModeId,
    pub context: Slot,
    pub stage: Slot,
    pub dock: Slot,
    pub document: DocumentKind,
}

impl Layout {
    /// What a mode is made of.
    pub fn of(mode: ModeId) -> Layout {
        let (context, stage, dock, document) = match mode {
            // The composer is the stage, and the conversation list is what
            // you pick from. No dock: a chat with a third column is a chat
            // competing with itself for attention.
            ModeId::Chat => (
                Slot::Explorer,
                Slot::Agent,
                Slot::Empty,
                DocumentKind::Conversation,
            ),
            ModeId::Codex => (
                Slot::Explorer,
                Slot::Editor,
                Slot::Panels,
                DocumentKind::Source,
            ),
            // The first layout that is not an editor's. The stage is the
            // notebook; the dock is where a table goes, because a dataframe
            // wants width and reading a table in a column the width of a
            // sidebar is not reading it.
            ModeId::Data => (
                Slot::Explorer,
                Slot::Cells,
                Slot::Table,
                DocumentKind::Notebook,
            ),
            ModeId::Design => (
                Slot::Layers,
                Slot::Editor,
                Slot::Inspector,
                DocumentKind::Component,
            ),
            ModeId::Modeling => (
                Slot::Layers,
                Slot::Editor,
                Slot::Inspector,
                DocumentKind::Scene,
            ),
            ModeId::Game => (
                Slot::Layers,
                Slot::Editor,
                Slot::Inspector,
                DocumentKind::Level,
            ),
            ModeId::Video => (
                Slot::Explorer,
                Slot::Editor,
                Slot::Inspector,
                DocumentKind::Timeline,
            ),
            // The investigations to pick from, the one picked, and the Lab
            // beside it: what a run drew wants to be seen next to the
            // hypothesis it tests.
            ModeId::Research => (
                Slot::Investigations,
                Slot::Board,
                Slot::Lab,
                DocumentKind::Investigation,
            ),
            // The sources, the tutor that answers from them, and what can be
            // made of them — Gemini Notebook's three columns.
            ModeId::Study => (
                Slot::Sources,
                Slot::Tutor,
                Slot::Studio,
                DocumentKind::StudyNotebook,
            ),
            // Overleaf's: the files, the source, and what it typesets to.
            ModeId::Tex => (
                Slot::Explorer,
                Slot::Editor,
                Slot::Typeset,
                DocumentKind::Manuscript,
            ),
            // Acrobat's: the pages small, the pages, and the tools.
            ModeId::Pdf => (
                Slot::Thumbnails,
                Slot::Pages,
                Slot::PdfTools,
                DocumentKind::Pdf,
            ),
        };
        Layout {
            mode,
            context,
            stage,
            dock,
            document,
        }
    }

    /// Whether this mode has a meaning for the action.
    ///
    /// The verb table, in the only form that has ever been needed: a
    /// question. A mode that cannot implement a verb says so, and the
    /// statusline reports it — silently doing nothing is the failure users
    /// cannot debug.
    pub fn supports(&self, action: &Action) -> bool {
        match self.mode {
            // The noun is prose, so the editing verbs belong to the composer
            // rather than to the mode.
            // A PDF's pages are moved through and its changes undone; what is
            // done to them is done with the PDF's own tools.
            ModeId::Chat | ModeId::Research | ModeId::Study | ModeId::Pdf => matches!(action.verb, Verb::Move | Verb::Undo | Verb::Redo),
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_has_a_layout_and_a_document() {
        // The contract is only a contract if it covers all eleven. A mode
        // added later fails to compile rather than falling through to a
        // default that puts an editor where its stage should be.
        for mode in ModeId::ALL {
            let layout = Layout::of(mode);
            assert_eq!(layout.mode, mode);
        }
    }

    #[test]
    fn data_mode_is_not_an_editor_wearing_a_hat() {
        // The claim M6 exists to test. If Data's layout were Codex's, the
        // contract would have described the editor rather than a mode.
        let data = Layout::of(ModeId::Data);
        let codex = Layout::of(ModeId::Codex);
        assert_ne!(data.stage, codex.stage);
        assert_ne!(data.dock, codex.dock);
        assert_ne!(data.document, codex.document);
        assert_eq!(data.stage, Slot::Cells);
        assert_eq!(data.document, DocumentKind::Notebook);
    }

    #[test]
    fn a_mode_that_cannot_do_a_verb_says_so() {
        use crate::action::{Object, Verb};
        let delete = Action {
            verb: Verb::Delete,
            object: Object::Line,
            count: 1,
            register: None,
        };
        assert!(!Layout::of(ModeId::Chat).supports(&delete));
        assert!(Layout::of(ModeId::Codex).supports(&delete));
        assert!(Layout::of(ModeId::Data).supports(&delete));
    }

    #[test]
    fn an_empty_slot_is_a_real_answer() {
        // Chat has no dock on purpose. A mode with nothing to put in a slot
        // should get the space back rather than an empty panel.
        assert_eq!(Layout::of(ModeId::Chat).dock, Slot::Empty);
    }
}

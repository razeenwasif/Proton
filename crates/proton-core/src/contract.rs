//! The mode contract: four slots, a verb table, and a document.
//!
//! A mode is the unit of "which application am I in", and the claim the whole
//! project rests on is that eight of them can share one editor because a mode
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
//! `dd` in Codex deletes a line. In Data it deletes a cell. In Research it does
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
    /// A project's investigations.
    Investigations,
    /// One investigation: its overview, its hypotheses ranked, the round's work.
    Board,
    /// Runs of code, and what they drew.
    Lab,
    /// A sheet's cells, by rows and columns, with the formula bar above and
    /// the sheets' tabs below.
    Grid,
    /// A circuit drawn on its grid, the project's circuits beside it.
    Schematic,
    /// A logic file as typed, and what the statement at the cursor is.
    Worksheet,
    /// A part's sketches and features in order, each step's mistake marked.
    History,
    /// A part in 3D, or a sketch flat on its plane.
    Viewport,
    /// The chosen step's settings, and the part's text.
    Inspector,
}

/// How a mode's work persists.
///
/// **Every one of these is a text file that diffs in Git.** That is the
/// load-bearing rule of the project rather than a matter of taste: it is what
/// lets the agent edit a notebook or an investigation by writing a patch
/// instead of driving a GUI, and what makes any of it reviewable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum DocumentKind {
    /// Whatever the project is written in.
    Source,
    /// `.ipynb` and `.sql`.
    Notebook,
    /// `research/<slug>/`: Markdown hypotheses and reviews, a JSON-lines
    /// tournament, the plan and the logs.
    Investigation,
    /// `Name.sheet/`: `workbook.json` and a `.cells` file a sheet, a line a
    /// cell. `.xlsx`, `.csv` and `.tsv` are opened and saved too.
    Workbook,
    /// `Name.circuit/`: `project.json`, a `.circ` file a circuit, a line a
    /// part or wire, and a `.tests` file a test table.
    CircuitProject,
    /// `.logic` theories and worksheets, `.proof` proofs and Logic4Fun's
    /// `.l4f`: a statement a line.
    LogicFile,
    /// `Name.part`: parameters, sketches and features, a statement a line.
    Part,
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
            // The investigations to pick from, the one picked, and the Lab
            // beside it: what a run drew wants to be seen next to the
            // hypothesis it tests.
            ModeId::Research => (
                Slot::Investigations,
                Slot::Board,
                Slot::Lab,
                DocumentKind::Investigation,
            ),
            // The project's workbooks, the grid, and the agent with its
            // sheet tools beside it — shut until asked for, as the grid
            // wants every column it can have.
            ModeId::Sheet => (
                Slot::Explorer,
                Slot::Grid,
                Slot::Agent,
                DocumentKind::Workbook,
            ),
            // The project's circuits, the drawing, and the agent with its
            // circuit tools beside it — shut until asked for, as the drawing
            // wants its room.
            ModeId::Circuit => (
                Slot::Explorer,
                Slot::Schematic,
                Slot::Agent,
                DocumentKind::CircuitProject,
            ),
            // The project's logic files and the one open, with what the line
            // at the cursor is beside it; and the agent with its logic
            // tools — shut until asked for.
            ModeId::Logic => (
                Slot::Explorer,
                Slot::Worksheet,
                Slot::Agent,
                DocumentKind::LogicFile,
            ),
            // The part's sketches and features in order, the part in 3D —
            // or a sketch flat on its plane — and the step's settings.
            ModeId::Cad => (
                Slot::History,
                Slot::Viewport,
                Slot::Inspector,
                DocumentKind::Part,
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
            ModeId::Research | ModeId::Sheet | ModeId::Circuit | ModeId::Logic | ModeId::Cad => matches!(action.verb, Verb::Move | Verb::Undo | Verb::Redo),
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_has_a_layout_and_a_document() {
        // The contract is only a contract if it covers all eight. A mode
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
        assert!(!Layout::of(ModeId::Research).supports(&delete));
        assert!(Layout::of(ModeId::Codex).supports(&delete));
        assert!(Layout::of(ModeId::Data).supports(&delete));
    }
}

//! Two panes onto one notebook.
//!
//! A split in Codex is a second window onto a buffer, with its own cursor. A
//! split in Data is the same idea with the mode's own noun: a second window
//! onto the notebook, with its own *cell*. The notebook is not copied and the
//! kernel is not doubled — there is one document and one kernel, and the panes
//! disagree only about where they are looking.
//!
//! The rule being tested is the one `View` already states for cursors: the
//! focused view's position lives on the mode, and the unfocused views hold
//! their own. Getting that backwards shows up as a pane that snaps back to
//! wherever it was when it last lost focus.

use proton_core::bus::Event;
use proton_core::split::Direction;
use proton_core::{Command, Key, ModeId, Workspace};

const NOTEBOOK: &str = r##"{
 "cells": [
  {"cell_type": "code", "execution_count": null, "metadata": {}, "outputs": [], "source": ["one"]},
  {"cell_type": "code", "execution_count": null, "metadata": {}, "outputs": [], "source": ["two"]},
  {"cell_type": "code", "execution_count": null, "metadata": {}, "outputs": [], "source": ["three"]},
  {"cell_type": "code", "execution_count": null, "metadata": {}, "outputs": [], "source": ["four"]}
 ],
 "metadata": {},
 "nbformat": 4,
 "nbformat_minor": 4
}
"##;

fn open(name: &str) -> Workspace {
    let root = std::env::temp_dir().join(format!(
        "proton-dsplit-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("n.ipynb"), NOTEBOOK).unwrap();

    let mut space = Workspace::rooted(&root);
    space.apply(Command::OpenBuffer {
        path: "n.ipynb".into(),
    });
    assert_eq!(space.mode(), ModeId::Data);
    space
}

fn keys(space: &mut Workspace, text: &str) {
    for c in text.chars() {
        space.apply(Command::Key { key: Key::char(c) });
    }
}

/// Which cell each pane is on, and which pane has the focus.
fn panes(space: &mut Workspace) -> (Vec<usize>, usize) {
    space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Splits { views, focused, .. } => {
                Some((views.iter().map(|v| v.cell).collect(), *focused))
            }
            _ => None,
        })
        .expect("a splits event")
}

/// The cell the focused pane is on, as the notebook reports it.
fn selected(space: &mut Workspace) -> usize {
    space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Notebook { cells, .. } => cells.iter().position(|c| c.selected),
            _ => None,
        })
        .expect("a selected cell")
}

#[test]
fn splitting_opens_a_second_pane_where_the_first_was_looking() {
    // You split because you want two of what you are already looking at.
    let mut space = open("split");
    keys(&mut space, "jj"); // to cell 2
    space.apply(Command::Split { vertical: true });

    let (cells, focused) = panes(&mut space);
    assert_eq!(cells.len(), 2, "there should be two panes");
    assert_eq!(cells, [2, 2], "both start where the split happened");
    assert_eq!(focused, 1, "the new pane takes the focus, as :vsplit does");
}

#[test]
fn the_panes_hold_their_own_cell() {
    // The whole point. Move in one pane, and the other stays put.
    let mut space = open("independent");
    space.apply(Command::Split { vertical: true });
    keys(&mut space, "jjj"); // the focused pane walks to the last cell

    let (cells, focused) = panes(&mut space);
    assert_eq!(focused, 1);
    assert_eq!(
        cells,
        [0, 3],
        "the pane that was not moved should not have moved"
    );
}

#[test]
fn focus_carries_the_selection_with_it() {
    // Going back to a pane puts you where you left it, not where the other
    // pane happens to be.
    let mut space = open("carry");
    keys(&mut space, "j"); // pane 0 sits on cell 1
    space.apply(Command::Split { vertical: true });
    keys(&mut space, "jj"); // pane 1 walks to cell 3
    assert_eq!(selected(&mut space), 3);

    space.apply(Command::StepSplit {
        direction: Direction::Previous,
    });
    assert_eq!(selected(&mut space), 1, "back to where pane 0 was");

    space.apply(Command::StepSplit {
        direction: Direction::Next,
    });
    assert_eq!(selected(&mut space), 3, "and pane 1 kept its place");
}

#[test]
fn one_notebook_and_one_kernel_behind_both_panes() {
    // Splitting must not fork the document. Editing through one pane is
    // visible through the other, because there is only one notebook.
    let mut space = open("shared");
    space.apply(Command::Split { vertical: true });
    keys(&mut space, "dd"); // delete a cell from the focused pane

    let cells = space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Notebook { cells, .. } => Some(cells.len()),
            _ => None,
        })
        .expect("a notebook event");
    assert_eq!(cells, 3, "the deletion is the document's, not the pane's");

    // And both panes still describe that same shorter notebook.
    let (per_pane, _) = panes(&mut space);
    assert!(
        per_pane.iter().all(|&cell| cell < 3),
        "a pane pointing past the end panics later: {per_pane:?}"
    );
}

#[test]
fn a_pane_whose_cell_was_deleted_lands_somewhere_real() {
    // The unfocused pane parked cell 3. While it was away the notebook got
    // shorter, so the index it parked no longer exists.
    let mut space = open("deleted");
    keys(&mut space, "jjj"); // pane 0 to the last cell
    space.apply(Command::Split { vertical: true });
    space.apply(Command::StepSplit {
        direction: Direction::Previous,
    });
    // Now pane 1 is parked on cell 3. Delete from the front until it is gone.
    keys(&mut space, "gg");
    keys(&mut space, "dd");
    keys(&mut space, "dd");

    space.apply(Command::StepSplit {
        direction: Direction::Next,
    });
    let landed = selected(&mut space);
    assert!(landed < 2, "clamped to a cell that exists, got {landed}");
}

#[test]
fn closing_a_split_leaves_the_survivor_where_it_was() {
    let mut space = open("close");
    keys(&mut space, "j"); // pane 0 on cell 1
    space.apply(Command::Split { vertical: true });
    keys(&mut space, "jj"); // pane 1 on cell 3
    space.apply(Command::CloseSplit { index: None });

    let (cells, focused) = panes(&mut space);
    assert_eq!(cells.len(), 1);
    assert_eq!(focused, 0);
    assert_eq!(
        selected(&mut space),
        1,
        "the pane that survived kept its cell"
    );
}

#[test]
fn splitting_in_codex_does_not_disturb_the_notebook() {
    // Parking happens in every mode. It must not move the Data selection as a
    // side effect of splitting a text buffer.
    let mut space = open("codex");
    keys(&mut space, "jj");
    space.apply(Command::SetMode {
        mode: ModeId::Codex,
    });
    space.apply(Command::Split { vertical: true });
    space.apply(Command::SetMode { mode: ModeId::Data });
    assert_eq!(selected(&mut space), 2, "the notebook selection survived");
}

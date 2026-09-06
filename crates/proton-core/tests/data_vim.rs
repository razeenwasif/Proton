//! The verb table, where it stops being a table.
//!
//! The mode contract's claim is that `dd` means "delete the mode's unit", not
//! "delete a line of text". Codex has always been the only mode that tested
//! it, which is to say it was never tested at all: a rule with one example is
//! a description of that example.
//!
//! These press the same keys against a notebook. Nothing in the modal engine
//! knows notebooks exist — `Object::Line` has always meant the mode's unit,
//! and that is what makes this routing rather than a second engine.

use proton_core::bus::Event;
use proton_core::data::CellView;
use proton_core::{Command, Key, ModeId, Workspace};

const NOTEBOOK: &str = r##"{
 "cells": [
  {
   "cell_type": "code",
   "execution_count": null,
   "metadata": {},
   "outputs": [],
   "source": [
    "one"
   ]
  },
  {
   "cell_type": "code",
   "execution_count": null,
   "metadata": {},
   "outputs": [],
   "source": [
    "two"
   ]
  },
  {
   "cell_type": "code",
   "execution_count": null,
   "metadata": {},
   "outputs": [],
   "source": [
    "three"
   ]
  }
 ],
 "metadata": {},
 "nbformat": 4,
 "nbformat_minor": 4
}
"##;

/// A workspace in Data mode with a three-cell notebook open.
fn open(name: &str) -> Workspace {
    let root = std::env::temp_dir().join(format!(
        "proton-dvim-{name}-{}-{:?}",
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
    assert_eq!(
        space.mode(),
        ModeId::Data,
        "opening a notebook is the switch"
    );
    space
}

/// Type keys, the way somebody would.
fn keys(space: &mut Workspace, text: &str) -> Vec<Event> {
    let mut events = Vec::new();
    for c in text.chars() {
        events = space.apply(Command::Key { key: Key::char(c) });
    }
    events
}

/// The cells as they are now.
fn cells(space: &mut Workspace) -> Vec<CellView> {
    space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Notebook { cells, .. } => Some(cells.clone()),
            _ => None,
        })
        .expect("a notebook event")
}

fn sources(cells: &[CellView]) -> Vec<String> {
    cells.iter().map(|c| c.source.clone()).collect()
}

fn selected(cells: &[CellView]) -> usize {
    cells.iter().position(|c| c.selected).unwrap_or(usize::MAX)
}

#[test]
fn j_and_k_move_a_cell_at_a_time() {
    let mut space = open("move");
    assert_eq!(selected(&cells(&mut space)), 0);

    keys(&mut space, "j");
    assert_eq!(selected(&cells(&mut space)), 1);
    keys(&mut space, "j");
    assert_eq!(selected(&cells(&mut space)), 2);
    // And it stops at the end rather than running off it.
    keys(&mut space, "jjj");
    assert_eq!(selected(&cells(&mut space)), 2);

    keys(&mut space, "k");
    assert_eq!(selected(&cells(&mut space)), 1);
    keys(&mut space, "kkk");
    assert_eq!(selected(&cells(&mut space)), 0);
}

#[test]
fn a_count_works_the_way_it_does_everywhere_else() {
    // `2j` is two cells, because the count is resolved by the same engine
    // that resolves it for text. Nothing here had to implement counting.
    let mut space = open("count");
    keys(&mut space, "2j");
    assert_eq!(selected(&cells(&mut space)), 2);
}

#[test]
fn dd_deletes_the_cell_the_cursor_is_on() {
    let mut space = open("dd");
    keys(&mut space, "j");
    keys(&mut space, "dd");

    let after = cells(&mut space);
    assert_eq!(sources(&after), ["one", "three"]);
    // The cursor stays put and the cells move up under it, as `dd` does.
    assert_eq!(selected(&after), 1);
}

#[test]
fn dd_at_the_end_steps_back_rather_than_pointing_at_nothing() {
    let mut space = open("ddend");
    keys(&mut space, "jj");
    keys(&mut space, "dd");

    let after = cells(&mut space);
    assert_eq!(sources(&after), ["one", "two"]);
    assert_eq!(
        selected(&after),
        1,
        "a cursor past the end is a panic later"
    );
}

#[test]
fn yy_and_p_move_a_cell_around() {
    let mut space = open("yank");
    keys(&mut space, "yy"); // the first cell
    keys(&mut space, "jj"); // to the last
    keys(&mut space, "p"); // after it

    let after = cells(&mut space);
    assert_eq!(sources(&after), ["one", "two", "three", "one"]);
    assert_eq!(selected(&after), 3, "on what was just pasted");
}

#[test]
fn capital_p_puts_it_before_rather_than_after() {
    let mut space = open("Paste");
    keys(&mut space, "yy");
    keys(&mut space, "j");
    keys(&mut space, "P");
    assert_eq!(sources(&cells(&mut space)), ["one", "one", "two", "three"]);
}

#[test]
fn what_dd_took_is_what_p_puts_back() {
    // The register is shared between the two, exactly as in vim: `dd` then
    // `p` is how you move a cell, and it would be a strange notebook where it
    // was not.
    let mut space = open("move-cell");
    keys(&mut space, "dd"); // takes "one"
    keys(&mut space, "j"); // now on "three"
    keys(&mut space, "p");
    assert_eq!(sources(&cells(&mut space)), ["two", "three", "one"]);
}

#[test]
fn o_and_upper_o_add_a_cell_below_and_above() {
    let mut space = open("open");
    keys(&mut space, "j");
    keys(&mut space, "o");
    let after = cells(&mut space);
    assert_eq!(sources(&after), ["one", "two", "", "three"]);
    assert_eq!(selected(&after), 2, "on the new one");

    let mut space = open("Open");
    keys(&mut space, "j");
    keys(&mut space, "O");
    let after = cells(&mut space);
    assert_eq!(sources(&after), ["one", "", "two", "three"]);
    assert_eq!(selected(&after), 1);
}

#[test]
fn gg_and_upper_g_go_to_the_ends() {
    let mut space = open("ends");
    keys(&mut space, "G");
    assert_eq!(selected(&cells(&mut space)), 2);
    keys(&mut space, "gg");
    assert_eq!(selected(&cells(&mut space)), 0);
}

#[test]
fn a_verb_the_mode_has_no_meaning_for_is_reported_rather_than_ignored() {
    // The rule the contract states: a mode that cannot do something says so.
    // Silently doing nothing is the failure users cannot debug.
    let mut space = open("unsupported");
    let events = keys(&mut space, "x"); // delete a character — of what?
    let acted = events
        .iter()
        .any(|event| matches!(event, Event::Action { .. } | Event::Unsupported { .. }));
    assert!(acted, "{events:?}");
    // Whatever it decided, it did not quietly mangle the notebook.
    assert_eq!(sources(&cells(&mut space)), ["one", "two", "three"]);
}

#[test]
fn none_of_this_touches_the_text_editor() {
    // The notebook is not a buffer, so a `dd` here must not have deleted a
    // line of whatever Codex had open.
    let mut space = open("separate");
    let before = space.editor().buffer().text().to_string();
    keys(&mut space, "dd");
    keys(&mut space, "p");
    assert_eq!(space.editor().buffer().text().to_string(), before);
}

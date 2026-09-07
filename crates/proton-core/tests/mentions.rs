//! `@selection` and `@dataframe`, end to end.
//!
//! The design's phrasing is the specification: *what the model can see is a
//! chip you can point at*. A question about "this" is one the model answers by
//! guessing, and letting it grep until something looks right is slower, costs
//! more, and is wrong often enough to matter.
//!
//! What is checked here is that the context reaches the conversation — not
//! that the parser finds the word, which is `mention`'s own business. The
//! conversation is the thing that gets sent, so it is the thing to assert on.

use proton_core::{Command, Workspace};

const NOTEBOOK: &str = r##"{
 "cells": [
  {
   "cell_type": "code",
   "execution_count": 1,
   "metadata": {},
   "outputs": [
    {
     "output_type": "execute_result",
     "execution_count": 1,
     "metadata": {},
     "data": {
      "text/html": [
       "<table><thead><tr><th>sat_id</th><th>battery</th></tr></thead><tbody><tr><td>17</td><td>3.41</td></tr><tr><td>31</td><td>3.58</td></tr></tbody></table>"
      ],
      "text/plain": [
       "   sat_id  battery"
      ]
     }
    }
   ],
   "source": [
    "df.groupby('sat_id').battery.mean()"
   ]
  }
 ],
 "metadata": {},
 "nbformat": 4,
 "nbformat_minor": 5
}
"##;

fn open(name: &str) -> (Workspace, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "proton-mention-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("n.ipynb"), NOTEBOOK).unwrap();
    std::fs::write(
        root.join("lib.rs"),
        "fn one() {\n    1\n}\n\nfn two() {\n    2\n}\n",
    )
    .unwrap();
    (Workspace::rooted(&root), root)
}

/// What was actually sent to the model.
///
/// The conversation rather than the transcript: the transcript is what gets
/// drawn, and a context block that only reached the drawing would be a chip
/// that lied about what the model could see.
fn sent(space: &Workspace) -> String {
    format!("{:?}", space.agent().conversation())
}

/// Ask, without needing a model to answer.
fn ask(space: &mut Workspace, text: &str) {
    space.apply(Command::Ask { text: text.into() });
}

#[test]
fn a_selection_reaches_the_model() {
    let (mut space, _) = open("selection");
    space.apply(Command::OpenBuffer {
        path: "lib.rs".into(),
    });
    // Select the first function: `v` then down twice, which is what a person
    // does rather than a range poked into the buffer.
    for key in "Vjj".chars() {
        space.apply(Command::Key {
            key: proton_core::Key::char(key),
        });
    }
    ask(&mut space, "what does @selection do?");

    let out = sent(&space);
    assert!(out.contains("what does @selection do?"), "{out}");
    assert!(
        out.contains("fn one()"),
        "the selection was not sent: {out}"
    );
    // And it says where it came from: a fragment with no file and no lines is
    // one the model cannot go back to.
    assert!(out.contains("lib.rs"), "{out}");
}

#[test]
fn the_words_that_were_typed_are_kept() {
    // `@selection` is not replaced by the text. The transcript keeps what was
    // written, or the reader cannot tell which half of the sentence is theirs.
    let (mut space, _) = open("verbatim");
    space.apply(Command::OpenBuffer {
        path: "lib.rs".into(),
    });
    for key in "Vj".chars() {
        space.apply(Command::Key {
            key: proton_core::Key::char(key),
        });
    }
    ask(&mut space, "explain @selection briefly");

    let out = sent(&space);
    assert!(out.contains("explain @selection briefly"), "{out}");
}

#[test]
fn a_selection_that_is_not_there_says_so_rather_than_going_quiet() {
    // Silence would leave the model answering about whatever it could find,
    // and the user believing it had been shown something.
    let (mut space, _) = open("noselection");
    space.apply(Command::OpenBuffer {
        path: "lib.rs".into(),
    });
    ask(&mut space, "what about @selection");

    let out = sent(&space);
    assert!(out.contains("Nothing is selected"), "{out}");
}

#[test]
fn a_dataframe_reaches_the_model_as_rows() {
    let (mut space, _) = open("dataframe");
    space.apply(Command::OpenBuffer {
        path: "n.ipynb".into(),
    });
    ask(&mut space, "summarise @dataframe");

    let out = sent(&space);
    assert!(out.contains("sat_id"), "the columns are missing: {out}");
    assert!(out.contains("3.41"), "the rows are missing: {out}");
}

#[test]
fn the_frames_real_shape_is_reported() {
    // A model told about the rows it was sent, with no mention of how many
    // there are, reasons about them as if they were all of it.
    let (mut space, _) = open("shape");
    space.apply(Command::OpenBuffer {
        path: "n.ipynb".into(),
    });
    ask(&mut space, "@dataframe");

    let out = sent(&space);
    assert!(out.contains("columns"), "{out}");
    assert!(out.contains("showing"), "{out}");
}

#[test]
fn a_dataframe_with_no_notebook_open_says_so() {
    let (mut space, _) = open("nonotebook");
    ask(&mut space, "look at @dataframe");
    assert!(
        sent(&space).contains("No notebook is open"),
        "{}",
        sent(&space)
    );
}

#[test]
fn a_cell_with_no_table_says_what_to_do_about_it() {
    let (mut space, root) = open("notable");
    std::fs::write(
        root.join("plain.ipynb"),
        r##"{"cells": [{"cell_type": "code", "execution_count": null,
             "metadata": {}, "outputs": [], "source": ["1 + 1"]}],
             "metadata": {}, "nbformat": 4, "nbformat_minor": 5}"##,
    )
    .unwrap();
    space.apply(Command::OpenBuffer {
        path: "plain.ipynb".into(),
    });
    ask(&mut space, "@dataframe");

    let out = sent(&space);
    assert!(out.contains("no table"), "{out}");
    assert!(
        out.contains("Run a cell"),
        "it should say what to do: {out}"
    );
}

#[test]
fn a_question_with_no_mention_is_sent_exactly_as_written() {
    // The common case, and the one where an accidental heading would be most
    // annoying.
    let (mut space, _) = open("plain");
    ask(&mut space, "why is the grid janking?");

    let out = sent(&space);
    assert!(out.contains("why is the grid janking?"), "{out}");
    assert!(!out.contains('#'), "something was appended: {out}");
}

#[test]
fn an_email_address_does_not_attach_a_file() {
    // The failure the word-boundary rule exists to prevent: a question about
    // email quietly sending the buffer's selection with it.
    let (mut space, _) = open("email");
    space.apply(Command::OpenBuffer {
        path: "lib.rs".into(),
    });
    ask(&mut space, "draft a note to bob@selection.com");

    let out = sent(&space);
    assert!(!out.contains("Nothing is selected"), "{out}");
    assert!(!out.contains("fn one()"), "{out}");
}

#[test]
fn both_mentions_can_be_asked_for_at_once() {
    let (mut space, _) = open("both");
    space.apply(Command::OpenBuffer {
        path: "n.ipynb".into(),
    });
    ask(&mut space, "does @selection agree with @dataframe?");

    let out = sent(&space);
    assert!(out.contains("@selection"), "{out}");
    assert!(out.contains("sat_id"), "the frame is missing: {out}");
}

//! The mode contract, through the seam the shell uses.
//!
//! `contract.rs` proves the table is complete; this proves the core actually
//! uses it — that switching modes rearranges the window, and that the
//! cross-mode jump lands where it said it would.

use proton_core::bus::Event;
use proton_core::contract::{DocumentKind, Slot};
use proton_core::{Command, ModeId, Workspace};

fn project(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "proton-contract-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn layout_in(events: &[Event]) -> Option<proton_core::contract::Layout> {
    events.iter().find_map(|event| match event {
        Event::Layout { layout } => Some(layout.clone()),
        _ => None,
    })
}

#[test]
fn switching_modes_rearranges_the_window() {
    // The point of the contract: a mode switch that only moved a highlight on
    // the rail would be a theme, not a mode.
    let mut space = Workspace::rooted(project("switch"));
    let layout = layout_in(&space.apply(Command::SetMode { mode: ModeId::Data }))
        .expect("a mode change carries its layout");

    assert_eq!(layout.mode, ModeId::Data);
    assert_eq!(layout.stage, Slot::Cells);
    assert_eq!(layout.dock, Slot::Table);
    assert_eq!(layout.document, DocumentKind::Notebook);

    let back = layout_in(&space.apply(Command::SetMode {
        mode: ModeId::Codex,
    }))
    .unwrap();
    assert_eq!(back.stage, Slot::Editor);
    assert_eq!(back.document, DocumentKind::Source);
}

#[test]
fn a_window_that_has_just_connected_is_told_the_layout() {
    // Without this the front end would not know which panels belong on screen
    // until the first time the user changed mode.
    let mut space = Workspace::rooted(project("sync"));
    assert!(layout_in(&space.apply(Command::Sync)).is_some());
}

#[test]
fn the_jump_arrives_in_the_right_mode_at_the_right_line() {
    // "open sat 17 in Codex". The three halves that have to happen together:
    // the mode changes, the file opens, and the cursor is on the line asked
    // for — not the top of the file, and not the file that was open before.
    let root = project("jump");
    let text: String = (0..40).map(|i| format!("line {i}\n")).collect();
    std::fs::write(root.join("data.py"), &text).unwrap();
    std::fs::write(root.join("other.txt"), "elsewhere\n").unwrap();

    let mut space = Workspace::rooted(&root);
    space.apply(Command::OpenBuffer {
        path: "other.txt".into(),
    });
    space.apply(Command::SetMode { mode: ModeId::Data });

    let events = space.apply(Command::JumpTo {
        mode: ModeId::Codex,
        path: "data.py".into(),
        line: 17,
        column: 2,
    });

    assert_eq!(
        layout_in(&events).map(|l| l.mode),
        Some(ModeId::Codex),
        "the jump should have changed mode"
    );
    assert!(
        space
            .editor()
            .buffer()
            .path()
            .is_some_and(|p| p.ends_with("data.py")),
        "the jump should have opened the file"
    );

    let cursor = events.iter().rev().find_map(|event| match event {
        Event::Cursor { line, column, .. } => Some((*line, *column)),
        _ => None,
    });
    assert_eq!(cursor, Some((17, 2)), "the cursor should be on line 17");
}

#[test]
fn jumping_to_the_mode_you_are_already_in_still_opens_the_file() {
    // The common case once Data mode has a table in it: both ends of the jump
    // can be Codex, and a no-op mode change must not swallow the rest.
    let root = project("same");
    std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    let mut space = Workspace::rooted(&root);

    space.apply(Command::JumpTo {
        mode: ModeId::Codex,
        path: "a.txt".into(),
        line: 2,
        column: 0,
    });
    assert!(space
        .editor()
        .buffer()
        .path()
        .is_some_and(|p| p.ends_with("a.txt")));
}

#[test]
fn a_jump_to_a_file_that_is_gone_says_so_rather_than_moving_the_cursor() {
    let mut space = Workspace::rooted(project("missing"));
    let events = space.apply(Command::JumpTo {
        mode: ModeId::Codex,
        path: "nowhere.txt".into(),
        line: 3,
        column: 0,
    });
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Notice { .. })),
        "{events:?}"
    );
}

#[test]
fn a_value_from_a_table_can_be_searched_for_across_the_project() {
    // The other end of the cross-mode jump. A dataframe cell holds a value —
    // "17" — and the useful question is where that appears in the code. The
    // panel seeds the query rather than typing it, because typing it would
    // re-run the search once per character of a term nobody is typing.
    let root = project("seed");
    std::fs::write(
        root.join("fleet.py"),
        "STATIONS = {}\nSAT_17_STATION = \"KIR-2\"\nother = 1\n",
    )
    .unwrap();
    std::fs::write(root.join("readme.md"), "nothing to see\n").unwrap();

    let mut space = Workspace::rooted(&root);
    // The index is walked in the background, so a search run before it lands
    // finds nothing — which is what keeps a cold index on a large repository
    // from freezing the window, and what a test has to wait out by hand.
    space.apply(Command::Sync);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while space.pickers().is_scanning() {
        space.apply(Command::Poll);
        assert!(
            std::time::Instant::now() < deadline,
            "the file walk never finished"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    space.apply(Command::OpenPicker {
        kind: proton_core::picker::PickerKind::Grep,
    });
    let mut events = space.apply(Command::PickerQuery {
        text: "SAT_17".into(),
    });
    // Live grep answers on its own thread when it cannot answer at once.
    while space.pickers().is_searching() {
        let polled = space.apply(Command::Poll);
        if !polled.is_empty() {
            events = polled;
        }
        assert!(std::time::Instant::now() < deadline, "the search never finished");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    let picker = events.iter().find_map(|event| match event {
        Event::Picker { query, items, .. } => Some((query.clone(), items.len())),
        _ => None,
    });
    let (query, hits) = picker.expect("the picker should have answered");
    assert_eq!(query, "SAT_17", "the query should be in the box, whole");
    assert!(hits > 0, "the search should have found the line");
}

#[test]
fn excel_shows_its_workbooks_and_the_quit_prompt_names_an_unsaved_one() {
    let root = project("excel");
    std::fs::create_dir_all(root.join("reports")).unwrap();
    std::fs::write(root.join("reports/q1.csv"), "a,b\n1,2\n").unwrap();
    let mut space = Workspace::rooted(root.clone());
    let events = space.apply(Command::SetMode { mode: ModeId::Sheet });
    let layout = layout_in(&events).expect("a mode change carries its layout");
    assert_eq!((layout.stage, layout.document), (Slot::Grid, DocumentKind::Workbook));
    let found = events.iter().find_map(|e| match e {
        Event::SheetHome { workbooks } => Some(workbooks.iter().map(|w| w.path.clone()).collect::<Vec<_>>()),
        _ => None,
    });
    assert_eq!(found, Some(vec!["reports/q1.csv".to_string()]));

    let events = space.apply(Command::SheetOpen { path: "reports/q1.csv".into() });
    let generation = events
        .iter()
        .find_map(|e| match e {
            Event::SheetBook { book: Some(b) } => Some(b.generation),
            _ => None,
        })
        .expect("the workbook open");
    space.apply(Command::SheetSet { generation, sheet: 0, cells: vec![("C2".into(), "=A2+B2".into())] });
    assert!(space.is_modified());
    assert_eq!(space.unsaved(), vec!["reports/q1.csv".to_string()]);
    assert!(space.save_unsaved().is_empty());
    assert!(space.unsaved().is_empty());
    assert_eq!(std::fs::read_to_string(root.join("reports/q1.csv")).unwrap(), "a,b,\n1,2,3\n");

    // The agent's tools, through the seam every tool is called by.
    let wrote = space.run_tool("sheet_write", &serde_json::json!({"cells": [{"at": "D2", "input": "=C2*10"}]})).unwrap();
    assert!(wrote.contains("D2\t30"), "{wrote}");
    assert_eq!(space.unsaved(), vec!["reports/q1.csv".to_string()], "an agent's edit is held too");
    let read = space.run_tool("sheet_read", &serde_json::json!({"range": "A1:D2"})).unwrap();
    assert!(read.ends_with("2\t1\t2\t3\t30\n"), "{read}");
    assert!(space.with_context("what is @sheet about?").contains("# @sheet\nSheet q1 of reports/q1.csv"));

    // A range to Data: written out, and read in by a cell of a new notebook.
    let events = space.apply(Command::SheetToData { generation, sheet: 0, range: "A1:D2".into() });
    assert!(events.iter().any(|e| matches!(e, Event::ModeChanged { mode: ModeId::Data })), "Data is not shown");
    let csv = root.join(".proton/excel/q1-q1-a1-d2.csv");
    assert_eq!(std::fs::read_to_string(&csv).unwrap(), "a,b,,\n1,2,3,30\n");
    let cell = events
        .iter()
        .find_map(|e| match e {
            Event::Notebook { cells, .. } => cells.last().map(|c| c.source.clone()),
            _ => None,
        })
        .expect("the notebook drawn");
    assert!(cell.contains("pd.read_csv(") && cell.contains("q1-q1-a1-d2.csv"), "{cell}");
    assert!(root.join("from-excel.ipynb").exists());
    // A cell with no table has nothing to bring back.
    let back = space.apply(Command::SheetFromData);
    assert!(back.iter().any(|e| matches!(e, Event::Notice { .. })));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn circuit_shows_its_projects_and_the_quit_prompt_names_an_unsaved_one() {
    let root = project("circuit");
    std::fs::create_dir_all(root.join("labs/adder.circuit/circuits")).unwrap();
    std::fs::write(root.join("labs/adder.circuit/project.json"), "{\n  \"version\": 1,\n  \"top\": \"main\"\n}\n").unwrap();
    std::fs::write(root.join("labs/adder.circuit/circuits/main.circ"), "part A in at 0,0 label=A\n").unwrap();
    let mut space = Workspace::rooted(root.clone());
    let events = space.apply(Command::SetMode { mode: ModeId::Circuit });
    let layout = layout_in(&events).expect("a mode change carries its layout");
    assert_eq!((layout.stage, layout.document), (Slot::Schematic, DocumentKind::CircuitProject));
    let found = events.iter().find_map(|e| match e {
        Event::CircuitHome { projects } => Some(projects.iter().map(|p| p.path.clone()).collect::<Vec<_>>()),
        _ => None,
    });
    assert_eq!(found, Some(vec!["labs/adder.circuit".to_string()]));

    let events = space.apply(Command::CircuitOpen { path: "labs/adder.circuit".into() });
    let parts = events
        .iter()
        .find_map(|e| match e {
            Event::CircuitProject { project: Some(p) } => Some(p.parts.len()),
            _ => None,
        })
        .expect("the project open");
    assert_eq!(parts, 1);
    space.apply(Command::CircuitAdd { name: "half".into() });
    assert!(space.is_modified());
    assert_eq!(space.unsaved(), vec!["labs/adder.circuit".to_string()]);
    assert!(space.save_unsaved().is_empty());
    assert!(space.unsaved().is_empty());
    assert_eq!(std::fs::read_to_string(root.join("labs/adder.circuit/circuits/half.circ")).unwrap(), "");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn logic_shows_its_files_answers_the_line_at_the_cursor_and_the_quit_prompt_names_an_unsaved_one() {
    let root = project("logic");
    std::fs::create_dir_all(root.join("week1")).unwrap();
    std::fs::write(root.join("week1/sheet.logic"), "? p -> q\n").unwrap();
    let mut space = Workspace::rooted(root.clone());
    let events = space.apply(Command::SetMode { mode: ModeId::Logic });
    let layout = layout_in(&events).expect("a mode change carries its layout");
    assert_eq!((layout.stage, layout.document), (Slot::Worksheet, DocumentKind::LogicFile));
    let found = events.iter().find_map(|e| match e {
        Event::LogicHome { files } => Some(files.iter().map(|f| f.path.clone()).collect::<Vec<_>>()),
        _ => None,
    });
    assert_eq!(found, Some(vec!["week1/sheet.logic".to_string()]));
    let generation = space
        .apply(Command::LogicOpen { path: "week1/sheet.logic".into() })
        .iter()
        .find_map(|e| match e {
            Event::LogicFile { file: Some(f) } => Some(f.generation),
            _ => None,
        })
        .expect("the file open");
    let status = space.apply(Command::LogicAsk { generation, line: 1, more: false }).iter().find_map(|e| match e {
        Event::LogicAnswer { answer: Some(a) } => a.status.clone(),
        _ => None,
    });
    assert_eq!(status.as_deref(), Some("contingent"));
    space.apply(Command::LogicEdit { generation, text: "? p or not p\n".into() });
    assert!(space.is_modified());
    assert_eq!(space.unsaved(), vec!["week1/sheet.logic".to_string()]);
    assert!(space.save_unsaved().is_empty());
    assert_eq!(std::fs::read_to_string(root.join("week1/sheet.logic")).unwrap(), "? p or not p\n");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_truth_table_from_logic_is_built_in_circuit_with_its_table_as_a_test() {
    let root = project("logic-circuit");
    let mut space = Workspace::rooted(root.clone());
    space.apply(Command::SetMode { mode: ModeId::Circuit });
    let events = space.apply(Command::CircuitFromTable { project: "logic".into(), circuit: "majority".into(), inputs: vec!["a".into(), "b".into(), "c".into()], column: "00010111".into(), style: "nand".into() });
    let view = events
        .iter()
        .find_map(|e| match e {
            Event::CircuitProject { project: Some(p) } => Some(p.clone()),
            _ => None,
        })
        .expect("the project open");
    assert_eq!((view.path.as_str(), view.shown.as_str()), ("logic.circuit", "majority"));
    assert!(view.parts.iter().all(|p| !matches!(p.kind.as_str(), "and" | "or")) && view.parts.iter().any(|p| p.kind == "nand"));
    let results = space
        .apply(Command::CircuitTestRun { generation: view.generation, circuit: "majority".into(), label: None })
        .into_iter()
        .find_map(|e| match e {
            Event::CircuitTests { results, .. } => Some(results),
            _ => None,
        })
        .unwrap();
    assert!(results.len() == 1 && results[0].failures.is_empty() && results[0].error.is_none() && results[0].rows == 8, "{results:?}");
    // A column of the wrong length is refused.
    let bad = space.apply(Command::CircuitFromTable { project: "logic".into(), circuit: "x".into(), inputs: vec!["a".into()], column: "010".into(), style: "andOr".into() });
    assert!(matches!(bad[0], Event::Notice { .. }));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_circuit_runs_graph_goes_to_data_as_a_dataframe() {
    let root = project("circuit-data");
    std::fs::create_dir_all(root.join("inv.circuit/circuits")).unwrap();
    std::fs::write(root.join("inv.circuit/project.json"), "{\n  \"version\": 1,\n  \"top\": \"main\"\n}\n").unwrap();
    std::fs::write(root.join("inv.circuit/circuits/main.circ"), "part A in at 0,0 label=A\npart N not at 2,0\npart Y out at 6,0 label=Y\nwire 0,0 2,0\nwire 4,0 6,0\n").unwrap();
    let mut space = Workspace::rooted(root.clone());
    space.apply(Command::SetMode { mode: ModeId::Circuit });
    let generation = space
        .apply(Command::CircuitOpen { path: "inv.circuit".into() })
        .iter()
        .find_map(|e| match e {
            Event::CircuitProject { project: Some(p) } => Some(p.generation),
            _ => None,
        })
        .expect("the project open");
    // Not running: nothing recorded, and it says so.
    assert!(space.apply(Command::CircuitTraceToData { generation }).iter().any(|e| matches!(e, Event::Notice { .. })));
    space.apply(Command::CircuitStart { generation });
    space.apply(Command::CircuitSet { generation, id: "A".into(), value: "1".into(), hold: false });
    let events = space.apply(Command::CircuitTraceToData { generation });
    assert!(events.iter().any(|e| matches!(e, Event::ModeChanged { mode: ModeId::Data })), "Data is not shown");
    assert_eq!(std::fs::read_to_string(root.join(".proton/circuit/inv-main.csv")).unwrap(), "sample,A,Y\n0,0,1\n1,1,0\n");
    let cell = events
        .iter()
        .find_map(|e| match e {
            Event::Notebook { cells, .. } => cells.last().map(|c| c.source.clone()),
            _ => None,
        })
        .expect("the notebook drawn");
    assert!(cell.contains("trace_df = pd.read_csv(") && cell.contains("inv-main.csv"), "{cell}");
    assert!(root.join("from-circuit.ipynb").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_agents_circuit_tools_build_test_and_analyse_a_circuit_held_until_saved() {
    let root = project("circuit-agent");
    let mut space = Workspace::rooted(root.clone());
    space.apply(Command::SetMode { mode: ModeId::Circuit });
    let none = space.run_tool("circuit_info", &serde_json::json!({})).unwrap();
    assert!(none.contains("No circuit project is open"), "{none}");
    let info = space.run_tool("circuit_open", &serde_json::json!({"new": "Inverter"})).unwrap();
    assert!(info.contains("Project inverter.circuit") && info.contains("- main (top, on screen)"), "{info}");
    // An input, a NOT and an output, then wired from the pins it was told.
    let made = space
        .run_tool("circuit_edit", &serde_json::json!({"ops": [
            {"op": "place", "kind": "in", "x": 0, "y": 0, "rot": 0, "mirror": false, "settings": [["label", "A"]]},
            {"op": "place", "kind": "not", "x": 3, "y": 0, "rot": 0, "mirror": false, "settings": []},
            {"op": "place", "kind": "out", "x": 8, "y": 0, "rot": 0, "mirror": false, "settings": [["label", "Y"]]}
        ]}))
        .unwrap();
    assert!(made.contains("U1: in 3,0 · out 5,0"), "{made}");
    space
        .run_tool("circuit_edit", &serde_json::json!({"ops": [{"op": "wire", "points": [[0, 0], [3, 0]]}, {"op": "wire", "points": [[5, 0], [8, 0]]}]}))
        .unwrap();
    assert_eq!(space.unsaved(), vec!["inverter.circuit".to_string()], "an agent's edit is held");
    let read = space.run_tool("circuit_read", &serde_json::json!({})).unwrap();
    assert!(read.contains("part U1 not at 3,0") && read.contains("wire 0,0 3,0") && read.contains("# Out1 out: in 8,0"), "{read}");
    // A table given in the call, run without keeping; then kept.
    let ran = space.run_tool("circuit_test", &serde_json::json!({"table": "A Y\n0 1\n1 0\n"})).unwrap();
    assert!(ran.contains("table: all 2 rows pass"), "{ran}");
    let wrong = space.run_tool("circuit_test", &serde_json::json!({"table": "A Y\n0 0\n", "label": "wrong", "keep": true})).unwrap();
    assert!(wrong.contains("wrong: 1 rows, failing:") && wrong.contains("Y is 1, not 0x0"), "{wrong}");
    let all = space.run_tool("circuit_test", &serde_json::json!({})).unwrap();
    assert!(all.contains("wrong: 1 rows, failing"), "{all}");
    let analysed = space.run_tool("circuit_analyse", &serde_json::json!({})).unwrap();
    assert!(analysed.contains("Y = A'") && analysed.contains("0 | 1\n1 | 0"), "{analysed}");
    let built = space.run_tool("circuit_synthesise", &serde_json::json!({"name": "majority", "expression": "M = A B + A C + B C", "style": "nor"})).unwrap();
    assert!(built.contains("Built majority") && built.contains("nor"), "{built}");
    // @circuit: the circuit on screen, what is selected, the last tests.
    let generation = space
        .apply(Command::CircuitShow { circuit: "main".into() })
        .iter()
        .find_map(|e| match e {
            Event::CircuitProject { project: Some(p) } => Some(p.generation),
            _ => None,
        })
        .unwrap();
    space.apply(Command::CircuitSelect { generation, circuit: "main".into(), ids: vec!["U1".into()], wires: vec![] });
    space.run_tool("circuit_test", &serde_json::json!({})).unwrap();
    let context = space.with_context("why does @circuit fail its test?");
    assert!(context.contains("# @circuit\nCircuit main of inverter.circuit") && context.contains("Selected: U1") && context.contains("Last tests run:"), "{context}");
    assert!(space.run_tool("circuit_edit", &serde_json::json!({"ops": [{"op": "place", "kind": "frob", "x": 0, "y": 0, "rot": 0, "mirror": false, "settings": []}]})).is_err());
    let _ = std::fs::remove_dir_all(root);
}

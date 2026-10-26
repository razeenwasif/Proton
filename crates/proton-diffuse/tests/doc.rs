//! A flow open for editing: undo, the file's text, saving.

mod common;

use common::{catalogue, lighthouse_text, sdxl, sdxl_ops};
use proton_diffuse::doc::{parse, text};
use proton_diffuse::{Flow, FlowDoc, Op};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("proton-diffuse-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_file_is_written_as_the_fixture_and_reads_back_the_same() {
    let flow = sdxl();
    let written = text(&flow);
    assert_eq!(written, lighthouse_text());
    assert_eq!(parse(&written).unwrap(), flow);
}

#[test]
fn a_bypass_and_a_seed_setting_are_one_line_each() {
    let mut flow = sdxl();
    proton_diffuse::op::apply(
        &mut flow,
        &[
            Op::Bypass { id: "6".into(), on: true },
            Op::Seed { id: "5".into(), mode: proton_diffuse::SeedMode::Increment },
        ],
        &catalogue(),
    )
    .unwrap();
    // Line by line, commas aside — a line gained after another moves its comma.
    let lines = |text: String| -> Vec<String> { text.lines().map(|l| l.trim_end_matches(',').to_string()).collect() };
    let (before, after) = (lines(text(&sdxl())), lines(text(&flow)));
    let added: Vec<&String> = after.iter().filter(|l| !before.contains(l)).collect();
    assert_eq!(added, ["      \"seed\": \"increment\"", "      \"bypass\": true"]);
    assert_eq!(after.len(), before.len() + 2);
}

#[test]
fn a_later_format_is_refused_and_so_is_a_file_that_is_not_a_flow() {
    assert!(parse("{\"format\": 2, \"nodes\": []}").unwrap_err().contains("later Proton"));
    assert!(parse("{\"nodes\": []}").unwrap_err().contains("no format"));
    assert_eq!(parse("{\"format\": 1}").unwrap(), Flow::default());
}

#[test]
fn undo_and_redo_step_through_batches_and_skip_what_changed_nothing() {
    let dir = scratch("undo");
    let mut doc = FlowDoc::fresh(&dir.join("lighthouse.flow.json"), Flow::default());
    let ops = sdxl_ops();
    doc.apply(&ops[..7], &catalogue()).unwrap();
    doc.apply(&ops[7..], &catalogue()).unwrap();
    assert_eq!(doc.get(), &sdxl());
    // Nothing changed, a refused batch: no steps.
    doc.apply(&[Op::Place { id: "1".into(), at: [16.0, 16.0] }], &catalogue()).unwrap();
    assert!(doc.apply(&[Op::Remove { ids: vec!["99".into()] }], &catalogue()).is_err());
    assert!(doc.undo());
    assert_eq!(doc.get().nodes.len(), 7);
    assert!(doc.get().links.is_empty());
    assert!(doc.undo());
    assert_eq!(doc.get(), &Flow::default());
    assert!(!doc.undo());
    assert!(doc.redo() && doc.redo());
    assert_eq!(doc.get(), &sdxl());
    assert!(!doc.can_redo());
}

#[test]
fn saving_writes_the_file_whole_and_undo_past_it_is_unsaved_again() {
    let dir = scratch("save");
    let path = dir.join("lighthouse.flow.json");
    let mut doc = FlowDoc::fresh(&path, Flow::default());
    assert!(doc.is_dirty());
    doc.apply(&sdxl_ops(), &catalogue()).unwrap();
    doc.save().unwrap();
    assert!(!doc.is_dirty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), lighthouse_text());
    assert!(!dir.join("lighthouse.flow.json.proton-saving").exists());
    assert_eq!(FlowDoc::read(&path).unwrap().get(), doc.get());
    doc.undo();
    assert!(doc.is_dirty());
    doc.redo();
    assert!(!doc.is_dirty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_changed_under_it_is_taken_as_a_step_that_can_be_undone() {
    let dir = scratch("reread");
    let path = dir.join("lighthouse.flow.json");
    let mut doc = FlowDoc::fresh(&path, sdxl());
    doc.save().unwrap();
    let mut edited = sdxl();
    edited.note = Some("edited elsewhere".into());
    std::fs::write(&path, text(&edited)).unwrap();
    assert!(doc.reread().unwrap());
    assert_eq!(doc.get().note.as_deref(), Some("edited elsewhere"));
    assert!(!doc.is_dirty());
    assert!(!doc.reread().unwrap());
    doc.undo();
    assert_eq!(doc.get(), &sdxl());
    let _ = std::fs::remove_dir_all(&dir);
}

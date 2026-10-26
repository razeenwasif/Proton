//! Diffuse through the seam the window uses: the project's flows, a flow
//! made from a starter and opened, edits held until saved and undone, the
//! quit prompt, and a look at the gateway on a worker.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use proton_core::bus::{Event, NoticeLevel};
use proton_core::diffuse::DiffuseView;
use proton_core::{Command, ModeId, Workspace};
use proton_diffuse::{HealthState, Known, Op};
use serde_json::json;

const CATALOGUE: &str = include_str!("../../proton-diffuse/tests/fixtures/catalogue-v1.json");

fn project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("proton-core-diffuse-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A project that has seen forge before: its catalogue kept.
fn kept_project(tag: &str) -> PathBuf {
    let root = project(tag);
    std::fs::create_dir_all(root.join(".proton/diffuse")).unwrap();
    std::fs::write(root.join(".proton/diffuse/catalogue.json"), CATALOGUE).unwrap();
    root
}

fn view(events: &[Event]) -> DiffuseView {
    events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::DiffuseView { view } => Some(view.clone()),
            _ => None,
        })
        .expect("a view")
}

fn known(events: &[Event]) -> Option<Known> {
    events.iter().find_map(|e| match e {
        Event::DiffuseKnown { known, .. } => Some(known.clone()),
        _ => None,
    })
}

fn warned(events: &[Event]) -> Option<String> {
    events.iter().find_map(|e| match e {
        Event::Notice { text, level: NoticeLevel::Warn | NoticeLevel::Error } => Some(text.clone()),
        _ => None,
    })
}

/// In Diffuse, with `lighthouse.flow.json` made from the text-to-image starter.
fn with_flow(tag: &str) -> (Workspace, PathBuf) {
    let root = kept_project(tag);
    let mut space = Workspace::rooted(&root);
    space.apply(Command::SetMode { mode: ModeId::Diffuse });
    let events = space.apply(Command::DiffuseNew { name: "lighthouse".into(), template: Some("textToImage".into()) });
    assert!(warned(&events).is_none(), "{events:?}");
    (space, root)
}

#[test]
fn entering_shows_the_flows_here_and_what_forge_was_last_known_to_have() {
    let root = kept_project("enter");
    std::fs::create_dir_all(root.join("art")).unwrap();
    std::fs::write(root.join("art/harbour.flow.json"), "{\"format\": 1}\n").unwrap();
    std::fs::write(root.join("notes.json"), "{}").unwrap();
    let mut space = Workspace::rooted(&root);
    let events = space.apply(Command::SetMode { mode: ModeId::Diffuse });
    assert_eq!(view(&events).flows, ["art/harbour.flow.json"]);
    let known = known(&events).expect("what was kept");
    assert_eq!(known.health.state, HealthState::Looking);
    assert_eq!(known.catalogue.unwrap().nodes.len(), 16);
    assert!(!known.fresh);
}

#[test]
fn a_flow_made_from_the_starter_is_written_open_and_ready_to_run() {
    let (mut space, root) = with_flow("new");
    let text = std::fs::read_to_string(root.join("lighthouse.flow.json")).unwrap();
    assert!(text.contains("\"type\": \"KSampler\""));
    let events = space.apply(Command::Sync);
    let shown = view(&events).shown.expect("the flow made is shown");
    assert_eq!(shown.path, "lighthouse.flow.json");
    assert_eq!(shown.flow.nodes.len(), 7);
    assert_eq!(shown.problems, vec![]);
    assert!(!shown.dirty);
    assert!(warned(&space.apply(Command::DiffuseNew { name: "lighthouse".into(), template: None }))
        .unwrap()
        .contains("already here"));
    assert!(warned(&space.apply(Command::DiffuseNew { name: "../out".into(), template: None })).is_some());
}

#[test]
fn edits_are_held_until_saved_and_undone_whole() {
    let (mut space, root) = with_flow("edit");
    let ops = vec![
        Op::Add { kind: "PreviewImage".into(), at: [700.0, 300.0], values: Default::default() },
        Op::Connect { from: "6:image".into(), to: "8:images".into() },
    ];
    let events = space.apply(Command::DiffuseEdit { path: "lighthouse.flow.json".into(), ops });
    let seen = view(&events);
    assert_eq!(seen.added, ["8"]);
    let shown = seen.shown.unwrap();
    assert!(shown.dirty && shown.can_undo);
    assert!(seen.open[0].dirty);
    assert_eq!(space.unsaved(), ["lighthouse.flow.json"]);
    // A refused edit says why and leaves the flow as it was.
    let refused = space.apply(Command::DiffuseEdit {
        path: "lighthouse.flow.json".into(),
        ops: vec![Op::Connect { from: "1:clip".into(), to: "5:model".into() }],
    });
    assert_eq!(warned(&refused).unwrap(), "model takes MODEL, and clip makes CLIP");
    assert_eq!(view(&refused).shown.unwrap().flow.nodes.len(), 8);
    // Undo takes the whole batch back.
    let undone = view(&space.apply(Command::DiffuseUndo)).shown.unwrap();
    assert_eq!(undone.flow.nodes.len(), 7);
    assert!(!undone.dirty);
    space.apply(Command::DiffuseRedo);
    space.apply(Command::DiffuseSave);
    assert!(space.unsaved().is_empty());
    assert!(std::fs::read_to_string(root.join("lighthouse.flow.json")).unwrap().contains("PreviewImage"));
}

#[test]
fn a_flow_with_changes_is_not_closed_and_quitting_saves_it() {
    let (mut space, root) = with_flow("quit");
    let set = Op::Set { id: "5".into(), name: "steps".into(), value: json!(12) };
    space.apply(Command::DiffuseEdit { path: "lighthouse.flow.json".into(), ops: vec![set] });
    let closing = space.apply(Command::DiffuseClose { path: "lighthouse.flow.json".into(), discard: false });
    assert!(warned(&closing).unwrap().contains("save or discard"));
    assert!(space.is_modified());
    assert!(space.save_unsaved().is_empty());
    assert!(std::fs::read_to_string(root.join("lighthouse.flow.json")).unwrap().contains("\"steps\": 12"));
    let closed = view(&space.apply(Command::DiffuseClose { path: "lighthouse.flow.json".into(), discard: false }));
    assert!(closed.open.is_empty() && closed.shown.is_none());
    // Opened again, as saved.
    let opened = view(&space.apply(Command::DiffuseOpen { path: "lighthouse.flow.json".into() }));
    assert_eq!(opened.shown.unwrap().flow.node("5").unwrap().values["steps"], json!(12));
    assert!(warned(&space.apply(Command::DiffuseOpen { path: "../elsewhere.flow.json".into() })).is_some());
    assert!(warned(&space.apply(Command::DiffuseOpen { path: "notes.json".into() })).is_some());
}

#[test]
fn without_forges_nodes_known_a_flow_still_opens_and_says_why_it_cannot_run() {
    let root = project("unknown");
    std::fs::write(
        root.join("old.flow.json"),
        "{\"format\": 1, \"nodes\": [{\"id\": \"1\", \"type\": \"KSampler\", \"at\": [0, 0]}]}\n",
    )
    .unwrap();
    let mut space = Workspace::rooted(&root);
    space.apply(Command::SetMode { mode: ModeId::Diffuse });
    let shown = view(&space.apply(Command::DiffuseOpen { path: "old.flow.json".into() })).shown.unwrap();
    assert_eq!(shown.problems.len(), 1);
    assert!(shown.problems[0].message.contains("not known yet"));
    // Moving a node needs no catalogue; adding one does.
    let moved = space.apply(Command::DiffuseEdit {
        path: "old.flow.json".into(),
        ops: vec![Op::Place { id: "1".into(), at: [5.0, 5.0] }],
    });
    assert!(warned(&moved).is_none());
    let added = space.apply(Command::DiffuseEdit {
        path: "old.flow.json".into(),
        ops: vec![Op::Add { kind: "KSampler".into(), at: [0.0, 0.0], values: Default::default() }],
    });
    assert_eq!(warned(&added).unwrap(), "the server has no KSampler node");
}

/// A gateway that answers everything with forge's image server off.
fn idle_gateway() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/img/v1", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).map(|n| n == 1).unwrap_or(false) {
                head.push(byte[0]);
            }
            let body = r#"{"error": "offline"}"#;
            let _ = write!(stream, "HTTP/1.1 503 Fake\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    base
}

#[test]
fn a_look_at_the_gateway_is_made_on_a_worker_and_said_when_it_is_back() {
    // The key a test sends: made up, and in Proton's own environment, so
    // the login shell is never asked.
    std::env::set_var("NEXUS_KEY", "nx-test-0123456789");
    let root = kept_project("look");
    let person = root.join("person.json");
    std::fs::write(&person, json!({"diffuse": {"gateway": idle_gateway()}}).to_string()).unwrap();
    let mut space = Workspace::rooted(&root);
    space.set_person_settings(Some(person));
    space.apply(Command::SetMode { mode: ModeId::Diffuse });
    assert!(space.apply(Command::DiffuseLook { again: false }).is_empty());
    let deadline = Instant::now() + Duration::from_secs(20);
    let found = loop {
        if let Some(found) = known(&space.apply(Command::Poll)) {
            break found;
        }
        assert!(Instant::now() < deadline, "the look never came back");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(found.health.state, HealthState::Offline);
    assert!(found.health.message.contains("Nexus › Inference"));
    // What was kept is still what the editor works from.
    assert_eq!(found.catalogue.unwrap().nodes.len(), 16);
}

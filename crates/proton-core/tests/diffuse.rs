//! Diffuse through the seam the window uses: the flows in Diffuse's own
//! folder — never the project open in Codex — a flow made from a starter
//! and opened, edits held until saved and undone, the quit prompt, a look at
//! the gateway on a worker, runs, and the Trash.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use proton_core::bus::{Event, NoticeLevel};
use proton_core::diffuse::DiffuseView;
use proton_core::{Command, ModeId, Session};
use proton_diffuse::{HealthState, Known, Op};
use serde_json::json;

const CATALOGUE: &str = include_str!("../../proton-diffuse/tests/fixtures/catalogue-v1.json");

fn project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("proton-core-diffuse-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Where a test's person keeps their settings: beside Diffuse's folder.
fn person_file(home: &Path) -> PathBuf {
    home.with_file_name(format!("{}-person.json", home.file_name().unwrap().to_string_lossy()))
}

/// The person's settings: Diffuse's folder is `home`, and its gateway `gateway`.
fn set_gateway(home: &Path, gateway: Option<&str>) {
    let mut diffuse = json!({"folder": home});
    if let Some(gateway) = gateway {
        diffuse["gateway"] = json!(gateway);
    }
    std::fs::write(person_file(home), json!({ "diffuse": diffuse }).to_string()).unwrap();
}

/// A session whose Diffuse keeps its flows in `home`, a folder of its own,
/// with the project open in Codex somewhere else.
fn session_in(home: &Path) -> Session {
    let project = home.with_file_name(format!("{}-project", home.file_name().unwrap().to_string_lossy()));
    let _ = std::fs::remove_dir_all(&project);
    std::fs::create_dir_all(&project).unwrap();
    set_gateway(home, None);
    Session::new(&project).with_person_settings(person_file(home))
}

/// A Diffuse folder that has seen forge before: its catalogue kept.
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
fn with_flow(tag: &str) -> (Session, PathBuf) {
    let root = kept_project(tag);
    let mut space = session_in(&root);
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
    let mut space = session_in(&root);
    // A flow in the project open in Codex is that project's, not Diffuse's.
    let project = root.with_file_name(format!("{}-project", root.file_name().unwrap().to_string_lossy()));
    std::fs::write(project.join("codex.flow.json"), "{\"format\": 1}\n").unwrap();
    let events = space.apply(Command::SetMode { mode: ModeId::Diffuse });
    let seen = view(&events);
    assert_eq!(seen.flows, ["art/harbour.flow.json"]);
    assert!(seen.folder.ends_with(&root.file_name().unwrap().to_string_lossy().to_string()), "{}", seen.folder);
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
    assert_eq!(space.unsaved(), ["lighthouse.flow.json (Diffuse)"]);
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
    assert!(!space.unsaved().is_empty());
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
    let mut space = session_in(&root);
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
    let mut space = session_in(&root);
    set_gateway(&root, Some(&idle_gateway()));
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

#[path = "../../proton-diffuse/tests/common/server.rs"]
#[allow(dead_code)]
mod server;

/// A PNG as forge writes one: its head, the graph sent as `proton-flow`,
/// then pixels nobody reads.
fn forge_png(graph: &str) -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(kind);
        out.extend(data);
        out.extend([0, 0, 0, 0]);
    };
    chunk(b"IHDR", &[0, 0, 4, 0, 0, 0, 4, 0, 8, 2, 0, 0, 0]);
    chunk(b"tEXt", &[b"proton-flow\0".as_slice(), graph.as_bytes()].concat());
    chunk(b"IDAT", &[0; 16]);
    chunk(b"IEND", &[]);
    out
}

/// A gateway serving forge: the catalogue, a run that goes through every
/// event and makes one image, and that image with the graph in it. A run
/// whose sampler asks for 2048 px runs out of memory.
fn serving_gateway() -> String {
    recording_gateway().0
}

/// What a recording gateway was sent: the last graph, and each upload's bytes.
type Recorded = (std::sync::Arc<std::sync::Mutex<String>>, std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>);

fn recording_gateway() -> (String, Recorded) {
    use server::{Fake, Reply};
    let sent = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let uploads = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = (sent.clone(), uploads.clone());
    let fake = Fake::start(move |seen| {
        let path = seen.path.as_str();
        match (seen.method.as_str(), path) {
            ("POST", "/img/v1/uploads") => {
                uploads.lock().unwrap().push(seen.body.clone());
                Reply::json(200, r#"{"id": "up_31", "width": 4, "height": 4}"#)
            }
            ("GET", "/img/v1/status") => Reply::json(200, r#"{"gpu": {"name": "NVIDIA GeForce RTX 4090", "total": 25757220864, "used": 7600000000}}"#),
            ("GET", "/img/v1/nodes") => Reply::json(200, CATALOGUE),
            ("GET", "/img/v1/models") => Reply::json(200, include_str!("../../proton-diffuse/tests/fixtures/models.json")),
            ("POST", "/img/v1/runs") => {
                let body: serde_json::Value = serde_json::from_slice(&seen.body).unwrap();
                let big = body["graph"]["nodes"].as_array().unwrap().iter().any(|n| n["values"]["width"] == json!(2048));
                *sent.lock().unwrap() = body["graph"].to_string();
                Reply::json(200, if big { r#"{"run": "r-oom", "position": 0}"# } else { r#"{"run": "r1", "position": 0}"# })
            }
            ("GET", "/img/v1/runs/r1/events") => Reply::stream(vec![
                (
                    Duration::ZERO,
                    concat!(
                        "event: queued\ndata: {\"position\": 0}\n\n",
                        "event: started\ndata: {}\n\n",
                        "event: node\ndata: {\"id\": \"1\", \"state\": \"running\"}\n\n",
                        "event: node\ndata: {\"id\": \"1\", \"state\": \"done\"}\n\n",
                        "event: node\ndata: {\"id\": \"5\", \"state\": \"running\"}\n\n",
                        "event: progress\ndata: {\"id\": \"5\", \"step\": 15, \"steps\": 30}\n\n",
                        "event: preview\ndata: {\"id\": \"5\", \"step\": 15, \"jpeg\": \"/9j/AA==\"}\n\n",
                    ),
                ),
                // The sampler at work for a while, as it is on forge.
                (Duration::from_millis(400), ""),
                (
                    Duration::ZERO,
                    concat!(
                    "event: node\ndata: {\"id\": \"5\", \"state\": \"done\"}\n\n",
                    "event: node\ndata: {\"id\": \"7\", \"state\": \"running\"}\n\n",
                    "event: image\ndata: {\"id\": \"7\", \"image\": \"image-abc123\", \"index\": 0, \"width\": 1024, \"height\": 1024}\n\n",
                    "event: node\ndata: {\"id\": \"7\", \"state\": \"done\"}\n\n",
                    "event: done\ndata: {\"seconds\": 9.5}\n\n",
                    ),
                ),
            ]),
            ("GET", "/img/v1/runs/r-oom/events") => Reply::stream(vec![(
                Duration::ZERO,
                "event: started\ndata: {}\n\nevent: error\ndata: {\"id\": \"6\", \"error\": \"oom\", \"message\": \"CUDA out of memory\"}\n\n",
            )]),
            ("GET", "/img/v1/images/image-abc123") => Reply::bytes("image/png", forge_png(&sent.lock().unwrap())),
            _ => Reply::json(404, r#"{"error": "not_found", "message": "no"}"#),
        }
    });
    (fake.base.clone(), recorded)
}

fn runs(events: &[Event]) -> Option<Vec<proton_core::diffuse::DiffuseRun>> {
    events.iter().rev().find_map(|e| match e {
        Event::DiffuseRuns { runs } => Some(runs.clone()),
        _ => None,
    })
}

/// Poll until `done` says so of what came back.
fn poll_until(space: &mut Session, mut done: impl FnMut(&[Event]) -> bool) -> Vec<Event> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut all = Vec::new();
    loop {
        let events = space.apply(Command::Poll);
        all.extend(events.clone());
        if done(&events) {
            return all;
        }
        assert!(Instant::now() < deadline, "it never came: {all:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_flow_runs_on_forge_its_image_saved_and_listed_and_its_seed_moved_on() {
    std::env::set_var("NEXUS_KEY", "nx-test-0123456789");
    let (mut space, root) = with_flow("run");
    set_gateway(&root, Some(&serving_gateway()));
    space.apply(Command::DiffuseLook { again: false });
    poll_until(&mut space, |e| known(e).is_some_and(|k| k.health.state == HealthState::Serving));
    space.apply(Command::DiffuseEdit {
        path: "lighthouse.flow.json".into(),
        ops: vec![Op::Seed { id: "5".into(), mode: proton_diffuse::SeedMode::Increment }],
    });
    let started = space.apply(Command::DiffuseRun { path: "lighthouse.flow.json".into() });
    let first = runs(&started).unwrap();
    assert_eq!(first[0].state, proton_core::diffuse::RunState::Sending);
    // Run with 42; the seed is 43 for the next.
    assert_eq!(view(&started).shown.unwrap().flow.node("5").unwrap().values["seed"], json!(43));
    let heard = poll_until(&mut space, |e| runs(e).is_some_and(|r| r[0].state.ended()));
    let run = runs(&heard).unwrap().remove(0);
    assert_eq!(run.state, proton_core::diffuse::RunState::Done, "{run:?}");
    assert_eq!(run.run.as_deref(), Some("r1"));
    assert_eq!(run.seconds, Some(9.5));
    let states: Vec<String> = run.nodes.iter().map(|n| format!("{} {:?}", n.id, n.state)).collect();
    assert_eq!(states, ["1 Done", "5 Done", "7 Done"]);
    assert!(run.nodes.iter().all(|n| n.seconds.is_some()));
    assert_eq!(run.outputs.len(), 1);
    assert_eq!(run.outputs[0].path, "outputs/lighthouse/image_0001.png");
    assert!(run.outputs[0].kept && run.outputs[0].width == 1024);
    assert!(root.join("outputs/lighthouse/image_0001.png").is_file());
    // A progress and a preview were heard on the way.
    assert!(heard.iter().any(|e| matches!(e, Event::DiffuseRuns { runs } if runs[0].progress.as_ref().is_some_and(|p| p.step == 15) && runs[0].preview.as_deref() == Some("data:image/jpeg;base64,/9j/AA=="))));
    // The gallery lists it, with what made it.
    let gallery = heard
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::DiffuseGallery { images } => Some(images.clone()),
            _ => None,
        })
        .expect("the gallery, again");
    assert_eq!(gallery[0].path, "outputs/lighthouse/image_0001.png");
    assert_eq!(gallery[0].made.seed, Some(42));
    assert_eq!(gallery[0].made.prompt.as_deref(), Some("a lighthouse at dusk, oil painting"));
    assert!(space.diffuse_file("outputs/lighthouse/image_0001.png").is_some());
    assert!(space.diffuse_file("lighthouse.flow.json").is_none());
    // The next is numbered after it.
    space.apply(Command::DiffuseRun { path: "lighthouse.flow.json".into() });
    poll_until(&mut space, |e| runs(e).is_some_and(|r| r[0].state.ended()));
    assert!(root.join("outputs/lighthouse/image_0002.png").is_file());
}

#[test]
fn a_run_forge_cannot_finish_says_why() {
    std::env::set_var("NEXUS_KEY", "nx-test-0123456789");
    let (mut space, root) = with_flow("oom");
    set_gateway(&root, Some(&serving_gateway()));
    space.apply(Command::DiffuseLook { again: false });
    poll_until(&mut space, |e| known(e).is_some_and(|k| k.health.state == HealthState::Serving));
    space.apply(Command::DiffuseEdit {
        path: "lighthouse.flow.json".into(),
        ops: vec![Op::Set { id: "4".into(), name: "width".into(), value: json!(2048) }],
    });
    space.apply(Command::DiffuseRun { path: "lighthouse.flow.json".into() });
    let heard = poll_until(&mut space, |e| runs(e).is_some_and(|r| r[0].state.ended()));
    let run = runs(&heard).unwrap().remove(0);
    assert_eq!(run.state, proton_core::diffuse::RunState::Failed);
    assert_eq!(run.message.as_deref(), Some("forge ran out of GPU memory: CUDA out of memory"));
    // A flow that cannot run is not sent.
    space.apply(Command::DiffuseEdit {
        path: "lighthouse.flow.json".into(),
        ops: vec![Op::Disconnect { to: "5:model".into() }],
    });
    let refused = space.apply(Command::DiffuseRun { path: "lighthouse.flow.json".into() });
    assert!(warned(&refused).unwrap().contains("can't run yet: model has no link"));
}

/// Where a test's Trash is: never the person's.
fn test_trash() -> PathBuf {
    let bin = std::env::temp_dir().join(format!("proton-core-diffuse-bin-{}", std::process::id()));
    std::env::set_var("PROTON_TRASH", &bin);
    bin
}

#[test]
fn a_flow_and_its_images_go_to_the_trash_and_only_what_the_window_was_shown() {
    let bin = test_trash();
    std::env::set_var("NEXUS_KEY", "nx-test-0123456789");
    let (mut space, root) = with_flow("trash");
    set_gateway(&root, Some(&serving_gateway()));
    space.apply(Command::DiffuseLook { again: false });
    poll_until(&mut space, |e| known(e).is_some_and(|k| k.health.state == HealthState::Serving));
    space.apply(Command::DiffuseRun { path: "lighthouse.flow.json".into() });
    poll_until(&mut space, |e| runs(e).is_some_and(|r| r[0].state.ended()));
    let image = "outputs/lighthouse/image_0001.png".to_string();
    assert!(root.join(&image).is_file());
    // Nothing the gallery did not list: not a flow, not a file outside.
    let refused = space.apply(Command::DiffuseDeleteImages { paths: vec!["lighthouse.flow.json".into()] });
    assert!(warned(&refused).unwrap().contains("not an image Diffuse may delete"));
    assert!(root.join("lighthouse.flow.json").is_file());
    let gone = space.apply(Command::DiffuseDeleteImages { paths: vec![image.clone()] });
    assert!(!root.join(&image).exists());
    assert!(bin.join("image_0001.png").is_file());
    assert!(runs(&gone).unwrap()[0].outputs.is_empty(), "the run still offers the image");
    let gallery = gone.iter().find_map(|e| match e {
        Event::DiffuseGallery { images } => Some(images.clone()),
        _ => None,
    });
    assert_eq!(gallery.map(|g| g.len()), Some(0));
    // The flow: closed, and moved; its images stay where they are.
    std::fs::write(root.join("outputs/lighthouse/kept.png"), b"\x89PNG").unwrap();
    let deleted = space.apply(Command::DiffuseDelete { path: "lighthouse.flow.json".into() });
    let after = view(&deleted);
    assert!(after.shown.is_none() && after.open.is_empty() && after.flows.is_empty(), "{after:?}");
    assert!(!root.join("lighthouse.flow.json").exists() && bin.join("lighthouse.flow.json").is_file());
    assert!(root.join("outputs/lighthouse/kept.png").is_file());
    assert!(warned(&space.apply(Command::DiffuseDelete { path: "../elsewhere.flow.json".into() })).is_some());
}

#[test]
fn diffuses_folder_is_moved_into_the_persons_settings_and_not_with_changes_left_behind() {
    let (mut space, root) = with_flow("folder");
    let elsewhere = project("folder-elsewhere");
    std::fs::write(elsewhere.join("harbour.flow.json"), "{\"format\": 1}\n").unwrap();
    space.apply(Command::DiffuseEdit {
        path: "lighthouse.flow.json".into(),
        ops: vec![Op::Place { id: "1".into(), at: [0.0, 0.0] }],
    });
    let refused = space.apply(Command::DiffuseFolder { folder: Some(elsewhere.display().to_string()) });
    assert!(warned(&refused).unwrap().contains("save or discard lighthouse.flow.json"));
    space.apply(Command::DiffuseSave);
    let moved = space.apply(Command::DiffuseFolder { folder: Some(elsewhere.display().to_string()) });
    let seen = view(&moved);
    assert_eq!(seen.flows, ["harbour.flow.json"]);
    assert!(seen.open.is_empty() && seen.shown.is_none());
    let kept: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(person_file(&root)).unwrap()).unwrap();
    assert_eq!(kept["diffuse"]["folder"], json!(elsewhere.display().to_string()));
    assert_eq!(space.diffuse_home(), elsewhere);
    // None puts it back to ~/Diffuse: only the setting is checked, nothing made there.
    space.apply(Command::DiffuseFolder { folder: None });
    let kept: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(person_file(&root)).unwrap()).unwrap();
    assert!(kept["diffuse"].get("folder").is_none());
    assert!(space.diffuse_home().ends_with("Diffuse"));
}

#[test]
fn an_outputs_graph_opens_as_a_new_flow_and_so_does_one_dropped_on_the_window() {
    std::env::set_var("NEXUS_KEY", "nx-test-0123456789");
    let (mut space, root) = with_flow("from-png");
    set_gateway(&root, Some(&serving_gateway()));
    space.apply(Command::DiffuseLook { again: false });
    poll_until(&mut space, |e| known(e).is_some_and(|k| k.health.state == HealthState::Serving));
    space.apply(Command::DiffuseRun { path: "lighthouse.flow.json".into() });
    poll_until(&mut space, |e| runs(e).is_some_and(|r| r[0].state.ended()));
    let sent = view(&space.apply(Command::Sync)).shown.unwrap().flow;

    // The output's graph, out of its PNG: a new flow named after it, unsaved.
    let opened = space.apply(Command::DiffuseOpenImage { path: "outputs/lighthouse/image_0001.png".into() });
    let seen = view(&opened);
    let shown = seen.shown.unwrap();
    assert_eq!(shown.path, "image_0001.flow.json");
    assert!(shown.dirty && !root.join("image_0001.flow.json").exists(), "it is written only on ⌘S");
    let catalogue = proton_diffuse::Catalogue::parse(CATALOGUE).unwrap();
    assert_eq!(
        shown.flow.graph(&catalogue, None).unwrap(),
        sent.graph(&catalogue, None).unwrap(),
        "not the graph that ran"
    );
    assert_eq!(shown.problems, vec![]);
    assert_eq!(seen.starters.len(), 5);
    // Again: a name of its own.
    let again = view(&space.apply(Command::DiffuseOpenImage { path: "outputs/lighthouse/image_0001.png".into() }));
    assert_eq!(again.shown.unwrap().path, "image_0001-2.flow.json");
    // One the window read out of a dropped PNG.
    let graph = sent.graph(&catalogue, None).unwrap();
    let dropped = view(&space.apply(Command::DiffuseOpenGraph { name: "harbour at dusk.png".into(), graph }));
    assert_eq!(dropped.shown.unwrap().path, "harbour-at-dusk.flow.json");
    // Not an image the gallery listed: refused.
    let refused = space.apply(Command::DiffuseOpenImage { path: "lighthouse.flow.json".into() });
    assert!(warned(&refused).is_some());
}

#[test]
fn the_first_flow_makes_diffuses_folder() {
    let root = project("fresh-folder").join("not-yet");
    let mut space = session_in(&root);
    space.apply(Command::SetMode { mode: ModeId::Diffuse });
    assert!(!root.exists(), "entering Diffuse made its folder");
    let events = space.apply(Command::DiffuseNew { name: "first".into(), template: None });
    assert!(warned(&events).is_none(), "{events:?}");
    assert!(root.join("first.flow.json").is_file());
}

/// A 4 × 4 picture, every pixel `rgb`, as a PNG.
fn picture(rgb: [u8; 3]) -> Vec<u8> {
    let rgba: Vec<u8> = (0..16).flat_map(|_| [rgb[0], rgb[1], rgb[2], 255]).collect();
    proton_raster::file::encode_png(4, 4, &rgba).unwrap()
}

#[test]
fn a_picture_and_its_painted_mask_go_to_forge_as_one_upload_named_in_the_graph() {
    std::env::set_var("NEXUS_KEY", "nx-test-0123456789");
    let root = kept_project("picture");
    let mut space = session_in(&root);
    space.apply(Command::SetMode { mode: ModeId::Diffuse });
    space.apply(Command::DiffuseNew { name: "boat".into(), template: Some("inpaint".into()) });
    // The picture, kept in the folder; the same bytes again are the same file,
    // other bytes of the name another.
    assert_eq!(space.diffuse_put("inputs/harbour.png", &picture([10, 20, 30])).unwrap(), "inputs/harbour.png");
    assert_eq!(space.diffuse_put("inputs/harbour.png", &picture([10, 20, 30])).unwrap(), "inputs/harbour.png");
    assert_eq!(space.diffuse_put("inputs/harbour.png", &picture([99, 99, 99])).unwrap(), "inputs/harbour-2.png");
    assert!(space.diffuse_put("../outside.png", &picture([0, 0, 0])).is_err());
    assert!(space.diffuse_put("inputs/notes.png", b"not a picture").is_err());
    // A Load Image naming a picture not there says so.
    let edit = |space: &mut Session, value: &str| {
        space.apply(Command::DiffuseEdit {
            path: "boat.flow.json".into(),
            ops: vec![Op::Set { id: "4".into(), name: "image".into(), value: json!(value) }],
        })
    };
    let missing = view(&edit(&mut space, "inputs/gone.png")).shown.unwrap().problems;
    assert!(missing.iter().any(|p| p.message == "inputs/gone.png is not in Diffuse's folder"), "{missing:?}");
    edit(&mut space, "inputs/harbour.png");
    // A mask: the left half painted.
    let mask: Vec<u8> = (0..16).flat_map(|i| if i % 4 < 2 { [255, 255, 255, 255] } else { [0, 0, 0, 255] }).collect();
    let mask = proton_raster::file::encode_png(4, 4, &mask).unwrap();
    assert_eq!(space.diffuse_put("boat.4.mask.png", &mask).unwrap(), "boat.4.mask.png");

    let (base, (sent, uploads)) = recording_gateway();
    set_gateway(&root, Some(&base));
    space.apply(Command::DiffuseLook { again: false });
    poll_until(&mut space, |e| known(e).is_some_and(|k| k.health.state == HealthState::Serving));
    let started = space.apply(Command::DiffuseRun { path: "boat.flow.json".into() });
    assert!(warned(&started).is_none(), "{started:?}");
    poll_until(&mut space, |e| runs(e).is_some_and(|r| r[0].state.ended()));
    // One upload: the picture, transparent where the mask is painted.
    let uploaded = uploads.lock().unwrap().clone();
    assert_eq!(uploaded.len(), 1);
    let (w, h, rgba) = proton_raster::file::decode_png(&uploaded[0]).unwrap();
    assert_eq!((w, h), (4, 4));
    for (i, pixel) in rgba.chunks(4).enumerate() {
        assert_eq!(&pixel[..3], &[10, 20, 30], "the picture's colour is kept under the mask");
        assert_eq!(pixel[3], if i % 4 < 2 { 0 } else { 255 }, "pixel {i}");
    }
    // The graph names forge's upload, not the file.
    let graph: serde_json::Value = serde_json::from_str(&sent.lock().unwrap()).unwrap();
    let load = graph["nodes"].as_array().unwrap().iter().find(|n| n["id"] == "4").unwrap().clone();
    assert_eq!(load["values"]["image"], json!("up_31"));
}

#[test]
fn an_output_is_sent_to_canvas_as_a_canvas_and_to_game_only_with_a_game_open() {
    std::env::set_var("NEXUS_KEY", "nx-test-0123456789");
    let (mut space, root) = with_flow("send");
    set_gateway(&root, Some(&serving_gateway()));
    space.apply(Command::DiffuseLook { again: false });
    poll_until(&mut space, |e| known(e).is_some_and(|k| k.health.state == HealthState::Serving));
    space.apply(Command::DiffuseRun { path: "lighthouse.flow.json".into() });
    poll_until(&mut space, |e| runs(e).is_some_and(|r| r[0].state.ended()));
    // forge's PNG here is head only; Canvas needs a whole one.
    std::fs::write(root.join("outputs/lighthouse/image_0001.png"), picture([200, 100, 50])).unwrap();
    let sent =
        space.apply(Command::DiffuseSend { path: "outputs/lighthouse/image_0001.png".into(), to: "canvas".into() });
    assert_eq!(space.active().mode(), ModeId::Raster);
    assert!(warned(&sent).is_none(), "{sent:?}");
    assert!(
        sent.iter()
            .any(|e| matches!(e, Event::RasterCanvas { canvas: Some(c) } if c.width == 4 && c.name == "image-0001")),
        "no canvas was made of it"
    );
    space.apply(Command::SetMode { mode: ModeId::Diffuse });
    let to_game =
        space.apply(Command::DiffuseSend { path: "outputs/lighthouse/image_0001.png".into(), to: "game".into() });
    assert_eq!(space.active().mode(), ModeId::Game);
    assert!(warned(&to_game).unwrap().contains("open a game in Game"));
}

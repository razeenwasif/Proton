//! The client against a fake gateway: the contract's requests, its three
//! ways of not being served, and a run's events as they come.

mod common;

use std::io::Cursor;
use std::time::{Duration, Instant};

use common::server::{nowhere, Fake, Reply};
use common::{catalogue, models, sdxl};
use proton_diffuse::client::{Events, NodeState};
use proton_diffuse::{Client, Error, Event, HealthState, Key};

/// A made-up key: what a test sends, never a real one.
const KEY: &str = "nx-test-0123456789";

fn client(bases: Vec<String>) -> Client {
    Client::new(bases, Key::new(KEY).unwrap())
}

const STATUS: &str = r#"{"gpu": {"name": "NVIDIA GeForce RTX 4090", "total": 25757220864, "used": 7623000000},
  "loaded": [{"name": "sd_xl_base_1.0.safetensors", "bytes": 6938078334}], "queue": [], "running": null}"#;

fn the_gateway() -> Fake {
    Fake::start(|seen| match (seen.method.as_str(), seen.path.as_str()) {
        ("GET", "/img/v1/nodes") => Reply::json(200, include_str!("fixtures/catalogue-v1.json")),
        ("GET", "/img/v1/models") => Reply::json(200, include_str!("fixtures/models.json")),
        ("GET", "/img/v1/status") => Reply::json(200, STATUS),
        ("POST", "/img/v1/runs") => Reply::json(200, r#"{"run": "r8", "position": 0}"#),
        ("POST", "/img/v1/runs/r8/cancel") => Reply::json(200, r#"{"ok": true}"#),
        ("POST", "/img/v1/uploads") => Reply::json(200, r#"{"id": "up_31", "width": 1024, "height": 1024}"#),
        ("GET", "/img/v1/images/img_7f3a91") => Reply::bytes("image/png", b"\x89PNG\r\n\x1a\nnot really".to_vec()),
        ("GET", "/img/v1/runs/r8/events") => {
            Reply::stream(vec![(Duration::ZERO, include_str!("fixtures/run-sdxl.events"))])
        }
        _ => Reply::json(404, r#"{"error": "not_found", "message": "no such path"}"#),
    })
}

#[test]
fn the_catalogue_and_models_are_asked_for_with_the_key_as_a_bearer() {
    let fake = the_gateway();
    let client = client(vec![fake.base.clone()]);
    assert_eq!(client.nodes().unwrap(), catalogue());
    assert_eq!(client.models().unwrap(), models());
    let status = client.status().unwrap();
    assert_eq!(status.gpu.name, "NVIDIA GeForce RTX 4090");
    assert_eq!(status.running, None);
    let seen = fake.seen();
    assert_eq!(seen.len(), 3);
    assert!(seen.iter().all(|s| s.header("authorization") == Some(&*format!("Bearer {KEY}"))));
}

#[test]
fn a_refused_key_and_an_idle_forge_are_told_apart_and_the_key_is_never_said() {
    let refusing = Fake::start(|_| Reply::json(401, r#"{"error": "key"}"#));
    let refused = client(vec![refusing.base.clone()]).nodes().unwrap_err();
    assert_eq!(refused, Error::Key);
    assert!(!refused.describe().contains(KEY));
    assert_eq!(format!("{:?}", Key::new(KEY).unwrap()), "Key(…)");
    let health = client(vec![refusing.base.clone()]).health();
    assert_eq!(health.state, HealthState::BadKey);
    assert!(!format!("{health:?}").contains(KEY));

    let idle =
        Fake::start(|_| Reply::json(503, r#"{"error": "offline", "message": "the image server is not running"}"#));
    assert_eq!(client(vec![idle.base.clone()]).models().unwrap_err(), Error::Offline);
    let health = client(vec![idle.base.clone()]).health();
    assert_eq!(health.state, HealthState::Offline);
    // Nexus's own words for it.
    let nexus = Fake::start(|_| Reply::json(503, r#"{"error": "not_serving", "message": "no node is serving images"}"#));
    assert_eq!(client(vec![nexus.base.clone()]).status().unwrap_err(), Error::Offline);
    let silent = Fake::start(|_| Reply::json(502, r#"{"error": "not_serving", "message": "the image server didn't answer"}"#));
    assert_eq!(client(vec![silent.base.clone()]).status().unwrap_err(), Error::Offline);
    assert_eq!(health.url.as_deref(), Some(idle.base.as_str()));
    assert!(health.message.contains("Nexus › Inference"));
}

#[test]
fn an_address_that_cannot_be_reached_passes_to_the_next_which_is_kept() {
    let fake = the_gateway();
    let home = nowhere();
    let client = client(vec![home.clone(), fake.base.clone()]);
    assert_eq!(client.nodes().unwrap(), catalogue());
    assert_eq!(client.base(), Some(fake.base.as_str()));
    client.models().unwrap();
    assert_eq!(fake.seen().len(), 2);
    let health = client.health();
    assert_eq!(health.state, HealthState::Serving);
    assert_eq!(health.url.as_deref(), Some(fake.base.as_str()));
    assert!(health.status.is_some() && health.ms.is_some());
}

#[test]
fn with_no_address_answering_each_ones_reason_is_said() {
    let (home, away) = (nowhere(), nowhere());
    let error = client(vec![home.clone(), away.clone()]).nodes().unwrap_err();
    let Error::Unreachable(why) = &error else { panic!("{error:?}") };
    assert_eq!(why.len(), 2);
    assert!(why[0].starts_with(&home) && why[1].starts_with(&away));
    let health = client(vec![home, away]).health();
    assert_eq!(health.state, HealthState::Unreachable);
    assert_eq!(health.url, None);
    assert_eq!(client(vec![]).nodes().unwrap_err(), Error::Unreachable(vec!["no gateway address is set".into()]));
}

#[test]
fn an_address_that_answers_with_an_error_has_answered() {
    let broken =
        Fake::start(|_| Reply::json(500, r#"{"error": "internal", "message": "CUDA error: device-side assert"}"#));
    let fake = the_gateway();
    let error = client(vec![broken.base.clone(), fake.base.clone()]).nodes().unwrap_err();
    assert_eq!(
        error,
        Error::Server { status: 500, code: "internal".into(), message: "CUDA error: device-side assert".into() }
    );
    assert!(fake.seen().is_empty());
}

#[test]
fn a_run_sends_the_graph_and_a_refusal_says_which_nodes() {
    let fake = the_gateway();
    let client = client(vec![fake.base.clone()]);
    let graph = sdxl().graph(&catalogue(), None).unwrap();
    let queued = client.run(&graph).unwrap();
    assert_eq!((queued.run.as_str(), queued.position), ("r8", 0));
    let seen = fake.seen();
    assert_eq!(seen[0].header("content-type"), Some("application/json"));
    let sent: serde_json::Value = serde_json::from_slice(&seen[0].body).unwrap();
    assert_eq!(sent, serde_json::json!({ "graph": graph }));

    let refusing = Fake::start(|_| {
        Reply::json(400, r#"{"error": "invalid", "nodes": {"1": "no checkpoint dreamshaper_8.safetensors"}}"#)
    });
    let error =
        proton_diffuse::Client::new(vec![refusing.base.clone()], Key::new(KEY).unwrap()).run(&graph).unwrap_err();
    let Error::Invalid(nodes) = &error else { panic!("{error:?}") };
    assert_eq!(nodes["1"], "no checkpoint dreamshaper_8.safetensors");
    assert!(error.describe().contains("node 1"));
}

#[test]
fn a_runs_events_are_read_to_its_end() {
    let fake = the_gateway();
    let events: Vec<Event> = client(vec![fake.base.clone()]).events("r8").unwrap().map(Result::unwrap).collect();
    // 22 events in the recording; the heartbeat is a kind this Proton skips.
    assert_eq!(events.len(), 21);
    assert_eq!(events[0], Event::Queued { position: 1 });
    assert_eq!(events[2], Event::Started);
    assert_eq!(events[3], Event::Node { id: "1".into(), state: NodeState::Cached });
    assert!(events.contains(&Event::Progress { id: "5".into(), step: 20, steps: 30 }));
    assert!(events.iter().any(|e| matches!(e, Event::Preview { step: 20, .. })));
    assert!(events.contains(&Event::Image {
        id: "7".into(),
        image: "img_7f3a91".into(),
        index: 0,
        width: 1024,
        height: 1024
    }));
    assert_eq!(events.last(), Some(&Event::Done { seconds: 14.2 }));
}

#[test]
fn events_arrive_as_they_are_sent() {
    let fake = Fake::start(|_| {
        Reply::stream(vec![
            (Duration::ZERO, "event: started\ndata: {}\n\n"),
            (Duration::from_millis(600), "event: done\ndata: {\"seconds\": 0.6}\n\n"),
        ])
    });
    let start = Instant::now();
    let mut events = client(vec![fake.base.clone()]).events("r9").unwrap();
    assert_eq!(events.next().unwrap().unwrap(), Event::Started);
    let first = start.elapsed();
    assert_eq!(events.next().unwrap().unwrap(), Event::Done { seconds: 0.6 });
    let second = start.elapsed();
    assert!(second - first >= Duration::from_millis(400), "{first:?} then {second:?}");
    assert!(events.next().is_none());
}

#[test]
fn a_stream_that_stops_early_says_so_and_an_error_ends_the_run() {
    let stopping = Fake::start(|_| Reply::stream(vec![(Duration::ZERO, "event: started\ndata: {}\n\n")]));
    let read: Vec<_> = client(vec![stopping.base.clone()]).events("r9").unwrap().collect();
    assert_eq!(read.len(), 2);
    assert_eq!(read[1], Err(Error::Lost("the stream ended before the run did".into())));

    let text = "event: error\ndata: {\"id\": \"6\", \"error\": \"oom\", \"message\": \"CUDA out of memory at 2048x2048\"}\n\nevent: done\ndata: {\"seconds\": 1}\n\n";
    let read: Vec<_> = Events::new(Box::new(Cursor::new(text))).collect();
    assert_eq!(
        read,
        [Ok(Event::Error {
            id: Some("6".into()),
            error: "oom".into(),
            message: "CUDA out of memory at 2048x2048".into()
        })]
    );
}

#[test]
fn the_stream_is_read_as_server_sent_events_are_written() {
    // \r\n endings, a comment, data over two lines, an id and a retry, an
    // event with no data at all.
    let text = ": hello\r\nid: 1\r\nretry: 3000\r\nevent: started\r\n\r\nevent: error\r\ndata: {\"error\": \"internal\",\r\ndata:  \"message\": \"two lines\"}\r\n\r\n";
    let read: Vec<_> = Events::new(Box::new(Cursor::new(text))).collect();
    assert_eq!(
        read,
        [Ok(Event::Started), Ok(Event::Error { id: None, error: "internal".into(), message: "two lines".into() })]
    );
    // Data that is not an object is not a run's event.
    let read: Vec<_> = Events::new(Box::new(Cursor::new("event: done\ndata: 12\n\n"))).collect();
    assert!(matches!(read[..], [Err(Error::Unreadable(_))]));
}

#[test]
fn images_go_up_and_come_down_and_a_run_is_cancelled() {
    let fake = the_gateway();
    let client = client(vec![fake.base.clone()]);
    let png = b"\x89PNG\r\n\x1a\nharbour".to_vec();
    let upload = client.upload(&png).unwrap();
    assert_eq!(upload.id, "up_31");
    let image = client.image("img_7f3a91").unwrap();
    assert!(image.starts_with(b"\x89PNG"));
    client.cancel("r8").unwrap();
    let seen = fake.seen();
    assert_eq!(seen[0].header("content-type"), Some("image/png"));
    assert_eq!(seen[0].body, png);
    assert_eq!(seen[2].path, "/img/v1/runs/r8/cancel");
    // Nothing that is not a picture is sent, and no id that is not one.
    assert!(client.upload(b"GIF89a").is_err());
    assert!(client.image("../../etc/passwd").is_err());
    assert!(client.events("r8/../x").is_err());
    assert_eq!(fake.seen().len(), 3);
}

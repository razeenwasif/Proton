//! What forge served last, kept so graphs edit while it is off.

mod common;

use common::server::{nowhere, Fake, Reply};
use common::{catalogue, models};
use proton_diffuse::cache::look;
use proton_diffuse::{Client, HealthState, Key};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("proton-diffuse-cache-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn client(base: &str) -> Client {
    Client::new(vec![base.to_string()], Key::new("nx-test-0123456789").unwrap())
}

fn serving() -> Fake {
    Fake::start(|seen| match seen.path.as_str() {
        "/img/v1/nodes" => Reply::json(200, include_str!("fixtures/catalogue-v1.json")),
        "/img/v1/models" => Reply::json(200, include_str!("fixtures/models.json")),
        _ => Reply::json(200, r#"{"gpu": {"name": "RTX 4090", "total": 24, "used": 7}}"#),
    })
}

#[test]
fn what_forge_says_is_kept_and_read_back_when_it_is_off() {
    let dir = scratch("kept");
    let fake = serving();
    let known = look(Some(&client(&fake.base)), &dir);
    assert_eq!(known.health.state, HealthState::Serving);
    assert!(known.fresh);
    assert_eq!(known.catalogue, Some(catalogue()));
    assert!(dir.join("catalogue.json").is_file() && dir.join("models.json").is_file());

    let idle = Fake::start(|_| Reply::json(503, r#"{"error": "offline"}"#));
    let known = look(Some(&client(&idle.base)), &dir);
    assert_eq!(known.health.state, HealthState::Offline);
    assert!(!known.fresh);
    assert_eq!(known.catalogue, Some(catalogue()));
    assert_eq!(known.models, Some(models()));

    // No key: nothing is asked, and what was kept is still there.
    let known = look(None, &dir);
    assert_eq!(known.health.state, HealthState::NoKey);
    assert!(known.health.message.contains("NEXUS_KEY"));
    assert_eq!(known.catalogue, Some(catalogue()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn with_nothing_kept_and_nothing_answering_there_is_no_catalogue() {
    let dir = scratch("none");
    let known = look(Some(&client(&nowhere())), &dir);
    assert_eq!(known.health.state, HealthState::Unreachable);
    assert_eq!((known.catalogue, known.models, known.fresh), (None, None, false));
}

#[test]
fn a_catalogue_the_server_garbles_falls_back_to_what_was_kept() {
    let dir = scratch("garbled");
    look(Some(&client(&serving().base)), &dir);
    let garbling = Fake::start(|seen| match seen.path.as_str() {
        "/img/v1/nodes" => Reply::json(200, r#"{"version": "one"}"#),
        _ => Reply::json(200, r#"{"gpu": {"name": "RTX 4090", "total": 24, "used": 7}}"#),
    });
    let known = look(Some(&client(&garbling.base)), &dir);
    assert_eq!(known.health.state, HealthState::Failing);
    assert!(known.health.message.contains("catalogue"));
    assert_eq!(known.catalogue, Some(catalogue()));
    let _ = std::fs::remove_dir_all(&dir);
}

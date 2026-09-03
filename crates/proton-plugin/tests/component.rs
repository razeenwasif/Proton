//! A real plugin, loaded and run.
//!
//! The other half of M5's exit criterion. `tests/plugins/hello` is a
//! component built from `examples/hello-plugin` and committed, so these tests
//! need no wasm toolchain — rebuilding it does, and that is written down in
//! that folder's README.
//!
//! What is being checked is the seam rather than the plugin: that a manifest
//! is read without running anything, that what the plugin declares comes back
//! across the component ABI intact, that a host call reaches the host, and
//! that a plugin which will not stop is stopped even though it is a *real*
//! plugin doing it in a real Rust loop.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use proton_plugin::{Host, Lent, Loaded, Outcome};

fn plugins() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/plugins")
}

fn hello() -> (Host, Loaded) {
    let host = Host::new().expect("an engine");
    let (found, problems) = proton_plugin::discover(&plugins());
    assert!(problems.is_empty(), "{problems:?}");
    let installed = found
        .into_iter()
        .find(|p| p.manifest.id == "hello")
        .expect("the example plugin");
    let wasm = installed.manifest.wasm.clone().expect("it has code");
    let loaded = Loaded::load(&host, installed, &wasm).expect("it should load");
    (host, loaded)
}

#[test]
fn a_plugin_is_discovered_from_its_manifest_alone() {
    // Nothing runs here. A plugin that only ships a theme should never have
    // its code executed, and finding out what it contributes by *calling* it
    // would mean running everything installed at startup.
    let (found, problems) = proton_plugin::discover(&plugins());
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].manifest.name, "Hello");
    assert_eq!(found[0].manifest.wasm.as_deref(), Some("hello.wasm"));
}

#[test]
fn what_a_plugin_declares_survives_the_crossing() {
    // A record with a list inside it, and a list of records. Between them
    // that is the canonical ABI doing its job — the shapes a hand-rolled
    // pointer-and-length interface gets wrong.
    let (_host, plugin) = hello();

    let theme = &plugin.themes()[0];
    assert_eq!(theme.id, "hello-dusk");
    assert!(theme.dark);
    let bg = theme
        .colours
        .iter()
        .find(|s| s.name == "bg")
        .expect("a background");
    assert_eq!(bg.value, "#161821");

    let ids: Vec<&str> = plugin.commands().iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, vec!["hello.greet", "hello.count", "hello.forever"]);
}

#[test]
fn a_command_runs_and_answers_in_the_shape_it_chose() {
    let (host, plugin) = hello();
    let out = plugin
        .run(&host, "hello.greet", Lent::default())
        .expect("it should run");
    match out {
        Outcome::Insert(text) => assert_eq!(text, "hello from a plugin\n"),
        other => panic!("expected an insert, got {other:?}"),
    }
}

#[test]
fn a_plugin_can_ask_the_host_what_it_cannot_see() {
    // The point of the import side of the world. A plugin has no view of the
    // buffer at all — it is outside the sandbox — so the only way it can
    // count the lines is to ask.
    let (host, plugin) = hello();
    let out = plugin
        .run(
            &host,
            "hello.count",
            Lent {
                buffer: "one\ntwo\nthree\n".into(),
                cursor: (1, 2),
                root: "/p".into(),
            },
        )
        .expect("it should run");
    match out {
        // One-based in the message, because that is what a gutter shows.
        Outcome::Message(said) => assert_eq!(said, "3 lines, cursor at 2:3"),
        other => panic!("expected a message, got {other:?}"),
    }
}

#[test]
fn a_real_plugin_that_will_not_stop_is_stopped() {
    // The exit criterion, with a plugin that loops on purpose in compiled
    // Rust rather than in three lines of hand-written wasm. Without the
    // deadline this test does not fail — it hangs, and so does the editor.
    let (host, plugin) = hello();

    let started = Instant::now();
    let why = plugin
        .run(&host, "hello.forever", Lent::default())
        .expect_err("an infinite loop must not return Ok");
    let took = started.elapsed();

    assert!(
        took < Duration::from_secs(2),
        "it should have been stopped promptly, took {took:?}"
    );
    // And said so in words a reader knows, rather than passing on the
    // engine's "epoch deadline reached".
    assert!(
        why.contains("did not finish in time"),
        "the message should say what happened: {why}"
    );
    assert!(why.starts_with("hello:"), "and which plugin it was: {why}");
}

#[test]
fn the_editor_is_still_usable_after_a_plugin_hangs() {
    // The guarantee is not that the *call* ends; it is that the editor keeps
    // working. So the same plugin is asked for something else afterwards and
    // has to answer normally.
    let (host, plugin) = hello();
    assert!(plugin.run(&host, "hello.forever", Lent::default()).is_err());

    let out = plugin
        .run(&host, "hello.greet", Lent::default())
        .expect("the next call should be fine");
    assert!(matches!(out, Outcome::Insert(_)));
}

#[test]
fn a_command_the_plugin_never_declared_is_refused_without_running_it() {
    // Checked against what it declared rather than passed through: a plugin
    // should not be handed an id it never offered, and an unknown command is
    // the editor's mistake to catch.
    let (host, plugin) = hello();
    let why = plugin
        .run(&host, "hello.nope", Lent::default())
        .expect_err("it should refuse");
    assert!(why.contains("no command hello.nope"), "{why}");
}

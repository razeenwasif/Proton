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

    // Sorted by folder, so the list reads the same every run — and load order
    // is what decides which of two plugins claiming one theme id wins.
    let ids: Vec<&str> = found.iter().map(|p| p.manifest.id.as_str()).collect();
    assert_eq!(ids, vec!["dusk", "hello"]);

    let hello = found.iter().find(|p| p.manifest.id == "hello").unwrap();
    assert_eq!(hello.manifest.name, "Hello");
    assert_eq!(hello.manifest.wasm.as_deref(), Some("hello.wasm"));
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

/* ── a plugin with no code at all ─────────────────────────────────── */

#[test]
fn a_theme_pack_needs_no_component_and_no_toolchain() {
    // Half of M5's exit criterion, and the half that should be easiest: a
    // palette is a list of colours, so shipping one should not mean writing a
    // program or compiling anything. `tests/plugins/dusk` is three TOML files.
    let (found, problems) = proton_plugin::discover(&plugins());
    assert!(problems.is_empty(), "{problems:?}");

    let pack = found
        .iter()
        .find(|p| p.manifest.id == "dusk")
        .expect("the theme pack");
    assert!(pack.manifest.wasm.is_none(), "no code");
    assert_eq!(pack.manifest.themes.len(), 2);

    let rose = proton_plugin::ThemeView::read(pack, "rose.toml").expect("a theme");
    assert_eq!(rose.id, "dusk-rose");
    assert_eq!(rose.name, "Dusk Rose");
    assert!(rose.dark);
    assert_eq!(rose.from, "dusk", "it says which plugin brought it");
    let bg = rose
        .colours
        .iter()
        .find(|(name, _)| name == "bg")
        .expect("a background");
    assert_eq!(bg.1, "#1f1a1d");
}

#[test]
fn a_theme_that_is_not_colours_is_refused_by_name() {
    // Checked where the file is read, not where it is used: a token that is
    // not a colour reaches the page as a CSS value that silently does
    // nothing, and "one thing is the wrong colour" is much harder to work out
    // than "this theme is broken, and here is the token".
    let dir = std::env::temp_dir().join(format!("proton-badtheme-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("bad");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("proton.toml"),
        "id = \"bad\"\nname = \"Bad\"\nthemes = [\"t.toml\"]\n",
    )
    .unwrap();
    std::fs::write(
        root.join("t.toml"),
        "id = \"t\"\nname = \"T\"\n\n[colours]\nbg = \"rebeccapurple\"\n",
    )
    .unwrap();

    let installed = proton_plugin::manifest::read(&root).expect("a manifest");
    let why = proton_plugin::ThemeView::read(&installed, "t.toml").expect_err("it should refuse");
    assert!(why.contains("bg"), "it should name the token: {why}");
    assert!(why.contains("not a colour"), "{why}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_theme_cannot_be_read_from_outside_its_plugin() {
    // The same rule the component path follows. A manifest is a file somebody
    // else wrote, and a path in it is untrusted input.
    let (found, _) = proton_plugin::discover(&plugins());
    let pack = found.iter().find(|p| p.manifest.id == "dusk").unwrap();
    let why =
        proton_plugin::ThemeView::read(pack, "../hello/proton.toml").expect_err("it should refuse");
    assert!(why.contains("outside the plugin"), "{why}");
}

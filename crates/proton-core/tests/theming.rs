//! Design mode's half of the two-way binding.
//!
//! M7's exit is stated in two directions: editing a token in the inspector
//! changes `theme.css`, and editing the component in Codex updates the
//! artboard. Both are here now:
//!
//! - a token set through the inspector reaches the disk,
//! - the same file saved as text in Codex reaches the inspector, and
//! - a component saved in Codex reaches the artboard.
//!
//! The thing worth guarding hardest is that neither direction reformats. A
//! palette editor that rewrote the stylesheet would make every colour tweak a
//! whole-file diff, and every mode's document diffing in Git is the rule the
//! project is built on.

use proton_core::bus::Event;
use proton_core::key::Key;
use proton_core::{Command, ModeId, Workspace};

/// Type, the way somebody types.
fn keys(space: &mut Workspace, text: &str) {
    for c in text.chars() {
        space.apply(Command::Key { key: Key::char(c) });
    }
}

const SHEET: &str = r#"/* The project's palette. */
:root {
  --bg: #16161e;
  --accent: #7aa2f7;
}

:root[data-theme="dark"] {
  --accent: #89b4fa;
}

.card { color: var(--accent); }
"#;

/// A workspace rooted somewhere disposable, with a `theme.css` in it.
fn open(name: &str) -> (Workspace, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "proton-theme-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("theme.css");
    std::fs::write(&path, SHEET).unwrap();

    (Workspace::rooted(&root), path)
}

/// The same, with a component for the artboard to draw.
fn open_with_component(name: &str, markup: &str) -> (Workspace, std::path::PathBuf) {
    let (_space, theme) = open(name);
    let root = theme.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(root.join("components")).unwrap();
    let path = root.join("components").join("card.html");
    std::fs::write(&path, markup).unwrap();
    // Rebuilt so the walk that looks for a component sees the one just
    // written — `open` made the workspace before this existed.
    (Workspace::rooted(&root), path)
}

/// What the artboard would draw.
fn artboard(space: &mut Workspace) -> (String, String, String) {
    space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Artboard {
                path,
                source,
                problem,
                ..
            } => Some((path.clone(), source.clone(), problem.clone())),
            _ => None,
        })
        .expect("an artboard event")
}

/// The tokens the inspector would draw.
fn theme(space: &mut Workspace) -> (Vec<(String, String, String)>, usize) {
    space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Theme {
                tokens, selected, ..
            } => Some((
                tokens
                    .iter()
                    .map(|t| (t.name.clone(), t.scope.clone(), t.value.clone()))
                    .collect(),
                *selected,
            )),
            _ => None,
        })
        .expect("a theme event")
}

#[test]
fn entering_design_mode_finds_the_project_theme() {
    // Nobody should have to point an inspector at their own stylesheet before
    // it will show anything.
    let (mut space, _) = open("discover");
    space.apply(Command::SetMode {
        mode: ModeId::Design,
    });

    let (tokens, _) = theme(&mut space);
    assert_eq!(tokens.len(), 3, "{tokens:?}");
    assert_eq!(tokens[0].0, "--bg");
}

#[test]
fn a_token_set_in_the_inspector_reaches_the_disk() {
    // The milestone, in one test.
    let (mut space, path) = open("write");
    space.apply(Command::SetMode {
        mode: ModeId::Design,
    });
    space.apply(Command::SetToken {
        name: "--accent".into(),
        scope: ":root".into(),
        value: "#ff0088".into(),
    });

    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("--accent: #ff0088;"), "{after}");
}

#[test]
fn writing_a_token_does_not_reformat_the_file() {
    // What makes it reviewable. One value moves; the comment, the blank line
    // and the rule at the bottom are all where they were.
    let (mut space, path) = open("diff");
    space.apply(Command::SetMode {
        mode: ModeId::Design,
    });
    space.apply(Command::SetToken {
        name: "--accent".into(),
        scope: ":root".into(),
        value: "#ff0088".into(),
    });

    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        after,
        SHEET.replacen("--accent: #7aa2f7;", "--accent: #ff0088;", 1),
        "the write touched something other than the one value"
    );
}

#[test]
fn the_scope_decides_which_palette_is_written() {
    // `--accent` is declared twice on purpose. An inspector that keyed on the
    // name would edit whichever the scanner reached first.
    let (mut space, path) = open("scope");
    space.apply(Command::SetMode {
        mode: ModeId::Design,
    });
    space.apply(Command::SetToken {
        name: "--accent".into(),
        scope: ":root[data-theme=\"dark\"]".into(),
        value: "#000000".into(),
    });

    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.contains("--accent: #7aa2f7;"), "light was touched");
    assert!(after.contains("--accent: #000000;"), "dark was not written");
}

#[test]
fn setting_a_token_that_is_not_there_says_nothing_and_writes_nothing() {
    let (mut space, path) = open("absent");
    space.apply(Command::SetMode {
        mode: ModeId::Design,
    });
    let events = space.apply(Command::SetToken {
        name: "--nope".into(),
        scope: ":root".into(),
        value: "#fff".into(),
    });
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), SHEET);
}

#[test]
fn setting_the_value_it_already_has_does_not_touch_the_file() {
    // A colour picker emits on every mouse move. Each one must not be a write.
    let (mut space, path) = open("noop");
    space.apply(Command::SetMode {
        mode: ModeId::Design,
    });
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();

    let events = space.apply(Command::SetToken {
        name: "--accent".into(),
        scope: ":root".into(),
        value: "#7aa2f7".into(),
    });
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before,
        "the file was rewritten with the bytes it already had"
    );
}

#[test]
fn editing_the_theme_in_codex_reaches_the_inspector() {
    // The other direction. Open the same file as text, change it, save, and
    // the inspector must be showing the new palette rather than the one the
    // file had when Design mode was entered.
    let (mut space, _) = open("codex");
    space.apply(Command::SetMode {
        mode: ModeId::Design,
    });
    assert_eq!(theme(&mut space).0[1].2, "#7aa2f7");

    space.apply(Command::OpenBuffer {
        path: "theme.css".into(),
    });
    // Into Codex to type it. Only Codex edits text — every other mode resolves
    // the action and declines to run it — so this is not incidental to the
    // test, it *is* what "editing the component in Codex" means.
    space.apply(Command::SetMode {
        mode: ModeId::Codex,
    });
    // Typed, not injected: this is the path a person takes, and it is the one
    // that has to end in the inspector noticing.
    for key in "Go:root { --extra: #00ff00; }".chars() {
        space.apply(Command::Key {
            key: proton_core::Key::char(key),
        });
    }
    space.apply(Command::Key {
        key: proton_core::Key::named("Escape"),
    });
    space.apply(Command::Save);

    let (tokens, _) = theme(&mut space);
    assert!(
        tokens
            .iter()
            .any(|(name, _, value)| name == "--extra" && value == "#00ff00"),
        "the inspector did not see what Codex saved: {tokens:?}"
    );
}

#[test]
fn selecting_a_token_that_is_not_there_lands_somewhere_real() {
    let (mut space, _) = open("select");
    space.apply(Command::SetMode {
        mode: ModeId::Design,
    });
    space.apply(Command::SelectToken { index: 99 });

    let (tokens, selected) = theme(&mut space);
    assert_eq!(selected, tokens.len() - 1, "clamped to the last token");
}

#[test]
fn a_stylesheet_with_no_tokens_says_so_rather_than_showing_nothing() {
    // An empty inspector that does not explain itself is the failure people
    // cannot debug.
    let (mut space, path) = open("empty");
    std::fs::write(&path, "body { margin: 0; }\n").unwrap();
    space.apply(Command::OpenTheme {
        path: "theme.css".into(),
    });

    let problem = space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Theme { problem, .. } => Some(problem.clone()),
            _ => None,
        })
        .expect("a theme event");
    assert!(!problem.is_empty(), "no reason was given");
}

#[test]
fn design_mode_does_not_take_css_away_from_the_editor() {
    // M7's second half needs Codex to keep opening these files as text. If
    // `.css` switched modes the way `.ipynb` does, the binding would only
    // ever work in one direction.
    let (mut space, _) = open("nohijack");
    space.apply(Command::OpenBuffer {
        path: "theme.css".into(),
    });
    assert_eq!(
        space.mode(),
        ModeId::Codex,
        "opening a stylesheet should stay in the editor"
    );
    assert!(space
        .editor()
        .buffer()
        .text()
        .to_string()
        .contains("--accent"));
}

#[test]
fn entering_design_mode_finds_a_component_for_the_artboard() {
    // Same reasoning as finding the theme: an artboard that is blank until
    // somebody navigates to their own file is a worse first run than one that
    // guesses at a conventional place and shows what it picked.
    let (mut space, path) = open_with_component("find-component", "<div class=card>Hi</div>");
    space.apply(Command::SetMode {
        mode: ModeId::Design,
    });

    let (shown, source, problem) = artboard(&mut space);
    assert!(problem.is_empty(), "{problem}");
    assert!(
        shown.ends_with("card.html"),
        "{shown} — wanted {}",
        path.display()
    );
    assert_eq!(source, "<div class=card>Hi</div>");
}

#[test]
fn a_component_saved_in_codex_reaches_the_artboard() {
    // The direction the milestone is stated in, and the one that cannot be
    // faked: the file is edited as text in the editor, saved, and the artboard
    // has to be holding the new markup afterwards.
    let (mut space, path) = open_with_component("codex-to-artboard", "<div>before</div>");
    space.apply(Command::SetMode {
        mode: ModeId::Design,
    });
    assert_eq!(artboard(&mut space).1, "<div>before</div>");

    // Opened, edited and saved the way any other file is: real keystrokes
    // through the editor, not a write behind its back. `o` opens a line below
    // and starts insert.
    // In Codex, because that is where the milestone says the editing happens
    // and because keystrokes in Design mode belong to the inspector.
    space.apply(Command::SetMode {
        mode: ModeId::Codex,
    });
    space.apply(Command::OpenBuffer {
        path: path.display().to_string(),
    });
    keys(&mut space, "o<p>added</p>");
    space.apply(Command::Key {
        key: Key::named("Escape"),
    });
    space.apply(Command::Save);

    let (_, source, problem) = artboard(&mut space);
    assert!(problem.is_empty(), "{problem}");
    assert!(
        source.contains("<p>added</p>"),
        "the artboard is still showing the file as it was when Design mode \
         opened: {source:?}"
    );
}

#[test]
fn a_component_that_needs_building_says_so_on_the_artboard() {
    // The answer belongs where somebody is looking for the component. A blank
    // artboard would read as "your component renders to nothing", which is a
    // different and much more alarming thing than "this file is JSX".
    let (mut space, _) = open_with_component("needs-building", "<div>fine</div>");
    space.apply(Command::SetMode {
        mode: ModeId::Design,
    });
    space.apply(Command::ShowComponent {
        path: "components/Card.jsx".into(),
    });

    let (_, source, problem) = artboard(&mut space);
    assert!(source.is_empty(), "it drew something it cannot render");
    assert!(problem.contains("built"), "{problem}");
}

#[test]
fn the_artboard_sends_source_rather_than_anything_rendered() {
    // The security property the iframe depends on. If the core ever starts
    // handing the shell assembled markup, the temptation to write it straight
    // into the document — where `window.__TAURI__` lives — becomes very hard
    // to resist. What crosses the seam is the file, byte for byte.
    let markup = "<div>\n  <script>alert(1)</script>\n</div>";
    let (mut space, _) = open_with_component("verbatim", markup);
    space.apply(Command::SetMode {
        mode: ModeId::Design,
    });

    assert_eq!(
        artboard(&mut space).1,
        markup,
        "the core altered the source on its way out"
    );
}

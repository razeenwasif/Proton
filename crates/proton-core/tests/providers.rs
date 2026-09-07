//! Switching the agent between providers.
//!
//! The claim: what `.proton/settings.json` says is what the next question
//! goes to. Not the next launch — a provider switch that needed a restart
//! would be a setting nobody trusts.

use proton_core::bus::Event;
use proton_core::{Command, Workspace};
use serde_json::json;

fn open(name: &str) -> (Workspace, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "proton-prov-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    (Workspace::rooted(&root), root)
}

/// What the agent pane's header would say.
fn shown(space: &mut Workspace) -> (String, String) {
    space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Agent {
                provider, model, ..
            } => Some((provider.clone(), model.clone())),
            _ => None,
        })
        .expect("an agent event")
}

#[test]
fn the_default_is_claude_with_its_own_default_model() {
    let (mut space, _) = open("default");
    let (provider, model) = shown(&mut space);
    assert_eq!(provider, "claude");
    assert!(model.starts_with("claude-"), "{model}");
}

#[test]
fn choosing_gemini_changes_what_the_next_question_goes_to() {
    let (mut space, _) = open("switch");
    space.apply(Command::SetSetting {
        key: "agent.provider".into(),
        value: json!("gemini"),
    });

    let (provider, model) = shown(&mut space);
    assert_eq!(provider, "gemini");
    assert!(
        model.starts_with("gemini-"),
        "the model should follow the provider, got {model}"
    );
}

#[test]
fn a_named_model_wins_over_the_providers_default() {
    let (mut space, _) = open("named");
    space.apply(Command::SetSetting {
        key: "agent.provider".into(),
        value: json!("gemini"),
    });
    space.apply(Command::SetSetting {
        key: "agent.model".into(),
        value: json!("gemini-2.5-flash"),
    });
    assert_eq!(shown(&mut space).1, "gemini-2.5-flash");
}

#[test]
fn clearing_the_model_goes_back_to_the_providers_default() {
    // The settings panel sends `null` for an emptied field.
    let (mut space, _) = open("clear");
    space.apply(Command::SetSetting {
        key: "agent.model".into(),
        value: json!("something-custom"),
    });
    assert_eq!(shown(&mut space).1, "something-custom");

    space.apply(Command::SetSetting {
        key: "agent.model".into(),
        value: json!(null),
    });
    assert!(shown(&mut space).1.starts_with("claude-"));
}

#[test]
fn a_setting_from_a_previous_session_is_honoured_on_startup() {
    // The one that would break silently: the file says Gemini, and the first
    // question of the session goes to Claude because nothing read it.
    let (_, root) = open("startup");
    std::fs::create_dir_all(root.join(".proton")).unwrap();
    std::fs::write(
        root.join(".proton").join("settings.json"),
        r#"{"agent": {"provider": "gemini"}}"#,
    )
    .unwrap();

    let mut fresh = Workspace::rooted(&root);
    assert_eq!(shown(&mut fresh).0, "gemini");
}

#[test]
fn a_provider_this_build_does_not_know_starts_anyway() {
    // A settings file from a newer Proton should not stop the editor opening.
    let (_, root) = open("unknown");
    std::fs::create_dir_all(root.join(".proton")).unwrap();
    std::fs::write(
        root.join(".proton").join("settings.json"),
        r#"{"agent": {"provider": "some-future-thing"}}"#,
    )
    .unwrap();

    let mut fresh = Workspace::rooted(&root);
    assert_eq!(shown(&mut fresh).0, "claude", "fell back to the default");
}

//! What the settings panel says about a credential.
//!
//! Two things it must never do: print one, and claim there is one when there
//! is not. The panel is the only place that says whether a key was found, so
//! it has to be right before a question is asked rather than after one fails.

use proton_core::bus::Event;
use proton_core::{Command, Workspace};
use serde_json::json;

fn open(name: &str) -> Workspace {
    let root = std::env::temp_dir().join(format!(
        "proton-cred-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    Workspace::rooted(&root)
}

fn credential(space: &mut Workspace) -> String {
    space
        .apply(Command::Sync)
        .iter()
        .find_map(|event| match event {
            Event::Agent { credential, .. } => Some(credential.clone()),
            _ => None,
        })
        .expect("an agent event")
}

#[test]
fn it_never_contains_the_credential_itself() {
    // This string is drawn on screen and ends up in screenshots.
    std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-secret-value-do-not-print");
    let mut space = open("secret");
    let said = credential(&mut space);
    std::env::remove_var("ANTHROPIC_API_KEY");

    assert!(!said.contains("secret-value"), "{said}");
    assert!(!said.contains("sk-ant"), "{said}");
    assert!(
        said.contains("ANTHROPIC_API_KEY"),
        "it should name the source: {said}"
    );
}

#[test]
fn it_follows_the_provider() {
    // Switching to Gemini and being told about ANTHROPIC_API_KEY would be
    // worse than being told nothing.
    std::env::set_var("GEMINI_API_KEY", "g-secret");
    let mut space = open("gemini");
    space.apply(Command::SetSetting {
        key: "agent.provider".into(),
        value: json!("gemini"),
    });
    let said = credential(&mut space);
    std::env::remove_var("GEMINI_API_KEY");

    assert!(said.contains("GEMINI_API_KEY"), "{said}");
    assert!(!said.contains("g-secret"), "{said}");
}

#[test]
fn with_nothing_set_it_says_what_to_set() {
    // "No credential" is not actionable; the name of the variable is.
    let mut space = open("none");
    space.apply(Command::SetSetting {
        key: "agent.provider".into(),
        value: json!("gemini"),
    });
    std::env::remove_var("GEMINI_API_KEY");
    std::env::remove_var("GOOGLE_API_KEY");

    let said = credential(&mut space);
    assert!(said.contains("GEMINI_API_KEY"), "{said}");
}

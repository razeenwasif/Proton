//! What the git panel is drawn from.
//!
//! Deliberately not a test of git. `proton-vcs` covers what a repository says;
//! this covers the seam — that the core asks, that the answer has the shape
//! the panel needs, and that a folder which is not a repository is an ordinary
//! answer rather than an error.

use proton_core::bus::Event;
use proton_core::{Command, Workspace};

/// The panel's answer: the branch at once, the changes when git has given
/// them — asked on a worker, and sent when they differ from the last.
fn repo_event(space: &mut Workspace) -> (Option<String>, Vec<(String, String)>) {
    let mut events = space.apply(Command::RefreshRepo);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
        let polled = space.apply(Command::Poll);
        let came = polled.iter().any(|event| matches!(event, Event::Repo { .. }));
        events.extend(polled);
        if came || !space.repo_asking() {
            break;
        }
    }
    events
        .iter()
        .rev()
        .find_map(|event| match event {
            Event::Repo { branch, files } => Some((
                branch.clone(),
                files
                    .iter()
                    .map(|f| (f.path.clone(), f.state.clone()))
                    .collect(),
            )),
            _ => None,
        })
        .expect("RefreshRepo should answer with a Repo event")
}

#[test]
fn a_folder_that_is_not_a_repository_is_an_ordinary_answer() {
    // Plenty of folders worth editing are not checkouts. The panel should say
    // so quietly rather than the command failing.
    let root = std::env::temp_dir().join(format!(
        "proton-norepo-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let mut space = Workspace::rooted(&root);
    let (branch, files) = repo_event(&mut space);
    assert_eq!(branch, None);
    assert!(files.is_empty(), "{files:?}");
}

#[test]
fn a_real_repository_reports_its_branch() {
    // Proton's own checkout, which is the repository this is running inside.
    let mut space = Workspace::rooted("../..");
    let (branch, _) = repo_event(&mut space);
    assert!(
        branch.is_some_and(|b| !b.is_empty()),
        "a checkout on a branch should name it"
    );
}

#[test]
fn every_change_has_a_state_letter_and_an_openable_path() {
    // The two things the panel depends on: a letter it can colour, and a path
    // it can hand straight to `openBuffer` without translating it.
    let mut space = Workspace::rooted("../..");
    let (_, files) = repo_event(&mut space);
    for (path, state) in &files {
        assert!(
            matches!(state.as_str(), "M" | "A" | "D" | "?"),
            "{path}: unexpected state {state:?}"
        );
        assert!(!path.is_empty());
        assert!(
            !path.contains('\\'),
            "{path}: paths are spelled with forward slashes, like openBuffer's"
        );
    }
}

#[test]
fn the_panel_is_told_on_sync_as_well_as_when_asked() {
    // Opening the panel asks, but a window that has just connected has not
    // opened anything yet — and a panel that is empty until you click it
    // twice is one people conclude is broken.
    let mut space = Workspace::rooted("../..");
    let events = space.apply(Command::Sync);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Repo { .. })),
        "Sync should carry the repository"
    );
}

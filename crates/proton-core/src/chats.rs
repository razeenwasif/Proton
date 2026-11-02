//! The agent dock's threads: conversations on disk.
//!
//! `.proton/chats/*.json`, one file per conversation, beside the settings and
//! the debug config. Every mode's document is a file that diffs in Git, and a
//! conversation that died with the process would be the one exception to the
//! rule the project is built on.
//!
//! ## The conversation is the document; the transcript is a view
//!
//! The agent keeps two things: `conversation`, which is what gets *sent* — the
//! real messages, including the thinking signatures the API validates and the
//! tool results — and `transcript`, which is what gets *drawn*. Only the first
//! is saved.
//!
//! Saving both would be two records of one thing, free to disagree the moment
//! either changed. The transcript is rebuilt on load instead, which is a
//! function that has to be right rather than a second file that has to be kept
//! in step. It is also what makes the file worth having: a conversation a
//! model can continue, and one the agent could edit by writing a patch.
//!
//! ## No clock
//!
//! `Workspace::apply` has no clock and this does not give it one. Ordering the
//! list by "most recent" uses the file's own modification time, which the
//! filesystem is already keeping and which stays true when a file is edited by
//! something that is not Proton.

use std::path::{Path, PathBuf};

use proton_agent::wire::Message;
use serde::{Deserialize, Serialize};

/// A conversation, as it sits in a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chat {
    /// Bumped only when the shape changes in a way an older build would read
    /// wrongly. A file it cannot read is skipped, not repaired.
    #[serde(default = "one")]
    pub version: u32,
    /// What to call it in the list. Taken from the first thing asked, and
    /// editable — it is a field in the file rather than the filename so that
    /// renaming does not move it.
    #[serde(default)]
    pub title: String,
    /// Which service produced it. A transcript you cannot attribute is one you
    /// cannot reason about later, and two providers answer differently.
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    /// The conversation as the API sees it. This is the document.
    #[serde(default)]
    pub messages: Vec<Message>,
}

fn one() -> u32 {
    1
}

impl Default for Chat {
    fn default() -> Chat {
        Chat {
            version: 1,
            title: String::new(),
            provider: String::new(),
            model: String::new(),
            messages: Vec::new(),
        }
    }
}

impl Chat {
    /// The file, as it will be written: two-space JSON with a trailing
    /// newline, so a conversation growing by one turn is a diff of that turn
    /// rather than of the whole file.
    pub fn to_text(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).unwrap_or_default();
        text.push('\n');
        text
    }
}

/// One row of the conversation list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    /// Relative to the project, so it survives the project moving.
    pub path: String,
    pub title: String,
    /// How many turns are in it, for the row's second line.
    pub turns: usize,
    pub provider: String,
}

/// Where conversations live.
pub fn dir(root: &Path) -> PathBuf {
    root.join(".proton").join("chats")
}

/// Every conversation, most recently changed first.
///
/// A file that will not parse is skipped rather than reported: the list is
/// drawn on every mode switch, and one bad file should not empty the sidebar
/// or raise a notice each time somebody looks at it.
pub fn list(root: &Path) -> Vec<Summary> {
    let dir = dir(root);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut found: Vec<(std::time::SystemTime, Summary)> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|it| it == "json"))
        .filter_map(|entry| {
            let path = entry.path();
            let chat = read(&path).ok()?;
            let when = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            Some((
                when,
                Summary {
                    path: relative(root, &path),
                    title: match chat.title.is_empty() {
                        true => name_of(&path),
                        false => chat.title.clone(),
                    },
                    turns: chat.messages.len(),
                    provider: chat.provider.clone(),
                },
            ))
        })
        .collect();

    // Newest first, so `sort_by_key` on the reverse of the timestamp.
    found.sort_by_key(|(when, _)| std::cmp::Reverse(*when));
    found.into_iter().map(|(_, summary)| summary).collect()
}

/// Read one conversation.
pub fn read(path: &Path) -> Result<Chat, String> {
    let text = std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("{}: {err}", path.display()))
}

/// Write one conversation, creating the directory if it is the first.
pub fn write(path: &Path, chat: &Chat) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("{}: {err}", parent.display()))?;
    }
    std::fs::write(path, chat.to_text()).map_err(|err| format!("{}: {err}", path.display()))
}

/// Delete one.
pub fn remove(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|err| format!("{}: {err}", path.display()))
}

/// A path for a new conversation, named after what it is about.
///
/// Readable rather than a uuid, because these are files somebody will see in a
/// diff and in a directory listing. A name already taken gets a number rather
/// than overwriting — two conversations can reasonably start with the same
/// question.
pub fn fresh_path(root: &Path, title: &str) -> PathBuf {
    let dir = dir(root);
    let stem = match slug(title) {
        stem if stem.is_empty() => "chat".to_string(),
        stem => stem,
    };
    let mut path = dir.join(format!("{stem}.json"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}.json"));
        n += 1;
    }
    path
}

/// A filename out of a title: lowercase words joined by dashes.
///
/// Deliberately strict about what it keeps. These become filenames on three
/// operating systems, and a title with a `?` or a `:` in it is the common
/// case for a question — Windows would refuse the file and the failure would
/// look like the conversation failing to save.
fn slug(title: &str) -> String {
    let mut out = String::new();
    for ch in title.chars() {
        match ch {
            'a'..='z' | '0'..='9' => out.push(ch),
            'A'..='Z' => out.extend(ch.to_lowercase()),
            _ if out.ends_with('-') || out.is_empty() => {}
            _ => out.push('-'),
        }
        if out.len() >= 48 {
            break;
        }
    }
    out.trim_matches('-').to_string()
}

/// A title out of the first thing asked.
pub fn title_from(text: &str) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let trimmed = line.trim();
    match trimmed.char_indices().nth(60) {
        Some((cut, _)) => format!("{}…", trimmed[..cut].trim_end()),
        None => trimmed.to_string(),
    }
}

fn name_of(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "proton-chats-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn a_title_becomes_a_filename_that_windows_will_accept() {
        // The common case for a chat title is a question, and `?` and `:` are
        // both illegal in a Windows filename. Getting this wrong looks like
        // the conversation failing to save.
        assert_eq!(slug("Why does the grid jank?"), "why-does-the-grid-jank");
        assert_eq!(slug("Fix: the parser"), "fix-the-parser");
        assert_eq!(slug("  spaced  out  "), "spaced-out");
        assert_eq!(slug("///"), "");
    }

    #[test]
    fn a_name_already_taken_gets_a_number_rather_than_overwriting() {
        let dir = root("collide");
        let first = fresh_path(&dir, "same question");
        write(&first, &Chat::default()).unwrap();
        let second = fresh_path(&dir, "same question");
        assert_ne!(first, second);
        assert!(second.to_string_lossy().contains("same-question-2"));
    }

    #[test]
    fn a_title_with_nothing_usable_in_it_still_makes_a_file() {
        let dir = root("unnameable");
        let path = fresh_path(&dir, "???");
        assert!(path.to_string_lossy().ends_with("chat.json"), "{path:?}");
    }

    #[test]
    fn the_title_is_the_first_line_of_the_first_question() {
        assert_eq!(
            title_from("why is it slow?\nmore detail"),
            "why is it slow?"
        );
        assert_eq!(title_from("\n\n  padded  "), "padded");
        let long = "a".repeat(100);
        assert!(title_from(&long).ends_with('…'));
        assert!(title_from(&long).chars().count() <= 61);
    }

    #[test]
    fn a_conversation_round_trips() {
        let dir = root("roundtrip");
        let chat = Chat {
            title: "why is it slow".into(),
            provider: "claude".into(),
            model: "claude-opus-5".into(),
            messages: vec![Message::user(vec![proton_agent::wire::Block::Text {
                text: "why is it slow".into(),
            }])],
            ..Chat::default()
        };
        let path = fresh_path(&dir, &chat.title);
        write(&path, &chat).unwrap();
        assert_eq!(read(&path).unwrap(), chat);
    }

    #[test]
    fn the_file_ends_with_a_newline_so_a_new_turn_is_a_small_diff() {
        let chat = Chat::default();
        assert!(chat.to_text().ends_with("}\n"));
    }

    #[test]
    fn the_list_puts_the_most_recently_changed_first() {
        let dir = root("order");
        for title in ["first", "second"] {
            let path = fresh_path(&dir, title);
            write(
                &path,
                &Chat {
                    title: title.into(),
                    ..Chat::default()
                },
            )
            .unwrap();
            // Filesystem timestamps are coarse; make the order unambiguous.
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let listed = list(&dir);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].title, "second");
    }

    #[test]
    fn a_file_that_will_not_parse_is_skipped_rather_than_emptying_the_list() {
        // The list is drawn on every mode switch. One bad file must not blank
        // the sidebar, and must not raise a notice each time somebody looks.
        let dir = root("broken");
        let good = fresh_path(&dir, "good");
        write(
            &good,
            &Chat {
                title: "good".into(),
                ..Chat::default()
            },
        )
        .unwrap();
        std::fs::write(super::dir(&dir).join("broken.json"), "{ not json").unwrap();

        let listed = list(&dir);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].title, "good");
    }

    #[test]
    fn a_file_from_a_newer_build_is_read_as_far_as_it_can_be() {
        // Unknown fields are ignored rather than fatal, so a conversation
        // written by a later Proton still opens.
        let dir = root("future");
        std::fs::create_dir_all(super::dir(&dir)).unwrap();
        let path = super::dir(&dir).join("x.json");
        std::fs::write(
            &path,
            r#"{"version": 1, "title": "hi", "messages": [], "somethingNew": 42}"#,
        )
        .unwrap();
        assert_eq!(read(&path).unwrap().title, "hi");
    }

    #[test]
    fn paths_are_relative_so_they_survive_the_project_moving() {
        let dir = root("relative");
        let path = fresh_path(&dir, "moved");
        write(&path, &Chat::default()).unwrap();
        let listed = list(&dir);
        assert_eq!(listed[0].path, ".proton/chats/moved.json");
    }
}

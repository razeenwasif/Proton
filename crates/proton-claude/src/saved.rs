//! Conversations with the engines whose sessions Proton does not read from
//! their own files, kept by Proton as the rows it drew.
//!
//! Claude Code's sessions are read back from the files Claude Code keeps.
//! The others are kept here, because each falls short of that a different
//! way: Codex has its threads but they are its own shape; Antigravity can
//! carry a conversation on by its id but cannot say what was in it; and
//! Gemini's CLI can do neither. So what was drawn is kept, with the engine's
//! own id for carrying on where the engine can.
//!
//! One file a conversation, and an index of them all, beside the artifacts.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::engine::Engine;
use crate::fold::Item;
use crate::history::SessionSummary;

/// One conversation, as kept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub id: String,
    pub engine: Engine,
    pub cwd: PathBuf,
    /// The engine's own id for it — a Codex thread, an Antigravity
    /// conversation — to carry on from.
    pub native: Option<String>,
    pub title: String,
    /// When it last changed, in milliseconds since the epoch.
    pub updated: u64,
    pub model: Option<String>,
    pub mode: String,
    pub effort: Option<String>,
    pub items: Vec<Item>,
}

/// One line of the index: enough to list it without reading it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    id: String,
    engine: Engine,
    cwd: PathBuf,
    title: String,
    updated: u64,
}

/// Where they are kept: `PROTON_CHATS` if it is set, otherwise beside the
/// artifacts.
pub fn dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("PROTON_CHATS") {
        return PathBuf::from(dir);
    }
    let root = crate::artifacts::root();
    root.parent().map(Path::to_path_buf).unwrap_or(root).join("chats")
}

/// A conversation's file. Ids are Proton's own UUIDs; anything else is not
/// a name to be trusted with a path.
fn file(dir: &Path, id: &str) -> Option<PathBuf> {
    let safe = !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    safe.then(|| dir.join(format!("{id}.json")))
}

fn index(dir: &Path) -> Vec<Entry> {
    std::fs::read_to_string(dir.join("index.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Write a file whole or not at all: to a neighbour first, then renamed over.
fn write(path: &Path, text: &str) -> Result<(), String> {
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, text).map_err(|e| format!("Could not keep the conversation: {e}"))?;
    std::fs::rename(&partial, path).map_err(|e| format!("Could not keep the conversation: {e}"))
}

fn same_folder(a: &Path, b: &Path) -> bool {
    a == b || a.canonicalize().ok().zip(b.canonicalize().ok()).is_some_and(|(a, b)| a == b)
}

pub fn save(record: &Record) -> Result<(), String> {
    save_in(&dir(), record)
}

pub fn save_in(dir: &Path, record: &Record) -> Result<(), String> {
    let path = file(dir, &record.id).ok_or("That is not a conversation's id.")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("Could not keep the conversation: {e}"))?;
    let text = serde_json::to_string(record).map_err(|e| e.to_string())?;
    write(&path, &text)?;
    let mut entries = index(dir);
    entries.retain(|e| e.id != record.id);
    entries.push(Entry {
        id: record.id.clone(),
        engine: record.engine,
        cwd: record.cwd.clone(),
        title: record.title.clone(),
        updated: record.updated,
    });
    entries.sort_by_key(|e| std::cmp::Reverse(e.updated));
    let text = serde_json::to_string_pretty(&entries).map_err(|e| e.to_string())?;
    write(&dir.join("index.json"), &text)
}

pub fn load(id: &str) -> Option<Record> {
    load_in(&dir(), id)
}

pub fn load_in(dir: &Path, id: &str) -> Option<Record> {
    let text = std::fs::read_to_string(file(dir, id)?).ok()?;
    serde_json::from_str(&text).ok()
}

/// The kept conversations in a folder, newest first.
pub fn list(cwd: &Path) -> Vec<SessionSummary> {
    list_in(&dir(), cwd)
}

pub fn list_in(dir: &Path, cwd: &Path) -> Vec<SessionSummary> {
    index(dir)
        .into_iter()
        .filter(|e| same_folder(&e.cwd, cwd))
        .map(|e| SessionSummary {
            id: e.id,
            title: e.title,
            updated: e.updated,
            cwd: String::new(),
            elsewhere: false,
            engine: e.engine,
        })
        .collect()
}

/// A kept conversation as the list shows it, whichever project it is in.
pub fn summary(id: &str) -> Option<SessionSummary> {
    index(&dir()).into_iter().find(|e| e.id == id).map(|e| SessionSummary {
        id: e.id,
        title: e.title,
        updated: e.updated,
        cwd: String::new(),
        elsewhere: false,
        engine: e.engine,
    })
}

/// Which engine a kept conversation is with, if it is one.
pub fn engine_of(id: &str) -> Option<Engine> {
    index(&dir()).into_iter().find(|e| e.id == id).map(|e| e.engine)
}

pub fn remove(id: &str) {
    let dir = dir();
    if let Some(path) = file(&dir, id) {
        let _ = std::fs::remove_file(path);
    }
    let mut entries = index(&dir);
    let before = entries.len();
    entries.retain(|e| e.id != id);
    if entries.len() != before {
        if let Ok(text) = serde_json::to_string_pretty(&entries) {
            let _ = write(&dir.join("index.json"), &text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_conversation_lists_in_its_folder_and_reads_back_whole() {
        let dir = std::env::temp_dir().join(format!("proton-saved-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let project = std::env::temp_dir();
        let record = |id: &str, updated: u64| Record {
            id: id.into(),
            engine: Engine::Codex,
            cwd: project.clone(),
            native: Some("thread-1".into()),
            title: format!("Chat {id}"),
            updated,
            model: Some("gpt-6.1-sol".into()),
            mode: "default".into(),
            effort: None,
            items: vec![Item::User { id: "you-1".into(), text: "hello".into(), images: 0 }],
        };
        save_in(&dir, &record("aaa", 1)).unwrap();
        save_in(&dir, &record("bbb", 2)).unwrap();
        save_in(&dir, &record("aaa", 3)).unwrap();
        let listed = list_in(&dir, &project);
        assert_eq!(listed.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["aaa", "bbb"], "newest first, once each");
        assert_eq!(listed[0].engine, Engine::Codex);
        assert!(list_in(&dir, Path::new("/nowhere")).is_empty());
        assert_eq!(load_in(&dir, "aaa").unwrap(), record("aaa", 3));
        assert!(load_in(&dir, "../etc/passwd").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

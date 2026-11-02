//! The sessions Claude Code keeps for a project.
//!
//! Claude Code writes every conversation to
//! `~/.claude/projects/<project>/<session>.jsonl`, where `<project>` is the
//! folder's path with everything but letters and digits turned into `-`.
//! That is the same file whether the session was started here, in a
//! terminal, or in the desktop app — so all of them are listed, and any of
//! them can be picked up again.
//!
//! Some of these files are large: a long session with screenshots in it runs
//! to tens of megabytes. Listing reads only the ends of each — the first
//! prompt from the start, a title from the end — and reading one back keeps
//! only the lines a transcript is drawn from, and only the latest of those.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::fold::title_of;

/// One session, as the list shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    /// When it was last written to, in milliseconds since the epoch.
    #[ts(type = "number")]
    pub updated: u64,
    /// The folder it runs in, when that is not the project's: a session
    /// from the desktop app, started somewhere else.
    #[serde(default)]
    pub cwd: String,
    /// Open in another Claude Code right now — the desktop app, a terminal —
    /// so what is sent here continues a copy of it rather than it.
    #[serde(default)]
    pub elsewhere: bool,
    /// Which agent it is with: Claude Code's own sessions, unless Proton
    /// kept it for another.
    #[serde(default)]
    pub engine: crate::engine::Engine,
}

/// Where Claude Code keeps its own files: `CLAUDE_CONFIG_DIR` if it is set,
/// otherwise `~/.claude`.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".claude"))
}

/// A folder's name among Claude Code's projects.
pub fn encode(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// The folders a project's sessions can be in: under the path it was opened
/// by, and under the path it really is, which differ wherever a symlink is
/// on the way — on macOS, `/tmp` and `/var` for a start.
pub fn project_dirs(root: &Path) -> Vec<PathBuf> {
    let Some(config) = config_dir() else { return Vec::new() };
    let mut dirs = vec![config.join("projects").join(encode(root))];
    if let Ok(real) = root.canonicalize() {
        let real = config.join("projects").join(encode(&real));
        if !dirs.contains(&real) {
            dirs.push(real);
        }
    }
    dirs
}

/// How many sessions the list holds at most, newest first.
const LISTED: usize = 200;

/// Titles already read, by session, with when its file was written then: a
/// file that has not changed is not read again, so listing a project is one
/// directory read however many sessions it has.
pub type Titles = HashMap<String, (u64, Option<String>)>;

/// The project's sessions, newest first.
pub fn sessions(root: &Path, titles: &mut Titles) -> Vec<SessionSummary> {
    let mut found: Vec<(SessionSummary, PathBuf)> = Vec::new();
    for dir in project_dirs(root) {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(String::from) else { continue };
            let updated = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or_default();
            if found.iter().any(|(s, _)| s.id == id) {
                continue;
            }
            found.push((SessionSummary { id, title: String::new(), updated, cwd: String::new(), elsewhere: false, engine: crate::engine::Engine::Claude }, path));
        }
    }
    found.sort_by(|a, b| b.0.updated.cmp(&a.0.updated));
    found.truncate(LISTED);
    found
        .into_iter()
        .filter_map(|(mut summary, path)| {
            let known = titles.get(&summary.id).filter(|(at, _)| *at == summary.updated).map(|(_, t)| t.clone());
            let found = known.unwrap_or_else(|| {
                let found = title(&path);
                titles.insert(summary.id.clone(), (summary.updated, found.clone()));
                found
            });
            // A session file with no prompt in it — one that was opened and
            // never spoken in — is not a conversation.
            summary.title = found?;
            Some(summary)
        })
        .collect()
}

/// One session as the list would show it, wherever it is — for a folder,
/// which holds conversations from every project. `None` if its file is gone
/// or has nothing said in it.
pub fn summary(cwd: &Path, id: &str, titles: &mut Titles) -> Option<SessionSummary> {
    let path = file(cwd, id)?;
    let updated = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default();
    let known = titles.get(id).filter(|(at, _)| *at == updated).map(|(_, t)| t.clone());
    let title = match known {
        Some(title) => title,
        None => {
            let found = title(&path);
            titles.insert(id.to_string(), (updated, found.clone()));
            found
        }
    }?;
    Some(SessionSummary { id: id.to_string(), title, updated, cwd: String::new(), elsewhere: false, engine: crate::engine::Engine::Claude })
}

/// The file a session is in.
pub fn file(root: &Path, id: &str) -> Option<PathBuf> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    project_dirs(root)
        .into_iter()
        .map(|dir| dir.join(format!("{id}.jsonl")))
        .find(|path| path.is_file())
}

/// How far into either end of a file a title is looked for.
const HEAD: u64 = 512 * 1024;
const TAIL: u64 = 256 * 1024;

/// A session's title: the last one it was given, or failing that its first
/// prompt. `None` if it has neither.
fn title(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    // The title it was given last, from the end.
    if length > 0 {
        let start = length.saturating_sub(TAIL);
        if file.seek(SeekFrom::Start(start)).is_ok() {
            let mut tail = String::new();
            let _ = file.by_ref().take(TAIL).read_to_string(&mut tail);
            let named = tail
                .lines()
                .rev()
                .filter(|line| line.contains("\"custom-title\""))
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .find_map(|value| value.get("customTitle").and_then(Value::as_str).map(String::from))
                .filter(|title| !title.trim().is_empty());
            if let Some(title) = named {
                return Some(title);
            }
        }
    }
    // Its first prompt, from the start.
    file.seek(SeekFrom::Start(0)).ok()?;
    let reader = BufReader::new(file.take(HEAD));
    for line in reader.lines().map_while(Result::ok) {
        if !line.contains("\"type\":\"user\"") && !line.contains("\"type\": \"user\"") {
            if line.contains("\"summary\"") {
                if let Some(summary) = serde_json::from_str::<Value>(&line)
                    .ok()
                    .filter(|v| v.get("type").and_then(Value::as_str) == Some("summary"))
                    .and_then(|v| v.get("summary").and_then(Value::as_str).map(String::from))
                {
                    return Some(summary);
                }
            }
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(text) = prompt(&value) {
            return Some(title_of(&text));
        }
    }
    None
}

/// What a user line said, if it is something a person said: not a tool
/// result, not one of the notes Claude Code writes in the user's name.
fn prompt(value: &Value) -> Option<String> {
    if value.get("isMeta").and_then(Value::as_bool) == Some(true)
        || value.get("isSidechain").and_then(Value::as_bool) == Some(true)
        || value.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    let content = value.get("message")?.get("content")?;
    let text = match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    let text = text.trim();
    if text.is_empty() || text.starts_with('<') || text.starts_with("[Request interrupted") {
        return None;
    }
    Some(text.to_string())
}

/// The kinds of line a transcript is drawn from.
const KEPT: [&str; 6] = ["user", "assistant", "system", "custom-title", "summary", "mode"];

/// The lines a transcript is drawn from.
fn kept(kind: &str) -> bool {
    KEPT.contains(&kind)
}

/// How many messages of a long session are read back: the latest, which is
/// what anyone picking it up again is looking for.
pub const READ_BACK: usize = 600;

/// A session's lines, as the fold reads them — the latest [`READ_BACK`]
/// messages of it, and whether anything earlier was left out.
pub fn load(path: &Path) -> std::io::Result<(Vec<Value>, bool)> {
    let reader = BufReader::new(File::open(path)?);
    // Cheap tests first: most of a long file is attachments and file
    // snapshots, and parsing them only to drop them is most of the work. A
    // line goes on to be parsed if a kept kind's `"type"` is anywhere in it,
    // and its own top-level `type` decides: Claude Code writes an answer's
    // message, which has a `"type":"message"` of its own, before the line's
    // `"type":"assistant"`, so the first `"type"` in a line is not always the
    // line's — taken as if it were, every answer was dropped.
    let mut lines = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let wanted = line.match_indices("\"type\":").any(|(at, _)| {
            let rest = line[at + 7..].trim_start().trim_start_matches('"');
            KEPT.iter().any(|kind| rest.strip_prefix(kind).is_some_and(|after| after.starts_with('"')))
        });
        if !wanted {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
        if !value.get("type").and_then(Value::as_str).is_some_and(kept) {
            continue;
        }
        if value.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        lines.push(value);
    }
    let messages = lines
        .iter()
        .filter(|v| matches!(v.get("type").and_then(Value::as_str), Some("user" | "assistant")))
        .count();
    if messages <= READ_BACK {
        return Ok((lines, false));
    }
    // Keep from the message that leaves READ_BACK after it; the titles and
    // modes from before it still count, so they are kept too.
    let mut seen = 0;
    let mut from = lines.len();
    for (i, value) in lines.iter().enumerate().rev() {
        if matches!(value.get("type").and_then(Value::as_str), Some("user" | "assistant")) {
            seen += 1;
            if seen == READ_BACK {
                from = i;
                break;
            }
        }
    }
    // A tool's result must not arrive without the call it answers: start at
    // a prompt someone typed.
    while from < lines.len() && prompt(&lines[from]).is_none() {
        from += 1;
    }
    let mut kept_lines: Vec<Value> = lines[..from]
        .iter()
        .filter(|v| matches!(v.get("type").and_then(Value::as_str), Some("custom-title" | "summary" | "mode")))
        .cloned()
        .collect();
    kept_lines.extend(lines.drain(from..));
    Ok((kept_lines, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_named_the_way_claude_code_names_it() {
        assert_eq!(
            encode(Path::new("/Users/me/Github/Proton/.claude/worktrees/x-1")),
            "-Users-me-Github-Proton--claude-worktrees-x-1"
        );
        assert_eq!(encode(Path::new("/private/tmp/claude-501/-Users-me")), "-private-tmp-claude-501--Users-me");
    }

    #[test]
    fn only_what_someone_typed_is_a_prompt() {
        let typed = serde_json::json!({ "type": "user", "message": { "content": "Fix the build" } });
        let meta = serde_json::json!({ "type": "user", "isMeta": true, "message": { "content": "Caveat" } });
        let command = serde_json::json!({ "type": "user", "message": { "content": "<command-name>/clear</command-name>" } });
        let result = serde_json::json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "content": "ok" }] } });
        assert_eq!(prompt(&typed).as_deref(), Some("Fix the build"));
        assert!(prompt(&meta).is_none() && prompt(&command).is_none() && prompt(&result).is_none());
    }

    #[test]
    fn ids_that_are_not_ids_name_no_file() {
        assert!(file(Path::new("/tmp"), "../../etc/passwd").is_none());
        assert!(file(Path::new("/tmp"), "").is_none());
    }
}

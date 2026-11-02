//! Artifacts published before Proton was there to keep them.
//!
//! Two places on this machine remember what has been published. The Claude
//! desktop app keeps, for each of its Code sessions, the artifacts it
//! published: the address, the title and the page's file. And every Claude
//! Code session's own file has the Artifact tool's answers in it, with the
//! artifact's id and which version each publish was. Between them they name
//! every artifact published from this machine, and importing copies the page
//! each one names — as it is now — into the store.
//!
//! What they cannot give back is an artifact made in claude.ai's own chat,
//! which was never on this machine, or the content of a version whose file
//! has since been overwritten: a page drafted in a scratch folder that has
//! been cleared is imported as its address alone, to be opened on claude.ai.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// An artifact found published on this machine.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub url: String,
    /// claude.ai's id for it, when a session's file recorded it.
    pub id: Option<String>,
    pub title: String,
    /// The page, where it was published from.
    pub source: Option<PathBuf>,
    /// What it brought with it: published path → source, as the tool took it.
    pub files: Option<Value>,
    pub icon: String,
    pub description: String,
    pub project: String,
    pub session: String,
    /// Which version the last publish was, when known.
    pub seq: Option<u32>,
    /// When it was last published, in milliseconds since the epoch.
    pub at: u64,
}

/// The same artifact's address, however it was written.
pub fn same_url(a: &str, b: &str) -> bool {
    let norm = |u: &str| u.trim().trim_end_matches('/').split(['?', '#']).next().unwrap_or_default().to_ascii_lowercase();
    !a.is_empty() && norm(a) == norm(b)
}

/// The last part of an artifact's address, which is an id in its own right.
pub fn id_from_url(url: &str) -> String {
    url.trim()
        .trim_end_matches('/')
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Every artifact published from this machine, newest first.
pub fn discover() -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    for one in from_sessions().into_iter().chain(from_desktop()) {
        merge(&mut found, one);
    }
    found.sort_by(|a, b| b.at.cmp(&a.at));
    found
}

/// Fold one finding into the list: one entry an address, the newest publish
/// deciding the page, and whatever either source knew filling the rest.
fn merge(found: &mut Vec<Found>, one: Found) {
    let Some(existing) = found.iter_mut().find(|e| same_url(&e.url, &one.url)) else {
        found.push(one);
        return;
    };
    let newer = one.at > existing.at;
    if newer {
        existing.at = one.at;
        if !one.title.is_empty() {
            existing.title = one.title.clone();
        }
        if one.source.is_some() {
            existing.source = one.source.clone();
        }
        if one.files.is_some() {
            existing.files = one.files.clone();
        }
        if !one.project.is_empty() {
            existing.project = one.project.clone();
            existing.session = one.session.clone();
        }
    }
    existing.id = existing.id.take().or(one.id);
    existing.seq = match (existing.seq, one.seq) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };
    if existing.icon.is_empty() {
        existing.icon = one.icon;
    }
    if existing.description.is_empty() {
        existing.description = one.description;
    }
    if existing.source.is_none() {
        existing.source = one.source;
    }
    if existing.files.is_none() {
        existing.files = one.files;
    }
}

/// Milliseconds since the epoch, from a number or an RFC 3339 time.
fn when(value: Option<&Value>) -> u64 {
    match value {
        Some(Value::Number(n)) => {
            let n = n.as_f64().unwrap_or(0.0);
            // Seconds or milliseconds, by size.
            if n < 1e11 { (n * 1000.0) as u64 } else { n as u64 }
        }
        Some(Value::String(s)) => iso_ms(s).unwrap_or(0),
        _ => 0,
    }
}

/// `2026-10-04T10:03:51.327Z` in milliseconds since the epoch.
pub fn iso_ms(text: &str) -> Option<u64> {
    let b = text.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b' ') {
        return None;
    }
    let num = |from: usize, to: usize| text.get(from..to)?.parse::<i64>().ok();
    let (y, m, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, min, s) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let mut ms = 0;
    if b.get(19) == Some(&b'.') {
        let digits: String = text[20..].chars().take_while(char::is_ascii_digit).take(3).collect();
        ms = format!("{digits:0<3}").parse::<i64>().ok()?;
    }
    // Days from the civil date, after Howard Hinnant.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let total = ((days * 24 + h) * 60 + min) * 60 + s;
    (total >= 0).then(|| (total * 1000 + ms) as u64)
}

/* ── the desktop app's records ─────────────────────────────────────── */

/// Where the Claude desktop app keeps its Code sessions' records.
/// Where Claude Code keeps its own files: `CLAUDE_CONFIG_DIR` if it is set,
/// otherwise `~/.claude`.
fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".claude"))
}

fn desktop_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("PROTON_CLAUDE_DESKTOP") {
        return Some(PathBuf::from(dir));
    }
    dirs::config_dir().map(|dir| dir.join("Claude").join("claude-code-sessions"))
}

fn from_desktop() -> Vec<Found> {
    let Some(root) = desktop_dir() else { return Vec::new() };
    let mut found = Vec::new();
    // `<organisation>/<account>/local_<session>.json`, two folders down.
    for org in read_dirs(&root) {
        for account in read_dirs(&org) {
            let Ok(entries) = std::fs::read_dir(&account) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                let named = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                if !named.starts_with("local_") || !named.ends_with(".json") {
                    continue;
                }
                let Some(record) = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else {
                    continue;
                };
                found.extend(desktop_record(&record));
            }
        }
    }
    found
}

fn desktop_record(record: &Value) -> Vec<Found> {
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let project = text(record, "cwd");
    let session = text(record, "cliSessionId");
    record
        .get("publishedArtifacts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|artifact| {
            let url = text(artifact, "url");
            (!url.is_empty()).then(|| Found {
                url,
                id: None,
                title: text(artifact, "title"),
                source: artifact.get("sourcePath").and_then(Value::as_str).map(PathBuf::from),
                files: None,
                icon: String::new(),
                description: String::new(),
                project: project.clone(),
                session: session.clone(),
                seq: None,
                at: when(artifact.get("updatedAt")),
            })
        })
        .collect()
}

fn read_dirs(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default()
}

/* ── the sessions' own files ───────────────────────────────────────── */

fn jsonl_files(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth < 4 {
                jsonl_files(&path, out, depth + 1);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}

fn from_sessions() -> Vec<Found> {
    let Some(config) = config_dir() else { return Vec::new() };
    let mut files = Vec::new();
    jsonl_files(&config.join("projects"), &mut files, 0);
    let mut found = Vec::new();
    for file in files {
        found.extend(session_file(&file));
    }
    found
}

/// The artifacts one session's file says were published. Most of a long
/// file is nothing to do with artifacts, so a line is only parsed when the
/// bytes in it could be one of the two this looks for.
pub fn session_file(path: &Path) -> Vec<Found> {
    let Ok(file) = File::open(path) else { return Vec::new() };
    let mut reader = BufReader::with_capacity(1 << 20, file);
    let call = memchr::memmem::Finder::new(b"\"name\":\"Artifact\"");
    let answer = memchr::memmem::Finder::new(b"\"artifact_id\"");
    let mut inputs: Map<String, Value> = Map::new();
    let mut found = Vec::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let is_call = call.find(&line).is_some();
        let is_answer = answer.find(&line).is_some();
        if !is_call && !is_answer {
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(&line) else { continue };
        let blocks = value
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if is_call {
            for block in &blocks {
                if block.get("type").and_then(Value::as_str) == Some("tool_use")
                    && block.get("name").and_then(Value::as_str) == Some("Artifact")
                {
                    if let (Some(id), Some(input)) = (block.get("id").and_then(Value::as_str), block.get("input")) {
                        inputs.insert(id.to_string(), input.clone());
                    }
                }
            }
        }
        if !is_answer {
            continue;
        }
        let Some(result) = value.get("toolUseResult").or_else(|| value.get("tool_use_result")) else { continue };
        let (Some(id), Some(url)) = (
            result.get("artifact_id").and_then(Value::as_str),
            result.get("url").and_then(Value::as_str),
        ) else {
            continue;
        };
        let input = blocks
            .iter()
            .find_map(|b| b.get("tool_use_id").and_then(Value::as_str))
            .and_then(|tool| inputs.get(tool))
            .cloned()
            .unwrap_or(Value::Null);
        let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        let source = result
            .get("path")
            .and_then(Value::as_str)
            .or_else(|| input.get("file_path").and_then(Value::as_str))
            .map(PathBuf::from);
        found.push(Found {
            url: url.to_string(),
            id: Some(id.to_string()),
            title: match text(result, "title") {
                t if t.is_empty() => text(&input, "title"),
                t => t,
            },
            source,
            files: input.get("files").cloned(),
            icon: match text(result, "icon") {
                i if i.is_empty() => text(&input, "icon"),
                i => i,
            },
            description: text(&input, "description"),
            project: text(&value, "cwd"),
            session: text(&value, "sessionId"),
            seq: result.get("seq").and_then(Value::as_u64).map(|s| s as u32),
            at: when(value.get("timestamp")),
        });
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn times_read_back_as_the_epoch_counts_them() {
        assert_eq!(iso_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_ms("2026-10-04T10:03:51.327Z"), Some(1_791_108_231_327));
        assert_eq!(iso_ms("2000-02-29T12:00:00.5Z"), Some(951_825_600_500));
        assert_eq!(iso_ms("not a time"), None);
        assert_eq!(when(Some(&json!(1_791_022_128_208u64))), 1_791_022_128_208);
        assert_eq!(when(Some(&json!(1_791_022_128))), 1_791_022_128_000);
    }

    #[test]
    fn an_address_is_the_same_however_it_is_written() {
        assert!(same_url("https://claude.ai/artifact/Abc/", "https://claude.ai/artifact/abc?x=1"));
        assert!(!same_url("", ""));
        assert_eq!(id_from_url("https://claude.ai/artifact/Vcd3gdeNur9STDdA42vqpY/"), "Vcd3gdeNur9STDdA42vqpY");
    }

    #[test]
    fn a_desktop_record_names_its_artifacts_and_where_they_came_from() {
        let record = json!({
            "cwd": "/Users/me/Github", "cliSessionId": "s-1",
            "publishedArtifacts": [
                { "url": "https://claude.ai/artifact/AAA", "title": "Notes", "sourcePath": "/tmp/notes.html", "updatedAt": 1_790_000_000_000u64 },
                { "title": "no address" }
            ]
        });
        let found = desktop_record(&record);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].title.as_str(), found[0].project.as_str(), found[0].session.as_str()), ("Notes", "/Users/me/Github", "s-1"));
        assert_eq!(found[0].source.as_deref(), Some(Path::new("/tmp/notes.html")));
    }

    #[test]
    fn a_session_file_gives_the_id_the_version_and_what_the_call_was() {
        let dir = std::env::temp_dir().join(format!("proton-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("s.jsonl");
        let call = json!({ "type": "assistant", "message": { "content": [
            { "type": "tool_use", "id": "toolu_1", "name": "Artifact",
              "input": { "file_path": "/tmp/p.html", "description": "A page", "icon": "chart", "files": { "a.js": "a.js" } } } ] } });
        let answer = json!({ "type": "user", "cwd": "/proj", "sessionId": "sess", "timestamp": "2026-10-04T10:00:00Z",
            "message": { "content": [{ "type": "tool_result", "tool_use_id": "toolu_1", "content": "Published" }] },
            "toolUseResult": { "url": "https://claude.ai/artifact/BBB", "path": "/tmp/p.html", "artifact_id": "0f8fad5b-d9cb-469f-a165-70867728950e", "title": "Page", "seq": 3 } });
        let noise = json!({ "type": "user", "message": { "content": "hello" } });
        std::fs::write(&file, format!("{noise}\n{call}\n{noise}\n{answer}\n")).unwrap();
        let found = session_file(&file);
        assert_eq!(found.len(), 1);
        let one = &found[0];
        assert_eq!(one.id.as_deref(), Some("0f8fad5b-d9cb-469f-a165-70867728950e"));
        assert_eq!((one.seq, one.title.as_str(), one.icon.as_str(), one.description.as_str()), (Some(3), "Page", "chart", "A page"));
        assert_eq!((one.project.as_str(), one.session.as_str()), ("/proj", "sess"));
        assert!(one.files.is_some());
        assert!(one.at > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_sources_naming_one_artifact_make_one_entry() {
        let mut found = Vec::new();
        let base = Found {
            url: "https://claude.ai/artifact/CCC".into(), id: None, title: "Old".into(), source: Some("/tmp/a.html".into()),
            files: None, icon: String::new(), description: String::new(), project: "/p".into(), session: "s".into(), seq: None, at: 10,
        };
        merge(&mut found, base.clone());
        merge(&mut found, Found { id: Some("uuid".into()), title: "New".into(), seq: Some(4), icon: "map".into(), at: 20, ..base.clone() });
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].title.as_str(), found[0].id.as_deref(), found[0].seq, found[0].icon.as_str()), ("New", Some("uuid"), Some(4), "map"));
    }
}

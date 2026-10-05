//! What the user has chosen, in a file.
//!
//! `.proton/settings.json`, beside the `.proton/debug.json` that already
//! decides what the debugger launches. Project-level rather than buried in an
//! OS config directory, for the reason every other document here is a file in
//! the repo: you can read it, diff it, and commit it if the team wants the
//! same editor.
//!
//! ## Unknown keys are kept
//!
//! Settings are parsed into a `Value` and edited in place, not into a struct
//! and printed back. A settings file written by a newer Proton, or by hand
//! with a key this build has never heard of, must not lose that key the first
//! time somebody changes a theme. Same discipline as the notebook writer and
//! the theme writer, for the same reason.
//!
//! ## Nothing secret lives here
//!
//! This file is meant to be committed, so it holds no credentials — the model
//! to use is a setting, the key to reach it with is not. Credentials come from
//! the environment or the provider's own CLI login, which is what
//! `proton_agent::auth` already does.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// The settings file, and everything that was in it.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Empty for a `Settings` that was defaulted rather than loaded. Such a
    /// one reads as all-defaults and refuses to write, because the alternative
    /// is a `.proton/settings.json` appearing in whatever directory the
    /// process happened to start in.
    path: PathBuf,
    doc: Value,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            path: PathBuf::new(),
            doc: Value::Object(Map::new()),
        }
    }
}

impl Settings {
    /// Read `.proton/settings.json`, or start an empty one.
    ///
    /// A missing file is the normal case and not a problem. A *malformed* one
    /// is left alone: overwriting somebody's hand-edited JSON because a brace
    /// is missing would lose the rest of it, so the defaults are used and the
    /// file is not touched until something is deliberately set.
    pub fn load(root: &Path) -> Settings {
        Settings::at(Settings::path_in(root))
    }

    /// The person's own settings, for what belongs to them rather than to any
    /// one project: `settings.json` beside the session, in Proton's config
    /// folder — `~/.config/proton/settings.json`. Read only by a session the
    /// shell restores, so no test reads the developer's.
    pub fn person() -> Settings {
        match crate::folder::config() {
            Some(dir) => Settings::at(dir.join("settings.json")),
            None => Settings::default(),
        }
    }

    /// The settings in `path`, as `load` reads a project's.
    pub fn at(path: PathBuf) -> Settings {
        let doc = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| Value::Object(Map::new()));
        Settings { path, doc }
    }

    /// Where a project's settings live.
    ///
    /// `PROTON_SETTINGS` overrides it, which exists for one reason worth
    /// stating: a test that roots a workspace at a real repository otherwise
    /// inherits whatever the developer happens to have chosen there. That is
    /// how `tests/agent.rs` — a suite whose whole point is a *fake* API on a
    /// loopback port — came to send its requests to Google, because the
    /// repository's own settings said `"provider": "gemini"` and the fake
    /// server is only ever pointed at by `ANTHROPIC_BASE_URL`.
    pub fn path_in(root: &Path) -> PathBuf {
        match std::env::var_os("PROTON_SETTINGS") {
            Some(named) => PathBuf::from(named),
            None => root.join(".proton").join("settings.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The library roots this file names, resolved against `project`.
    ///
    /// Here rather than in the workspace because two programs read it now: the
    /// editor, and the MCP server that offers the same library to whatever
    /// else the user has a subscription to. Two copies of "what counts as a
    /// root" would drift, and the thing they would drift about is which
    /// directories a model may read.
    ///
    /// A leading `~/` is expanded and a relative path is taken against the
    /// project, because those are the two ways somebody writes a path by hand
    /// and neither should silently mean "the directory the process started
    /// in".
    pub fn library_roots(&self, project: &Path) -> Vec<PathBuf> {
        self.get("library.roots")
            .and_then(|value| value.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|value| value.as_str())
                    .filter(|text| !text.trim().is_empty())
                    .map(|text| expand(text, project))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Where investigations are kept, for a person who keeps them in a folder
    /// of their own rather than in each project's `research/`:
    /// `research.folder`. A leading `~/` is the home folder, and so is what a
    /// relative path is taken from, there being no project to take it from.
    pub fn research_folder(&self) -> Option<PathBuf> {
        let text = self.get("research.folder")?.as_str()?.trim();
        let home = crate::folder::home()?;
        match text {
            "" => None,
            "~" => Some(home),
            text => Some(expand(text, &home)),
        }
    }

    /// Everything set, as one object — what the front end draws the panels
    /// from. Defaults are *not* merged in: the difference between "chosen" and
    /// "left alone" is worth keeping, or a settings file fills up with every
    /// default the moment anything is changed.
    pub fn all(&self) -> &Value {
        &self.doc
    }

    /// One value, by dotted path — `agent.provider`.
    pub fn get(&self, key: &str) -> Option<&Value> {
        let mut node = &self.doc;
        for part in key.split('.') {
            node = node.get(part)?;
        }
        Some(node)
    }

    pub fn string(&self, key: &str, fallback: &str) -> String {
        self.get(key)
            .and_then(Value::as_str)
            .unwrap_or(fallback)
            .to_string()
    }

    pub fn bool(&self, key: &str, fallback: bool) -> bool {
        self.get(key).and_then(Value::as_bool).unwrap_or(fallback)
    }

    pub fn number(&self, key: &str, fallback: u64) -> u64 {
        self.get(key).and_then(Value::as_u64).unwrap_or(fallback)
    }

    /// Set one value and write the file.
    ///
    /// Answers whether anything changed. Writing the value that is already
    /// there is not a change: the settings panel emits on every click, and a
    /// file rewritten with its own bytes still shows up as a modification to
    /// whatever is watching the repository.
    pub fn set(&mut self, key: &str, value: Value) -> Result<bool, String> {
        if self.get(key) == Some(&value) {
            return Ok(false);
        }
        // A `null` removes the key rather than storing a null, so that
        // "back to the default" leaves a file that looks like it never had
        // an opinion.
        let removing = value.is_null();

        let mut node = &mut self.doc;
        let parts: Vec<&str> = key.split('.').collect();
        let Some((last, path)) = parts.split_last() else {
            return Ok(false);
        };
        for part in path {
            let map = node
                .as_object_mut()
                .ok_or_else(|| format!("{key}: {part} is not an object"))?;
            node = map
                .entry(part.to_string())
                .or_insert_with(|| Value::Object(Map::new()));
        }
        let map = node
            .as_object_mut()
            .ok_or_else(|| format!("{key} is not an object"))?;
        match removing {
            true => {
                map.remove(*last);
            }
            false => {
                map.insert(last.to_string(), value);
            }
        }
        self.write()?;
        Ok(true)
    }

    /// The file, as it will be written: two-space JSON with a trailing
    /// newline, and keys in sorted order because `serde_json`'s map is a
    /// `BTreeMap` — which is what makes the diff of a settings change one
    /// line rather than a reshuffle.
    pub fn to_text(&self) -> String {
        let mut text = serde_json::to_string_pretty(&self.doc).unwrap_or_default();
        text.push('\n');
        text
    }

    fn write(&self) -> Result<(), String> {
        if self.path.as_os_str().is_empty() {
            return Err("no project to save settings in".into());
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
        }
        std::fs::write(&self.path, self.to_text())
            .map_err(|err| format!("{}: {err}", self.path.display()))
    }
}

/// A path as a person writes one: `~/OnyxVault`, `../notes`, or absolute.
fn expand(text: &str, project: &Path) -> PathBuf {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\")) {
        // `USERPROFILE` first on Windows, where `HOME` is set by MSYS2 to
        // something that is not where anybody's documents are.
        let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"));
        if let Some(home) = home {
            return PathBuf::from(home).join(rest);
        }
    }
    let path = Path::new(text);
    match path.is_absolute() {
        true => path.to_path_buf(),
        false => project.join(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_research_folder_is_taken_from_home() {
        let Some(home) = crate::folder::home() else { return };
        let with = |value: Value| Settings { path: PathBuf::new(), doc: serde_json::json!({ "research": { "folder": value } }) };
        assert_eq!(with("~/Github/Research".into()).research_folder(), Some(home.join("Github/Research")));
        assert_eq!(with("Github/Research".into()).research_folder(), Some(home.join("Github/Research")), "a relative path is from home");
        assert_eq!(with("~".into()).research_folder(), Some(home.clone()));
        let elsewhere = if cfg!(windows) { r"D:\Research" } else { "/srv/research" };
        assert_eq!(with(elsewhere.into()).research_folder(), Some(PathBuf::from(elsewhere)));
        assert_eq!(with("  ".into()).research_folder(), None);
        assert_eq!(with(3.into()).research_folder(), None);
        assert_eq!(Settings::default().research_folder(), None, "unset is each project's research/");
    }

    fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "proton-settings-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn a_missing_file_is_not_a_problem() {
        let settings = Settings::load(&root("missing"));
        assert_eq!(settings.string("theme", "tokyo-night"), "tokyo-night");
    }

    #[test]
    fn setting_a_value_writes_it() {
        let dir = root("write");
        let mut settings = Settings::load(&dir);
        assert!(settings.set("theme", "nord".into()).unwrap());

        let text = std::fs::read_to_string(dir.join(".proton").join("settings.json")).unwrap();
        assert!(text.contains("\"nord\""), "{text}");
        assert!(text.ends_with('\n'), "no trailing newline");
        // And it is there on the next read.
        assert_eq!(Settings::load(&dir).string("theme", "x"), "nord");
    }

    #[test]
    fn a_dotted_key_nests() {
        let dir = root("nested");
        let mut settings = Settings::load(&dir);
        settings.set("agent.provider", "gemini".into()).unwrap();
        settings.set("agent.model", "x".into()).unwrap();

        assert_eq!(settings.string("agent.provider", ""), "gemini");
        let text = std::fs::read_to_string(settings.path()).unwrap();
        assert!(text.contains("\"agent\""), "{text}");
        // One object, not two keys with dots in their names.
        assert_eq!(text.matches("\"agent\"").count(), 1, "{text}");
    }

    #[test]
    fn writing_the_value_it_already_has_is_not_a_change() {
        let dir = root("noop");
        let mut settings = Settings::load(&dir);
        settings.set("theme", "nord".into()).unwrap();
        assert!(!settings.set("theme", "nord".into()).unwrap());
    }

    #[test]
    fn null_removes_rather_than_storing_a_null() {
        // "Back to the default" should leave a file with no opinion in it,
        // not one asserting that the opinion is nothing.
        let dir = root("clear");
        let mut settings = Settings::load(&dir);
        settings.set("theme", "nord".into()).unwrap();
        settings.set("theme", Value::Null).unwrap();
        assert!(settings.get("theme").is_none());
        assert!(!std::fs::read_to_string(settings.path())
            .unwrap()
            .contains("theme"));
    }

    #[test]
    fn a_key_this_build_does_not_know_survives_a_write() {
        // The rule that makes the file safe to hand-edit and safe to share
        // with a newer Proton.
        let dir = root("unknown");
        std::fs::create_dir_all(dir.join(".proton")).unwrap();
        std::fs::write(
            dir.join(".proton").join("settings.json"),
            r#"{"fromTheFuture": {"deep": [1, 2, 3]}, "theme": "nord"}"#,
        )
        .unwrap();

        let mut settings = Settings::load(&dir);
        settings.set("theme", "catppuccin".into()).unwrap();

        let after = Settings::load(&dir);
        assert_eq!(after.string("theme", ""), "catppuccin");
        assert_eq!(
            after
                .get("fromTheFuture.deep")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(3),
            "a key nobody understood was dropped"
        );
    }

    #[test]
    fn a_malformed_file_is_left_alone_rather_than_overwritten() {
        // Somebody is mid-edit with a brace missing. Losing the rest of their
        // file because of it would be the worst thing this could do.
        let dir = root("broken");
        std::fs::create_dir_all(dir.join(".proton")).unwrap();
        let path = dir.join(".proton").join("settings.json");
        std::fs::write(&path, "{ \"theme\": \"nord\" ").unwrap();

        let settings = Settings::load(&dir);
        assert_eq!(settings.string("theme", "default"), "default");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{ \"theme\": \"nord\" ",
            "reading a broken file changed it"
        );
    }

    #[test]
    fn the_file_is_sorted_so_a_change_is_one_line() {
        let dir = root("sorted");
        let mut settings = Settings::load(&dir);
        settings.set("zebra", 1.into()).unwrap();
        settings.set("alpha", 2.into()).unwrap();
        let text = settings.to_text();
        assert!(
            text.find("alpha") < text.find("zebra"),
            "keys are not sorted: {text}"
        );
    }
}

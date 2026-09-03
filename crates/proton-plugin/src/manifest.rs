//! What a plugin says it is, before any of it runs.
//!
//! A plugin is a folder with a `proton.toml` in it. The manifest is read
//! first and on its own: it says what the plugin contributes, and the editor
//! decides whether to load the wasm at all. That ordering is the point —
//! a plugin that only ships a theme should never have its code run, and
//! discovering what it contributes by *calling* it would mean running
//! everything installed at startup.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What a plugin declares about itself.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
    /// The component, relative to the plugin's folder. Absent for a plugin
    /// that is only data — a theme pack has nothing to run, and making it
    /// ship an empty component to say so would be a tax on the simplest
    /// useful plugin there is.
    #[serde(default)]
    pub wasm: Option<String>,
    /// Themes shipped as files rather than built by code, for the same
    /// reason: a palette is a list of colours and does not need a program.
    #[serde(default)]
    pub themes: Vec<String>,
    /// Tree-sitter grammars, as wasm. Loaded by tree-sitter's own engine
    /// rather than this one — a grammar is not a component and does not want
    /// the component ABI — but declared here so one manifest describes the
    /// whole plugin.
    #[serde(default)]
    pub grammars: Vec<Grammar>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Grammar {
    /// The language it parses, matching the ids in `proton-syntax`.
    pub language: String,
    pub extensions: Vec<String>,
    /// The compiled grammar, relative to the plugin's folder.
    pub wasm: String,
    /// Its `highlights.scm`, relative to the plugin's folder.
    pub highlights: String,
}

/// A plugin as the editor holds it: its manifest, and where it lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub manifest: Manifest,
    pub root: PathBuf,
}

impl Installed {
    /// Resolve one of the manifest's paths against the plugin's folder.
    ///
    /// Refuses anything that climbs out of it. A manifest is a file a plugin
    /// author wrote and a path in it is untrusted input like any other:
    /// `wasm = "../../../etc/passwd"` should not read anything, and the fact
    /// that nobody has tried it yet is not a reason to allow it.
    pub fn file(&self, relative: &str) -> Result<PathBuf, String> {
        let path = self.root.join(relative);
        // Compared after canonicalising, because `a/../../b` only escapes
        // once the `..` are resolved — and a textual check for ".." would
        // both miss that and reject a folder legitimately called `..foo`.
        let (Ok(inside), Ok(root)) = (path.canonicalize(), self.root.canonicalize()) else {
            return Err(format!("{relative}: no such file"));
        };
        if !inside.starts_with(&root) {
            return Err(format!("{relative}: outside the plugin's folder"));
        }
        Ok(inside)
    }

    pub fn name(&self) -> &str {
        &self.manifest.name
    }
}

/// What the front end draws in the plugin list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct PluginView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    /// What it brought: `2 themes`, `a grammar`, `3 commands`. A sentence
    /// rather than counts, because a list of numbers is not what anybody
    /// scanning an installed-plugins list is reading it for.
    pub provides: String,
    /// Why it is not loaded, when it is not. Empty when all is well — a
    /// plugin that failed silently is one you uninstall in confusion.
    pub problem: String,
}

/// Read one plugin's manifest.
pub fn read(root: &Path) -> Result<Installed, String> {
    let path = root.join("proton.toml");
    let text =
        std::fs::read_to_string(&path).map_err(|err| format!("{}: {err}", path.display()))?;
    let manifest: Manifest =
        toml::from_str(&text).map_err(|err| format!("{}: {err}", path.display()))?;

    if manifest.id.trim().is_empty() {
        return Err(format!("{}: a plugin needs an id", path.display()));
    }
    Ok(Installed {
        manifest,
        root: root.to_path_buf(),
    })
}

/// Every plugin in a folder, in a stable order.
///
/// A folder that is not a plugin is skipped rather than reported: the plugins
/// directory is somewhere people put things, and a stray `.DS_Store` is not
/// an error anybody needs to hear about. A folder that *is* a plugin and has
/// a broken manifest is a different matter, and comes back as one.
pub fn discover(dir: &Path) -> (Vec<Installed>, Vec<String>) {
    let mut found = Vec::new();
    let mut problems = Vec::new();

    let Ok(entries) = std::fs::read_dir(dir) else {
        return (found, problems);
    };
    let mut roots: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("proton.toml").is_file())
        .collect();
    // Sorted, because a list that reshuffles between runs is one you cannot
    // read twice — and load order decides which of two plugins claiming the
    // same theme id wins.
    roots.sort();

    for root in roots {
        match read(&root) {
            Ok(plugin) => found.push(plugin),
            Err(why) => problems.push(why),
        }
    }
    (found, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "proton-manifest-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn plugin(dir: &Path, name: &str, toml: &str) -> PathBuf {
        let root = dir.join(name);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("proton.toml"), toml).unwrap();
        root
    }

    #[test]
    fn a_manifest_needs_only_an_id_and_a_name() {
        // The simplest useful plugin is a theme pack: no code, no grammar,
        // nothing to run. Making it declare an empty component to say so
        // would be a tax on the easiest thing anybody writes.
        let dir = scratch("minimal");
        let root = plugin(&dir, "p", "id = \"nord\"\nname = \"Nord\"\n");
        let found = read(&root).expect("a manifest");
        assert_eq!(found.manifest.id, "nord");
        assert!(found.manifest.wasm.is_none());
        assert!(found.manifest.themes.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_manifest_with_no_id_is_refused() {
        let dir = scratch("noid");
        let root = plugin(&dir, "p", "id = \"\"\nname = \"Nameless\"\n");
        assert!(read(&root).unwrap_err().contains("needs an id"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn discovery_skips_what_is_not_a_plugin_and_reports_what_is_broken() {
        let dir = scratch("discover");
        plugin(&dir, "good", "id = \"a\"\nname = \"A\"\n");
        plugin(&dir, "broken", "id = \"b\"\nname =\n");
        // Not a plugin at all — no manifest. Somewhere people put things.
        std::fs::create_dir_all(dir.join("notes")).unwrap();
        std::fs::write(dir.join("stray.txt"), "hello").unwrap();

        let (found, problems) = discover(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].manifest.id, "a");
        assert_eq!(problems.len(), 1, "the broken one is worth saying");
        assert!(problems[0].contains("proton.toml"), "{problems:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_plugins_folder_is_not_an_error() {
        // Most people have none. An editor that complains at startup about
        // an empty extension point is one you learn to ignore.
        let (found, problems) = discover(Path::new("/no/such/place"));
        assert!(found.is_empty() && problems.is_empty());
    }

    #[test]
    fn a_path_cannot_climb_out_of_the_plugins_folder() {
        // A manifest is a file somebody else wrote, and a path in it is
        // untrusted input like any other.
        let dir = scratch("escape");
        let root = plugin(&dir, "p", "id = \"a\"\nname = \"A\"\n");
        std::fs::write(root.join("real.wasm"), b"\0asm").unwrap();
        std::fs::write(dir.join("outside.wasm"), b"\0asm").unwrap();
        let found = read(&root).unwrap();

        assert!(found.file("real.wasm").is_ok());
        let why = found.file("../outside.wasm").unwrap_err();
        assert!(why.contains("outside the plugin"), "{why}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_grammar_declares_what_it_parses_and_where_its_files_are() {
        let dir = scratch("grammar");
        let root = plugin(
            &dir,
            "p",
            r#"
id = "zig"
name = "Zig"
version = "0.1.0"

[[grammars]]
language = "zig"
extensions = ["zig", "zon"]
wasm = "zig.wasm"
highlights = "highlights.scm"
"#,
        );
        let found = read(&root).expect("a manifest");
        let grammar = &found.manifest.grammars[0];
        assert_eq!(grammar.language, "zig");
        assert_eq!(grammar.extensions, vec!["zig", "zon"]);
        let _ = std::fs::remove_dir_all(dir);
    }
}

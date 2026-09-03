//! Plugins, from the editor's side of the sandbox.
//!
//! `proton-plugin` owns the engine, the deadline and the component ABI. What
//! lives here is the editor's half: where plugins are found, what the ones
//! that loaded contribute, and what to do with what a command returns.
//!
//! Plugins are **per user, not per project** — they sit beside `session.json`
//! rather than in a repository, because a plugin is something you installed
//! and not something a checkout brings with it. A repository that could drop
//! code into the editor by existing would be a repository you could not
//! safely open.

use std::path::PathBuf;

use proton_plugin::{Host, Lent, Loaded, PluginView, ThemeView};

#[derive(Default)]
pub struct Plugins {
    host: Option<Host>,
    loaded: Vec<Loaded>,
    /// Palettes read from files a manifest listed. Separate from the loaded
    /// components because a theme pack has no code at all — reading its
    /// files is the whole of loading it.
    themes: Vec<ThemeView>,
    /// Plugins with no code at all — a theme pack is the common one. Kept so
    /// the list shows what you installed rather than only what runs.
    data: Vec<proton_plugin::Installed>,
    /// Tree-sitter grammars, which are wasm but not components: tree-sitter
    /// loads and runs them through its own engine, so they are held there
    /// rather than here. See `proton_syntax::wasm` for why that engine is a
    /// separate one.
    grammars: proton_syntax::Grammars,
    /// Plugins that are installed and did not load, and why. Kept rather than
    /// dropped: a plugin that failed silently is one you uninstall in
    /// confusion, having never been told what was wrong with it.
    problems: Vec<(String, String)>,
}

impl std::fmt::Debug for Plugins {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plugins")
            .field("loaded", &self.loaded.len())
            .field("problems", &self.problems.len())
            .finish()
    }
}

impl Plugins {
    /// Load everything in the plugins folder.
    ///
    /// Called explicitly rather than on construction, and off by default, for
    /// the reason the language servers are: a core test must not start a wasm
    /// engine and compile whatever happens to be installed on the machine
    /// running it.
    pub fn load(&mut self, from: &std::path::Path) {
        let host = match Host::new() {
            Ok(host) => host,
            Err(why) => {
                self.problems.push(("wasm".into(), why));
                return;
            }
        };

        let (found, broken) = proton_plugin::discover(from);
        for why in broken {
            self.problems.push((String::new(), why));
        }

        for installed in found {
            let id = installed.manifest.id.clone();

            // Themes first, and whatever the component turns out to be. A
            // palette is a list of colours and needs no program to ship it,
            // so these are read whether or not the plugin has code — and a
            // broken one is named rather than dropped.
            for relative in &installed.manifest.themes {
                match ThemeView::read(&installed, relative) {
                    Ok(theme) => self.themes.push(theme),
                    Err(why) => self.problems.push((id.clone(), why)),
                }
            }

            // Grammars, which are wasm but not components. Read here and
            // handed to tree-sitter, which owns the engine that runs them.
            for grammar in &installed.manifest.grammars {
                match read_grammar(&installed, grammar) {
                    Ok(added) => {
                        if let Err(why) = self.grammars.add(added) {
                            self.problems.push((id.clone(), why));
                        }
                    }
                    Err(why) => self.problems.push((id.clone(), why)),
                }
            }

            // A plugin with no component is not a failure: a theme pack has
            // nothing to run, which is the simplest useful plugin there is.
            // It is still listed, so you can see what you installed.
            let Some(wasm) = installed.manifest.wasm.clone() else {
                self.data.push(installed);
                continue;
            };
            match Loaded::load(&host, installed, &wasm) {
                Ok(plugin) => self.loaded.push(plugin),
                Err(why) => self.problems.push((id, why)),
            }
        }
        self.host = Some(host);
    }

    pub fn is_empty(&self) -> bool {
        self.loaded.is_empty() && self.problems.is_empty() && self.themes.is_empty()
    }

    /// The grammars the plugins brought, for the highlighter.
    pub fn grammars(&self) -> &proton_syntax::Grammars {
        &self.grammars
    }

    /// Every palette every plugin brought.
    ///
    /// From two places, which are the same thing downstream: files a manifest
    /// listed, and whatever a component's `themes()` returned. The file is
    /// how you ship a palette without writing a program, and that is the
    /// whole point of a theme pack.
    pub fn themes(&self) -> Vec<ThemeView> {
        let mut all = self.themes.clone();
        all.extend(self.loaded.iter().flat_map(|plugin| {
            let from = plugin.installed.manifest.id.clone();
            plugin.themes().iter().map(move |theme| ThemeView {
                id: theme.id.clone(),
                name: theme.name.clone(),
                dark: theme.dark,
                colours: theme
                    .colours
                    .iter()
                    .map(|s| (s.name.clone(), s.value.clone()))
                    .collect(),
                from: from.clone(),
            })
        }));
        all
    }

    /// Every command every loaded plugin offers, as `(plugin id, command)`.
    ///
    /// Flattened, because that is how they are chosen: you look for what you
    /// want to do, not for which plugin happens to provide it.
    pub fn commands(&self) -> Vec<(String, proton_plugin::Command)> {
        self.loaded
            .iter()
            .flat_map(|plugin| {
                let id = plugin.installed.manifest.id.clone();
                plugin
                    .commands()
                    .iter()
                    .map(move |c| (id.clone(), c.clone()))
            })
            .collect()
    }

    /// Run one, and say what it produced.
    pub fn run(
        &self,
        plugin: &str,
        command: &str,
        lent: Lent,
    ) -> Result<proton_plugin::Outcome, String> {
        let (Some(host), Some(found)) = (
            self.host.as_ref(),
            self.loaded
                .iter()
                .find(|p| p.installed.manifest.id == plugin),
        ) else {
            return Err(format!("{plugin} is not loaded"));
        };
        found.run(host, command, lent)
    }

    /// What the front end lists.
    pub fn view(&self) -> Vec<PluginView> {
        let mut all: Vec<PluginView> = self
            .loaded
            .iter()
            .map(|plugin| {
                let manifest = &plugin.installed.manifest;
                PluginView {
                    id: manifest.id.clone(),
                    name: manifest.name.clone(),
                    description: manifest.description.clone(),
                    version: manifest.version.clone(),
                    provides: provides(plugin),
                    problem: String::new(),
                }
            })
            .collect();

        // The ones with no code. They still brought something, or they would
        // not be installed.
        all.extend(self.data.iter().map(|installed| {
            let manifest = &installed.manifest;
            let themes = self.themes.iter().filter(|t| t.from == manifest.id).count();
            PluginView {
                id: manifest.id.clone(),
                name: manifest.name.clone(),
                description: manifest.description.clone(),
                version: manifest.version.clone(),
                provides: count(themes, "theme", "themes").unwrap_or_else(|| "nothing yet".into()),
                problem: String::new(),
            }
        }));

        all.extend(self.problems.iter().map(|(id, why)| PluginView {
            id: id.clone(),
            name: match id.is_empty() {
                true => "a plugin".into(),
                false => id.clone(),
            },
            description: String::new(),
            version: String::new(),
            provides: String::new(),
            problem: why.clone(),
        }));
        all
    }
}

/// What a plugin brought, in words.
///
/// A sentence rather than a row of counts: a list of numbers is not what
/// anybody scanning their installed plugins is reading it for.
fn provides(plugin: &Loaded) -> String {
    let manifest = &plugin.installed.manifest;
    let parts: Vec<String> = [
        // Both kinds of theme: the ones its code builds and the ones it ships
        // as files. To a reader they are the same thing.
        count(
            plugin.themes().len() + manifest.themes.len(),
            "theme",
            "themes",
        ),
        count(plugin.commands().len(), "command", "commands"),
        count(manifest.grammars.len(), "grammar", "grammars"),
    ]
    .into_iter()
    .flatten()
    .collect();

    match parts.is_empty() {
        true => "nothing yet".into(),
        false => parts.join(" · "),
    }
}

/// Read a grammar's two files off the disk.
///
/// Both go through the plugin's own path check, for the reason every path in
/// a manifest does: it is a file somebody else wrote.
fn read_grammar(
    installed: &proton_plugin::Installed,
    grammar: &proton_plugin::Grammar,
) -> Result<proton_syntax::Added, String> {
    let wasm = std::fs::read(installed.file(&grammar.wasm)?)
        .map_err(|err| format!("{}: {err}", grammar.wasm))?;
    let query = std::fs::read_to_string(installed.file(&grammar.highlights)?)
        .map_err(|err| format!("{}: {err}", grammar.highlights))?;

    Ok(proton_syntax::Added {
        id: grammar.language.clone(),
        // Lower-cased once here, because that is how the highlighter looks
        // one up and a manifest saying `Zig` should not mean no colours.
        extensions: grammar
            .extensions
            .iter()
            .map(|e| e.to_ascii_lowercase())
            .collect(),
        wasm,
        query,
    })
}

fn count(n: usize, one: &str, many: &str) -> Option<String> {
    match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{n} {many}")),
    }
}

/// Where plugins live: beside the session, because they belong to the person
/// rather than to the project.
pub fn folder() -> Option<PathBuf> {
    Some(crate::folder::config()?.join("plugins"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workspace_with_no_plugins_folder_says_nothing() {
        // Most people have none. An editor that complains at startup about an
        // empty extension point is one you learn to ignore.
        let mut plugins = Plugins::default();
        plugins.load(std::path::Path::new("/no/such/place"));
        assert!(plugins.commands().is_empty());
        assert!(plugins.view().is_empty(), "{:?}", plugins.view());
    }

    #[test]
    fn plugins_are_not_loaded_until_they_are_asked_for() {
        // Default is empty and starts no engine, so a core test does not
        // compile whatever is installed on the machine running it.
        let plugins = Plugins::default();
        assert!(plugins.is_empty());
        assert!(plugins.commands().is_empty());
    }
}

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

use proton_plugin::{Host, Lent, Loaded, PluginView};

#[derive(Default)]
pub struct Plugins {
    host: Option<Host>,
    loaded: Vec<Loaded>,
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
            // A plugin with no component is not a failure: a theme pack has
            // nothing to run, which is the simplest useful plugin there is.
            let Some(wasm) = installed.manifest.wasm.clone() else {
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
        self.loaded.is_empty() && self.problems.is_empty()
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
    let mut parts = Vec::new();
    let say = |n: usize, one: &str, many: &str| match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{n} {many}")),
    };
    parts.extend(say(plugin.themes().len(), "theme", "themes"));
    parts.extend(say(plugin.commands().len(), "command", "commands"));
    parts.extend(say(
        plugin.installed.manifest.grammars.len(),
        "grammar",
        "grammars",
    ));
    match parts.is_empty() {
        true => "nothing yet".into(),
        false => parts.join(" · "),
    }
}

/// Where plugins live: beside the session, because they belong to the person
/// rather than to the project.
pub fn folder() -> Option<PathBuf> {
    if cfg!(windows) {
        let base = std::env::var_os("APPDATA")?;
        return Some(PathBuf::from(base).join("Proton").join("plugins"));
    }
    if let Some(base) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(base).join("proton").join("plugins"));
    }
    Some(
        crate::folder::home()?
            .join(".config")
            .join("proton")
            .join("plugins"),
    )
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

//! One loaded plugin: instantiated, and asked what it brought.

use wasmtime::component::{Component, Linker};
use wasmtime::Store;

use crate::host::Host;
use crate::manifest::Installed;

wasmtime::component::bindgen!({
    path: "wit",
    world: "plugin",
});

// The world's `use editor.{...}` already puts these here; bindgen emits them
// at the top level and re-exporting them from the interface would be the same
// names twice.

/// What the editor lends a plugin while one of its calls is running.
///
/// A borrow rather than a copy of the workspace: the whole surface is three
/// functions, and a plugin call is a moment rather than a session. Holding
/// snapshots would mean deciding when they go stale.
#[derive(Debug, Default, Clone)]
pub struct Lent {
    pub buffer: String,
    pub cursor: (u32, u32),
    pub root: String,
}

impl proton::plugin::editor::Host for Lent {
    fn read_buffer(&mut self) -> String {
        self.buffer.clone()
    }

    fn cursor(&mut self) -> (u32, u32) {
        self.cursor
    }

    fn root(&mut self) -> String {
        self.root.clone()
    }
}

/// A plugin with code, ready to be asked things.
pub struct Loaded {
    pub installed: Installed,
    component: Component,
    linker: Linker<Lent>,
    /// What it said it contributes, asked for once at load.
    themes: Vec<Theme>,
    commands: Vec<Command>,
}

impl std::fmt::Debug for Loaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Loaded")
            .field("id", &self.installed.manifest.id)
            .field("themes", &self.themes.len())
            .field("commands", &self.commands.len())
            .finish()
    }
}

impl Loaded {
    /// Compile a plugin's component and ask it what it brought.
    ///
    /// Both calls happen here, at load, rather than the first time anybody
    /// looks: a picker that has to run wasm to know what to list is a picker
    /// that stutters the first time it opens.
    pub fn load(host: &Host, installed: Installed, wasm: &str) -> Result<Loaded, String> {
        let path = installed.file(wasm)?;
        let component = Component::from_file(host.engine(), &path)
            .map_err(|err| format!("{}: {err}", installed.manifest.id))?;

        let mut linker = Linker::new(host.engine());
        Plugin::add_to_linker::<Lent, wasmtime::component::HasSelf<Lent>>(&mut linker, |it| it)
            .map_err(|err| format!("{}: {err}", installed.manifest.id))?;

        let mut loaded = Loaded {
            installed,
            component,
            linker,
            themes: Vec::new(),
            commands: Vec::new(),
        };
        let (themes, commands) = loaded.declared(host)?;
        loaded.themes = themes;
        loaded.commands = commands;
        Ok(loaded)
    }

    fn instantiate(&self, host: &Host, lent: Lent) -> Result<(Store<Lent>, Plugin), String> {
        let mut store = host.store(lent);
        let plugin = Plugin::instantiate(&mut store, &self.component, &self.linker)
            .map_err(|err| format!("{}: {err}", self.installed.manifest.id))?;
        Ok((store, plugin))
    }

    fn declared(&self, host: &Host) -> Result<(Vec<Theme>, Vec<Command>), String> {
        let (mut store, plugin) = self.instantiate(host, Lent::default())?;
        let themes = plugin
            .call_themes(&mut store)
            .map_err(|err| self.blame("themes", &err))?;
        Host::refresh(&mut store);
        let commands = plugin
            .call_commands(&mut store)
            .map_err(|err| self.blame("commands", &err))?;
        Ok((themes, commands))
    }

    pub fn themes(&self) -> &[Theme] {
        &self.themes
    }

    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    /// Run one of this plugin's commands.
    ///
    /// A fresh instance per call. A plugin that kept state between calls
    /// would be a plugin whose behaviour depends on what you did earlier,
    /// and instantiating a compiled component is cheap — the expensive part
    /// was the compile, and that is held.
    pub fn run(&self, host: &Host, id: &str, lent: Lent) -> Result<Outcome, String> {
        if !self.commands.iter().any(|c| c.id == id) {
            return Err(format!("{}: no command {id}", self.installed.manifest.id));
        }
        let (mut store, plugin) = self.instantiate(host, lent)?;
        plugin
            .call_run(&mut store, id)
            .map_err(|err| self.blame(id, &err))
    }

    /// Say which plugin failed, and say plainly when the deadline was what
    /// stopped it — "epoch deadline reached" is the engine's words for
    /// something the reader knows as a plugin that would not stop.
    fn blame(&self, what: &str, err: &wasmtime::Error) -> String {
        let id = &self.installed.manifest.id;
        match err.downcast_ref::<wasmtime::Trap>() {
            Some(wasmtime::Trap::Interrupt) => {
                format!("{id}: {what} did not finish in time and was stopped")
            }
            _ => format!("{id}: {what} failed: {err}"),
        }
    }
}

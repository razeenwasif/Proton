//! Plugins: wasm components, sandboxed, on a deadline.
//!
//! Two things are configured on day one rather than added later, because both
//! are guarantees rather than features and a guarantee retrofitted is a
//! guarantee that was false in between.
//!
//! **A plugin cannot hang the editor.** Wasm does not stop on its own; an
//! infinite loop spins forever and takes the thread with it unless the engine
//! is told otherwise. [`Host`] switches on epoch interruption and every store
//! it hands out already carries a deadline, so there is no path that runs
//! plugin code off the clock.
//!
//! **The API is coarse.** A host call crosses a sandbox boundary and costs
//! more than a function call, so nothing here fires per keystroke. A plugin
//! contributes *things* — themes, grammars, commands — and is called when one
//! of them is chosen.
//!
//! And one thing follows from both: a plugin's manifest is read before any of
//! its code runs, so a plugin that only ships a theme never runs at all.

pub mod host;
pub mod manifest;
pub mod plugin;

pub use host::{Host, Shared, DEADLINE};
pub use manifest::{discover, Grammar, Installed, Manifest, PluginView, ThemeView};
pub use plugin::proton::plugin::editor::Swatch;
pub use plugin::{Command, Lent, Loaded, Outcome, Theme};

//! Game mode: Proton's own engine, made for a few kinds of game rather than
//! every kind.
//!
//! ## Why its own, when the architecture once said to embed one
//!
//! An engine embedded is an engine whose files are somebody else's, whose
//! scenes play in somebody else's window, and whose runs the agent cannot
//! step through. The kinds of game Proton is for — hand-drawn and painted
//! side-scrollers on dozens of parallax planes, pixel art lit like a modern
//! game, isometric rooms, low-poly worlds; Metroidvanias and roguelites — are
//! narrow enough that an engine shaped to them is smaller than a general one,
//! and every part of it can be something the editor, the window and the agent
//! all see.
//!
//! ## The pieces
//!
//! - [`format`]: what a game is on disk — `game.json`, `world.json`, a file
//!   per room and per prefab, all text that diffs.
//! - [`doc`] and [`project`]: those files open for editing — changed by ops
//!   addressed by id, undone and redone, held until saved.
//! - [`path`]: every path a file names, held to the game's folder.
//! - [`template`]: a new game to start from, with its art drawn in code.
//! - [`trace`]: collision traced from where art is opaque.
//! - [`import`]: a painter's layered `.psd`, or a `.png`, brought in as a
//!   room's planes.
//!
//! - [`sim`]: the game, run — a tick at a time, in whole numbers, written
//!   once and run twice: natively in the core, where tests and the agent
//!   replay recorded input, and as WebAssembly in the window, where it is
//!   played.

pub mod doc;
pub mod format;
#[cfg(feature = "editor")]
pub mod import;
pub mod path;
pub mod project;
pub mod sim;
#[cfg(feature = "editor")]
pub mod template;
pub mod text;
pub mod trace;

pub use doc::{Doc, Document, Op};
pub use project::{Project, Target};

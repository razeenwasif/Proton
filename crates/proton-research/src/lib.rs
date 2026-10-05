//! Research's investigations: the files they are, and the co-scientist
//! round that grows them.
//!
//! An investigation is a folder in the project — `research/<slug>/` — and
//! everything it knows is a file in it that diffs in Git: the scientist's
//! brief, the plan, each hypothesis and each review, the tournament one
//! match to a line, the experiments and what running them gave. Nothing is
//! kept only in memory, so a window that closes mid-round loses nothing,
//! and whatever this crate caches can be rebuilt by reading the folder.
//!
//! This crate is pure: it starts no processes and talks to no engine. It
//! decides what the next agent task is and what an answer means; the core
//! runs the tasks on Claude Code, Codex and Antigravity, and the Lab runs
//! the code. That split is what lets the whole round be tested without a
//! model in sight.
//!
//! The shape is Google's AI co-scientist — Generation, Reflection, Ranking
//! by an Elo tournament, Proximity, Evolution, Meta-review, a supervisor
//! over them and the scientist in the loop — with the roles of the user's
//! own AGENTS.md: Claude Code the Principal Investigator, Codex the Lab
//! Engineer, Antigravity the Validation Sandbox.

pub mod classify;
pub mod elo;
pub mod lock;
pub mod markdown;
pub mod model;
pub mod parse;
pub mod prompts;
pub mod proximity;
pub mod schema;
pub mod store;
pub mod supervisor;
pub mod view;

pub use model::{Author, Discipline, Experiment, Family, Hypothesis, Investigation, Match, Review, Role, Seat};
pub use store::{Folder, Store};

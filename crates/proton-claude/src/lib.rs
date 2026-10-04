//! Claude Code — and Codex, Gemini and Antigravity beside it — in Proton's
//! Chat tab.
//!
//! Not a harness written to resemble Claude Code: Claude Code itself. The
//! `claude` command already is the harness — its tools, its permission rules,
//! CLAUDE.md, skills, subagents, compaction, the claude.ai connectors and the
//! Artifact tool — and it speaks a line-delimited JSON protocol to whatever
//! hosts it, which is how its own SDKs drive it. Proton is that host. What is
//! Proton's is everything around the protocol: the window, the questions it
//! asks you, and a copy of every artifact it publishes.
//!
//! Layered smallest first:
//!
//! - [`process`] finds `claude`, starts it for a project, and carries lines
//!   both ways. It knows the protocol's envelopes and nothing of a transcript.
//! - [`fold`] turns those lines — live ones, or a session's file read back —
//!   into the rows a transcript is drawn from, and says which ones changed.
//! - [`chat`] is one conversation: a process when one is running, its fold,
//!   and the questions waiting on you.
//! - [`history`] reads the sessions Claude Code keeps for a project, so any of
//!   them — started here, in a terminal or in the desktop app — can be picked
//!   up again.
//! - [`artifacts`] keeps what the Artifact tool publishes, a version at a time,
//!   so it can be shown beside the chat without a round trip to claude.ai.
//! - [`import`] finds what was published before Proton was keeping it: in
//!   the desktop app's records and in every session's own file.
//!
//! The other engines are each the real CLI, driven over the protocol it
//! offers a host and folded into the same rows — so one transcript, one set
//! of questions and one composer serve all four:
//!
//! - [`engine`] names them and says what any conversation can be asked to
//!   do; [`pipe`] and [`rpc`] are the plumbing they share.
//! - [`codex`] drives `codex app-server`; [`gemini`] the Gemini CLI's Agent
//!   Client Protocol mode; [`agy`] Antigravity's stream-json print mode.
//! - [`patch`] turns what each says about an edit into the hunks Claude
//!   Code's edits are drawn with, and [`saved`] keeps the conversations
//!   whose own files Proton does not read.
//! - [`folders`] keeps the folders conversations are grouped into, across
//!   every project and engine.
//!
//! Nothing here blocks the caller, and nothing here knows what Proton looks
//! like.

pub mod agy;
pub mod artifacts;
pub mod chat;
pub mod codex;
pub mod engine;
pub mod fold;
pub mod folders;
pub mod gemini;
pub mod history;
pub mod import;
pub mod patch;
pub mod pipe;
pub mod process;
pub mod rpc;
pub mod saved;

pub use artifacts::{Artifact, ArtifactRef, Imported, Store, Version};
pub use chat::{Answer, Chat, Settings};
pub use engine::{Conversation, Engine, ModeChoice};
pub use fold::{Activity, Ask, AskKind, Capabilities, Command, Fold, Item, Level, Limit, ModelChoice, Status, Task, Tool, ToolState};
pub use history::SessionSummary;
pub use process::on_output;

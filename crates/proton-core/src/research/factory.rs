//! Where a task's conversation comes from: the engines themselves, or — in
//! tests — scripted ones that answer as an engine would.
//!
//! Every task gets a conversation of its own. A co-scientist's reviewer
//! must not remember writing what it reviews, so nothing is carried from
//! one task to the next but what the files say.

use std::path::Path;

use proton_claude::chat::Settings;
use proton_claude::engine::{self, Unattended};
use proton_claude::{Conversation, Engine};
use proton_research::model::Seat;
use proton_research::supervisor::Dispatch;

/// Makes the conversations a round's tasks run in.
pub trait AgentFactory: Send {
    /// Why a seat cannot sit at all — its engine is not installed, say —
    /// or `None` if it can. Asked before a round starts, so a seat that
    /// cannot is planned around rather than failed on.
    fn unavailable(&mut self, seat: Seat) -> Option<String>;

    /// A fresh unattended conversation for one task, working in `cwd`.
    fn create(&mut self, task: &Dispatch, how: &Unattended, settings: Settings, cwd: &Path) -> Result<Box<dyn Conversation>, String>;
}

/// The engine a seat is.
pub fn engine_of(seat: Seat) -> Option<Engine> {
    match seat {
        Seat::Claude => Some(Engine::Claude),
        Seat::Codex => Some(Engine::Codex),
        Seat::Antigravity => Some(Engine::Antigravity),
        Seat::Gemini => Some(Engine::Gemini),
        Seat::Scientist => None,
    }
}

/// Claude Code, Codex and Antigravity, as they are installed here.
pub struct Engines;

impl AgentFactory for Engines {
    fn unavailable(&mut self, seat: Seat) -> Option<String> {
        match engine_of(seat) {
            Some(Engine::Gemini) => Some("The Gemini CLI cannot take part unattended yet.".into()),
            Some(engine) => engine::missing(engine),
            None => Some("The scientist is not an engine.".into()),
        }
    }

    fn create(&mut self, task: &Dispatch, how: &Unattended, settings: Settings, cwd: &Path) -> Result<Box<dyn Conversation>, String> {
        let engine = engine_of(task.seat).ok_or_else(|| format!("{} is not an engine.", task.seat.name()))?;
        engine::unattended(engine, cwd.to_path_buf(), settings, how)
    }
}

//! Where a task's conversation comes from: the engines themselves, or — in
//! tests — scripted ones that answer as an engine would.
//!
//! Every task gets a conversation of its own. A co-scientist's reviewer
//! must not remember writing what it reviews, so nothing is carried from
//! one task to the next but what the files say.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

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

    /// The model a seat is to run when the investigation names none, if it
    /// must be named: see [`Engines::default_model`].
    fn default_model(&mut self, _seat: Seat) -> Option<String> {
        None
    }
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
#[derive(Default)]
pub struct Engines {
    /// The models chosen for seats that name none, once chosen.
    chosen: BTreeMap<Seat, Option<String>>,
}

/// The first Gemini model Antigravity offers: it lists its newest first.
fn gemini(capabilities: &proton_claude::Capabilities) -> Option<String> {
    capabilities.models.iter().map(|m| m.value.clone()).find(|v| v.starts_with("gemini"))
}

impl AgentFactory for Engines {
    /// Antigravity runs Gemini, Claude and GPT models alike and does not say
    /// which it is running, so a seat whose investigation names none is
    /// given a Gemini model by name: its family is then known, not assumed,
    /// and nobody reviews or judges their own. The list is the one the Chat
    /// tab remembers, else asked of `agy` once.
    fn default_model(&mut self, seat: Seat) -> Option<String> {
        if seat != Seat::Antigravity {
            return None;
        }
        self.chosen
            .entry(seat)
            .or_insert_with(|| {
                let remembered = engine::remembered().into_iter().find(|c| c.engine == Engine::Antigravity);
                remembered.as_ref().and_then(gemini).or_else(|| {
                    let asked = proton_claude::agy::probe().recv_timeout(Duration::from_secs(25)).ok()?;
                    engine::remember(&asked);
                    gemini(&asked)
                })
            })
            .clone()
    }

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

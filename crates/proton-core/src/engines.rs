//! What each agent offers: its models, their efforts and whose account it
//! is, for Notebook's tutor to choose from.
//!
//! Each engine is asked once, on a thread, and what it answers is remembered
//! for the next run; until then what was remembered, or a short list, is on
//! offer. The engines are `proton-claude`'s.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

use proton_claude::chat;
use proton_claude::{engine, Capabilities, Engine};

use crate::bus::Event;

pub struct Engines {
    root: PathBuf,
    capabilities: HashMap<Engine, Capabilities>,
    /// Claude Code's handshake, asked of a `claude` on a thread.
    probing: Option<Receiver<Capabilities>>,
    /// The other engines, likewise; asked once a run.
    probing_others: Option<Receiver<Capabilities>>,
    probed_others: bool,
}

impl std::fmt::Debug for Engines {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engines").field("root", &self.root).finish()
    }
}

impl Default for Engines {
    fn default() -> Engines {
        Engines::new(Path::new("."))
    }
}

/// The models on offer before any `claude` has said which it has.
fn fallback() -> Capabilities {
    let efforts: Vec<String> = ["low", "medium", "high", "xhigh", "max"].iter().map(|s| s.to_string()).collect();
    let model = |value: &str, name: &str, description: &str| proton_claude::ModelChoice {
        value: value.into(),
        resolved: String::new(),
        name: name.into(),
        description: description.into(),
        efforts: efforts.clone(),
    };
    Capabilities {
        engine: Engine::Claude,
        models: vec![
            model("default", "Default (recommended)", "Claude Code's choice"),
            model("opus", "Opus", "For everyday, complex tasks"),
            model("fable", "Fable", "For the hardest, longest-running tasks"),
            model("sonnet", "Sonnet", "Fast and capable"),
            model("haiku", "Haiku", "Fastest, for quick answers"),
        ],
        modes: Engine::Claude.modes(),
        ..Capabilities::default()
    }
}

impl Engines {
    pub fn new(root: &Path) -> Engines {
        let mut capabilities = HashMap::new();
        let mut claude = chat::remembered().unwrap_or_else(fallback);
        claude.engine = Engine::Claude;
        claude.modes = Engine::Claude.modes();
        capabilities.insert(Engine::Claude, claude);
        for engine in [Engine::Codex, Engine::Gemini, Engine::Antigravity] {
            capabilities.insert(engine, engine::fallback(engine));
        }
        for known in engine::remembered() {
            if known.engine != Engine::Claude {
                capabilities.insert(known.engine, known);
            }
        }
        Engines { root: root.to_path_buf(), capabilities, probing: None, probing_others: None, probed_others: false }
    }

    /// What is known, for a window that has just connected, and the asking
    /// begun of any engine not yet asked.
    pub fn sync(&mut self) -> Vec<Event> {
        let claude_known = self.capabilities.get(&Engine::Claude).is_some_and(|c| !c.commands.is_empty());
        if !claude_known && self.probing.is_none() && self.root.is_dir() {
            self.probing = Some(chat::probe(self.root.clone()));
        }
        if !self.probed_others && self.root.is_dir() {
            self.probed_others = true;
            self.probing_others = Some(engine::probe_others(self.root.clone()));
        }
        Engine::ALL
            .iter()
            .filter_map(|engine| self.capabilities.get(engine))
            .map(|capabilities| Event::ClaudeCapabilities { capabilities: capabilities.clone() })
            .collect()
    }

    /// What an engine offers, as it last said: remembered, and told.
    fn learned(&mut self, mut capabilities: Capabilities) -> Vec<Event> {
        let engine = capabilities.engine;
        let known = self.capabilities.get(&engine);
        // An engine may answer with less than it answered before: what it
        // leaves out stays as it was known.
        if capabilities.models.is_empty() {
            match known {
                Some(known) if !known.models.is_empty() => capabilities.models = known.models.clone(),
                _ if engine == Engine::Claude => return Vec::new(),
                _ => {}
            }
        }
        if let Some(known) = known {
            if capabilities.plan.is_empty() && capabilities.email.is_empty() {
                capabilities.plan = known.plan.clone();
                capabilities.email = known.email.clone();
            }
            if capabilities.commands.is_empty() {
                capabilities.commands = known.commands.clone();
            }
        }
        if capabilities.modes.is_empty() {
            capabilities.modes = engine.modes();
        }
        if Some(&capabilities) == known {
            return Vec::new();
        }
        match engine {
            Engine::Claude => chat::remember(&capabilities),
            _ => engine::remember(&capabilities),
        }
        self.capabilities.insert(engine, capabilities.clone());
        vec![Event::ClaudeCapabilities { capabilities }]
    }

    pub fn poll(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        if let Some(probing) = &self.probing {
            match probing.try_recv() {
                Ok(capabilities) => {
                    self.probing = None;
                    events.extend(self.learned(capabilities));
                }
                Err(TryRecvError::Disconnected) => self.probing = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        while let Some(probing) = &self.probing_others {
            match probing.try_recv() {
                Ok(capabilities) => events.extend(self.learned(capabilities)),
                Err(TryRecvError::Disconnected) => {
                    self.probing_others = None;
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        events
    }
}

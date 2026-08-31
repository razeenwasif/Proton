//! Workspace state — the thing the shell is a view of.
//!
//! `apply` is deliberately synchronous and total: one command in, a list of
//! events out, no I/O, no clock. That is what makes the core testable headless
//! and what will let a second front end reuse it verbatim.

use crate::action::Action;
use crate::bus::{Command, Event};
use crate::mode::ModeId;
use crate::vim::{Outcome, Vim, VimMode};

#[derive(Debug, Default)]
pub struct Workspace {
    mode: ModeId,
    vim: Vim,
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mode(&self) -> ModeId {
        self.mode
    }

    pub fn vim_mode(&self) -> VimMode {
        self.vim.mode()
    }

    pub fn apply(&mut self, command: Command) -> Vec<Event> {
        match command {
            Command::Sync => vec![self.state_event()],
            Command::SetMode { mode } => self.set_mode(mode),
            Command::Perform { action } => self.perform(action),
            Command::Key { key } => {
                let before = self.vim.mode();
                let outcome = self.vim.key(&key);
                let mut events = self.events_for(outcome);
                // The mode transition is reported after whatever the key did,
                // so a statusline applying events in order ends up correct.
                if self.vim.mode() != before {
                    events.push(self.vim_event());
                }
                events
            }
        }
    }

    fn set_mode(&mut self, mode: ModeId) -> Vec<Event> {
        if mode == self.mode {
            return Vec::new();
        }
        self.mode = mode;
        vec![Event::ModeChanged { mode }]
    }

    fn perform(&mut self, action: Action) -> Vec<Event> {
        let description = action.describe();
        if self.mode.supports(&action) {
            vec![Event::Action {
                action,
                description,
            }]
        } else {
            vec![Event::Unsupported {
                description,
                mode: self.mode,
            }]
        }
    }

    fn events_for(&mut self, outcome: Outcome) -> Vec<Event> {
        match outcome {
            Outcome::Action(action) => self.perform(action),
            Outcome::Pending(keys) => vec![Event::Pending { keys }],
            Outcome::Leader(prefix) => vec![Event::Leader {
                prefix: Some(prefix),
            }],
            Outcome::Cmdline(text) => vec![Event::Cmdline { text }],
            Outcome::Input(c) => vec![Event::Input {
                text: c.to_string(),
            }],
            Outcome::Ignored => Vec::new(),
        }
    }

    fn vim_event(&self) -> Event {
        let mode = self.vim.mode();
        Event::VimMode {
            mode,
            label: mode.label().to_string(),
        }
    }

    fn state_event(&self) -> Event {
        let vim_mode = self.vim.mode();
        Event::State {
            mode: self.mode,
            vim_mode,
            vim_label: vim_mode.label().to_string(),
        }
    }
}

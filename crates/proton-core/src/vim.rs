//! The modal input engine.
//!
//! This is an application service, not an editor feature: it lives in the core,
//! it knows nothing about buffers or pixels, and its only output is a resolved
//! [`Action`] plus mode transitions. Every workspace mode consumes the same
//! stream.
//!
//! M0 implements counts, registers, operator-pending state, the doubled-operator
//! forms (`dd`, `yy`, `cc`), the common motions, and the leader. Text objects
//! (`ciw`), marks, and macros land in M1 — the state machine is shaped to take
//! them without restructuring.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::action::{Action, Object, Verb};
use crate::key::Key;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub enum VimMode {
    #[default]
    Normal,
    Insert,
    Visual,
    VisualLine,
    Replace,
    Command,
}

impl VimMode {
    /// What the statusline shows.
    pub fn label(self) -> &'static str {
        match self {
            VimMode::Normal => "NORMAL",
            VimMode::Insert => "INSERT",
            VimMode::Visual => "VISUAL",
            VimMode::VisualLine => "V-LINE",
            VimMode::Replace => "REPLACE",
            VimMode::Command => "COMMAND",
        }
    }
}

/// What a keystroke produced.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// A complete action, ready for the active mode to execute.
    Action(Action),
    /// The key was absorbed into a pending command (`2`, `d`, `"a`). The string
    /// is what `showcmd` displays in the corner.
    Pending(String),
    /// The leader was pressed and which-key should open, or a subsequent key
    /// narrowed it. The string is the prefix so far.
    Leader(String),
    /// The command line changed. `None` means it closed without executing.
    Cmdline(Option<String>),
    /// The command line was submitted. Distinct from `Cmdline` because a
    /// consumer has to be able to tell a keystroke from an execution — they
    /// carry the same text and mean entirely different things.
    Execute(String),
    /// A literal character typed in insert mode.
    Input(char),
    /// Nothing happened. Deliberately distinct from an action with no effect.
    Ignored,
}

#[derive(Debug, Default)]
struct Pending {
    count: Option<u32>,
    operator: Option<Verb>,
    /// The count typed *after* an operator: the 3 in `d3w`.
    operator_count: Option<u32>,
    register: Option<String>,
    /// Set after `"` while waiting for the register name.
    awaiting_register: bool,
}

impl Pending {
    fn is_empty(&self) -> bool {
        self.count.is_none()
            && self.operator.is_none()
            && self.operator_count.is_none()
            && self.register.is_none()
            && !self.awaiting_register
    }

    /// vim multiplies the two counts: `2d3w` deletes six words.
    fn total_count(&self) -> u32 {
        self.count
            .unwrap_or(1)
            .saturating_mul(self.operator_count.unwrap_or(1))
            .max(1)
    }

    fn clear(&mut self) {
        *self = Pending::default();
    }

    fn showcmd(&self) -> String {
        let mut s = String::new();
        if let Some(r) = &self.register {
            s.push('"');
            s.push_str(r);
        } else if self.awaiting_register {
            s.push('"');
        }
        if let Some(c) = self.count {
            s.push_str(&c.to_string());
        }
        if let Some(op) = self.operator {
            s.push(match op {
                Verb::Delete => 'd',
                Verb::Yank => 'y',
                Verb::Change => 'c',
                _ => '?',
            });
        }
        if let Some(c) = self.operator_count {
            s.push_str(&c.to_string());
        }
        s
    }
}

#[derive(Debug, Default)]
pub struct Vim {
    mode: VimMode,
    pending: Pending,
    cmdline: String,
    /// Keys typed after the leader, before which-key resolves.
    leader: Option<String>,
}

impl Vim {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mode(&self) -> VimMode {
        self.mode
    }

    pub fn cmdline(&self) -> &str {
        &self.cmdline
    }

    pub fn leader_prefix(&self) -> Option<&str> {
        self.leader.as_deref()
    }

    /// Feed one keystroke. Returns what it produced and leaves the engine in
    /// whatever state that implies.
    pub fn key(&mut self, k: &Key) -> Outcome {
        // Escape always unwinds one level, from any mode. This is checked first
        // so no other branch can swallow it.
        if k.key == "Escape" {
            return self.escape();
        }

        match self.mode {
            VimMode::Insert | VimMode::Replace => self.key_insert(k),
            VimMode::Command => self.key_cmdline(k),
            VimMode::Normal | VimMode::Visual | VimMode::VisualLine => self.key_normal(k),
        }
    }

    fn escape(&mut self) -> Outcome {
        // In normal mode with something pending, Escape cancels the pending
        // command rather than doing nothing.
        if self.mode == VimMode::Normal && (!self.pending.is_empty() || self.leader.is_some()) {
            self.pending.clear();
            self.leader = None;
            return Outcome::Pending(String::new());
        }
        let was_cmdline = self.mode == VimMode::Command;
        self.pending.clear();
        self.leader = None;
        self.cmdline.clear();
        self.mode = VimMode::Normal;
        if was_cmdline {
            Outcome::Cmdline(None)
        } else {
            Outcome::Ignored
        }
    }

    fn key_insert(&mut self, k: &Key) -> Outcome {
        match k.as_char() {
            Some(c) if k.is_bare_char() => Outcome::Input(c),
            _ => Outcome::Ignored,
        }
    }

    fn key_cmdline(&mut self, k: &Key) -> Outcome {
        match k.key.as_str() {
            "Enter" => {
                let line = std::mem::take(&mut self.cmdline);
                self.mode = VimMode::Normal;
                Outcome::Execute(line)
            }
            "Backspace" => {
                if self.cmdline.pop().is_none() {
                    self.mode = VimMode::Normal;
                    return Outcome::Cmdline(None);
                }
                Outcome::Cmdline(Some(self.cmdline.clone()))
            }
            _ => match k.as_char() {
                Some(c) if k.is_bare_char() => {
                    self.cmdline.push(c);
                    Outcome::Cmdline(Some(self.cmdline.clone()))
                }
                _ => Outcome::Ignored,
            },
        }
    }

    fn key_normal(&mut self, k: &Key) -> Outcome {
        let Some(c) = k.as_char().filter(|_| k.is_bare_char()) else {
            return Outcome::Ignored;
        };

        // Leader sequence in progress: collect until a mode or plugin claims it.
        if let Some(prefix) = &mut self.leader {
            prefix.push(c);
            return Outcome::Leader(prefix.clone());
        }

        // `"a` selects a register for the next operator.
        if self.pending.awaiting_register {
            self.pending.awaiting_register = false;
            self.pending.register = Some(c.to_string());
            return Outcome::Pending(self.pending.showcmd());
        }
        if c == '"' {
            self.pending.awaiting_register = true;
            return Outcome::Pending(self.pending.showcmd());
        }

        // Digits build a count. A leading `0` is the line-start motion, not a
        // count — that ordering is why this branch tests for an existing count.
        if c.is_ascii_digit() && !(c == '0' && self.count_slot().is_none()) {
            let digit = c.to_digit(10).unwrap();
            let slot = self.count_slot_mut();
            *slot = Some(slot.unwrap_or(0).saturating_mul(10).saturating_add(digit));
            return Outcome::Pending(self.pending.showcmd());
        }

        // Leader is Space, and only in normal mode with nothing pending.
        if c == ' ' && self.mode == VimMode::Normal && self.pending.is_empty() {
            self.leader = Some(String::new());
            return Outcome::Leader(String::new());
        }

        // An operator already waiting: this key must be a motion, a doubled
        // operator, or nothing.
        if let Some(op) = self.pending.operator {
            if Self::doubles(op, c) {
                let count = self.pending.total_count();
                let register = self.pending.register.clone();
                self.pending.clear();
                if op == Verb::Change {
                    self.mode = VimMode::Insert;
                }
                let mut action = Action::new(op, Object::Line, count);
                action.register = register;
                return Outcome::Action(action);
            }
            if let Some(object) = Self::motion(c) {
                let count = self.pending.total_count();
                let register = self.pending.register.clone();
                self.pending.clear();
                if op == Verb::Change {
                    self.mode = VimMode::Insert;
                }
                let mut action = Action::new(op, object, count);
                action.register = register;
                return Outcome::Action(action);
            }
            // Not a motion — cancel, as vim does.
            self.pending.clear();
            return Outcome::Pending(String::new());
        }

        // Visual mode: an operator applies to the selection immediately.
        let visual = matches!(self.mode, VimMode::Visual | VimMode::VisualLine);
        if visual {
            if let Some(verb) = Self::operator(c) {
                let count = self.pending.total_count();
                let register = self.pending.register.clone();
                self.pending.clear();
                self.mode = if verb == Verb::Change {
                    VimMode::Insert
                } else {
                    VimMode::Normal
                };
                let mut action = Action::new(verb, Object::Selection, count);
                action.register = register;
                return Outcome::Action(action);
            }
        }

        self.normal_command(c)
    }

    fn normal_command(&mut self, c: char) -> Outcome {
        let count = self.pending.total_count();

        // Mode entries.
        let enter = |vim: &mut Vim, mode: VimMode, verb: Verb, object: Object| {
            vim.pending.clear();
            vim.mode = mode;
            Outcome::Action(Action::new(verb, object, 1))
        };

        match c {
            'i' => return enter(self, VimMode::Insert, Verb::Insert, Object::Here),
            'a' => return enter(self, VimMode::Insert, Verb::Insert, Object::After),
            'I' => return enter(self, VimMode::Insert, Verb::Insert, Object::LineStart),
            'A' => return enter(self, VimMode::Insert, Verb::Insert, Object::LineEnd),
            'o' => return enter(self, VimMode::Insert, Verb::Insert, Object::LineBelow),
            'O' => return enter(self, VimMode::Insert, Verb::Insert, Object::LineAbove),
            'R' => {
                self.pending.clear();
                self.mode = VimMode::Replace;
                return Outcome::Ignored;
            }
            'v' => {
                self.pending.clear();
                self.mode = if self.mode == VimMode::Visual {
                    VimMode::Normal
                } else {
                    VimMode::Visual
                };
                return Outcome::Ignored;
            }
            'V' => {
                self.pending.clear();
                self.mode = if self.mode == VimMode::VisualLine {
                    VimMode::Normal
                } else {
                    VimMode::VisualLine
                };
                return Outcome::Ignored;
            }
            ':' => {
                self.pending.clear();
                self.mode = VimMode::Command;
                self.cmdline.clear();
                return Outcome::Cmdline(Some(String::new()));
            }
            _ => {}
        }

        // Operators begin an operator-pending state.
        if let Some(verb) = Self::operator(c) {
            self.pending.operator = Some(verb);
            return Outcome::Pending(self.pending.showcmd());
        }

        // Standalone verbs.
        let standalone = match c {
            'u' => Some(Action::new(Verb::Undo, Object::None, count)),
            'p' => Some(Action::new(Verb::Paste, Object::After, count)),
            'P' => Some(Action::new(Verb::Paste, Object::Here, count)),
            '.' => Some(Action::new(Verb::Repeat, Object::None, count)),
            'x' => Some(Action::new(Verb::Delete, Object::CharRight, count)),
            'D' => Some(Action::new(Verb::Delete, Object::LineEnd, count)),
            'C' => Some(Action::new(Verb::Change, Object::LineEnd, count)),
            _ => None,
        };
        if let Some(mut action) = standalone {
            action.register = self.pending.register.clone();
            self.pending.clear();
            if action.verb == Verb::Change {
                self.mode = VimMode::Insert;
            }
            return Outcome::Action(action);
        }

        // Bare motions.
        if let Some(object) = Self::motion(c) {
            self.pending.clear();
            return Outcome::Action(Action::new(Verb::Move, object, count));
        }

        self.pending.clear();
        Outcome::Ignored
    }

    /// Which count a digit belongs to: before an operator it is the outer
    /// count, after it the inner one.
    fn count_slot(&self) -> Option<u32> {
        if self.pending.operator.is_some() {
            self.pending.operator_count
        } else {
            self.pending.count
        }
    }

    fn count_slot_mut(&mut self) -> &mut Option<u32> {
        if self.pending.operator.is_some() {
            &mut self.pending.operator_count
        } else {
            &mut self.pending.count
        }
    }

    fn operator(c: char) -> Option<Verb> {
        match c {
            'd' => Some(Verb::Delete),
            'y' => Some(Verb::Yank),
            'c' => Some(Verb::Change),
            _ => None,
        }
    }

    /// `dd`, `yy`, `cc` — an operator doubled means "this whole line".
    fn doubles(op: Verb, c: char) -> bool {
        matches!(
            (op, c),
            (Verb::Delete, 'd') | (Verb::Yank, 'y') | (Verb::Change, 'c')
        )
    }

    fn motion(c: char) -> Option<Object> {
        Some(match c {
            'h' => Object::CharLeft,
            'l' => Object::CharRight,
            'j' => Object::LineDown,
            'k' => Object::LineUp,
            'w' => Object::WordForward,
            'b' => Object::WordBackward,
            'e' => Object::WordEnd,
            '0' => Object::LineStart,
            '$' => Object::LineEnd,
            'G' => Object::FileEnd,
            _ => return None,
        })
    }
}

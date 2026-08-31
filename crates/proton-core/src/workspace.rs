//! Workspace state — the thing the shell is a view of.
//!
//! `apply` is deliberately total: one command in, a list of events out. The
//! only I/O is opening and saving a file, which is what a command asked for.
//! No clock, no threads, no windowing — that is what makes the core testable
//! headless and lets a second front end reuse it verbatim.

use crate::action::{Action, Verb};
use crate::bus::{BufferTab, Command, Event, NoticeLevel, Selection};
use crate::editor::Editor;
use crate::mode::ModeId;
use crate::vim::{Outcome, Vim, VimMode};

/// What the front end can currently show. The core keeps it so a buffer change
/// can send back exactly the lines that are on screen and nothing more.
#[derive(Debug, Clone, Copy)]
struct Viewport {
    first_line: usize,
    lines: usize,
}

impl Default for Viewport {
    fn default() -> Self {
        // Enough to be useful before the front end reports its real height.
        Self {
            first_line: 0,
            lines: 80,
        }
    }
}

#[derive(Debug, Default)]
pub struct Workspace {
    mode: ModeId,
    vim: Vim,
    editor: Editor,
    viewport: Viewport,
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            editor: Editor::new(),
            ..Default::default()
        }
    }

    pub fn mode(&self) -> ModeId {
        self.mode
    }

    pub fn vim_mode(&self) -> VimMode {
        self.vim.mode()
    }

    pub fn editor(&self) -> &Editor {
        &self.editor
    }

    pub fn apply(&mut self, command: Command) -> Vec<Event> {
        match command {
            Command::Sync => vec![
                self.state_event(),
                self.buffers_event(),
                self.viewport_event(),
                self.cursor_event(),
            ],

            Command::SetMode { mode } => self.set_mode(mode),

            Command::Perform { action } => {
                let (mut events, changed) = self.act(&action);
                events.extend(self.after_action(changed));
                events
            }

            Command::Viewport { first_line, lines } => {
                self.viewport = Viewport {
                    first_line,
                    lines: lines.max(1),
                };
                vec![self.viewport_event()]
            }

            Command::FocusBuffer { index } => {
                if !self.editor.focus(index) {
                    return vec![notice(format!("no buffer {index}"), NoticeLevel::Warn)];
                }
                vec![
                    self.buffers_event(),
                    self.viewport_event(),
                    self.cursor_event(),
                ]
            }

            Command::OpenBuffer { path } => match self.editor.open(&path) {
                Ok(_) => vec![
                    self.buffers_event(),
                    self.viewport_event(),
                    self.cursor_event(),
                ],
                Err(err) => vec![notice(format!("{path}: {err}"), NoticeLevel::Error)],
            },

            Command::Save => match self.editor.save() {
                Ok(()) => {
                    let name = self.editor.buffer().name();
                    vec![
                        notice(format!("wrote {name}"), NoticeLevel::Info),
                        self.buffers_event(),
                    ]
                }
                Err(err) => vec![notice(err.to_string(), NoticeLevel::Error)],
            },

            Command::Key { key } => {
                let before = self.vim.mode();
                let mut events = Vec::new();
                let mut changed = false;

                // Insert-mode editing keys that are text entry rather than
                // commands, so the modal engine never sees them as actions.
                if matches!(before, VimMode::Insert | VimMode::Replace)
                    && self.mode == ModeId::Codex
                {
                    match key.key.as_str() {
                        "Enter" => {
                            self.editor.insert_newline();
                            changed = true;
                        }
                        "Backspace" => changed = self.editor.backspace(),
                        _ => {}
                    }
                }

                for outcome in self.vim.keys(&key) {
                    let (evts, did) = self.events_for(outcome);
                    events.extend(evts);
                    changed |= did;
                }

                // The mode transition is reported after whatever the key did,
                // so a statusline applying events in order ends up correct.
                if self.vim.mode() != before {
                    // Leaving insert mode steps the cursor back one, as vim
                    // does: in insert you sit *between* characters, in normal
                    // you sit *on* one, and the character you land on is the
                    // one you just typed.
                    if matches!(before, VimMode::Insert | VimMode::Replace)
                        && self.vim.mode() == VimMode::Normal
                        && self.mode == ModeId::Codex
                    {
                        let buf = self.editor.buffer_mut();
                        buf.end_group();
                        let at = buf.cursor.offset;
                        let floor = buf.line_start();
                        buf.cursor =
                            buf.clamp(buf.cursor.moved_to(at.saturating_sub(1).max(floor)));
                    }
                    events.push(self.vim_event());
                }
                events.extend(self.after_action(changed));
                events
            }
        }
    }

    /* ── plumbing ─────────────────────────────────────────────────── */

    fn set_mode(&mut self, mode: ModeId) -> Vec<Event> {
        if mode == self.mode {
            return Vec::new();
        }
        self.mode = mode;
        vec![Event::ModeChanged { mode }]
    }

    /// Run an action against the active mode, reporting whether text changed.
    ///
    /// Only Codex edits text today; the other modes accept the action and
    /// report it, and gain their own nouns with their document types.
    fn act(&mut self, action: &Action) -> (Vec<Event>, bool) {
        let description = action.describe();
        if !self.mode.supports(action) {
            return (
                vec![Event::Unsupported {
                    description,
                    mode: self.mode,
                }],
                false,
            );
        }
        let changed = self.mode == ModeId::Codex && self.editor.execute(action, self.vim.mode());

        // An action that opens insert mode starts an undo group, so the typing
        // that follows is part of the same change: `ciwfoo<Esc>` is one thing
        // the user did and one press of `u` should take it back.
        //
        // This keys off the verb rather than off a mode transition, because by
        // the time an action reaches here the engine has *already* switched to
        // insert — the operator sets the mode as it resolves.
        if self.mode == ModeId::Codex
            && matches!(action.verb, Verb::Change | Verb::Insert)
            && self.vim.mode() == VimMode::Insert
        {
            // `c` and `o` edited before insert mode opened, and that edit is
            // part of the group; `i` and `a` had nothing to edit.
            self.editor.buffer_mut().begin_group(changed);
        }
        (
            vec![Event::Action {
                action: action.clone(),
                description,
            }],
            changed,
        )
    }

    fn events_for(&mut self, outcome: Outcome) -> (Vec<Event>, bool) {
        match outcome {
            Outcome::Action(action) => self.act(&action),
            Outcome::Input(c) => {
                if self.mode == ModeId::Codex {
                    self.editor.insert_char(c);
                    (Vec::new(), true)
                } else {
                    (
                        vec![Event::Input {
                            text: c.to_string(),
                        }],
                        false,
                    )
                }
            }
            Outcome::Pending(keys) => (vec![Event::Pending { keys }], false),
            Outcome::Leader(prefix) => (
                vec![Event::Leader {
                    prefix: Some(prefix),
                }],
                false,
            ),
            Outcome::Cmdline(text) => (vec![Event::Cmdline { text }], false),
            Outcome::Execute(text) => (vec![Event::CmdlineRun { text }], false),
            Outcome::Ignored => (Vec::new(), false),
        }
    }

    /// The redraw that follows anything which may have moved the cursor. A
    /// text change costs a viewport; a bare motion costs only a cursor, which
    /// is the difference between a responsive editor and a stuttering one.
    fn after_action(&mut self, text_changed: bool) -> Vec<Event> {
        let scrolled = self.scroll_to_cursor();
        let mut events = Vec::new();
        if text_changed {
            events.push(self.viewport_event());
            events.push(self.buffers_event());
        } else if scrolled {
            events.push(self.viewport_event());
        }
        events.push(self.cursor_event());
        events
    }

    /// Keep the cursor on screen with a line of margin, and say whether the
    /// window actually moved.
    fn scroll_to_cursor(&mut self) -> bool {
        let buf = self.editor.buffer();
        let line = buf.position(buf.cursor.offset).line;
        let height = self.viewport.lines;
        let margin = 1.min(height / 4);
        let before = self.viewport.first_line;

        if line < before + margin {
            self.viewport.first_line = line.saturating_sub(margin);
        } else if line + margin >= before + height {
            self.viewport.first_line = (line + margin + 1).saturating_sub(height);
        }
        self.viewport.first_line != before
    }

    fn viewport_event(&self) -> Event {
        let buf = self.editor.buffer();
        Event::Viewport {
            first_line: self.viewport.first_line,
            lines: buf.lines(self.viewport.first_line, self.viewport.lines),
            total_lines: buf.len_lines(),
            version: buf.version(),
        }
    }

    fn cursor_event(&self) -> Event {
        let buf = self.editor.buffer();
        let cursor = buf.cursor;
        let at = buf.position(cursor.offset);
        let selection = cursor.has_selection().then(|| {
            let anchor = buf.position(cursor.anchor);
            Selection {
                line: anchor.line,
                column: anchor.column,
                offset: cursor.anchor,
            }
        });
        Event::Cursor {
            line: at.line,
            column: at.column,
            offset: cursor.offset,
            selection,
        }
    }

    fn buffers_event(&self) -> Event {
        Event::Buffers {
            names: self
                .editor
                .buffers()
                .iter()
                .map(|b| BufferTab {
                    name: b.name(),
                    path: b.path().map(|p| p.display().to_string()),
                    modified: b.is_modified(),
                })
                .collect(),
            active: self.editor.active(),
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

fn notice(text: String, level: NoticeLevel) -> Event {
    Event::Notice { text, level }
}

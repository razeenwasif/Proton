//! Workspace state — the thing the shell is a view of.
//!
//! `apply` is deliberately total: one command in, a list of events out. The
//! only I/O is opening and saving a file, which is what a command asked for.
//! No clock, no threads, no windowing — that is what makes the core testable
//! headless and lets a second front end reuse it verbatim.

use std::path::Path;

use proton_lsp::client::Incoming;

use crate::action::{Action, Verb};
use crate::agent::Agent;
use crate::bus::{BufferTab, Command, Event, NoticeLevel, Selection, ServerState};
use crate::editor::Editor;
use crate::ex::Ex;
use crate::highlights::Highlighters;
use crate::key::Key;
use crate::lsp::{self, Popup, Servers, Severity};
use crate::mode::ModeId;
use crate::picker::{Accepted, PickerKey, PickerKind, Pickers};
use crate::trace::Visualizer;
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
    pickers: Pickers,
    servers: Servers,
    highlights: Highlighters,
    popup: Popup,
    agent: Agent,
    visualizer: Visualizer,
}

impl Workspace {
    pub fn new() -> Self {
        Self::rooted(".").with_language_servers()
    }

    /// The project root every picker searches. Separate from `new` so a test
    /// can point at a fixture instead of the process's working directory.
    pub fn rooted(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            editor: Editor::new(),
            pickers: Pickers::new(root),
            servers: Servers::new(root),
            agent: Agent::new(&root.display().to_string()),
            visualizer: Visualizer::new(root),
            ..Default::default()
        }
    }

    /// Opt in to language servers.
    ///
    /// Off by default and on for the shell, because otherwise every test that
    /// opens a `.rs` file spawns rust-analyzer — and every core test opens
    /// one. Making it explicit here is cheaper than making it conditional in
    /// eighty tests.
    pub fn with_language_servers(mut self) -> Self {
        self.enable_language_servers();
        self
    }

    /// The same, for a workspace that is already in a collection and cannot be
    /// taken by value — the session builds them into a `Vec`.
    pub fn enable_language_servers(&mut self) {
        self.servers.enable();
    }

    /// Tell a workspace how tall the window is without moving it.
    ///
    /// The height is a property of the window and the scroll position is a
    /// property of the workspace, and a newly focused workspace needs the first
    /// without losing the second. Sending it a whole `Viewport` would carry
    /// another project's scroll across with it.
    pub fn set_height(&mut self, lines: usize) {
        self.viewport.lines = lines.max(1);
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

    pub fn pickers(&self) -> &Pickers {
        &self.pickers
    }

    /// How many steps the loaded trace has. For the statusline and for
    /// anything driving the scrubber.
    pub fn visualizer_steps(&self) -> usize {
        self.visualizer.steps()
    }

    pub fn servers(&self) -> &Servers {
        &self.servers
    }

    /// The folder this workspace is a view of.
    pub fn root(&self) -> &Path {
        self.pickers.root()
    }

    /// Whether anything in this workspace has unsaved changes. The session
    /// asks before closing it, because a closed workspace takes its buffers
    /// with it and there is no undo for that.
    pub fn is_modified(&self) -> bool {
        self.editor.buffers().iter().any(|b| b.is_modified())
    }

    /// Abandon a half-typed leader sequence.
    ///
    /// The session calls this when it claims a prefix the workspace did not
    /// resolve — `␣fp` is the session's, and without this the workspace would
    /// still be waiting for the rest of a sequence that already happened.
    pub fn cancel_leader(&mut self) {
        self.vim.cancel_leader();
    }

    pub fn apply(&mut self, command: Command) -> Vec<Event> {
        match command {
            Command::Sync => {
                // The front end connecting is the signal that the window is
                // up, which is the right moment to start walking the tree: not
                // during construction, where it would be behind the first
                // paint, and not on first `␣ff`, where it would be in front of
                // the user.
                self.pickers.warm();
                vec![
                    self.state_event(),
                    self.buffers_event(),
                    self.viewport_event(),
                    self.cursor_event(),
                    self.diagnostics_event(),
                    self.servers_event(),
                    self.agent_event(),
                    self.trace_event(),
                ]
            }

            Command::Trace => self.trace_buffer(),

            Command::TraceSeek { step } => self.trace_seek(step),

            Command::TraceStep { by } => {
                let to = (self.visualizer.step_index() as i64 + by).max(0) as usize;
                self.trace_seek(to)
            }

            Command::Ask { text } => self.ask(&text),

            Command::CancelTurn => {
                self.agent.cancel();
                vec![self.agent_event()]
            }

            Command::Poll => self.poll_servers(),

            Command::Complete => self.request_completion(),

            Command::GotoDefinition => self.goto_definition(),

            Command::AcceptCompletion => self.accept_completion(),

            Command::CloseCompletion => {
                self.popup.close();
                vec![Event::CompletionClosed]
            }

            Command::SelectCompletion { index } => {
                self.popup.select(index);
                vec![self.completion_event()]
            }

            Command::SetMode { mode } => self.set_mode(mode),

            Command::Perform { action } => {
                let mode = self.vim.mode();
                let (mut events, changed) = self.act(&action, mode);
                events.extend(self.after_action(changed));
                if changed {
                    self.resync();
                }
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
                    // Diagnostics are per-file, so switching buffers changes
                    // which set the gutter should be drawing.
                    self.diagnostics_event(),
                ]
            }

            // Relative paths resolve against the project root, not the
            // process's working directory. The front end should not have to
            // know where the binary happened to be launched from, and a path
            // from the picker has to mean the same thing as one from the tree.
            Command::OpenBuffer { path } => self.open_path(&path),

            // Through the same door `:w` uses, so the overwrite guard cannot be
            // walked around by whichever of the two the front end happens to
            // send.
            Command::Save => self.write_buffer(false),

            // Switching projects is the session's job, and the session takes
            // these before a workspace ever sees one. A workspace reached
            // directly — a test, or a second front end that has not built a
            // session yet — says so rather than ignoring it.
            Command::OpenWorkspace { .. }
            | Command::FocusWorkspace { .. }
            | Command::CloseWorkspace { .. }
            | Command::CycleWorkspace { .. }
            | Command::OpenWorkspacePicker => vec![notice(
                "workspaces are the session's, not this workspace's".into(),
                NoticeLevel::Warn,
            )],

            Command::OpenPicker { kind } => self.open_picker(kind),

            Command::SelectPick { index } => match self.pickers.open_mut() {
                Some(picker) => {
                    picker.select(index);
                    vec![self.picker_event()]
                }
                None => Vec::new(),
            },

            // While a picker is open it owns the keyboard, so the modal engine
            // is suspended rather than consulted: `d` types a `d` instead of
            // starting an operator.
            Command::Key { key } if self.pickers.is_open() => self.picker_key(&key),

            Command::Key { key } => {
                // The completion popup gets first refusal on the keys that
                // mean something to it, and passes the rest through so typing
                // keeps narrowing the list. Unlike the picker it does not take
                // the whole keyboard — you are still in insert mode, and most
                // keys still belong to the buffer.
                if self.popup.is_open() {
                    if let Some(events) = self.completion_key(&key) {
                        return events;
                    }
                }

                let before = self.vim.mode();
                let typed = key.as_char().filter(|_| key.is_bare_char());
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
                    let (evts, did) = self.events_for(outcome, before);
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
                        // A block insert types on one line and lands on all of
                        // them, and this is the moment it lands: before the
                        // group closes, so the whole rectangle is one undo.
                        changed |= self.editor.finish_block_insert();

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
                if changed {
                    self.resync();
                }
                events.extend(self.completion_after(typed));
                events
            }
        }
    }

    /* ── telescope ────────────────────────────────────────────────── */

    fn buffer_labels(&self) -> Vec<(String, Option<String>)> {
        self.editor
            .buffers()
            .iter()
            .map(|b| (b.name(), b.path().map(|p| p.display().to_string())))
            .collect()
    }

    fn open_picker(&mut self, kind: PickerKind) -> Vec<Event> {
        let labels = self.buffer_labels();
        self.pickers.begin(kind, labels);
        // Close which-key on the way: the leader sequence resolved.
        vec![Event::Leader { prefix: None }, self.picker_event()]
    }

    fn picker_key(&mut self, key: &Key) -> Vec<Event> {
        let labels = self.buffer_labels();
        match PickerKey::of(key) {
            PickerKey::Cancel => {
                self.pickers.close();
                return vec![Event::PickerClosed];
            }
            PickerKey::Accept => return self.accept_picker(),
            PickerKey::Move(by) => self.move_selection(by),
            PickerKey::Backspace => {
                // Backspace on an empty query closes the picker, the way it
                // closes the command line.
                if !self.pickers.pop(labels) {
                    return vec![Event::PickerClosed];
                }
            }
            PickerKey::Type(c) => self.pickers.push(c, labels),
            PickerKey::Ignored => return Vec::new(),
        }
        vec![self.picker_event()]
    }

    fn move_selection(&mut self, delta: i32) {
        if let Some(picker) = self.pickers.open_mut() {
            picker.move_by(delta);
        }
    }

    fn accept_picker(&mut self) -> Vec<Event> {
        let accepted = self
            .pickers
            .open()
            .map(|p| p.accept())
            .unwrap_or(Accepted::Nothing);
        self.pickers.close();
        let mut events = vec![Event::PickerClosed];

        match accepted {
            Accepted::OpenPath { path, line, column } => {
                let full = self.pickers.root().join(&path);
                match self.editor.open(&full) {
                    Ok(_) => {
                        // A grep hit knows where it came from; jump there
                        // rather than dropping the user at the top of the file.
                        if let Some(line) = line {
                            let buf = self.editor.buffer_mut();
                            let at = buf.offset_of(proton_text::Position {
                                line,
                                column: column.unwrap_or(0),
                            });
                            buf.cursor = buf.clamp(buf.cursor.moved_to(at));
                        }
                        events.push(self.buffers_event());
                        events.extend(self.after_action(true));
                    }
                    Err(err) => events.push(notice(format!("{path}: {err}"), NoticeLevel::Error)),
                }
            }
            Accepted::FocusBuffer { index } => {
                if self.editor.focus(index) {
                    events.push(self.buffers_event());
                    events.extend(self.after_action(true));
                }
            }
            // A workspace's own pickers never produce this — the folder picker
            // belongs to the session, which handles its own accept.
            Accepted::OpenWorkspace { .. } | Accepted::Nothing => {}
        }
        events
    }

    fn picker_event(&self) -> Event {
        let Some(picker) = self.pickers.open() else {
            return Event::PickerClosed;
        };
        Event::Picker {
            kind: picker.kind(),
            title: picker.kind().title().to_string(),
            query: picker.query().to_string(),
            items: picker.items().to_vec(),
            selected: picker.selected(),
            total: match picker.kind() {
                PickerKind::Buffers => self.editor.buffers().len(),
                _ => self.pickers.file_count(),
            },
            elapsed_ms: picker.elapsed_ms(),
            scanning: self.pickers.is_scanning(),
        }
    }

    /* ── ex commands ──────────────────────────────────────────────── */

    /// Run a `:` line.
    ///
    /// Anything this does not recognise comes back out as `CmdlineRun` for the
    /// front end, which is where `:vsplit` and `:q` still live — splits are
    /// view state and the core has no opinion about them yet. The same channel
    /// carries the *residue* of a compound command: `:wq` writes here and
    /// hands on the `q`, so one line can be half core and half chrome without
    /// either half knowing about the other.
    fn run_ex(&mut self, line: String) -> Vec<Event> {
        let Some(ex) = Ex::parse(&line) else {
            return vec![Event::CmdlineRun { text: line }];
        };

        // Minimum abbreviation lengths are vim's. `:w` is write and `:sav` is
        // saveas, because `:s` is substitute.
        if ex.is("write", 1) {
            return self.ex_write(&ex);
        }
        if ex.is("saveas", 3) {
            return self.ex_saveas(&ex);
        }
        if ex.is("edit", 1) {
            return self.ex_edit(&ex);
        }
        if ex.name == "wq" {
            let mut events = self.ex_write(&ex);
            // Only close if the write actually happened — `:wq` on a file that
            // changed underneath you must not close the window on the way to
            // losing the work.
            if !refused(&events) {
                events.push(Event::CmdlineRun { text: "q".into() });
            }
            return events;
        }
        if ex.is("xit", 1) {
            // `:x` writes only when there is something to write, which is the
            // whole difference from `:wq`: it leaves mtimes alone, and build
            // systems watching the file stay quiet.
            let mut events = if self.editor.buffer().is_modified() {
                self.ex_write(&ex)
            } else {
                Vec::new()
            };
            if !refused(&events) {
                events.push(Event::CmdlineRun { text: "q".into() });
            }
            return events;
        }

        vec![Event::CmdlineRun { text: line }]
    }

    /// `:w`, `:w!`, `:w {file}`.
    fn ex_write(&mut self, ex: &Ex) -> Vec<Event> {
        let Some(arg) = &ex.arg else {
            return self.write_buffer(ex.bang);
        };
        let path = self.pickers.resolve(arg);

        // `:w {file}` on a buffer with no name of its own adopts the name —
        // vim does this too, and the alternative is writing a file you then
        // have no way to keep editing.
        if self.editor.buffer().path().is_none() {
            self.editor.buffer_mut().rename(path);
            return self.write_buffer(true);
        }

        // Otherwise it is a copy, not a rename: the buffer keeps its own name
        // and stays modified. `:saveas` is the one that moves house.
        match self.editor.buffer().write_to(&path) {
            Ok(()) => vec![notice(
                format!("wrote a copy to {}", path.display()),
                NoticeLevel::Info,
            )],
            Err(err) => vec![notice(
                format!("{}: {err}", path.display()),
                NoticeLevel::Error,
            )],
        }
    }

    /// `:saveas {file}` — write it there and keep editing it there.
    fn ex_saveas(&mut self, ex: &Ex) -> Vec<Event> {
        let Some(arg) = &ex.arg else {
            return vec![notice(
                ":saveas needs a file name".into(),
                NoticeLevel::Warn,
            )];
        };
        self.editor.buffer_mut().rename(self.pickers.resolve(arg));
        // Forced, because the guard is about the file this buffer came from
        // and it no longer came from there.
        let mut events = self.write_buffer(true);
        // The name in the tab bar changed, and so did the language, which is
        // what decides the grammar and which server sees the file.
        self.resync();
        events.push(self.viewport_event());
        events
    }

    /// `:e`, `:e!`, `:e {file}`.
    fn ex_edit(&mut self, ex: &Ex) -> Vec<Event> {
        if let Some(arg) = &ex.arg {
            let path = self.pickers.resolve(arg);
            if path.is_dir() {
                // Vim would open a directory listing. Proton opens folders as
                // projects, which is a different verb and lives elsewhere.
                return vec![notice(
                    format!("{} is a folder — ␣fp opens folders", path.display()),
                    NoticeLevel::Warn,
                )];
            }
            return self.open_path(arg);
        }

        // No argument: re-read this file from disk.
        if self.editor.buffer().path().is_none() {
            return vec![notice(
                "no file to re-read — :e {file} opens one".into(),
                NoticeLevel::Warn,
            )];
        }
        if !ex.bang && self.editor.buffer().is_modified() {
            return vec![notice(
                format!(
                    "{} has unsaved changes — :e! to discard them",
                    self.editor.buffer().name()
                ),
                NoticeLevel::Warn,
            )];
        }

        match self.editor.buffer_mut().reload() {
            Ok(()) => {
                let name = self.editor.buffer().name();
                self.resync();
                let mut events = vec![
                    notice(format!("re-read {name}"), NoticeLevel::Info),
                    self.buffers_event(),
                    self.viewport_event(),
                    self.diagnostics_event(),
                ];
                events.push(self.cursor_event());
                events
            }
            Err(err) => vec![notice(err.to_string(), NoticeLevel::Error)],
        }
    }

    /// Write the active buffer to its own file.
    ///
    /// `force` is `:w!`. Without it a write refuses when the file changed
    /// underneath us — formatters, language servers and `git checkout` rewrite
    /// files under an open editor constantly, and silently discarding that is
    /// the one data-loss bug an editor must not have.
    fn write_buffer(&mut self, force: bool) -> Vec<Event> {
        if !force && self.editor.buffer().changed_on_disk() {
            return vec![notice(
                format!(
                    "{} changed on disk since it was read — :w! to overwrite, \
                     :e! to discard yours",
                    self.editor.buffer().name()
                ),
                NoticeLevel::Warn,
            )];
        }
        match self.editor.save() {
            Ok(()) => {
                let name = self.editor.buffer().name();
                // Some servers only run the expensive checks on save —
                // `cargo check` diagnostics do not exist until then.
                self.servers.saved(self.editor.buffer());
                vec![
                    notice(format!("wrote {name}"), NoticeLevel::Info),
                    self.buffers_event(),
                ]
            }
            Err(err) => vec![notice(err.to_string(), NoticeLevel::Error)],
        }
    }

    /// Open a file into a buffer, or focus it if it is already open.
    ///
    /// Relative paths resolve against the project root, not the process's
    /// working directory. The front end should not have to know where the
    /// binary happened to be launched from, and a path from the picker has to
    /// mean the same thing as one typed into `:e`.
    fn open_path(&mut self, path: &str) -> Vec<Event> {
        match self.editor.open(self.pickers.resolve(path)) {
            Ok(_) => {
                self.resync();
                vec![
                    self.buffers_event(),
                    self.viewport_event(),
                    self.cursor_event(),
                    self.diagnostics_event(),
                ]
            }
            Err(err) => vec![notice(format!("{path}: {err}"), NoticeLevel::Error)],
        }
    }

    /* ── language servers ─────────────────────────────────────────── */

    /// Tell whichever server owns the active buffer what it now says.
    fn resync(&mut self) {
        if self.mode == ModeId::Codex {
            self.servers.sync(self.editor.buffer());
        }
    }

    /// Drain the mailboxes and turn what is in them into events.
    ///
    /// This is the only place the core learns anything it was not asked about,
    /// and it still learns it synchronously, when something outside chooses to
    /// ask. `apply` stays total.
    fn poll_servers(&mut self) -> Vec<Event> {
        let mut events = Vec::new();

        // The file walk lands here too. It is the same shape of problem — work
        // finished on a thread the core cannot see — so it uses the same
        // heartbeat rather than inventing a second one.
        let labels = self.buffer_labels();
        if self.pickers.poll(labels) {
            events.push(self.picker_event());
        }

        // The agent runs on the same heartbeat. It has to: a turn takes tens
        // of seconds and arrives in pieces, and the core has no clock of its
        // own to notice them with.
        if self.agent.is_busy() {
            events.extend(self.poll_agent());
        }

        // A trace is seconds of compiling and stepping, so it arrives the same
        // way a language server reply does.
        if self.visualizer.poll() {
            events.push(self.trace_event());
            events.extend(self.follow_trace());
        }

        let messages = self.servers.poll();
        if messages.is_empty() {
            return events;
        }

        let mut diagnostics_moved = false;
        let mut status_moved = false;

        for (_, message) in messages {
            match message {
                Incoming::Diagnostics { .. } => diagnostics_moved = true,
                Incoming::Ready { .. } | Incoming::Progress { .. } | Incoming::Exited => {
                    status_moved = true
                }
                Incoming::Completion { id, items } => {
                    // `fill` refuses a reply to a question we stopped asking,
                    // which is most of them: we re-ask on every keystroke.
                    if self.popup.fill(id, items) {
                        events.push(self.completion_event());
                    }
                }
                Incoming::Definition { id, target } => {
                    if self.servers.wanted_definition(id) {
                        events.extend(self.jump_to(target));
                    }
                }
                // A server saying no is worth showing once; a server logging
                // its own startup is not.
                Incoming::Failed { text } => events.push(notice(text, NoticeLevel::Warn)),
                Incoming::Log { .. } => {}
            }
        }

        if diagnostics_moved {
            events.push(self.diagnostics_event());
        }
        if status_moved {
            events.push(self.servers_event());
        }
        events
    }

    fn request_completion(&mut self) -> Vec<Event> {
        let buffer = self.editor.buffer();
        let Some(path) = buffer.path().map(Path::to_path_buf) else {
            return Vec::new();
        };
        let at = lsp::position_of(buffer, buffer.cursor.offset);
        if let Some(id) = self.servers.completion(&path, at) {
            // Nothing is shown yet. The popup appears when the reply lands,
            // which keeps a slow server from being a stuttering one.
            self.popup.expect(id);
        }
        Vec::new()
    }

    /// Ask again after a keystroke, if there is any reason to.
    ///
    /// A `.` opens the popup; word characters keep it fed; anything else
    /// closes it. The alternative — asking on every character — makes a
    /// language server the busiest thing in the process for no visible gain.
    fn completion_after(&mut self, typed: Option<char>) -> Vec<Event> {
        if self.mode != ModeId::Codex || self.vim.mode() != VimMode::Insert {
            return Vec::new();
        }
        let Some(c) = typed else {
            return Vec::new();
        };
        let trigger = c == '.' || c == ':';
        let feeding = self.popup.is_open() && (c.is_alphanumeric() || c == '_');
        if trigger || feeding {
            return self.request_completion();
        }
        if self.popup.is_open() {
            self.popup.close();
            return vec![Event::CompletionClosed];
        }
        Vec::new()
    }

    /// While the popup is showing it takes the keys that mean something to it
    /// and passes on the rest, so typing carries on narrowing the list.
    /// `None` means "not mine".
    fn completion_key(&mut self, key: &Key) -> Option<Vec<Event>> {
        let moved = |ws: &mut Self, delta: i32| {
            ws.popup.move_by(delta);
            Some(vec![ws.completion_event()])
        };
        match key.key.as_str() {
            "Tab" | "ArrowDown" => moved(self, 1),
            "ArrowUp" => moved(self, -1),
            "n" if key.ctrl => moved(self, 1),
            "p" if key.ctrl => moved(self, -1),
            "Enter" => Some(self.accept_completion()),
            // Escape dismisses the popup and *stays* in insert mode. Leaving
            // insert as well would punish anyone who wanted to keep typing.
            "Escape" => {
                self.popup.close();
                Some(vec![Event::CompletionClosed])
            }
            // Backspace belongs to the buffer, but the list it was built from
            // no longer describes what is there.
            "Backspace" => {
                self.popup.close();
                None
            }
            _ => None,
        }
    }

    fn accept_completion(&mut self) -> Vec<Event> {
        let Some(item) = self.popup.chosen().cloned() else {
            return Vec::new();
        };
        self.popup.close();

        lsp::apply_completion(self.editor.buffer_mut(), &item);

        let mut events = vec![Event::CompletionClosed];
        self.resync();
        events.extend(self.after_action(true));
        events
    }

    fn goto_definition(&mut self) -> Vec<Event> {
        let buffer = self.editor.buffer();
        let Some(path) = buffer.path().map(Path::to_path_buf) else {
            return vec![notice("no file to look in".into(), NoticeLevel::Warn)];
        };
        let at = lsp::position_of(buffer, buffer.cursor.offset);
        match self.servers.definition(&path, at) {
            Some(_) => Vec::new(),
            None => vec![notice(
                "no language server for this file".into(),
                NoticeLevel::Warn,
            )],
        }
    }

    fn jump_to(&mut self, target: Option<proton_lsp::Jump>) -> Vec<Event> {
        let Some(jump) = target else {
            return vec![notice("no definition found".into(), NoticeLevel::Info)];
        };

        let here = self.editor.buffer().path().map(proton_lsp::uri::plain);
        if here.as_deref() != Some(jump.path.as_path()) {
            if let Err(err) = self.editor.open(&jump.path) {
                return vec![notice(
                    format!("{}: {err}", jump.path.display()),
                    NoticeLevel::Error,
                )];
            }
            self.resync();
        }

        let buffer = self.editor.buffer_mut();
        let at = buffer.offset_at_utf16(jump.line as usize, jump.character as usize);
        buffer.cursor = buffer.clamp(buffer.cursor.moved_to(at));
        // `true` because the view may be looking at a different buffer now,
        // not because the text changed.
        self.after_action(true)
    }

    fn completion_event(&self) -> Event {
        if !self.popup.is_open() {
            return Event::CompletionClosed;
        }
        Event::Completion {
            items: self.popup.items().to_vec(),
            selected: self.popup.selected(),
        }
    }

    fn diagnostics_event(&self) -> Event {
        let buffer = self.editor.buffer();
        let items: Vec<_> = buffer
            .path()
            .map(|path| self.servers.diagnostics_for(path))
            .unwrap_or_default()
            .iter()
            .map(|raw| lsp::diagnostic(buffer, raw))
            .collect();
        Event::Diagnostics {
            path: buffer.path().map(|p| p.display().to_string()),
            errors: items
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .count(),
            warnings: items
                .iter()
                .filter(|d| d.severity == Severity::Warning)
                .count(),
            items,
        }
    }

    fn servers_event(&self) -> Event {
        Event::Servers {
            states: self
                .servers
                .status()
                .into_iter()
                .map(|(language, status, note)| ServerState {
                    language: language.to_string(),
                    status,
                    note,
                })
                .collect(),
        }
    }

    /* ── the agent ────────────────────────────────────────────────── */

    /// Put a question to the agent.
    fn ask(&mut self, text: &str) -> Vec<Event> {
        if text.trim().is_empty() {
            return Vec::new();
        }
        match self.agent.ask(text) {
            Ok(()) => vec![self.agent_event()],
            // A missing credential is the common case and it is not an error
            // in the code — it is a thing the user has to go and do.
            Err(why) => vec![notice(why, NoticeLevel::Warn)],
        }
    }

    /// Drain the agent, run whatever it asked for, and continue the turn.
    ///
    /// The tools run here rather than in `proton-agent` because this is where
    /// the editor is: `grep` is the same index Telescope searches, and
    /// `read_file` reads through the same buffers the user has open, so what
    /// the agent sees and what the user sees cannot drift apart.
    fn poll_agent(&mut self) -> Vec<Event> {
        let calls = self.agent.poll();
        let mut events = Vec::new();

        for call in calls {
            let answer = self.run_tool(&call.name, &call.input);
            // An `open_buffer` that succeeded changed what is on screen.
            if call.name == "open_buffer" && answer.is_ok() {
                events.push(self.buffers_event());
                events.extend(self.after_action(true));
            }
            self.agent.answer(&call.id, answer);
        }

        // Every outstanding call answered? Then the turn continues, with all
        // the results in one message.
        match self.agent.resume() {
            Ok(_) => {}
            Err(why) => events.push(notice(why, NoticeLevel::Error)),
        }

        events.push(self.agent_event());
        events
    }

    /// Run one tool against the workspace.
    fn run_tool(&mut self, name: &str, input: &serde_json::Value) -> Result<String, String> {
        let text = |key: &str| {
            input
                .get(key)
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .ok_or_else(|| format!("{name}: `{key}` is required and must be a string"))
        };
        let count = |key: &str, fallback: usize| {
            input
                .get(key)
                .and_then(|v| v.as_u64())
                .map(|n| n as usize)
                .unwrap_or(fallback)
        };

        match name {
            "read_file" => {
                let path = text("path")?;
                let buffer = proton_text::Buffer::open(self.pickers.resolve(&path))
                    .map_err(|err| format!("{path}: {err}"))?;

                // 1-based on the wire, because that is what the model sees in
                // the gutter and in every error message it has ever read.
                let from = count("from", 1).max(1) - 1;
                let to = count("to", buffer.len_lines()).min(buffer.len_lines());
                let lines = buffer.lines(from, to.saturating_sub(from));

                Ok(lines
                    .iter()
                    .enumerate()
                    .map(|(i, line)| format!("{:>5}  {line}", from + i + 1))
                    .collect::<Vec<_>>()
                    .join("\n"))
            }

            "grep" => {
                let query = text("query")?;
                let hits = self.pickers.grep(&query, count("limit", 40));
                if hits.is_empty() {
                    return Ok(format!("no matches for {query:?}"));
                }
                Ok(hits.join("\n"))
            }

            "find_files" => {
                let query = input.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let found = self.pickers.find(query, count("limit", 40));
                if found.is_empty() {
                    return Ok(format!("no files matching {query:?}"));
                }
                Ok(found.join("\n"))
            }

            "open_buffer" => {
                let path = text("path")?;
                self.editor
                    .open(self.pickers.resolve(&path))
                    .map_err(|err| format!("{path}: {err}"))?;
                self.resync();
                Ok(format!("opened {path}"))
            }

            "diagnostics" => {
                let path = match input.get("path").and_then(|v| v.as_str()) {
                    Some(path) => self.pickers.resolve(path),
                    None => match self.editor.buffer().path() {
                        Some(path) => path.to_path_buf(),
                        None => return Ok("no file is open".into()),
                    },
                };
                let raw = self.servers.diagnostics_for(&path).to_vec();
                if raw.is_empty() {
                    return Ok("no diagnostics".into());
                }
                Ok(raw
                    .iter()
                    .map(|d| {
                        format!(
                            "{}:{} {:?} {}",
                            d.range.start.line + 1,
                            d.range.start.character + 1,
                            lsp::Severity::from_lsp(d.severity),
                            d.message
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }

            other => Err(format!("no such tool: {other}")),
        }
    }

    fn agent_event(&self) -> Event {
        let usage = self.agent.usage();
        Event::Agent {
            transcript: self.agent.transcript().to_vec(),
            activity: self.agent.activity(),
            // The cache number earns its place in the UI: it is the only
            // visible sign that the prefix is still being hit, and a silent
            // miss is a cost regression rather than an error.
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cached_tokens: usage.cache_read_input_tokens,
        }
    }

    /* ── the visualizer ───────────────────────────────────────────── */

    /// Trace the active buffer.
    fn trace_buffer(&mut self) -> Vec<Event> {
        let buffer = self.editor.buffer();
        let Some(path) = buffer.path().map(Path::to_path_buf) else {
            return vec![notice(
                "save the file before tracing it".into(),
                NoticeLevel::Warn,
            )];
        };
        // The buffer's text, not the file's: tracing what is on disk while the
        // editor shows something else is the kind of wrong that wastes an hour.
        let source = buffer.contents();

        match self.visualizer.trace(&path, &source) {
            Ok(()) => vec![self.trace_event()],
            // Not an error in the code — a missing toolchain or an
            // unsupported language is something the user has to go and fix,
            // and the tracer's own reason is better than anything we'd invent.
            Err(why) => vec![notice(why, NoticeLevel::Warn)],
        }
    }

    /// Move to a step, and take the editor's cursor with it.
    fn trace_seek(&mut self, to: usize) -> Vec<Event> {
        if !self.visualizer.seek(to) {
            return Vec::new();
        }
        let mut events = vec![self.trace_event()];
        events.extend(self.follow_trace());
        events
    }

    /// Put the cursor on the line the current step is about to run.
    ///
    /// This is the half that makes it part of the editor rather than a panel
    /// beside it: scrubbing the trace moves the buffer, so the code and the
    /// memory are never describing different moments.
    fn follow_trace(&mut self) -> Vec<Event> {
        let Some(line) = self.visualizer.current_line() else {
            return Vec::new();
        };
        // Trace lines are 1-based, as every compiler and every gutter says.
        let line = (line.saturating_sub(1)) as usize;
        let buffer = self.editor.buffer_mut();
        if line >= buffer.len_lines() {
            return Vec::new();
        }
        let at = buffer.offset_of(proton_text::Position { line, column: 0 });
        buffer.cursor = buffer.clamp(buffer.cursor.moved_to(at));
        self.after_action(false)
    }

    fn trace_event(&self) -> Event {
        Event::Trace {
            state: self.visualizer.state(),
            subject: self.visualizer.subject().to_string(),
            message: self.visualizer.message().to_string(),
            step: self.visualizer.step_index(),
            steps: self.visualizer.steps(),
            view: self.visualizer.view(),
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
    fn act(&mut self, action: &Action, was: VimMode) -> (Vec<Event>, bool) {
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
        // `gd` is a verb the engine resolves like any other, but the answer
        // comes from a language server rather than from this buffer, so it is
        // routed rather than executed.
        if action.verb == Verb::Goto {
            let mut events = vec![Event::Action {
                action: action.clone(),
                description,
            }];
            events.extend(self.goto_definition());
            return (events, false);
        }

        let changed = self.mode == ModeId::Codex && self.editor.execute(action, was);

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

    /// `was` is the modal mode the key arrived in, not the one the engine is
    /// in now. The two differ exactly when it matters: an operator in visual
    /// mode switches back to normal *as* it resolves, so asking the engine
    /// afterwards is asking too late to find out it was a block.
    fn events_for(&mut self, outcome: Outcome, was: VimMode) -> (Vec<Event>, bool) {
        match outcome {
            Outcome::Action(action) => self.act(&action, was),
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
            // A leader sequence that names a picker opens it here rather than
            // in the front end: the keymap is core config, and a second front
            // end must not have to reinvent it.
            Outcome::Leader(prefix) if PickerKind::for_leader(&prefix).is_some() => (
                self.open_picker(PickerKind::for_leader(&prefix).unwrap()),
                false,
            ),
            // `␣ct` traces the buffer. Upstream binds Space to play/pause and
            // the arrows to stepping; Proton's leader *is* Space, so the
            // pane's own keys stay scoped to the pane and the verbs that
            // belong to the editor live under the leader like everything else.
            Outcome::Leader(prefix) if prefix == "ct" => {
                let mut events = vec![Event::Leader { prefix: None }];
                events.extend(self.trace_buffer());
                (events, false)
            }
            Outcome::Leader(prefix) => (
                vec![Event::Leader {
                    prefix: Some(prefix),
                }],
                false,
            ),
            Outcome::Cmdline(text) => (vec![Event::Cmdline { text }], false),
            // The core runs what it owns; whatever it does not recognise comes
            // back out as `CmdlineRun` for the front end, which still owns
            // splits. See `run_ex`.
            Outcome::Execute(text) => (self.run_ex(text), false),
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

    fn viewport_event(&mut self) -> Event {
        let first_line = self.viewport.first_line;
        let count = self.viewport.lines;
        // Highlighting is computed here rather than in a second event,
        // because the colours and the text they colour must never be a frame
        // apart — one stale and the other fresh is visibly wrong.
        let spans = self
            .highlights
            .spans(self.editor.buffer(), first_line, count);
        let buf = self.editor.buffer();
        Event::Viewport {
            first_line,
            lines: buf.lines(first_line, count),
            total_lines: buf.len_lines(),
            version: buf.version(),
            highlights: spans,
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

pub(crate) fn notice(text: String, level: NoticeLevel) -> Event {
    Event::Notice { text, level }
}

/// Whether a command declined to do the thing it was asked.
///
/// A refusal is a warning or an error and nothing else, which is what lets
/// `:wq` tell "wrote it" from "would not write it" without either half of the
/// compound knowing how the other reports itself.
fn refused(events: &[Event]) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            Event::Notice {
                level: NoticeLevel::Warn | NoticeLevel::Error,
                ..
            }
        )
    })
}

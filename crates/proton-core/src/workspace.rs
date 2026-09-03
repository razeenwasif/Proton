//! Workspace state — the thing the shell is a view of.
//!
//! `apply` is deliberately total: one command in, a list of events out. The
//! only I/O is opening and saving a file, which is what a command asked for.
//! No clock, no threads, no windowing — that is what makes the core testable
//! headless and lets a second front end reuse it verbatim.

use std::path::{Path, PathBuf};

use proton_lsp::client::Incoming;

use crate::action::{Action, Object, Verb};
use crate::agent::Agent;
use crate::bus::{
    BufferTab, Command, DebugFrame, DebugPhase, DebugVariable, Event, Hit, NoticeLevel, Selection,
    ServerState, SplitView, Stop, TermRun,
};
use crate::debug::{self, Debugger};
use crate::editor::Editor;
use crate::ex::Ex;
use crate::git::Git;
use crate::highlights::Highlighters;
use crate::key::Key;
use crate::lsp::{self, Popup, Servers, Severity};
use crate::mode::ModeId;
use crate::picker::{Accepted, PickerKey, PickerKind, Pickers};
use crate::split::{Direction, Splits};
use crate::trace::Visualizer;
use crate::tree::Tree;
use crate::vim::{Outcome, Vim, VimMode};

/// What a formatting reply will be checked against when it arrives.
///
/// Formatting is the first thing here that both *asks* a server a question
/// and *edits* on the answer, so it is the first that can be stale in a way
/// that corrupts rather than merely disappoints: edits computed against one
/// version of a file, applied to another, land in the wrong places. The file
/// and the version are what make that checkable.
#[derive(Debug)]
struct Formatting {
    path: PathBuf,
    version: u64,
    /// A `:w` is waiting on this, so the file is written again once the edits
    /// land. False for `:fmt`, which formats without saving.
    then_write: bool,
}

#[derive(Debug, Default)]
pub struct Workspace {
    mode: ModeId,
    vim: Vim,
    editor: Editor,
    /// The views onto the buffers. Always at least one; what used to be a
    /// single viewport is the focused one of these.
    splits: Splits,
    pickers: Pickers,
    servers: Servers,
    highlights: Highlighters,
    popup: Popup,
    agent: Agent,
    visualizer: Visualizer,
    tree: Tree,
    /// The gutter, and what git has to say about the project.
    git: Git,
    /// Breakpoints, and the debug session when there is one.
    debugger: Debugger,
    /// The terminal, once something has asked for one. Not started on boot:
    /// a shell nobody opened is a process nobody wanted.
    terminal: Option<proton_term::Terminal>,
    /// How big the panel is. Kept even with no terminal, so the one opened
    /// next starts the right size rather than at a default nobody chose.
    term_size: (u16, u16),
    /// How many lines the active buffer had before the last change, so a
    /// breakpoint can be carried along by however many were added or removed.
    lines_before: usize,
    /// Whether a hover panel is showing. Only a flag: the text belongs to the
    /// front end once it has been sent, and what the core needs to know is
    /// when to take it away.
    hover_open: bool,
    /// A formatting request in flight.
    formatting: Option<Formatting>,
    /// The code actions the picker is showing, in its order. Held because a
    /// picker row carries a label, not an action — and the action is what has
    /// to be applied or resolved when a row is chosen.
    actions: Vec<proton_lsp::CodeAction>,
    /// The title of the action waiting on `codeAction/resolve`, so the notice
    /// its edit produces can say which one it was.
    pending_action: Option<String>,
    /// Where the cursor was when a search prompt opened.
    ///
    /// Every keystroke previews from here rather than from the last preview,
    /// and abandoning the line puts the cursor back.
    search_origin: Option<proton_text::Cursor>,
    /// The last search, for `n` and `N`.
    ///
    /// Document state rather than modal state: `n` means "again", and only
    /// this layer knows what again was.
    search: Option<Search>,
    /// Whether matches are painted. `:noh` turns this off without forgetting
    /// the pattern, which is the distinction people use it for — the highlight
    /// has served its purpose and `n` still has to work.
    highlight: bool,
}

/// A search, remembered.
#[derive(Debug, Clone)]
struct Search {
    pattern: String,
    /// Which way it was going. `N` is the opposite of *this* rather than of
    /// forwards — after `?foo`, `n` keeps going backwards.
    forward: bool,
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
            tree: Tree::new(root),
            git: Git::discover(root),
            debugger: Debugger::new(root),
            term_size: (24, 80),
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
        self.splits.view_mut().lines = lines.max(1);
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
                let mut events = vec![
                    self.state_event(),
                    self.splits_event(),
                    self.buffers_event(),
                ];
                // Every split, not just the focused one — on connect the front
                // end has nothing, and an unfocused pane with no content in it
                // is a blank rectangle.
                events.extend(self.every_view());
                events.extend([
                    self.debug_event(),
                    self.diagnostics_event(),
                    self.servers_event(),
                    self.agent_event(),
                    self.trace_event(),
                    self.tree_event(),
                ]);
                events
            }

            Command::PlaceCursor { line, column } => {
                let buffer = self.editor.buffer_mut();
                // Clamped twice over: the line against the document, then the
                // offset against the line, because a click past the end of a
                // short line is the ordinary case rather than an error.
                let line = line.min(buffer.len_lines().saturating_sub(1));
                let at = buffer.offset_of(proton_text::Position { line, column });
                buffer.cursor = buffer.clamp(buffer.cursor.moved_to(at));
                // The mode is left alone deliberately. A click in visual mode
                // extends the selection, which is what vim's mouse does and
                // what anybody dragging a selection means.
                self.after_action(false)
            }

            Command::SelectTo { line, column } => {
                let was = self.vim.mode();
                self.vim.enter_visual();
                let buffer = self.editor.buffer_mut();
                let line = line.min(buffer.len_lines().saturating_sub(1));
                let at = buffer.offset_of(proton_text::Position { line, column });
                // The head moves and the anchor stays, which is the whole of
                // what a visual selection is.
                buffer.cursor = buffer.clamp(buffer.cursor.extended_to(at));

                let mut events = self.after_action(false);
                if self.vim.mode() != was {
                    events.push(self.vim_event());
                }
                events
            }

            Command::OpenTerminal => {
                if self.terminal.is_none() {
                    let (rows, cols) = self.term_size;
                    match proton_term::Terminal::open(self.pickers.root(), rows, cols) {
                        Ok(term) => self.terminal = Some(term),
                        Err(err) => {
                            return vec![notice(format!("no terminal: {err}"), NoticeLevel::Error)]
                        }
                    }
                }
                vec![self.terminal_event()]
            }

            Command::TerminalInput { text } => {
                if let Some(term) = self.terminal.as_mut() {
                    term.write(&text);
                }
                // Nothing is drawn from here: what the shell makes of it comes
                // back on the next poll, which is the only thing that knows.
                Vec::new()
            }

            Command::TerminalSize { rows, cols } => {
                self.term_size = (rows.max(1), cols.max(1));
                match self.terminal.as_mut() {
                    Some(term) => {
                        term.resize(rows, cols);
                        vec![self.terminal_event()]
                    }
                    None => Vec::new(),
                }
            }

            Command::CloseTerminal => {
                // Dropping it kills the shell, which is what closing a
                // terminal means.
                self.terminal = None;
                vec![self.terminal_event()]
            }

            Command::ToggleBreakpoint { line } => {
                let Some(path) = self.editor.buffer().path().map(Path::to_path_buf) else {
                    return vec![notice(
                        "this buffer is not a file yet".into(),
                        NoticeLevel::Warn,
                    )];
                };
                let buffer = self.editor.buffer();
                // Lines are 1-based everywhere a debugger speaks, and 0-based
                // everywhere the buffer does. The conversion lives here so
                // only one place has to remember.
                let line = line.unwrap_or(buffer.position(buffer.cursor.offset).line) + 1;
                let set = self.debugger.toggle(&path, line);
                let mut events = vec![notice(
                    format!("breakpoint {} line {line}", if set { "on" } else { "off" }),
                    NoticeLevel::Info,
                )];
                events.extend(self.every_view());
                events
            }

            Command::DebugStart => match self.debugger.start() {
                Ok(()) => {
                    let mut events = vec![notice("debugging".into(), NoticeLevel::Info)];
                    events.push(self.debug_event());
                    events
                }
                Err(why) => vec![notice(why, NoticeLevel::Error)],
            },

            Command::DebugStop => {
                self.debugger.stop();
                let mut events = vec![self.debug_event()];
                events.extend(self.every_view());
                events
            }

            Command::DebugResume => {
                self.debugger.resume();
                let mut events = vec![self.debug_event()];
                events.extend(self.every_view());
                events
            }

            Command::DebugStepOver => {
                self.debugger.step_over();
                Vec::new()
            }
            Command::DebugStepInto => {
                self.debugger.step_into();
                Vec::new()
            }
            Command::DebugStepOut => {
                self.debugger.step_out();
                Vec::new()
            }

            Command::ToggleTreeNode { path } => {
                if !self.tree.toggle(&path) {
                    return vec![notice(format!("{path}: not a folder"), NoticeLevel::Warn)];
                }
                vec![self.tree_event()]
            }

            Command::RevealInTree => {
                let Some(path) = self.editor.buffer().path().map(Path::to_path_buf) else {
                    return vec![notice(
                        "this buffer is not a file yet".into(),
                        NoticeLevel::Warn,
                    )];
                };
                self.tree.reveal(&path);
                vec![self.tree_event()]
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

            Command::Hover => self.request_hover(),

            Command::CloseHover => self.close_hover(),

            Command::FindReferences => self.find_references(),

            Command::Format => self.request_format(false),

            Command::PromptRename => self.prompt_rename(),

            Command::Rename { to } => self.request_rename(&to),

            Command::CodeActions => self.request_code_actions(),

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

            Command::Viewport {
                view,
                first_line,
                lines,
            } => {
                let Some(target) = self.splits.at_mut(view) else {
                    // A split that has just closed can still have a scroll
                    // event in flight. Not an error, and not worth a notice.
                    return Vec::new();
                };
                target.first_line = first_line;
                target.lines = lines.max(1);
                vec![self.viewport_of(view)]
            }

            Command::Split { vertical } => {
                // The cursor goes with the copy, so the new split opens where
                // the old one was looking.
                self.park_cursor();
                if self.splits.split(vertical).is_none() {
                    self.restore_cursor();
                    return vec![notice(
                        "four splits is as many as are worth having".into(),
                        NoticeLevel::Warn,
                    )];
                }
                self.entered_split()
            }

            Command::CloseSplit { index } => {
                let index = index.unwrap_or(self.splits.focused());
                self.park_cursor();
                if self.splits.close(index).is_none() {
                    self.restore_cursor();
                    return vec![notice(
                        "that is the last split — :q closes the buffer".into(),
                        NoticeLevel::Warn,
                    )];
                }
                self.entered_split()
            }

            Command::OnlySplit => {
                self.park_cursor();
                self.splits.only();
                self.entered_split()
            }

            Command::FocusSplit { index } => {
                if index == self.splits.focused() {
                    return Vec::new();
                }
                self.park_cursor();
                if !self.splits.focus(index) {
                    self.restore_cursor();
                    return vec![notice(format!("no split {index}"), NoticeLevel::Warn)];
                }
                self.entered_split()
            }

            Command::StepSplit { direction } => {
                self.park_cursor();
                if self.splits.step(direction).is_none() {
                    // Nothing that way. Put the cursor back and say nothing —
                    // walking into the edge is not an error.
                    self.restore_cursor();
                    return Vec::new();
                }
                self.entered_split()
            }

            Command::FocusBuffer { index } => {
                if !self.editor.focus(index) {
                    return vec![notice(format!("no buffer {index}"), NoticeLevel::Warn)];
                }
                self.switched()
            }

            // Relative paths resolve against the project root, not the
            // process's working directory. The front end should not have to
            // know where the binary happened to be launched from, and a path
            // from the picker has to mean the same thing as one from the tree.
            // A click on a tab's `×` closes whichever tab was clicked, which
            // need not be the one you are in — so it is focused first and then
            // closed through the one path that knows how to refuse.
            Command::CloseBuffer { index, force } => {
                if index >= self.editor.buffers().len() {
                    return vec![notice(format!("no buffer {index}"), NoticeLevel::Warn)];
                }
                let was = self.editor.active();
                self.editor.focus(index);
                let events = self.ex_quit(&Ex {
                    name: "q".into(),
                    bang: force,
                    arg: None,
                });
                if refused(&events) {
                    // Put the user back where they were rather than leaving
                    // them in a buffer they only pointed at.
                    self.editor.focus(was);
                }
                events
            }

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
        // Asked for as the picker opens rather than kept up to date: a status
        // walk is not free, and nothing else on screen needs it.
        if kind == PickerKind::Changed {
            if !self.git.is_repo() {
                return vec![notice("not a git repository".into(), NoticeLevel::Warn)];
            }
            let changes = self
                .git
                .changes()
                .into_iter()
                .map(|c| (c.path, c.state.letter()))
                .collect();
            self.pickers.set_changes(changes);
        }
        // `␣ls` asks here rather than at the keymap, so the request and the
        // picker that shows its answer cannot get out of step. `gr` has
        // already asked by the time it calls this — its question needs a
        // cursor position, which the picker is about to take the keyboard
        // away from.
        if kind == PickerKind::Symbols {
            let path = self.editor.buffer().path().map(Path::to_path_buf);
            match path.as_deref().and_then(|p| self.servers.symbols(p)) {
                Some(_) => {}
                None => {
                    return vec![notice(
                        "no language server for this file".into(),
                        NoticeLevel::Warn,
                    )]
                }
            }
        }
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
            Accepted::RunAction { index } => events.extend(self.chose_action(index)),
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

    /* ── search ───────────────────────────────────────────────────── */

    /// Follow the search as it is typed.
    ///
    /// Every keystroke searches from where the cursor was when the prompt
    /// opened — never from where the last preview left it, or each character
    /// would jump to the match after the one before and the cursor would walk
    /// away down the file as you typed.
    fn preview_search(&mut self, pattern: &str, forward: bool) -> Vec<Event> {
        let origin = *self
            .search_origin
            .get_or_insert(self.editor.buffer().cursor);

        // Nothing typed yet, or nothing found: sit where you started rather
        // than somewhere a prefix of the pattern happened to match.
        let landing = if pattern.is_empty() {
            None
        } else {
            self.editor.buffer().find(pattern, origin.offset, forward)
        };

        let selecting = self.vim.is_selecting();
        let buffer = self.editor.buffer_mut();
        buffer.cursor = match landing {
            Some(at) if selecting => buffer.clamp(origin.extended_to(at)),
            Some(at) => buffer.clamp(origin.moved_to(at)),
            None => buffer.clamp(origin),
        };
        // The viewport goes too, because the matches painted on it are the
        // pattern being typed. That is the whole point of an incremental
        // search: you see where you would land before committing to it.
        let mut events = self.after_action(false);
        events.push(self.viewport_event());
        events
    }

    /// Put the cursor back where the search started. `⎋` out of `/foo`.
    fn abandon_search(&mut self) -> Vec<Event> {
        let Some(origin) = self.search_origin.take() else {
            return Vec::new();
        };
        let buffer = self.editor.buffer_mut();
        buffer.cursor = buffer.clamp(origin);
        let mut events = self.after_action(false);
        // The highlight went with the prompt.
        events.push(self.viewport_event());
        events
    }

    /// Run a search and remember it, so `n` has something to repeat.
    ///
    /// The pattern belongs to the document rather than to the modal engine —
    /// `n` means "again" and only this layer knows what again was.
    fn search(&mut self, pattern: &str, forward: bool) -> Vec<Event> {
        // Undo the preview before committing. Typing the pattern has already
        // moved the cursor to the first match, and searching from *there*
        // would land on the second one.
        if let Some(origin) = self.search_origin.take() {
            let buffer = self.editor.buffer_mut();
            buffer.cursor = buffer.clamp(origin);
        }
        if pattern.is_empty() {
            // An empty `/` in vim repeats the last search rather than
            // matching nothing, which is what anybody pressing `/` then Enter
            // by accident wants.
            return self.search_again(forward);
        }
        self.search = Some(Search {
            pattern: pattern.to_string(),
            forward,
        });
        self.jump_to_match(pattern, forward)
    }

    fn search_again(&mut self, forward: bool) -> Vec<Event> {
        let Some(search) = self.search.clone() else {
            return vec![notice("no previous search".into(), NoticeLevel::Warn)];
        };
        // `N` is the opposite of whichever direction the search was going, not
        // of "forwards" — after `?foo`, `n` keeps going backwards.
        let forward = if forward {
            search.forward
        } else {
            !search.forward
        };
        self.jump_to_match(&search.pattern, forward)
    }

    fn jump_to_match(&mut self, pattern: &str, forward: bool) -> Vec<Event> {
        let buffer = self.editor.buffer();
        let Some(at) = buffer.find(pattern, buffer.cursor.offset, forward) else {
            return vec![notice(format!("{pattern} not found"), NoticeLevel::Warn)];
        };

        let wrapped = if forward {
            at <= buffer.cursor.offset
        } else {
            at >= buffer.cursor.offset
        };
        // Extends rather than moves in visual mode: a search is a motion, so
        // `v/foo` selects up to the match exactly as `v}` selects to the
        // paragraph. Keyed on the mode rather than on there already being a
        // selection, because `v` on its own leaves the anchor and the head in
        // the same place and there is nothing yet to notice.
        let visual = self.vim.is_selecting();
        let buffer = self.editor.buffer_mut();
        buffer.cursor = buffer.clamp(if visual {
            buffer.cursor.extended_to(at)
        } else {
            buffer.cursor.moved_to(at)
        });

        let mut events = Vec::new();
        if wrapped {
            // Silently wrapping looks like the search went the wrong way.
            events.push(notice(
                format!(
                    "search hit {} and continued from the {}",
                    if forward { "the bottom" } else { "the top" },
                    if forward { "top" } else { "bottom" }
                ),
                NoticeLevel::Info,
            ));
        }
        // Landing on a match paints them again, whatever `:noh` said about the
        // last search — you asked to be shown where they are.
        self.highlight = true;
        events.extend(self.after_action(false));
        // The pattern changed, so the matches on screen did. `after_action`
        // only redraws the text when the window moved, which is not the same
        // question.
        events.push(self.viewport_event());
        events
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
        // Splits are the core's now. `:vs` is vim's minimum for `:vsplit`, and
        // `:sp` for `:split` — `:s` is substitute.
        if ex.is("vsplit", 2) || ex.is("vnew", 2) {
            return self.apply(Command::Split { vertical: true });
        }
        if ex.is("split", 2) || ex.is("new", 3) {
            return self.apply(Command::Split { vertical: false });
        }
        if ex.is("only", 2) {
            return self.apply(Command::OnlySplit);
        }
        if ex.is("close", 3) {
            return self.apply(Command::CloseSplit { index: None });
        }
        if ex.is("terminal", 4) {
            return self.apply(Command::OpenTerminal);
        }
        // Spelled out rather than abbreviated: `:fo` is `:fold` in vim and
        // `:f` is `:file`, so there is no short form going spare.
        if ex.name == "fmt" || ex.name == "format" {
            return self.apply(Command::Format);
        }
        // `:ren` is vim's `:rename`… for a *file*. Ours renames a symbol, so
        // it is spelled out rather than abbreviated into that collision.
        if ex.name == "rename" {
            return match &ex.arg {
                Some(to) => self.apply(Command::Rename { to: to.clone() }),
                None => vec![notice("rename to what?".into(), NoticeLevel::Warn)],
            };
        }
        if ex.is("nohlsearch", 3) {
            // Vim's `:noh`. The pattern is kept — `n` still works — and only
            // the painting stops, which is exactly the distinction people use
            // it for.
            self.highlight = false;
            return vec![self.viewport_event()];
        }
        if ex.is("quit", 1) || ex.is("bdelete", 2) {
            return self.ex_quit(&ex);
        }
        // No overlap with `:bdelete` — `bn` and `bp` are not prefixes of it,
        // and `bd` is not a prefix of these — so the order here is only about
        // reading, not about precedence.
        if ex.is("bnext", 2) {
            return self.ex_buffer(1);
        }
        if ex.is("bprevious", 2) || ex.is("bNext", 2) {
            return self.ex_buffer(-1);
        }
        if ex.name == "wq" {
            let mut events = self.ex_write(&ex);
            // Only close if the write actually happened. `:wq` on a file that
            // changed underneath you must not take the buffer with it on the
            // way to losing the work.
            if !refused(&events) {
                // Forced, because the write is what the bang was about and the
                // buffer is unmodified now anyway.
                events.extend(self.ex_quit(&Ex {
                    name: "q".into(),
                    bang: true,
                    arg: None,
                }));
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
                events.extend(self.ex_quit(&Ex {
                    name: "q".into(),
                    bang: true,
                    arg: None,
                }));
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

    /// `:q`, `:q!`, and `:bd` under its own name.
    ///
    /// With splits open it closes the split, the way vim's does. With one
    /// split it closes the *buffer* — vim would quit, and an editor whose `:q`
    /// sometimes closes the whole window is an editor you lose work in. The
    /// tab bar is what buffers look like here, and a tab you cannot close is
    /// the more obvious missing thing. `:bd` always means the buffer.
    fn ex_quit(&mut self, ex: &Ex) -> Vec<Event> {
        if self.splits.len() > 1 && !ex.is("bdelete", 2) {
            return self.apply(Command::CloseSplit { index: None });
        }
        if !ex.bang && self.editor.buffer().is_modified() {
            return vec![notice(
                format!(
                    "{} has unsaved changes — :wq to write, :q! to discard them",
                    self.editor.buffer().name()
                ),
                NoticeLevel::Warn,
            )];
        }

        let name = self.editor.buffer().name();
        // Before it goes: the server is holding a copy of a file nobody is
        // editing any more, and diagnostics for it are still in the gutter's
        // reach.
        self.servers.closed(self.editor.buffer());
        let closed = self.editor.active();
        self.editor.close(closed);
        // Every index above the closed one just shifted down, and a split
        // still pointing at the old number would draw the wrong file — or
        // past the end of the list.
        self.splits
            .buffer_closed(closed, self.editor.buffers().len());
        // The index is about to be reused by whatever opens next, and stale
        // marks on the wrong file are worse than none.
        self.git.forget_all();

        // The buffer you land on is a different file, so everything that is
        // per-file is now wrong on screen.
        self.resync();
        let mut events = vec![notice(format!("closed {name}"), NoticeLevel::Info)];
        events.extend(self.switched());
        events
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
                // A save changes what git thinks of the file on disk, so the
                // blob it was compared against has to be read again.
                self.git.forget(self.editor.active());
                let mut events = vec![
                    notice(format!("wrote {name}"), NoticeLevel::Info),
                    self.buffers_event(),
                ];
                events.extend(self.request_format(true));
                events
            }
            Err(err) => vec![notice(err.to_string(), NoticeLevel::Error)],
        }
    }

    /// Ask the server to format the active buffer.
    ///
    /// **`:w` writes first and formats second.** The core has no clock, so it
    /// cannot wait for a reply with a timeout the way an editor with a
    /// blocking save does; and a `:w` that might not write, because a server
    /// went quiet, is worse than a file written twice. So the file goes to
    /// disk as it is, the question goes out, and if the answer changes
    /// anything the file is written again. The visible cost is that a watcher
    /// sees two writes for a save that reformats.
    fn request_format(&mut self, then_write: bool) -> Vec<Event> {
        let Some(path) = self.editor.buffer().path().map(Path::to_path_buf) else {
            return match then_write {
                true => Vec::new(),
                false => vec![notice("no file to format".into(), NoticeLevel::Warn)],
            };
        };
        // The server has to be looking at the text we are asking about, or it
        // formats the version before the edit that prompted the save.
        self.resync();

        if self.servers.format(&path).is_none() {
            self.formatting = None;
            return match then_write {
                // Saving a file whose language has no formatter is the
                // ordinary case, not something to say anything about.
                true => Vec::new(),
                false => vec![notice(
                    "no formatter for this file".into(),
                    NoticeLevel::Warn,
                )],
            };
        }
        self.formatting = Some(Formatting {
            path,
            version: self.editor.buffer().version(),
            then_write,
        });
        Vec::new()
    }

    /// Put a server's formatting edits in the buffer, if they still fit.
    fn formatted(&mut self, edits: &[proton_lsp::TextEdit]) -> Vec<Event> {
        let Some(pending) = self.formatting.take() else {
            return Vec::new();
        };
        let here = self.editor.buffer().path().map(proton_lsp::uri::plain);
        if here.as_deref() != Some(pending.path.as_path()) {
            // Switched files while it was thinking. Silent: the save that
            // asked already happened and said so.
            return Vec::new();
        }
        if self.editor.buffer().version() != pending.version {
            return vec![notice(
                "kept your edits — the file changed while it was being formatted".into(),
                NoticeLevel::Info,
            )];
        }
        if !lsp::apply_edits(self.editor.buffer_mut(), edits) {
            return Vec::new();
        }

        let mut events = vec![notice(
            format!("formatted {}", self.editor.buffer().name()),
            NoticeLevel::Info,
        )];
        if pending.then_write {
            // Forced: we wrote this file a moment ago, so the copy on disk
            // being newer than the one we read is us.
            if let Err(err) = self.editor.save() {
                events.push(notice(err.to_string(), NoticeLevel::Error));
            } else {
                self.servers.saved(self.editor.buffer());
                self.git.forget(self.editor.active());
            }
        }
        self.resync();
        events.push(self.buffers_event());
        events.extend(self.after_action(true));
        events
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
                self.switched()
            }
            Err(err) => vec![notice(format!("{path}: {err}"), NoticeLevel::Error)],
        }
    }

    /// Everything that is per-file, after the active buffer changed.
    ///
    /// Diagnostics because the gutter was drawing another file's; the tree
    /// because it marks where you are, and a tree that cannot say that is half
    /// a tree.
    fn switched(&mut self) -> Vec<Event> {
        // The focused view is showing whatever the editor just made active.
        // Without this the view keeps pointing at the buffer it had, and the
        // pane draws a file nobody asked for.
        self.splits.view_mut().buffer = self.editor.active();
        // And the line count is now this buffer's, so the next edit measures
        // against the right number. Left at the old one, the first edit after
        // opening a file would shift its breakpoints by the whole difference.
        self.lines_before = self.editor.buffer().len_lines();
        vec![
            self.buffers_event(),
            self.viewport_event(),
            self.cursor_event(),
            self.diagnostics_event(),
            self.tree_event(),
        ]
    }

    /// `:bn` and `:bp` — the next or previous buffer, wrapping.
    fn ex_buffer(&mut self, by: i64) -> Vec<Event> {
        let count = self.editor.buffers().len() as i64;
        if count < 2 {
            return vec![notice("only one buffer is open".into(), NoticeLevel::Warn)];
        }
        let next = (self.editor.active() as i64 + by).rem_euclid(count) as usize;
        self.editor.focus(next);
        let mut events = vec![notice(self.editor.buffer().name(), NoticeLevel::Info)];
        events.extend(self.switched());
        events
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

        // The shell is on the same heartbeat as everything else that happens
        // on a thread the core cannot see.
        if self.terminal.as_mut().is_some_and(|t| t.poll()) {
            events.push(self.terminal_event());
        }

        // The debugger is on the same heartbeat as everything else that
        // happens on a thread the core cannot see.
        if self.debugger.poll() {
            events.push(self.debug_event());
            // Where it stopped is drawn in the gutter, so the text goes too.
            events.extend(self.every_view());
        }

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
                    if self.servers.wanted(lsp::Ask::Definition, id) {
                        events.extend(self.jump_to(target));
                    }
                }
                Incoming::Hover { id, text } => {
                    if self.servers.wanted(lsp::Ask::Hover, id) {
                        events.extend(self.show_hover(text));
                    }
                }
                Incoming::References { id, places } => {
                    if self.servers.wanted(lsp::Ask::References, id) {
                        let rows = lsp::reference_rows(&places, self.pickers.root());
                        events.extend(self.fill_picker(rows));
                    }
                }
                Incoming::Formatted { id, edits } => {
                    if self.servers.wanted(lsp::Ask::Format, id) {
                        events.extend(self.formatted(&edits));
                    }
                }
                Incoming::Renamed { id, changes } => {
                    if self.servers.wanted(lsp::Ask::Rename, id) {
                        events.extend(self.apply_workspace_edit(&changes, "rename"));
                    }
                }
                Incoming::CodeActions { id, actions } => {
                    if self.servers.wanted(lsp::Ask::CodeActions, id) {
                        self.actions = actions;
                        let rows = self.action_rows();
                        events.extend(self.fill_picker(rows));
                    }
                }
                Incoming::Resolved { id, action } => {
                    if self.servers.wanted(lsp::Ask::ResolveAction, id) {
                        let title = self
                            .pending_action
                            .take()
                            .unwrap_or_else(|| action.title.clone());
                        match action.edit.clone() {
                            Some(edit) => {
                                events.extend(self.apply_workspace_edit(&edit.resolve(), &title))
                            }
                            // Resolved, and still no edit. The server had
                            // nothing beyond a command to run.
                            None => events.push(notice(
                                format!("\"{title}\" needs a server command Proton cannot run"),
                                NoticeLevel::Warn,
                            )),
                        }
                    }
                }
                Incoming::Symbols { id, symbols } => {
                    if self.servers.wanted(lsp::Ask::Symbols, id) {
                        let rows = self.symbol_rows(&symbols);
                        events.extend(self.fill_picker(rows));
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

    /// `K`. Nothing is shown until the answer arrives — a panel that opens
    /// empty and fills a moment later reads as a flicker, and the answer to
    /// "what is this" is often "nothing", which deserves no panel at all.
    fn request_hover(&mut self) -> Vec<Event> {
        let buffer = self.editor.buffer();
        let Some(path) = buffer.path().map(Path::to_path_buf) else {
            return vec![notice("no file to look in".into(), NoticeLevel::Warn)];
        };
        let at = lsp::position_of(buffer, buffer.cursor.offset);
        match self.servers.hover(&path, at) {
            Some(_) => Vec::new(),
            None => vec![notice(
                "no language server for this file".into(),
                NoticeLevel::Warn,
            )],
        }
    }

    fn close_hover(&mut self) -> Vec<Event> {
        // Forgetting the request as well as the panel: a reply still in
        // flight would otherwise reopen what was just dismissed.
        self.servers.forget(lsp::Ask::Hover);
        if !self.hover_open {
            return Vec::new();
        }
        self.hover_open = false;
        vec![Event::HoverClosed]
    }

    fn show_hover(&mut self, text: String) -> Vec<Event> {
        if text.trim().is_empty() {
            return self.close_hover();
        }
        let buffer = self.editor.buffer();
        let at = buffer.position(buffer.cursor.offset);
        self.hover_open = true;
        vec![Event::Hover {
            text,
            line: at.line,
            column: at.column,
        }]
    }

    /// `gr`. The picker opens straight away and fills when the server
    /// answers, so the keystroke has an effect before the round trip does.
    fn find_references(&mut self) -> Vec<Event> {
        let buffer = self.editor.buffer();
        let Some(path) = buffer.path().map(Path::to_path_buf) else {
            return vec![notice("no file to look in".into(), NoticeLevel::Warn)];
        };
        let at = lsp::position_of(buffer, buffer.cursor.offset);
        if self.servers.references(&path, at).is_none() {
            return vec![notice(
                "no language server for this file".into(),
                NoticeLevel::Warn,
            )];
        }
        self.open_picker(PickerKind::References)
    }

    /// The symbol list, as rows.
    ///
    /// Nesting becomes context rather than indentation: `len` under `Buffer`
    /// reads as "len · Buffer" instead of as an indented `len`, which keeps
    /// the label a bare name for the matcher to work on — a query that had to
    /// match through leading spaces would find nothing.
    fn symbol_rows(&self, symbols: &[proton_lsp::Symbol]) -> Vec<crate::picker::PickerItem> {
        let buffer = self.editor.buffer();
        let here = buffer.path().map(|p| p.to_string_lossy().into_owned());
        let mut parents: Vec<&str> = Vec::new();
        let mut rows = Vec::new();

        for symbol in symbols {
            parents.truncate(symbol.depth);
            let context = parents.join("::");
            parents.push(&symbol.name);

            let detail = match (context.is_empty(), symbol.detail.as_deref()) {
                (true, detail) => detail.unwrap_or_default().to_string(),
                (false, Some(detail)) => format!("{context}  {detail}"),
                (false, None) => context,
            };

            // A document symbol names no file because it is in this one; the
            // flat shape says which, and for a document it says this one too.
            let path = symbol
                .uri
                .as_deref()
                .and_then(proton_lsp::uri::to_path)
                .map(|p| p.to_string_lossy().into_owned())
                .or_else(|| here.clone());

            // Converted against the live buffer rather than the file on
            // disk: the symbols describe what is on screen, unsaved edits
            // included.
            let at = buffer.offset_at_utf16(symbol.line as usize, symbol.character as usize);
            let position = buffer.position(at);

            rows.push(crate::picker::PickerItem {
                label: symbol.name.clone(),
                detail,
                positions: Vec::new(),
                path,
                line: Some(symbol.line as usize),
                column: Some(position.column),
                kind: Some(lsp::symbol_kind(symbol.kind)),
            });
        }
        rows
    }

    /// `␣lr` — open the `:` line holding the name the symbol has now.
    fn prompt_rename(&mut self) -> Vec<Event> {
        let buffer = self.editor.buffer();
        if buffer.path().is_none() {
            return vec![notice("no file to rename in".into(), NoticeLevel::Warn)];
        }
        let Some(word) = buffer.word_at(buffer.cursor.offset) else {
            return vec![notice(
                "no symbol under the cursor".into(),
                NoticeLevel::Warn,
            )];
        };
        // No `prepareRename` first. It would tell us whether the symbol can be
        // renamed and what its name is, at the cost of a round trip before the
        // prompt opens — and the buffer already knows the name. A symbol the
        // server refuses to rename says so when the rename is asked for, which
        // is one message later and no less clear.
        let line = self.vim.open_cmdline(format!("rename {word}"));
        vec![
            Event::Leader { prefix: None },
            Event::Cmdline {
                text: Some(line),
                prompt: ":".into(),
            },
        ]
    }

    fn request_rename(&mut self, to: &str) -> Vec<Event> {
        let to = to.trim();
        if to.is_empty() {
            return vec![notice("rename to what?".into(), NoticeLevel::Warn)];
        }
        let buffer = self.editor.buffer();
        let Some(path) = buffer.path().map(Path::to_path_buf) else {
            return vec![notice("no file to rename in".into(), NoticeLevel::Warn)];
        };
        let at = lsp::position_of(buffer, buffer.cursor.offset);
        self.resync();
        match self.servers.rename(&path, at, to) {
            Some(_) => Vec::new(),
            None => vec![notice(
                "no language server that renames for this file".into(),
                NoticeLevel::Warn,
            )],
        }
    }

    /// `␣la` — what the server offers for the line the cursor is on.
    fn request_code_actions(&mut self) -> Vec<Event> {
        let buffer = self.editor.buffer();
        let Some(path) = buffer.path().map(Path::to_path_buf) else {
            return vec![notice("no file to act on".into(), NoticeLevel::Warn)];
        };
        let line = buffer.position(buffer.cursor.offset).line as u32;
        let range = proton_lsp::Range {
            start: proton_lsp::Position { line, character: 0 },
            end: proton_lsp::Position {
                line,
                character: u32::MAX,
            },
        };
        // The diagnostics on that line go with the request. A quick fix is a
        // fix *for a diagnostic*, and a server handed none has nothing to
        // offer one for — ask with an empty context on a line with an error
        // and you get refactors and no fixes.
        let here: Vec<serde_json::Value> = self
            .servers
            .diagnostics_for(&path)
            .iter()
            .filter(|d| d.range.start.line <= line && line <= d.range.end.line)
            .map(|d| {
                serde_json::json!({
                    "range": d.range,
                    "severity": d.severity,
                    "source": d.source,
                    "message": d.message,
                })
            })
            .collect();

        self.resync();
        if self.servers.code_actions(&path, range, &here).is_none() {
            return vec![notice(
                "no language server with actions for this file".into(),
                NoticeLevel::Warn,
            )];
        }
        self.open_picker(PickerKind::Actions)
    }

    /// The offered actions, as rows.
    ///
    /// One that cannot be applied is still listed, marked rather than hidden:
    /// a menu that quietly drops half of what the server offered is one you
    /// cannot trust to be showing you everything.
    fn action_rows(&self) -> Vec<crate::picker::PickerItem> {
        self.actions
            .iter()
            .enumerate()
            .map(|(index, action)| crate::picker::PickerItem {
                label: action.title.clone(),
                detail: match action.is_applicable() {
                    true => action.kind.clone().unwrap_or_default(),
                    false => "needs a server command".into(),
                },
                positions: Vec::new(),
                path: None,
                // Addressed by position, like a buffer row: the thing chosen
                // is not a place.
                line: Some(index),
                column: None,
                kind: None,
            })
            .collect()
    }

    /// A chosen action: apply its edit, or ask the server to fill one in.
    fn chose_action(&mut self, index: usize) -> Vec<Event> {
        let Some(action) = self.actions.get(index).cloned() else {
            return Vec::new();
        };
        if let Some(edit) = action.edit.clone() {
            return self.apply_workspace_edit(&edit.resolve(), &action.title);
        }
        if action.data.is_some() {
            let path = self.editor.buffer().path().map(Path::to_path_buf);
            self.pending_action = Some(action.title.clone());
            if path
                .as_deref()
                .and_then(|p| self.servers.resolve_action(p, action.raw.clone()))
                .is_some()
            {
                return Vec::new();
            }
        }
        // A bare `Command`, or an action whose server has gone. Both need
        // `workspace/executeCommand`, which runs code inside the server rather
        // than editing text, and Proton does not run it.
        vec![notice(
            format!(
                "\"{}\" needs a server command Proton cannot run",
                action.title
            ),
            NoticeLevel::Warn,
        )]
    }

    /// Apply an edit that may touch several files.
    ///
    /// Public because it is the seam, not because a test needed it: a rename,
    /// a code action and — when M3's second half lands — an agent's staged
    /// diff are all "a set of edits across files, applied or refused as one".
    ///
    /// Three rules, all of them about not half-doing it.
    ///
    /// **File operations are refused outright.** Renaming a Rust module
    /// renames its file, and rust-analyzer sends that in the same list as the
    /// text edits. Creating, moving or deleting a file touches things that are
    /// not open, cannot be taken back with `u`, and may be under version
    /// control — so the whole edit is declined and named, rather than applied
    /// with the file half of it silently dropped.
    ///
    /// **Everything is checked before anything is applied.** A version that
    /// has moved on, or a file that will not open, refuses the lot. An edit
    /// applied to four files out of six is worse than one applied to none.
    ///
    /// **Nothing is written.** Every touched file is opened as a buffer and
    /// left modified, so the change is visible, `u` takes it back, and it
    /// reaches the disk only when you say so.
    pub fn apply_workspace_edit(
        &mut self,
        changes: &proton_lsp::Changes,
        what: &str,
    ) -> Vec<Event> {
        if changes.file_operations > 0 {
            return vec![notice(
                format!(
                    "\"{what}\" also creates, moves or deletes {} file{} — Proton does not do \
                     that yet, so nothing was changed",
                    changes.file_operations,
                    if changes.file_operations == 1 {
                        ""
                    } else {
                        "s"
                    },
                ),
                NoticeLevel::Warn,
            )];
        }
        if changes.is_empty() {
            return vec![notice(
                format!("{what}: nothing to change"),
                NoticeLevel::Info,
            )];
        }

        // Pass one: resolve and check. Nothing is touched here.
        let mut planned = Vec::new();
        for file in &changes.files {
            if file.edits.is_empty() {
                continue;
            }
            let Some(path) = proton_lsp::uri::to_path(&file.uri) else {
                return vec![notice(
                    format!("{what}: cannot read {}", file.uri),
                    NoticeLevel::Error,
                )];
            };
            if !self.servers.is_current(&path, file.version) {
                return vec![notice(
                    format!(
                        "{what}: {} changed while it was being worked out, so nothing \
                             was changed",
                        path.display()
                    ),
                    NoticeLevel::Warn,
                )];
            }
            if !path.exists() {
                return vec![notice(
                    format!(
                        "{what}: {} is not there, so nothing was changed",
                        path.display()
                    ),
                    NoticeLevel::Error,
                )];
            }
            planned.push((path, file.edits.clone()));
        }

        // Pass two: apply. The active buffer is restored afterwards, because
        // landing in the last file a rename happened to touch is not where
        // anybody was.
        let was = self.editor.active();
        let mut touched = 0;
        for (path, edits) in &planned {
            if self.editor.open(path).is_err() {
                continue;
            }
            if lsp::apply_edits(self.editor.buffer_mut(), edits) {
                touched += 1;
            }
        }
        self.editor.focus(was);
        self.splits.view_mut().buffer = self.editor.active();

        self.resync();
        let mut events = vec![notice(
            match touched {
                0 => format!("{what}: nothing to change"),
                1 => format!("{what} — 1 file changed, none written yet"),
                n => format!("{what} — {n} files changed, none written yet"),
            },
            NoticeLevel::Info,
        )];
        events.push(self.buffers_event());
        events.extend(self.after_action(true));
        events
    }

    /// Fill a server-fed picker, if it is still the one open.
    fn fill_picker(&mut self, items: Vec<crate::picker::PickerItem>) -> Vec<Event> {
        if self.pickers.set_held(items) {
            vec![self.picker_event()]
        } else {
            Vec::new()
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

    /* ── splits ───────────────────────────────────────────────────── */

    /// Put the focused view's cursor back on the view before the focus moves.
    ///
    /// The buffer holds the focused view's cursor, because that is where every
    /// operator reaches for it. The other views hold their own. These two
    /// exchange them, and they are the only place that has to know.
    fn park_cursor(&mut self) {
        let cursor = self.editor.buffer().cursor;
        let buffer = self.editor.active();
        let view = self.splits.view_mut();
        view.cursor = cursor;
        view.buffer = buffer;
    }

    /// The other half: make the newly focused view's cursor the live one.
    fn restore_cursor(&mut self) {
        let view = *self.splits.view();
        // The buffer it was showing may have closed while it was not looking.
        if !self.editor.focus(view.buffer) {
            self.editor.focus(0);
            self.splits.view_mut().buffer = self.editor.active();
        }
        let buffer = self.editor.buffer_mut();
        buffer.cursor = buffer.clamp(view.cursor);
    }

    /// Everything the front end has to redraw when the focused split changes.
    fn entered_split(&mut self) -> Vec<Event> {
        self.restore_cursor();
        self.resync();
        let mut events = vec![self.splits_event()];
        // Every split, because splitting and closing renumber them and the one
        // that was at index 1 may now be showing something else.
        events.extend(self.every_view());
        events.push(self.buffers_event());
        events.push(self.diagnostics_event());
        events.push(self.tree_event());
        events
    }

    /// The content and cursor of every split.
    fn every_view(&mut self) -> Vec<Event> {
        (0..self.splits.len())
            .flat_map(|view| [self.viewport_of(view), self.cursor_of(view)])
            .collect()
    }

    fn splits_event(&self) -> Event {
        Event::Splits {
            views: self
                .splits
                .views()
                .iter()
                .map(|view| SplitView {
                    buffer: view.buffer,
                    name: self
                        .editor
                        .buffers()
                        .get(view.buffer)
                        .map(|b| b.name())
                        .unwrap_or_default(),
                })
                .collect(),
            focused: self.splits.focused(),
            vertical: self.splits.is_vertical(),
        }
    }

    /// Redraw every split showing the buffer that just changed.
    ///
    /// Two views on one file must not disagree about what is in it, even for
    /// a frame — which is the whole reason a split is a view rather than a
    /// copy.
    fn redraw_others(&mut self) -> Vec<Event> {
        let buffer = self.editor.active();
        let focused = self.splits.focused();
        self.splits
            .showing(buffer)
            .into_iter()
            .filter(|&view| view != focused)
            .flat_map(|view| [self.viewport_of(view), self.cursor_of(view)])
            .collect()
    }

    /* ── the terminal ─────────────────────────────────────────────── */

    fn terminal_event(&mut self) -> Event {
        let Some(term) = self.terminal.as_mut() else {
            // Closed. An empty screen rather than no event at all, so the
            // panel clears instead of keeping a dead shell on it.
            return Event::Terminal {
                rows: Vec::new(),
                cursor: (0, 0),
                cursor_visible: false,
                running: false,
            };
        };
        let running = term.is_alive();
        let screen = term.screen();
        Event::Terminal {
            rows: screen
                .rows
                .iter()
                .map(|row| row.iter().map(term_run).collect())
                .collect(),
            cursor: screen.cursor,
            cursor_visible: screen.cursor_visible && running,
            running,
        }
    }

    /* ── debugging ────────────────────────────────────────────────── */

    fn debug_event(&self) -> Event {
        let root = self.pickers.root();
        Event::Debug {
            phase: match self.debugger.phase() {
                debug::Phase::Idle => DebugPhase::Idle,
                debug::Phase::Running => DebugPhase::Running,
                debug::Phase::Stopped => DebugPhase::Stopped,
            },
            frames: self
                .debugger
                .frames()
                .iter()
                .map(|frame| DebugFrame {
                    name: frame.name.clone(),
                    // Relative to the project when it is inside it: an
                    // absolute path in a stack is unreadable at a glance and
                    // the project is what the reader is holding in their head.
                    path: frame.path.as_ref().map(|path| {
                        path.strip_prefix(root)
                            .unwrap_or(path)
                            .to_string_lossy()
                            .replace('\\', "/")
                    }),
                    line: frame.line,
                })
                .collect(),
            variables: self
                .debugger
                .variables()
                .iter()
                .map(|variable| DebugVariable {
                    name: variable.name.clone(),
                    value: variable.value.clone(),
                    expandable: variable.children != 0,
                })
                .collect(),
            output: self.debugger.output().to_vec(),
        }
    }

    /// The tree as it stands.
    ///
    /// Recomputed from the disk each time rather than cached, which is what
    /// makes a file another process created show up without an invalidation
    /// rule to get wrong. It costs one directory read per *expanded* folder,
    /// so a closed tree costs one.
    fn tree_event(&self) -> Event {
        Event::Tree {
            rows: self.tree.rows(self.editor.buffer().path()),
        }
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
        // `gd`, `gr` and `K` are verbs the engine resolves like any other, but
        // their answers come from a language server rather than from this
        // buffer, so they are routed rather than executed.
        let asked = match (action.verb, action.object) {
            (Verb::Goto, Object::Definition) => Some(Workspace::goto_definition as fn(_) -> _),
            (Verb::Goto, Object::References) => Some(Workspace::find_references as fn(_) -> _),
            (Verb::Explain, _) => Some(Workspace::request_hover as fn(_) -> _),
            _ => None,
        };
        if let Some(ask) = asked {
            let mut events = vec![Event::Action {
                action: action.clone(),
                description,
            }];
            events.extend(ask(self));
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
            Outcome::Leader(prefix) if PickerKind::for_leader(&prefix).is_some() => {
                // Every arm below resolves the sequence, so the engine has to
                // be told: it accumulates until something claims a prefix, and
                // a second `␣db` would otherwise arrive as `db db`.
                self.vim.cancel_leader();
                (
                    self.open_picker(PickerKind::for_leader(&prefix).unwrap()),
                    false,
                )
            }
            // `␣ct` traces the buffer. Upstream binds Space to play/pause and
            // the arrows to stepping; Proton's leader *is* Space, so the
            // pane's own keys stay scoped to the pane and the verbs that
            // belong to the editor live under the leader like everything else.
            Outcome::Leader(prefix) if prefix == "ct" => {
                self.vim.cancel_leader();
                let mut events = vec![Event::Leader { prefix: None }];
                events.extend(self.trace_buffer());
                (events, false)
            }
            // `␣tt` opens a terminal.
            Outcome::Leader(prefix) if terminal_command(&prefix).is_some() => {
                self.vim.cancel_leader();
                let mut events = vec![Event::Leader { prefix: None }];
                events.extend(self.apply(terminal_command(&prefix).unwrap()));
                (events, false)
            }
            // The debugger, under `␣d` as the rail has always shown it.
            Outcome::Leader(prefix) if debug_command(&prefix).is_some() => {
                self.vim.cancel_leader();
                let mut events = vec![Event::Leader { prefix: None }];
                events.extend(self.apply(debug_command(&prefix).unwrap()));
                (events, false)
            }
            // The `␣l` family: rename, code actions and format.
            Outcome::Leader(prefix) if lsp_command(&prefix).is_some() => {
                self.vim.cancel_leader();
                let mut events = vec![Event::Leader { prefix: None }];
                events.extend(self.apply(lsp_command(&prefix).unwrap()));
                (events, false)
            }
            // The window family, under `␣w` as the rail has always shown it.
            Outcome::Leader(prefix) if window_command(&prefix).is_some() => {
                self.vim.cancel_leader();
                let mut events = vec![Event::Leader { prefix: None }];
                events.extend(self.apply(window_command(&prefix).unwrap()));
                (events, false)
            }
            // `␣e` finds the file you are in. The rail has always claimed this
            // binding in its tooltip; now it exists.
            Outcome::Leader(prefix) if prefix == "e" => {
                self.vim.cancel_leader();
                let mut events = vec![Event::Leader { prefix: None }];
                events.extend(self.apply(Command::RevealInTree));
                (events, false)
            }
            Outcome::Leader(prefix) => (
                vec![Event::Leader {
                    prefix: Some(prefix),
                }],
                false,
            ),
            Outcome::Search { pattern, forward } => (self.search(&pattern, forward), false),
            Outcome::Repeat { forward } => (self.search_again(forward), false),
            Outcome::SearchWord { forward } => {
                let buffer = self.editor.buffer();
                match buffer.word_at(buffer.cursor.offset) {
                    Some(word) => (self.search(&word, forward), false),
                    None => (
                        vec![notice("no word under the cursor".into(), NoticeLevel::Warn)],
                        false,
                    ),
                }
            }

            Outcome::Cmdline(text) => {
                let prompt = self.vim.prompt();
                let mut events = vec![Event::Cmdline {
                    text: text.clone(),
                    prompt: prompt.to_string(),
                }];
                match (&text, prompt) {
                    // The line was abandoned. A search that moved the cursor
                    // while you typed it has to put the cursor back.
                    (None, _) => events.extend(self.abandon_search()),
                    (Some(pattern), '/' | '?') => {
                        events.extend(self.preview_search(pattern, prompt == '/'));
                    }
                    _ => {}
                }
                (events, false)
            }
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
        // The panel describes the word it was opened on. Once anything has
        // happened it is describing somewhere the cursor no longer is, so it
        // goes — the same rule vim's own popups follow.
        if self.hover_open {
            self.hover_open = false;
            self.servers.forget(lsp::Ask::Hover);
            events.push(Event::HoverClosed);
        }
        if text_changed {
            // Breakpoints are places in the text, so they move with it. The
            // line count is the signal: anything else leaves them on the line
            // number they were put on rather than the code.
            if let Some(path) = self.editor.buffer().path().map(Path::to_path_buf) {
                let now = self.editor.buffer().len_lines();
                let by = now as isize - self.lines_before as isize;
                let at = self
                    .editor
                    .buffer()
                    .position(self.editor.buffer().cursor.offset)
                    .line;
                self.debugger.shift(&path, at + 1, by);
                self.lines_before = now;
            }
            events.push(self.viewport_event());
            events.push(self.buffers_event());
            // Any other split on this file has to move with it. Two views of
            // one buffer disagreeing about what is in it, even for a frame, is
            // the bug that makes a split feel like a copy.
            events.extend(self.redraw_others());
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
        let height = self.splits.view().lines;
        let margin = 1.min(height / 4);
        let before = self.splits.view().first_line;

        if line < before + margin {
            self.splits.view_mut().first_line = line.saturating_sub(margin);
        } else if line + margin >= before + height {
            self.splits.view_mut().first_line = (line + margin + 1).saturating_sub(height);
        }
        self.splits.view().first_line != before
    }

    /// The focused split's content.
    fn viewport_event(&mut self) -> Event {
        self.viewport_of(self.splits.focused())
    }

    /// One split's content.
    ///
    /// An unfocused split may be showing a different buffer, so this reads the
    /// view's rather than the editor's active one — which is the difference
    /// between a split and a second copy of the same pane.
    fn viewport_of(&mut self, view: usize) -> Event {
        let Some(pane) = self.splits.at(view).copied() else {
            return Event::Viewport {
                view,
                first_line: 0,
                lines: Vec::new(),
                total_lines: 0,
                version: 0,
                highlights: Vec::new(),
                matches: Vec::new(),
                git: Vec::new(),
                stops: Vec::new(),
            };
        };
        let first_line = pane.first_line;
        let count = pane.lines;
        let buffer = pane
            .buffer
            .min(self.editor.buffers().len().saturating_sub(1));

        // Highlighting is computed here rather than in a second event,
        // because the colours and the text they colour must never be a frame
        // apart — one stale and the other fresh is visibly wrong.
        let spans = self
            .highlights
            .spans(&self.editor.buffers()[buffer], first_line, count);
        let pattern = self.highlight_pattern().map(str::to_owned);
        let buf = &self.editor.buffers()[buffer];
        let lines = buf.lines(first_line, count);

        // Searched per visible line rather than by asking the document where
        // every match is. A pattern cannot contain a newline — Enter submits
        // it — so a line is a complete unit, and the cost is the window rather
        // than the file however many thousand hits there are below the fold.
        let matches = match &pattern {
            Some(pattern) => lines
                .iter()
                .map(|line| {
                    proton_text::find_in(line, pattern)
                        .into_iter()
                        .map(|(start, end)| Hit {
                            start: start as u32,
                            end: end as u32,
                        })
                        .collect()
                })
                .collect(),
            None => Vec::new(),
        };

        // The gutter is computed for the whole file and sent for the window,
        // like everything else here. The diff is against the buffer rather
        // than the file on disk: the point of a gutter is seeing what you have
        // done *before* you save.
        let git = self.git.marks(buffer, buf, first_line, count);

        // Breakpoints are 1-based, like everything a debugger says.
        let stops: Vec<Stop> = match buf.path() {
            Some(path) => {
                let stopped = self.debugger.stopped_at();
                self.debugger
                    .in_file(path)
                    .into_iter()
                    .filter(|(line, _)| *line > first_line && *line <= first_line + count)
                    .map(|(line, verified)| Stop {
                        line: line - 1,
                        verified,
                        here: stopped == Some((path, line)),
                    })
                    .collect()
            }
            None => Vec::new(),
        };

        Event::Viewport {
            view,
            first_line,
            total_lines: buf.len_lines(),
            version: buf.version(),
            lines,
            highlights: spans,
            matches,
            git,
            stops,
        }
    }

    /// What to paint as matched, if anything.
    ///
    /// The pattern being typed wins over the last one committed, which is the
    /// whole of what makes an incremental search useful: you see where you
    /// would land, and everywhere else you could have landed, before pressing
    /// Enter.
    fn highlight_pattern(&self) -> Option<&str> {
        if self.vim.mode() == VimMode::Command && matches!(self.vim.prompt(), '/' | '?') {
            let typed = self.vim.cmdline();
            return (!typed.is_empty()).then_some(typed);
        }
        if !self.highlight {
            return None;
        }
        self.search.as_ref().map(|search| search.pattern.as_str())
    }

    fn cursor_event(&self) -> Event {
        self.cursor_of(self.splits.focused())
    }

    /// One split's cursor.
    ///
    /// The focused view's lives on the buffer, where the operators reach for
    /// it; every other view holds its own. See [`crate::split`].
    fn cursor_of(&self, view: usize) -> Event {
        let focused = view == self.splits.focused();
        let pane = self.splits.at(view).copied().unwrap_or_default();
        let buffer = pane
            .buffer
            .min(self.editor.buffers().len().saturating_sub(1));
        let buf = &self.editor.buffers()[buffer];
        let cursor = if focused { buf.cursor } else { pane.cursor };
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
            view,
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

/// One terminal run, as the bridge carries it.
fn term_run(run: &proton_term::Run) -> TermRun {
    TermRun {
        text: run.text.clone(),
        fg: run.fg,
        bg: run.bg,
        // Turned into a colour here rather than three numbers on the wire: the
        // front end would only put it back together to use it.
        rgb: run.rgb.map(|(r, g, b)| format!("#{r:02x}{g:02x}{b:02x}")),
        bold: run.bold,
        italic: run.italic,
        underline: run.underline,
        inverse: run.inverse,
    }
}

/// The `␣d` family. `␣db` is the one the rail has advertised all along.
fn debug_command(prefix: &str) -> Option<Command> {
    Some(match prefix {
        // Toggle a breakpoint on the cursor's line — the thing you do most.
        "db" => Command::ToggleBreakpoint { line: None },
        "dr" => Command::DebugStart,
        "dq" => Command::DebugStop,
        "dc" => Command::DebugResume,
        "dn" => Command::DebugStepOver,
        "di" => Command::DebugStepInto,
        "do" => Command::DebugStepOut,
        _ => return None,
    })
}

/// `␣t` opens a terminal. Vims own `:terminal` too, and both work.
fn terminal_command(prefix: &str) -> Option<Command> {
    match prefix {
        "tt" => Some(Command::OpenTerminal),
        _ => None,
    }
}

/// The `␣w` family — vim's `⌃w`, under the leader where everything else lives.
///
/// The `␣l` family — everything answered by a language server.
///
/// `␣ls` is not here: it opens a picker, so it goes through the picker's own
/// table with the rest of them. `␣d` was never an option for any of these:
/// that letter is the debugger's, and `␣ds` reading as "debug s…" was the
/// mistake this family is the correction of.
fn lsp_command(prefix: &str) -> Option<Command> {
    Some(match prefix {
        "lr" => Command::PromptRename,
        "la" => Command::CodeActions,
        "lf" => Command::Format,
        _ => return None,
    })
}

/// Core config rather than front-end config, for the same reason the picker's
/// prefixes are: a second front end must not have to reinvent the keymap.
fn window_command(prefix: &str) -> Option<Command> {
    Some(match prefix {
        "wv" => Command::Split { vertical: true },
        "ws" => Command::Split { vertical: false },
        "wq" | "wc" => Command::CloseSplit { index: None },
        "wo" => Command::OnlySplit,
        "wh" => Command::StepSplit {
            direction: Direction::Left,
        },
        "wl" => Command::StepSplit {
            direction: Direction::Right,
        },
        "wk" => Command::StepSplit {
            direction: Direction::Up,
        },
        "wj" => Command::StepSplit {
            direction: Direction::Down,
        },
        "ww" => Command::StepSplit {
            direction: Direction::Next,
        },
        _ => return None,
    })
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

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
use crate::bus::{BufferTab, Command, Event, Hit, NoticeLevel, Selection, ServerState, SplitView};
use crate::editor::Editor;
use crate::ex::Ex;
use crate::highlights::Highlighters;
use crate::key::Key;
use crate::lsp::{self, Popup, Servers, Severity};
use crate::mode::ModeId;
use crate::picker::{Accepted, PickerKey, PickerKind, Pickers};
use crate::split::{Direction, Splits};
use crate::trace::Visualizer;
use crate::tree::Tree;
use crate::vim::{Outcome, Vim, VimMode};

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
            // The window family, under `␣w` as the rail has always shown it.
            Outcome::Leader(prefix) if window_command(&prefix).is_some() => {
                let mut events = vec![Event::Leader { prefix: None }];
                events.extend(self.apply(window_command(&prefix).unwrap()));
                (events, false)
            }
            // `␣e` finds the file you are in. The rail has always claimed this
            // binding in its tooltip; now it exists.
            Outcome::Leader(prefix) if prefix == "e" => {
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
        if text_changed {
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

        Event::Viewport {
            view,
            first_line,
            total_lines: buf.len_lines(),
            version: buf.version(),
            lines,
            highlights: spans,
            matches,
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

/// The `␣w` family — vim's `⌃w`, under the leader where everything else lives.
///
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

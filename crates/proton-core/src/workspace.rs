//! Workspace state — the thing the shell is a view of.
//!
//! `apply` is deliberately total: one command in, a list of events out. The
//! only I/O is opening and saving a file, which is what a command asked for.
//! No clock, no threads, no windowing — that is what makes the core testable
//! headless and lets a second front end reuse it verbatim.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use proton_lsp::client::Incoming;

use crate::action::{Action, Object, Verb};
use crate::agent::Agent;
use crate::bus::{
    BufferTab, Command, DebugFrame, DebugPhase, DebugVariable, Event, Hit, NoticeLevel, Selection,
    ServerState, SplitView, Stop, TermRun, ViewerKind,
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
use crate::review;
use crate::split::{Direction, Splits, View};
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
    /// The notebook Data mode has open, and the kernel running it.
    data: crate::data::Data,
    design: crate::design::Design,
    settings: crate::settings::Settings,
    /// The user's own documents, from the roots the settings name. Empty
    /// unless somebody listed one: nothing here discovers a vault, because a
    /// library that could reach anywhere under the home directory is one that
    /// can put a medical record in a prompt by accident.
    library: proton_library::Shelf,
    /// The progress last reported, so the panel is told when it moves rather
    /// than on every heartbeat for the minutes a first walk over a corpus of
    /// PDFs takes.
    library_progress: (usize, usize),
    /// Which folders of the library a search may draw on. Empty is all of
    /// them. Read from the settings so it survives a restart — somebody
    /// working through one subject for a week should not re-tick it daily.
    library_scope: proton_library::Scope,
    /// Tool calls waiting on a kernel.
    ///
    /// A cell run is the one tool that cannot answer when it is asked: the
    /// tool loop is synchronous and a kernel is not. The turn stays paused
    /// while these sit here, because `Agent::resume` only continues once every
    /// outstanding call has an answer — so deferring one is simply not
    /// answering it yet.
    waiting: Vec<Waiting>,
    /// The file the open conversation lives in. `None` until it has a
    /// question in it — a conversation with nothing said is not worth a file.
    chat_path: Option<PathBuf>,
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
    /// Changes the agent has proposed and nobody has accepted yet.
    review: review::Review,
    /// What has been asked of the agent, running and waiting.
    tasks: crate::tasks::Queue,
    /// The loaded plugins. Empty unless somebody loaded them. Shared with
    /// the session, which loads them once for every project.
    plugins: Arc<crate::plugins::Plugins>,
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

/// What running a tool produced.
///
/// Almost every tool answers in the breath it is called. `run_cell` cannot:
/// the tool loop is synchronous and a kernel is not. That difference deserves
/// a type rather than a special case buried in the loop — with a type, a tool
/// added later that also has to wait says so in the same way, and the loop
/// does not have to know its name.
pub enum ToolOutcome {
    /// Done. Send this back to the model.
    Answered(Result<String, String>),
    /// Started, and will be answered on a later heartbeat. The turn stays
    /// paused meanwhile, because `Agent::resume` waits for every outstanding
    /// call — so deferring one is simply not answering it yet.
    Deferred,
}

/// A `run_cell` that has been started and not yet answered.
#[derive(Debug)]
struct Waiting {
    /// The tool call to answer when the cell stops running.
    id: String,
    cell: usize,
    /// Heartbeats spent waiting.
    ///
    /// Beats rather than seconds, because the core has no clock. The heartbeat
    /// is nominally 50ms, so `PATIENCE` is about ten minutes — but this is a
    /// bound on how long one turn may be blocked, not a measurement of time,
    /// and it does not depend on that rate being exact.
    beats: u32,
}

/// How many beats to wait for a cell before giving up on it.
///
/// Generous on purpose: training a model or reading a large frame is a cell
/// that legitimately runs for minutes, and a bound tight enough to catch a
/// typo would break the work this is for. What it catches is the cell that
/// never finishes at all.
const PATIENCE: u32 = 12_000;

impl Workspace {
    pub fn new() -> Self {
        Self::rooted(".").with_language_servers()
    }

    /// The project root every picker searches. Separate from `new` so a test
    /// can point at a fixture instead of the process's working directory.
    pub fn rooted(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        let mut space = Self {
            editor: Editor::new(),
            pickers: Pickers::new(root),
            servers: Servers::new(root),
            agent: Agent::new(&root.display().to_string()),
            visualizer: Visualizer::new(root),
            tree: Tree::new(root),
            git: Git::discover(root),
            debugger: Debugger::new(root),
            settings: crate::settings::Settings::load(root),
            term_size: (24, 80),
            ..Default::default()
        };
        // The settings were loaded a line ago; the agent has to be told what
        // they say, or the first question of every session goes to whatever
        // the default provider is regardless of what was chosen.
        space.follow_agent_settings();
        // And the library, which starts its walk here rather than on the first
        // question: a vault takes the better part of a second to read, and the
        // moment somebody asks about their notes is the worst one to spend it.
        space.follow_library_settings();
        space
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

    /// The agent, for what it has been told and what it has said.
    ///
    /// Its `conversation` is what actually goes to the model, which is a
    /// different thing from the transcript that gets drawn — and the one worth
    /// asserting on when the question is whether something was really sent.
    pub fn agent(&self) -> &crate::agent::Agent {
        &self.agent
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

    /// A cheap summary of everything the stored layout depends on, except the
    /// cursor.
    ///
    /// Used to notice that the layout changed without asking what it is:
    /// building a `Layout` allocates a path per buffer, and doing that after
    /// every keystroke to find out whether anything moved would cost more than
    /// the saving it triggers. This is a handful of integers.
    ///
    /// Deliberately blind to the cursor. A file write per cursor move is not
    /// worth it, and the cursor is written whenever something structural
    /// happens anyway — as well as when the window closes.
    pub fn shape(&self) -> Vec<usize> {
        let mut shape = Vec::with_capacity(self.splits.len() + 3);
        shape.push(self.editor.buffers().len());
        shape.push(self.splits.focused());
        shape.push(usize::from(self.splits.is_vertical()));
        shape.extend(self.splits.views().iter().map(|view| view.buffer));
        shape
    }

    /// What is open here, and how the panes are arranged.
    ///
    /// Only files. A buffer with no path is something typed but never saved,
    /// and writing its *name* into the session would promise to reopen a
    /// thing that does not exist. Panes pointing at one are folded onto the
    /// first buffer rather than dropped, so the arrangement survives even
    /// when one of its contents cannot.
    pub fn layout(&self) -> Layout {
        let mut files = Vec::new();
        // Where each editor buffer ended up in `files`, so views can be
        // renumbered — the two lists differ as soon as one buffer is skipped.
        let mut moved: Vec<Option<usize>> = Vec::new();
        for buffer in self.editor.buffers() {
            match buffer.path() {
                Some(path) => {
                    moved.push(Some(files.len()));
                    files.push(path.to_path_buf());
                }
                None => moved.push(None),
            }
        }

        Layout {
            files,
            views: self
                .splits
                .views()
                .iter()
                .map(|view| StoredView {
                    file: moved.get(view.buffer).copied().flatten().unwrap_or(0),
                    first_line: view.first_line,
                    line: view.cursor.offset,
                })
                .collect(),
            focused: self.splits.focused(),
            vertical: self.splits.is_vertical(),
        }
    }

    /// Put one back. Best effort, and deliberately so.
    ///
    /// A session is a description of a world that has moved on: files get
    /// deleted, renamed, or live on a drive that is not plugged in. Every one
    /// that still opens is opened, the rest are passed over, and the panes are
    /// rebuilt around what survived. Refusing to restore anything because one
    /// file is missing would be the worse answer by far.
    pub fn restore_layout(&mut self, layout: &Layout) {
        let mut opened: Vec<Option<usize>> = Vec::new();
        for file in &layout.files {
            opened.push(self.editor.open(file).ok());
        }
        if opened.iter().all(Option::is_none) {
            return;
        }

        let views: Vec<View> = layout
            .views
            .iter()
            .map(|stored| {
                let buffer = opened.get(stored.file).copied().flatten().unwrap_or(0);
                let mut view = View {
                    buffer,
                    first_line: stored.first_line,
                    ..View::default()
                };
                // Clamped against the file as it is *now*. An offset saved
                // last week can point past the end of a file somebody has
                // since edited, and a cursor outside its buffer is the kind
                // of thing that panics three operations later.
                let len = self.editor.buffers()[buffer].len_chars();
                view.cursor.offset = stored.line.min(len);
                view.cursor.anchor = view.cursor.offset;
                view
            })
            .collect();

        let count = self.editor.buffers().len();
        self.splits
            .restore(views, layout.focused, layout.vertical, count);
        // The focused pane's buffer is the one the editor is showing, and the
        // two disagreeing is what makes a restored window edit the wrong file.
        let showing = self.splits.view().buffer;
        self.editor.focus(showing);
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
                    // Once, here. Plugins are loaded when the session is built
                    // and there is no way to install one without restarting,
                    // so there is nothing later to send.
                    self.plugins_event(),
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
                    self.repo_event(),
                    // A window that has just connected to a session with a
                    // notebook open would otherwise show an empty stage until
                    // something happened to change a cell.
                    self.notebook_event(),
                    self.theme_event(),
                    self.settings_event(),
                    self.library_event(),
                    self.chats_event(),
                    Event::Layout {
                        layout: self.mode.layout(),
                    },
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

            Command::PickerQuery { text } => {
                let labels = self.buffer_labels();
                self.pickers.seed(&text, labels);
                // The library is asked again rather than narrowed. Every
                // other picker is done at the line above.
                self.ask_library();
                vec![self.picker_event()]
            }

            Command::RefreshRepo => vec![self.repo_event()],

            /* ── notebooks ────────────────────────────────────────── */
            Command::RunCell { index } => {
                let cwd = self.pickers.root().to_path_buf();
                match crate::data::python(&cwd) {
                    Some(python) => match self.data.run(index, &python, &cwd) {
                        Ok(()) => vec![self.notebook_event()],
                        Err(why) => vec![notice(why, NoticeLevel::Error)],
                    },
                    None => vec![notice(
                        "no python to run a notebook with — set PROTON_PYTHON".into(),
                        NoticeLevel::Error,
                    )],
                }
            }

            Command::SelectCell { index } => {
                self.data.select_cell(index);
                vec![self.notebook_event()]
            }

            /* ── themes ────────────────────────────────────── */
            Command::OpenTheme { path } => {
                let full = self.pickers.root().join(&path);
                match self.design.open(&full) {
                    Ok(()) => vec![self.theme_event()],
                    Err(why) => vec![notice(why, NoticeLevel::Error)],
                }
            }

            Command::SetToken { name, scope, value } => {
                match self.design.set(&name, &scope, &value) {
                    // Nothing changed: the token is not there, or it already had
                    // that value. Neither is worth an event or a notice.
                    Ok(false) => Vec::new(),
                    // The file on disk moved, so anything showing it has to be
                    // told — the gutter, the tree, and the buffer if Codex has the
                    // same file open, which for a two-way binding it often does.
                    Ok(true) => {
                        let mut events = vec![self.theme_event()];
                        events.push(self.repo_event());
                        events.extend(self.every_view());
                        events
                    }
                    Err(why) => vec![notice(why, NoticeLevel::Error)],
                }
            }

            Command::SelectToken { index } => {
                self.design.select(index);
                vec![self.theme_event()]
            }

            /* ── settings ──────────────────────────────────── */
            Command::SetSetting { key, value } => match self.settings.set(&key, value) {
                // Already had that value. The panel emits on every click and
                // most clicks change nothing.
                Ok(false) => Vec::new(),
                Ok(true) => {
                    if key.starts_with("agent.") {
                        self.follow_agent_settings();
                    }
                    let mut events = vec![self.settings_event()];
                    // A root added here starts a walk here. The panel says
                    // "reading" until the heartbeat brings the answer, which
                    // is the difference between a slow library and an empty
                    // one.
                    if key.starts_with("library.") {
                        self.follow_library_settings();
                        events.push(self.library_event());
                    }
                    // The pane's header names the provider, so it has to be
                    // told when the provider changed under it.
                    events.push(self.agent_event());
                    events
                }
                Err(why) => vec![notice(why, NoticeLevel::Error)],
            },

            Command::RunAll => {
                let cwd = self.pickers.root().to_path_buf();
                match crate::data::python(&cwd) {
                    Some(python) => match self.data.run_all(&python, &cwd) {
                        Ok(0) => vec![notice(
                            "nothing to run: no code cells".into(),
                            NoticeLevel::Info,
                        )],
                        Ok(n) => vec![
                            notice(format!("running {n} cells"), NoticeLevel::Info),
                            self.notebook_event(),
                        ],
                        Err(why) => vec![notice(why, NoticeLevel::Error)],
                    },
                    None => vec![notice(
                        "no python to run a notebook with — set PROTON_PYTHON".into(),
                        NoticeLevel::Error,
                    )],
                }
            }

            Command::SetCell { index, source } => {
                self.data.set_source(index, &source);
                vec![self.notebook_event()]
            }

            Command::AddCell { code } => {
                self.data.add_cell(code);
                vec![self.notebook_event()]
            }

            Command::RemoveCell { index } => {
                self.data.remove_cell(index);
                vec![self.notebook_event()]
            }

            Command::SaveNotebook => match self.data.save() {
                Ok(()) => vec![
                    notice("notebook written".into(), NoticeLevel::Info),
                    self.notebook_event(),
                ],
                Err(why) => vec![notice(why, NoticeLevel::Error)],
            },

            // The cross-mode jump. One command, because switching, opening
            // and moving as three would be three redraws and two chances to
            // land in the new mode looking at the old file.
            Command::JumpTo {
                mode,
                path,
                line,
                column,
            } => {
                let mut events = self.set_mode(mode);
                events.extend(self.open_path(&path));
                events.extend(self.apply(Command::PlaceCursor { line, column }));
                events
            }

            Command::SetLibraryScope { sources } => {
                // Through the settings rather than straight onto the field, so
                // one place decides and the choice survives a restart. The
                // write comes back around and sets the scope.
                match self
                    .settings
                    .set("library.scope", serde_json::json!(sources))
                {
                    Ok(_) => {
                        self.follow_library_settings();
                        vec![self.settings_event(), self.library_event()]
                    }
                    Err(why) => vec![notice(why, NoticeLevel::Error)],
                }
            }

            Command::RefreshLibrary => {
                if self.library.roots().is_empty() {
                    return vec![notice(
                        "no library roots: add one in Settings › Library".into(),
                        NoticeLevel::Info,
                    )];
                }
                self.library.refresh();
                vec![self.library_event()]
            }

            Command::OpenCitation { cite } => {
                let Some(cited) = proton_library::chunk::parse_cite(&cite) else {
                    return vec![notice(
                        format!("{cite}: not a citation"),
                        NoticeLevel::Error,
                    )];
                };
                // Through the library's roots rather than the workspace's: a
                // citation names a file in somebody's vault, which is nowhere
                // near the project. `locate` is also what refuses to leave the
                // roots, and the path in a citation is text a model wrote.
                let Some(path) = self.library.locate(&cited.path) else {
                    return vec![notice(
                        format!("{}: not in any library root", cited.path),
                        NoticeLevel::Error,
                    )];
                };
                // A page is not something this editor can show. Opening a PDF
                // as text would put a screenful of binary in a buffer, which
                // is worse than not following the citation at all — so it goes
                // to whatever the user already reads PDFs with.
                if cited.place == proton_library::chunk::Place::Pages {
                    return match open_outside(&path) {
                        Ok(()) => vec![notice(
                            format!("{} — page {}", cited.path, cited.from),
                            NoticeLevel::Info,
                        )],
                        Err(why) => vec![notice(why, NoticeLevel::Error)],
                    };
                }
                self.apply(Command::JumpTo {
                    mode: ModeId::Codex,
                    path: path.display().to_string(),
                    // The citation counts from one, as a gutter does; every
                    // line number crossing this bus counts from zero.
                    line: cited.from - 1,
                    column: 0,
                })
            }

            Command::OpenTerminal => {
                // A shell that has exited is as good as no shell: asking for a
                // terminal when the last one is dead means wanting a live one,
                // not being shown its corpse a second time.
                let gone = self.terminal.as_mut().is_some_and(|t| !t.is_alive());
                if gone {
                    self.terminal = None;
                }
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

            Command::TerminalScroll { lines } => {
                // Only redrawn when the view actually moved, so a wheel spun
                // at either end of the history costs nothing.
                let moved = self.terminal.as_mut().is_some_and(|t| t.scroll(lines));
                match moved {
                    true => vec![self.terminal_event()],
                    false => Vec::new(),
                }
            }

            Command::TerminalToBottom => {
                let moved = self
                    .terminal
                    .as_mut()
                    .is_some_and(proton_term::Terminal::scroll_to_bottom);
                match moved {
                    true => vec![self.terminal_event()],
                    false => Vec::new(),
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

            Command::Ask { text } => {
                let mut events = self.ask(&text);
                // Saved as soon as it is asked, not when the answer lands. A
                // question whose answer never arrived is still something you
                // asked, and losing it because the model timed out would be
                // the worst moment to lose it.
                events.extend(self.save_chat());
                events
            }

            /* ── conversations ────────────────────────────────── */
            Command::NewChat => {
                // Whatever was open is already on disk, so this only lets go
                // of it. Nothing is written for an empty conversation.
                self.agent.clear();
                self.waiting.clear();
                self.chat_path = None;
                vec![self.agent_event(), self.chats_event()]
            }

            Command::OpenChat { path } => {
                let full = self.pickers.root().join(&path);
                match crate::chats::read(&full) {
                    Ok(chat) => {
                        self.agent.restore(chat.messages);
                        self.chat_path = Some(full);
                        vec![self.agent_event(), self.chats_event()]
                    }
                    Err(why) => vec![notice(why, NoticeLevel::Error)],
                }
            }

            Command::DeleteChat { path } => {
                let full = self.pickers.root().join(&path);
                if let Err(why) = crate::chats::remove(&full) {
                    return vec![notice(why, NoticeLevel::Error)];
                }
                // Deleting the one on screen leaves it on screen and unowned,
                // which would silently write it back on the next question.
                if self.chat_path.as_deref() == Some(full.as_path()) {
                    self.agent.clear();
                    self.chat_path = None;
                }
                vec![self.agent_event(), self.chats_event()]
            }

            Command::RenameChat { path, title } => {
                let full = self.pickers.root().join(&path);
                match crate::chats::read(&full) {
                    Ok(mut chat) => {
                        chat.title = title;
                        match crate::chats::write(&full, &chat) {
                            // The file keeps its name. A rename that moved it
                            // would break every link to it for a cosmetic
                            // change.
                            Ok(()) => vec![self.chats_event()],
                            Err(why) => vec![notice(why, NoticeLevel::Error)],
                        }
                    }
                    Err(why) => vec![notice(why, NoticeLevel::Error)],
                }
            }

            Command::DropTask { id } => {
                self.tasks.drop_task(id);
                vec![self.tasks_event()]
            }

            Command::ClearTasks => {
                self.tasks.clear_finished();
                vec![self.tasks_event()]
            }

            Command::CancelTurn => {
                self.agent.cancel();
                // The cell keeps running — it is the user's kernel and their
                // code — but nothing is waiting on it any more. Leaving these
                // would answer into a turn that no longer exists.
                self.waiting.clear();
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

            Command::ApplyHunk { path, hunk } => self.apply_hunk(&path, hunk),

            Command::RejectHunk { path, hunk } => self.reject_hunk(&path, hunk),

            Command::ApplyStaged { path } => self.apply_staged(path.as_deref()),

            Command::RejectStaged { path } => self.reject_staged(path.as_deref()),

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
        // The plugins are already loaded, so their commands are known now.
        // Set before the picker opens rather than after, which is the only
        // way this differs from the ones a server fills.
        if kind == PickerKind::PluginCommands {
            if self.plugins.commands().is_empty() {
                return vec![notice(
                    "no plugin commands — nothing installed that offers any".into(),
                    NoticeLevel::Warn,
                )];
            }
            let rows = self.plugin_rows();
            self.pickers.set_rows(rows);
        }
        if kind == PickerKind::Library {
            if self.library.roots().is_empty() {
                return vec![notice(
                    "no library roots: add one in Settings › Library".into(),
                    NoticeLevel::Info,
                )];
            }
            // Opened with an empty query, which the library answers with
            // nothing — there is no "everything" to list when the rows are an
            // answer rather than a catalogue. The prompt is the invitation.
            self.pickers.set_rows(Vec::new());
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
        // After the keystroke, whatever it was: typing and backspacing both
        // change the query, and for the library a changed query is a new
        // question rather than a narrower answer to the old one.
        self.ask_library();
        vec![self.picker_event()]
    }

    /// How many passages the picker shows.
    ///
    /// Far fewer than the two hundred rows every other picker allows. These
    /// are ranked by relevance rather than filtered, so the tail is not "less
    /// exact" but "about something else" — and a passage is several lines,
    /// which makes a long list something to scroll rather than to read.
    const SHELVES: usize = 20;

    /// Re-ask the library, if that is the picker that is open.
    ///
    /// Called after every change to the query, because for this one picker
    /// typing is a new question rather than a narrowing of the last answer.
    /// Cheap enough to do on a keystroke: a search over twenty-eight thousand
    /// passages measured at 158µs.
    fn ask_library(&mut self) {
        let Some(query) = self.pickers.query_of(PickerKind::Library) else {
            return;
        };
        // An empty query has no answer. Every other picker lists everything
        // when nothing is typed; a library cannot, because there is no order
        // to list a corpus in that means anything.
        let rows = match query.trim().is_empty() {
            true => Vec::new(),
            false => self
                .library
                .search_in(&query, Self::SHELVES, &self.library_scope)
                .into_iter()
                .map(|hit| crate::picker::PickerItem {
                    // The citation is the label because it is both what
                    // identifies the passage and what opens it — what you are
                    // looking at is what you get.
                    label: hit.cite,
                    detail: snippet(&hit.passage.text),
                    positions: Vec::new(),
                    path: None,
                    line: None,
                    column: None,
                    kind: None,
                })
                .collect(),
        };
        self.pickers.refill(rows);
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
            Accepted::OpenCitation { cite } => {
                // Straight to the command the agent pane's citations already
                // use: it resolves against the library's roots, refuses one
                // that points outside them, and knows that a page goes to a
                // PDF reader and a line goes to the editor.
                events.extend(self.apply(Command::OpenCitation { cite }));
            }
            Accepted::FocusBuffer { index } => {
                if self.editor.focus(index) {
                    events.push(self.buffers_event());
                    events.extend(self.after_action(true));
                }
            }
            Accepted::RunAction { index } => events.extend(self.chose_action(index)),
            Accepted::RunPluginCommand { index } => events.extend(self.run_plugin(index)),
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
                // The other direction of the two-way binding. Editing the
                // theme as text in Codex and saving it has to reach the
                // inspector, or the palette on screen is the one the file had
                // when Design mode was opened.
                if self.design.reload() {
                    events.push(self.theme_event());
                }
                // On by default: a formatter configured and not run is the
                // surprise, not the other way round. Off is for a project
                // whose formatter disagrees with its own history.
                if self.settings.bool("editor.formatOnSave", true) {
                    events.extend(self.request_format(true));
                }
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
        let resolved = self.pickers.resolve(path);

        // A notebook is Data mode's document, so opening one is a mode switch
        // as much as a file open — the cells belong on the stage, and the
        // editor has nothing useful to do with a page of JSON.
        if resolved.extension().is_some_and(|e| e == "ipynb") {
            return match self.data.open(&resolved) {
                Ok(()) => {
                    let mut events = self.set_mode(ModeId::Data);
                    events.push(self.notebook_event());
                    events
                }
                Err(why) => vec![notice(why, NoticeLevel::Error)],
            };
        }

        // Some files are to be looked at rather than edited, and a PDF is the
        // first of them. It does not become a buffer: a buffer has an undo
        // history, a cursor and a `:w`, and offering any of those for a file
        // that cannot be edited would be a promise the editor cannot keep.
        //
        // Until this, opening one failed — `read_to_string` refuses invalid
        // UTF-8 — so the whole of the old behaviour was an error notice.
        if let Some(kind) = ViewerKind::of(&resolved) {
            return vec![Event::Document {
                path: resolved.display().to_string(),
                name: crate::folder::name_of(&resolved),
                kind,
            }];
        }

        match self.editor.open(resolved) {
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
        // The kernel, on the same heartbeat as everything else out of process.
        // A cell that has stopped running is a tool call that can be answered.
        // Here rather than in `poll_agent`, because what decides it is the
        // kernel rather than the agent — and because `poll_agent` only runs
        // while a turn is in flight, which is one condition too many for
        // something whose whole job is to end a wait.
        events.extend(self.answer_finished_cells());

        if self.data.poll() {
            // A kernel that died under a running cell is news, not a state:
            // the chip says what is wrong now, but only a notice tells
            // somebody who is looking at the cell rather than at the header.
            if let Some(why) = self.data.complaint() {
                events.push(notice(why, NoticeLevel::Error));
            }
            events.push(self.notebook_event());
        }
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

        // And the library's walk, which is the same shape again: a second of
        // reading somebody's vault, on a thread with nothing to call back to.
        // It answers twice — the notes, then the PDFs — so this is not a
        // one-shot.
        if self.library.poll() {
            events.push(self.library_event());
        }
        // Progress, while the slow half runs. Sent when the numbers change
        // rather than on every beat: a walk that takes minutes would otherwise
        // send twelve hundred events saying nothing new.
        if self.library.progress() != self.library_progress {
            self.library_progress = self.library.progress();
            events.push(self.library_event());
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

    /* ── plugins ──────────────────────────────────────────────────── */

    /// Load whatever is installed. Explicit, and off in tests, because a core
    /// test must not start a wasm engine and compile whatever happens to be
    /// on the machine running it.
    /// Load plugins from a folder named outright.
    ///
    /// A workspace on its own, for tests and for a single-project embedding.
    /// A session loads them once and shares them — see [`Self::share_plugins`]
    /// — because a plugin belongs to the person and not to the checkout.
    pub fn with_plugins_from(mut self, folder: &Path) -> Self {
        let mut plugins = crate::plugins::Plugins::default();
        plugins.load(folder);
        self.plugins = Arc::new(plugins);
        self
    }

    /// Use the session's already-loaded plugins.
    pub fn share_plugins(&mut self, plugins: &Arc<crate::plugins::Plugins>) {
        self.plugins = Arc::clone(plugins);
    }

    /// The same, for the builder chain a session's `build` uses.
    pub fn sharing_plugins(mut self, plugins: &Arc<crate::plugins::Plugins>) -> Self {
        self.share_plugins(plugins);
        self
    }

    pub fn plugins(&self) -> &crate::plugins::Plugins {
        &self.plugins
    }

    fn plugins_event(&self) -> Event {
        Event::Plugins {
            items: self.plugins.view(),
            themes: self.plugins.themes(),
        }
    }

    /// Every plugin command, as picker rows.
    ///
    /// The plugin's name is the dim half, because you are looking for what
    /// you want to do rather than for which plugin happens to provide it —
    /// but you still want to know who is about to run.
    fn plugin_rows(&self) -> Vec<crate::picker::PickerItem> {
        self.plugins
            .commands()
            .into_iter()
            .enumerate()
            .map(|(index, (plugin, command))| crate::picker::PickerItem {
                label: command.title,
                detail: plugin,
                positions: Vec::new(),
                path: None,
                // Addressed by position, like a code action: the thing chosen
                // is not a place.
                line: Some(index),
                column: None,
                kind: None,
            })
            .collect()
    }

    /// Run the chosen one, and do what it asked for.
    fn run_plugin(&mut self, index: usize) -> Vec<Event> {
        let all = self.plugins.commands();
        let Some((plugin, command)) = all.get(index).cloned() else {
            return Vec::new();
        };

        // What the plugin is lent for the length of the call. It cannot see
        // the buffer — it is outside the sandbox — so anything it needs has
        // to be handed over here.
        let buffer = self.editor.buffer();
        let at = buffer.position(buffer.cursor.offset);
        let lent = proton_plugin::Lent {
            buffer: buffer.contents(),
            cursor: (at.line as u32, at.column as u32),
            root: self.pickers.root().to_string_lossy().into_owned(),
        };

        match self.plugins.run(&plugin, &command.id, lent) {
            Ok(proton_plugin::Outcome::Message(text)) => vec![notice(text, NoticeLevel::Info)],
            Ok(proton_plugin::Outcome::Insert(text)) => self.insert_from_plugin(&text),
            // The plugin's own refusal, which is not the same as a crash and
            // should not read like one.
            Ok(proton_plugin::Outcome::Failed(why)) => {
                vec![notice(format!("{plugin}: {why}"), NoticeLevel::Warn)]
            }
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }

    /// Put a plugin's text in the buffer, at the cursor.
    ///
    /// An ordinary edit, so `u` takes it back. A plugin whose work you can
    /// only undo in git is one you would not run twice.
    fn insert_from_plugin(&mut self, text: &str) -> Vec<Event> {
        let buffer = self.editor.buffer_mut();
        let at = buffer.cursor.offset;
        let landing = at + text.chars().count();
        buffer.apply(
            vec![proton_text::Edit {
                start: at,
                end: at,
                text: text.to_string(),
            }],
            proton_text::Cursor::at(landing),
        );
        self.resync();
        let mut events = vec![self.buffers_event()];
        events.extend(self.after_action(true));
        events
    }

    /* ── the agent's staged changes ───────────────────────────────── */

    /// What one staged file says right now — from the buffer if it is open,
    /// which it always is, because staging opens it.
    fn staged_text(&self, path: &Path) -> Option<String> {
        self.editor
            .buffers()
            .iter()
            .find(|b| b.path() == Some(path))
            .map(|b| b.contents())
            .or_else(|| std::fs::read_to_string(path).ok())
    }

    /// Everything the agent has proposed and nobody has accepted yet.
    ///
    /// Computed on the spot, like every other view of the review: the hunks
    /// are the diff between each buffer and what was proposed, so there is
    /// nothing stored that this could return a stale copy of.
    pub fn staged(&self) -> Vec<review::ChangeView> {
        self.review
            .view(self.pickers.root(), |path| self.staged_text(path))
    }

    fn review_event(&self) -> Event {
        Event::Review {
            files: self.staged(),
        }
    }

    /// Accept one hunk: put it in the buffer, and leave it there unsaved.
    ///
    /// Applied as an ordinary edit, so it joins the undo history like
    /// anything else you did. Accepting the agent's work and then pressing
    /// `u` has to take it back — the alternative is a change you can only
    /// undo in git.
    fn apply_hunk(&mut self, path: &str, index: usize) -> Vec<Event> {
        let path = self.pickers.resolve(path);
        let Some(current) = self.staged_text(&path) else {
            return vec![notice(
                format!("{}: not open any more", path.display()),
                NoticeLevel::Warn,
            )];
        };
        let Some((lines, replacement)) = self.review.accept(&path, index, &current) else {
            // The file moved under the review. Not an error: the panel is
            // about to be redrawn without that hunk in it.
            return vec![self.review_event()];
        };

        let was = self.editor.active();
        if self.editor.open_or_create(&path, &current).is_err() {
            return vec![notice(
                format!("{}: cannot open", path.display()),
                NoticeLevel::Error,
            )];
        }

        let buffer = self.editor.buffer_mut();
        let start = buffer.offset_of(proton_text::Position {
            line: lines.start,
            column: 0,
        });
        // The end of the last line it replaces, newline included — except at
        // the end of the file, where there is no newline to include and
        // reaching for one lands past the rope.
        let end = match lines.end < buffer.len_lines() {
            true => buffer.offset_of(proton_text::Position {
                line: lines.end,
                column: 0,
            }),
            false => buffer.len_chars(),
        };
        let mut text = replacement.join("\n");
        if !text.is_empty() && end < buffer.len_chars() {
            text.push('\n');
        }
        let landing = start + text.chars().count();
        buffer.apply(
            vec![proton_text::Edit { start, end, text }],
            proton_text::Cursor::at(landing),
        );

        // Nothing left to argue about in this file.
        let now = self.editor.buffer().contents();
        if self.review.hunks(&path, &now).is_empty() {
            self.review.forget(&path);
        }

        self.editor.focus(was);
        self.splits.view_mut().buffer = self.editor.active();
        self.resync();

        let mut events = vec![self.review_event(), self.buffers_event()];
        events.extend(self.after_action(true));
        events
    }

    /// Refuse one hunk. The buffer is untouched; what changes is what is
    /// still being proposed.
    fn reject_hunk(&mut self, path: &str, index: usize) -> Vec<Event> {
        let path = self.pickers.resolve(path);
        let Some(current) = self.staged_text(&path) else {
            return vec![self.review_event()];
        };
        self.review.reject(&path, index, &current);
        vec![self.review_event()]
    }

    /// Take everything staged, for one file or for all of them.
    ///
    /// Hunk by hunk from the bottom up, so that applying one cannot move the
    /// lines the next one describes. Top-down would need every later hunk
    /// renumbered after every edit, which is the bookkeeping this whole
    /// design exists to avoid.
    fn apply_staged(&mut self, path: Option<&str>) -> Vec<Event> {
        let paths = match path {
            Some(one) => vec![self.pickers.resolve(one)],
            None => self.review.paths(),
        };

        let mut applied = 0;
        for path in &paths {
            let Some(current) = self.staged_text(path) else {
                continue;
            };
            let count = self.review.hunks(path, &current).len();
            for index in (0..count).rev() {
                self.apply_hunk(&path.to_string_lossy(), index);
                applied += 1;
            }
        }

        let mut events = vec![notice(
            match applied {
                0 => "nothing staged".into(),
                1 => "applied 1 hunk — not written yet".into(),
                n => format!("applied {n} hunks — not written yet"),
            },
            NoticeLevel::Info,
        )];
        events.push(self.review_event());
        events.push(self.buffers_event());
        events.extend(self.after_action(true));
        events
    }

    fn reject_staged(&mut self, path: Option<&str>) -> Vec<Event> {
        match path {
            Some(one) => self.review.forget(&self.pickers.resolve(one)),
            None => self.review.clear(),
        }
        vec![
            notice("discarded what was staged".into(), NoticeLevel::Info),
            self.review_event(),
        ]
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
    /// Ask the agent something, or queue it if it is already working.
    ///
    /// The agent runs one turn at a time and always will — a conversation is
    /// a sequence, and two turns interleaving into one transcript would be
    /// neither. What changed is what happens to the second question: it used
    /// to be refused, which meant remembering it yourself and asking again at
    /// the right moment. Now it waits.
    fn ask(&mut self, text: &str) -> Vec<Event> {
        if text.trim().is_empty() {
            return Vec::new();
        }
        let text = &self.with_context(text);
        let busy = self.agent.is_busy();
        self.tasks.push(text, busy);
        if busy {
            return vec![self.tasks_event()];
        }
        match self.agent.ask(text) {
            Ok(()) => vec![self.agent_event(), self.tasks_event()],
            Err(why) => {
                // A missing credential is the common case and it is not an
                // error in the code — it is a thing the user has to go and
                // do. The task still records that it was asked and failed.
                self.tasks.finished(&why, 0, Vec::new(), false);
                vec![notice(why, NoticeLevel::Warn), self.tasks_event()]
            }
        }
    }

    /// Start whatever is next, once a turn has ended.
    fn task_finished(&mut self) -> Vec<Event> {
        let last = self.agent.transcript().last();
        let ok = last.is_some();
        let note = last.map(|e| first_sentence(&e.text)).unwrap_or_default();
        let tools = last.map_or(0, |e| e.calls.len());
        // What it left for you to look at. The point of the group being
        // called "ready for review" rather than "done".
        let files = self.staged().into_iter().map(|c| c.path).collect();

        let mut events = Vec::new();
        if let Some(next) = self.tasks.finished(&note, tools, files, ok) {
            match self.agent.ask(&next) {
                Ok(()) => events.push(self.agent_event()),
                Err(why) => {
                    self.tasks.finished(&why, 0, Vec::new(), false);
                    events.push(notice(why, NoticeLevel::Warn));
                }
            }
        }
        events.push(self.tasks_event());
        events
    }

    fn tasks_event(&self) -> Event {
        Event::Tasks {
            tasks: self.tasks.all().to_vec(),
        }
    }

    /// Drain the agent, run whatever it asked for, and continue the turn.
    ///
    /// The tools run here rather than in `proton-agent` because this is where
    /// the editor is: `grep` is the same index Telescope searches, and
    /// `read_file` reads through the same buffers the user has open, so what
    /// the agent sees and what the user sees cannot drift apart.
    fn poll_agent(&mut self) -> Vec<Event> {
        let was_busy = self.agent.is_busy();
        let calls = self.agent.poll();
        let mut events = Vec::new();

        for call in calls {
            let answer = match self.call_tool(&call.id, &call.name, &call.input) {
                ToolOutcome::Answered(answer) => answer,
                ToolOutcome::Deferred => {
                    // The cell is drawn as running straight away, so the
                    // notebook shows what the agent is doing to it.
                    events.push(self.notebook_event());
                    continue;
                }
            };
            // Both of these change what is on screen: `open_buffer` puts a
            // file in front of you, and `edit_file` opens the one it is
            // proposing to change so the diff has something to sit beside.
            if matches!(call.name.as_str(), "open_buffer" | "edit_file") && answer.is_ok() {
                events.push(self.buffers_event());
                events.extend(self.after_action(true));
            }
            if call.name == "edit_file" {
                events.push(self.review_event());
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

        // The turn ended between the start of this poll and its end, which is
        // the only moment the next task may start: two turns in flight would
        // interleave into one transcript.
        if was_busy && !self.agent.is_busy() {
            // The answer is complete: write it down. This is the other half of
            // saving on `Ask` — between them, nothing said is ever only in
            // memory.
            events.extend(self.save_chat());
            events.extend(self.task_finished());
        } else if was_busy {
            // Still going. The row says what it is doing, which is the whole
            // reason it is a queue and not a spinner.
            let last = self.agent.transcript().last();
            let note = last.map(|e| first_sentence(&e.text)).unwrap_or_default();
            let tools = last.map_or(0, |e| e.calls.len());
            self.tasks.progress(&note, tools);
            events.push(self.tasks_event());
        }
        events
    }

    /// Append whatever the question pointed at.
    ///
    /// The typed words are kept exactly as written and the context follows
    /// under headings. Substituting `@selection` for the selected text inline
    /// would make a short question unreadable in the pane it is drawn in, and
    /// would leave the reader unable to tell which half of the sentence they
    /// wrote.
    fn with_context(&self, text: &str) -> String {
        let mentions = crate::mention::found(text);
        if mentions.is_empty() {
            return text.to_string();
        }

        let mut out = text.to_string();
        for mention in mentions {
            out.push_str("\n\n# ");
            out.push_str(mention.word());
            out.push('\n');
            out.push_str(&match mention {
                crate::mention::Mention::Selection => self.selected_text(),
                crate::mention::Mention::Dataframe => self.selected_frame(),
            });
        }
        out
    }

    /// The text selected in the focused editor.
    fn selected_text(&self) -> String {
        let buffer = self.editor.buffer();
        let cursor = buffer.cursor;
        if !cursor.has_selection() {
            // Said rather than left empty. Silence would leave the model
            // answering about whatever it could find and the user believing it
            // had been shown something.
            return "Nothing is selected.".into();
        }

        let range = cursor.selection();
        let text = buffer.slice(range);
        let from = buffer.position(range.start).line + 1;
        let to = buffer.position(range.end).line + 1;
        let name = buffer.name();
        // Fenced, and told where it came from: a fragment with no file and no
        // line numbers is a fragment the model cannot go back to.
        format!("`{name}` lines {from}-{to}\n\n```\n{text}\n```")
    }

    /// The table the selected notebook cell produced.
    fn selected_frame(&self) -> String {
        if !self.data.is_open() {
            return "No notebook is open.".into();
        }
        let cells = self.data.cells();
        let Some(cell) = cells.get(self.data.selected()) else {
            return "No cell is selected.".into();
        };

        let table = cell.outputs.iter().find_map(|output| match output {
            crate::data::OutputView::Table { table } => Some(table),
            _ => None,
        });
        let Some(table) = table else {
            return format!(
                "Cell {} produced no table. Run a cell that ends in a dataframe.",
                self.data.selected()
            );
        };

        // The shape the frame reported rather than the number of rows sent: a
        // model told about thirty rows of a million would reason about the
        // thirty as if they were all of it.
        let rows = table.total.unwrap_or(table.rows.len());
        let mut out = format!(
            "Cell {}: {} rows x {} columns, showing {}.\n\n",
            self.data.selected(),
            rows,
            table.columns.len(),
            table.rows.len()
        );
        out.push_str(&table.columns.join("\t"));
        out.push('\n');
        for row in &table.rows {
            out.push_str(&row.join("\t"));
            out.push('\n');
        }
        out
    }

    /// Run one tool for the agent, which may or may not answer now.
    ///
    /// The agent's entry point; `run_tool` is the synchronous half and is
    /// still what everything else calls. Split so that the loop asks one
    /// question — "is there an answer yet?" — rather than knowing which tools
    /// are slow.
    pub fn call_tool(&mut self, id: &str, name: &str, input: &serde_json::Value) -> ToolOutcome {
        if name != "run_cell" {
            return ToolOutcome::Answered(self.run_tool(name, input));
        }
        match self.start_cell(input) {
            Ok(cell) => {
                self.waiting.push(Waiting {
                    id: id.to_string(),
                    cell,
                    beats: 0,
                });
                ToolOutcome::Deferred
            }
            // A start that failed has to answer, or the turn waits for ever on
            // a cell that never began.
            Err(why) => ToolOutcome::Answered(Err(why)),
        }
    }

    /// How many cell runs are waiting to be answered.
    pub fn waiting_on_cells(&self) -> usize {
        self.waiting.len()
    }

    /// Start a cell running, answering which one.
    ///
    /// Opens the notebook in Data mode if it is not the one open: a cell
    /// running in a notebook nobody can see is a side effect with no window
    /// onto it, and the outputs have to land somewhere the user can look.
    fn start_cell(&mut self, input: &serde_json::Value) -> Result<usize, String> {
        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or("run_cell: `path` is required")?;
        let cell = input
            .get("cell")
            .and_then(|v| v.as_u64())
            .ok_or("run_cell: `cell` is required")? as usize;

        let full = self.pickers.resolve(path);
        if self.data.path() != Some(full.as_path()) {
            self.data.open(&full)?;
        }

        let cwd = self.pickers.root().to_path_buf();
        let python = crate::data::python(&cwd)
            .ok_or("no python to run a notebook with — set PROTON_PYTHON")?;
        self.data.run(cell, &python, &cwd)?;
        Ok(cell)
    }

    /// Answer every `run_cell` whose cell has stopped running.
    fn answer_finished_cells(&mut self) -> Vec<Event> {
        if self.waiting.is_empty() {
            return Vec::new();
        }

        let cells = self.data.cells();
        let problem = self.data.problem().map(str::to_string);
        let mut done: Vec<(String, Result<String, String>)> = Vec::new();
        self.waiting.retain_mut(|waiting| {
            let Some(view) = cells.get(waiting.cell) else {
                // Deleted while it ran. Saying so beats waiting forever for a
                // cell that is not there.
                done.push((
                    waiting.id.clone(),
                    Err(format!("cell {} is no longer there", waiting.cell)),
                ));
                return false;
            };
            if !view.running {
                // A kernel that died stops the cell without running it, and
                // "ran; no output" would be a lie about why there is nothing.
                // Reported as a failure because this time the *tool* did
                // break, unlike a cell that raised.
                done.push(match problem.clone() {
                    Some(why) => (waiting.id.clone(), Err(why)),
                    None => (waiting.id.clone(), Ok(cell_result(view))),
                });
                return false;
            }
            waiting.beats += 1;
            if waiting.beats >= PATIENCE {
                // Left running rather than interrupted: it is the user's
                // kernel and their code, and killing it to unblock a turn
                // would be the agent deciding that its own progress matters
                // more than what they were computing.
                done.push((
                    waiting.id.clone(),
                    Ok(format!(
                        "cell {} is still running and has been left running. What it \
                         had printed so far:\n{}",
                        waiting.cell,
                        cell_result(view)
                    )),
                ));
                return false;
            }
            true
        });

        for (id, answer) in done {
            self.agent.answer(&id, answer);
        }
        vec![self.notebook_event()]
    }

    /// The notebook as it is *now*.
    ///
    /// From Data mode's copy when it is the one open, because that is what the
    /// user is looking at and it may hold cells they have typed and not saved.
    /// Proposing a diff against the file on disk instead would be a diff
    /// against a notebook neither of us is looking at — the same reason
    /// `edit_file` reads from the buffer.
    fn notebook_now(&self, path: &Path) -> Result<proton_data::notebook::Notebook, String> {
        if self.data.path() == Some(path) {
            if let Some(text) = self.data.to_text() {
                return proton_data::notebook::Notebook::parse(&text);
            }
        }
        proton_data::notebook::Notebook::read(path)
    }

    /// Run one tool against the workspace.
    ///
    /// Public because the tools are the *editor's* capabilities rather than
    /// the agent's: `grep` is the index Telescope searches and `read_file`
    /// reads the buffers the user has open. The agent is the first caller and
    /// a plugin will be the second.
    pub fn run_tool(&mut self, name: &str, input: &serde_json::Value) -> Result<String, String> {
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

            "list_sources" => {
                if self.library.roots().is_empty() {
                    return Ok("there is no library: the user has not listed any roots in                                Settings › Library"
                        .into());
                }
                let sources = self.library.sources();
                if sources.is_empty() {
                    return Ok("the library is empty".into());
                }
                Ok(sources
                    .iter()
                    .map(|s| {
                        format!(
                            "{}  ({} document{}, {} passages)",
                            s.path,
                            s.documents,
                            if s.documents == 1 { "" } else { "s" },
                            s.passages
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(
                        "
",
                    ))
            }

            "search_library" => {
                let query = text("query")?;
                if self.library.roots().is_empty() {
                    return Ok("there is no library: the user has not listed any roots in \
                               Settings › Library, so there are no notes to search"
                        .into());
                }
                // A walk takes about a second on a vault of a thousand notes,
                // and it started when the session did — so this is only ever
                // hit by a question asked immediately after launch. Saying so
                // is better than answering "nothing found" about a library
                // that has not been read yet.
                if !self.library.is_built() {
                    return Ok("the library is still being read — ask again in a moment".into());
                }

                // The user's own narrowing, unless the model named its own.
                // Their choice is the default because they made it in a panel
                // they can see; a model that wants one subject can still ask.
                let asked = input
                    .get("sources")
                    .and_then(|v| v.as_array())
                    .map(|list| {
                        list.iter()
                            .filter_map(|v| v.as_str())
                            .map(str::to_string)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let scope = match asked.is_empty() {
                    true => self.library_scope.clone(),
                    false => proton_library::Scope::of(asked),
                };

                let hits = self.library.search_in(&query, count("limit", 6), &scope);
                if hits.is_empty() {
                    // "Nothing found" and "not all of it has been read yet"
                    // are different answers, and a walk over a corpus of PDFs
                    // runs for minutes after the notes are already searchable.
                    let (read, found) = self.library.progress();
                    let more = match self.library.is_building() {
                        true => format!(
                            " — still reading, {read} of {found} documents so far, \
                             so this may not be the whole library"
                        ),
                        false => String::new(),
                    };
                    // Which shelf was asked, when it was not all of them: the
                    // likeliest reason an answer is empty is that the question
                    // went to the wrong one.
                    let where_ = match scope.is_everything() {
                        true => format!(" ({} passages searched)", self.library.len()),
                        false => format!(" within {}", scope.prefixes().join(", ")),
                    };
                    return Ok(format!(
                        "nothing in the library about {query:?}{where_}{more}"
                    ));
                }
                // The citation on its own line above the passage, so what to
                // quote back is unambiguous even after the text runs to
                // sixty lines.
                Ok(hits
                    .iter()
                    .map(|hit| format!("[[{}]]\n{}", hit.cite, hit.passage.text))
                    .collect::<Vec<_>>()
                    .join("\n\n---\n\n"))
            }

            "open_buffer" => {
                let path = text("path")?;
                self.editor
                    .open(self.pickers.resolve(&path))
                    .map_err(|err| format!("{path}: {err}"))?;
                self.resync();
                Ok(format!("opened {path}"))
            }

            "edit_file" => {
                let path = self.pickers.resolve(&text("path")?);
                let old = text("old_text")?;
                let new = text("new_text")?;

                // The file as it is *now*, from the buffer when one is open —
                // an agent editing what is on disk while the user has unsaved
                // changes in front of them would propose a diff against a
                // file neither of them is looking at.
                let existing = self
                    .editor
                    .buffers()
                    .iter()
                    .find(|b| b.path() == Some(path.as_path()))
                    .map(|b| b.contents())
                    .or_else(|| std::fs::read_to_string(&path).ok());

                let (current, creates) = match (existing, old.is_empty()) {
                    (Some(text), _) => (text, false),
                    // An empty `old_text` on a file that is not there is how
                    // a new one is asked for.
                    (None, true) => (String::new(), true),
                    (None, false) => return Err(format!("{}: no such file", path.display())),
                };

                // A second edit to a file already staged applies to what was
                // staged, not to the buffer. Several calls in one turn are
                // ordinary — a rename touches three places — and each one
                // starting again from the buffer would leave only the last.
                let base = self
                    .review
                    .get(&path)
                    .map(|change| change.target.clone())
                    .unwrap_or_else(|| current.clone());

                let target = match creates && base.is_empty() {
                    true => new.clone(),
                    false => review::substitute(&base, &old, &new)?,
                };
                if target == current {
                    return Ok("that is already what the file says".into());
                }

                // Opened so the change can be looked at. Staging something
                // invisible and asking to have it reviewed is not a review.
                self.editor
                    .open_or_create(&path, &current)
                    .map_err(|err| format!("{}: {err}", path.display()))?;
                self.review.stage(&path, target, creates);
                self.resync();

                let hunks = self.review.hunks(&path, &current).len();
                Ok(format!(
                    "staged {hunks} hunk{} in {} for review — nothing is applied or written \
                     until the user accepts it",
                    if hunks == 1 { "" } else { "s" },
                    path.display()
                ))
            }

            "read_notebook" => {
                let path = self.pickers.resolve(&text("path")?);
                let notebook = self.notebook_now(&path)?;
                let cells = notebook.cells();
                if cells.is_empty() {
                    return Ok(format!("{} has no cells", path.display()));
                }

                let from = count("from", 0).min(cells.len().saturating_sub(1));
                let to = count("to", cells.len().saturating_sub(1)).min(cells.len() - 1);
                let want_outputs = input
                    .get("outputs")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                let mut out = String::new();
                for (index, cell) in cells.iter().enumerate().take(to + 1).skip(from) {
                    let count = cell
                        .execution_count
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| " ".into());
                    out.push_str(&format!(
                        "# cell {index} [{count}] {}\n{}\n",
                        match cell.kind {
                            proton_data::notebook::CellKind::Code => "code",
                            proton_data::notebook::CellKind::Markdown => "markdown",
                            _ => "raw",
                        },
                        cell.source
                    ));
                    if want_outputs && !cell.outputs.is_empty() {
                        // Truncated hard: a cell that printed a dataframe or a
                        // traceback can be thousands of lines, and a notebook
                        // of those would fill the window with what the model
                        // has already read once.
                        out.push_str(&format!("## out\n{}\n", short_output(&cell.outputs)));
                    }
                    out.push('\n');
                }
                Ok(out)
            }

            "edit_cell" => {
                let path = self.pickers.resolve(&text("path")?);
                let index = count("cell", usize::MAX);
                if index == usize::MAX {
                    return Err("edit_cell: `cell` is required".into());
                }
                let action = input
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("replace");

                // Staged on top of anything already staged for this file, the
                // way `edit_file` does: several cell edits in one turn is the
                // ordinary case, and each one starting from the file on disk
                // would leave only the last.
                let current = self.notebook_now(&path)?.to_text();
                let base = self
                    .review
                    .get(&path)
                    .map(|change| change.target.clone())
                    .unwrap_or_else(|| current.clone());
                let mut notebook = proton_data::notebook::Notebook::parse(&base)?;

                let what = match action {
                    "delete" => {
                        if !notebook.remove(index) {
                            return Err(format!("there is no cell {index}"));
                        }
                        format!("deleted cell {index}")
                    }
                    "insert" => {
                        let kind = match input.get("kind").and_then(|v| v.as_str()) {
                            Some("markdown") => proton_data::notebook::CellKind::Markdown,
                            _ => proton_data::notebook::CellKind::Code,
                        };
                        let at = notebook.insert(index, kind, &text("source")?);
                        format!("inserted a cell at {at}")
                    }
                    "replace" => {
                        if !notebook.set_source(index, &text("source")?) {
                            return Err(format!("there is no cell {index}"));
                        }
                        format!("rewrote cell {index}")
                    }
                    other => return Err(format!("edit_cell: unknown action {other:?}")),
                };

                let target = notebook.to_text();
                if target == current {
                    return Ok("that is already what the cell says".into());
                }

                // Opened so the change can be looked at, exactly as `edit_file`
                // does — staging something invisible and asking to have it
                // reviewed is not a review.
                self.editor
                    .open_or_create(&path, &current)
                    .map_err(|err| format!("{}: {err}", path.display()))?;
                self.review.stage(&path, target, false);
                self.resync();

                Ok(format!(
                    "{what} in {} — staged for review; nothing is written until the user \
                     accepts it",
                    path.display()
                ))
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

            // Declared to the model and dispatched by `call_tool`, which is
            // where a tool that cannot answer immediately has to go. Saying
            // "no such tool" here would be a lie about one that is offered.
            "run_cell" => Err(
                "run_cell is started by the agent loop rather than run here — call \
                 call_tool for it"
                    .into(),
            ),

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
            provider: self.agent.provider().id().to_string(),
            model: self.agent.model().to_string(),
            credential: self.agent.credential(),
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
        let cell = self.data.selected();
        let view = self.splits.view_mut();
        view.cursor = cursor;
        view.buffer = buffer;
        // Parked for every mode, not just Data. A view is one window onto the
        // workspace and remembers where it was in each kind of document, so
        // splitting in Codex and switching to Data finds each pane where it
        // left off rather than all of them on cell zero.
        view.cell = cell;
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
        // Clamped inside `set_selected`: cells can be deleted while a view is
        // not looking, and a selection past the end panics later.
        self.data.set_selected(view.cell);
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
        // Data's panes differ only in which cell they are on, so a focus
        // change moves the selection without changing a character of the
        // notebook. Without this the highlight stays on the pane you left.
        if self.data.is_open() {
            events.push(self.notebook_event());
        }
        events
    }

    /// The content and cursor of every split.
    fn every_view(&mut self) -> Vec<Event> {
        (0..self.splits.len())
            .flat_map(|view| [self.viewport_of(view), self.cursor_of(view)])
            .collect()
    }

    /// Point the library at whatever roots the settings name.
    ///
    /// `library.roots` is an array of directories, written by hand or by the
    /// settings panel. Everything about it is deliberately explicit: no root
    /// is guessed, none is added because a directory looked like a vault, and
    /// removing one here removes its documents from the next answer.
    ///
    /// A leading `~/` is expanded and a relative path is taken against the
    /// project, because those are the two ways somebody writes a path by hand
    /// and neither should silently mean "the directory the process started
    /// in".
    fn follow_library_settings(&mut self) {
        let roots = self.settings.library_roots(self.pickers.root());

        // Where extracted PDF text is kept. Set before the roots, or the walk
        // the next line starts extracts everything again and throws it away.
        self.library.cache_in(proton_library::pdf::cache_dir());
        // On by default: the documents somebody keeps are as much their PDFs
        // as their notes, and the cache means the minutes it costs are paid
        // once rather than at every launch.
        self.library
            .set_pdfs(self.settings.bool("library.pdfs", true));
        self.library.set_roots(roots);
        self.library_scope = proton_library::Scope::of(
            self.settings
                .get("library.scope")
                .and_then(|v| v.as_array())
                .map(|list| {
                    list.iter()
                        .filter_map(|v| v.as_str())
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        );
    }

    /// Point the agent at whatever the settings name.
    ///
    /// Called on startup and after any `agent.*` change, so switching provider
    /// takes effect on the next question rather than the next launch.
    fn follow_agent_settings(&mut self) {
        let provider = proton_agent::Provider::parse(&self.settings.string("agent.provider", ""));
        let model = self.settings.string("agent.model", "");
        self.agent.use_provider(provider, &model);
    }

    /// Write the open conversation, naming its file if it has none yet.
    ///
    /// Quiet on success: this runs after every question and every answer, and
    /// a notice each time would be a notice nobody reads. A *failure* is worth
    /// saying, because the alternative is believing the work is saved.
    fn save_chat(&mut self) -> Vec<Event> {
        let messages = self.agent.conversation().to_vec();
        if messages.is_empty() {
            return Vec::new();
        }

        let title = self
            .agent
            .transcript()
            .iter()
            .find(|entry| entry.speaker == crate::agent::Speaker::You)
            .map(|entry| crate::chats::title_from(&entry.text))
            .unwrap_or_default();

        let path = match self.chat_path.clone() {
            Some(path) => path,
            None => {
                let fresh = crate::chats::fresh_path(self.pickers.root(), &title);
                self.chat_path = Some(fresh.clone());
                fresh
            }
        };

        // The title is only set when the file has none: renaming is a
        // deliberate act and must not be undone by the next answer.
        let existing = crate::chats::read(&path).ok();
        let chat = crate::chats::Chat {
            title: match existing.as_ref().map(|c| c.title.clone()) {
                Some(kept) if !kept.is_empty() => kept,
                _ => title,
            },
            provider: self.agent.provider().id().to_string(),
            model: self.agent.model().to_string(),
            messages,
            ..crate::chats::Chat::default()
        };

        match crate::chats::write(&path, &chat) {
            Ok(()) => vec![self.chats_event()],
            Err(why) => vec![notice(why, NoticeLevel::Error)],
        }
    }

    fn chats_event(&self) -> Event {
        Event::Chats {
            conversations: crate::chats::list(self.pickers.root()),
            current: self
                .chat_path
                .as_ref()
                .and_then(|path| path.strip_prefix(self.pickers.root()).ok())
                .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default(),
        }
    }

    fn settings_event(&self) -> Event {
        Event::Settings {
            values: self.settings.all().clone(),
            path: self.settings.path().display().to_string(),
        }
    }

    /// What the library has read, for the panel that lists the roots.
    fn library_event(&self) -> Event {
        Event::Library {
            roots: self
                .library
                .roots()
                .iter()
                .map(|root| root.display().to_string())
                .collect(),
            missing: self
                .library
                .missing()
                .iter()
                .map(|root| root.display().to_string())
                .collect(),
            documents: self.library.documents(),
            passages: self.library.len(),
            indexing: self.library.is_building(),
            pdfs: self.library.reads_pdfs(),
            read: self.library.progress().0,
            found: self.library.progress().1,
            elapsed_ms: self
                .library
                .elapsed()
                .map(|took| took.as_millis() as u64)
                .unwrap_or_default(),
            sources: self
                .library
                .sources()
                .into_iter()
                .map(|source| crate::bus::LibrarySource {
                    path: source.path,
                    label: source.label,
                    documents: source.documents,
                    passages: source.passages,
                })
                .collect(),
            scope: self.library_scope.prefixes().to_vec(),
        }
    }

    /// What Design mode's inspector draws.
    fn theme_event(&self) -> Event {
        Event::Theme {
            path: self
                .design
                .path()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            tokens: self.design.tokens(),
            selected: self.design.selected(),
            problem: self.design.problem().unwrap_or_default().to_string(),
        }
    }

    fn splits_event(&self) -> Event {
        Event::Splits {
            views: self
                .splits
                .views()
                .iter()
                .enumerate()
                .map(|(index, view)| SplitView {
                    buffer: view.buffer,
                    // The focused view's cell is on `Data`, not on the view —
                    // the view's copy is only written when focus leaves, so
                    // reading it here would draw the focused pane wherever it
                    // was last time it lost focus.
                    cell: match index == self.splits.focused() {
                        true => self.data.selected(),
                        false => view.cell,
                    },
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
                scrollback: 0,
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
            scrollback: screen.scrollback,
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

    /// The notebook that is open, and what its cells have produced.
    fn notebook_event(&self) -> Event {
        Event::Notebook {
            path: self
                .data
                .path()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            cells: self.data.cells(),
            problem: self.data.problem().unwrap_or_default().to_string(),
        }
    }

    /// What the repository has right now.
    ///
    /// Asked of git each time rather than kept, which is the same discipline
    /// the gutter and the file tree follow. Git's answer moves for reasons
    /// that never pass through this core — a commit in the terminal panel, a
    /// branch switched from outside — so a remembered list would be stale
    /// exactly when somebody looked at it to find out what had happened.
    fn repo_event(&self) -> Event {
        Event::Repo {
            branch: self.git.branch(),
            files: self
                .git
                .changes()
                .into_iter()
                .map(|change| crate::bus::RepoFile {
                    path: change.path,
                    state: change.state.letter().to_string(),
                })
                .collect(),
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
        // The layout goes with it. A mode change that only moved a highlight
        // on the rail would be a theme rather than a mode, and the panels
        // would still be the last mode's.
        let mut events = vec![
            Event::ModeChanged { mode },
            Event::Layout {
                layout: mode.layout(),
            },
        ];

        // Design with nothing open goes looking, once. Asking somebody to
        // find their own stylesheet before the inspector will show anything
        // is a worse first run than guessing at the conventional name and
        // saying which one was picked.
        if mode == ModeId::Design && !self.design.is_open() {
            if let Some(path) = crate::design::discover(self.pickers.root()) {
                if self.design.open(&path).is_ok() {
                    events.push(notice(
                        format!("theme: {}", path.display()),
                        NoticeLevel::Info,
                    ));
                }
            }
            events.push(self.theme_event());
        }
        events
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

        // The verb table, where it stops being a table. `dd` in Codex deletes
        // a line and in Data deletes a cell, and the engine above knows about
        // neither — `Object::Line` has always meant "the mode's unit", which
        // is what makes this a routing decision rather than a second engine.
        if self.mode == ModeId::Data {
            let changed = self.data.execute(action);
            let mut events = vec![Event::Action {
                action: action.clone(),
                description,
            }];
            if changed || matches!(action.verb, Verb::Move | Verb::Yank) {
                events.push(self.notebook_event());
            }
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
        let spans = self.highlights.spans(
            &self.editor.buffers()[buffer],
            first_line,
            count,
            self.plugins.grammars(),
        );
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

/// How much of a passage travels with a picker row.
///
/// Enough to preview — a few lines, which is what says whether this is the
/// passage you wanted — and not the whole thing. Twenty rows of a sixty-line
/// passage would be sixty kilobytes crossing the bus on every keystroke, to
/// draw a pane that shows about eight lines.
const PREVIEW: usize = 400;

/// The readable part of a passage, for a picker row and its preview.
///
/// Line breaks are kept, because the preview draws them; the front end takes
/// the first line for the row itself. Headings are dropped: they are already
/// in the citation beside this, and repeating them would spend the row saying
/// what the row already says.
fn snippet(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(line.trim_start());
        if out.chars().count() >= PREVIEW {
            break;
        }
    }
    // Cut on a character boundary, and say that it was cut.
    match out.char_indices().nth(PREVIEW) {
        Some((at, _)) => format!("{}…", out[..at].trim_end()),
        None => out,
    }
}

/// Hand a file to whatever the desktop opens it with.
///
/// The only place Proton starts something that is not Proton, and it exists
/// for one case: a citation into a PDF. There is no PDF viewer here and
/// opening one as text would fill a buffer with binary, so following that
/// citation means handing the file to the reader the user already has.
///
/// No shell is involved. The path goes to the program as one argument rather
/// than through `cmd /c`, so a filename with a quote or an `&` in it is a
/// filename rather than a second command — and the caller has already
/// established that the path is a real file inside a listed library root.
fn open_outside(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("{}: no such file", path.display()));
    }
    // `explorer` rather than `cmd /c start`: it takes the path as an argument
    // and opens it with its registered handler. Its exit status is famously
    // not zero on success, so it is not checked — what matters is that
    // spawning it worked.
    let program = match () {
        _ if cfg!(windows) => "explorer",
        _ if cfg!(target_os = "macos") => "open",
        _ => "xdg-open",
    };
    std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("could not open {}: {err}", path.display()))
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
/// An answer's first sentence, for a one-line summary of a task.
///
/// A row in a queue is a row: the transcript is where the answer is read, and
/// this only has to be enough to tell one task from another at a glance.
/// What a run produced, for the tool result.
///
/// An error is reported as text rather than as a failed tool call: the cell
/// ran, and a traceback is the answer to "what does this do" as much as a
/// number is. Marking it failed would tell the model the *tool* broke.
fn cell_result(view: &crate::data::CellView) -> String {
    if view.outputs.is_empty() {
        return "ran; no output".to_string();
    }
    let mut text = String::new();
    for output in &view.outputs {
        match output {
            crate::data::OutputView::Stream { text: it, .. } => text.push_str(it),
            crate::data::OutputView::Text { text: it } => text.push_str(it),
            crate::data::OutputView::Error {
                ename,
                evalue,
                traceback,
            } => {
                text.push_str(&format!("{ename}: {evalue}\n"));
                text.push_str(&traceback.join("\n"));
            }
            crate::data::OutputView::Table { table } => {
                text.push_str(&format!(
                    "table: {} columns x {} rows shown{}",
                    table.columns.len(),
                    table.rows.len(),
                    table.total.map(|n| format!(" of {n}")).unwrap_or_default()
                ));
            }
            crate::data::OutputView::Image { mime, .. } => {
                // Not the bytes: a base64 PNG is tens of thousands of tokens
                // and the model cannot see it anyway.
                text.push_str(&format!("<{mime}>"));
            }
        }
        text.push('\n');
    }

    const LINES: usize = 40;
    let lines: Vec<&str> = text.lines().collect();
    match lines.len() > LINES {
        true => format!(
            "{}\n… {} more lines",
            lines[..LINES].join("\n"),
            lines.len() - LINES
        ),
        false => text.trim_end().to_string(),
    }
}

/// What a cell printed, in few enough lines to be worth sending.
///
/// A dataframe or a traceback can be thousands of lines. The model has to know
/// what happened, not read all of it, and a notebook of untruncated outputs
/// would fill the context with what it has already seen once.
fn short_output(outputs: &[proton_data::notebook::Output]) -> String {
    const LINES: usize = 20;
    let mut text = String::new();
    for output in outputs {
        match output {
            proton_data::notebook::Output::Stream { text: it, .. } => text.push_str(it),
            proton_data::notebook::Output::Error {
                ename,
                evalue,
                traceback,
            } => {
                // The traceback matters more than anything else a cell can
                // print, so it is kept whole up to the line budget rather than
                // summarised to its first line.
                text.push_str(&format!("{ename}: {evalue}\n"));
                text.push_str(&traceback.join("\n"));
            }
            proton_data::notebook::Output::Data { data, .. } => {
                if let Some(plain) = data.get("text/plain").and_then(|v| v.as_str()) {
                    text.push_str(plain);
                } else {
                    let kinds: Vec<&str> = data.keys().map(String::as_str).collect();
                    text.push_str(&format!("<{}>", kinds.join(", ")));
                }
            }
        }
        text.push('\n');
    }

    let lines: Vec<&str> = text.lines().collect();
    match lines.len() > LINES {
        true => format!(
            "{}\n… {} more lines",
            lines[..LINES].join("\n"),
            lines.len() - LINES
        ),
        false => text.trim_end().to_string(),
    }
}

fn first_sentence(text: &str) -> String {
    const MOST: usize = 90;
    let text = text.trim();
    let end = text
        .find(". ")
        .map(|at| at + 1)
        .or_else(|| text.find('\n'))
        .unwrap_or(text.len());
    let line: String = text[..end].chars().take(MOST).collect();
    match line.chars().count() < text[..end].chars().count() {
        true => format!("{line}…"),
        false => line,
    }
}

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

/// What a workspace had open, in a shape that survives being written down.
///
/// Paths rather than buffer handles, and an index into `files` rather than a
/// buffer index, because the numbering on the other side of a restart is not
/// the numbering that was saved: a file that no longer opens shifts every
/// buffer after it.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Layout {
    pub files: Vec<PathBuf>,
    #[serde(default)]
    pub views: Vec<StoredView>,
    #[serde(default)]
    pub focused: usize,
    #[serde(default)]
    pub vertical: bool,
}

/// One pane, as stored.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct StoredView {
    /// Which of `files` it was showing.
    pub file: usize,
    #[serde(default)]
    pub first_line: usize,
    /// The cursor, as a character offset into that file.
    #[serde(default)]
    pub line: usize,
}

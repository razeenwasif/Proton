//! Workspace state — the thing the shell is a view of.
//!
//! `apply` is deliberately total: one command in, a list of events out. The
//! only I/O is opening and saving a file, which is what a command asked for.
//! No clock, no threads, no windowing — that is what makes the core testable
//! headless and lets a second front end reuse it verbatim.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

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
    /// The last selection, for `gv`: which buffer, its two ends, and whether
    /// it was charwise, linewise or a block.
    last_visual: Option<(usize, usize, usize, VimMode)>,
    /// The views onto the buffers. Always at least one; what used to be a
    /// single viewport is the focused one of these.
    splits: Splits,
    pickers: Pickers,
    /// The project folder under both of its names, for showing a path that
    /// arrived through either one relative to it. See [`crate::folder::Root`].
    root: crate::folder::Root,
    servers: Servers,
    highlights: Highlighters,
    popup: Popup,
    agent: Agent,
    visualizer: Visualizer,
    tree: Tree,
    /// The gutter, and what git has to say about the project.
    git: Git,
    /// The project's changes as git last gave them, and an asking in flight:
    /// a status walks the working tree, so it is asked on a worker rather
    /// than with everything waiting on the editor.
    repo_asked: std::cell::RefCell<RepoAsked>,
    /// Breakpoints, and the debug session when there is one.
    debugger: Debugger,
    /// The terminal, once something has asked for one. Not started on boot:
    /// a shell nobody opened is a process nobody wanted.
    terminal: Option<proton_term::Terminal>,
    /// Each row of the terminal's screen as the window was last sent it, by a
    /// hash: what a poll sends is only the rows that are not.
    term_sent: Vec<u64>,
    /// The notebook Data mode has open, and the kernel running it.
    data: crate::data::Data,
    /// The workbook open in Excel mode, with its history.
    sheets: crate::sheet::Sheets,
    /// The circuit project open in Circuit mode.
    circuits: crate::circuit::Circuits,
    settings: crate::settings::Settings,
    /// Polled while another project is on screen: what it hears is taken in,
    /// but its notebook, terminal and conversation are not drawn into events
    /// nobody will see — switching back draws them all.
    quiet: bool,
    /// What the agent's last event showed, so a heartbeat on which nothing
    /// changed sends nothing — while a turn ran, every one sent the whole
    /// conversation, and kept the heartbeat at its fastest.
    agent_shown: Option<u64>,
    /// How the agent signs in, as the pane says it: asked of the disk once a
    /// provider and kept, where every agent event asked it again.
    credential: std::cell::RefCell<Option<(String, String)>>,
    /// What each file's `.editorconfig` says about its indentation, read the
    /// first time it is needed and forgotten when an `.editorconfig` is
    /// written here.
    editorconfigs: std::collections::HashMap<PathBuf, crate::typing::Config>,
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
    /// The local model that turns a question into a vector, when the settings
    /// name one. `None` is lexical search, which is a complete answer.
    embedder: Option<proton_library::Embedder>,
    /// Library searches waiting on a query vector.
    asking: Vec<Asking>,
    /// A council debate in flight, and what it has said so far. Two minutes of
    /// three subscriptions, so the rounds are shown as they land rather than
    /// all at the end.
    council: Option<proton_council::Running>,
    /// The Chat tab: Claude Code's sessions for this project, and the ones
    /// running here.
    claude: crate::claude::Claude,
    /// The investigations, in the Research tab: the project's own, in its
    /// `research/`, or the session's, where the person keeps them in a
    /// folder of their own and every project shows the same ones.
    research: Arc<Mutex<crate::research::Research>>,
    /// Whether `research` is the session's. The session polls and stops a
    /// shared one itself, once; were every workspace to poll it, what a
    /// round said could be drained by a project in the background and
    /// dropped with the rest of that project's events.
    research_shared: bool,
    /// The files opened in the document viewer: the only ones the shell's
    /// `read_document` will read for the page.
    documents: std::collections::BTreeSet<PathBuf>,
    council_progress: Vec<String>,
    council_round: usize,
    /// The brief on screen, and which past debate it came from.
    council_brief: String,
    council_showing: String,
    council_problem: String,
    /// Tool calls waiting on a kernel.
    ///
    /// A cell run is the one tool that cannot answer when it is asked: the
    /// tool loop is synchronous and a kernel is not. The turn stays paused
    /// while these sit here, because `Agent::resume` only continues once every
    /// outstanding call has an answer — so deferring one is simply not
    /// answering it yet.
    waiting: Vec<Waiting>,
    /// Each entry of the conversation as the window was last sent it, by a
    /// hash: so a streaming answer sends the entries from the first that
    /// changed rather than the whole conversation on every token.
    agent_sent: std::cell::RefCell<Vec<u64>>,
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
    /// clang-format's answer, when it is formatting instead of a server.
    external_format: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    /// clang-format, once looked for; a test names its own.
    clang_format: Option<Option<PathBuf>>,
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
    /// The cell, as the agent named it, and by the key that follows it if
    /// cells are added or deleted around it while it runs.
    cell: usize,
    key: u64,
    /// Heartbeats spent waiting.
    ///
    /// Beats rather than seconds, because the core has no clock. The heartbeat
    /// is nominally 50ms, so `PATIENCE` is about ten minutes — but this is a
    /// bound on how long one turn may be blocked, not a measurement of time,
    /// and it does not depend on that rate being exact.
    beats: u32,
}

/// A `search_library` waiting on the vector for its question.
///
/// Embedding one query against a local model measured at two seconds, and the
/// core answers a tool call on the heartbeat that delivers it — so doing it
/// inline would stop the editor dead for two seconds every time the agent
/// looked something up. It waits here instead, on exactly the mechanism a cell
/// run already uses.
#[derive(Debug)]
struct Asking {
    /// The tool call to answer once the vector lands.
    id: String,
    /// Everything needed to finish the search, kept because the library will
    /// be searched on a later beat than the one that asked.
    query: String,
    limit: usize,
    scope: proton_library::Scope,
    vector: std::sync::mpsc::Receiver<Result<Vec<f32>, String>>,
    beats: u32,
}

/// How many beats to wait for a query vector before searching without one.
///
/// Twenty seconds. Unlike a cell, there is a good answer available the moment
/// this runs out: the lexical search, which is what the library did before
/// vectors existed. So the bound is tight, and missing it costs a worse
/// ranking rather than a failure.
const PATIENCE_ASKING: u32 = 400;

/// How many beats to wait for a cell before giving up on it.
///
/// Generous on purpose: training a model or reading a large frame is a cell
/// that legitimately runs for minutes, and a bound tight enough to catch a
/// typo would break the work this is for. What it catches is the cell that
/// never finishes at all.
const PATIENCE: u32 = 12_000;

/// The project's changes as git last gave them, and an asking in flight.
#[derive(Debug, Default)]
struct RepoAsked {
    files: Vec<crate::bus::RepoFile>,
    asking: Option<std::sync::mpsc::Receiver<Vec<proton_vcs::Change>>>,
    again: bool,
}

/// An entry of the conversation, as a number that moves when it does.
fn entry_hash(entry: &crate::agent::Entry) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    entry.hash(&mut hash);
    hash.finish()
}

impl Workspace {
    pub fn new() -> Self {
        Self::rooted(".").with_language_servers()
    }

    /// The project root every picker searches. Separate from `new` so a test
    /// can point at a fixture instead of the process's working directory.
    pub fn rooted(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        // Resolved once and shared, so that the buffers, the review and what
        // is shown all agree on what the project's other name is.
        let folder = crate::folder::Root::new(root);
        let mut space = Self {
            editor: Editor::rooted(folder.clone()),
            review: review::Review::new(folder.clone()),
            root: folder,
            pickers: Pickers::new(root),
            servers: Servers::new(root),
            agent: Agent::new(&root.display().to_string()),
            claude: crate::claude::Claude::new(root),
            research: Arc::new(Mutex::new(crate::research::Research::new(root))),
            research_shared: false,
            documents: std::collections::BTreeSet::new(),
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
        space.follow_claude_settings();
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

    /// Whether this is polled in the background: see `quiet`.
    pub fn set_quiet(&mut self, quiet: bool) {
        self.quiet = quiet;
    }

    /// Put text at the cursor exactly as it is, as a paste does: none of the
    /// indentation or closing brackets typing it would add.
    pub fn paste_text(&mut self, text: &str) {
        let buf = self.editor.buffer_mut();
        let at = buf.cursor.offset;
        buf.apply(
            vec![proton_text::Edit::insert(at, text)],
            proton_text::Cursor::at(at + text.chars().count()),
        );
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
    /// thing that does not exist. Scratch panes remain scratch panes; their
    /// unsaved contents are not part of the layout store.
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
                .enumerate()
                .map(|(index, view)| StoredView {
                    file: moved.get(view.buffer).copied().flatten().unwrap_or(0),
                    scratch: moved.get(view.buffer).copied().flatten().is_none(),
                    first_line: view.first_line,
                    line: if index == self.splits.focused() {
                        self.editor.buffer().cursor.offset
                    } else { view.cursor.offset },
                })
                .collect(),
            focused: self.splits.focused(),
            vertical: self.splits.is_vertical(),
            chat: self
                .chat_path
                .as_ref()
                .and_then(|path| path.strip_prefix(self.pickers.root()).ok())
                .map(Path::to_path_buf),
        }
    }

    /// The conversation the agent has open, if it has been saved.
    pub fn open_chat(&self) -> Option<&Path> {
        self.chat_path.as_deref()
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
        let views: Vec<View> = layout
            .views
            .iter()
            .map(|stored| {
                let buffer = if stored.scratch { 0 } else {
                    opened.get(stored.file).copied().flatten().unwrap_or(0)
                };
                let mut view = View {
                    buffer,
                    first_line: stored.first_line.min(self.editor.buffers()[buffer].len_lines().saturating_sub(1)),
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
        self.restore_cursor();

        // The agent's conversation, if its file is still there. One deleted
        // since is passed over like a file that was, and the panel starts on
        // a new thread.
        if let Some(chat) = &layout.chat {
            let full = self.pickers.root().join(chat);
            if let Ok(saved) = crate::chats::read(&full) {
                self.agent.restore(saved.messages);
                self.chat_path = Some(full);
            }
        }
    }

    /// Whether anything in this workspace has unsaved changes. The session
    /// asks before closing it, because a closed workspace takes its buffers
    /// with it and there is no undo for that.
    pub fn is_modified(&self) -> bool {
        self.editor.buffers().iter().any(|b| b.is_modified()) || !self.sheets.unsaved().is_empty() || !self.circuits.unsaved().is_empty()
    }

    /// The files with unsaved changes, as the project names them.
    pub fn unsaved(&self) -> Vec<String> {
        let mut files: Vec<String> = self
            .editor
            .unsaved()
            .iter()
            .map(|buffer| match buffer.path() {
                Some(path) => self.root.shown(path),
                None => buffer.name(),
            })
            .collect();
        // So is a workbook.
        files.extend(self.sheets.unsaved());
        files.extend(self.circuits.unsaved());
        files
    }

    /// Save every file with unsaved changes; the ones that could not be, and
    /// why.
    pub fn save_unsaved(&mut self) -> Vec<String> {
        let failed = self.editor.save_unsaved();
        if failed.len() != self.editor.unsaved().len() {
            self.resync();
        }
        let mut failed: Vec<String> = failed.into_iter().map(|(name, why)| format!("{name}: {why}")).collect();
        failed.extend(self.sheets.save_unsaved());
        failed.extend(self.circuits.save_unsaved());
        failed
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
        let mut events = self.dispatch(command);
        events.extend(self.highlights.take_notices().into_iter().map(|text| notice(text, NoticeLevel::Warn)));
        if let Some(text) = self.editor.take_clipboard() {
            events.push(Event::Clipboard { text });
        }
        events
    }

    fn dispatch(&mut self, command: Command) -> Vec<Event> {
        // Passive paints and heartbeats do not divide typing. An explicit
        // command (click, save, mode/buffer/split switch, picker, etc.) does.
        // Sync also marks re-entry into a workspace after a project switch.
        if !matches!(
            &command,
            Command::Key { .. } | Command::Poll | Command::Viewport { .. }
        ) {
            self.editor.buffer_mut().end_group();
        }
        match command {
            Command::Sync => {
                // The front end connecting is the signal that the window is
                // up, which is the right moment to start walking the tree: not
                // during construction, where it would be behind the first
                // paint, and not on first `␣ff`, where it would be in front of
                // the user.
                //
                // Except over the home folder or anything broader. A launch
                // from the Start menu or the Finder lands there with nothing
                // to restore, and walking it would read everything kept on the
                // machine, at every such launch, without anybody having asked
                // for a file. There the walk waits until something does: a
                // picker over the files, or a search by the agent.
                if !crate::folder::is_too_broad(self.root(), crate::folder::home().as_deref()) {
                    self.pickers.warm();
                }
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
                    // This project's shell, or the empty screen that says it
                    // has none. A session switching projects syncs the one it
                    // switches to, and without this the panel went on showing
                    // the last project's shell while keystrokes went to this
                    // one — where, with no shell to take them, they vanished.
                    self.terminal_event(),
                    // A window that has just connected to a session with a
                    // notebook open would otherwise show an empty stage until
                    // something happened to change a cell.
                    self.notebook_event(),
                    self.settings_event(),
                    self.library_event(),
                    self.council_event(),
                    self.chats_event(),
                    Event::Layout {
                        layout: self.mode.layout(),
                    },
                ]);
                events.extend(self.claude.sync());
                events.extend(self.research().sync());
                // Excel's home page and workbook, when it is the mode on
                // screen: finding the workbooks walks the project.
                if self.mode == ModeId::Sheet {
                    let root = self.root().to_path_buf();
                    events.extend(self.sheets.events(&root));
                }
                if self.mode == ModeId::Circuit {
                    let root = self.root().to_path_buf();
                    events.extend(self.circuits.events(&root));
                }
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
                let buffer = self.editor.buffer_mut();
                let line = line.min(buffer.len_lines().saturating_sub(1));
                let at = buffer.offset_of(proton_text::Position { line, column });
                // A drag that has gone nowhere is a click: no selection, and
                // insert mode kept for somebody who clicked to type there.
                let visual = matches!(was, VimMode::Visual | VimMode::VisualLine | VimMode::VisualBlock);
                if !visual && buffer.clamp(buffer.cursor.moved_to(at)).offset == buffer.cursor.offset {
                    return self.after_action(false);
                }
                self.vim.enter_visual();
                let buffer = self.editor.buffer_mut();
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
                match self.data.run(index, &cwd) {
                    Ok(()) => vec![self.notebook_event()],
                    Err(why) => vec![notice(why, NoticeLevel::Error)],
                }
            }

            Command::SelectCell { index } => {
                self.data.select_cell(index);
                vec![Event::CellSelected { index: self.data.selected() }]
            }

            /* ── settings ──────────────────────────────────── */
            Command::SetSetting { key, value } => match {
                self.credential.replace(None);
                self.settings.set(&key, value)
            } {
                // Already had that value. The panel emits on every click and
                // most clicks change nothing.
                Ok(false) => Vec::new(),
                Ok(true) => {
                    if key.starts_with("agent.") {
                        self.follow_agent_settings();
                    }
                    if key.starts_with("claude.") || key.starts_with("chat.") {
                        self.follow_claude_settings();
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
                match self.data.run_all(&cwd) {
                    Ok(0) => vec![notice(
                        "nothing to run: no code cells".into(),
                        NoticeLevel::Info,
                    )],
                    Ok(n) => vec![
                        notice(format!("running {n} cells"), NoticeLevel::Info),
                        self.notebook_event(),
                    ],
                    Err(why) => vec![notice(why, NoticeLevel::Error)],
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

            Command::AnswerProposal { id, accept } => {
                let answered = match accept {
                    true => self.data.accept(id),
                    false => match self.data.reject(id) {
                        true => Ok(String::new()),
                        false => Err("that proposal has already been answered".into()),
                    },
                };
                match answered {
                    Ok(_) => vec![self.notebook_event()],
                    Err(why) => vec![notice(why, NoticeLevel::Warn), self.notebook_event()],
                }
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

            Command::Convene { question } => self.convene(&question),

            Command::OpenDebate { name } => {
                let at = self.council_room().join(&name);
                match std::fs::read_to_string(at.join("brief.md")) {
                    Ok(text) => {
                        self.council_brief = text;
                        self.council_problem.clear();
                    }
                    Err(_) => {
                        // No brief means it did not finish. The rounds are
                        // still there and are most of the value, so show those
                        // rather than an empty pane.
                        self.council_brief = ["01-positions.md", "02-responses.md"]
                            .iter()
                            .filter_map(|file| std::fs::read_to_string(at.join(file)).ok())
                            .collect::<Vec<_>>()
                            .join("\n\n");
                        self.council_problem =
                            "this debate has no brief \u{2014} its rounds are below".into();
                    }
                }
                self.council_showing = name;
                vec![self.council_event()]
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
                // One of the open investigation's papers, as the team cites
                // them: a page of its PDF opens the viewer there, its
                // abstract the paper. Anything else is the library's.
                let paper = self.research().cited(&cited.path);
                match paper {
                    Some(crate::research::Cited::Pdf(path)) => {
                        let page = (cited.place == proton_library::chunk::Place::Pages).then(|| u32::try_from(cited.from).ok()).flatten();
                        return vec![self.document_event(&path, ViewerKind::Pdf, page)];
                    }
                    Some(crate::research::Cited::Paper(id)) => return self.research().show_paper(Some(id)),
                    None => {}
                }
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
                // A page: the viewer shows a PDF at it. Anything else with
                // pages goes to whatever the person reads it with, since as
                // text it would be a screenful of binary in a buffer.
                if cited.place == proton_library::chunk::Place::Pages {
                    if let Some(kind) = ViewerKind::of(&path) {
                        let page = u32::try_from(cited.from).ok();
                        return vec![self.document_event(&path, kind, page)];
                    }
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

            Command::TerminalPaste { text } => {
                if let Some(term) = self.terminal.as_mut() {
                    term.paste(&text);
                }
                Vec::new()
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
                let size = (rows.max(1), cols.max(1));
                // The size it already is answers nothing. Every answer is a
                // whole screen, and the panel measures itself again after
                // each screen it draws — so answering an unchanged size kept
                // the two of them at it once a frame for as long as the panel
                // was open, re-sending an idle shell sixty times a second.
                if size == self.term_size {
                    return Vec::new();
                }
                self.term_size = size;
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
                // Signed in since it was last asked, perhaps: asked again.
                self.credential.replace(None);
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

            /* ── Claude Code ───────────────────────────────── */
            Command::ClaudeNew { engine } => self.claude.start(engine),
            Command::ClaudeEngine { engine } => {
                let events = self.claude.set_engine(engine);
                let _ = self.settings.set("chat.engine", serde_json::Value::String(engine.key().into()));
                events
            }
            Command::ClaudeOpen { session } => self.claude.open(&session),
            // Where investigations are kept is the window's: the session
            // answers it for every project at once.
            Command::ResearchFolder { .. } => {
                vec![notice("Where investigations are kept is set for the whole window.".into(), NoticeLevel::Warn)]
            }
            // Notebook is the window's, whichever project is open: the
            // session answers its commands before a workspace sees them.
            Command::StudyFolder { .. }
            | Command::StudyRoots { .. }
            | Command::StudyExclude { .. }
            | Command::StudyKbPause { .. }
            | Command::StudyKbRefresh
            | Command::StudyKbBrowse { .. }
            | Command::StudyKbSearch { .. }
            | Command::StudyKbAll { .. }
            | Command::StudyDiscover { .. }
            | Command::StudyNew { .. }
            | Command::StudyOpen { .. }
            | Command::StudyClose
            | Command::StudyDetails { .. }
            | Command::StudyArchive { .. }
            | Command::StudyAdd { .. }
            | Command::StudyAddFolder { .. }
            | Command::StudyDownload { .. }
            | Command::StudyPaste { .. }
            | Command::StudyAddUrls { .. }
            | Command::StudyTranscribe { .. }
            | Command::StudyMakeReadable { .. }
            | Command::StudyArtifacts
            | Command::StudyAddArtifacts { .. }
            | Command::StudyRemove { .. }
            | Command::StudySelect { .. }
            | Command::StudyRefresh { .. }
            | Command::StudyRole { .. }
            | Command::StudyShow { .. }
            | Command::StudyShowNote { .. }
            | Command::StudyNote { .. }
            | Command::StudyNoteDelete { .. }
            | Command::StudyNoteSource { .. }
            | Command::StudyAsk { .. }
            | Command::StudyStopAnswer
            | Command::StudyChatNew
            | Command::StudyChatOpen { .. }
            | Command::StudyTutor { .. }
            | Command::StudySaveAnswer { .. }
            | Command::StudyMake { .. }
            | Command::StudyCodeRun { .. }
            | Command::StudyCodeSave { .. }
            | Command::StudyCodePython { .. }
            | Command::StudyCodePythons
            | Command::StudyCancelJob { .. }
            | Command::StudyShowMade { .. }
            | Command::StudyBinMade { .. }
            | Command::StudyShowTask { .. }
            | Command::StudyReview { .. }
            | Command::StudyRate { .. }
            | Command::StudyReviewUndo
            | Command::StudyReviewEnd
            | Command::StudyCard { .. }
            | Command::StudyCardDelete { .. }
            | Command::StudyDeckExport { .. }
            | Command::StudyQuizStart { .. }
            | Command::StudyQuizAnswer { .. }
            | Command::StudyQuizDispute { .. }
            | Command::StudyQuizMarkAgain { .. }
            | Command::StudyQuizFinish { .. }
            | Command::StudyExamSit { .. }
            | Command::StudyExamExport { .. }
            | Command::StudyProgress
            | Command::StudyLesson { .. }
            | Command::StudyVoiceSetup { .. }
            | Command::StudyVoiceFind { .. }
            | Command::StudyVoiceSample { .. }
            | Command::StudyVoiceKey
            | Command::StudyAudioVoice { .. }
            | Command::StudySlidesExport { .. }
            | Command::StudySaveImage { .. } => vec![notice("Notebook is the window's, not a project's.".into(), NoticeLevel::Warn)],
            Command::ResearchNew { title, goal, discipline, literature } => self.research().create(&title, &goal, discipline, literature),
            Command::ResearchOpen { slug } => self.research().open(&slug),
            Command::ResearchShow { hypothesis } => self.research().show(hypothesis),
            Command::ResearchStart => self.research().start(),
            Command::ResearchPause => self.research().pause(),
            Command::ResearchResume => self.research().resume(),
            Command::ResearchStop => self.research().stop(),
            Command::ResearchFeedback { text } => self.research().feedback(&text),
            Command::ResearchAddHypothesis { title, statement } => self.research().add_hypothesis(&title, &statement),
            Command::ResearchSetStatus { hypothesis, status } => self.research().set_status(&hypothesis, status),
            Command::ResearchSeat { seat, enabled } => self.research().set_seat(seat, enabled),
            Command::ResearchTask { task } => self.research().watch(task),
            Command::ResearchApprove { experiment, unsandboxed } => self.research().approve(&experiment, unsandboxed),
            Command::ResearchReject { experiment } => self.research().reject(&experiment),
            Command::ResearchAutoRun { enabled } => self.research().set_auto_run(enabled),
            Command::ResearchStopRun { run } => self.research().stop_run(&run),
            Command::ResearchShowRun { run } => self.research().show_run(run),
            Command::ResearchSetLiterature { enabled, sources } => self.research().set_literature(enabled, sources),
            Command::ResearchSearch { query } => self.research().search(&query),
            Command::ResearchAddPaper { paper } => self.research().add_paper(&paper),
            Command::ResearchShowPaper { paper } => self.research().show_paper(paper),
            Command::ResearchOpenPaper { paper, page } => {
                let found = self.research().paper_pdf(&paper);
                match found {
                    Ok(path) => vec![self.document_event(&path, ViewerKind::Pdf, page)],
                    Err(why) => vec![notice(why, NoticeLevel::Warn)],
                }
            }
            Command::ResearchFetchPdf { paper } => self.research().fetch_pdf(&paper),
            Command::LabPlan => self.research().lab_do(|lab| lab.plan()),
            Command::LabCreate => self.research().lab_do(|lab| lab.create()),
            Command::LabCancel => self.research().lab_do(|lab| {
                lab.cancel();
                Ok(())
            }),
            Command::LabRemove => self.research().lab_do(|lab| lab.remove()),
            Command::LeanPlan => self.research().lean_do(|lean| lean.plan()),
            Command::LeanCreate => self.research().lean_do(|lean| lean.create()),
            Command::LeanCancel => self.research().lean_do(|lean| {
                lean.cancel();
                Ok(())
            }),
            Command::LeanRemove => self.research().lean_do(|lean| lean.remove()),
            Command::ClaudeSend { text, images } => self.claude.send(&text, &images),
            Command::ClaudeStop => self.claude.interrupt(),
            Command::ClaudeAnswer { request, answer } => self.claude.answer(&request, answer),
            // The choice is the session's, and the default for the next one —
            // which is a project setting, so it is there when Proton is next
            // opened on this folder.
            Command::ClaudeMode { mode } => {
                let events = self.claude.set_mode(&mode);
                let _ = self.settings.set("claude.mode", serde_json::Value::String(mode));
                events
            }
            Command::ClaudeModel { model } => {
                let key = chat_setting(self.claude.active_engine(), "model");
                let events = self.claude.set_model(&model);
                let saved = match model.as_str() {
                    "" | "default" => serde_json::Value::Null,
                    _ => serde_json::Value::String(model),
                };
                let _ = self.settings.set(&key, saved);
                events
            }
            Command::ClaudeEffort { effort } => {
                let key = chat_setting(self.claude.active_engine(), "effort");
                let events = self.claude.set_effort(effort.as_deref());
                let saved = effort.filter(|e| !e.is_empty()).map(serde_json::Value::String).unwrap_or(serde_json::Value::Null);
                let _ = self.settings.set(&key, saved);
                events
            }
            Command::ClaudeSessions => self.claude.refresh(),
            Command::ClaudeDelete { session } => self.claude.delete(&session),
            Command::ChatFolderNew { name, sessions } => self.claude.folder_new(&name, &sessions),
            Command::ChatFolderRename { folder, name } => self.claude.folder_rename(&folder, &name),
            Command::ChatFolderDelete { folder } => self.claude.folder_delete(&folder),
            Command::ChatFolderCollapse { folder, collapsed } => self.claude.folder_collapse(&folder, collapsed),
            Command::ChatFolderMove { session, folder } => self.claude.folder_move(&session, folder.as_deref()),
            Command::ForgetArtifact { id } => self.claude.forget_artifact(&id),
            Command::ImportArtifacts => self.claude.import_artifacts(),
            Command::OpenLink { url } => match open_link(&url) {
                Ok(()) => Vec::new(),
                Err(why) => vec![notice(why, NoticeLevel::Warn)],
            },

            Command::CancelTurn => {
                self.agent.cancel();
                // The cell keeps running — it is the user's kernel and their
                // code — but nothing is waiting on it any more. Leaving these
                // would answer into a turn that no longer exists.
                self.waiting.clear();
                vec![self.agent_event()]
            }

            Command::Poll => self.poll_servers(),

            Command::SheetToData { generation, sheet, range } => self.sheet_to_data(generation, sheet as usize, &range),
            Command::CircuitTraceToData { generation } => match self.circuits.trace_csv(generation) {
                Ok((csv, label)) => self.csv_to_data("circuit", &label, &csv, "trace_df", "from-circuit.ipynb", format!("the graph of {label} is in the notebook as trace_df: run the cell to load it")),
                Err(why) => vec![notice(why, NoticeLevel::Warn)],
            },
            Command::SheetFromData => self.data_to_sheet(),

            command @ (Command::SheetNew { .. }
            | Command::SheetOpen { .. }
            | Command::SheetClose { .. }
            | Command::SheetSave
            | Command::SheetView { .. }
            | Command::SheetSet { .. }
            | Command::SheetSelect { .. }
            | Command::SheetCopy { .. }
            | Command::SheetPaste { .. }
            | Command::SheetFill { .. }
            | Command::SheetInsert { .. }
            | Command::SheetDelete { .. }
            | Command::SheetFormat { .. }
            | Command::SheetWidth { .. }
            | Command::SheetFreeze { .. }
            | Command::SheetSort { .. }
            | Command::SheetFilter { .. }
            | Command::SheetFilterValues { .. }
            | Command::SheetFilterSet { .. }
            | Command::SheetRuleAdd { .. }
            | Command::SheetRuleRemove { .. }
            | Command::SheetChartAdd { .. }
            | Command::SheetChartSet { .. }
            | Command::SheetChartRemove { .. }
            | Command::SheetUndo
            | Command::SheetRedo
            | Command::SheetTab { .. }
            | Command::SheetAddSheet { .. }
            | Command::SheetRenameSheet { .. }
            | Command::SheetRemoveSheet { .. }) => {
                let root = self.root().to_path_buf();
                let mut events = self.sheets.apply(&root, command);
                if events.iter().any(|e| matches!(e, Event::SheetHome { .. })) {
                    // A workbook made or saved is files the tree and git see.
                    events.push(self.tree_event());
                    events.push(self.repo_event());
                }
                events
            }

            command @ (Command::CircuitNew { .. }
            | Command::CircuitOpen { .. }
            | Command::CircuitClose { .. }
            | Command::CircuitSave
            | Command::CircuitShow { .. }
            | Command::CircuitAdd { .. }
            | Command::CircuitSetTop { .. }
            | Command::CircuitEdit { .. }
            | Command::CircuitUndo
            | Command::CircuitRedo
            | Command::CircuitCopy { .. }
            | Command::CircuitStart { .. }
            | Command::CircuitStop
            | Command::CircuitSet { .. }
            | Command::CircuitClock { .. }
            | Command::CircuitStep { .. }
            | Command::CircuitQuick { .. }
            | Command::CircuitSaveMemory { .. }
            | Command::CircuitTestSet { .. }
            | Command::CircuitTestRun { .. }
            | Command::CircuitTraceClear { .. }
            | Command::CircuitAnalyse { .. }
            | Command::CircuitSynthesise { .. }) => {
                let root = self.root().to_path_buf();
                let mut events = self.circuits.apply(&root, command);
                if events.iter().any(|e| matches!(e, Event::CircuitHome { .. })) {
                    // A project made or saved is files the tree and git see.
                    events.push(self.tree_event());
                    events.push(self.repo_event());
                }
                events
            }

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
                let (mut events, changed) = self.act(&action, mode, false);
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

            // The session asks every project; a workspace on its own, itself.
            Command::AskToQuit => {
                let files = self.unsaved();
                match files.is_empty() {
                    true => Vec::new(),
                    false => vec![Event::Unsaved { files }],
                }
            }

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
            Command::EditorPaste { text } => self.paste(text),
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
                // The selection as it stands, in case this key ends it.
                let held = matches!(
                    before,
                    VimMode::Visual | VimMode::VisualLine | VimMode::VisualBlock
                )
                .then(|| (self.editor.active(), self.editor.buffer().cursor));
                let typed = key.as_char().filter(|_| key.is_bare_char());
                let mut events = Vec::new();
                let mut changed = false;

                // Insert-mode editing keys that are text entry rather than
                // commands, so the modal engine never sees them as actions.
                if matches!(before, VimMode::Insert | VimMode::Replace)
                    && self.mode.edits_text()
                {
                    match key.key.as_str() {
                        "Enter" => {
                            self.editor.buffer_mut().ensure_group();
                            self.enter();
                            changed = true;
                        }
                        "Backspace" => {
                            self.editor.buffer_mut().ensure_group();
                            changed = self.backspace();
                        }
                        _ => {}
                    }
                }

                let outcomes = self.vim.keys(&key);
                // Only dot and macro replay fan out from one key. Keep that
                // invocation together, including its initial change action.
                let replay = outcomes.len() > 1 && self.mode.edits_text();
                if replay {
                    self.editor.buffer_mut().begin_group(false);
                }
                for outcome in outcomes {
                    let (evts, did) = self.events_for(outcome, before, replay);
                    events.extend(evts);
                    changed |= did;
                }
                if replay {
                    self.editor.buffer_mut().end_group();
                }

                // The mode transition is reported after whatever the key did,
                // so a statusline applying events in order ends up correct.
                if self.vim.mode() != before {
                    // Leaving visual mode lets go of the selection — kept for
                    // `gv`, and gone from the screen, where it used to stay lit
                    // after Escape until the cursor next moved.
                    let visual = |mode: VimMode| {
                        matches!(
                            mode,
                            VimMode::Visual | VimMode::VisualLine | VimMode::VisualBlock
                        )
                    };
                    if visual(before) && !visual(self.vim.mode()) && self.mode.edits_text() {
                        if let Some((buffer, cursor)) = held {
                            self.last_visual = Some((buffer, cursor.anchor, cursor.offset, before));
                        }
                        let buf = self.editor.buffer_mut();
                        let at = buf.cursor.offset;
                        buf.cursor = buf.cursor.moved_to(at);
                    }
                    // Leaving insert mode steps the cursor back one, as vim
                    // does: in insert you sit *between* characters, in normal
                    // you sit *on* one, and the character you land on is the
                    // one you just typed.
                    if matches!(before, VimMode::Insert | VimMode::Replace)
                        && self.vim.mode() == VimMode::Normal
                        && self.mode.edits_text()
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
        //
        // Words only, even when the vectors exist. This runs on every
        // keystroke, and embedding one query against a local model measured at
        // two seconds — so the picker would be two seconds behind the letter
        // being typed, which is not a picker. The vectors are for
        // `search_library`, where a question is asked once and the wait is
        // hidden inside a turn that was already going to take longer.
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
        // Live grep may still be searching for the query as typed; a hit from
        // the keystroke before it is not the one being chosen.
        self.pickers.settle();
        let accepted = self
            .pickers
            .open()
            .map(|p| p.accept())
            .unwrap_or(Accepted::Nothing);
        self.pickers.close();
        let mut events = vec![Event::PickerClosed];

        match accepted {
            Accepted::OpenPath { path, line, column } => {
                let opened = self.open_path(&path);
                let text_opened = opened.iter().any(|event| matches!(event, Event::Buffers { .. }));
                events.extend(opened);
                if text_opened {
                    // Text is edited in Codex.
                    if !self.mode.edits_text() {
                        events.extend(self.set_mode(ModeId::Codex));
                    }
                    // Grep and symbol results carry a location: that exact
                    // line, revealed.
                    if let Some(line) = line {
                        let buf = self.editor.buffer_mut();
                        let at = buf.offset_of(proton_text::Position {
                            line,
                            column: column.unwrap_or(0),
                        });
                        buf.cursor = buf.clamp(buf.cursor.moved_to(at));
                    }
                    events.extend(self.after_action(true));
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
                    self.resync();
                    events.extend(self.switched());
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
                // Passages, and only the ones the scope admits. The project's
                // file count is what every other picker is choosing from and
                // has nothing to do with this one.
                PickerKind::Library => self.library.passages_in(&self.library_scope),
                _ => self.pickers.file_count(),
            },
            elapsed_ms: picker.elapsed_ms(),
            // Whose walk is still running depends on which list is being
            // searched. Saying "scanning" because the *file* index is warming
            // would be the wrong reassurance about the wrong thing.
            scanning: match picker.kind() {
                PickerKind::Library => self.library.is_building(),
                PickerKind::Grep => self.pickers.is_scanning() || self.pickers.is_searching(),
                _ => self.pickers.is_scanning(),
            },
            // Only the lists read from the walk can be cut short by it.
            cut_short: matches!(picker.kind(), PickerKind::Files | PickerKind::Grep)
                && self.pickers.is_cut_short(),
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
        self.highlights.forget(self.editor.buffer());
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
                if name == ".editorconfig" {
                    self.editorconfigs.clear();
                }
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
                // On by default: a formatter configured and not run is the
                // surprise, not the other way round. Off is for a project
                // whose formatter disagrees with its own history.
                // Not a LaTeX file: texlab formats with latexindent, which is
                // seldom installed.
                let tex = self.editor.buffer().path().is_some_and(is_latex);
                // C and C++ are left as written unless asked for: clangd's
                // and clang-format's house style rewrites code that has its
                // own. `␣lf` still formats one when asked.
                let c = self.editor.buffer().path().is_some_and(crate::formatters::is_c_family);
                if self.settings.bool("editor.formatOnSave", true) && !tex && (!c || self.settings.bool("editor.formatC", false)) {
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
            // No server formats it, but clang-format can: C and C++ without clangd.
            if crate::formatters::is_c_family(&path) {
                let program = self.clang_format.get_or_insert_with(crate::formatters::clang_format).clone();
                if let Some(program) = program {
                    let text = self.editor.buffer().contents();
                    self.external_format = Some(crate::formatters::start(&program, &path, text));
                    self.formatting = Some(Formatting { path, version: self.editor.buffer().version(), then_write });
                    return Vec::new();
                }
            }
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

    /// clang-format's answer, once it has one: put in the buffer through the
    /// same door as a server's edits, so a buffer edited meanwhile is left be.
    fn poll_external_format(&mut self) -> Vec<Event> {
        let Some(rx) = &self.external_format else { return Vec::new() };
        match rx.try_recv() {
            Ok(Ok(formatted)) => {
                self.external_format = None;
                let edits = crate::formatters::edits(&self.editor.buffer().contents(), &formatted);
                self.formatted(&edits)
            }
            Ok(Err(why)) => {
                self.external_format = None;
                self.formatting = None;
                vec![notice(why, NoticeLevel::Warn)]
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => Vec::new(),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.external_format = None;
                self.formatting = None;
                Vec::new()
            }
        }
    }

    /// Name the clang-format this workspace formats C with.
    #[cfg(test)]
    pub(crate) fn use_clang_format(&mut self, program: Option<PathBuf>) {
        self.clang_format = Some(program);
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
            return match self.open_notebook(&resolved) {
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
            return vec![self.document_event(&resolved, kind, None)];
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
            self.splits_event(),
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
        if self.mode.edits_text() {
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
        self.answer_embedded_searches();
        // A quick run on its own thread: how far it has gone, and its end.
        let ran = self.circuits.poll();
        if !self.quiet {
            events.extend(ran);
        }
        events.extend(self.poll_external_format());

        if let Some(event) = self.poll_repo() {
            if !self.quiet {
                events.push(event);
            }
        }
        let polled = self.data.poll();
        if polled != crate::data::Polled::Nothing {
            // A kernel that died under a running cell is news, not a state:
            // the chip says what is wrong now, but only a notice tells
            // somebody who is looking at the cell rather than at the header.
            if let Some(why) = self.data.complaint() {
                events.push(notice(why, NoticeLevel::Error));
            }
            if !self.quiet {
                events.push(match polled {
                    crate::data::Polled::Cells(keys) => self.notebook_cells_event(&keys),
                    _ => self.notebook_event(),
                });
            }
        }
        if self.terminal.as_mut().is_some_and(|t| t.poll()) && !self.quiet {
            events.push(self.terminal_delta());
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
        // Only while a walk or a search is out: the labels are a list of every
        // buffer, and building one on every beat to hand to nothing is waste.
        if self.pickers.is_waiting() {
            let labels = self.buffer_labels();
            if self.pickers.poll(labels) {
                events.push(self.picker_event());
            }
        }

        events.extend(self.poll_council());

        // Claude Code's conversations: every one that is running, whether
        // or not it is on screen.
        events.extend(self.claude.poll());

        // Research's rounds, the same way: their calls are conversations
        // with the same engines, each one task long. Unless they are the
        // session's, which polls them once for every project.
        if !self.research_shared {
            events.extend(self.research().poll());
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
                        let rows = lsp::reference_rows(&places, &self.root);
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
        if !self.mode.edits_text() || self.vim.mode() != VimMode::Insert {
            return Vec::new();
        }
        let Some(c) = typed else {
            return Vec::new();
        };
        // `.` and `:` everywhere, and in a LaTeX file whatever texlab asks
        // for — its `\`, `{` and `,`. Other servers' own characters (Rust's
        // `(`, TypeScript's `"`) would open a list where none opened before.
        let server = self.editor.buffer().path().is_some_and(|p| is_latex(p) && self.servers.can(p).triggers_on(c));
        let trigger = c == '.' || c == ':' || server;
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

    /// What a file says right now: the buffer holding it when there is one,
    /// which for a staged file there always is, because staging opens it —
    /// and the disk otherwise.
    ///
    /// Found the way the editor finds what to focus, so it is the same buffer
    /// that accepting a hunk then types into, whichever name of the project
    /// either path was spelled through.
    fn file_now(&self, path: &Path) -> Option<String> {
        match self.editor.find(path) {
            Some(index) => Some(self.editor.buffers()[index].contents()),
            None => std::fs::read_to_string(path).ok(),
        }
    }

    /// Proton is going: the notebook's kernel is stopped, and Research's runs
    /// are stopped and its calls let go — the project's own, that is; the
    /// session stops one it shares.
    pub fn shutdown(&mut self) {
        // A notebook with changes is saved rather than lost: Jupyter saves as
        // it goes, and quitting is the last chance to.
        if let Err(why) = self.data.save_if_modified() {
            eprintln!("proton: the notebook could not be saved: {why}");
        }
        self.data.shutdown();
        if !self.research_shared {
            self.research().shutdown();
        }
    }

    /// Where a file a Lab run left is: see `Research::lab_file`.
    pub fn lab_file(&self, key: &str) -> Option<PathBuf> {
        self.research().lab_file(key)
    }

    /// A file to show in the document viewer, remembered so the shell will
    /// read it for the page: it reads only what was opened here this way.
    fn document_event(&mut self, path: &Path, kind: ViewerKind, page: Option<u32>) -> Event {
        self.documents.insert(path.to_path_buf());
        Event::Document { path: path.display().to_string(), name: crate::folder::name_of(path), kind, page }
    }

    /// The file at `path`, if it was opened in the document viewer here.
    pub fn document_file(&self, path: &Path) -> Option<PathBuf> {
        self.documents.contains(path).then(|| path.to_path_buf())
    }

    /// The investigations this workspace shows.
    fn research(&self) -> MutexGuard<'_, crate::research::Research> {
        self.research.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether the project's own investigations have a round or a run going.
    /// One the session shares is the session's to ask.
    pub fn research_busy(&self) -> bool {
        !self.research_shared && self.research().has_work()
    }

    /// Everything the Research tab draws, as `Sync` gives it.
    pub fn research_events(&mut self) -> Vec<Event> {
        self.research().sync()
    }

    /// Show the project's own investigations again, in its `research/`,
    /// rather than the session's.
    pub fn own_research(&mut self) {
        if self.research_shared {
            self.research = Arc::new(Mutex::new(crate::research::Research::new(&self.root().to_path_buf())));
            self.research_shared = false;
        }
    }

    /// Show the session's investigations, kept in a folder of their own,
    /// rather than the project's: see `Session::with_research_in`.
    pub fn share_research(&mut self, research: &Arc<Mutex<crate::research::Research>>) {
        self.research().shutdown();
        self.research = Arc::clone(research);
        self.research_shared = true;
    }

    /// The same, for the builder chain a session's `build` uses.
    pub fn sharing_research(mut self, research: Option<&Arc<Mutex<crate::research::Research>>>) -> Self {
        if let Some(research) = research {
            self.share_research(research);
        }
        self
    }

    /// Everything the agent has proposed and nobody has accepted yet.
    ///
    /// Computed on the spot, like every other view of the review: the hunks
    /// are the diff between each buffer and what was proposed, so there is
    /// nothing stored that this could return a stale copy of.
    pub fn staged(&self) -> Vec<review::ChangeView> {
        self.review.view(|path| self.file_now(path))
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
        let Some(current) = self.file_now(&path) else {
            return vec![notice(
                format!("{}: not open any more", path.display()),
                NoticeLevel::Warn,
            )];
        };
        let Some((lines, text)) = self.review.accept(&path, index, &current) else {
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
        // Up to the start of the line after the hunk, so the newline ending
        // its last line is replaced too, and `text` brings the target's own
        // endings in its place, the file's final newline included. A file
        // without a final newline has no line after its last, and asking for
        // one would land at the start of that last line, so the stretch runs
        // to the end of the file instead.
        let end = match lines.end < buffer.len_lines() {
            true => buffer.offset_of(proton_text::Position {
                line: lines.end,
                column: 0,
            }),
            false => buffer.len_chars(),
        };
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
        let Some(current) = self.file_now(&path) else {
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
            let Some(current) = self.file_now(path) else {
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
            self.editor.buffer_mut().end_group();
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
                    if call.name == "run_cell" {
                        events.push(self.notebook_event());
                    }
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
            // A workbook opened, written to or charted: on screen at once.
            if crate::sheet::TOOLS.contains(&call.name.as_str()) {
                events.extend(self.sheets.take_pending());
            }
            self.agent.answer(&call.id, answer);
        }

        // Every outstanding call answered? Then the turn continues, with all
        // the results in one message.
        match self.agent.resume() {
            Ok(_) => {}
            Err(why) => events.push(notice(why, NoticeLevel::Error)),
        }

        // Only when there is something new to show, and somebody to see it.
        let shown = self.agent_fingerprint();
        if !self.quiet && self.agent_shown != Some(shown) {
            self.agent_shown = Some(shown);
            events.push(self.agent_delta());
        }

        // The turn ended between the start of this poll and its end, which is
        // the only moment the next task may start: two turns in flight would
        // interleave into one transcript.
        if was_busy && !self.agent.is_busy() {
            // The answer is complete: write it down. This is the other half of
            // saving on `Ask` — between them, nothing said is ever only in
            // memory.
            events.extend(self.save_chat());
            events.extend(self.task_finished());
            events.extend(self.reread_after_hosted_turn());
        } else if was_busy {
            // Still going. The row says what it is doing, which is the whole
            // reason it is a queue and not a spinner.
            let last = self.agent.transcript().last();
            let note = last.map(|e| first_sentence(&e.text)).unwrap_or_default();
            let tools = last.map_or(0, |e| e.calls.len());
            // Said only when it moved: sent on every heartbeat, it kept the
            // shell's loop at sixty a second through a turn's long silences.
            if self.tasks.progress(&note, tools) {
                events.push(self.tasks_event());
            }
        }
        events
    }

    /// What Tab inserts at the cursor: a tab where the file is indented with
    /// tabs, and otherwise spaces to the next stop of the file's own indent —
    /// so a Tab after two characters of a four-wide indent adds two, and in a
    /// file indented by two a Tab is two.
    fn indent_here(&mut self) -> String {
        let indent = self.indent();
        if indent.tabs {
            return "\t".into();
        }
        let buffer = self.editor.buffer();
        let at = buffer.position(buffer.cursor.offset);
        let before: String = buffer.line(at.line).chars().take(at.column).collect();
        let column = crate::typing::columns(&before, indent.tab);
        " ".repeat(indent.width - column % indent.width)
    }

    /// How wide a tab stop is: `editor.tabSize`, four unless it says.
    fn tab_size(&self) -> usize {
        self.settings.number("editor.tabSize", 4).clamp(1, 16) as usize
    }

    /// How the buffer being edited is indented: its `.editorconfig`, then its
    /// own lines, then `editor.tabSize` (`typing::indent_of`).
    fn indent(&mut self) -> crate::typing::Indent {
        let fallback = self.tab_size();
        let buffer = self.editor.buffer();
        let (path, name, lines) = (buffer.path().map(Path::to_path_buf), buffer.name(), buffer.lines(0, 1000));
        let config = path.map(|path| self.editorconfig(&path)).unwrap_or_default();
        crate::typing::indent_of(config, &name, &lines, fallback)
    }

    /// What the `.editorconfig` files above a file say about it, read once.
    fn editorconfig(&mut self, path: &Path) -> crate::typing::Config {
        *self
            .editorconfigs
            .entry(path.to_path_buf())
            .or_insert_with(|| crate::typing::editorconfig(path))
    }

    /// How wide a tab in the buffer at `index` is drawn.
    fn tab_drawn(&mut self, index: usize) -> usize {
        let path = self.editor.buffers().get(index).and_then(|b| b.path().map(Path::to_path_buf));
        let config = path.map(|path| self.editorconfig(&path)).unwrap_or_default();
        config.tab.or(config.width).unwrap_or_else(|| self.tab_size())
    }

    /// Enter in insert mode: a new line indented as the code it starts
    /// belongs (`typing::newline`), or bare with `editor.autoIndent` off.
    fn enter(&mut self) {
        if !self.settings.bool("editor.autoIndent", true) {
            self.editor.insert_newline();
            return;
        }
        let indent = self.indent();
        let buf = self.editor.buffer();
        let at = buf.cursor.offset;
        let here = buf.position(at);
        let line = buf.line(here.line);
        let before: String = line.chars().take(here.column).collect();
        let after: String = line.chars().skip(here.column).collect();
        let eol = crate::typing::eol(buf, here.line);
        let extension = extension_of(&buf.name());
        let (take_before, take_after, text, cursor) = crate::typing::newline(&before, &after, &indent, &extension, eol);
        let start = at - take_before;
        self.editor.buffer_mut().apply(
            vec![proton_text::Edit::replace(proton_text::Range::new(start, at + take_after), text)],
            proton_text::Cursor::at(start + cursor),
        );
    }

    /// Backspace in insert mode: both halves of an empty pair, spaces back to
    /// the last indent stop, a line ending whole, or one character.
    fn backspace(&mut self) -> bool {
        let pairs = self.settings.bool("editor.autoClosingBrackets", true);
        let buf = self.editor.buffer();
        let at = buf.cursor.offset;
        if at == 0 {
            return false;
        }
        let here = buf.position(at);
        let line = buf.line(here.line);
        let before: String = line.chars().take(here.column).collect();
        let (prev, next) = (before.chars().last(), line.chars().nth(here.column));
        let crlf = here.column == 0 && at >= 2 && buf.slice(proton_text::Range::new(at - 2, at)) == "\r\n";
        let (from, to) = if here.column > 0 && pairs && crate::typing::empty_pair(prev, next) {
            (at - 1, at + 1)
        } else if here.column > 0 && before.chars().all(|c| c == ' ') {
            let indent = self.indent();
            (at - crate::typing::back_to_stop(&before, &indent), at)
        } else if crlf {
            (at - 2, at)
        } else {
            (at - 1, at)
        };
        self.editor.buffer_mut().apply(
            vec![proton_text::Edit::delete(proton_text::Range::new(from, to))],
            proton_text::Cursor::at(from),
        );
        true
    }

    /// A character typed in insert mode: with its partner, or as a step over
    /// the one already there (`typing::typed`), unless
    /// `editor.autoClosingBrackets` is off; and a closing bracket typed where
    /// only indentation comes before it, back at its block's indentation.
    fn type_char(&mut self, c: char) {
        let pairs = self.settings.bool("editor.autoClosingBrackets", true);
        let outdent = self.settings.bool("editor.autoIndent", true);
        let buf = self.editor.buffer();
        let at = buf.cursor.offset;
        let here = buf.position(at);
        let line = buf.line(here.line);
        let before: String = line.chars().take(here.column).collect();
        let (prev, next) = (before.chars().last(), line.chars().nth(here.column));
        let extension = extension_of(&buf.name());
        let typed = match pairs {
            true => crate::typing::typed(c, prev, next, &extension),
            false => crate::typing::Typed::Insert(c.to_string()),
        };
        let text = match typed {
            crate::typing::Typed::Over => {
                let buf = self.editor.buffer_mut();
                buf.cursor = proton_text::Cursor::at(at + 1);
                return;
            }
            crate::typing::Typed::Insert(text) => text,
        };
        if outdent && matches!(c, ')' | ']' | '}') && !before.is_empty() && before.trim().is_empty() {
            let previous = (0..here.line).rev().map(|l| buf.line(l)).find(|l| !l.trim().is_empty());
            if let Some(previous) = previous {
                let indent = self.indent();
                if let Some(target) = crate::typing::outdented(&before, &previous, &indent, &extension) {
                    let start = at - here.column;
                    let cursor = start + target.chars().count() + 1;
                    self.editor.buffer_mut().apply(
                        vec![proton_text::Edit::replace(proton_text::Range::new(start, at), format!("{target}{text}"))],
                        proton_text::Cursor::at(cursor),
                    );
                    return;
                }
            }
        }
        self.editor.buffer_mut().apply(
            vec![proton_text::Edit::insert(at, text)],
            proton_text::Cursor::at(at + 1),
        );
    }

    /// Re-read the open files a provider changed on its own.
    ///
    /// Antigravity edits the project with its own tools rather than through
    /// `edit_file`, so nothing here saw the change happen; without this the
    /// buffer would go on showing the old text until `:e`. Only files you have
    /// not touched since — a buffer with unsaved work is left alone, and the
    /// `:w` guard will say the disk moved under it.
    fn reread_after_hosted_turn(&mut self) -> Vec<Event> {
        if self.agent.provider() != proton_agent::Provider::Antigravity {
            return Vec::new();
        }
        let names = self.editor.reread_changed_on_disk();
        if names.is_empty() {
            return Vec::new();
        }
        self.resync();
        let mut events = vec![
            notice(format!("re-read {}", names.join(", ")), NoticeLevel::Info),
            self.buffers_event(),
        ];
        events.extend(self.every_view());
        events.push(self.diagnostics_event());
        events
    }

    /// Append whatever the question pointed at.
    ///
    /// The typed words are kept exactly as written and the context follows
    /// under headings. Substituting `@selection` for the selected text inline
    /// would make a short question unreadable in the pane it is drawn in, and
    /// would leave the reader unable to tell which half of the sentence they
    /// wrote.
    pub fn with_context(&self, text: &str) -> String {
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
                crate::mention::Mention::Sheet => self.sheets.about(),
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
        let Some(cell) = self.data.cell_at(self.data.selected()) else {
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
        if name == "search_library" {
            if let Some(deferred) = self.start_library_search(id, input) {
                return deferred;
            }
        }
        if name != "run_cell" {
            return ToolOutcome::Answered(self.run_tool(name, input));
        }
        match self.start_cell(input) {
            Ok((cell, key)) => {
                self.waiting.push(Waiting {
                    id: id.to_string(),
                    cell,
                    key,
                    beats: 0,
                });
                ToolOutcome::Deferred
            }
            // A start that failed has to answer, or the turn waits for ever on
            // a cell that never began.
            Err(why) => ToolOutcome::Answered(Err(why)),
        }
    }

    /// Begin embedding a question, if this search would be better for it.
    ///
    /// `None` means "not this one" and the caller answers it synchronously —
    /// which covers every case where there is nothing to gain: no model
    /// configured, no vectors made yet, or a library that cannot be searched
    /// at all and should say so now rather than after a two-second wait.
    fn start_library_search(&mut self, id: &str, input: &serde_json::Value) -> Option<ToolOutcome> {
        let embedder = self.embedder.clone()?;
        if !self.library.has_vectors() || self.library_excuse().is_some() {
            return None;
        }
        let query = input.get("query").and_then(|v| v.as_str())?.to_string();

        let (done, vector) = std::sync::mpsc::channel();
        let asked = query.clone();
        // Detached: it holds nothing the editor needs, and a send into a
        // dropped receiver is how it learns the question was abandoned.
        std::thread::Builder::new()
            .name("proton-embed-query".into())
            .spawn(move || {
                let made = embedder
                    .vectors(&[asked])
                    .map(|mut all| all.pop().unwrap_or_default());
                let _ = done.send(made);
            })
            .ok()?;

        self.asking.push(Asking {
            id: id.to_string(),
            limit: input
                .get("limit")
                .and_then(|v| v.as_u64())
                .map_or(6, |n| n as usize),
            scope: self.asked_scope(input),
            query,
            vector,
            beats: 0,
        });
        Some(ToolOutcome::Deferred)
    }

    /// Answer every library search whose vector has arrived.
    fn answer_embedded_searches(&mut self) {
        if self.asking.is_empty() {
            return;
        }
        let mut ready: Vec<(String, String, usize, proton_library::Scope, Vec<f32>)> = Vec::new();
        self.asking.retain_mut(|asking| {
            let vector = match asking.vector.try_recv() {
                Ok(Ok(vector)) => vector,
                // A model that is not running is not a reason to answer
                // nothing: the words still work. Same for running out of
                // patience, which is why both fall through to an empty vector
                // rather than to an error.
                Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => Vec::new(),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    asking.beats += 1;
                    if asking.beats < PATIENCE_ASKING {
                        return true;
                    }
                    Vec::new()
                }
            };
            ready.push((
                asking.id.clone(),
                asking.query.clone(),
                asking.limit,
                asking.scope.clone(),
                vector,
            ));
            false
        });

        let done: Vec<(String, Result<String, String>)> = ready
            .into_iter()
            .map(|(id, query, limit, scope, vector)| {
                (id, self.library_answer(&query, limit, &scope, &vector))
            })
            .collect();
        for (id, answer) in done {
            self.agent.answer(&id, answer);
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
    fn start_cell(&mut self, input: &serde_json::Value) -> Result<(usize, u64), String> {
        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or("run_cell: `path` is required")?;
        let cell = input
            .get("cell")
            .and_then(|v| v.as_u64())
            .ok_or("run_cell: `cell` is required")? as usize;

        let full = self.pickers.resolve(path);
        self.open_notebook(&full)?;

        let cwd = self.pickers.root().to_path_buf();
        self.data.run(cell, &cwd)?;
        let key = self.data.key_at(cell).ok_or_else(|| format!("there is no cell {cell}"))?;
        Ok((cell, key))
    }

    /// Open a notebook in Data, keeping the one already open if it is that
    /// one, and saving the one open first if it has changes — opening another
    /// would otherwise lose them. Refused if that save fails.
    /// A range of the workbook open into Data: its values written as CSV
    /// under `.proton/excel/`, and a cell that reads it with pandas put at
    /// the end of the notebook open — or of a new `from-excel.ipynb` — for
    /// the person to run.
    fn sheet_to_data(&mut self, generation: u32, sheet: usize, range: &str) -> Vec<Event> {
        let (csv, label) = match self.sheets.range_csv(generation, sheet, range) {
            Ok(x) => x,
            Err(why) => return vec![notice(why, NoticeLevel::Warn)],
        };
        let said = format!("{range} is in the notebook as sheet_df: run the cell to load it");
        self.csv_to_data("excel", &label, &csv, "sheet_df", "from-excel.ipynb", said)
    }

    /// A table as a CSV under `.proton/<folder>/`, and a cell of the notebook
    /// open — or of `notebook`, made if it must be — reading it as `var`;
    /// Data shown with the cell selected.
    fn csv_to_data(&mut self, folder: &str, label: &str, csv: &str, var: &str, notebook: &str, said: String) -> Vec<Event> {
        let root = self.root().to_path_buf();
        let dir = root.join(".proton").join(folder);
        let file = dir.join(format!("{label}.csv"));
        if let Err(why) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, csv)) {
            return vec![notice(format!("{}: {why}", file.display()), NoticeLevel::Error)];
        }
        if !self.data.is_open() {
            let path = root.join(notebook);
            if !path.exists() {
                let empty = r#"{"cells": [], "metadata": {"kernelspec": {"display_name": "Python 3", "language": "python", "name": "python3"}, "language_info": {"name": "python"}}, "nbformat": 4, "nbformat_minor": 5}"#;
                if let Err(why) = std::fs::write(&path, empty) {
                    return vec![notice(format!("{}: {why}", path.display()), NoticeLevel::Error)];
                }
            }
            if let Err(why) = self.open_notebook(&path) {
                return vec![notice(why, NoticeLevel::Warn)];
            }
        }
        let shown = serde_json::to_string(&file.display().to_string()).unwrap_or_default();
        let code = format!("import pandas as pd\n\n{var} = pd.read_csv({shown})\n{var}");
        let Some(index) = self.data.add_cell(true) else {
            return vec![notice("the notebook could not take a cell".into(), NoticeLevel::Warn)];
        };
        self.data.set_source(index, &code);
        self.data.select_cell(index);
        let mut events = self.set_mode(ModeId::Data);
        events.push(self.notebook_event());
        events.push(Event::CellSelected { index });
        events.push(self.tree_event());
        events.push(notice(said, NoticeLevel::Info));
        events
    }

    /// The selected notebook cell's dataframe, as a sheet in Excel.
    fn data_to_sheet(&mut self) -> Vec<Event> {
        let index = self.data.selected();
        let table = self.data.cell_at(index).and_then(|cell| {
            cell.outputs.iter().find_map(|output| match output {
                crate::data::OutputView::Table { table } => Some(table.clone()),
                _ => None,
            })
        });
        let Some(table) = table else {
            return vec![notice("the selected cell has no table: run one that ends in a dataframe".into(), NoticeLevel::Warn)];
        };
        let root = self.root().to_path_buf();
        let mut events = self.sheets.add_table(&root, &format!("Cell {index}"), &table.columns, &table.rows);
        events.extend(self.set_mode(ModeId::Sheet));
        events.push(self.tree_event());
        if let Some(total) = table.total.filter(|t| *t > table.rows.len()) {
            events.push(notice(
                format!("the notebook showed {} of the frame's {total} rows, and those are what came: write it out with to_csv to bring all of it", table.rows.len()),
                NoticeLevel::Warn,
            ));
        }
        events
    }

    fn open_notebook(&mut self, path: &Path) -> Result<(), String> {
        if self.data.path() == Some(path) {
            return Ok(());
        }
        self.data
            .save_if_modified()
            .map_err(|why| format!("the notebook open has changes that could not be saved: {why}"))?;
        self.data.open(path)
    }

    /// Answer every `run_cell` whose cell has stopped running.
    fn answer_finished_cells(&mut self) -> Vec<Event> {
        if self.waiting.is_empty() {
            return Vec::new();
        }

        // Each waited-on cell alone: every cell's outputs, cloned on every
        // beat a turn waits, were what reading one flag cost.
        let data = &self.data;
        let problem = data.problem().map(str::to_string);
        let mut done: Vec<(String, Result<String, String>)> = Vec::new();
        self.waiting.retain_mut(|waiting| {
            let Some(view) = data.cell_keyed(waiting.key) else {
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
                    None => (waiting.id.clone(), Ok(cell_result(&view))),
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
                        cell_result(&view)
                    )),
                ));
                return false;
            }
            true
        });

        if done.is_empty() {
            return Vec::new();
        }
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

    /// What a search tells the agent while the project's files are still being
    /// listed, having started the walk that lists them.
    ///
    /// Searched as it stands, an unlisted project finds nothing, and "no
    /// matches" is an answer the agent believes. The window connecting starts
    /// the walk, but a question can come before it lands, and in a home folder
    /// nothing starts it until something asks for the files. This is
    /// something asking.
    fn files_unlisted(&mut self) -> Option<String> {
        self.pickers.warm();
        self.pickers.is_scanning().then(|| {
            "the project's files are still being listed, so nothing was searched; \
             try again in a moment"
                .to_string()
        })
    }

    /// A search's answer, with what it could not search said after it.
    ///
    /// Over a home folder or anything broader, the walk stops at a limit and
    /// the files past it are never searched. Said plainly, "no matches" reads
    /// as an answer about the whole folder, and so does a list of hits that
    /// left out the one wanted. The agent believes both.
    fn as_far_as_listed(&self, answer: String) -> String {
        if !self.pickers.is_cut_short() {
            return answer;
        }
        format!(
            "{answer}\n(this folder is too broad to list whole: only {} of its files were \
             listed, and only those were searched)",
            self.pickers.file_count()
        )
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
                if let Some(unlisted) = self.files_unlisted() {
                    return Ok(unlisted);
                }
                let hits = self.pickers.grep(&query, count("limit", 40));
                if hits.is_empty() {
                    return Ok(self.as_far_as_listed(format!("no matches for {query:?}")));
                }
                Ok(self.as_far_as_listed(hits.join("\n")))
            }

            "find_files" => {
                let query = input.get("query").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(unlisted) = self.files_unlisted() {
                    return Ok(unlisted);
                }
                let found = self.pickers.find(query, count("limit", 40));
                if found.is_empty() {
                    return Ok(self.as_far_as_listed(format!("no files matching {query:?}")));
                }
                Ok(self.as_far_as_listed(found.join("\n")))
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
                if let Some(why) = self.library_excuse() {
                    return Ok(why);
                }
                let scope = self.asked_scope(input);
                // No vector: either embeddings are off, or this call arrived
                // by a path that does not defer. Lexical is the answer then,
                // and it is a complete one.
                self.library_answer(&query, count("limit", 6), &scope, &[])
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
                let existing = self.file_now(&path);

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
                // What it proposed is not in the cells until it is accepted.
                if self.data.path() == Some(path.as_path()) {
                    for proposal in self.data.proposals() {
                        out.push_str(&format!(
                            "# proposed, waiting for the user: {} (cell {})\n",
                            proposal.said, proposal.cell
                        ));
                    }
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

                // The notebook open in Data: proposed on the cell itself,
                // where it is accepted into the notebook being looked at. A
                // diff of the file's JSON, accepted into a buffer Data never
                // reads, was overwritten by Data's next save.
                if self.data.path() == Some(path.as_path()) {
                    let kind = match input.get("kind").and_then(|v| v.as_str()) {
                        Some("markdown") => proton_data::notebook::CellKind::Markdown,
                        _ => proton_data::notebook::CellKind::Code,
                    };
                    let source = match action {
                        "delete" => String::new(),
                        _ => text("source")?,
                    };
                    let said = self.data.propose(action, index, kind, &source)?;
                    if said == "that is already what the cell says" {
                        return Ok(said);
                    }
                    return Ok(format!(
                        "proposed to {said} in {} — shown on the cell for the user to accept or \
                         reject; nothing changes until they do, and cell numbers stay as they \
                         are until then",
                        path.display()
                    ));
                }

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

            // The workbook open in Excel: its edits held, as the window's are.
            name if crate::sheet::TOOLS.contains(&name) => {
                let root = self.root().to_path_buf();
                self.sheets.tool(&root, name, input)
            }

            other => Err(format!("no such tool: {other}")),
        }
    }

    /// What the agent's pane would show, in one number: the length of the
    /// conversation, what it is doing, what it has cost, and the last two
    /// entries whole — the only ones that grow while a turn runs.
    fn agent_fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        let transcript = self.agent.transcript();
        transcript.len().hash(&mut hash);
        for entry in transcript.iter().rev().take(2) {
            entry.hash(&mut hash);
        }
        format!("{:?} {:?} {} {}", self.agent.activity(), self.agent.usage(), self.agent.provider().id(), self.agent.model()).hash(&mut hash);
        hash.finish()
    }

    /// How the agent signs in, asked once a provider: see `credential`.
    fn credential_said(&self) -> String {
        let provider = self.agent.provider().id().to_string();
        let mut kept = self.credential.borrow_mut();
        if let Some((for_provider, said)) = kept.as_ref() {
            if *for_provider == provider {
                return said.clone();
            }
        }
        let said = self.agent.credential();
        *kept = Some((provider, said.clone()));
        said
    }

    /// The whole conversation, for the window to draw afresh: after a new
    /// chat, a chat opened, a project focused.
    fn agent_event(&self) -> Event {
        self.agent_from(0)
    }

    /// What changed of the conversation since the window was last sent it:
    /// the entries from the first that differs — while an answer streams,
    /// the one entry growing.
    fn agent_delta(&self) -> Event {
        let sent = self.agent_sent.borrow();
        let from = self.agent.transcript().iter().zip(sent.iter()).take_while(|(entry, was)| entry_hash(entry) == **was).count();
        drop(sent);
        self.agent_from(from)
    }

    fn agent_from(&self, from: usize) -> Event {
        let transcript = self.agent.transcript();
        *self.agent_sent.borrow_mut() = transcript.iter().map(entry_hash).collect();
        let usage = self.agent.usage();
        Event::Agent {
            transcript: transcript[from.min(transcript.len())..].to_vec(),
            from: from.min(transcript.len()) as u32,
            activity: self.agent.activity(),
            // The cache number earns its place in the UI: it is the only
            // visible sign that the prefix is still being hit, and a silent
            // miss is a cost regression rather than an error.
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            provider: self.agent.provider().id().to_string(),
            model: self.agent.model().to_string(),
            credential: self.credential_said(),
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
        // Notes get written while the editor is open — that is what a vault is
        // for — and until this they stayed invisible until somebody remembered
        // to press a button.
        self.library
            .set_watching(self.settings.bool("library.watch", true));
        // Off unless a model is named. It needs Ollama running and costs
        // minutes of local compute the first time, so it is not something to
        // start on somebody's behalf — and lexical search without it is a
        // complete answer, just a narrower one.
        let model = self.settings.string("library.embeddings", "");
        self.embedder = (!model.is_empty()).then(|| {
            proton_library::Embedder::new(&model).caching_in(proton_library::pdf::cache_dir())
        });
        self.library.embed_with(self.embedder.clone());
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

    /// Why the library cannot answer at all, when it cannot.
    ///
    /// Said rather than returning nothing, because "there is no library" and
    /// "nothing in the library about that" are different facts and only one of
    /// them is about the question.
    fn library_excuse(&self) -> Option<String> {
        if self.library.roots().is_empty() {
            return Some(
                "there is no library: the user has not listed any roots in \
                 Settings › Library, so there are no notes to search"
                    .into(),
            );
        }
        // A walk takes about a second on a vault of a thousand notes, and it
        // started when the session did — so this is only ever hit by a question
        // asked immediately after launch. Saying so is better than answering
        // "nothing found" about a library that has not been read yet.
        (!self.library.is_built())
            .then(|| "the library is still being read — ask again in a moment".into())
    }

    /// The scope a tool call asked for, or the user's own.
    ///
    /// Their choice is the default because they made it in a panel they can
    /// see; a model that wants one subject can still ask.
    fn asked_scope(&self, input: &serde_json::Value) -> proton_library::Scope {
        let asked: Vec<String> = input
            .get("sources")
            .and_then(|v| v.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|v| v.as_str())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        match asked.is_empty() {
            true => self.library_scope.clone(),
            false => proton_library::Scope::of(asked),
        }
    }

    /// The passages that answer a question, by words and — given a vector —
    /// by meaning too.
    fn library_answer(
        &self,
        query: &str,
        limit: usize,
        scope: &proton_library::Scope,
        vector: &[f32],
    ) -> Result<String, String> {
        let hits = match vector.is_empty() {
            true => self.library.search_in(query, limit, scope),
            false => self.library.search_both(query, vector, limit, scope),
        };
        if hits.is_empty() {
            // "Nothing found" and "not all of it has been read yet" are
            // different answers, and a walk over a corpus of PDFs runs for
            // minutes after the notes are already searchable.
            let (read, found) = self.library.progress();
            let more = match self.library.is_building() {
                true => format!(
                    " — still reading, {read} of {found} documents so far, \
                     so this may not be the whole library"
                ),
                false => String::new(),
            };
            // Which shelf was asked, when it was not all of them: the likeliest
            // reason an answer is empty is that the question went to the wrong
            // one.
            let where_ = match scope.is_everything() {
                true => format!(" ({} passages searched)", self.library.len()),
                false => format!(" within {}", scope.prefixes().join(", ")),
            };
            return Ok(format!(
                "nothing in the library about {query:?}{where_}{more}"
            ));
        }
        // The citation on its own line above the passage, so what to quote back
        // is unambiguous even after the text runs to sixty lines.
        Ok(proton_library::quote(&hits))
    }

    /// What a new conversation in the Chat tab starts with: the engine, the
    /// permission mode, and each engine's model and effort.
    fn follow_claude_settings(&mut self) {
        let chosen = |text: String| (!text.is_empty()).then_some(text);
        let mut models = std::collections::HashMap::new();
        for engine in proton_claude::Engine::ALL {
            let model = chosen(self.settings.string(&chat_setting(engine, "model"), ""));
            let effort = chosen(self.settings.string(&chat_setting(engine, "effort"), ""));
            models.insert(engine, (model, effort));
        }
        self.claude.configure(crate::claude::Defaults {
            engine: proton_claude::Engine::parse(&self.settings.string("chat.engine", "claude")).unwrap_or_default(),
            mode: self.settings.string("claude.mode", "default"),
            models,
        });
    }

    /// Point the agent at whatever the settings name.
    ///
    /// Called on startup and after any `agent.*` change, so switching provider
    /// takes effect on the next question rather than the next launch.
    fn follow_agent_settings(&mut self) {
        let provider = proton_agent::Provider::parse(&self.settings.string("agent.provider", ""));
        let model = self.settings.string("agent.model", "");
        self.agent.use_provider(provider, &model);
        self.agent
            .allow_edits(self.settings.bool("agent.antigravityEdits", false));
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

    /// Where debates are kept: beside the conversations, for the same reason.
    /// A debate is a document about this project and belongs with it.
    fn council_room(&self) -> PathBuf {
        self.pickers.root().join(".proton").join("council")
    }

    /// Put a question to every model that has a subscription.
    fn convene(&mut self, question: &str) -> Vec<Event> {
        if question.trim().is_empty() {
            return Vec::new();
        }
        if self.council.as_ref().is_some_and(|one| !one.is_done()) {
            return vec![notice(
                "a debate is already running, and each one spends three subscriptions".into(),
                NoticeLevel::Warn,
            )];
        }

        // Only members whose CLI is installed, and each handed the library
        // where its CLI can take one for a single run \u{2014} so the council
        // argues about this project's notes and cites them.
        let server = server_beside_us();
        // On the login shell's PATH: opened from the Dock, Proton has the
        // system's bare one, and none of the three CLIs is on it.
        let path = proton_claude::process::login_path();
        let members: Vec<proton_council::Member> = proton_council::Member::usual()
            .into_iter()
            .map(|member| member.with_path(path))
            .filter_map(proton_council::Member::installed)
            .map(|member| match &server {
                Some(server) => member.with_library(server, self.pickers.root()),
                None => member,
            })
            .collect();

        if members.len() < 2 {
            return vec![notice(
                format!(
                    "a council needs at least two of claude, codex and agy on PATH; {} found",
                    members.len()
                ),
                NoticeLevel::Error,
            )];
        }

        let room = self.council_room().join(slugged(question));
        // Said, where there is a library to argue from and no way to it.
        let unread = server.is_none() && !self.settings.library_roots(self.pickers.root()).is_empty();
        match proton_council::Running::start(question, members, &room) {
            Ok(running) => {
                self.council = Some(running);
                self.council_progress.clear();
                self.council_round = 1;
                self.council_brief.clear();
                self.council_showing.clear();
                self.council_problem.clear();
                let mut events = vec![self.council_event()];
                if unread {
                    events.push(notice(
                        "proton-mcp was not found, so the council argues without your library".into(),
                        NoticeLevel::Warn,
                    ));
                }
                events
            }
            Err(why) => {
                self.council_problem = why.clone();
                vec![notice(why, NoticeLevel::Error), self.council_event()]
            }
        }
    }

    /// Drain a debate, on the same heartbeat as everything else that happens
    /// on a thread the core cannot see.
    fn poll_council(&mut self) -> Vec<Event> {
        let Some(running) = self.council.as_mut() else {
            return Vec::new();
        };
        let updates = running.poll();
        if updates.is_empty() {
            return Vec::new();
        }
        for update in updates {
            match update {
                proton_council::Update::Round { round, said } => {
                    for one in said {
                        self.council_progress.push(match &one.text {
                            Ok(text) => format!(
                                "round {round} \u{2022} {} \u{2022} {} words",
                                one.member,
                                text.split_whitespace().count()
                            ),
                            Err(why) => format!(
                                "round {round} \u{2022} {} \u{2022} did not answer: {why}",
                                one.member
                            ),
                        });
                    }
                    self.council_round = round + 1;
                }
                proton_council::Update::Brief(Ok(brief)) => {
                    self.council_brief = brief;
                    self.council_round = 0;
                }
                proton_council::Update::Brief(Err(why)) => {
                    self.council_problem = why;
                    self.council_round = 0;
                }
            }
        }
        vec![self.council_event()]
    }

    fn council_event(&self) -> Event {
        Event::Council {
            held: proton_council::held(&self.council_room())
                .into_iter()
                .map(|one| crate::bus::DebateSummary {
                    name: one.name,
                    question: one.question,
                    finished: one.finished,
                })
                .collect(),
            running: self
                .council
                .as_ref()
                .filter(|one| !one.is_done())
                .map(|one| one.question().to_string())
                .unwrap_or_default(),
            round: self.council_round,
            progress: self.council_progress.clone(),
            brief: self.council_brief.clone(),
            showing: self.council_showing.clone(),
            problem: self.council_problem.clone(),
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
            meaning: self.library.meaning().say(),
            learning: self.library.meaning() == proton_library::Meaning::Coming,
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

    /// The whole screen, for the window to draw afresh.
    fn terminal_event(&mut self) -> Event {
        self.terminal_screen(false)
    }

    /// The screen's rows that changed since the window was last sent it.
    fn terminal_delta(&mut self) -> Event {
        self.terminal_screen(true)
    }

    fn terminal_screen(&mut self, delta: bool) -> Event {
        let Some(term) = self.terminal.as_mut() else {
            self.term_sent.clear();
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
        let hashes: Vec<u64> = screen.rows.iter().map(row_hash).collect();
        let sent = std::mem::replace(&mut self.term_sent, hashes);
        let fresh = !delta || sent.len() != screen.rows.len();
        Event::Terminal {
            rows: screen
                .rows
                .iter()
                .zip(&self.term_sent)
                .enumerate()
                .map(|(i, (row, hash))| (fresh || sent[i] != *hash).then(|| row.iter().map(term_run).collect()))
                .collect(),
            cursor: screen.cursor,
            cursor_visible: screen.cursor_visible && running,
            running,
            scrollback: screen.scrollback,
        }
    }

    /* ── debugging ────────────────────────────────────────────────── */

    fn debug_event(&self) -> Event {
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
                    // Under either of its names, because an adapter reports
                    // where the program really ran.
                    path: frame.path.as_ref().map(|path| self.root.shown(path)),
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

    /// These cells of the notebook, as they are now.
    fn notebook_cells_event(&self, keys: &[u64]) -> Event {
        Event::NotebookCells {
            path: self.data.path().map(|p| p.display().to_string()).unwrap_or_default(),
            cells: self.data.cells_keyed(keys),
            modified: self.data.is_modified(),
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
            language: self.data.language(),
            modified: self.data.is_modified(),
            proposals: self.data.proposals(),
        }
    }

    /// What the repository has right now.
    ///
    /// Asked of git each time rather than kept, which is the same discipline
    /// the gutter and the file tree follow. Git's answer moves for reasons
    /// that never pass through this core — a commit in the terminal panel, a
    /// branch switched from outside — so a remembered list would be stale
    /// exactly when somebody looked at it to find out what had happened.
    ///
    /// The branch now, with the changes as last asked; git is asked again on
    /// a worker, and its answer goes when it comes if it is different.
    fn repo_event(&self) -> Event {
        self.ask_repo();
        Event::Repo {
            branch: self.git.branch(),
            files: self.repo_asked.borrow().files.clone(),
        }
    }

    fn ask_repo(&self) {
        let mut asked = self.repo_asked.borrow_mut();
        if asked.asking.is_some() {
            // Asked while asking: what changed since the asking began is
            // asked about once it has answered.
            asked.again = true;
            return;
        }
        let Some(asking) = self.git.asking() else {
            asked.files.clear();
            return;
        };
        let (done, answer) = std::sync::mpsc::channel();
        let started = std::thread::Builder::new().name("proton-git-status".into()).spawn(move || {
            let _ = done.send(asking.changes());
        });
        if started.is_ok() {
            asked.asking = Some(answer);
        }
    }

    /// Whether git is being asked for the project's changes.
    pub fn repo_asking(&self) -> bool {
        self.repo_asked.borrow().asking.is_some()
    }

    /// Git's answer, when it has come and says something new.
    fn poll_repo(&mut self) -> Option<Event> {
        let mut asked = self.repo_asked.borrow_mut();
        let answer = match asked.asking.as_ref()?.try_recv() {
            Ok(changes) => changes,
            Err(std::sync::mpsc::TryRecvError::Empty) => return None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                asked.asking = None;
                return None;
            }
        };
        asked.asking = None;
        let files: Vec<crate::bus::RepoFile> = answer
            .into_iter()
            .map(|change| crate::bus::RepoFile { path: change.path, state: change.state.letter().to_string() })
            .collect();
        let again = std::mem::take(&mut asked.again);
        let changed = files != asked.files;
        asked.files = files;
        drop(asked);
        if again {
            self.ask_repo();
        }
        changed.then(|| Event::Repo { branch: self.git.branch(), files: self.repo_asked.borrow().files.clone() })
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

        // Research reads its folders again: a brief or a hypothesis edited
        // by hand since is what the board should show.
        if mode == ModeId::Research {
            events.extend(self.research().sync());
        }
        // Excel shows the project's workbooks, and the one open.
        if mode == ModeId::Sheet {
            let root = self.root().to_path_buf();
            events.extend(self.sheets.events(&root));
        }
        // And Circuit its circuit projects.
        if mode == ModeId::Circuit {
            let root = self.root().to_path_buf();
            events.extend(self.circuits.events(&root));
        }

        events
    }

    /// ⌘V: the system clipboard's text into the buffer, as vim's `"+`
    /// register would put it — typed in at the cursor as it was copied in
    /// insert mode (no indent added, no bracket closed: it is already what it
    /// should be), before the cursor in normal mode (`P`; on the line above
    /// when it is whole lines), and in place of the selection in visual mode.
    /// One undo step, whatever it was.
    fn paste(&mut self, text: String) -> Vec<Event> {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if text.is_empty() || !self.mode.edits_text() || self.pickers.is_open() {
            return Vec::new();
        }
        let before = self.vim.mode();
        let visual = matches!(before, VimMode::Visual | VimMode::VisualLine | VimMode::VisualBlock);
        if !visual && !matches!(before, VimMode::Insert | VimMode::Replace | VimMode::Normal) {
            return Vec::new();
        }
        self.popup.close();
        let mut events = Vec::new();
        let buf = self.editor.buffer_mut();
        buf.end_group();
        buf.begin_group(false);
        if visual {
            // The selection goes, as `d` would take it, and the paste lands
            // where it began.
            let held = (self.editor.active(), self.editor.buffer().cursor);
            let (evts, _) = self.act(&Action::new(Verb::Delete, Object::Selection, 1), before, false);
            events.extend(evts);
            self.vim.keys(&Key { key: "Escape".into(), ctrl: false, alt: false, shift: false, meta: false });
            self.last_visual = Some((held.0, held.1.anchor, held.1.offset, before));
        }
        let buf = self.editor.buffer_mut();
        let n = text.chars().count();
        let (at, cursor) = match before {
            VimMode::Insert | VimMode::Replace => (buf.cursor.offset, buf.cursor.offset + n),
            // Whole lines go above the line, the cursor at their start.
            _ if text.ends_with('\n') && !visual => (buf.line_start(), buf.line_start()),
            // Else before the cursor, which ends on the last character put.
            _ => (buf.cursor.offset, buf.cursor.offset + n.saturating_sub(1)),
        };
        buf.apply(vec![proton_text::Edit::insert(at, text)], proton_text::Cursor::at(cursor));
        buf.end_group();
        if self.vim.mode() != before {
            events.push(self.vim_event());
        }
        events.extend(self.after_action(true));
        self.resync();
        events
    }

    /// Run an action against the active mode, reporting whether text changed.
    ///
    /// Only Codex edits text today; the other modes accept the action and
    /// report it, and gain their own nouns with their document types.
    fn act(&mut self, action: &Action, was: VimMode, replay: bool) -> (Vec<Event>, bool) {
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
            // A move changes only which cell is marked; a yank, nothing on
            // screen. Everything else is the notebook again.
            match action.verb {
                Verb::Move => events.push(Event::CellSelected { index: self.data.selected() }),
                Verb::Yank => {}
                _ if changed => events.push(self.notebook_event()),
                _ => {}
            }
            return (events, false);
        }

        if self.mode.edits_text()
            && !replay
            && action.verb == Verb::Move
            && matches!(was, VimMode::Insert | VimMode::Replace)
        {
            self.editor.buffer_mut().end_group();
        }
        // `>` and `<` shift by the file's own indent — the same one Tab types —
        // and `o` and `O` open a line a level in where the code calls for it.
        if matches!(action.verb, Verb::Indent | Verb::Outdent)
            || (action.verb == Verb::Insert && matches!(action.object, Object::LineBelow | Object::LineAbove))
        {
            let unit = self.indent().unit();
            self.editor.set_indent(&unit);
        }
        let changed = self.mode.edits_text() && self.editor.execute(action, was);

        // An action that opens insert mode starts an undo group, so the typing
        // that follows is part of the same change: `ciwfoo<Esc>` is one thing
        // the user did and one press of `u` should take it back.
        //
        // This keys off the verb rather than off a mode transition, because by
        // the time an action reaches here the engine has *already* switched to
        // insert — the operator sets the mode as it resolves.
        if self.mode.edits_text()
            && !replay
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
    fn events_for(&mut self, outcome: Outcome, was: VimMode, replay: bool) -> (Vec<Event>, bool) {
        match outcome {
            Outcome::Action(action) => self.act(&action, was, replay),
            Outcome::Page { down, half, count } => {
                // Half the view for `⌃d` and `⌃u`; the view less two lines of
                // overlap for `⌃f`, `⌃b` and the page keys — vim's arithmetic.
                // In visual mode it extends the selection, like any motion.
                let lines = self.splits.view().lines.max(1);
                let step = match half {
                    true => (lines / 2).max(1),
                    false => lines.saturating_sub(2).max(1),
                } * count.max(1) as usize;
                let object = if down { Object::LineDown } else { Object::LineUp };
                self.act(&Action::new(Verb::Move, object, step as u32), was, replay)
            }
            Outcome::Reselect => {
                let Some((buffer, anchor, offset, kind)) = self.last_visual else {
                    return (Vec::new(), false);
                };
                if buffer != self.editor.active() || !self.mode.edits_text() {
                    return (Vec::new(), false);
                }
                let buf = self.editor.buffer_mut();
                let len = buf.len_chars();
                buf.cursor = buf.clamp(proton_text::Cursor {
                    offset: offset.min(len),
                    anchor: anchor.min(len),
                    goal_column: None,
                });
                self.vim.reselect(kind);
                (Vec::new(), false)
            }
            Outcome::Indent => {
                if !self.mode.edits_text() {
                    return (Vec::new(), false);
                }
                let indent = self.indent_here();
                self.editor.buffer_mut().ensure_group();
                for c in indent.chars() {
                    self.editor.insert_char(c);
                }
                (Vec::new(), true)
            }
            Outcome::Input(c) => {
                if self.mode.edits_text() {
                    self.editor.buffer_mut().ensure_group();
                    match was {
                        VimMode::Insert => self.type_char(c),
                        _ => self.editor.insert_char(c),
                    }
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
                tab_size: self.tab_size() as u32,
            };
        };
        let first_line = pane.first_line;
        let count = pane.lines;
        let buffer = pane
            .buffer
            .min(self.editor.buffers().len().saturating_sub(1));
        let tab_size = self.tab_drawn(buffer) as u32;

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
            tab_size,
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
        // Visual mode selects from the moment it starts — `V` lights the whole
        // line and `v` the character under the cursor before anything has
        // moved, as vim's do — and that is exactly when the anchor and the
        // head are still in one place, so a selection judged by the distance
        // between them was no selection at all, and `V` looked like it had
        // done nothing.
        let selecting = focused && self.vim.is_selecting();
        let selection = (selecting || cursor.has_selection()).then(|| {
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

/// Where an engine's model or effort is saved: Claude Code's where they
/// always were, the others' beside the engine chosen for new conversations.
fn chat_setting(engine: proton_claude::Engine, what: &str) -> String {
    match engine {
        proton_claude::Engine::Claude => format!("claude.{what}"),
        other => format!("chat.{}.{what}", other.key()),
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

/// Where `proton-mcp` is, so a council member can be handed the library:
/// see [`proton_council::mcp_server`].
fn server_beside_us() -> Option<PathBuf> {
    proton_council::mcp_server(&std::env::current_exe().ok()?)
}

/// A directory name out of a question.
///
/// Windows refuses `?` in a filename and a question usually ends in one, so
/// this is the difference between keeping the debate and losing it.
fn slugged(question: &str) -> String {
    let words: String = question
        .chars()
        .map(|c| match c.is_alphanumeric() {
            true => c.to_ascii_lowercase(),
            false => '-',
        })
        .collect();
    let short: Vec<&str> = words
        .split('-')
        .filter(|part| !part.is_empty())
        .take(8)
        .collect();
    match short.is_empty() {
        true => "debate".into(),
        false => short.join("-"),
    }
}

/// Open a web address in the browser — the only kind of link Proton follows
/// out of a conversation.
fn open_link(url: &str) -> Result<(), String> {
    // A web address and nothing else: a link in a reply is the model's
    // writing, and `file:` or a custom scheme from it is not something to
    // hand to the desktop.
    let lower = url.trim().to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) || url.chars().any(char::is_whitespace) {
        return Err(format!("not a web address: {url}"));
    }
    let program = match () {
        _ if cfg!(windows) => "explorer",
        _ if cfg!(target_os = "macos") => "open",
        _ => "xdg-open",
    };
    std::process::Command::new(program)
        .arg(url.trim())
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("could not open {url}: {err}"))
}

/// Hand a file to whatever the desktop opens it with.
///
/// The only place Proton starts something that is not Proton, and it exists
/// for one case: a citation by page into a file the document viewer cannot
/// show. Opened as text it would fill a buffer with binary, so following that
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
/// A row of the terminal, as a number that moves when it does.
fn row_hash(row: &Vec<proton_term::Run>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    row.hash(&mut hash);
    hash.finish()
}

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
            // Nor the SVG's text: a figure's paths are as long, and no more
            // telling.
            crate::data::OutputView::Svg { .. } => text.push_str("<image/svg+xml>"),
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
                // A string, or — saved by Jupyter — a list of lines.
                if let Some(plain) = data.get("text/plain").and_then(crate::data::as_text) {
                    text.push_str(&plain);
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
    /// The conversation the agent panel had open: its file in
    /// `.proton/chats/`, relative to the project. The conversation itself is
    /// already in that file; this is only which one was on screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<PathBuf>,
}

/// One pane, as stored.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct StoredView {
    /// Which of `files` it was showing.
    pub file: usize,
    /// An unnamed pane, rather than the first file. Absent in older stores.
    #[serde(default)]
    pub scratch: bool,
    #[serde(default)]
    pub first_line: usize,
    /// The cursor, as a character offset into that file.
    #[serde(default)]
    pub line: usize,
}

/// A file name's extension, as the typing rules know languages by.
fn extension_of(name: &str) -> String {
    name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()).unwrap_or_default()
}


/// Whether a file is LaTeX's — a `.tex`, a `.bib`, a package or a class:
/// texlab completes on its own characters there, and is not asked to format
/// it, which it does with latexindent, seldom installed.
fn is_latex(path: &Path) -> bool {
    const LATEX: [&str; 10] = ["tex", "bib", "sty", "cls", "bst", "bbx", "cbx", "ltx", "def", "cfg"];
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| LATEX.contains(&e))
}

#[cfg(test)]
mod tests {
    //! The agent's staged changes, in a workspace whose root is not spelled
    //! the way the editor spells the files it opens.
    //!
    //! The editor canonicalises a file when it opens it, and a workspace joins
    //! every path the agent or the review panel names to its root as it was
    //! given. The session roots a project by its real path, but a workspace
    //! does not insist on one: through a link — on a Mac, anything under
    //! `/tmp` or `/var` — the two spellings disagree, and `folder::Root` is
    //! what reconciles them. These check that end to end, and live here
    //! because CI runs the library's tests and not `tests/staging.rs`.
    //!
    //! A `..` in the root disagrees in the same way on every platform, and is
    //! what most of these use: it needs neither the working directory changed
    //! under the other tests nor, on Windows, the privilege a link does.
    //!
    //! The later sections are about other things — where a file ends, and
    //! what the agent's searches see before the files are listed — and live
    //! here for the same reason.

    use super::*;

    const SOURCE: &str = "fn one() {\n    1\n}\n\nfn two() {\n    2\n}\n";

    /// A folder for one test, and everything that names it, removed together.
    struct Project {
        base: PathBuf,
    }

    impl Project {
        fn new(tag: &str) -> Project {
            let base = std::env::temp_dir().join(format!(
                "proton-spelling-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(base.join("project").join("sub")).unwrap();
            Project { base }
        }

        /// The project, named the way it was made.
        fn folder(&self) -> PathBuf {
            self.base.join("project")
        }

        /// The same folder, named the long way round.
        fn around(&self) -> PathBuf {
            self.folder().join("sub").join("..")
        }

        /// The same folder again, through a link beside it.
        #[cfg(unix)]
        fn linked(&self) -> PathBuf {
            let link = self.base.join("link");
            std::os::unix::fs::symlink(self.folder(), &link).unwrap();
            link
        }

        /// The same folder by its real path, the spelling the editor gives
        /// the files it opens, for a test that is about something else.
        fn real(&self) -> PathBuf {
            crate::folder::normalise(&self.folder())
        }
    }

    impl Drop for Project {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    /// A file of `text` opened in a project of its own, the cursor at its
    /// start, in normal mode.
    fn opened(tag: &str, name: &str, text: &str) -> (Project, Workspace) {
        let project = Project::new(tag);
        std::fs::write(project.folder().join(name), text).unwrap();
        let mut ws = Workspace::rooted(project.real());
        ws.apply(Command::OpenBuffer { path: name.into() });
        (project, ws)
    }

    /// Press these: characters as typed, `\n` as Enter, `\x08` as Backspace,
    /// `\t` as Tab and `\x1b` as Escape.
    fn press(ws: &mut Workspace, keys: &str) {
        for c in keys.chars() {
            let key = match c {
                '\n' => Key::named("Enter"),
                '\x08' => Key::named("Backspace"),
                '\t' => Key::named("Tab"),
                '\x1b' => Key::named("Escape"),
                c => Key::char(c),
            };
            ws.apply(Command::Key { key });
        }
    }

    fn contents(ws: &Workspace) -> String {
        ws.editor().buffer().contents()
    }

    #[test]
    fn brackets_close_as_they_are_typed_and_open_into_an_indented_block() {
        let (_project, mut ws) = opened("pairs", "main.rs", "fn main() \n");
        // Typed in, the closing halves step over the ones already there.
        press(&mut ws, "A{\nlet v = vec![(1, \"a\")];\x1b");
        assert_eq!(contents(&ws), "fn main() {\n    let v = vec![(1, \"a\")];\n}\n");
        // Backspace in an empty pair takes both halves.
        press(&mut ws, "ox(\x08\x1b");
        assert_eq!(contents(&ws), "fn main() {\n    let v = vec![(1, \"a\")];\n    x\n}\n");
        // `o` below a line that opens a block goes a level in, and a closing
        // bracket typed there goes back to the block's own indentation.
        press(&mut ws, "ggo\x1b");
        assert_eq!(ws.editor().buffer().line(1), "    ");
        press(&mut ws, "ddggo}\x1b");
        assert_eq!(ws.editor().buffer().line(1), "}");
        // With the setting off, a bracket is only itself.
        ws.settings = crate::settings::Settings::saying(serde_json::json!({"editor": {"autoClosingBrackets": false}}));
        press(&mut ws, "ddGOf(\x1b");
        assert!(contents(&ws).contains("\nf(\n"), "{:?}", contents(&ws));
    }

    #[test]
    fn tab_and_enter_follow_the_files_own_indentation() {
        let (project, mut ws) = opened("two", "app.js", "function a() {\n  return 1;\n}\n");
        press(&mut ws, "jA\nif (b) {\n\tc();\x1b");
        assert_eq!(contents(&ws), "function a() {\n  return 1;\n  if (b) {\n      c();\n  }\n}\n");
        // An `.editorconfig` says otherwise, and is believed — and says how
        // wide a tab is drawn.
        std::fs::write(project.folder().join(".editorconfig"), "root = true\n[*.go]\nindent_style = tab\ntab_width = 8\n").unwrap();
        std::fs::write(project.folder().join("main.go"), "package main\n").unwrap();
        let events = ws.apply(Command::OpenBuffer { path: "main.go".into() });
        assert!(events.iter().any(|e| matches!(e, Event::Viewport { tab_size: 8, .. })), "{events:?}");
        press(&mut ws, "ggofunc main() {\nx\x1b");
        assert_eq!(contents(&ws), "package main\nfunc main() {\n\tx\n}\n");
    }

    #[cfg(unix)]
    #[test]
    fn a_save_is_whole_and_through_a_link_writes_the_file_it_leads_to() {
        use std::os::unix::fs::PermissionsExt;
        let project = Project::new("save");
        let real = project.folder().join("real.txt");
        std::fs::write(&real, "one\n").unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).unwrap();
        std::os::unix::fs::symlink(&real, project.folder().join("link.txt")).unwrap();
        // Only the link is ever opened.
        let mut ws = Workspace::rooted(project.real());
        ws.apply(Command::OpenBuffer { path: "link.txt".into() });
        assert!(ws.editor().buffer().path().is_some_and(|p| p.ends_with("link.txt") || p.ends_with("real.txt")));
        press(&mut ws, "Atwo\x1b:w\n");
        let link = std::fs::symlink_metadata(project.folder().join("link.txt")).unwrap();
        assert!(link.file_type().is_symlink(), "the link was replaced by a file");
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "onetwo\n");
        assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o640);
        let left: Vec<_> = std::fs::read_dir(project.folder())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains("proton-saving"))
            .collect();
        assert!(left.is_empty(), "{left:?}");

        // A writable file in a folder nothing new may be made in is still
        // saved, in place.
        let shut = project.folder().join("shut");
        std::fs::create_dir(&shut).unwrap();
        std::fs::write(shut.join("a.txt"), "a\n").unwrap();
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o555)).unwrap();
        ws.apply(Command::OpenBuffer { path: "shut/a.txt".into() });
        press(&mut ws, "Ab\x1b:w\n");
        let saved = std::fs::read_to_string(shut.join("a.txt"));
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(saved.unwrap(), "ab\n");
    }

    #[test]
    fn a_crlf_file_keeps_its_line_endings_as_lines_are_opened_and_joined() {
        let (_project, mut ws) = opened("crlf", "a.c", "int a() {\r\n}\r\n");
        press(&mut ws, "A\nreturn 0;\x1b");
        assert_eq!(contents(&ws), "int a() {\r\n    return 0;\r\n}\r\n");
        press(&mut ws, "o\x1b");
        assert_eq!(contents(&ws), "int a() {\r\n    return 0;\r\n    \r\n}\r\n");
        // A level of indentation goes with one Backspace, and the line ending
        // after it with one more, both halves of it.
        press(&mut ws, "I\x08\x08\x1b");
        assert_eq!(contents(&ws), "int a() {\r\n    return 0;\r\n}\r\n");
    }

    #[test]
    fn the_agents_edits_to_the_open_notebook_are_proposed_on_its_cells_and_kept() {
        use proton_data::notebook::{CellKind, Notebook};
        let project = Project::new("proposals");
        let mut one = Notebook::blank();
        one.push(CellKind::Code, "a = 1");
        one.write(&project.folder().join("one.ipynb")).unwrap();
        Notebook::blank().write(&project.folder().join("two.ipynb")).unwrap();
        let mut ws = Workspace::rooted(project.real());
        ws.apply(Command::OpenBuffer { path: "one.ipynb".into() });

        let said = ws
            .run_tool("edit_cell", &serde_json::json!({"path": "one.ipynb", "cell": 0, "source": "a = 2"}))
            .unwrap();
        assert!(said.contains("proposed"), "{said}");
        assert!(ws.review.is_empty(), "no diff of the file's JSON staged: {:?}", ws.review);
        let id = ws.data.proposals()[0].id;
        let events = ws.apply(Command::AnswerProposal { id, accept: true });
        assert!(events.iter().any(|e| matches!(e, Event::Notebook { modified: true, .. })), "{events:?}");
        assert_eq!(ws.data.cells()[0].source, "a = 2");

        // Opening another notebook saves this one first rather than losing it.
        ws.apply(Command::OpenBuffer { path: "two.ipynb".into() });
        assert_eq!(Notebook::read(&project.folder().join("one.ipynb")).unwrap().cells()[0].source, "a = 2");
        // And quitting saves the one open.
        ws.data.add_cell(true);
        ws.shutdown();
        assert_eq!(Notebook::read(&project.folder().join("two.ipynb")).unwrap().len(), 1);
    }

    /// Call `edit_file` the way the agent's tool runner does.
    fn edit(ws: &mut Workspace, path: &str, old: &str, new: &str) -> Result<String, String> {
        ws.run_tool(
            "edit_file",
            &serde_json::json!({ "path": path, "old_text": old, "new_text": new }),
        )
    }

    /// Press each of these keys, then Escape.
    fn typed(ws: &mut Workspace, keys: &str) {
        for c in keys.chars() {
            ws.apply(Command::Key { key: Key::char(c) });
        }
        ws.apply(Command::Key {
            key: Key::named("Escape"),
        });
    }

    /// Every buffer with a file behind it. A lookup that compares spellings
    /// ends up with one file open in two of them.
    fn files(ws: &Workspace) -> Vec<PathBuf> {
        ws.editor()
            .buffers()
            .iter()
            .filter_map(|b| b.path())
            .map(Path::to_path_buf)
            .collect()
    }

    /// Stage two hunks against a buffer with unsaved work in it, then refuse
    /// one and take the other, in a workspace rooted at `root`.
    fn review_the_buffer(root: &Path) {
        let mut ws = Workspace::rooted(root);
        ws.apply(Command::OpenBuffer {
            path: "a.rs".into(),
        });
        // A line only the buffer has. Matching against it proves which one
        // was read: a lookup that missed the buffer reads the disk, and is
        // told the line is not in the file.
        typed(&mut ws, "Otyped");

        edit(&mut ws, "a.rs", "typed\n", "TYPED\n").unwrap();
        edit(&mut ws, "a.rs", "    2\n", "    22\n").unwrap();
        let staged = ws.staged();
        assert_eq!(staged.len(), 1, "one file: {staged:?}");
        assert_eq!(staged[0].path, "a.rs", "named relative to the project");
        assert_eq!(staged[0].hunks.len(), 2, "{:?}", staged[0].hunks);

        // The panel answers with the path it showed.
        ws.apply(Command::RejectHunk {
            path: "a.rs".into(),
            hunk: 1,
        });
        ws.apply(Command::ApplyHunk {
            path: "a.rs".into(),
            hunk: 0,
        });

        // Into the buffer that was typed in, rather than a second copy of the
        // file read from the disk.
        assert_eq!(files(&ws).len(), 1, "{:?}", files(&ws));
        assert_eq!(ws.editor().buffer().contents(), format!("TYPED\n{SOURCE}"));
        assert!(ws.staged().is_empty(), "{:?}", ws.staged());
    }

    #[test]
    fn a_review_is_of_the_buffer_however_the_project_was_named() {
        let project = Project::new("named");
        std::fs::write(project.folder().join("a.rs"), SOURCE).unwrap();
        review_the_buffer(&project.around());
    }

    #[cfg(unix)]
    #[test]
    fn a_review_is_of_the_buffer_in_a_project_reached_through_a_link() {
        // How this was found: on a Mac the temporary folder is under `/var`,
        // which is a link into `/private`. Given a link of its own here, so it
        // is tested where `/tmp` is a real folder too.
        let project = Project::new("linked");
        std::fs::write(project.folder().join("a.rs"), SOURCE).unwrap();
        review_the_buffer(&project.linked());
    }

    #[test]
    fn the_agent_and_the_panel_can_name_one_file_two_ways() {
        // The agent can be handed a buffer's own path, which is canonical,
        // and the panel answers with the one it showed, which is joined to the
        // root as it was given. They are one change: the second edit stacks on
        // the first, in one row named relative to the project.
        let project = Project::new("twice");
        std::fs::write(project.folder().join("a.rs"), SOURCE).unwrap();
        let mut ws = Workspace::rooted(project.around());
        let canonical = crate::folder::normalise(&project.folder().join("a.rs"));
        let canonical = canonical.display().to_string();

        edit(&mut ws, &canonical, "    1\n", "    11\n").unwrap();
        edit(&mut ws, "a.rs", "    2\n", "    22\n").unwrap();
        let staged = ws.staged();
        assert_eq!(staged.len(), 1, "{staged:?}");
        assert_eq!(staged[0].path, "a.rs");
        assert_eq!(staged[0].hunks.len(), 2, "{:?}", staged[0].hunks);

        ws.apply(Command::ApplyStaged {
            path: Some("a.rs".into()),
        });
        assert_eq!(
            ws.editor().buffer().contents(),
            "fn one() {\n    11\n}\n\nfn two() {\n    22\n}\n"
        );
        assert!(ws.staged().is_empty(), "{:?}", ws.staged());
    }

    #[test]
    fn a_file_the_agent_created_is_one_buffer_once_it_is_written() {
        // A buffer made for a file that is not there yet keeps the path it was
        // resolved to, and once the file exists the editor opens it by its
        // canonical path. Compared as text, the next edit opened the file a
        // second time, and accepting it landed in a copy nobody was looking at.
        let project = Project::new("created");
        let mut ws = Workspace::rooted(project.around());
        edit(&mut ws, "new.rs", "", "fn fresh() {}").unwrap();
        ws.apply(Command::ApplyStaged { path: None });
        ws.apply(Command::Save);
        assert!(project.folder().join("new.rs").exists(), "`:w` wrote it");

        edit(&mut ws, "new.rs", "fresh", "fresher").unwrap();
        ws.apply(Command::ApplyStaged { path: None });
        assert_eq!(files(&ws).len(), 1, "{:?}", files(&ws));
        assert_eq!(ws.editor().buffer().contents(), "fn fresher() {}");
        assert!(ws.staged().is_empty(), "{:?}", ws.staged());
    }

    /* ── where a file ends ────────────────────────────────────────── */

    // The diff counts a newline as part of the line it ends, so a hunk on a
    // file's last line covers the file's final newline as well. Accepting it
    // has to leave the file ending the way the target does. When it did not,
    // the newline was gone and the diff still had one hunk to offer, and
    // accepting that hunk changed nothing, so it could never be cleared.

    #[test]
    fn accepting_a_change_to_the_last_line_keeps_the_newline_after_it() {
        let project = Project::new("last-line");
        std::fs::write(project.folder().join("a.rs"), "one\ntwo\n").unwrap();
        let mut ws = Workspace::rooted(project.real());
        edit(&mut ws, "a.rs", "two", "TWO").unwrap();

        ws.apply(Command::ApplyStaged { path: None });
        assert_eq!(ws.editor().buffer().contents(), "one\nTWO\n");
        assert!(ws.staged().is_empty(), "{:?}", ws.staged());
        // Forgotten, rather than kept with nothing left to show.
        assert!(ws.review.is_empty(), "{:?}", ws.review);
    }

    #[test]
    fn accepting_a_file_the_agent_created_keeps_its_final_newline() {
        // The buffer starts empty, so the one hunk replaces nothing at the
        // very end of the file, and everything it brings is the last line.
        let project = Project::new("new-file");
        let mut ws = Workspace::rooted(project.real());
        edit(&mut ws, "new.rs", "", "fn fresh() {}\n").unwrap();

        ws.apply(Command::ApplyStaged { path: None });
        assert_eq!(ws.editor().buffer().contents(), "fn fresh() {}\n");
        assert!(ws.staged().is_empty(), "{:?}", ws.staged());
        assert!(ws.review.is_empty(), "{:?}", ws.review);
    }

    /// Poll until a pending formatting has landed, or give up.
    fn settle_format(ws: &mut Workspace) -> Vec<Event> {
        let mut events = Vec::new();
        for _ in 0..500 {
            events.extend(ws.apply(Command::Poll));
            if ws.external_format.is_none() {
                return events;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("clang-format never answered");
    }

    #[cfg(unix)]
    #[test]
    fn c_with_no_server_to_format_it_is_formatted_by_clang_format_on_fmt_and_on_save_when_asked() {
        use std::os::unix::fs::PermissionsExt;
        let project = Project::new("clang-format");
        let root = project.real();
        std::fs::write(root.join("main.c"), "int main(){\nreturn 0;\n}\n").unwrap();
        // A stand-in that reformats the way clang-format would, so the test
        // needs no LLVM: it takes the text on stdin and the name it is for.
        let fake = project.base.join("clang-format");
        std::fs::write(
            &fake,
            "#!/bin/sh\ncase \"$1\" in --assume-filename=*main.c) sed -e 's/^int main(){$/int main() {/' -e 's/^return/  return/' ;; *) exit 2 ;; esac\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut ws = Workspace::rooted(&root);
        ws.use_clang_format(Some(fake));
        ws.apply(Command::OpenBuffer { path: "main.c".into() });
        // `:fmt` formats the buffer, unsaved edits and all, and does not write.
        typed(&mut ws, "ggOint x;");
        ws.apply(Command::Format);
        let events = settle_format(&mut ws);
        assert_eq!(ws.editor().buffer().contents(), "int x;\nint main() {\n  return 0;\n}\n");
        assert!(events.iter().any(|e| matches!(e, Event::Notice { text, .. } if text == "formatted main.c")), "{events:?}");
        assert_eq!(std::fs::read_to_string(root.join("main.c")).unwrap(), "int main(){\nreturn 0;\n}\n", "`:fmt` wrote the file");
        typed(&mut ws, "u");
        assert_eq!(ws.editor().buffer().contents(), "int x;\nint main(){\nreturn 0;\n}\n", "one `u` does not take the formatting back");

        // A save writes C as it is written: formatting it on save is asked for.
        typed(&mut ws, "u");
        ws.apply(Command::Save);
        settle_format(&mut ws);
        assert_eq!(std::fs::read_to_string(root.join("main.c")).unwrap(), "int main(){\nreturn 0;\n}\n", "a save formatted C unasked");
        // Asked, a save formats, and writes what it formatted.
        ws.apply(Command::SetSetting { key: "editor.formatC".into(), value: serde_json::json!(true) });
        typed(&mut ws, "ggOint x;");
        ws.apply(Command::Save);
        settle_format(&mut ws);
        assert_eq!(std::fs::read_to_string(root.join("main.c")).unwrap(), "int x;\nint main() {\n  return 0;\n}\n");

        // Nothing to format with: `:fmt` says so, a save says nothing.
        let mut bare = Workspace::rooted(&root);
        bare.use_clang_format(None);
        bare.apply(Command::OpenBuffer { path: "main.c".into() });
        let said = bare.apply(Command::Format);
        assert!(said.iter().any(|e| matches!(e, Event::Notice { text, .. } if text == "no formatter for this file")), "{said:?}");
    }

    /* ── the agent's searches, before the files are listed ───────────── */

    #[test]
    fn a_search_before_the_files_are_listed_lists_them_and_says_so() {
        // The window connecting starts the walk everywhere but a home folder,
        // and a question can come before it lands. Searched as it stood, the
        // empty list answered "no matches", which the agent believes; in a
        // home folder it would have gone on answering that for good.
        let project = Project::new("unlisted");
        std::fs::write(project.folder().join("a.rs"), SOURCE).unwrap();
        let mut ws = Workspace::rooted(project.real());

        for (tool, query) in [("find_files", "a.rs"), ("grep", "fn one")] {
            let said = ws
                .run_tool(tool, &serde_json::json!({ "query": query }))
                .unwrap();
            assert!(said.contains("still being listed"), "{tool}: {said}");
        }
        assert!(ws.pickers().is_scanning(), "and the walk was started");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while ws.pickers().is_scanning() {
            ws.apply(Command::Poll);
            assert!(std::time::Instant::now() < deadline, "the walk never finished");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let found = ws.run_tool("find_files", &serde_json::json!({ "query": "a.rs" }));
        assert_eq!(found.as_deref(), Ok("a.rs"));
        let hits = ws
            .run_tool("grep", &serde_json::json!({ "query": "fn one" }))
            .unwrap();
        assert!(hits.starts_with("a.rs:1:"), "{hits}");
    }

    /* ── a folder too broad to list whole ────────────────────────────── */

    /// Pump the heartbeat until the walk lands, and say what the open picker
    /// was told when it did: how many files it is choosing from, and whether
    /// that is fewer than the folder holds.
    fn listed(ws: &mut Workspace) -> (usize, bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut told = None;
        while ws.pickers().is_scanning() {
            for event in ws.apply(Command::Poll) {
                if let Event::Picker {
                    total, cut_short, ..
                } = event
                {
                    told = Some((total, cut_short));
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the walk never finished"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        told.expect("the walk landed and the picker was not told")
    }

    #[test]
    fn a_walk_cut_short_says_so_to_the_picker_and_to_the_agent() {
        // Over a home folder or anything broader the walk stops at a limit,
        // and which files it had reached by then changes from one launch to
        // the next. A file it never reached looks just like one that does not
        // exist, to somebody reading the picker and to the agent, which
        // believes what a search tells it, unless each of them is told.
        let project = Project::new("cut-short");
        for name in ["a", "b", "c", "d", "e", "f"] {
            std::fs::write(project.folder().join(format!("{name}.rs")), SOURCE).unwrap();
        }

        // A limit the walk reaches with files left over, one it reaches with
        // none left, and none at all, which is how an ordinary project walks.
        for (limit, total, cut_short) in [(Some(4), 4, true), (Some(6), 6, false), (None, 6, false)]
        {
            let mut ws = Workspace::rooted(project.real());
            if let Some(limit) = limit {
                ws.pickers.limit_files(limit);
            }
            ws.apply(Command::OpenPicker {
                kind: PickerKind::Files,
            });
            assert_eq!(listed(&mut ws), (total, cut_short), "limit {limit:?}");

            let search = |ws: &mut Workspace, tool: &str, query: &str| {
                ws.run_tool(tool, &serde_json::json!({ "query": query }))
                    .unwrap()
            };
            let found = search(&mut ws, "find_files", "");
            let hits = search(&mut ws, "grep", "fn one");
            let missing = search(&mut ws, "grep", "fn three");
            assert_eq!(
                found.lines().filter(|l| l.ends_with(".rs")).count(),
                total,
                "{found}"
            );
            assert_eq!(
                hits.lines().filter(|l| l.contains(".rs:1:")).count(),
                total,
                "{hits}"
            );
            for said in [&found, &hits, &missing] {
                assert_eq!(
                    said.contains(&format!("only {total} of its files were listed")),
                    cut_short,
                    "limit {limit:?}: {said}"
                );
            }

            // The open buffers are not the folder's files, and their list is
            // never cut short.
            ws.apply(Command::Key {
                key: Key::named("Escape"),
            });
            let buffers = ws.apply(Command::OpenPicker {
                kind: PickerKind::Buffers,
            });
            assert!(
                buffers.iter().any(|e| matches!(
                    e,
                    Event::Picker {
                        cut_short: false,
                        ..
                    }
                )),
                "{buffers:?}"
            );
        }
    }
}


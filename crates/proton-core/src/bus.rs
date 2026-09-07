//! The command/event bus — the seam between the core and whatever draws it.
//!
//! The shell sends [`Command`]s and receives [`Event`]s. Nothing in this module
//! names a widget, a pixel or a DOM node: if a variant here needed to, the
//! design would be wrong and the GPU front end in `docs/ARCHITECTURE.md` would
//! be unreachable.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::action::Action;
use crate::agent::{Activity, Entry};
use crate::key::Key;
use crate::lsp::{Completion, Diagnostic, ServerStatus};
use crate::mode::ModeId;
use crate::picker::{PickerItem, PickerKind};
use crate::review::ChangeView;
use crate::split::Direction;
use crate::tasks::Task;
use crate::trace::{StepView, TraceState};
use crate::tree::TreeRow;
use crate::vim::VimMode;
use proton_plugin::{PluginView, ThemeView};
use proton_syntax::Span;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Command {
    /// A raw keystroke. The core decides what it means.
    Key {
        key: Key,
    },
    /// An already-resolved action, from a menu, a click, or a replayed macro.
    Perform {
        action: Action,
    },
    SetMode {
        mode: ModeId,
    },
    /// Open a file into a buffer, or focus it if it is already open.
    OpenBuffer {
        path: String,
    },
    FocusBuffer {
        index: usize,
    },
    /// Close a buffer — the `×` on a tab, and what `:q` does. Refuses while it
    /// holds unsaved changes unless `force`, which is `:q!`.
    #[serde(rename_all = "camelCase")]
    CloseBuffer {
        index: usize,
        force: bool,
    },
    Save,
    /// What the view can show. The core does not know how tall the window is,
    /// so the front end says, and gets exactly those lines back — which is what
    /// keeps a large file affordable over the bridge.
    #[serde(rename_all = "camelCase")]
    Viewport {
        /// Which split is reporting. Each has its own height and scroll, so
        /// each has to speak for itself.
        view: usize,
        first_line: usize,
        lines: usize,
    },

    /* ── splits ───────────────────────────────────────────────────── */
    /// Split the focused view in two. The new one shows the same buffer,
    /// scrolled the same way — you split because you want two of what you are
    /// looking at.
    Split {
        vertical: bool,
    },
    /// Close one. `None` is the focused one, which is what `:q` and `␣wq`
    /// mean; a click on a splits `×` names the one it is on.
    CloseSplit {
        index: Option<usize>,
    },
    /// Close every split but the focused one — `:only`.
    OnlySplit,
    FocusSplit {
        index: usize,
    },
    /// Move the focus — `⌃w h` and friends.
    StepSplit {
        direction: Direction,
    },
    /// Put the cursor somewhere outright — a click in the text.
    ///
    /// A motion cannot say this. `Perform { Move }` is always relative to where
    /// the cursor already is, and a mouse knows exactly where it wants to be.
    /// Clamped by the core, so the front end may send whatever its hit test
    /// produced without checking it against a document it does not hold.
    PlaceCursor {
        line: usize,
        column: usize,
    },

    /// Drag the far end of a selection there — a mouse drag.
    ///
    /// Starts a charwise visual selection if one is not already running, which
    /// is what makes a drag leave you somewhere `y` and `d` mean what you
    /// expect rather than in a selection the modal engine cannot see.
    SelectTo {
        line: usize,
        column: usize,
    },

    /// Open a folder in the tree, or close it if it was open.
    ToggleTreeNode {
        path: String,
    },
    /// Open every folder above the file being edited, and say where it is.
    RevealInTree,

    /// Open a Telescope picker. The picker owns the keyboard until it closes.
    OpenPicker {
        kind: PickerKind,
    },
    /// Put a whole query into the open picker at once.
    ///
    /// For a search something else already knows the whole of — a value
    /// clicked in a dataframe, a term the agent picked. Typing it a letter at
    /// a time would re-query on every one.
    PickerQuery {
        text: String,
    },
    /// Select a row directly, for a click. The keyboard moves by delta; a
    /// mouse knows exactly which row it is on.
    SelectPick {
        index: usize,
    },
    /// Select a completion row directly, for a click.
    SelectCompletion {
        index: usize,
    },
    /// Put the selected completion in the buffer.
    AcceptCompletion,
    CloseCompletion,
    /// Ask for completions where the cursor is.
    Complete,
    /// Jump to where the symbol under the cursor is defined — `gd`.
    GotoDefinition,
    /// Ask what the symbol under the cursor is — `K`.
    Hover,
    /// Dismiss the hover panel.
    CloseHover,
    /// Every use of the symbol under the cursor, in a picker — `gr`.
    FindReferences,
    /// Reformat the active buffer without saving it — `:fmt`. Saving formats
    /// on its own; this is for reformatting something you are not done with.
    Format,
    /// Open the command line holding `rename <the name it has now>` — `␣lr`.
    /// The rename itself is the ex command, so it can also be typed.
    PromptRename,
    /// Rename the symbol under the cursor, everywhere it is used.
    Rename {
        to: String,
    },
    /// What the server would offer for the cursor's line — `␣la`.
    CodeActions,

    /* ── the agent's staged changes ───────────────────────────────── */
    /// Put one staged hunk in the buffer. Still unsaved: accepting is the
    /// same as having typed it.
    #[serde(rename_all = "camelCase")]
    ApplyHunk {
        path: String,
        hunk: usize,
    },
    /// Refuse one, so it stops being proposed.
    #[serde(rename_all = "camelCase")]
    RejectHunk {
        path: String,
        hunk: usize,
    },
    /// Take every hunk of one file, or of everything when no path is given.
    ApplyStaged {
        path: Option<String>,
    },
    /// Refuse the lot.
    RejectStaged {
        path: Option<String>,
    },
    /// Drain whatever the language servers have said.
    ///
    /// The core has no clock and no threads, so it cannot notice a reply on
    /// its own; something outside has to ask. That is the whole cost of
    /// keeping `apply` total, and it is a good trade.
    Poll,
    /* ── the visualizer ───────────────────────────────────────────── */
    /// Trace the active buffer — `␣ct`.
    Trace,
    /// Move to a step. The pane scrubs, so this is absolute rather than a
    /// delta: dragging a slider should not depend on how many events arrived.
    TraceSeek {
        step: usize,
    },
    /// Step by a delta, for the arrow keys.
    TraceStep {
        by: i64,
    },

    /* ── the agent ────────────────────────────────────────────────── */
    /// Put a question to the agent.
    Ask {
        text: String,
    },
    /// Stop the turn in flight. What has arrived stays on screen; nothing is
    /// committed to the conversation.
    CancelTurn,
    /// Start a new conversation. The one open is already on disk.
    NewChat,
    /// Open a saved conversation, by path relative to the project.
    OpenChat {
        path: String,
    },
    /// Delete one. There is no undo: it is a file, and the shell asks first.
    DeleteChat {
        path: String,
    },
    /// Give one a name of its own, rather than the first line of the question.
    RenameChat {
        path: String,
        title: String,
    },
    /// Take one task off the queue. A running one stays: removing its row
    /// would not stop the turn.
    DropTask {
        id: usize,
    },
    /// Clear everything that has finished, leaving what is still to come.
    ClearTasks,
    /* ── workspaces ───────────────────────────────────────────────── */
    /// Open a folder as a workspace, or focus it if it is already open.
    ///
    /// A workspace is one project: its own buffers, its own file index, its
    /// own language servers, its own agent conversation. Several are open at
    /// once and one is active.
    OpenWorkspace {
        path: String,
    },
    FocusWorkspace {
        index: usize,
    },
    /// Close one. Refuses while it holds unsaved buffers, because closing it
    /// discards them and nothing else would say so.
    CloseWorkspace {
        index: usize,
    },
    /// Step to the next or previous workspace, wrapping.
    CycleWorkspace {
        by: i64,
    },
    /// Open the workspace picker — the ones open, the ones opened before, and
    /// any folder on the disk.
    OpenWorkspacePicker,

    /* ── debugging ────────────────────────────────────────────────── */
    /// Put a breakpoint on a line, or take one off. Works with no debugger
    /// running — that is the ordinary way to set them.
    #[serde(rename_all = "camelCase")]
    ToggleBreakpoint {
        /// The line the cursor is on when absent.
        line: Option<usize>,
    },
    /// Ask what the repository has. Answered with `Event::Repo`.
    RefreshRepo,

    /* ── notebooks ────────────────────────────────────────────────── */
    /// Run one cell. Answers when it has been sent, not when it is done —
    /// the output arrives on the heartbeat like everything else.
    RunCell {
        index: usize,
    },
    /// Replace a cell's code.
    #[serde(rename_all = "camelCase")]
    SetCell {
        index: usize,
        source: String,
    },
    /// Add one at the end. `code` false makes it markdown.
    AddCell {
        code: bool,
    },
    RemoveCell {
        index: usize,
    },
    /// Put the cursor on a cell — a click. `j`/`k` move it by a delta; a
    /// mouse knows exactly which cell it means.
    SelectCell {
        index: usize,
    },
    /// Run every code cell, top to bottom.
    RunAll,

    /* ── themes ────────────────────────────────────────────── */
    /// Point Design mode at a stylesheet. Answered with `Event::Theme`.
    OpenTheme {
        path: String,
    },
    /// Change one token and write the file.
    ///
    /// Scope as well as name, because a theme states the same token once per
    /// palette and a name alone would edit whichever came first.
    SetToken {
        name: String,
        scope: String,
        value: String,
    },
    /// Put the inspector on a token — a click.
    SelectToken {
        index: usize,
    },

    /* ── settings ──────────────────────────────────────────── */
    /// Change one setting, by dotted key. `null` clears it back to the
    /// default rather than storing a null.
    SetSetting {
        key: String,
        // A setting is a string, a number or a flag depending on which one it
        // is, so the binding says so rather than inventing a union that would
        // have to be widened for every setting added.
        #[ts(type = "unknown")]
        value: serde_json::Value,
    },
    /// Read the library's roots again.
    ///
    /// Notes get written while the editor is open, and the walk happens once
    /// at startup. This is the button that says "I just wrote that down".
    RefreshLibrary,
    /// Open what a citation names.
    ///
    /// The citation as it was written — `Attention.md § Sparse patterns
    /// L42-58` — because that is the string the front end has, and turning it
    /// back into a file needs the library's roots, which only the core knows.
    OpenCitation {
        cite: String,
    },
    /// Write the notebook back.
    SaveNotebook,
    /// Open a place in another mode: the cross-mode jump.
    ///
    /// One command rather than "switch, then open, then move", because those
    /// three separately are three redraws and two chances to end up somewhere
    /// nobody asked for — in the new mode, at the old file.
    #[serde(rename_all = "camelCase")]
    JumpTo {
        mode: ModeId,
        path: String,
        /// Zero-based, as every line number crossing this bus is.
        line: usize,
        #[serde(default)]
        column: usize,
    },
    /// Open a terminal in the project, or focus the one there is.
    OpenTerminal,
    /// Type at it. The front end sends what the keyboard produced; the shell
    /// decides what it means.
    TerminalInput {
        text: String,
    },
    /// How big the panel is, in cells. Programs redraw from this.
    #[serde(rename_all = "camelCase")]
    TerminalSize {
        rows: u16,
        cols: u16,
    },
    /// Look back through what has scrolled off the top.
    ///
    /// Rows, signed: positive goes further into the history, negative comes
    /// forward. Clamped at both ends by however much history there is.
    TerminalScroll {
        lines: i32,
    },
    /// Back to the live screen, however far back the view is.
    ///
    /// Its own command rather than a very large `TerminalScroll`, because
    /// "return to the bottom" is what is meant and a number big enough to
    /// mean it is a number that has to be guessed.
    TerminalToBottom,
    CloseTerminal,

    /// Launch what `.proton/debug.json` names.
    DebugStart,
    DebugStop,
    DebugResume,
    DebugStepOver,
    DebugStepInto,
    DebugStepOut,

    /// Ask for the current state — used once on connect so the shell can render
    /// without guessing.
    Sync,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    /// The modal state changed. Drives the statusline.
    VimMode {
        mode: VimMode,
        label: String,
    },
    /// A pending command, for `showcmd`. Empty string means "cleared".
    Pending {
        keys: String,
    },
    /// The leader was pressed, or a subsequent key narrowed it.
    /// `None` closes which-key.
    Leader {
        prefix: Option<String>,
    },
    /// The command line opened, changed, or (with `None`) closed.
    Cmdline {
        text: Option<String>,
        /// Which line it is: `:` for an ex command, `/` or `?` for a search.
        /// They share the prompt, and the character is the only thing telling
        /// a reader which one they are typing into.
        prompt: String,
    },
    /// The command line was submitted. A separate event from `Cmdline` on
    /// purpose: the two carry the same text and mean entirely different
    /// things, and a consumer that cannot tell them apart runs every
    /// keystroke as a command.
    CmdlineRun {
        text: String,
    },
    /// An action resolved and the active mode accepted it.
    Action {
        action: Action,
        description: String,
    },
    /// The active mode has no meaning for this action. Not an error — a
    /// reportable fact, so the statusline can show it.
    Unsupported {
        description: String,
        mode: ModeId,
    },
    /// A literal character was typed in insert mode. M1 turns these into rope
    /// edits; M0 only proves the round trip.
    Input {
        text: String,
    },
    ModeChanged {
        mode: ModeId,
    },

    /* ── buffers ──────────────────────────────────────────────────── */
    /// The visible window of the active buffer. Sent whenever the text, the
    /// viewport or the active buffer changes — never the whole file.
    #[serde(rename_all = "camelCase")]
    Viewport {
        /// Which split this is the content of.
        view: usize,
        first_line: usize,
        lines: Vec<String>,
        total_lines: usize,
        version: u64,
        /// Coloured runs, one list per line in `lines`.
        ///
        /// Empty when the file has no grammar, which means "draw it plain" —
        /// not "these lines happen to have no colours". Otherwise it is
        /// exactly as long as `lines`, so the two can be walked together.
        highlights: Vec<Vec<Span>>,
        /// Where the current search matches, one list per line in `lines`.
        ///
        /// A separate layer from `highlights` rather than another scope: a
        /// match is a background and a scope is a colour, and a match inside a
        /// string has to stay string-coloured. Empty when nothing is being
        /// searched for.
        ///
        /// Scoped to the window like everything else here — a file with ten
        /// thousand hits sends only the ones you can see.
        matches: Vec<Vec<Hit>>,
        /// Git marks, one per line in `lines`. Empty outside a repository, or
        /// for a file git has never seen — both of which mean the same thing
        /// to a gutter: nothing to compare against.
        git: Vec<Option<GitMark>>,
        /// Breakpoints in this window, and whether the program is stopped on
        /// one. A list rather than one-per-line because there are usually
        /// none, and a screenful of `null` to say so is waste.
        stops: Vec<Stop>,
    },
    /// Where the cursor is now. Split from `Viewport` because a motion is the
    /// common case and must not cost a re-render of the text.
    #[serde(rename_all = "camelCase")]
    Cursor {
        /// Which split the cursor belongs to. Every view has one; only the
        /// focused one can move.
        view: usize,
        line: usize,
        column: usize,
        offset: usize,
        /// Anchor line/column when there is a selection.
        selection: Option<Selection>,
    },
    /// The open buffers, for the tab bar.
    #[serde(rename_all = "camelCase")]
    Buffers {
        names: Vec<BufferTab>,
        active: usize,
    },
    /// The file tree, as the rows that are visible.
    ///
    /// Only what is expanded — a closed folder contributes one row whatever is
    /// inside it, which is what keeps this affordable on a large project
    /// without a virtualised list on the other side.
    Tree {
        rows: Vec<TreeRow>,
    },
    /// The splits: how many, which buffer each shows, and which has the focus.
    ///
    /// Sent when the set or the focus changes, not on every keystroke — the
    /// content of each one arrives as its own `Viewport`.
    #[serde(rename_all = "camelCase")]
    Splits {
        views: Vec<SplitView>,
        focused: usize,
        /// Side by side rather than stacked.
        vertical: bool,
    },
    /// The terminal, as rows of styled runs.
    ///
    /// Runs rather than cells: a row is mostly one colour, and a cell per
    /// character would put eighty objects on the bridge for a line of prose.
    #[serde(rename_all = "camelCase")]
    Terminal {
        rows: Vec<Vec<TermRun>>,
        cursor: (u16, u16),
        cursor_visible: bool,
        /// The shell is still there. A dead terminal keeps its last screen:
        /// what a command printed before it exited is the useful part.
        running: bool,
        /// How far back through the history this is, in rows; `0` is live.
        /// Sent because a terminal scrolled back looks exactly like one whose
        /// shell has stopped answering, and the panel should be able to say
        /// which it is.
        scrollback: usize,
    },
    /// The debugger: where it is, and what it can see from there.
    ///
    /// Sent whole rather than as deltas, for the reason the agent transcript
    /// is: a stack is a few dozen frames, the panel re-renders from it, and a
    /// delta protocol for something this small is a bug waiting to be written.
    #[serde(rename_all = "camelCase")]
    Debug {
        phase: DebugPhase,
        frames: Vec<DebugFrame>,
        variables: Vec<DebugVariable>,
        output: Vec<String>,
    },
    /// The open workspaces, for the switcher above the buffer bar.
    ///
    /// One level up from `Buffers`: those are files inside a project, these
    /// are the projects. Sent whenever the set or the active one changes —
    /// which is rare, so it goes whole.
    #[serde(rename_all = "camelCase")]
    Workspaces {
        items: Vec<WorkspaceTab>,
        active: usize,
    },
    /// The picker opened, or its query or selection changed.
    #[serde(rename_all = "camelCase")]
    Picker {
        kind: PickerKind,
        title: String,
        query: String,
        items: Vec<PickerItem>,
        selected: usize,
        /// How many candidates the picker is choosing from, before matching.
        total: usize,
        elapsed_ms: f64,
        /// The tree is still being walked. An empty list under this flag means
        /// "not yet", not "no matches" — and those look identical without it.
        scanning: bool,
    },
    /// The picker closed. Distinct from an empty result: one means "nothing
    /// matched", the other means "the keyboard is yours again".
    PickerClosed,
    /// Something the user should see: a failed open, a save, a macro register.
    Notice {
        text: String,
        level: NoticeLevel,
    },
    /* ── language servers ─────────────────────────────────────────── */
    /// Diagnostics for the active buffer, in its own coordinates. Sent when
    /// they change and when the active buffer does — a file with no server
    /// gets an empty list, which is how the gutter knows to clear.
    #[serde(rename_all = "camelCase")]
    Diagnostics {
        path: Option<String>,
        items: Vec<Diagnostic>,
        errors: usize,
        warnings: usize,
    },
    /// The completion popup opened, or its list or selection changed.
    Completion {
        items: Vec<Completion>,
        selected: usize,
    },
    /// The popup closed. Distinct from an empty list for the same reason
    /// `PickerClosed` is: one means "nothing matched", the other means "the
    /// keyboard is yours again".
    CompletionClosed,
    /// What the server said the thing under the cursor is.
    ///
    /// Anchored to a place in the buffer rather than to the cursor, because
    /// the panel should stay pointing at the word it describes even once the
    /// cursor has moved off it.
    Hover {
        text: String,
        line: usize,
        column: usize,
    },
    /// The hover panel closed — or was never opened, because the server had
    /// nothing to say about what is under the cursor.
    HoverClosed,
    /// What each language server is doing, for the statusline.
    Servers {
        states: Vec<ServerState>,
    },
    /// The queue: what is running, what is waiting, and what is ready.
    Tasks {
        tasks: Vec<Task>,
    },
    /// What is installed, and the palettes it brought.
    ///
    /// Sent once at startup and never again: plugins are loaded when the
    /// session is built and there is no way to install one without
    /// restarting yet. Themes come with it rather than as their own event
    /// because the two are read together — a palette you cannot attribute is
    /// one you cannot uninstall.
    Plugins {
        items: Vec<PluginView>,
        themes: Vec<ThemeView>,
    },
    /// What the agent has proposed and nobody has accepted yet.
    ///
    /// Sent whole and recomputed every time, because the hunks *are* the live
    /// diff between each buffer and what was proposed — there is no stored
    /// list of them that a delta could describe.
    Review {
        files: Vec<ChangeView>,
    },
    /// Open something to look at rather than to edit.
    ///
    /// Carries the path rather than the bytes: a PDF is megabytes, the shell
    /// can hand the page an ArrayBuffer directly, and putting a file that size
    /// through the event channel would make every other event wait behind it.
    Document {
        /// Absolute, because the shell reads it and the shell has no idea what
        /// the workspace root is.
        path: String,
        /// What to put on the tab.
        name: String,
        kind: ViewerKind,
    },
    /// The notebook that is open, and what its cells have produced.
    ///
    /// Sent whole, like the agent transcript and for the same reason: a
    /// notebook is tens of cells, the panel redraws from it, and a delta
    /// protocol for something that size is a synchronisation bug waiting to
    /// be written.
    #[serde(rename_all = "camelCase")]
    Notebook {
        /// Empty when no notebook is open, which is how the panel knows to
        /// say so rather than showing the last one.
        path: String,
        cells: Vec<crate::data::CellView>,
        /// Why there is no kernel, when there is a reason worth showing.
        problem: String,
    },
    /// The saved conversations, most recently changed first.
    #[serde(rename_all = "camelCase")]
    Chats {
        conversations: Vec<crate::chats::Summary>,
        /// Which one is open, as a project-relative path. Empty for a
        /// conversation that has not been saved yet — one with no question in
        /// it has nothing worth a file.
        current: String,
    },
    /// Everything in `.proton/settings.json`, as one object.
    ///
    /// Sent whole rather than key by key: it is a few dozen values, the panel
    /// redraws from it, and a per-key protocol for something this small is a
    /// synchronisation bug waiting to be written — the same call the agent
    /// transcript makes.
    #[serde(rename_all = "camelCase")]
    Settings {
        #[ts(type = "Record<string, unknown>")]
        values: serde_json::Value,
        /// Where they are stored, for the panel to show and for a `:e` on it.
        path: String,
    },
    /// What the library has read, and from where.
    ///
    /// Counts rather than contents: the panel says how much is there and
    /// whether it is still reading, and the passages themselves only ever
    /// leave through a search.
    #[serde(rename_all = "camelCase")]
    Library {
        /// The roots, resolved — `~/OnyxVault` as the directory it means.
        roots: Vec<String>,
        /// Roots that were named and are not there. A typo in a settings file
        /// otherwise reads exactly like a vault with nothing in it.
        missing: Vec<String>,
        documents: usize,
        passages: usize,
        /// True while a walk is in flight, so "reading" and "empty" are
        /// different things on screen.
        indexing: bool,
        /// How long the last walk took. Zero if none has finished.
        elapsed_ms: u64,
    },
    /// The theme Design mode is editing.
    #[serde(rename_all = "camelCase")]
    Theme {
        /// Empty when none is open, which is how the panel knows to say so.
        path: String,
        tokens: Vec<proton_design::Token>,
        /// Which one the inspector is on.
        selected: usize,
        /// Why there is no theme, when there is a reason worth showing.
        problem: String,
    },
    /// The active mode's four slots, verb table and document type.
    ///
    /// Sent when the mode changes and on connect, because it is what tells the
    /// front end which panels belong on screen at all — a mode switch that
    /// only changed a highlighted rail button would be a theme, not a mode.
    Layout {
        layout: crate::contract::Layout,
    },
    /// What the repository has, for the git panel.
    ///
    /// Recomputed rather than kept, the way the gutter and the file tree are:
    /// git's answer changes because of things that never went through this
    /// core — a rebase in a terminal, a branch switch, another editor — so a
    /// remembered list is a list that is wrong exactly when it matters.
    #[serde(rename_all = "camelCase")]
    Repo {
        /// `None` outside a repository, on a detached `HEAD`, or before the
        /// first commit. All three mean the same thing to the panel.
        branch: Option<String>,
        files: Vec<RepoFile>,
    },
    /// The whole agent transcript, and what it is doing.
    ///
    /// Sent whole rather than as deltas: a turn is at most a few dozen
    /// entries, the front end re-renders one panel from it, and a delta
    /// protocol for something this small is a synchronisation bug waiting to
    /// be written.
    #[serde(rename_all = "camelCase")]
    Agent {
        transcript: Vec<Entry>,
        activity: Activity,
        input_tokens: u64,
        output_tokens: u64,
        /// Served from cache. Surfaced because a silently invalidated prefix
        /// is a cost regression rather than an error, and nothing else shows it.
        cached_tokens: u64,
        /// Which service answered, and on which model. Shown because the two
        /// give different answers to the same question, and a transcript that
        /// does not say which one produced it is a transcript you cannot
        /// reason about a week later.
        provider: String,
        model: String,
        /// Where the credential was found, or what to set if there is none.
        /// Never the credential: this is drawn on screen and goes in logs.
        credential: String,
    },
    /// The Visualizer pane.
    ///
    /// Carries **one step**, not the trace. A sixty-step run is 178 KB of
    /// JSON and a long one is megabytes, none of which is on screen at once —
    /// the same discipline the viewport uses for text.
    #[serde(rename_all = "camelCase")]
    Trace {
        state: TraceState,
        /// What was traced, for the header.
        subject: String,
        /// Step count, an error, or what it is doing.
        message: String,
        step: usize,
        steps: usize,
        view: Option<StepView>,
    },
    /// Full state, in reply to [`Command::Sync`].
    #[serde(rename_all = "camelCase")]
    State {
        mode: ModeId,
        vim_mode: VimMode,
        vim_label: String,
    },
}

/// A selection's far end, when there is one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub line: usize,
    pub column: usize,
    pub offset: usize,
}

/// One split, for the layout. Its *content* arrives as a `Viewport`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct SplitView {
    /// Which buffer it shows, as an index into the buffer bar.
    pub buffer: usize,
    pub name: String,
    /// Which cell it is on, for the modes whose document has cells. Panes
    /// onto one notebook differ only in this and in where they are scrolled,
    /// so without it every pane would draw the same selection.
    pub cell: usize,
}

/// One run of terminal characters sharing a look.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct TermRun {
    pub text: String,
    /// An ANSI palette index, which the theme colours. `None` is the default
    /// foreground, which is whatever the pane is already using.
    pub fg: Option<u8>,
    pub bg: Option<u8>,
    /// A true colour, when the program asked for one the palette has no name
    /// for. As `#rrggbb`, ready to go straight into a style.
    pub rgb: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

/// A breakpoint on a visible line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct Stop {
    /// Zero-based, like every other line number crossing this bus.
    pub line: usize,
    /// The adapter confirmed it could place it. An unverified one is a
    /// breakpoint the debugger will never hit, and saying so is the
    /// difference between a bug and a misunderstanding.
    pub verified: bool,
    /// The program is stopped right here.
    pub here: bool,
}

/// Where the debugger is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum DebugPhase {
    /// No adapter. Breakpoints still work — they are waiting for one.
    Idle,
    Running,
    Stopped,
}

/// One frame on the stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DebugFrame {
    pub name: String,
    /// Relative to the project when it is inside it. Absent for a frame in a
    /// library, which is ordinary rather than an error.
    pub path: Option<String>,
    pub line: usize,
}

/// One variable in scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DebugVariable {
    pub name: String,
    pub value: String,
    /// It has children to expand — a struct, an array.
    pub expandable: bool,
}

/// A file that is shown rather than edited.
///
/// Deliberately a small closed set rather than "anything that is not text".
/// A format only belongs here once something can actually draw it; treating
/// every binary as a document would replace one honest error — "this is not
/// text" — with a viewer that has nothing to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum ViewerKind {
    Pdf,
}

impl ViewerKind {
    /// What this path is, if it is one of these at all.
    pub fn of(path: &std::path::Path) -> Option<ViewerKind> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "pdf" => Some(ViewerKind::Pdf),
            _ => None,
        }
    }
}

/// One changed path, for the git panel.
///
/// The path is relative and spelled with forward slashes, which is the same
/// spelling `openBuffer` and the file index use — so a row in this list can be
/// clicked open without anything having to translate it first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct RepoFile {
    pub path: String,
    /// `M`, `A`, `D` or `?`, as git spells it. One letter because the panel is
    /// narrow and because it is the notation anybody reading a git list
    /// already knows.
    pub state: String,
}

/// What git says happened to one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub enum GitMark {
    Added,
    Modified,
    /// Lines were deleted here. There is no line to mark, so it attaches to
    /// the one that took their place.
    Removed,
}

/// A search hit on one line, in character columns.
///
/// Characters rather than bytes, because that is what the front end indexes by
/// and what the rest of the viewport already speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct Hit {
    pub start: u32,
    pub end: u32,
}

/// One entry in the buffer bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct BufferTab {
    pub name: String,
    pub path: Option<String>,
    pub modified: bool,
}

/// One entry in the workspace switcher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTab {
    /// The folder's name, or enough of its parent to tell two apart. Two
    /// checkouts both called `src` are the common case, not the exotic one.
    pub name: String,
    /// The full path, for the tooltip and for reopening it later.
    pub path: String,
    /// Something in it is unsaved.
    pub modified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum NoticeLevel {
    Info,
    Warn,
    Error,
}

/// One language server's condition, for the statusline. `note` carries
/// whatever the server last said it was busy with — "indexing", "loading
/// sysroot" — because half a minute of silence at startup looks identical to
/// a broken editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct ServerState {
    pub language: String,
    pub status: ServerStatus,
    pub note: Option<String>,
}

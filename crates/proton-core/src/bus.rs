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
use crate::split::Direction;
use crate::trace::{StepView, TraceState};
use crate::tree::TreeRow;
use crate::vim::VimMode;
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
    /// What each language server is doing, for the statusline.
    Servers {
        states: Vec<ServerState>,
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

//! The command/event bus — the seam between the core and whatever draws it.
//!
//! The shell sends [`Command`]s and receives [`Event`]s. Nothing in this module
//! names a widget, a pixel or a DOM node: if a variant here needed to, the
//! design would be wrong and the GPU front end in `docs/ARCHITECTURE.md` would
//! be unreachable.

use std::collections::BTreeMap;

use proton_claude::folders::FolderView as ChatFolder;
use proton_claude::{Artifact, Capabilities as ClaudeCapabilities, Engine as ChatEngine, Item as ClaudeItem, SessionSummary as ClaudeSession, Status as ClaudeStatus};
use proton_research::supervisor::{RoundStatus, TaskRow as ResearchTask};
use proton_research::view::{HypothesisDetail, HypothesisRow, SeatRow as ResearchSeatRow, Summary as ResearchSummary};
use proton_research::model::{HypothesisStatus, Seat as ResearchSeat};
use proton_research::Discipline as ResearchDiscipline;
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

    /* ── Claude Code, in the Chat tab ──────────────────────────────── */
    /// A new conversation on screen, with the engine named or the one new
    /// conversations start with. Nothing starts until something is said.
    ClaudeNew {
        #[serde(default)]
        #[ts(optional)]
        engine: Option<ChatEngine>,
    },
    /// The engine new conversations start with — Claude Code, Codex, Gemini
    /// or Antigravity — and a new conversation with it on screen.
    ClaudeEngine {
        engine: ChatEngine,
    },
    /// Show one of the project's sessions, reading it back if it is not open.
    ClaudeOpen {
        session: String,
    },
    /// Say something in the conversation on screen.
    ClaudeSend {
        text: String,
        images: Vec<ClaudeImage>,
    },
    /// Stop the turn in progress, as Escape does in a terminal.
    ClaudeStop,
    /// Answer what a conversation is waiting on.
    ClaudeAnswer {
        request: String,
        answer: ClaudeAnswer,
    },
    /// The permission mode — `default`, `acceptEdits`, `plan`, `auto` — for
    /// the conversation on screen and those started after it.
    ClaudeMode {
        mode: String,
    },
    ClaudeModel {
        model: String,
    },
    ClaudeEffort {
        effort: Option<String>,
    },
    /// Read the project's sessions again.
    ClaudeSessions,
    /// Delete a conversation: its transcript goes to the Trash.
    ClaudeDelete {
        session: String,
    },
    /// A new folder for conversations — named, or "New folder" until it is —
    /// with these put in it.
    ChatFolderNew {
        name: String,
        sessions: Vec<String>,
    },
    ChatFolderRename {
        folder: String,
        name: String,
    },
    /// Delete a folder. Its conversations go back to the list.
    ChatFolderDelete {
        folder: String,
    },
    ChatFolderCollapse {
        folder: String,
        collapsed: bool,
    },
    /// Put a conversation in a folder, taking it out of any other — or, with
    /// no folder, take it out of the one it is in.
    ChatFolderMove {
        session: String,
        folder: Option<String>,
    },
    /// Forget an artifact, and the copies of it kept here.
    ForgetArtifact {
        id: String,
    },
    /// Bring in the artifacts published from this machine before Proton was
    /// keeping them: from the desktop app's records and the sessions' files.
    ImportArtifacts,
    /// Open a web address in the browser.
    OpenLink {
        url: String,
    },
    /* ── research ─────────────────────────────────────────────────── */
    /// Start an investigation, in a folder of its own under the project's
    /// `research/`.
    ResearchNew {
        title: String,
        goal: String,
        discipline: ResearchDiscipline,
    },
    /// Show one of the project's investigations.
    ResearchOpen {
        slug: String,
    },
    /// Show one hypothesis of the open investigation, or none.
    ResearchShow {
        hypothesis: Option<String>,
    },
    /// Run a round of the open investigation: its agents' calls, on their
    /// own engines, until the overview is written.
    ResearchStart,
    /// Start nothing new in the open investigation's round; what is
    /// running finishes.
    ResearchPause,
    ResearchResume,
    /// Stop the open investigation's round, and every call it has out.
    ResearchStop,
    /// Tell the open investigation's team something. It goes into
    /// `feedback.md`, which every task after it is shown.
    ResearchFeedback {
        text: String,
    },
    /// A hypothesis of the scientist's own: reviewed, and played in the
    /// tournament, with the agents' ones.
    ResearchAddHypothesis {
        title: String,
        statement: String,
    },
    /// Pin a hypothesis of the open investigation, retire it, or bring it back.
    ResearchSetStatus {
        hypothesis: String,
        status: HypothesisStatus,
    },
    /// Seat an engine at the open investigation's table, or stand it down.
    ResearchSeat {
        seat: ResearchSeat,
        enabled: bool,
    },
    /// Show what one of the open investigation's tasks said; `None` for none.
    ResearchTask {
        task: Option<String>,
    },
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
    /// Put a component on the artboard. Answered with `Event::Artboard`.
    ShowComponent {
        path: String,
    },
    /// Pick a layer — a click in the layer tree.
    SelectLayer {
        index: usize,
    },
    /// Rename a layer's text, from the inspector.
    SetLayerText {
        index: usize,
        text: String,
    },
    /// Set or clear one of a layer's attributes. An empty value removes it.
    SetLayerAttribute {
        index: usize,
        name: String,
        value: String,
    },
    /* ── scenes ────────────────────────────────────────────── */
    /// Point Modeling at a scene. Answered with `Event::Scene`.
    OpenScene {
        path: String,
    },
    /// Pick a node — a click in the outliner.
    SelectNode {
        index: usize,
    },
    /// Rename the node at `index`.
    RenameNode {
        index: usize,
        name: String,
    },
    /// Move, turn or resize the node at `index`.
    SetNodeTransform {
        index: usize,
        part: proton_scene::Part,
        to: [f64; 3],
    },
    /// Point a node at its geometry. An empty `path` clears it.
    SetNodeMesh {
        index: usize,
        path: String,
    },
    /// Put a modifier on the end of a node's stack.
    SetNodeModifier {
        index: usize,
        kind: String,
    },
    /// Take one off.
    RemoveNodeModifier {
        index: usize,
        row: usize,
    },
    /// Move one up or down the stack, which is what makes it a stack: mirror
    /// then array is two rows of pairs, array then mirror is one row
    /// reflected into two.
    MoveNodeModifier {
        index: usize,
        row: usize,
        to: usize,
    },
    /// Change one setting on one modifier: `off`, `axis`, `weld`, `count`,
    /// `levels`, or one axis of `offset`.
    #[serde(rename_all = "camelCase")]
    SetModifierField {
        index: usize,
        row: usize,
        field: String,
        value: String,
    },
    /// Ask for one mesh's geometry, by the path the scene writes.
    ///
    /// Asked for rather than pushed with the scene: geometry is the one thing
    /// here large enough to be worth sending once and keeping. The front end
    /// knows what it already holds; the core does not, and would otherwise
    /// re-send megabytes on every click in the outliner.
    LoadMesh {
        path: String,
    },
    /// Put a node under another. An empty `parent` moves it to the root.
    ReparentNode {
        index: usize,
        parent: String,
    },
    /// Add a node under the selection.
    AddNode {
        kind: String,
        name: String,
    },
    /// Remove the node at `index`, and everything under it.
    RemoveNode {
        index: usize,
    },

    /* ── Godot scenes ─────────────────────────── */
    /// Point Game mode at a `.tscn`. Answered with `Event::Level`.
    OpenLevel {
        path: String,
    },
    /// Pick a node — a click in the hierarchy.
    SelectGameNode {
        index: usize,
    },
    /// Change one property on the node at `index`.
    #[serde(rename_all = "camelCase")]
    SetGameProperty {
        index: usize,
        name: String,
        value: String,
    },

    /* ── cuts and captions ────────────────────── */
    /// Point Video mode at an `.edl`. Answered with `Event::Cut`.
    OpenTimeline {
        path: String,
    },
    /// Pick an event — a click on a clip in the timeline.
    SelectClip {
        index: usize,
    },
    /// Change one field of the event at `index`: a reel or one of its four
    /// timecodes.
    #[serde(rename_all = "camelCase")]
    SetClipField {
        index: usize,
        field: String,
        value: String,
    },
    /// Change one field of the caption at `index`: its words or its times.
    #[serde(rename_all = "camelCase")]
    SetCaptionField {
        index: usize,
        field: String,
        value: String,
    },
    /// Read the list at a different rate.
    ///
    /// An EDL does not record the rate its timecodes are counted at, so the
    /// one on screen is an assumption and this is how a person corrects it.
    /// Nothing is written: the file does not change, only what it means.
    SetTimelineRate {
        fps: u32,
    },

    /* ── shader graphs ────────────────────────── */
    /// Change one setting on one node of a material's graph.
    SetMaterialParam {
        material: String,
        node: String,
        param: String,
        value: proton_scene::ParamValue,
    },
    /// Move a graph node's box on the canvas.
    MoveGraphNode {
        material: String,
        node: String,
        at: [f64; 2],
    },
    /// Wire an output to an input. Both are written `node:port`.
    ConnectPorts {
        material: String,
        from: String,
        to: String,
    },
    /// Move a wire from one input to another. One gesture, one line.
    #[serde(rename_all = "camelCase")]
    RewirePort {
        material: String,
        from_input: String,
        to_input: String,
    },
    /// Remove whatever feeds an input.
    DisconnectPort {
        material: String,
        to: String,
    },

    /// Open the line that wrote a layer, in Codex.
    ///
    /// The binding pointing the other way from the artboard: a layer is a
    /// place in a file, and the point of knowing that is being able to go
    /// there.
    RevealLayer {
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
    /// Put a question to the council: every model with a subscription answers
    /// it, reads the others, and one writes the brief.
    Convene {
        question: String,
    },
    /// Show a debate that already happened, by its directory name.
    OpenDebate {
        name: String,
    },
    /// Narrow the library to these folders, or widen it again with an empty
    /// list. Named as `library_sources` lists them.
    SetLibraryScope {
        sources: Vec<String>,
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
    /// Paste into it — not the same as typing it: line breaks are sent as
    /// Return, and a shell that asked for bracketed paste gets the text
    /// bracketed, so a pasted command waits for Enter rather than running.
    TerminalPaste {
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
        /// The language its code cells are in, to colour them by: what the
        /// notebook says, else Python.
        language: String,
    },
    /// The project's conversations, newest first — Claude Code's sessions
    /// and the other engines' — then the desktop app's from other folders;
    /// which one is on screen; which are working or waiting on you; and the
    /// engine a new one starts with.
    ClaudeSessions {
        sessions: Vec<ClaudeSession>,
        /// The folders conversations are grouped into, each with what it
        /// holds; what is in a folder is not in `sessions` as well.
        folders: Vec<ChatFolder>,
        active: String,
        busy: Vec<String>,
        waiting: Vec<String>,
        engine: ChatEngine,
    },
    /// Rows of the conversation on screen: the ones that changed, or with
    /// `reset`, all of them. `earlier` says a long session's start was left
    /// out.
    ClaudeItems {
        session: String,
        items: Vec<ClaudeItem>,
        reset: bool,
        earlier: bool,
    },
    ClaudeStatus {
        status: ClaudeStatus,
    },
    /// What one engine offers — its models, modes and commands, whose
    /// account it is, or why it cannot be used here. One for each engine.
    ClaudeCapabilities {
        capabilities: ClaudeCapabilities,
    },
    /// Every artifact kept here, newest first, and whether an import is
    /// reading the sessions for more.
    Artifacts {
        artifacts: Vec<Artifact>,
        importing: bool,
    },
    /// The project's investigations, newest first, and which one is open.
    #[serde(rename_all = "camelCase")]
    ResearchList {
        investigations: Vec<ResearchSummary>,
        open: Option<String>,
    },
    /// The open investigation, as its board draws it: the brief, the plan,
    /// the overview, the logs, and the hypotheses ranked.
    #[serde(rename_all = "camelCase")]
    ResearchBoard {
        slug: String,
        title: String,
        discipline: ResearchDiscipline,
        brief: String,
        plan: String,
        overview: String,
        verdicts: String,
        sandbox: String,
        /// What the scientist has told the team.
        feedback: String,
        hypotheses: Vec<HypothesisRow>,
    },
    /// One hypothesis of the open investigation, whole; `None` for none.
    #[serde(rename_all = "camelCase")]
    ResearchHypothesis {
        slug: String,
        detail: Option<HypothesisDetail>,
    },
    /// What one task said, drawn as Chat draws a conversation: live while
    /// it runs.
    #[serde(rename_all = "camelCase")]
    ResearchTranscript {
        slug: String,
        task: String,
        items: Vec<ClaudeItem>,
    },
    /// Where the open investigation's round stands: its agent calls spent
    /// and allowed, the table, and every task.
    #[serde(rename_all = "camelCase")]
    ResearchRound {
        slug: String,
        round: u32,
        status: RoundStatus,
        used: u32,
        cap: u32,
        seats: Vec<ResearchSeatRow>,
        tasks: Vec<ResearchTask>,
        /// Another Proton is running its rounds, in words: read only here.
        held: Option<String>,
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
    /// The council: what has been debated, and what is being debated now.
    #[serde(rename_all = "camelCase")]
    Council {
        /// Past debates, newest first.
        held: Vec<DebateSummary>,
        /// The question being debated now, empty when none is.
        running: String,
        /// Which round is in flight — 1 or 2 — or 3 while the brief is being
        /// written. Zero when nothing is running.
        round: usize,
        /// One line per member per round so far, in order.
        progress: Vec<String>,
        /// The brief on screen, as markdown. Empty when there is none.
        brief: String,
        /// Which past debate is being shown, by directory name.
        showing: String,
        /// Why the last debate could not be run or finished.
        problem: String,
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
        /// Whether PDFs are read as well as notes.
        pdfs: bool,
        /// How many documents have been read and how many were found. The
        /// first walk over a corpus of PDFs takes minutes, and a panel that
        /// can only say "reading" for that long is one you assume has hung.
        read: usize,
        found: usize,
        /// How long the last walk took. Zero if none has finished.
        elapsed_ms: u64,
        /// The folders the library is made of, for the panel to tick through.
        sources: Vec<LibrarySource>,
        /// Which of them a search is currently narrowed to. Empty is
        /// everything — a panel with no ticks is "no restriction", not "no
        /// sources".
        scope: Vec<String>,
        /// What a search can draw on: "matching by word", "…and by meaning",
        /// or why it cannot. A sentence rather than a flag because the failure
        /// case has to carry its reason, and the panel is where somebody is
        /// looking when they wonder why answers changed.
        meaning: String,
        /// Whether the vectors are still being made. The panel says so; the
        /// distinction between "not yet" and "never" is the whole point.
        learning: bool,
    },
    /// The component the artboard is drawing.
    ///
    /// Separate from `Theme` because they are two files that change for two
    /// reasons: a token edit repaints the palette without touching the markup,
    /// and a save in Codex repaints the markup without touching the palette.
    /// One event for both would reload the component's iframe every time
    /// somebody dragged a colour.
    #[serde(rename_all = "camelCase")]
    Artboard {
        /// Empty when nothing is on the artboard.
        path: String,
        /// The markup, exactly as the file has it.
        ///
        /// Sent as source rather than as anything pre-rendered, because the
        /// only safe place to render it is inside a sandboxed frame in the
        /// webview — the core has no business building markup, and the shell
        /// must not be handed something that looks ready to inject.
        source: String,
        /// Whether the source is a whole page, which is wrapped differently
        /// from a fragment.
        document: bool,
        /// Why there is nothing to draw, when there is a reason worth showing.
        problem: String,
        /// The element tree, in document order, for the layer panel.
        layers: Vec<proton_design::Layer>,
        /// Which layer is selected, as an index into `layers`. `usize::MAX`
        /// when none is — an index nothing can equal, rather than an
        /// `Option`, because every consumer of this is comparing.
        selected: usize,
    },
    /// The scene Modeling mode is editing.
    #[serde(rename_all = "camelCase")]
    Scene {
        /// Empty when none is open.
        path: String,
        /// Every object, parents before children, with depth filled in.
        nodes: Vec<proton_scene::Node>,
        /// Which one is selected, or `usize::MAX` for none.
        selected: usize,
        /// Why there is no scene, when there is a reason worth showing.
        problem: String,
        /// Every material in the scene, with its graph.
        ///
        /// All of them rather than only the selected object's: they are small,
        /// the panel switches between them as the selection moves, and sending
        /// one at a time would mean a round trip on every click in the
        /// outliner.
        materials: Vec<proton_scene::Material>,
        /// Those same materials, compiled into programs a renderer can run.
        ///
        /// Compiled here rather than evaluated here: a surface property that
        /// varies across a mesh is one value per triangle, and at this
        /// project's budget that is eighty thousand colours crossing the seam
        /// on every nudge of a slider. A dozen instructions cross instead.
        shaders: Vec<proton_scene::Shader>,
        /// Every distinct mesh the scene names, so the front end can ask for
        /// the ones it does not already hold.
        meshes: Vec<String>,
    },
    /// One mesh's geometry, in answer to `Command::LoadMesh`.
    ///
    /// Its own event rather than a field of `Scene`, because it is the only
    /// thing this seam carries that is measured in megabytes, and because it
    /// arrives on its own schedule: a mesh re-exported from Blender while the
    /// scene is open comes back through here without the scene having moved.
    Geometry(crate::modeling::Geometry),
    /// The Godot scene Game mode is showing.
    ///
    /// `Level` rather than `Scene`, because `Event::Scene` is Modeling's and
    /// two events with one name is a front end that draws the wrong panel.
    #[serde(rename_all = "camelCase")]
    Level {
        /// Empty when none is open.
        path: String,
        nodes: Vec<proton_game::GameNode>,
        /// The file's text.
        ///
        /// Sent whole, like Design's component source and for the same reason:
        /// what Game mode can honestly show in the middle of the screen is
        /// Godot's file, until there is an engine process to run the scene.
        source: String,
        /// Which one is selected, or `usize::MAX`.
        selected: usize,
        /// Why there is no scene, when there is a reason worth showing.
        problem: String,
    },
    /// The cut Video mode is showing, and the captions beside it.
    ///
    /// `Cut` rather than `Timeline`, because the timeline is the widget and
    /// this is the document — and because `Event::Scene` and `Event::Level`
    /// already taught this enum what two events with one name cost.
    #[serde(rename_all = "camelCase")]
    Cut {
        /// Empty when none is open.
        path: String,
        /// From the list's `TITLE:` line.
        title: String,
        clips: Vec<proton_video::Clip>,
        /// The lanes, top to bottom, as an NLE stacks them.
        tracks: Vec<String>,
        captions: Vec<proton_video::Caption>,
        /// Empty when the cut has no caption file beside it, which is
        /// ordinary rather than a problem.
        captions_path: String,
        /// The sources the cut refers to, and whether they are on disk.
        media: Vec<crate::video::MediaItem>,
        /// How the list is being counted.
        ///
        /// Sent so the front end can *show* it. An EDL does not record its
        /// rate, so this is an assumption, and an assumption nobody can see is
        /// indistinguishable from a fact.
        rate: proton_video::Rate,
        /// Where the programme ends, in frames — the width of the ruler.
        length: u64,
        /// The list's text.
        source: String,
        /// Which event is selected, or `usize::MAX`.
        selected: usize,
        /// Why there is no cut, when there is a reason worth showing.
        problem: String,
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

/// One debate that already happened, for the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct DebateSummary {
    /// The directory it lives in, which is also how it is asked for.
    pub name: String,
    pub question: String,
    /// Whether it got as far as a brief.
    pub finished: bool,
}

/// One folder of the library, as the sources panel draws it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(rename_all = "camelCase")]
pub struct LibrarySource {
    /// What selects it: the prefix a citation starts with.
    pub path: String,
    /// What to show.
    pub label: String,
    pub documents: usize,
    pub passages: usize,
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

/// A picture to go with a prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
pub struct ClaudeImage {
    /// `image/png`, `image/jpeg`…
    pub media: String,
    /// The bytes, in base64.
    pub data: String,
}

/// Your answer to what Claude Code asked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../ui/bindings/")]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ClaudeAnswer {
    /// Yes — and with `remember`, the rule it suggested, for the session.
    Allow { remember: bool },
    /// No, and what to tell it instead.
    Deny { message: String },
    /// An AskUserQuestion's answers, by question.
    Questions { answers: BTreeMap<String, String> },
}

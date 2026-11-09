//! The command/event bus — the seam between the core and whatever draws it.
//!
//! The shell sends [`Command`]s and receives [`Event`]s. Nothing in this module
//! names a widget, a pixel or a DOM node: if a variant here needed to, the
//! design would be wrong and the GPU front end in `docs/ARCHITECTURE.md` would
//! be unreachable.


use proton_claude::Item as ClaudeItem;
use proton_research::supervisor::{RoundStatus, TaskRow as ResearchTask};
use proton_research::view::{
    HypothesisDetail, HypothesisRow, LiteratureSourceRow, PaperDetail, PaperResult, PaperRow, SeatRow as ResearchSeatRow, Summary as ResearchSummary,
};
use proton_research::model::{Forked as ResearchForked, HypothesisStatus, LiteratureSource, Seat as ResearchSeat};
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
    /// ⌘V in the editor: the system clipboard's text into the buffer —
    /// typed in as copied in insert mode, put before the cursor (`P`) in
    /// normal mode, over the selection in visual mode. One undo step.
    EditorPaste {
        text: String,
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
    /// Take back what one of the agent's calls wrote: the file as it was
    /// before, or gone if the call made it — only if nothing has changed it
    /// since.
    UndoAgentEdit {
        call: String,
    },
    /// Run a command the agent proposed, or decline it: its answer either way.
    AgentCommand {
        call: String,
        run: bool,
    },
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

    /// Open a web address in the browser.
    OpenLink {
        url: String,
    },
    /* ── research ─────────────────────────────────────────────────── */
    /// Keep the investigations in this folder, for every project — typed as
    /// a path is, `~` and all — or, given nothing, in each project's own
    /// `research/` again. Saved in the person's own settings.
    ResearchFolder {
        folder: String,
    },
    /// Start an investigation, in a folder of its own where investigations
    /// are kept: searching the literature with the team's queries, or not.
    ResearchNew {
        title: String,
        goal: String,
        discipline: ResearchDiscipline,
        #[serde(default)]
        literature: bool,
    },
    /// Show one of the project's investigations.
    ResearchOpen {
        slug: String,
    },
    /// Copy an investigation to take another direction: all it had at the
    /// end of `round`, or as it stands given none, under a new title; the
    /// direction, if given, is the scientist's first word to the copy.
    ResearchFork {
        slug: String,
        round: Option<u32>,
        title: String,
        #[serde(default)]
        direction: String,
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
    /// Run an experiment of the open investigation in the Lab, having read
    /// it: in the sandbox, or — on a machine with none, and only when this
    /// says so — without one.
    ResearchApprove {
        experiment: String,
        #[serde(default)]
        unsandboxed: bool,
    },
    /// Whether what the agents propose for the open investigation runs,
    /// in the sandbox, without asking first.
    ResearchAutoRun {
        enabled: bool,
    },
    /// Turn an experiment down.
    ResearchReject {
        experiment: String,
    },
    /// Stop a run.
    ResearchStopRun {
        run: String,
    },
    /// Show one of the open investigation's runs whole; `None` for none.
    ResearchShowRun {
        run: Option<String>,
    },
    /// Whether the open investigation searches the literature — each round
    /// with the team's queries, and when the scientist asks — and in which
    /// sources. What is not given stays as it is.
    ResearchSetLiterature {
        #[serde(default)]
        enabled: Option<bool>,
        #[serde(default)]
        sources: Option<Vec<LiteratureSource>>,
    },
    /// Search the literature for the open investigation, in its sources.
    ResearchSearch {
        query: String,
    },
    /// Add a paper to the open investigation: one the last search found, by
    /// its key, or one named by a DOI, an arXiv id or a link to either.
    ResearchAddPaper {
        paper: String,
    },
    /// Show one of the open investigation's papers whole; `None` for none.
    ResearchShowPaper {
        paper: Option<String>,
    },
    /// Open a paper's PDF in the document viewer, at a page if one is given.
    ResearchOpenPaper {
        paper: String,
        #[serde(default)]
        page: Option<u32>,
    },
    /// Try again to keep a paper's open PDF.
    ResearchFetchPdf {
        paper: String,
    },
    /// The scientist's first say-so for the Lab's Python: conda works out
    /// what it would be, fetching conda-forge's index if its copy is old.
    LabPlan,
    /// The second: make exactly what was planned.
    LabCreate,
    /// Stop what the Lab is doing, or put a plan aside.
    LabCancel,
    /// Take the Lab's Python away.
    LabRemove,
    /// The scientist's first say-so for Lean: GitHub is asked which Lean
    /// Mathlib has tagged, and what making it would download and keep.
    LeanPlan,
    /// The second: make exactly what was planned, in `~/.proton/lean`.
    LeanCreate,
    /// Stop making Lean — nothing it made is kept — or put a plan aside.
    LeanCancel,
    /// Take Lean and Mathlib away.
    LeanRemove,
    /* ── excel ────────────────────────────────────────────────────── */
    /// Make a workbook: a `.sheet` folder named after it, or `folder` —
    /// which may end `.xlsx`, `.csv` or `.tsv` to be kept as one.
    SheetNew {
        name: String,
        folder: Option<String>,
    },
    /// Open a workbook in the project: a `.sheet` folder, `.xlsx`, `.csv`
    /// or `.tsv`.
    SheetOpen {
        path: String,
    },
    /// Close it; refused while it has unsaved changes unless `discard`.
    SheetClose {
        discard: bool,
    },
    /// ⌘S: write it back to what it was opened from.
    SheetSave,
    /// The cells on screen: a sheet, its rows and columns from and to.
    SheetView {
        generation: u32,
        sheet: u32,
        rows: [u32; 2],
        cols: [u32; 2],
    },
    /// What the window has selected, for `@sheet`.
    SheetSelect {
        generation: u32,
        sheet: u32,
        range: String,
    },
    /// Cells typed into, by reference — `B7`, `=SUM(A:A)` — as one step to undo.
    SheetSet {
        generation: u32,
        sheet: u32,
        cells: Vec<(String, String)>,
    },
    /// The cells of a sheet, or of `within`, that have this text: answered
    /// with `SheetMatches` carrying the same `ask`. A capital in it makes it
    /// case-sensitive; `whole` asks for all of what a cell holds or shows.
    SheetFind {
        generation: u32,
        sheet: u32,
        ask: u32,
        text: String,
        whole: bool,
        within: Option<String>,
    },
    /// That text replaced in what cells hold, as one step: in the cell `at`,
    /// or in every cell a search finds.
    SheetReplace {
        generation: u32,
        sheet: u32,
        text: String,
        with: String,
        whole: bool,
        within: Option<String>,
        at: Option<String>,
    },
    /// The cells of a range onto the clipboard, as tab-separated values;
    /// kept here too, so pasting them brings their formulas. A cut is
    /// emptied when it is pasted.
    SheetCopy {
        generation: u32,
        sheet: u32,
        range: String,
        cut: bool,
    },
    /// Text pasted at a cell: what was copied here, or tab-separated text.
    SheetPaste {
        generation: u32,
        sheet: u32,
        at: String,
        text: String,
    },
    /// `to`, which starts with `from`, filled from it: a series continued,
    /// formulas moved.
    SheetFill {
        generation: u32,
        sheet: u32,
        from: String,
        to: String,
    },
    /// Rows, or `cols`, put in before `at`.
    SheetInsert {
        generation: u32,
        sheet: u32,
        cols: bool,
        at: u32,
        n: u32,
    },
    /// Rows, or `cols`, taken out from `at`.
    SheetDelete {
        generation: u32,
        sheet: u32,
        cols: bool,
        at: u32,
        n: u32,
    },
    /// A range's style changed: only the parts the patch gives.
    SheetFormat {
        generation: u32,
        sheet: u32,
        range: String,
        patch: Box<crate::sheet::SheetStylePatch>,
    },
    /// A column's width in points, or the default's.
    SheetWidth {
        generation: u32,
        sheet: u32,
        col: u32,
        width: Option<f64>,
    },
    /// The rows and columns that stay put while the rest scrolls.
    SheetFreeze {
        generation: u32,
        sheet: u32,
        rows: u32,
        cols: u32,
    },
    /// A range's rows sorted by some of its columns, each ascending or not.
    SheetSort {
        generation: u32,
        sheet: u32,
        range: String,
        keys: Vec<(u32, bool)>,
    },
    /// A filter over a range, its first row the headers; none to take it off.
    SheetFilter {
        generation: u32,
        sheet: u32,
        range: Option<String>,
    },
    /// The values a filtered column has, to tick.
    SheetFilterValues {
        generation: u32,
        sheet: u32,
        col: u32,
    },
    /// What a filtered column lets through: values ticked, a condition.
    SheetFilterSet {
        generation: u32,
        sheet: u32,
        col: u32,
        values: Option<Vec<String>>,
        criteria: String,
    },
    /// A conditional colour added.
    SheetRuleAdd {
        generation: u32,
        sheet: u32,
        rule: Box<crate::sheet::SheetRuleView>,
    },
    SheetRuleRemove {
        generation: u32,
        sheet: u32,
        index: u32,
    },
    /// A chart added over a range.
    SheetChartAdd {
        generation: u32,
        sheet: u32,
        chart: Box<crate::sheet::SheetChart>,
    },
    /// A chart moved, resized, retitled or made another kind.
    SheetChartSet {
        generation: u32,
        sheet: u32,
        index: u32,
        chart: Box<crate::sheet::SheetChart>,
    },
    SheetChartRemove {
        generation: u32,
        sheet: u32,
        index: u32,
    },
    /// A range sent to Data: written as CSV, and a cell that reads it
    /// into a dataframe put in the notebook open (or a new one).
    SheetToData {
        generation: u32,
        sheet: u32,
        range: String,
    },
    /// The selected notebook cell's dataframe brought to Excel as a sheet.
    SheetFromData,
    SheetUndo,
    SheetRedo,
    /// Show another sheet.
    SheetTab {
        sheet: u32,
    },
    /// A sheet after the one shown, named or `SheetN`.
    SheetAddSheet {
        name: Option<String>,
    },
    SheetRenameSheet {
        sheet: u32,
        name: String,
    },
    SheetRemoveSheet {
        sheet: u32,
    },
    /* ── circuit ──────────────────────────────────────────────────── */
    /// Make a circuit project: a `.circuit` folder named after it, with
    /// one empty circuit, `main`.
    CircuitNew {
        name: String,
    },
    /// Open a `.circuit` folder in the project.
    CircuitOpen {
        path: String,
    },
    /// Close it; refused while it has unsaved changes unless `discard`.
    CircuitClose {
        discard: bool,
    },
    /// ⌘S: write it back to its folder.
    CircuitSave,
    /// Show another of its circuits.
    CircuitShow {
        circuit: String,
    },
    /// A new, empty circuit in the project, shown; held until ⌘S.
    CircuitAdd {
        name: String,
    },
    /// Make a circuit the one that runs when the project does.
    CircuitSetTop {
        circuit: String,
    },
    /// Changes to a circuit's drawing, made together as one step to undo.
    CircuitEdit {
        generation: u32,
        circuit: String,
        ops: Vec<crate::circuit::CircuitOp>,
    },
    CircuitUndo,
    CircuitRedo,
    /// Run the circuit shown: built, powered on, its inputs to click.
    CircuitStart {
        generation: u32,
    },
    /// Stop running it, or stop a quick run.
    CircuitStop,
    /// An input, clock or button set — a number, as typed: `1`, `0x2a`.
    /// `hold`: the change waits for gate steps instead of settling.
    CircuitSet {
        generation: u32,
        id: String,
        value: String,
        hold: bool,
    },
    /// One clock edge: every clock flips. `hold` as for `CircuitSet`.
    CircuitClock {
        generation: u32,
        hold: bool,
    },
    /// One gate delay of what is waiting.
    CircuitStep {
        generation: u32,
    },
    /// Run the clocks as fast as they go, nothing drawn, until a Break
    /// rises or it is stopped.
    CircuitQuick {
        generation: u32,
    },
    /// A running RAM's contents kept as `data/<name>.hex`: a held edit, as
    /// any other, for a ROM's `file=` to read.
    CircuitSaveMemory {
        generation: u32,
        id: String,
        name: String,
    },
    /// A test table of a circuit written (`text`) or taken away (none):
    /// a held edit.
    CircuitTestSet {
        generation: u32,
        circuit: String,
        label: String,
        text: Option<String>,
    },
    /// Run a circuit's test tables, or the one labelled.
    CircuitTestRun {
        generation: u32,
        circuit: String,
        label: Option<String>,
    },
    /// A circuit written out beside the project, in `export/`: `format` is
    /// `verilog`, `vhdl` or `svg`.
    CircuitExport {
        generation: u32,
        circuit: String,
        format: String,
    },
    /// A Digital `.dig` file of the project, and the circuits it places,
    /// made into a `.circuit` project beside it and opened.
    CircuitImport {
        path: String,
    },
    /// A truth table and minimised forms: of a circuit of the project, or
    /// of expressions — `Y = A·B + C'; Z = A ^ B`.
    CircuitAnalyse {
        generation: u32,
        circuit: Option<String>,
        expression: Option<String>,
    },
    /// A new circuit built from a circuit's table or from expressions:
    /// `style` is `andOr`, `nand` or `nor`. Its table comes with it as a
    /// test. A held edit.
    CircuitSynthesise {
        generation: u32,
        circuit: Option<String>,
        expression: Option<String>,
        style: String,
        name: String,
    },
    /// A state machine written (`text`) or taken away (none): a held edit.
    CircuitMachineSet {
        generation: u32,
        name: String,
        text: Option<String>,
    },
    /// A machine read: the project's by name, or text as written, for its
    /// diagram and state table.
    CircuitMachineShow {
        generation: u32,
        name: Option<String>,
        text: Option<String>,
    },
    /// A machine built as `circuit` (and `circuit_logic`) with D or JK
    /// flip-flops — `flipflop` is `d` or `jk` — its run as a test. A held edit.
    CircuitMachineBuild {
        generation: u32,
        name: String,
        flipflop: String,
        circuit: String,
    },
    /// Forget what the run has recorded for the graph.
    CircuitTraceClear {
        generation: u32,
    },
    /// What the run has recorded, to Data: a CSV, and a cell reading it.
    CircuitTraceToData {
        generation: u32,
    },
    /// A part made: `name.part`, its head and nothing drawn.
    CadNew {
        name: String,
    },
    /// A part made from the library — `family` with `params`, the rest its
    /// defaults — written as `library/NAME.part`; opened, or with `add`
    /// added to the assembly open.
    CadLibrary {
        family: String,
        params: Vec<(String, String)>,
        add: bool,
    },
    /// An assembly made: `name.assembly`, its head and nothing in it.
    CadNewAssembly {
        name: String,
    },
    /// The open assembly's parts checked for overlaps and for gaps nearer
    /// than its clearance.
    CadFit {
        generation: u32,
    },
    /// A drawing of the part open, or the assembly's blueprint, written
    /// beside it as `NAME.drawing` and opened — on blue with `blueprint`.
    /// One there already is opened.
    CadDraw {
        generation: u32,
        blueprint: bool,
    },
    /// A `.part` or `.assembly` of the project opened.
    CadOpen {
        path: String,
    },
    /// The part put away; unsaved changes kept from it only with `discard`.
    CadClose {
        discard: bool,
    },
    /// The part written, as it is now.
    CadSave,
    /// The part's text as typed now: held until saved, keystrokes in a row
    /// one step to undo.
    CadText {
        generation: u32,
        text: String,
    },
    /// Edits made as one step to undo: lines of the language added,
    /// replaced or taken out.
    CadEdit {
        generation: u32,
        ops: Vec<crate::cad::CadOp>,
    },
    /// A sketch's point dragged to `(x, y)`, the solver holding the rest;
    /// `done` as the mouse lets go, the drag one step to undo.
    CadDrag {
        generation: u32,
        sketch: String,
        point: String,
        x: f64,
        y: f64,
        done: bool,
    },
    CadUndo {
        generation: u32,
    },
    CadRedo {
        generation: u32,
    },
    /// The part's bodies written beside it as `stl` or `3mf`, to 10 µm.
    CadExport {
        generation: u32,
        format: String,
    },
    /// A Logic file made: `name.logic`, a `theory` or a `worksheet` to
    /// start; or `name.proof`, a `proof`.
    LogicNew {
        name: String,
        kind: String,
    },
    /// A `.logic`, `.proof` or `.l4f` file of the project opened.
    LogicOpen {
        path: String,
    },
    /// The file put away; unsaved changes kept from it only with `discard`.
    LogicClose {
        discard: bool,
    },
    /// The file written, as typed.
    LogicSave,
    /// The file as typed now: held until saved.
    LogicEdit {
        generation: u32,
        text: String,
    },
    /// The statement at `line` (from 1) answered: a question, a formula, a
    /// declaration. `more` finds the least forms past what is found as typed.
    LogicAsk {
        generation: u32,
        line: u32,
        more: bool,
    },
    /// The theory's models searched for, on a thread: the first `keep`,
    /// and with `count` all of them counted, for some seconds at most.
    LogicSolve {
        generation: u32,
        keep: u32,
        count: bool,
    },
    /// The search stopped where it is.
    LogicStop,
    /// A proof found: of a `.proof` file's goal from its premises, in its
    /// own system; or of the argument at `line` of another file in `system`
    /// (`fitch`, `sequent` or `resolution`). The answer for `line` comes
    /// back with it.
    LogicProve {
        generation: u32,
        line: u32,
        system: String,
    },
    /// A truth table's column — `0`, `1`, `x` — read back as formulas over
    /// the names given (spaces or commas between), or `p`, `q`, `r`.
    LogicFromTable {
        names: String,
        column: String,
    },
    /// A truth table built as a circuit, from Logic: the project named opened
    /// (or made), a circuit of `inputs` and one output `Y` whose column is
    /// `column` (`0`, `1` or `x` a row, the first input the top bit), in
    /// `style` — `andOr`, `nand` or `nor` — its table the test "truth". A
    /// held edit.
    CircuitFromTable {
        project: String,
        circuit: String,
        inputs: Vec<String>,
        column: String,
        style: String,
    },
    /// What is selected in the circuit on screen, for `@circuit`.
    CircuitSelect {
        generation: u32,
        circuit: String,
        ids: Vec<String>,
        wires: Vec<[i32; 4]>,
    },
    /// The parts and wires named, as `.circ` text for the clipboard.
    CircuitCopy {
        generation: u32,
        circuit: String,
        ids: Vec<String>,
        wires: Vec<[i32; 4]>,
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
    /// Proton is about to quit: would anything be lost? Answered with
    /// `Unsaved`, naming the files with unsaved changes, or with nothing.
    AskToQuit,
    /// Write the notebook back.
    SaveNotebook,
    /// Accept or reject what the agent proposed to do to the notebook.
    AnswerProposal {
        #[ts(type = "number")]
        id: u64,
        accept: bool,
    },
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
    /// Text yanked or deleted into the unnamed register, or `"+` / `"*`: for
    /// the system clipboard as well, as vim's `clipboard=unnamedplus` puts
    /// it — so ⌘V, here or in another app, pastes what was just yanked.
    Clipboard {
        text: String,
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
        /// How many columns a tab is drawn as: the file's `.editorconfig`, or
        /// `editor.tabSize`. A browser draws one as eight.
        tab_size: u32,
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
        /// Every row of the screen; `None` for a row that is as the window
        /// was last sent it. A shell printing a line changes a row or two of
        /// the screen, and sending the rest again was most of the work.
        rows: Vec<Option<Vec<TermRun>>>,
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
        /// The walk stopped at its limit with files left unlisted, so `total`
        /// is some of the folder rather than all of it. Only a home folder or
        /// something broader is walked with a limit, and there a file missing
        /// from the list may simply not have been reached.
        cut_short: bool,
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
        /// The page to show first, counting from one: a citation's. `None`
        /// for the first page.
        page: Option<u32>,
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
        /// Whether it has changes its file does not.
        modified: bool,
        /// What the agent proposed and nobody has answered yet — also drawn
        /// on each cell, and listed here for the agent's panel.
        proposals: Vec<crate::data::ProposalView>,
    },
    /// Some of the open notebook's cells changed and nothing else did: while
    /// a cell runs, what it has printed so far. Sent in place of the whole
    /// notebook, which carried every cell's outputs, images and all, with
    /// each line a cell printed.
    NotebookCells {
        path: String,
        cells: Vec<crate::data::CellAt>,
        modified: bool,
    },
    /// The notebook's cursor moved to another cell, and nothing else changed:
    /// sent in place of the whole notebook, which `j` and `k` resent with
    /// every output in it.
    CellSelected {
        index: usize,
    },
    /// The project's `.part` and `.assembly` files, for CAD's home page.
    CadHome {
        parts: Vec<crate::cad::CadFound>,
    },
    /// The part open, built and drawn — or none.
    CadPart {
        part: Option<Box<crate::cad::CadPartView>>,
    },
    /// The parts library's families, for CAD's pages.
    CadFamilies {
        families: Vec<crate::cad_assembly::CadFamily>,
    },
    /// The assembly open, solved and drawn — or none.
    CadAssembly {
        assembly: Option<Box<crate::cad_assembly::CadAssemblyView>>,
    },
    /// The drawing open, laid out on its sheet — or none.
    CadDrawing {
        drawing: Option<Box<crate::cad_drawing::CadDrawingView>>,
    },
    /// The project's `.logic`, `.proof` and `.l4f` files, for Logic's home page.
    LogicHome {
        files: Vec<crate::logic::LogicFound>,
    },
    /// The Logic file open, as typed, and what is wrong with it — or none.
    LogicFile {
        file: Option<Box<crate::logic::LogicFileView>>,
    },
    /// A theory's models, as found so far, or none.
    LogicModels {
        models: Option<Box<crate::logic::LogicModelsView>>,
    },
    /// What the statement at the cursor is, or a table read back; none when
    /// there is nothing to say.
    LogicAnswer {
        answer: Option<Box<crate::logic::LogicAnswerView>>,
    },
    /// The project's circuit projects, for Circuit mode's home page.
    CircuitHome {
        projects: Vec<crate::circuit::CircuitFound>,
    },
    /// The circuit project open — its circuits, the one shown drawn with
    /// its pins placed, and whether it is saved — or none.
    CircuitProject {
        project: Option<Box<crate::circuit::CircuitProjectView>>,
    },
    /// What was copied, for the system clipboard.
    CircuitClip {
        text: String,
    },
    /// A circuit written out: where, and what was written.
    CircuitExported {
        path: String,
        format: String,
        text: String,
    },
    /// A truth table, minimised, or why there is none.
    CircuitAnalysis {
        generation: u32,
        analysis: Box<crate::circuit::CircuitAnalysisView>,
    },
    /// A state machine read, or why it does not read.
    CircuitMachine {
        generation: u32,
        machine: Box<crate::circuit::CircuitMachineView>,
    },
    /// Test tables run: each one's rows, and the rows that failed.
    CircuitTests {
        generation: u32,
        circuit: String,
        results: Vec<crate::circuit::CircuitTestResult>,
    },
    /// The circuit running — every wire's value, the inputs' and displays',
    /// what waits for a gate step, a quick run — or not running.
    CircuitRun {
        run: Option<Box<crate::circuit::CircuitRunView>>,
    },
    /// The project's workbooks, for Excel mode's home page.
    SheetHome {
        workbooks: Vec<crate::sheet::SheetFound>,
    },
    /// A filtered column's values as they show, each with how many rows
    /// have it, and what is ticked and asked of it now.
    SheetFilterValues {
        col: u32,
        values: Vec<(String, u32)>,
        ticked: Option<Vec<String>>,
        criteria: String,
    },
    /// What a `SheetFind` found: its first cells as `[row, col]`, row by row;
    /// how many in all, how many of those only show the text, and how many
    /// more are in rows a filter hides.
    #[serde(rename_all = "camelCase")]
    SheetMatches {
        generation: u32,
        sheet: u32,
        ask: u32,
        matches: Vec<[u32; 2]>,
        total: u32,
        shown_only: u32,
        hidden: u32,
    },
    /// What was copied, for the system clipboard.
    SheetClip {
        text: String,
    },
    /// The workbook open — its sheets, styles and whether it is saved — or none.
    SheetBook {
        book: Option<crate::sheet::SheetBookView>,
    },
    /// Cells of a sheet as they are drawn: every one with anything in the
    /// rectangle, or — `partial` — only those a change moved, which replace
    /// what the window has for them.
    SheetCells {
        generation: u32,
        #[ts(type = "number")]
        version: u64,
        sheet: u32,
        rows: [u32; 2],
        cols: [u32; 2],
        cells: Vec<crate::sheet::SheetCellView>,
        partial: bool,
    },
    /// Quitting now would lose these files' unsaved changes: the quit waits
    /// for the person to say whether to save them.
    Unsaved {
        files: Vec<String>,
    },
    /// The investigations, newest first, which one is open, and where they
    /// are kept: `research/` in the project, or a folder of their own —
    /// `~/Github/Research/` — as the window says it.
    #[serde(rename_all = "camelCase")]
    ResearchList {
        investigations: Vec<ResearchSummary>,
        open: Option<String>,
        folder: String,
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
        /// Where it was forked from, if it was.
        forked: Option<ResearchForked>,
        /// The rounds it can be forked from, by their ends kept.
        round_ends: Vec<u32>,
    },
    /// One hypothesis of the open investigation, whole; `None` for none.
    #[serde(rename_all = "camelCase")]
    ResearchHypothesis {
        slug: String,
        detail: Option<HypothesisDetail>,
    },
    /// The open investigation's experiments and runs, and the sandbox runs
    /// are in here.
    #[serde(rename_all = "camelCase")]
    ResearchLab {
        slug: String,
        experiments: Vec<proton_research::view::ExperimentRow>,
        runs: Vec<proton_lab::run::Row>,
        sandbox: proton_lab::Sandbox,
        /// Whether what the agents propose runs, in the sandbox, without asking.
        auto_run: bool,
    },
    /// One run, whole; `None` for none.
    #[serde(rename_all = "camelCase")]
    ResearchRun {
        slug: String,
        detail: Option<proton_lab::run::Detail>,
    },
    /// The open investigation's literature: whether it is searched, the
    /// sources and whether their keys were found — never a key — and its
    /// papers, with those whose PDFs are downloading and the names being
    /// looked up to add.
    #[serde(rename_all = "camelCase")]
    ResearchLiterature {
        slug: String,
        enabled: bool,
        /// Whether what the team cites is checked.
        check: bool,
        /// Whether this Proton asks anybody at all.
        live: bool,
        sources: Vec<LiteratureSourceRow>,
        papers: Vec<PaperRow>,
        fetching: Vec<String>,
        adding: Vec<String>,
    },
    /// One paper of the open investigation, whole; `None` for none.
    #[serde(rename_all = "camelCase")]
    ResearchPaper {
        slug: String,
        detail: Option<PaperDetail>,
    },
    /// The scientist's last search of the open investigation: what it asked,
    /// whether it is still asking, what it found, and which sources failed.
    #[serde(rename_all = "camelCase")]
    ResearchResults {
        slug: String,
        query: String,
        searching: bool,
        results: Vec<PaperResult>,
        failures: Vec<String>,
    },
    /// Where the Lab's Python stands: one for the machine, whichever
    /// project is open.
    LabEnv {
        env: proton_lab::EnvState,
    },
    /// Where Lean and Mathlib stand: one for the machine, whichever project
    /// is open.
    LeanEnv {
        env: proton_lab::LeanState,
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
        /// The round's search of the literature, if it has one.
        literature: Option<proton_research::supervisor::LiteratureRow>,
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
        /// The conversation's entries from `from` on: those before it are as
        /// the window was last sent them. A whole conversation is from 0.
        transcript: Vec<Entry>,
        from: u32,
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


# Proton — architecture

This document describes the implemented system and records its build history.
All eight modes read real documents. The design reference is [`DESIGN.md`](../design/DESIGN.md),
and the tool choices are in [`STACK.md`](STACK.md).

The capability table below describes current behavior. Milestone sections later
in this document retain historical decisions and measurements.

| Mode | Implemented document and interaction | Remaining limitation |
|---|---|---|
| Codex | Text buffers, modal editing, search, splits, LSP/DAP, Git, terminal | Language servers, debuggers, and trace backends are external tools |
| Data | `.ipynb` cells, kernel execution, outputs and dataframe views; an agent whose edits are proposed on the cells; unsaved changes kept | Execution requires a Jupyter kernel; no SQL cells, charts or lineage yet |
| Research | `research/<slug>/` investigations; rounds on Claude Code, Codex and Antigravity, followed call by call; an Elo leaderboard; your feedback and hypotheses | Experiments run sandboxed once approved; figures, 3D scenes and molecules drawn in the Lab |
| Excel | `Name.sheet/` workbooks, `.xlsx`, `.csv`, `.tsv`; Proton's own formula engine recalculating only what a change reaches; a grid drawn by the rows on screen; formats, frozen panes, sorting, filters, colour rules, charts; sheet tools for the agent; ranges to and from Data | `.xlsx` merged cells, conditional formats, filters, pictures, pivot tables and macros are not kept (said on opening); a million rows take about 4 s to open and 0.7–1 GB |
| Logic | `.logic` worksheets and theories, `.proof` proofs and Logic4Fun's `.l4f`; Proton's own calculator, model finder and provers (`proton-logic`): truth tables, forms, K-maps, models found and counted, why there is none, semantic tableaux, natural deduction, the sequent calculus and resolution checked and found; logic tools for the agent | Natural deduction is found within a bound for first-order logic (29 of 37 Pelletier problems); no modal, intuitionistic or temporal logics; no equality reasoning in the provers |
| Circuit | `Name.circuit/` projects and Digital's `.dig` files; Proton's own simulator with gate delays, quick run and Break; editing, subcircuits, memories, test tables and graphs; truth tables, K-maps, synthesis, state machines; Verilog, VHDL and SVG out; circuit tools for the agent | No 74xx library, FPGA boards or co-simulation; the written Verilog and VHDL are checked by their text unless Icarus or GHDL is installed |
| CAD | `.part` files: parameters, sketches solved by Proton's own constraint solver, extrudes and revolves joined, cut and met with bodies, holes (tapped, counterbored, countersunk), patterns and mirrors, rounds and chamfers (mitred, smooth and ball-cornered), shells, sweeps along paths in space, lofts, on Proton's own B-rep kernel (`proton-cad`) with its own booleans, each body checked valid and measured exactly; the steps, a 3D view with edges and faces picked, sketching with the mouse, dimensions typed in place; STL and 3MF out, and STEP AP214 out exact and read back by Proton's own reader; `.assembly` files: parts and assemblies placed by mates solved on Proton's own solver, freedoms counted, placements written back, overlaps and clearances checked, a bill of materials, exploded views; a parts library (Mini-ITX, SFX, M.2, DIMM, graphics cards, fans, radiators, blocks, pumps, G1/4 fittings, ISO fasteners, the FormD T1 and its published limits) written as `.part` files; liquid-cooling routes between fittings' sockets, hard and soft tube bent to its radius, with cut and bend lists and the loop's order; `.drawing` files: a part's or an assembly's views projected with hidden lines, sections hatched, details, dimensions measured on the model, a title block, balloons and a bill of materials, an assembly's blueprint on paper or white on blue, written as PDF, SVG and DXF by Proton's own writers; the agent's `cad_read`, `cad_write` (held until saved), `cad_measure` and `cad_check` (with the T1's published limits), and `@cad` | Electronics is a later milestone; the T1's inside is not published (docs/CAD.md); a spline in a profile is refused; rounds only where they are analytic; near-tangent faces can leave a few triangles unmatched |

```bash
cargo test --workspace        # headless; some integrations require external tools
cargo test --workspace -- --include-ignored --test-threads=1   # plus the live ones
                              # serial: the tracer, gdb and gcc all want the machine
git submodule update --init   # the Code Visualizer checkout, for the Visualizer pane
cargo run -p proton-shell     # the app
node ui/serve.mjs             # the front end alone, in a browser
npm ci && npm run check       # checked frontend modules and generated protocol
```

---

## 1. The commitment

Everything else follows from one decision: **the core is a library with no UI
dependency, and the front end is replaceable.**

```
╭──────────────────────────────────────────────────────────────╮
│  proton-core            pure Rust · no windowing · no webview │
│                                                               │
│   buffers · vim engine · syntax · LSP/DAP · git · index       │
│   agent runtime · trace client · wasm host · mode registry    │
╰───────────────────────────────┬──────────────────────────────╯
                                │  Command in · Event out
                                │  serde-typed, transport-agnostic
                ╭───────────────┴────────────────╮
                │                                │
      ╭─────────▼──────────╮        ╭────────────▼───────────╮
      │  proton-shell      │        │  GPU editor surface    │
      │  webview · chrome  │  ────▶ │  later, only if the    │
      │  all eight modes   │        │  latency budget fails  │
      ╰────────────────────╯        ╰────────────────────────╯
```

Two rules keep that seam honest:

1. **`proton-core` never names a widget.** It emits viewport, cursor, and document state through `Event`, rather than
   calling a renderer. If a core crate would need to know pixels, the design is
   wrong.
2. **The vim engine lives in the core.** It resolves keys to *actions*
   (`Delete·Line`, count 2), which is exactly the boundary that lets a different
   front end reuse it verbatim.

The webview ships first because eight modes demand it. The GPU surface is held
in reserve for the editor pane alone, and only once a measured
input-to-photon number justifies it. See STACK.md § "The actual decision".

---

## 2. Crate layout

One cargo workspace. Every crate below `proton-shell` builds and tests headless.

| Crate | Owns |
|---|---|
| `proton-core` | Sessions, workspaces, modal engine, command/event bus, orchestration |
| `proton-text` | Rope buffers, undo, selections, motions, text objects |
| `proton-index` | File walking, fuzzy matching, grep |
| `proton-syntax` | Tree-sitter parsing and highlights |
| `proton-lsp` / `proton-dap` | Language-server / debug-adapter protocols and supervision |
| `proton-vcs` / `proton-term` | Git state / pseudo-terminal and terminal screen |
| `proton-agent` / `proton-council` | Model conversations and tools / multi-model debates, from the terminal |
| `proton-claude` | Claude Code, Codex and Antigravity as engines: interactive, and unattended with structured answers |
| `proton-research` | Investigations as files, and the co-scientist round as a pure state machine |
| `proton-literature` | The literature's sources — arXiv, Semantic Scholar, OpenAlex, Crossref — each host asked no oftener than it asks, on a worker the core polls; open PDFs downloaded, capped and checked |
| `proton-lab` | The Lab's Python — a conda environment, or a venv where there is no conda — planned, made and removed on the scientist's say-so, and the jobs that run for it; and Lean and Mathlib, made the same way |
| `proton-library` / `proton-mcp` | Document retrieval and citations / MCP service |
| `proton-data` | Notebook documents and kernel integration |
| `proton-plugin` | Wasmtime plugin host and capability grants |
| `proton-trace` | Code Visualizer client and sidecar integration |
| `proton-shell` | Tauri window, IPC, native backdrop, packaging |

These are the 29 current workspace members. The modal engine remains in
`proton-core/src/vim.rs`; there is no separate `proton-vim` or `proton-cli` crate.

`proton-lsp` ended up with no async runtime and no protocol crate. Two threads
and a channel do what `tokio` would, for a client that talks to two or three
processes; and the protocol surface Proton actually uses is a handful of
response shapes, each with a subtlety worth writing down rather than
inheriting.

`proton-agent` went the same way, and the table above has been corrected to
match. It has no async runtime, no HTTP framework and no SSE crate: streaming a
response off a worker thread wants a plain `Read`, not a `Stream`, and the
whole of SSE is "`event:`, `data:`, a blank line dispatches". `ureq` over
`rustls` keeps the tree small and leaves no OpenSSL to find on Windows.

**Why Tauri v2 rather than raw `wry`:** the updater, packaging, and the
permission model are all things you would otherwise write. On Windows 11 the
WebView2 runtime is already present, so it adds no install burden. If Tauri
becomes a constraint, `wry` is underneath it and the swap is contained to
`proton-shell`.

### The bridge

Rust owns `Command` and `Event` in `crates/proton-core/src/bus.rs`.
`ts-rs` generates their discriminated unions in `ui/bindings/`. The Tauri
bridge serializes commands such as `openBuffer`, `pickerQuery`, and `setMode`,
then delivers returned events and asynchronous pushes to the frontend.
Both native channels carry `{ sequence, events }`. `BridgeSession` assigns the
sequence under the same mutex that applies commands and polls, and empty
polls consume no sequence. `ui/transport.js` serializes commands and merges
replies/pushes in that order. An initial sync establishes the baseline after
events missed before listener setup. Duplicate batches are ignored. A missing
batch blocking a reply for two seconds triggers a fresh sync, without replaying
the uncertain user action. Dependent UI gestures reserve their commands together
so focus and editing cannot be separated by another keystroke.
`Viewport` carries a window of source lines; other events carry cursor,
buffer-list, tree, settings, or mode-specific document state. Events are not
uniformly deltas, and not all of them have version fields.

`ui/protocol.ts` adapts generated `bigint` fields to JSON wire numbers.
`npm run check` strictly checks the JavaScript modules listed in
`ui/tsconfig.json`, including native bridge, transport, editor state, text rows,
file tree, buffer tabs, resizing, feedback, display settings, history, and timing.
Negative contract assertions reject invalid commands, pane updates, native binary
read arguments, and JavaScript bigint in JSON commands. `native-api.d.ts` describes
the narrow Tauri surface; command and event payloads use the generated protocol.
`app.js` and the legacy demo implementation remain outside the complete check.
The demo has an explicitly declared facade in `preview-bridge.d.ts`; this checks
its consumers, not the fixture implementation itself.

Frontend responsibilities now include:

- `app.js`: command routing and mode/document rendering.
- `bridge.js`: strictly checked native API wiring, binary reads, and window controls.
- `preview-bridge.js`: legacy standalone demo fixtures, dynamically imported only without Tauri.
- `transport.js`: ordered command delivery, event batches, recovery, and typed key commands.
- `editor-state.js`: typed pane state and one paint per affected pane per event batch.
- `editor-row.js`: text rows, character selections, syntax/search spans, diagnostics, and gutters; pane and diagnostic state are explicit inputs.
- `html.js`: shared HTML escaping and SVG icon markup.
- `buffer-tabs.js`: semantic buffer tabs, manual keyboard activation, safe close, and stable focus.
- `split-resize.js`: pointer and keyboard editor-divider resizing with accessible values.
- `editor-feedback.js`: delayed file-loading status, empty-buffer guidance, and dismissible warnings/errors.
- `performance.js`: opt-in, bounded command/render timing; no text or paths recorded.
- `file-tree.js`: accessible tree rendering, keyboard focus, navigation.
- `overlays.js`: modal visibility and focus containment.
- `panels.js`: panel visibility, resizing, responsive layout, local preferences.
- `display-settings.js`: project-scoped text size, density, and blur controls.
- `viewport.js`, `meshbytes.js`, `document.js`: 3D and document views.

The browser harness at `/tests/ui/index.html` loads production UI with an
in-memory transport. `npm run test:unit` covers ordering and pure row rendering.
`npm run test:browser` runs the harness and standalone preview in isolated
Chromium contexts using Playwright, with a managed loopback server on port 5180.
CI runs browser checks on Linux, macOS, and Windows and retains failure artifacts.
These checks make no model calls or writes to a real workspace, and do not
replace native webview testing. Native checks still require a rebuilt shell,
because the frontend is embedded in that binary.

Transport requests have a 15-second reply timeout. A failed request cancels
input already queued against uncertain state; the next fresh interaction
establishes a snapshot before proceeding. Passive viewport updates are skipped
while document/project navigation is pending and measured again afterward, so
one project's old scroll position cannot be sent to another project.

For files at least 1 MB, syntax parsing has a cooperative 25 ms budget. Exceeding
it clears the incomplete parse and pauses highlighting for that buffer, with a
visible notice. Editing, scrolling, saving and undo continue using the rope.
Reopening retries highlighting; reducing the buffer below 1 MB removes the
limit. This bounds parser work, not file I/O, query compilation, language-server
work, or total input-to-display latency.

Session layouts distinguish scratch panes from file indices, clamp restored
scroll positions, and read the live cursor of the focused pane. The native
shell saves a final layout on normal exit. This restores positions and files;
it does not persist unsaved buffer contents or provide crash recovery.
`npm run test:transport` also runs the ordering/recovery tests in Node and CI.
Editor batches apply state before painting, without waiting for an animation
frame. Unchanged rows and buffer-tab markup keep their existing DOM nodes.

---

## 3. Core subsystems

### Workspaces

A **workspace** is one project: its own buffers, file index, language servers,
agent conversation and trace. A **session** holds several and keeps one active.
Both live in `proton-core`; the shell owns a `Mutex<Session>` and nothing else.

The split is not cosmetic. Everything that makes a project a project is rooted
somewhere — the walk, the servers, the agent's idea of what "this codebase"
means — so a second project is a second set of all of it, and two projects
sharing one file index would not be two projects.

Two rules keep that from being expensive:

- **Only the active workspace draws.** Every command goes to it and only its
  events cross the bridge. The others are state, not screen.
- **Every workspace is still polled.** A background project's servers keep
  answering, and a mailbox nobody drains fills up and leaves the project stale
  the moment you switch back. So `Poll` goes to all of them and only the active
  one's events are kept.

**Who claims a keystroke.** Switching projects is not something a project can
do to itself, but the modal engine — where a leader sequence is assembled —
lives inside the workspace. The rule that resolves it: *the session claims
leader prefixes the workspace did not.* A workspace reporting an unresolved
`␣fp` is reporting that nothing below knew what it meant, and the layer above
takes it from there. Nothing in the modal engine has to know a session exists,
and the same mechanism is what a plugin keymap will use.

**Opening a folder** cannot reuse any of Telescope: the folder you want is by
definition outside the project, so there is no index and nothing to walk in the
background. `proton-core/src/folder.rs` completes a path one segment at a time
the way `:e` does — one `read_dir` per keystroke, whatever the size of the
disk — and the picker shows recents when the query is a name and the disk when
it is a path. The trailing separator is what tells those apart.

**What is remembered**, in `%APPDATA%\Proton\session.json`: the open set, which
one was active, and the projects opened before. A named folder is an
instruction and is opened on top of the restored set; the working directory is
only a fallback, used when there is nothing to restore and nothing was named.
Keeping those apart is what stops a bare `proton` typed in a downloads folder
from adding it to the projects list for good.

### The file tree

Held as *which folders are open* and nothing else. The rows are computed from
that and the disk whenever anyone asks, so there is no cached listing to go
stale — a file another process created shows up the next time the tree is
drawn rather than never, and there is no invalidation rule to get wrong.

That is affordable because a tree only ever shows what has been expanded: the
cost is one directory read per *open* folder, not a walk. A closed tree costs
one. `␣ff` is the thing that walks, and it does that once, on a worker.

Listing goes through `proton_index::children`, which applies the same ignore
rules the walk does. A tree offering `target/` while `␣ff` refuses to find
anything in it would be two different answers to "what is in this project",
and the user would be left deciding which to believe.

The event carries only the visible rows, which is the viewport discipline
applied to a different shape of data: a closed folder contributes one row
however much is inside it. A folder with more than 500 entries is cut off with
a row saying how many were left out — ten thousand generated files is ten
thousand rows nobody reads, and the count is the useful part.

`␣e` reveals the file you are in, opening every folder above it so the row
exists to scroll to. The frontend exposes tree/treeitem roles, levels,
expanded and selected state, and one Tab stop per tree. Up/Down, Home/End,
Left/Right, and type-to-find move focus; Enter opens a file or toggles a folder.
Escape returns focus to the editor. Core refreshes preserve focus or move it
to the closest surviving ancestor when a folder closes.

### The terminal

`proton-term` is the same arrangement as every other foreign process here: the
shell runs out of process, a reader thread pumps its bytes into a channel, and
the core drains that on its heartbeat. `portable-pty` for the pseudo-terminal
and `vt100` for the screen — 34 crates, measured before committing to them.

**ConPTY asks a question and stalls until it is answered.** The first thing a
Windows pseudo-console sends is `ESC[6n` — *where is the cursor?* — and it will
not pump the child's output until something replies `ESC[<row>;<col>R`. A
terminal that ignores it reads four bytes and then nothing, which looks exactly
like the shell failing to start. That was measured, not read: the probe that
found it saw a live child process, no output, and those four bytes. `vt100`
parses the screen but does not answer queries, so the reply is sent by hand.

The screen crosses the bridge as **rows of runs**, merged by look, for the
reason the editor's viewport merges its spans: a row is mostly one colour, and
a cell per character would be eighty objects for a line of prose. Trailing
blanks are trimmed — but only *unpainted* ones, because a run with a background
is drawing those spaces on purpose and cutting them shortens the bar.

Colours arrive as palette indices and are coloured by the theme, so a terminal
in Nord does not look like a terminal in Tokyo Night; only a true colour, which
has no name in the palette, is sent as itself.

The panel takes the whole keyboard while it has the focus, exactly as the
picker does and for the same reason — `d` types a `d` at the shell rather than
starting an operator, and `⌃c` interrupts a program rather than doing nothing.

**Opening the panel is the request.** There is no button to start a shell:
showing a terminal panel means wanting a terminal, and a panel that sits empty
until you find the play button is one that does nothing for the length of time
it takes to notice. Asking for a terminal when the last one has exited gets a
new one rather than its corpse a second time. The only control is a restart,
and it appears only once a shell has gone.

**Cell size is measured, not inferred from a row.** A `.trow` is a block, so
its width is the container's whatever is written in it — dividing that by the
row's character count made the terminal's width depend on how long the first
line happened to be. A one-character prompt reported twenty columns in a panel
that fits fifty, which is what "the terminal is cut off" turns out to mean.
The measurement is a hidden span of known length in the terminal's own font,
and the padding comes off both axes: it is inside `clientHeight`, so leaving it
in claims a row that is drawn where it cannot be seen. Sideways scrolling is
off, because the shell already wraps at the width it was given — a horizontal
scrollbar could only mean the measurement was wrong, and it would steal the
height of a row on its way in.

**Scrolling back is not a second screen.** `vt100` keeps two thousand rows of
history, and once an offset is set its `cell` answers from that history — so
the rows the panel draws while scrolled back come from the code that draws the
live screen, not from a second path that could disagree with it. The offset
lives in `vt100` rather than beside it, for the same reason: two copies of a
position are two positions.

The offset counts rows from the *bottom*, which is the detail that matters.
Holding it still while new output arrives would slide the history downward
under the reader — exactly when you have scrolled back to read an error from a
build that is still printing. So `poll` measures how much history grew and
moves the offset by that much, keeping the view on the same text. The bound is
never tracked here either: `set_scrollback` clamps to what was really kept, so
the position is set and then *read back* rather than predicted.

Typing returns to the live screen, because what you are about to type will
appear there. A resize returns to it too — reflowing changes what a row number
means, and keeping the number would land somewhere arbitrary while looking
deliberate. The cursor is not drawn while scrolled back, since its row belongs
to the live screen. And the panel says when it is looking at history, because
a terminal scrolled back looks exactly like one whose shell has stopped
answering.

### Documents you look at

**A PDF never becomes a buffer.** A buffer has an undo history, a cursor and a
`:w`; offering any of those for a file that cannot be edited would be a promise
the editor cannot keep. So `openBuffer` on a `.pdf` answers with
`Event::Document` instead, and the viewer covers the stage of whatever mode is
on screen rather than becoming a pane — Codex's code but not its shell, Data's
notebook, anything else's whole — and closing it puts back what was there
because nothing moved. A library citation by page opens the viewer at that
page (`Event::Document`'s `page`) rather than handing the file to the
desktop's reader. While it is up it takes the keyboard as the picker does:
`j`/`k`, Space, `⌃d`/`⌃u`, `gg`/`G`, `+`/`−`, `0` to fit, Escape or `q` to
close — and nothing reaches the buffer or cell underneath, where `dd` would
delete a line nobody can see.

Before this, opening one failed outright: `read_to_string` refuses invalid
UTF-8, so the whole of the old behaviour was an error notice. The viewer only
reads; a PDF to be changed is Quark's.

`ViewerKind` is a small closed set rather than "anything that is not text".
A format belongs in it only once something can actually draw it — treating
every binary as a document would replace one honest error with a viewer that
has nothing to show.

**The bytes do not go through the event channel.** `Event::Document` carries a
path; the shell answers a separate `read_document` with `ipc::Response`, which
hands the webview an ArrayBuffer directly. Base64 through the JSON channel
would inflate a PDF by a third and make every other event queue behind it, and
Tauri's asset protocol would mean enabling a second scheme and choosing a path
scope for it. It reads only a file the core has shown as a document — asked
under the session's lock, read after letting go of it — no larger than 128 MB,
and off the main thread, which a large read would otherwise hold.

`pdf.js` is vendored, because `script-src 'self'` means nothing loads from a
CDN at run time — an editor that needs the network to open a file is one that
stops working on a train. It is imported on first use rather than at boot: it
is 1.7MB, and a session that never opens a PDF should never pay for it. Pages
are drawn as they come within a screen or so of the window and let go of as
they leave it: drawn all at once, each is a canvas of eight megabytes on a
high-DPI screen, and a seven-hundred-page textbook would ask for more memory
than the window has. Each drawn page carries pdf.js's text layer over it, so
its words can be selected and copied; the layer's rules are written into
`styles.css`, since pdf.js's viewer stylesheet is not vendored. The vendored
build has no `cmaps/`, `standard_fonts/` or `wasm/`: a PDF whose text needs a
predefined CJK CMap draws its glyphs without their text, an unembedded standard
font is drawn in the nearest system one, and a JPEG 2000 picture is missing.

### Git

The gutter has always had git; the *panel* is new, and it lives in the right
dock beside the agent, the debugger and the terminal rather than on the left
rail. The rail button is a jump to it, the way the debugger's and the
visualizer's already were — the left rail names things, the dock holds them.

**The list is asked for, never remembered.** Git's answer moves for reasons
that never pass through this core: a commit made in the terminal panel, a
branch switched in another window, a rebase. A remembered list would be stale
exactly when somebody opened the panel to find out what had happened. So
opening it asks, and `Sync` carries one too — a panel that is empty until you
click it twice is one people conclude is broken.

Paths are relative and spelled with forward slashes, which is the spelling
`openBuffer` and the file index already use. That is what lets a row be
clicked open with nothing in between translating it.

A folder that is not a repository, a detached `HEAD`, and a checkout with no
commits yet are all `None`, and all mean the same thing to the panel: there is
no name to show. None of them is an error.

### Debugging

`proton-dap` is layered the way `proton-lsp` is and for the same reasons: an
adapter is a third-party process that can hang or die, so it lives out of
process behind two threads and a channel, and the core drains a mailbox on the
same heartbeat as everything else. The framing is literally
`proton_lsp::transport` — DAP took it from LSP — and sharing the
implementation beats a copy that could rot into disagreeing with the original.

**The handshake is not a straight line**, and getting it wrong looks exactly
like a breakpoint that does not work. `initialize`, then `launch` *without
waiting for it*, then the adapter's `initialized` **event** says it is ready to
be configured, and only then may breakpoints be set; the `launch` response
comes back last. The reader thread answers the `initialized` event itself, the
same way `proton-lsp`'s reader answers server requests — nothing above should
need to know the shape of a handshake in order to set a breakpoint.

**A breakpoint's first answer is not its last.** `setBreakpoints` before the
program is loaded comes back `verified: false, reason: pending`; the real
answer arrives later as a `breakpoint` event. The response is deliberately not
read at all — a UI that draws it shows every breakpoint as broken.

**Breakpoints exist without an adapter.** You set them while reading, long
before anything runs, so they are a per-file set of lines that shifts with
edits the way marks do, and the adapter is told about them when one starts.
An unverified one draws hollow: a breakpoint the debugger will never hit is
the difference between a bug and a misunderstanding, and it should say which.

**What to run comes from `.proton/debug.json`** — an adapter, its arguments,
and a program. Absent, the session says what is missing. Guessing which binary
you meant and debugging the wrong one is worse than asking.

The live tests drive GDB's own DAP interpreter, which is already on this
machine for the Visualizer and debugs C, C++ and Rust. They compile a small
program, stop in it, and read the locals back — because the protocol module's
other tests prove what Proton does with a message, and only a real adapter
proves the messages are the ones it will get.

### Git

Two questions with different answers. **What changed in this file** is the
gutter: a line diff between the version at `HEAD` and the buffer *as it is
now*, not the file on disk — the point of a gutter is seeing what you have
done before you save. **What have I touched** is `␣gg`, which is a Telescope
picker like every other list in the editor rather than a panel of its own.

`gix` rather than shelling out to `git`, as planned — and measured before
committing to it: 135 crates and about a minute on a clean build. That is a
real cost, but a one-off, and it buys packfiles, deltas, worktrees and every
future object-format change. Shelling out would have meant parsing porcelain
and a hard dependency on a binary.

**What is cached, and against what.** Reading the blob at `HEAD` goes through
packfiles and deltas, and only changes when the commit does — so it is read
once per buffer and kept. The diff has to run whenever the buffer changes,
because a gutter that only moves when you save is not showing you anything.
So the blob is cached against the buffer and the marks against that buffer's
*version*: typing recomputes one line diff over a string already in memory,
and scrolling recomputes nothing. A save drops the blob; closing a buffer
drops everything, because the index it used will be reused by whatever opens
next and stale marks on the wrong file are worse than none.

Histogram rather than Myers, which is what git itself defaults to and for the
same reason: on source it finds the hunk boundaries a person would. A gutter
that marks a closing brace instead of the function you changed is worse than
no gutter.

A deletion has no line to mark, so it attaches to the line that took its
place — and loses to a mark already there, because the change that *is* there
is more useful to see than the one beside it. Not being in a repository at all
is ordinary rather than an error: plenty of folders worth editing are not
checkouts, and the gutter simply has nothing to say about them.

### Splits

A split is a **view**, not a copy: it knows which buffer it shows, where its
cursor is and where it is scrolled to. So two splits on one file scroll apart
and keep their own cursors, which is the only reason to open the same file
twice — and an edit in either shows in both, because they are views of one
document rather than two of anything.

**How wide each one is, is the front end's.** `Command` and `Event` name no
pixels, so the shares live in the DOM and a second front end would size its
panes its own way. Dragging a divider moves only the pair either side of it:
the two share the span they already had between them, and everything further
out keeps the size it was given. A drag between panes 2 and 3 that shoved
pane 1 about would be a layout you cannot aim.

**Where the cursor lives.** It is a field on `Buffer`, and every motion and
operator in `editor.rs` reaches for it there; moving it onto the view would
mean threading it through all of them. It does not have to be. *Only the
focused view receives commands*, so its is the only cursor that can move and
every other view's is frozen. The buffer holds the focused view's, each other
view holds its own, and they are exchanged on a focus change. One swap, and
nothing below has to know splits exist.

**One row, not a tree.** Vim nests; this is a single row or column. That
covers what people do — two or three panes side by side — and a tree is a
great deal of layout code for the case it does not. The shape is a `Vec` and a
direction, which a tree can replace without the rest of the core noticing.
Focus moves *along* the layout, and across it there is nothing, exactly as
vim's does in the same layout.

`Viewport` and `Cursor` events carry a view index, so each pane reports its own
height and scroll and is redrawn on its own. `:vsplit`, `:split`, `:only`,
`:close`, `⌃w` and the `␣w` family are all the core's; the front end builds
panes from the layout it is given and sends clicks back.

**`:q` closes the split while there are several**, and the buffer when there is
one. Vim would quit, and an editor whose `:q` sometimes shuts the whole window
is one you lose work in. `:bd` always means the buffer.

### Text

`ropey` behind a `TextStore` trait. It already tracks UTF-16 code units in its
node info (`char_to_utf16_cu` is O(log n)), which is the thing LSP needs and the
thing that is painful to retrofit — verified, not assumed. Undo is a **tree**,
not a stack, and it is per-buffer state in the core so `u` works identically in
every buffer, whichever mode opened it.

Large files are a policy in the open path, not a property of the rope: mmap,
lazy newline indexing, skip tree-sitter over a size threshold, never notify the
LSP. Keep that policy in one function so it is testable.

### Input

```
keystroke ─▶ vim state machine ─▶ Action { verb, object, count, register }
                                        │
                                        ▼
                            active mode's verb table
```

Every mode implements the same verb set over its own nouns. A mode that cannot
implement a verb returns `Unsupported`, and the statusline says so — silently
doing nothing is the failure users cannot debug.

Non-negotiable for this to feel real rather than emulated: operators compose
with motions and text objects, counts multiply, registers and marks are global,
macros record *actions* (so they replay across modes), `.` repeats, undo is a
tree.

In Codex, continuous typing shares one undo step, including newlines and
backspaces. Cursor movement, explicit commands such as save or pane switching,
and leaving Insert end the group; viewport updates and polling do not. Each
dot or macro replay groups its edits into one step. Saving freezes that history
revision so later typing becomes dirty and can be undone back to the saved
state. Undo and redo end any open group before walking history.

Pane buttons and platform shortcuts dispatch typed `Perform` actions through
`ui/history-controls.js`. Native text fields retain their own history. Normal
mode accepts `u` and Ctrl+R; macOS accepts Command+Z / Shift+Command+Z, and
Windows/Linux accept Ctrl+Z / Ctrl+Shift+Z or Ctrl+Y.

**Search** is literal, with vim's smartcase: a pattern containing a capital is
case-sensitive, one without is not. No regular expressions — `/` earns its keep
long before `\<foo\>` does, and a half-supported regex dialect is worse than an
honest literal one. Folding is ASCII rather than Unicode because
`char::to_lowercase` can change how many characters there are, and a match
offset that is off by one is worse than not folding a Turkish dotted I.

`Buffer::matches` returns *every* match rather than the next one. That is what
makes wrapping fall out — and what `hlsearch` will want when it arrives. The
pattern is remembered by the workspace rather than the modal engine, because
`n` means "again" and only that layer knows what again was; `N` is the opposite
of the direction the search was going, so after `?foo` it is `n` that keeps
going up. Wrapping says so, since silently wrapping looks exactly like the
search having gone the wrong way.

`/` and `:` share the line at the bottom and differ in one thing — what Enter
means — so they share a mode and the prompt character distinguishes them. A
search is a motion, so a selection survives it: `v/foo` extends to the match
exactly as `v}` extends to the paragraph, and closing the line gives visual
mode back.

**Matches are painted per visible line, not per file.** A pattern cannot
contain a newline — Enter submits it — so a line is a complete unit, and the
cost is the window rather than the document however many thousand hits are
below the fold. They travel as their own layer beside the syntax spans rather
than as another scope: a match is a background and a scope is a colour, and a
match inside a string has to stay string-coloured.

**Incremental search previews from where the prompt opened**, never from where
the last preview landed — otherwise each character jumps to the match after the
one before, and the cursor walks away down the file as you type. Committing
undoes the preview before jumping, for the same reason. `⎋` puts the cursor
back where it started; `:noh` stops the painting without forgetting the
pattern, which is the distinction people use it for.

The folding rule lives in `proton-text`'s `search` module because two callers
need it: the whole-rope scan that answers "where do I jump next" and the
per-line scan that answers "what do I paint". A search that jumps to a match it
will not highlight is worse than one that does neither.

**The `:` line** parses in `proton-core/src/ex.rs` and runs in the workspace.
Names abbreviate the way vim's do — `:w`, `:wr`, `:wri`, `:writ`, `:write` are
one command — and `!` is a separate token, because every command that can
refuse needs somewhere to say "I know, do it anyway".

`:w`, `:e`, `:q`, `:bn`, `:bp`, `:saveas`, `:bd`, `:wq` and `:x` are the
core's. Everything else passes back out as `CmdlineRun` for the front end,
which still owns `:vsplit`, `:close` and `:only` — splits are view state and
the core has no opinion about them yet. Ranges (`:1,5d`) are deliberately not
parsed rather than half-parsed.

**`:q` closes the buffer, not a split.** Vim's closes a window and leaves the
buffer hidden, which makes sense when buffers are invisible; here they are the
tab bar, and a tab you cannot close is the more obvious missing thing. Splits
keep `:close` and `␣wq`. The editor always has at least one buffer — `buffer()`
indexes the list and every caller assumes it succeeds — so closing the last one
leaves a fresh empty buffer rather than nothing to draw.

Two behaviours are worth stating because they look like bugs otherwise:

- **`:w {file}` is a copy**, and leaves you editing the original, still
  modified — vim's rule. `:saveas` is the one that moves house. The exception,
  also vim's, is a buffer with no name of its own: writing that one adopts the
  name, because the alternative is a file you have no way to keep editing.
- **A write refuses when the file changed underneath it.** Formatters,
  language servers and `git checkout` rewrite files under an open editor
  constantly, and silently discarding that is the one data-loss bug an editor
  must not have. `:w!` overrides; `:e!` throws yours away instead. Both
  `Command::Save` and `:w` go through the same guard, because a guard with a
  way around it is not one — and so does the `×` on a tab, which is never
  forced: losing unsaved work should cost a deliberate `:q!`, not a mis-aimed
  pointer.

`:e` re-reads through the ordinary edit path rather than rebuilding the buffer,
which keeps the parse tree incremental, the marks shifted, and the reload
itself undoable — `u` after an `:e!` you regret gets your work back.

### Syntax and semantics

Tree-sitter for highlighting, folds, text objects and structural selection; LSP
for everything semantic; DAP for the debug rail. Both language servers and
debug adapters run out of process, so a crash costs a squiggle rather than the
editor. Grammars load as wasm through the same sandbox as plugins, which means a
plugin can add a language without a native build step.

**Some languages are two queries.** TypeScript was the first: its own
`highlights.scm` covers only what TypeScript adds to JavaScript, so used alone
every `const` in a `.ts` file comes out plain. C++ is the same shape and an
easier one to get wrong — its query is seventy lines about templates,
namespaces and `auto`, with no rule for comments, none for numbers, and for
strings only `raw_string_literal`. A `.cpp` file highlighted by C++'s query
alone gets its templates coloured and every comment and ordinary string left
grey. Because the C++ *grammar* is a superset of C's, C's query matches against
it, and the two are simply joined in that order. The `queries` field is a list
for exactly this.

Which grammar claims `.h` has to be decided rather than inferred, since a
header does not say which language it is. It goes to C: C's query against a C++
header still colours the comments, strings and types, where C++'s against a C
header is the worse of the two trades. `.s` and `.S` both arrive lower-cased
and land on the same assembly grammar — the difference between them is whether
the preprocessor runs, not what the file is.

**Some languages are two grammars.** Markdown is the first: one grammar for the
block structure — headings, fences, lists, quotes — and a second for what is
inside a paragraph, where emphasis, links and code spans live. The block
grammar marks those stretches as `inline` nodes and stops, so a single pass
colours the headings and leaves the prose plain.

The second pass is one parse, not one per paragraph: tree-sitter's
`included_ranges` takes all the ranges at once and returns a tree covering
them, which also keeps the offsets absolute so the captures need no rebasing.
It is rebuilt whole rather than incrementally, because the ranges themselves
move whenever a paragraph does and a tree edited against ranges that have
shifted colours the file wrong rather than failing — the worse of the two. Its
captures join the first grammar's in one list and sort as the narrower spans
they are, so a code span inside a paragraph is code rather than paragraph.

Markup needs a vocabulary code does not, and two of its entries are not colours
at all: **emphasis is a slope and strong is a weight**, which is how the text
itself says them. Painting those as two more colours would be a legend to learn
instead of a thing you can already read. It is also the one capture family
where the *tail* carries the meaning — `text` alone is prose — so `text.title`
and `markup.heading` are both read for the same reason the two spellings exist.

### Plugins

Wasmtime, Component Model, WIT-defined interfaces. Two things configured on day
one, not later — both are guarantees rather than features, and a guarantee
retrofitted is a guarantee that was false in between:

- **`Config::epoch_interruption`** with a per-call deadline. Without it the
  "a plugin can't hog the thread" guarantee is false — an infinite loop in wasm
  spins forever by default. Epochs rather than fuel: fuel counts instructions,
  and two plugins hanging the editor for the same wall-clock time would get
  very different treatment. One ticker thread serves the whole engine, and
  every store the host hands out already carries a deadline, so there is no
  path that runs plugin code off the clock.
- **A coarse API.** Host calls are not free, so there is no per-keystroke hook.
  Plugins get grammars, themes, LSP adapters, commands, and mode contributions.
  The WIT world's import side is three functions — read the buffer, where is
  the cursor, where is the project — because every one of them is a permission
  granted to every plugin, and the bar for adding one is not "a plugin might
  want it" but "a plugin cannot do its job without it".

Two orderings follow. **The manifest is read before the wasm is**, so a plugin
that only ships a theme never has its code run and nothing is executed at
startup merely for being installed. And **plugins are per user, not per
project**: they sit beside `session.json`, because a repository that could drop
code into the editor by existing would be a repository you could not safely
open. The session loads them once and shares one set with every workspace.

A command's result is applied as an ordinary edit, so `u` takes it back — the
rule everything that writes to a buffer here follows.

### Agent runtime

No official Anthropic Rust SDK exists, so this is raw HTTP against
`POST https://api.anthropic.com/v1/messages` with `x-api-key` and
`anthropic-version: 2023-06-01`. Decisions that are settled:

| | |
|---|---|
| Models | `claude-opus-5` for agent work and chat; `claude-haiku-4-5` for completion and the semantic index |
| Thinking | `thinking: {"type": "adaptive", "display": "summarized"}` — **not** `budget_tokens`, which is rejected with a 400 on Opus 5. `display` must be set explicitly: the default omits the text, and the dock's "thought 6s" panel needs it |
| Effort | `output_config: {"effort": "xhigh"}` for agentic coding; `"low"` for completion and subagents |
| Transport | Always `"stream": true`. Output caps are large and non-streaming requests hit HTTP timeouts |
| Auth | `ANTHROPIC_API_KEY`, else `ANTHROPIC_AUTH_TOKEN`, else the OAuth profile on disk. Never prompt for a key without checking all three |

Four API behaviours the runtime must get right, because each has a silent
failure mode:

- **Parallel tool calls come back in one assistant message, and every
  `tool_result` must go back in a *single* user message.** Splitting them across
  messages trains the model out of parallel calls. A failed tool returns
  `tool_result` with `is_error: true` — never a dropped block.
- **Prompt caching is prefix-matched** in the order `tools` → `system` →
  `messages`. The repo map and system prompt sit behind a `cache_control`
  breakpoint; anything volatile goes after it. Assert on
  `usage.cache_read_input_tokens` in tests — a silent invalidator (a timestamp,
  an unsorted tool list) shows up as a cost regression, not an error.
- **Mid-conversation system messages** — appending `{"role": "system", ...}` to
  `messages` rather than editing top-level `system` — is supported on Opus 5 and
  is exactly right for Proton: "the user switched to Data mode", "the trace
  finished", without invalidating the cached prefix.
- **Context editing** (beta `context-management-2025-06-27`,
  `clear_tool_uses_20250919`) matters more here than in most apps. A trace JSON
  or a `grep` over a large repo is a very large tool result, and a long agent
  task will otherwise fill the window with results it has already summarised.

Long tasks in the queue carry a task budget (beta `task-budgets-2026-03-13`) so
the agent paces itself instead of being cut off. And `stop_reason` is checked
before `content` on every response — `"refusal"` is an HTTP 200.

Everything the agent proposes lands in the **shadow worktree** (`proton-vcs`),
surfaced as per-hunk diffs. Nothing touches the user's tree until accepted.

### Research rounds

Research drives Claude Code, Codex and Antigravity through `proton-claude`'s
conversations, unattended: each agent task gets a fresh
conversation that reads and searches and changes nothing, brings none of the
person's MCP servers, plugins or global instructions, is never asked
anything, and gives its final answer whole, held to a JSON Schema. Nothing
carries from one task to the next but the files, because a reviewer must not
remember writing what it reviews.

Investigations are kept in the project's `research/` — or, where the
person's own settings (`~/.config/proton/settings.json`, beside the session)
name a `research.folder`, in that folder, each a folder in it, whichever
project is open. Then one `Research` is the session's, shared by every
workspace rather than one per project over its `research/`, and it is polled
and stopped by the session, once: the session polls every workspace but keeps
only the one on screen's events, so a shared Research polled by each would
hand what a round said to a project in the background, to be dropped with
the rest of that project's events. Only folders with an `investigation.json`
are investigations, so one kept beside the person's own work leaves that
alone. The setting is read when the shell restores the session, so no test
reads the developer's; `Command::ResearchFolder`, from Change under the
Research list, is the session's to answer — it refuses while a round or a run
is going, makes the folder only when its parent is there, writes the setting,
and moves every project at once.

The split is three ways. `proton-research::supervisor` decides what each
task is and what an answer means, and starts nothing. `proton-core`'s
`research::driver` starts each task through an `AgentFactory` — the engines,
or in tests scripted conversations over a real `Fold` — watches it on the
core's heartbeat, and writes what it answers into the investigation's
folder. Time is passed in, never read, so a round with a timeout, a limit
that resets in ten minutes and a relaunch is played in a test in a third of
a second.

| | |
|---|---|
| A call | its own conversation, working in `research/<slug>/`, told its role and its rules with the task itself rather than by any instructions file |
| A question | refused: nobody is there to answer it |
| An answer that will not read | asked for once more, in the same conversation |
| No answer, or none in 15 minutes | stopped, and tried once more — said in a notice, and counted in the round's report, which gives each task's calls |
| Signed out | the seat sits the rest of the round out; its reviews and judging go to another family |
| At its limit | the seat waits until the limit resets |
| A judge running its own family's model | stopped as it says so; the task goes elsewhere, or the round goes on without it |
| A seat that does not say its model | Antigravity runs Gemini, Claude and GPT models alike and no longer reports which, so a seat whose investigation names none is given Antigravity's newest Gemini by name: its family is known, not assumed |
| Records | `tasks/T0001.json` (what it was asked and answered, by which model), `.investigation/transcripts/`, `.investigation/round.json` |
| Two Protons | the first holds `.investigation/lock` with a heartbeat; the other is read only until it goes quiet |
| Proton closed mid-round | back paused: spending the scientist's limits is theirs to start |

One round runs at a time in a project.

A round of an investigation that uses the literature searches it first, for
no agent call: once the plan is framed — on the plan's own `queries`, or the
last overview's, or, with neither, the title's words — the driver hands the
supervisor's `LiteratureSearch` to its source and the hypotheses wait for
the answer, two minutes at most (`wait_s`). The best of what each query
found that the investigation does not already have, by any of a paper's
names, is numbered on (`P004`…) and written as an `Effect::Paper`, at most six
a round; open PDFs are sent for. Every task is then shown the literature —
a line a paper, who and when, its DOI or arXiv id, whether its full text is
beside it, and the start of its abstract, within 5 kB — and each reviewer the
papers closest to each hypothesis (TF-IDF, as the tournament pairs them), to
judge novelty by. A hypothesis names the papers it rests on in `references`;
the overview names next round's `queries`. A search that fails is said and
the round goes on; one past its deadline is not waited for, and what it finds
still reaches the reviews; one still out when the round ends is let go.
`used` counts agent calls only, and a round's report ends with what was
searched for and added.

**Forking.** `Store::fork` copies an investigation under a new title, from
the end of a round or as it stands. A round's report is written with
`rounds/round-002.json` beside it (`RoundEnd`): what each round rewrites —
the plan, the overview, every hypothesis's status — as it then was, and how
long `VERDICT.md`, `SANDBOX_LOG.md`, `feedback.md` and `tournament.jsonl` had
grown, since they are only added to. A fork after round N has the hypotheses,
reviews, matches, experiments, proofs, rounds and task records of rounds up
to N, those files cut back to their lengths then, the runs started before the
round ended, and the papers found by its rounds or added before it ended,
PDFs and all. An experiment whose run came later is proposed again, as it was.
A fork as it stands has everything, and waits for a round under way. Neither
takes `.investigation/` — the lock, the round's state, transcripts, caches —
so the fork's next round is N + 1, made afresh, and its numbers carry on from
the copy's. Files are copied with `std::fs::copy`, which on APFS clones them;
links are not followed. `investigation.json` keeps `forked: {from, title,
round, at}`, the brief gains a `## Forked` section saying so, and the
direction given is the scientist's first feedback. Rounds that ended before
this kept nothing to fork from, so only forking as it stands is offered for
them. `Command::ResearchFork` makes and opens it; the board sends `forked`
and `roundEnds`, and the list each one's `forkedFrom`.

`tests/live_round.rs` plays one round on the real engines, signed in as
whoever runs it; it is ignored unless asked for, since it spends their
limits, and it is run from a clean environment, as Proton is started, because
a shell inside another agent's session carries that session's variables and
the engines read them. The first, on a physics question, took 31 minutes and
fifteen calls of its twenty: Claude's calls took under a minute,
Antigravity's one to five, Codex's up to ten. It is what found Antigravity's
missing model and a retry the report did not account for.

### The Lab

`proton-lab` makes the Lab's Python (a conda environment of the scientific
stack, on two say-sos: a dry-run plan, then exactly that plan, pinned) and
runs experiments in it. Where there is no conda it makes a virtual environment
of a Python 3.9 or newer that is here — Homebrew's, python.org's, pyenv's, one
on the PATH, macOS's own only when the developer tools are installed, since
otherwise it offers to install them — with the same two say-sos. Planning makes
the venv empty and asks its pip, brought up to 22.2 if it is older, for
`install --dry-run --report`, with each package's size from PyPI; making it
installs exactly that list, pinned and `--no-deps`, and `pip freeze` is its
lock. A venv planned and not wanted is removed, so nothing is left behind but
what the scientist made. What makes it — conda, or a Python — is looked for
the first time a window draws the Lab, on a thread, because asking a Python
its version means starting it; a Proton that never shows the Lab never asks.
A run is `research/<slug>/runs/R005/`: the approved
code as `main.py`, a helper it can import, and `run.json` — the code and the
environment's lock by SHA-256, the Python, the sandbox, the limits, who
approved it and when, and how it ended. Its output goes to files, not pipes;
it is a process group of its own, stopped as one on a timeout, a Stop or
Proton closing; it has an environment of its own (none of the person's
variables, so none of their keys) and limits on CPU time and file size. Four
run at once at most. Quitting stops every run and says so in its record, its
experiment and the sandbox log; a run a crashed Proton left marked running is
settled the next time its investigation is drawn — stopped if its process is
still there working in the run's folder (a pid something else has since is
left alone), and closed out either way.

| | |
|---|---|
| macOS | Seatbelt via `sandbox-exec`: network denied; writes only in the run's folder; the home folder unreadable but for that folder and the Lab's Python (or Proton's Lean) — the folders on the way to them may be looked at, not into, since macOS's `realpath(3)` asks each for its attributes |
| Linux | bubblewrap: the root read only, the home folder replaced by an empty one, the Lab's Python and the run's folder bound back, no network |
| Either | tried on the machine before it is trusted: a probe in a stand-in home must write its folder and be refused writing and reading beside it |
| Neither | runs only when the scientist says so, knowing the code can do what they can; the record says it ran unsandboxed |

When a run ends its experiment is marked done or failed, and what it showed
— how it ended, its numbers, what it saved, the end of its output — is
appended to the investigation's `SANDBOX_LOG.md` under a heading naming it.
Then the round has it reviewed: a simulation review, by a seat of neither the
proposer's model family nor the hypothesis author's, which reads that entry
and the code, says whether the run supports the hypothesis, and may propose
the next experiment — at once if a round is running, else first thing in the
next. An investigation can let what its agents propose run without asking;
only ever in a sandbox, two at a time, and the record says the policy
approved it.

### The literature

An investigation keeps its papers in `literature/`: `P003.md` says what the
sources said about one — its names (DOI, arXiv id, Semantic Scholar's and
OpenAlex's ids), its abstract, how often it is cited, who added it and which
queries found it — and `P003.pdf` beside it is its open copy, kept out of Git
by the investigation's `.gitignore` (megabytes each, and other people's
copyright). `proton-research::cite` is the one definition of a paper's
identity, so a paper is never added twice under two of its names.

`proton-literature` does the asking: arXiv's Atom API, Semantic Scholar,
OpenAlex, Crossref for what a DOI names, and the DOI system itself for one
Crossref never heard of — a dataset's DOI is real too. Each host is asked no
oftener than it asks to be, by gates the whole process shares (arXiv once every
three seconds, its PDFs and its API one arXiv; Semantic Scholar once a second
with a key, gentler without); a busy answer is asked again after its
Retry-After, three times at most. Every job — a search, a lookup, a set of
downloads — runs on a worker the core polls, each source on a thread of its
own; dropped, it is cancelled.

The keys some sources give more to (`SEMANTIC_SCHOLAR_API_KEY`,
`OPENALEX_API_KEY`, and `CROSSREF_MAILTO` for Crossref's polite pool) are the
person's own environment's: Proton's, then the login shell's, which an app
opened from the Dock does not inherit — asked the first time a worker needs
them, looked for again a minute later if one was missing, held in memory only.
The page is only ever told whether each was found. A source that says it is
out of calls is left alone — OpenAlex without a key until midnight UTC, which
is when its hundred a day come back — and the tab says so.

A PDF is kept only when it is one: the size it states, then the bytes it sends,
within the investigation's limit (40 MB); `%PDF-` at its start (a publisher's
sign-in page is not a paper, and arXiv's copy is tried next); written to a
`.partial` and renamed whole, never over a copy already there — one the
scientist put there is theirs — and never into a folder that has gone. A
paywalled paper keeps its abstract, and says where a copy would go.

The agents read the papers' full text. Claude Code and Codex are given
`proton-mcp --literature <investigation>` for each call once there are papers:
its `search_literature` searches every PDF page by page and every abstract —
the shelf reads past the investigation's `.gitignore`, which keeps the PDFs out
of Git and which the walker would otherwise honour from the folder above — and
cites from the investigation's folder, `[[literature/P003.pdf § Results p. 4]]`,
the path a call working there knows the paper by. Its tools are marked
read-only and allowed up front. Claude Code is started `--restricted` for these
calls rather than `--safe-mode`, which drops even the servers `--mcp-config`
names (Claude Code 2.1.288): restricted ignores the person's settings, and so
their hooks and plugins; `--strict-mcp-config` their servers;
`--disable-slash-commands` their skills; and `CLAUDE_CODE_DISABLE_CLAUDE_MDS`
and `CLAUDE_CODE_DISABLE_AUTO_MEMORY` their CLAUDE.md and memory. Codex is given
the server in its thread's config, after the overrides that switch the
person's off, and calls it unasked with approval `never`. Antigravity takes no
server, so it — and everyone, where no `proton-mcp` is found beside Proton — is
given the passages most about its task in its prompt instead: the round's
queries for proposing, the hypotheses in front of it for judging. Those are
read at once from the abstracts and from PDF text already extracted; a worker
extracts each new PDF into `.investigation/pdf-text/`, so no call waits on it.
A citation of `literature/…` anywhere on the board opens the paper's PDF at the
page in the document viewer, or the paper itself.

What the team cites is checked. After anything is written, the DOIs and
arXiv ids in the hypotheses' references and the reviews' citations that the
investigation does not have as papers are asked of what issues them — arXiv's
`id_list`, twenty at a time; Crossref for a DOI, and the DOI system for one
Crossref has no record of, since a dataset's DOI is real too — with no other
source. Each name is kept in `.investigation/citations.json` as verified,
unverified or unknown: unreachable is never taken for made up, and is asked
again after an hour; an answer either way is never asked again. A name
nothing could be found for is said in a notice and told to every task after,
with where it was cited, so none is cited again and what rests on it is
weighed for it; a hypothesis's page marks each name it and its reviews cite.

Three kinds of live check are run by hand, since they reach the network or
spend a plan. `proton-literature`'s `tests/live.rs` asks each source without a
key (and records what they answer, trimmed, as the fixtures the offline tests
read); Semantic Scholar's pool for callers without a key answered 429 more
often than not on 2026-10-06. `proton-claude`'s `tests/literature_live.rs`
runs Claude Code and Codex unattended, as a round does, with the literature
server, and checks each searched a paper through it and cited it — it is what
found `--safe-mode` dropping the server. And `tests/live_round.rs` plays a
round with the literature on, unless told otherwise.

An investigation made before there was a literature has it off, so none starts
telling third parties its topic on its own; a new one has it on unless the
scientist says otherwise. Off, nothing is asked of anybody: the scientist's
searches and lookups are refused with the reason, and whatever was being asked
is let go. Research outside the app (tests) asks nobody: its source is
`Offline`, or a script.

### Lean

A mathematics investigation's leading hypothesis is stated in Lean 4 with
Mathlib each round, and proved — or its negation — where it can be. Lean and
Mathlib are Proton's own, one for the machine in `~/.proton/lean`, made as the
Lab's Python is, on two say-sos (`proton-lab::lean`). Planning asks GitHub
for Lean's newest stable releases and takes the newest one Mathlib has tagged
(Mathlib's `lean-toolchain` at that tag must name it), and elan's latest; the
plan says what each download costs — elan's and the toolchain's sizes exactly,
Mathlib's sources and prebuilt library as estimates, since only its cache tool
knows them — what it takes on disk at its fullest and keeps, and whether there
is room. Making it downloads elan with curl and checks it against the SHA-256
GitHub publishes for it, installs it with no default toolchain, installs the
plan's Lean with it (elan unpacks the `.tar.zst`, which macOS's `tar` cannot
read, and checks it against no digest, so the plan does not call it verified),
writes a Lake project requiring Mathlib at its tag, runs `lake update` (with
`MATHLIB_NO_CACHE_ON_UPDATE`, so the library is a step of its own) and `lake exe
cache get`, asks Lake where Lean finds Mathlib, and checks a file importing
Mathlib in the sandbox checks run in. Then it deletes what was downloaded only
to be unpacked, measures itself, and writes `ready.json` last: a Lean is ready
only when that marker says so and everything it names is there, inside
`~/.proton/lean`, and nothing is started to find out. Every step is a process
group of its own with none of the person's environment but what a download
needs and a `HOME` inside `~/.proton/lean`, so nothing is written to `~/.elan`
or `~/.cache` and no `.curlrc` or `.gitconfig` changes what is made. A making
cancelled or failed is taken away whole; one cut short by Proton closing is
stopped and taken away the next time Proton looks (`making.json`, as a run's
record is reaped); removing takes the marker first, and waits for a check.

The round's Proof step (`proton-research::supervisor`) comes once the leader
is verified, in mathematics, with Lean made, unless Lean has already settled
that hypothesis or found no statement of it. A seat of another model family
than its author — the Lab Engineer first — is told to state it as the theorem
Proton names after it, `H004`, at the top level, importing Mathlib alone, and
to prove it or its negation; it may read Mathlib's sources (Claude Code is
given the folder with `--add-dir`; Codex's sandbox reads it already) but cannot
run Lean. Its answer, `{label, kind: proof | disproof | none, lean, notes}`, is
vetted (`proton-research::lean`): the file is read as Lean reads it, comments
and strings skipped, and refused if it imports anything but Mathlib, uses a
word that makes Lean run code, take something on trust or skip its checks
(`#eval`, `axiom`, `elab` and `macro` and `syntax`, `run_cmd`, `native_decide`,
`implemented_by`, simprocs, Lean's own namespaces and `IO`, `debug.` options),
leaves a namespace open, or lacks the theorem; over 100 KB it is refused, not
cut. The file Lean is given ends with Proton's two questions, `#check @H004`
and `#print axioms H004`, on lines it records, and their answers are read from
those lines alone, so nothing the file prints can pass for them. A proof counts
only on `propext`, `Classical.choice` and `Quot.sound`; with `sorry` in it the
statement stands and the proof is open.

The supervisor returns `Answered::Check`; the driver makes the attempt's
folder, `proofs/H004/attempt-N/`, keeps the call's conversation open, and
checks the file through a `Checker` (`research::lean`): `Live` runs the
toolchain's own `lean --json --threads=N` with `LEAN_PATH`, no elan and no
Lake, in the Lab's sandbox with `~/.proton/lean` its one other readable tree,
a process group with limits on CPU time (its timeout times its threads, and
30 s) and file size, one check at a time on the machine; `Offline` never
starts one; `Scripted` answers as a test says. While Lean has a proof only the
check is looked at, so an answer is never read twice. What Lean said goes to
`lean_checked`: a proof or a disproof ends the call; Lean's errors (6 KB at
most, with their lines), a `sorry`, or a refused file go back to the prover
in the same conversation, with the call's whole time again, while the proof's
calls (`LeanPolicy.calls`, three a round, however many seats make them) and the
round can spare one, keeping back what the evolution and the overview still
need. A statement Lean accepted with `sorry` is kept meanwhile, and written as
an open proof when the calls run out. What it comes to is `proofs/H004.lean`
beside `proofs/H004.json`, an entry in `VERDICT.md`, and — for a proof or a
disproof only — a formal review, Strong or Flawed, which the leader's score
sees; a disproof retires nothing by itself. The overview waits for the proof;
the evolution does not. Judges are told what Lean has settled; the evolution
and the overview what it has said. A check is stopped with its round, and one
a crashed Proton left running is settled when the investigation is opened; a
round closed mid-check asks again with the file that was being checked.

`LeanPolicy` (on, three calls, 300 s a check) is on for investigations made
before there was Lean too: nothing leaves the machine. Checks run without
asking only in a sandbox; on a machine with none, a round says Lean is not
ready. The tests read hand-written `lean --json` output
(`crates/proton-research/fixtures/lean`), play rounds with a scripted prover
and Lean, and run a whole round through `Live` with a stand-in `lean` in a
stand-in home, in the sandbox; a real Lean is the scientist's to make.

### Modes that moved

Eight modes that were here moved to apps of their own in October 2026: LaTeX
and the Reader to Quark, with `proton-tex`, `proton-pdf` and the Lab's TeX, OCR
and covers; and Design, Model, Game, Video, Canvas and Diffuse to an app of
their own, with `proton-design`, `proton-scene`, `proton-mesh`, `proton-game`
and its runtime, `proton-raster` and its runtime, `proton-diffuse` and
`proton-video`, the window's WebAssembly build, and the shell's mesh, picture,
pen and keychain code. What stays is what the five modes then left used: Codex edits
`.tex` and `.bib` with texlab from `PATH`, the document viewer draws PDFs for
Codex and Research, and the Lab's 3D scenes are drawn by three.js. A
saved session that names their slugs still loads: they are passed over, and a
project only they listed is closed rather than given to Codex.

Chat and then Notebook went to Fairy in November 2026. Chat took the Council
window with it; Notebook took `proton-study`, `proton-voice`, the core's
`study` and `engines` modules, `proton-mcp --notebook`, the shell's `study://`
and `artifact://` schemes, and the practice problems it set in Circuit and
Logic. A saved session that names `chat` or `study` loads as above. The keys
moved up to close the gap: Excel is ⌃4, Circuit ⌃5, Logic ⌃6 and CAD ⌃7.

### Trace

Code Visualizer, sidecar first and native second — [`CODE-VISUALIZER.md`](CODE-VISUALIZER.md)
has the full plan. `proton-trace` owns process supervision, the port handshake,
and mapping `GET /api/toolchain` onto the explorer's Targets list.

---

## 4. The mode contract

A mode is a plugin with four slots and a verb table:

```rust
trait Mode {
    fn id(&self) -> ModeId;
    fn context_pane(&self) -> View;      // left
    fn stage(&self) -> View;             // centre
    fn dock(&self) -> View;              // right
    fn verbs(&self) -> &VerbTable;       // what dd, yy, / mean here
    fn document(&self) -> DocumentKind;  // how it persists
}
```

That is the whole contract. Nothing about the modes that ship is privileged,
and Research was an extension rather than a fork.

**It is built, in `proton-core::contract`, and it is a struct rather than a
trait.** Every answer above is data: none of it needs to run, and a mode that
is a value can be compared, sent over the bus and written down in a test. The
sketch above described the same thing in a language where the answers might
have been computed.

A `Slot` says *what* belongs in a position — the file tree, the editor, a
notebook's cells — and never how to draw it, because the core still names no
widget. A closed enum rather than a string, so a front end that has not
implemented a slot fails to compile instead of rendering a blank rectangle.

The verb table is a question — `supports(action)` — because that is the only
form of it anything has ever needed, and `ModeId::supports` now asks the
contract rather than answering separately, so there is one table and not two
that can drift.

The test that keeps this honest is that **Data's layout is not Codex's**:
different stage, different dock, different document. Data mode was written
after the contract for exactly this reason — a contract that only ever
described text editors would not have been a contract, it would have been the
editor with extra words. If that test ever passes trivially, it has stopped
meaning anything.

`Event::Layout` goes with every mode change and with `Sync`. A mode switch that
only moved a highlight on the rail would be a theme rather than a mode.

**Every mode's document is a text file that diffs in Git.** This is the load-
bearing rule of the entire project:

| Mode | Document |
|---|---|
| Codex | Source files |
| Data | `.ipynb` + `.sql` |
| Research | `research/<slug>/` — Markdown with TOML front matter, `tournament.jsonl`; papers as `literature/P003.md`, their PDFs beside them and out of Git |

It is not tidiness. It is what lets the agent edit a notebook or an
investigation by writing a patch instead of driving a GUI, and what makes any of
it reviewable.

---

## 5. The shell

The prototype now lives in `ui/` and is the real chrome; its stylesheet is
already the design system. Three notes for the build:

- **Splits are core state now**, not cloned markup. See § 3 → Splits.
- **The mode switcher is a vertical rail**, outboard of the panel rail — see the
  note in `DESIGN.md` § The shell. The prototype still shows it in the titlebar;
  build it vertical.
- **The editor pane is virtualised** from day one: visible window plus one
  screen of overscan, syntax highlighting off the main thread, deltas over the
  bridge. This is the pane that decides whether the webview bet holds.

  The scrollbar is the *file's*, not the window's: a spacer of the document's
  full height holds a loaded window translated to its place inside. Scrolling
  proposes a `first_line`; everything that moves the window for its own reasons
  — `G`, a grep hit, the cursor leaving the screen — comes back as a viewport
  event and the pane follows it.

  `first_line` therefore has to mean exactly one thing, and it means the line
  at the top of the pane. Overscan is extra rows *below* rather than a lower
  starting line: offsetting the request would leave the page and the core
  disagreeing by the overscan, and the correction would scroll the user back a
  little on every frame they scrolled.

- **The session remembers the layout, not just the projects.** Which files were
  open, which pane each was in, how the panes were arranged, and where the
  cursor was. Stored per workspace and keyed by root, because the restored
  project list has already dropped whatever has gone and the indices no longer
  line up with what was written.

  Added *without* bumping the session version, and that is the point: the
  version check discards a file it does not recognise, so bumping it would have
  thrown away the project list — the part that already worked — to gain the
  part that did not. A session file from before layouts existed still opens its
  projects and simply has no layouts.

  Everything is clamped on the way back in, because a session describes a world
  that has moved on. Files get deleted, renamed, or live on a drive that is not
  plugged in; a cursor offset saved last week can point past the end of a file
  somebody has since edited, and a cursor outside its buffer panics three
  operations later. Every file that still opens is opened, the rest are passed
  over, and the panes are rebuilt around what survived.

  Saving is triggered by a **fingerprint** rather than a list of commands:
  a handful of integers — buffer count, focused pane, orientation, and which
  buffer each pane shows — compared before and after every command. Building a
  `Layout` allocates a path per buffer, which is not something to do after each
  keystroke to find out whether anything moved. The fingerprint is deliberately
  blind to the cursor, so a cursor move does not cost a file write; the cursor
  rides along with the next structural change. A command added next year that
  opens a file in a pane is covered by this and would not have been covered by
  an enumeration.

- **The window remembers where it was**, and this is the one piece of state
  that deliberately does *not* go through the command bus. `proton-core` names
  no widget and has no idea the window has a size, so geometry is the shell's
  alone: read once before the window is shown, written back as it moves. The
  size in `tauri.conf.json` is the first-run default, not the size.

  Size is stored **logical**, position **physical**. That looks inconsistent
  and is not: a logical size reopens the window at the same apparent size on a
  monitor with a different scale factor, while monitor layout is expressed in
  physical coordinates, so a physical position is the one that can be checked
  against the screens that actually exist.

  **Tauri's accessors are not symmetric, and getting that wrong is invisible
  once.** `set_size` sets the *inner* size, so it pairs with `inner_size`;
  `set_position` and `outer_position` are both the outer one, so those match.
  Saving the outer size and restoring it through `set_size` adds the window
  frame back at every launch — measured here at 16×9, so the window crept from
  1240 to 1256 to 1272 across three runs. One open-and-close cycle looks
  correct, because one frame's worth of growth reads as a rounding error; it
  takes two to see it. That the position restored *exactly* while the size
  drifted is the tell, and the reason to suspect the pairing rather than the
  scale-factor arithmetic.

  Which it is checked against, because the interesting failure is not the
  size. Close on a second monitor, unplug it, reopen: the stored position is
  still perfectly valid and describes a place that is no longer on any screen,
  so the window opens invisibly and the app looks like it failed to start. The
  check is a point a little inside the top-left corner rather than the corner
  itself — what has to be reachable is the strip you can grab to drag it back.
  That decision is a function over plain rectangles rather than over a window,
  because a monitor cannot be unplugged from inside a test.

  A maximized window stores the size it will *return* to, or unmaximizing
  after a restart would do nothing. That needs a size underneath it, so a first
  run seeds one from the window it was just given — otherwise somebody who
  opens Proton and immediately maximizes it saves nothing at all. A minimized
  window stores nothing, since its reported size would reopen the app as a
  sliver.

  **Writes are debounced rather than throttled, and that is what makes
  maximizing work.** The `Resized` event that arrives when a window is
  maximized arrives while `is_maximized()` still answers *false* — so saving
  from inside the event records the screen-filling size as an ordinary one and
  loses the size to return to. A throttle does not help, because the first
  event of a maximize passes one. The event only notes that the window moved;
  the write happens once it has held still, by which point the window answers
  honestly. Closing writes directly, since by then it has long since settled.
  The same mechanism keeps a corner-drag from writing a file per frame, so it
  replaces the throttle rather than joining it.

- **The mouse goes through the core.** A click is `PlaceCursor` and a drag is
  `SelectTo`, because a motion cannot express either — `Move` is always
  relative to where the cursor already is, and a mouse knows exactly where it
  wants to be. A drag leaves a real visual selection rather than one the modal
  engine cannot see, so `y` and `d` mean what you expect afterwards. The
  browser's own selection is suppressed: two selections disagreeing about what
  is selected is worse than either.

  The hit test asks the browser where the click landed and counts characters to
  that point, which is correct however the highlighter cut the row up.
- **Themes stay CSS custom properties.** Four palettes, sixteen tokens each,
  switched by a `data-theme` attribute. Plugin-contributed themes are just more
  token sets.

---

## 6. Process model

| Process | Why separate |
|---|---|
| Proton (shell + core) | — |
| Language servers, debug adapters | Third-party, crash-prone, per-language |
| Code Visualizer sidecar | Python + gdb; a different runtime entirely |
| Plugin wasm instances | Sandboxed and interruptible, in-process but isolated |
| ffmpeg (Video) | Long-running decode, kill-able |

The rule: anything third-party, anything that can hang, and anything with a
foreign runtime goes out of process.

---

## 7. Build order

Each milestone has an exit criterion. Do not start the next one until it is met.

**M0 — Skeleton. ✅ done.** Cargo workspace, Tauri v2 shell, the `Command`/`Event`
bridge with `ts-rs` generation, the prototype moved to `ui/` and served as the
real front end. The modal engine landed here rather than in M1 because the
exit criterion needs something real to round-trip: counts, operators, doubled
operators, registers, visual mode, the leader and the command line all work.
*Exit met: `[bridge] Sync -> 1 event(s): [State { mode: Codex, vim_mode: Normal }]`
in the running app, plus 23 headless tests.*

**M0.5 — Surface. ✅ done.** The mode switcher moved from the titlebar to a
vertical rail outboard of the panel rail, and the window ground now comes from
the compositor rather than from the page. `window-vibrancy` asks DWM for Mica
(`DWMWA_SYSTEMBACKDROP_TYPE`), the shell reports what was *actually* applied,
and the front end paints its own ground whenever the answer is `solid` —
transparency effects off, battery saver, Windows 10, Linux. Switchable at
runtime from Settings → Appearance.

Surfaces are veiled on a ladder — rails 34%, window 40%, sidebars 46%, stage
64% — scaled together by a three-step Translucency setting, so the stage is
frosted rather than opaque but always the most solid thing on screen. Panes are
resizable: each hairline is a real 1px grid track that takes the drag, writes
one custom property, and persists per mode. The titlebar is gone — window
controls, identity, modes and project actions share one vertical rail, and the
stage splits with `:vsplit` / `:split`.

The core grew one event doing it. `Outcome::Cmdline` and `Outcome::Execute` are
now distinct, because a consumer that cannot tell a keystroke from a submission
runs `:`, `:v`, `:vs` and `:vsp` on the way to `:vsplit`.
*Exit met: `[shell] backdrop: mica` on this machine.*

**M1 — The editor. ✅ done.** `proton-text` ships: rope buffers behind a
`TextStore` surface, an undo *tree*, word motions and text objects. The modal
engine gained text objects, `gg`, big-word motions, `.` and macros, and actions
now become real edits through `proton-core::Editor`. The front end renders from
`Viewport`/`Cursor` events and asks for only the lines it can show, which is the
virtualisation.

`proton-syntax` ships highlighting. A grammar table, the grammar's own
`highlights.scm`, and twelve scopes — because a reader can tell about a dozen
colours apart and a grammar will happily name four hundred captures. Anything
unrecognised is left uncoloured rather than given a thirteenth colour meaning
"we did not know".

Two things about it were decided by measurement rather than by taste, and the
numbers are in `crates/proton-syntax/tests/latency.rs`:

**The query runs over the visible window only.** A ten-thousand-line file has
ten thousand lines of syntax and sixty on screen. Scoped, a redraw costs
0.25 ms and *does not grow with the file* — which is the property that matters,
because it means a large file scrolls like a small one.

**Parsing is incremental, and that was not a guess.** A full re-parse measured
2 ms on a 600-line file and **36 ms on a ten-thousand-line one** — a visible
stutter on every keystroke, in files that are not rare. Journalling changes in
`proton-text` and feeding them to `Tree::edit` first brings that to **0.17 ms,
flat across all three sizes**. The journal is bounded and answers "start over"
when it runs out, because a parse tree that disagrees with the text by one byte
does not fail loudly — it quietly miscolours everything after the disagreement.
`tests/highlight.rs` compares an incrementally-updated tree against a fresh one
after every edit, which is the only assertion that would catch that.

Compiled queries are cached per language: building Rust's measured 14 ms, and
paying it per file opened is a pause you feel in a session that opens a lot of
them.

**Marks** live on the buffer rather than on the editor, because a mark is a
place in *that* text and has to move when the text around it does. A mark set
at an offset and then left alone through an hour of editing above it is not at
that offset any more, and one that still claims to be is worse than no mark —
it takes you somewhere confidently wrong. So every edit shifts them, and a mark
inside deleted text collapses to where that text began rather than dangling.

`m`, `` ` `` and `'` all claim the next key as a name, in normal mode and
operator-pending alike. That is what makes `d'a` work, and it falls out of
marks being an *object* like any other: the operator machinery needed no
changes. It did surface that linewise motions were not linewise — `dj` took a
rectangle of characters rather than two lines — which is now fixed for `dj`,
`dk` and `d'a` together.

**Visual block** is the one selection that is not a range of offsets. The text
between two columns on different lines is not contiguous, so it gets its own
shape in `block.rs` and the operators ask for a list of ranges. `⌃v` enters;
`d`, `c`, `y` and `x` take the rectangle; `I` and `A` insert on every line at
once, which is the reason anyone uses the mode. A block yank remembers it was
a block, so putting it back gives a column again rather than a run.

Two things fell out of building it. A line too short to reach the block is left
alone rather than clamped into it — clamping is how you eat the wrong
characters. And `I` and `A` differ on purpose: `A` pads a short line with
spaces to reach the column, because the point is that the result *is* a column;
`I` does not, because a column that line never had is not a place to insert.

Selections are now drawn, which they were not before — the events carried them
from M0 and nothing rendered them. They are painted per character rather than
as a box behind the line, since a box cannot be the shape of a rectangle
selection.

**Measured on this machine** (release, a 660-line file, 60-line viewport):

| | M1, before highlighting | with highlighting |
|---|---|---|
| motion (`j`/`k`) | 0.9 µs | 1.0 µs |
| word motion (`w`) | 3.1 µs | 26 µs |
| insert a character | 18.2 µs | 335 µs |
| undo | 0.6 µs | 6.8 µs |
| full window re-render | 1.0 ms | — |

The second column is an order of magnitude up, and that is the colours: a
change re-parses and re-queries the window before the event is built, and a
scroll re-queries it. Worth it, because the alternative is a frame where the
text is fresh and its colours are stale. At 0.34 ms an insert is **4% of a
120 Hz frame**, so the headroom survives. **The webview bet still holds.**

*Exit: Proton edits its own source, and `2dd`, `ciw`, `.`, `u` and a recorded
macro all behave. Measure input-to-photon here and write the number down.*

**M2 — Telescope + LSP. ✅ done.** Telescope ships: `proton-index` walks the
tree with `ignore` (so `.gitignore` is honoured), scores with `nucleo` in its
path-aware configuration, and backs three pickers — files, live grep and
buffers. They differ only in where their candidates come from, which is why
symbols and references cost no new matching code when they arrived — only a
place to hold rows that a server sends after the picker is already open.

The picker owns the keyboard while it is open and the modal engine is suspended
rather than consulted; leader bindings that name a picker resolve in the core,
because the keymap is core config and a second front end must not reinvent it.
The walk itself runs on a worker and is collected by the heartbeat below, so a
cold index on a large repository never sits in front of a keystroke — a picker
opened before the walk lands says `scanning…` rather than showing an empty list
that is indistinguishable from no matches.

`proton-lsp` is the language-server client: three layers, smallest first.
`transport` frames bytes (`Content-Length`, counted in bytes — the one framing
bug that only appears once someone opens a file with an emoji in it), `protocol`
gives those bytes a shape, and `client` owns a process and turns its replies
into a mailbox. Hand-written rather than imported: nearly every response needs
an `untagged` enum, and each of those subtleties is worth stating explicitly
instead of inheriting — LSP has accumulated older spellings of the same answer
and kept them all legal, so reading one means knowing which shapes it has.

**Nothing in the client blocks.** Requests leave through a writer thread,
because rust-analyzer stops reading its stdin for whole seconds while it
indexes and a full pipe buffer must not be able to wedge a keystroke. Replies
arrive on a reader thread and wait in a mailbox. Every request the *server*
sends gets an answer, including methods we have never heard of — silence there
is a hang, and a hang during startup is indistinguishable from a broken editor.

That leaves the core's `apply` total and synchronous, which was the constraint
worth protecting. The cost is one command: `Command::Poll`, sent by a 50 ms
heartbeat in the shell. The core has no clock and no threads, so something
outside has to ask on its behalf; an idle poll is a `try_recv` on a handful of
channels and produces no events, so it never redraws anything.

Two coordinate systems meet here and it is worth being exact about which is
which. **LSP counts columns in UTF-16 code units; a rope counts characters.**
They agree for ASCII and diverge on the first emoji, which is the whole reason
`proton-text` carries a UTF-16 index. The conversion happens once, in
`proton-core::lsp`, against the buffer that knows — not rediscovered by each
caller. Windows adds a second spelling problem: `canonicalize` returns
`\\?\C:\…`, which no server accepts and which never matches the diagnostics
that come back, so paths are normalised on the way in and the display path and
the LSP path are the same normalisation.

Completion honours `textEdit` — a server sending one is saying "you typed
`HashM`, replace all of it", and appending instead is how you get
`HashMHashMap` — but clamps the span to end at the cursor, because the list can
be a keystroke or two behind the buffer and eating text ahead of the cursor is
unrecoverable. The popup does not own the keyboard the way the picker does: you
are still in insert mode, it borrows the handful of keys that mean something to
a list, and it stays closed until it has something to show rather than stealing
Enter from someone mid-word on the chance a list might arrive.

**The read-only three — `K`, `gr`, `␣ls`.** Hover, references and document
symbols ask nothing of the buffer, which is what makes them one piece of work
rather than three: no edits to apply, no ranges to reconcile, nothing that can
be wrong in a way you have to undo.

`K` is answered as **plain text with the markdown taken off**. We ask for
`plaintext` and rust-analyzer sends markdown anyway — `contentFormat` is a
preference, not a contract, and the deprecated `MarkedString` shape is fenced
by definition — so the fences come off in `Hover::text`. The horizontal rules
go with them: a `---` is a section break, the blank line it sits between is
already that break, and without a rule to draw it is a row of stray dashes.
The alternative was a markdown renderer in the panel, which is a lot of front
end for a signature and two sentences. A hover that reduces to nothing is
reported as nothing rather than as an empty panel, because "no hover" is the
common answer — on whitespace, on a keyword, on a comment.

`gr` and `␣ls` are **pickers whose rows arrive late**, which is the one thing
Telescope had never had to do: every other picker recomputes its rows from the
query, and these two cannot. So the rows are held, the query narrows what came
back, and an empty query keeps the server's own order — reading order is
meaning for a symbol list, and ranking an empty pattern would shuffle it into
whatever the matcher considers a tie. Both open immediately and fill on the
reply, so the keystroke has an effect before the round trip does; an answer to
a picker that has since been closed is dropped rather than reopening it.

Two details in the rows themselves. A reference carries **the line it is on**,
because `path:12` says nothing about which of the eleven uses this is — files
are read once each rather than once per hit, since references cluster. And a
nested symbol carries **its parent as context rather than as indentation**:
`len` under `Buffer` reads as `len · Buffer`, which keeps the label a bare name
for the matcher to work on. A query that had to match through leading spaces
would find nothing.

**Formatting, and the order `:w` does things in.** A save formats when the
server offers to — rustfmt behind rust-analyzer, gofmt behind gopls — and the
options we send are a floor rather than a policy, because a server with a real
formatter behind it reads the *repository's* configuration and ignores them.
The style that wins should be the project's, not the editor's.

**The file is written first and formatted second.** An editor with a blocking
save formats first and waits with a timeout; this core has no clock, so it
cannot. And a `:w` that might not write, because a server went quiet, is worse
than a file written twice. So the text goes to disk as it is, the question goes
out, and if the answer changes anything the file is written again. The cost is
visible and worth naming: a watcher sees two writes for a save that reformats.

Formatting is the first thing here that both asks a server a question *and*
edits on the answer, which makes it the first that can be stale in a way that
corrupts rather than merely disappoints — edits computed against one version of
a file, applied to another, land in the wrong places. So the request remembers
which file and which buffer version it was asked about, and a reply that no
longer matches is dropped with a notice rather than applied.

Two rules in the applier itself, which is the piece rename and code actions
will reuse. **Every offset is converted before anything is applied**, because
they all describe the document the server saw; converting them one at a time as
the text moves underneath leaves every edit but the first pointing somewhere
else. And **the cursor is kept by line and column, not by offset** — a
whole-document format is usually one edit replacing everything, and an
offset-preserving cursor lands at the end of the file. The whole set goes to
`Buffer::apply` at once, so a reformat is one press of `u`.

`:fmt` formats without saving, which is the only reason it exists separately.

C and C++ go to clangd, which formats with `clang-format` behind it. Where
clangd is not installed — a Mac with Xcode has `clang-format` but keeps it off
the PATH — the editor runs `clang-format` itself (`formatters.rs`): the buffer
on stdin, `--assume-filename` so the project's `.clang-format` is found, on a
thread, so the core waits for nothing. Its answer is turned into one edit of
the lines that changed and goes through the same door as a server's, so the
version check, the cursor and the single `u` all hold.

**Rename and code actions — `␣lr` and `␣la`.** Both come back as a
`WorkspaceEdit`: a set of text edits across however many files the change
reaches. `Workspace::apply_workspace_edit` is where they land, and it is public
because it is the seam rather than because a test wanted it — a rename, a code
action and, when M3's second half arrives, an agent's staged diff are all the
same shape of thing.

Three rules there, all of them about not half-doing it.

**File operations are refused outright.** Renaming a Rust module renames its
file, and rust-analyzer sends that in the same list as the text edits.
Creating, moving or deleting a file touches things that are not open, cannot be
taken back with `u`, and may be under version control — so the whole edit is
declined and named, rather than applied with the file half silently dropped. A
`mod` pointing at nothing, reported as success, is the outcome that rule
exists to prevent.

**Everything is checked before anything is applied.** A version that has moved
on, a file that is not there — either refuses the lot. The version in a
workspace edit is the server's count of *our* `didChange` notifications, not
the buffer's own version, so `Servers::is_current` is the only place the two
can be compared; claiming `documentChanges` in the handshake is what makes
those versions arrive at all.

**Nothing is written.** Every touched file is opened as a buffer and left
modified, so the change is visible, `u` takes it back, and it reaches the disk
when you say so. A rename that saves six files behind you is not one you can
look at before you commit it.

The action list is a picker like the others, with two details. An action that
*cannot* be applied is listed and marked rather than hidden, because a menu
that quietly drops half of what the server offered is one you cannot trust to
be showing everything. And most of rust-analyzer's actions arrive **without an
edit** — computing twenty of them to draw a list nobody has read yet is what
makes a code-action menu slow — so the chosen one is resolved through
`codeAction/resolve` and the edit lands a round trip after the Enter.

Rename reuses the `:` line rather than growing a prompt of its own: `␣lr`
opens it holding `rename <the name it has now>`, read from the buffer rather
than from `prepareRename`, which would put a round trip in front of the prompt
to learn something the text already says. The rename is an ordinary ex command,
so it can also be typed.

`workspace/applyEdit` — a server pushing an edit at us rather than answering
one we asked for — is declined in the shape the server expects, `applied:
false`. It is a *request*: the reply is read, and answering `null`, or worse
`applied: true` for an edit nobody applied, leaves the server believing the
workspace says something it does not.

All four of these live under `␣l`, which is what the which-key rail has always
labelled `+lsp`: `␣lr` rename, `␣la` action, `␣ls` symbols, `␣lf` format. The
symbol picker was briefly `␣ds`, which was a mistake — `␣d` is the debugger's
letter and `␣ds` reads as "debug s…".

*Exit: met. `␣ff` and `␣fg` are faster than reaching for the mouse;
`crates/proton-core/tests/live_server.rs` drives a real rust-analyzer through
the same seam the shell uses and gets back a completion list, a `gd` that lands
on the declaration, a type error in the gutter, a hover carrying the signature
with no fences left in it, a `gr` that finds the declaration and the call site,
a symbol list that reads down the file, a `:w` on a badly spaced file that
comes back rustfmt-clean on disk as well as in the buffer, a rename that
reaches the declaration and the call site without writing either, and a code
action — "Extract into variable" — resolved and applied.*

Still open, and deliberately deferred: document sync is full-text rather than
incremental. Incremental ships less over the pipe, but it means getting ranges
right on both sides of every edit, and a server that disagrees with the editor
about the text produces diagnostics on the wrong lines — the worst kind of
wrong. Revisit when edits are batched. The language table is hard-coded until
it becomes user config in M4.

**M3 — Agent. ✅ done.** The runtime ships: `proton-agent` is the
Messages API, layered the way `proton-lsp` is, and for the same reasons. `sse`
frames bytes and knows nothing about their meaning; `stream` assembles events
into a turn; `wire` gives both directions one set of types, because a response
has to round-trip into the next request; `client` owns the connection and hands
back a mailbox drained on the same 50 ms heartbeat the language servers use.

The Codex provider uses `proton-agent/src/codex.rs` to launch a local Codex
app-server over JSONL stdio. Codex manages its own login. Startup checks the
account and exact selected model before sending a prompt; GPT-6 Astra is the
provider default and model fallback is disabled. A current desktop runtime is
preferred on macOS, with `PROTON_CODEX_BIN` and PATH alternatives. The dynamic
tool protocol is experimental and was verified against Codex 0.153.4.

Each question uses an ephemeral thread with Proton's serialized conversation.
No Codex environment is attached, and workspace operations use the `proton`
dynamic-tool namespace. Tool requests enter the existing workspace executor;
results return to the same running turn, and completed exchanges are saved in
Proton's history. Text and reasoning summaries stream through existing deltas.
Cancellation kills and reaps the child process; startup and whole-turn timeouts
prevent a stalled process from leaving the agent busy forever. Unexpected
permission requests fail visibly. Claude and Gemini retain their HTTP paths.

Two deltas cannot be used as they arrive, and both are quiet if you get them
wrong. **`input_json_delta` carries fragments of a JSON string**, so a tool's
arguments only exist once its block stops — anything reading them mid-stream is
reading something that is not there. And **a thinking block's `signature`
arrives after its text and may be split**, so it is appended rather than
assigned; a truncated one fails the *next* request, one turn after the mistake.

Most of the request builder is one idea: **prompt caching is prefix-matched, in
the order `tools` → `system` → `messages`**. The tool list is sorted before it
is sent, because a `HashMap` iteration order is not stable and a tool list that
reshuffles is a cache that never hits. Nothing reports that — it shows up as
`cache_read_input_tokens` going to zero, which is a cost regression rather than
an error, so the number is asserted on in tests and shown in the statusline.

**The tools are the editor's own capabilities.** `grep` is the index Telescope
searches and `read_file` reads through the same resolver the picker uses, so
what the agent sees and what the user sees cannot drift apart. They run in
`proton-core`, where the workspace is, rather than in `proton-agent`, which
knows nothing about buffers.

One rule shapes the whole of `core::agent`: **every tool result goes back in a
single user message.** The model issues parallel calls in one assistant turn,
and splitting the answers across several user messages trains it out of doing
that. A failed tool still answers, with `is_error` — dropping the block leaves a
`tool_use` with no result, which is a conversation the API rejects.

**`edit_file`: written, or staged.** Since 2026-10-31 the agent writes what it
asks for, with `agent.writeFiles` on — the default: the file is written whole
in the project (a neighbour renamed over it), an open buffer reads it again,
and the call is kept (`Written`: the path, what it was — nothing, if the call
made it — and what was written) so its row offers Undo. Undo puts back what
was there, or removes what was made, only if the file is still exactly what
the call wrote and has no unsaved work in its buffer. Off, or for a file with
unsaved changes, one already staged, or outside the project (judged by its
nearest folder that exists, `..` and `.git` refused), the change is staged as
below. The result says which — "wrote…", "created…" or "staged…" — because a
model that assumes either reports what did not happen. Antigravity follows the
same switch (`accept-edits`); its refusals now name what was refused, a
command, a write outside the project, or a tool of the person's own MCP
servers, rather than calling every refusal an edit.

**`run_command`, asked first.** The agent may propose a command line. The call
is deferred, as `run_cell` is; its row shows the whole command with Run and
Skip (`Command::AgentCommand`). Run starts it in the project, in the person's
shell as a login shell (so their PATH), with no input and a process group of
its own, its output read on threads and its end kept (16 kB); the heartbeat
answers the call with its exit and that end, and stops one that runs past
`PATIENCE`. Skip answers that the user declined and not to try it another
way. A cancelled or cleared conversation stops what it started. Antigravity
cannot stop mid-turn to ask, so its own commands stay refused and it is told to
give them to the person instead.

**The file open, with every question asked in Codex.** `agent.openFile`, on
unless turned off — from Settings or the chip in Codex's composer, which names
the file. The file in the editor follows the question under its own heading
(`open_file_context`): its path from the project, the line the cursor is on,
and its text — what is on screen, unsaved changes said to be — whole up to
60,000 characters, else 300 numbered lines either side of the cursor with
`read_file` named for the rest. Asked again with the same text, it is not
sent again: a line says it is still open, unchanged, and where the cursor is;
a new conversation, or one reopened, shows it afresh. Only a file on disk, and
only for a question asked in Codex — the other modes have their own mentions.
The bubble shows what was typed: `agent::split_context` cuts the question at
the first of these headings — the file's, or a mention's — and the entry
names what followed (`Entry.attached`) as chips, in a reopened conversation
too; the task list shows the same (`Task.shown`). An answer's `file://` link
is drawn as the file's name, its path over it, and opens it in Codex's
editor at its `#L` line.

**The review it stages.** A staged change reaches neither the buffer nor the
disk until somebody accepts it.

The tool is **search and replace with a uniqueness rule**, not a line range and
not a whole new file. A line range is brittle — the numbers move as the model
plans its next edit — and a whole file is expensive and invites drift. So
`old_text` must appear **exactly once**: matching twice means the model has not
pinned the place down, and picking the first match is how an edit lands in the
wrong function, so it is refused *with the count*, which tells the model to
send more context. Matching nothing means the file is not what the model thinks
it is, and the answer to that is to read it again.

One idea carries the whole of `core::review`: **a staged change is a target
text, and the hunks are the live diff between the buffer and that target.**
Accepting a hunk edits the *buffer* to match the target over its lines;
rejecting one edits the *target* to match the buffer. Both leave the two texts
agreeing about that stretch, so the next diff has one fewer hunk — and there is
no per-hunk state to go stale, no indices to renumber after an edit, and no way
for the list on screen to disagree with the file. It is the discipline the file
tree and the git gutter already follow: recompute from what is there rather
than remember. The diff itself is `proton-vcs::hunk`, which is the gutter's
Histogram diff with the hunks kept instead of flattened into marks.

Three consequences worth stating. **Accepting is the same as having typed it** —
an ordinary edit, so `u` takes it back; a change you can only undo in git is
not one you would accept in the first place. **Nothing is written**: staging
opens the file so the diff has something to sit beside, accepting types into
that buffer, and `:w` is still the only thing that reaches the disk. And
**several calls in one turn stack**, because a second edit applies to what was
already staged rather than starting again from the buffer — a rename that
touches three places would otherwise arrive as one third of a rename.

*Exit: half met.* `cache_read_input_tokens` is non-zero across turns and
asserted end-to-end in `crates/proton-core/tests/agent.rs`, which drives the
whole loop — question, tool call against the real index, result, answer —
against a scripted API on a loopback socket — which now also drives a turn that
calls `edit_file`, and checks that the file on disk is untouched afterwards.
**Applicable hunks are done**: `edit_file` stages, the review panel draws the
hunks, and Refer and Apply act on them one at a time.

**The task queue.** The agent runs one turn at a time and always will — a
conversation is a sequence, and two turns interleaving into one transcript
would be neither. What the queue changes is what happens to the *second*
question: it used to be refused, which meant remembering it yourself and asking
again at the right moment. Now it waits, and starts on its own when the one in
front of it ends.

Two groups, as the design has always drawn them. **In progress** is the running
task plus everything behind it; **ready for review** is everything that
finished, each row carrying the answer's first sentence, how many tools it ran,
and the files it staged — which is the thing you have to go and decide about,
and the reason the group is called that rather than "done". A failure is a
state rather than a disappearance: a task that vanished when it went wrong
would leave you wondering whether you had asked at all.

The one thing the queue will not do is drop the running task. Removing its row
would not stop the turn, and a list that lies about what is happening is worse
than one you cannot tidy.

*M3 is done.* The shadow worktree was deliberately *not* a prerequisite — a
staged diff held against the buffer was enough to review and apply, as planned,
and a real git worktree earns its place later, when proposed changes need to be
run against tests before they are accepted.

**M4 — Code Visualizer. ✅ done.** `proton-trace` is the second consumer of
upstream's trace format. Nothing forks or patches it: the checkout is a
submodule pinned to a commit, and Proton uses the extension point the project
was built around.

The sidecar came first, and was worth it for the reason the integration notes
predicted — it is where the process supervision, the port handshake and the
toolchain detection got proven. Two things there are in no document and were
found by running it. **Python buffers stdout when it is not a terminal**, so
without `-u` the readiness line sits in a buffer and the handshake waits
forever. And **on Windows the toolchain is not on `PATH`**: the `python` there
is the Microsoft Store stub, and the real one is in MSYS2 — which is also where
the `gcc` and Python-enabled `gdb` the backends need live — so the child is
handed a `PATH` with MSYS2 in front rather than asked to find it.

**A pointer is an edge, never an inline expansion**, and that rule has two
halves. Exact address match first, then the *smallest range containing it*, so
`&node->next` lands on the field and `&arr[2]` on the cell. Resolving only
exact matches leaves half the arrows in a real program dangling; resolving to
the largest container aims every arrow at the same box. The resolution lives in
Rust where it is tested, and the pane draws the edge list it is handed rather
than re-resolving addresses in the DOM.

Tested against traces captured from the real tracer rather than hand-written
ones, because a hand-written fixture only tests what you already believed the
format was. The linked-list fixture carries 287 pointers across 63 steps: 202
resolve, 70 are null, 3 dangle and 8 point outside what is drawn.

The pane gets **one step**, not the trace — the same discipline the viewport
uses for text. Scrubbing moves the editor's cursor to the line the step is
about to run, which is what makes this part of the editor rather than a panel
beside it.

*Exit: met.* `crates/proton-core/tests/visualizer.rs` presses `␣ct` on a buffer
with a read-after-free and finds the dangling edge, through the seam the shell
uses. A dangling pointer draws as a stub ending in a cross rather than as no
arrow at all: an absent arrow reads as a pointer nobody followed, which deletes
exactly the evidence the tool exists to show.

Still open, and the shape of the rest: the trace as an agent tool (`@trace` as
a context scope), `[` and `]` to jump between changes to one value, gutter
marks on traced lines, and writing traces to `.proton/traces/` so `␣ft` finds
them.

**M5 — Plugins. ✅ done.** The host ships: `proton-plugin` is a
wasmtime engine with the Component Model on and epoch interruption on, a
manifest read before any code runs, and a component loader. `examples/hello-plugin`
is a real plugin built to a real component and committed, so the tests need no
wasm toolchain to run.

**The guarantee is the interesting half.** Wasm does not stop on its own: by
default an infinite loop in a plugin spins forever and takes the thread with
it, so "a plugin cannot hang the editor" is a claim rather than a property
unless the engine is told otherwise. Epochs rather than fuel, because fuel
counts instructions and the thing anybody waiting cares about is measured in
milliseconds — a tight loop and a slow one should be stopped after the same
wait. One ticker thread serves the whole engine, and every store the host
hands out already carries a deadline, so there is no path that runs plugin
code off the clock.

**The manifest is read before the wasm is.** A plugin that only ships a theme
has nothing to run, and discovering what a plugin contributes by *calling* it
would mean running everything installed at startup. Paths in a manifest are
resolved against the plugin's own folder and refused if they climb out of it:
a manifest is a file somebody else wrote.

Plugins are **per user, not per project** — they sit beside `session.json`,
because a repository that could drop code into the editor by existing would be
a repository you could not safely open. The session loads them once and shares
one set with every workspace; loading them per project would be a second
engine and a second compile of every component for a second open folder.

Wasmtime is **252 crates and about a minute and a half** on a clean build,
measured before committing to it — the largest dependency here by some way,
and the one with the least substitutable job.

**Themes without a toolchain.** A palette is a list of colours, so shipping
one should not mean writing a program: `themes = ["rose.toml"]` in a manifest
is the whole of it, and `tests/plugins/dusk` is three TOML files and no code.
A theme built by a component's `themes()` export arrives through the same
list, because to everything downstream they are the same thing.

Colours are checked where the file is read rather than where it is used — a
token that is not a colour reaches the page as a CSS value that silently does
nothing, and "one thing is the wrong colour" is much harder to work out than
"this theme is broken, and here is the token". Only hex is accepted: a theme
is a palette rather than a stylesheet, and letting `var(--x)` through would be
letting a plugin write CSS. The front end holds an allow-list of the fifteen
tokens for the same reason.

This works at all because of the design's own rule that **colour is a token
set** — a built-in theme is a `[data-theme]` block and a plugin's is the same
custom properties set from JavaScript, so the extension point was already
there. A built-in stays underneath as the base, so a palette that sets five
tokens gets sensible values for the other ten.

**Grammars.** A tree-sitter grammar is C, and shipping one normally means a
native build per platform; compiled to wasm it is one file that works
everywhere. They are loaded by tree-sitter's own engine rather than the
plugin host's, and that engine is deliberately a *separate* one: the host's
has epoch interruption on and every store carries a deadline, and tree-sitter
creates its stores internally where no deadline can be set — sharing it would
mean the first tick traps a parse. **So a grammar's external scanner is not on
a clock.** The deadline covers plugin commands, which is what it was built
for, and this is a real limit rather than an oversight.

The built-in table is asked first, so a plugin cannot decide how Rust is
coloured by being installed. A grammar is loaded once when it is added rather
than the first time a file wants it, so a grammar that is not one is reported
when you install it instead of showing up as a file that mysteriously has no
colours.

*Exit: met.* A deliberately infinite plugin is killed rather than
hanging the app, proven twice: against hand-written wasm in
`crates/proton-plugin/tests/deadline.rs`, and against a real compiled-Rust
loop in a real component in `tests/component.rs`. A theme installs with no
toolchain at all and is covered end to end. And the grammar path has now been
run against a real grammar: `tree-sitter-go` compiled to wasm, loaded through
`Grammars::add`, found by extension, and asked for spans — with the same
discipline the built-in grammars are held to, that columns are characters and
no span runs past its line.

`tests/live_grammar.rs` stays `#[ignore]` rather than committing that wasm.
Building it is a one-line `tree-sitter build --wasm`, but checking somebody
else's grammar binary into this repository is a licensing decision rather than
a testing one, and the test's value is in having been run, not in running on
every `cargo test`. It runs the moment `PROTON_TEST_GRAMMAR_WASM` points at a
grammar; the module comment holds the invocation.

One thing worth recording, because the tooling moved: tree-sitter's CLI no
longer needs emscripten or Docker to produce a `.wasm` grammar. Since 0.25 it
downloads a wasi-sdk clang and drives that directly, which removes the
toolchain argument against ever regenerating one of these.

**M6 — Data mode. ✅ done.** Closest to Codex; the first real exercise of
the mode contract by something that is not a text editor.
*Exit: a notebook runs, a dataframe renders, and "open sat 17 in Codex" jumps to
the right line.*

**The kernel ships.** `proton-data` speaks the Jupyter wire protocol to a real
kernel: five ports, HMAC-SHA256 signing, and the same two-threads-and-a-channel
arrangement as every other foreign process here, drained on the core's
heartbeat.

Three things about that protocol are worth writing down, because each one fails
in a way that does not look like its cause.

*The delimiter is not an offset.* A message is a list of frames, and everything
before `<IDS|MSG>` is routing identity the socket added. Counting frames from
the front works until a DEALER puts an identity in — then the identity is read
as the signature and nothing verifies.

*A bad signature is silence.* The four JSON frames are signed, and a kernel
drops what does not authenticate without saying so, because a message it cannot
authenticate is not one it can safely answer. So a signing bug does not look
like an error; it looks like a kernel that ignores you. Incoming messages are
verified the same way, constant-time, since that compares a secret against
bytes from outside.

*The reply is not the output.* `execute_reply` says only whether it worked and
what the execution count was. Everything the code actually did — what it
printed, returned, or raised — arrives on the **iopub** broadcast, tied back to
its request by `parent_header`. A client that waits for the reply and renders
its content displays nothing at all, and one that ignores `parent_header` files
one cell's output under another.

Two smaller decisions follow from the protocol rather than from taste. The five
ports are **bound and held** until the moment the kernel is spawned, because a
kernel is handed its ports rather than choosing them: asking the OS for a free
port and closing it leaves a race that is lost only on a busy machine.
And readiness is *asked for* rather than waited for — `kernel_info_request`
until a reply — because a SUB socket drops everything published before its
subscription is live, so a kernel that starts quickly can say "idle" to nobody
and a client waiting for it waits forever.

`tests/live_kernel.rs` runs against a real `ipykernel` behind
`PROTON_TEST_KERNEL`, the way the language-server and adapter tests do: code
runs, printing and results arrive as different messages, a traceback survives,
state persists between cells, and two cells in flight keep their output apart.

**A kernel goes when Proton does.** It runs whatever the notebook says, which
can start processes of its own, so it is started in a session of its own — the
leader of a process group, with no terminal, as Jupyter starts kernels — and
the group is stopped as one: when the kernel is let go of, when it is found to
have exited, and when Proton quits. The shell exits without dropping the
session, so quitting stops every workspace's kernel by name. The connection
file, which names the signing key, is its owner's alone to read and goes with
the kernel. Nothing reconnects to a kernel across a relaunch, so one a crashed
Proton left is out of everybody's reach: the next Proton to start stops it, if
its file names a pid that is still that kernel — its command line names the
file — and no longer the child of the Proton that started it. Another Proton's
kernel, and a pid handed to something else since, are left alone.

**The document ships, and it is the part that had to be exact.** A notebook is
JSON, so Data mode obeys the load-bearing rule — but that rule is only worth
anything if a notebook Proton saved looks like one Jupyter saved. A diff in
which every line has moved is a diff nobody reviews and Git cannot merge.

So `proton-data::notebook` keeps the parsed JSON and writes back into it.
`source` is a list of lines that each keep their newline; everything unknown is
preserved; the formatting is nbformat's one space of indent. Measured against
71 notebooks Jupyter really wrote — 69 come back byte for byte, and the two
that do not were never in nbformat's shape to begin with. What is guaranteed
for every file is weaker and more useful: nothing is lost, and the second save
changes nothing the first did not. Cell ids are not invented, because they
arrived in 4.5 and adding one to a 4.4 notebook makes it invalid.

**A dataframe is a table, not a page of HTML.** It arrives as `text/html` and
`text/plain` at once. The HTML is *not* put in the page — a kernel runs
whatever code the notebook contains, and injecting its markup would leave the
editor's chrome one `<style>` away from whatever a downloaded notebook felt
like doing. `proton-data::table` scans out the rows and the front end draws its
own table, which is also how it ends up themed and selectable. Pandas' `N rows
× M columns` is carried through: showing thirty rows of a million without
saying so is the one thing a table viewer must not do.

**Which cell an output belongs to is something only this end knows.** A kernel
answers on iopub tagged by the `msg_id` of the request, not by cell, so the
mapping is remembered when the cell is run. Lose it and one cell's output lands
under another — worse than no output, because it is believable. While a cell
runs, what it has produced *so far* is shown rather than what it produced last
time, and outputs reach the document only when the cell is done: a notebook
saved mid-run must not hold half an answer beside a number implying a whole one.

**The panel patches cell by cell, and here that is correctness rather than
speed.** Cells are editable, so replacing the box's markup takes the caret with
it — on every heartbeat of output that arrives while somebody is thinking. The
cell being edited is skipped entirely.

*Exit: met.* A notebook runs against a real kernel, a real pandas frame renders
as columns and rows, and the jump works from both ends — `JumpTo` for a known
place, and a click on any table cell to search the project for that value,
which is "open sat 17 in Codex" as it is actually reached.

**M7 — Design mode.** (Moved to an app of its own in October 2026; kept here as history.) Two-way binding between artboard and component source.
The hardest of the seven, because the binding has to survive a human editing
both sides.

**Both documents are held as text and edited by splice.** `proton-design`
owns two formats — a stylesheet's custom properties and a component's markup —
and neither is ever re-printed from a model. A `Token` carries the span its
value occupies and a `Layer` carries the span its element occupies, so a change
replaces those bytes and moves nothing else. That is the load-bearing decision
and it is the same one Data mode made about `.ipynb`: a palette editor that
reformatted, or a layer inspector that re-emitted the tree, would turn every
colour tweak and every rename into a whole-file diff — and a diff in which
every line has moved is one nobody reviews. The markup tests use a fixture
with single quotes, odd indentation and a comment in it, and assert the result
is byte for byte the original apart from the change.

**The markup parser is deliberately not a conforming one.** It handles what
people write — nesting, void elements, self-closing syntax, comments,
doctypes, raw-text elements, `>` inside quoted attributes — plus the tags that
close each other without being told to. That last part is not pedantry: the
browser applies those rules when it renders the artboard, so a tree without
them would show a nesting the picture does not have and clicking a layer would
outline the wrong element. Beyond that it does not guess; an unclosed `<div>`
closes when its parent does, which leaves the tree wrong in the same shape the
file is wrong. It is tested against this repo's own 92 KB `index.html`.

**The artboard is a sandboxed frame, and that is a security boundary.** The
source is somebody's file and `withGlobalTauri` puts the filesystem and the
shell in the app's document, so rendering markup there with `innerHTML` would
hand anything in it those keys — no malice required, an analytics snippet is
enough. It renders in an iframe with `allow-scripts` and *not*
`allow-same-origin`, so the origin stays opaque, and a CSP with a per-render
nonce admits only the one script that reports clicks — the component's own
scripts do not run, and `default-src 'none'` means no network. Messages coming
back are checked against `event.source`, which cannot be forged, rather than
against an origin that is the string `"null"`.

*Exit: met.* `crates/proton-core/tests/theming.rs` sets a token through the
inspector and reads it back off the disk; opens the component in Codex, types
into it, saves, and finds the artboard holding the new markup; and edits a
layer from the design side and finds the file changed and otherwise untouched.
Clicking the picture selects the layer, which the milestone did not ask for.

**Modeling, Game, Video — documents, then engines.** These followed M0–M7 and
moved to an app of their own in October 2026, with Canvas and Diffuse; their
history is in that app.

**The Lab's files on the seam.** `Event::ResearchRun` names the pictures and 3D scenes a
run saved, and the page asks the shell's `read_lab` for each by
`<investigation>/<run>/<path>`. The core says where the file is, under the
session's lock, and gives only a file inside that run's folder of a kind the
Lab shows — no `..`, no absolute path, no link out, no code; the shell reads it
after letting go of the lock. A scene is drawn by `ui/lab3d.js` with three.js —
`ui/vendor/three/`, its own build files unminified and imported the first time
a scene is opened: spheres, bonds and arrows sharing three geometries, points as
one instanced mesh, lines and meshes, OrbitControls, and frames built the
first time they are shown. It draws only on a change, so a scene left open
costs nothing.

**Molecules in the Lab.** A run's `.xyz`, `.pdb`, `.cif`, `.mol`, `.sdf` and
`.smi` files (`proton_lab::run::MOLECULES`; the helper's `lab.xyz` and
`lab.smiles` write the first and last) come to the window as
`LabRunDetail.molecules`, and `read_lab` gives them as it gives a scene. Drawn
with the three.js the scenes use, rather than Mol* or NGL, which would have
been a download. `ui/molecule.js` is pure and unit-tested: the files by their
columns or tables (the first of each atom's alternate places; `CONECT` for
HETATMs; `MODEL`s, SDF records and trajectory frames as frames), bonds found
by covalent radii on a grid where a file has none (120,000 atoms in about
0.6 s), and a protein's helices and strands from its `HELIX`/`SHEET` or
`_struct_conf`/`_struct_sheet_range` — or, without them, guessed from
α-carbon spacing and said to be. SMILES is parsed, its unwritten hydrogens
added, and laid out by a small force field: bond lengths from covalent radii,
angles as 1–3 distances, flat fours where a flat atom or a double bond says so,
and nothing nearer than it should be, first in four dimensions so rings can
pass through each other, then in three; repeatable, no stereo, and said so.
Each SMILES in a file is laid out when it is first shown. `ui/lab-molecule.js`
draws atoms as impostors — a quad a sphere, the sphere found per pixel and
written to the depth buffer — and bonds as one instanced cylinder, half each
atom's colour, double and triple bonds side by side; a protein's cartoon is a
tube along its α-carbons, a strand's averaged flat, flattened into a ribbon by
each residue's C=O and into an arrow at a strand's end. A molecule is turned
to face the camera with its widest spread. Clicking picks by casting the ray
against the drawn spheres on the CPU; two picks give their distance.

---

## 8. Risks, named

- **The trenchcoat risk.** Seven modes is several products. The mode contract
  and the diffable-document rule are the two things that decide whether it holds
  together; if either erodes, stop and fix it rather than adding a mode.
- **Webview editor latency.** The known cost of the M0 bet. M1's exit criterion
  exists specifically to catch it early, while swapping one pane is still cheap.
- **GPUI's pull.** It will look increasingly attractive during M1. Resist until
  there is a number; revisit with STACK.md § "If you go GPU-native anyway",
  where text *shaping* — not rasterisation — is the real cost.
- **Agent cost.** Caching is not an optimisation here, it is the difference
  between usable and not. Treat a cache-hit regression as a build failure.
- **Solo scope.** M0–M4 is a real editor with a real agent. Everything after is
  optional. Ship in that order so that abandoning M5+ still leaves a product.

---

## 9. This machine

Verified, not assumed — worth recording because it changes what is cheap:

| | |
|---|---|
| Rust | 1.96.0 (rustc + cargo) |
| Node | 25.9.0, npm 11.12.1 — for the shell's front-end build |
| MSYS2 UCRT64 | `gcc`, `gdb` **built with Python support**, `python3` — all present at `C:\msys64\ucrt64\bin` |
| WebView2 | Ships with Windows 11 |

The gdb-with-Python check is the one that usually fails on Windows and blocks
Code Visualizer's C/C++ backends. It passes here, so M4 needs no toolchain work
— detect `C:\msys64\ucrt64\bin` rather than asking the user.

---

## 10. Working on it

`frontendDist` points at a directory, so `generate_context!` **embeds** the
front end into the binary. `tauri-build` emits a `rerun-if-changed` for that
directory, so a UI edit does get picked up — but only by rebuilding:

```bash
cargo run -p proton-shell     # ~3s incremental after a ui/ edit
node ui/serve.mjs             # or iterate in a browser, no rebuild at all
```

The browser path is the faster loop for anything that is not shell behaviour.
`bridge.js` dynamically loads `preview-bridge.js` when Tauri is absent, so the page
runs either way — but the stand-in is a demo, not a second implementation, and
must never grow past keeping the design work honest. The real state machine
lives in `crates/proton-core/src/vim.rs`.

---

## 11. Packaging

```bash
git submodule update --init          # the tracer, which is bundled
cargo tauri build                    # → target/release/bundle/
```

Two bundles come out: an NSIS `.exe` and an MSI. The NSIS one is the one to
use — it installs per-user, needs no elevation, and carries an uninstaller.

**Install where Windows keeps per-user programs.** Tauri's NSIS default is
`%LOCALAPPDATA%\Proton`, which is where applications put their *data*, not
their binaries. `/D` moves it, and has to be the last argument and unquoted —
that is NSIS's rule, not a Tauri one:

```bash
Proton_0.0.1_x64-setup.exe /S /D=%LOCALAPPDATA%\Programs\Proton
```

### What ships

The tracer goes in the bundle: `server/` and `backends/` from the
Code Visualizer submodule, and its examples. Upstream's `web/` is 2.5 MB of
frontend that Proton replaces, so it is left out.

An installed copy has no project tree, so `proton-trace` looks for the checkout
beside the executable as well as under `vendor/`. Both paths are tested.

### What it still needs from the machine

Bundling the tracer does not bundle a toolchain, and the backends shell out to
one:

| | |
|---|---|
| Python 3 | MSYS2's, at `C:\msys64\ucrt64\bin` — the `python` on `PATH` is the Microsoft Store stub, which is not a Python |
| `gcc` / `g++` | for the C and C++ backends |
| `gdb` **with Python support** | the C and C++ backends drive it; `gdb --batch -ex "python print(1)"` is the check |

Without them the Visualizer reports which language is unusable and why —
`GET /api/toolchain` distinguishes "not implemented" from "not usable here",
and that reason is what the pane shows. Everything else works.

### Where it finds programs

Opened from the Finder or the Dock, the app is given launchd's PATH,
`/usr/bin:/bin:/usr/sbin:/sbin`, and so is everything it starts by name —
ffmpeg, pandoc, tesseract, git, a kernel's `!pip`. So the first thing `main`
does, while it is the only thread, is ask a login shell that reads no rc files
for its PATH (a seventh of a second here) and add the folders it lacks after
its own; a PATH a terminal gave already has them and is left as it is
(`proton_claude::pipe::launch_path`). The answer is kept in
`~/.proton/launch-path.json`, keyed by the shell and the times of
`/etc/paths`, `/etc/paths.d` and the login files, and used while none of them
changes; once the window is up the shell is asked again for the next launch. The interactive login shell's PATH,
which has conda's and nvm's folders, takes seconds, so it is asked on a thread
of its own at launch and given to what needs it: the engines, and a notebook's
kernel, which runs with its own Python's folder ahead of it so that `!pip`
installs where `import` looks.

### Which project it opens

`Proton.exe <folder>`, the way every editor takes it.

With no argument it reopens last session's projects. Failing that it falls back
to the working directory, and failing *that* — a Start-menu launch, where the
working directory is the install folder and indexing it would be nonsense — to
the home directory.

Once it is open, `␣fp` opens a folder anywhere on the disk, `␣pn` / `␣pp` step
between the ones already open, and the tiles at the top of the rail switch and
close them. See § 3 → Workspaces.

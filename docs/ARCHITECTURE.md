# Proton — architecture

The build document. **M0 through M2 and M4 are implemented, and M3 is
half-built**; everything from M5 on is a plan. The design it serves is in [`../design/DESIGN.md`](../design/DESIGN.md), and the
reasoning behind the tool choices is in [`STACK.md`](STACK.md). This file says
**what we build and in what order**, and is the one to argue with before writing
code.

```bash
cargo test --workspace        # 252 tests, headless
cargo test --workspace -- --include-ignored   # plus 10 driving rust-analyzer and the tracer
git submodule update --init   # the Code Visualizer checkout, for the Visualizer pane
cargo run -p proton-shell     # the app
node ui/serve.mjs             # the front end alone, in a browser
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
      │  all seven modes   │        │  latency budget fails  │
      ╰────────────────────╯        ╰────────────────────────╯
```

Two rules keep that seam honest:

1. **`proton-core` never names a widget.** It emits `Event::BufferChanged`, not
   "repaint line 42". If a core crate would need to know pixels, the design is
   wrong.
2. **The vim engine lives in the core.** It resolves keys to *actions*
   (`Delete·Line`, count 2), which is exactly the boundary that lets a different
   front end reuse it verbatim.

The webview ships first because seven modes demand it. The GPU surface is held
in reserve for the editor pane alone, and only once a measured
input-to-photon number justifies it. See STACK.md § "The actual decision".

---

## 2. Crate layout

One cargo workspace. Every crate below `proton-shell` builds and tests headless.

| Crate | Owns | Key deps |
|---|---|---|
| `proton-text` | Rope, undo tree, multi-cursor selections, UTF‑8 ↔ UTF‑16 ↔ line/col | `ropey` |
| `proton-vim` | Modal state machine, operators, motions, text objects, registers, marks, macros, which-key tree | — |
| `proton-syntax` | Tree-sitter parsing, incremental re-parse, viewport-scoped highlight queries | `tree-sitter` |
| `proton-lsp` | LSP client, server supervision, JSON-RPC framing, a non-blocking mailbox | `serde_json` |
| `proton-vcs` | Status, gutter, blame, staging, the shadow worktree | `gix` |
| `proton-index` | The index behind every Telescope picker: file walking, fuzzy matching, grep | `ignore`, `nucleo-matcher` |
| `proton-agent` | Messages API client, SSE streaming, request building, auth | `ureq` |
| `proton-trace` | Code Visualizer client: trace format, pointer resolution, sidecar | `ureq` |
| `proton-plugin` | Wasmtime host, WIT interfaces, capability grants | `wasmtime`, `wit-bindgen` |
| `proton-core` | Workspace state, mode registry, command/event bus, config | all of the above |
| `proton-shell` | Window, webview, IPC bridge, system backdrop, packaging | `tauri` v2, `window-vibrancy` |
| `proton-cli` | `proton trace`, `proton fmt`, headless smoke tests | `clap` |

**Built so far: `proton-text`, `proton-index`, `proton-syntax`, `proton-lsp`,
`proton-agent`, `proton-trace`, `proton-core`, `proton-shell`.** The rest of the table is the shape it grows
into. `proton-vim` has not split out of `proton-core` yet — the modal engine is
one file there and would gain nothing but a manifest from moving today.

`proton-lsp` ended up with no async runtime and no protocol crate. Two threads
and a channel do what `tokio` would, for a client that talks to two or three
processes; and the protocol surface Proton actually uses is three response
shapes, each with a subtlety worth writing down rather than inheriting.

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

Rust owns the types; TypeScript is generated from them with `ts-rs`. Two enums,
both `#[derive(Serialize, Deserialize, TS)]`:

```rust
// shell → core
enum Command {
    Key { seq: KeySeq, mode: ModeId },
    Action(Action),              // already-resolved, from a menu or a click
    OpenBuffer { path: PathBuf },
    SetMode(ModeId),
    Agent(AgentCommand),
    ...
}

// core → shell
enum Event {
    BufferChanged { id: BufferId, edits: Vec<Edit>, version: u64 },
    Diagnostics  { id: BufferId, items: Vec<Diagnostic> },
    VimMode(VimMode),
    AgentDelta(AgentDelta),      // streamed
    TraceStep(TraceStep),
    ...
}
```

Events are **deltas with a version**, never whole-buffer snapshots — that is
what keeps the webview affordable at scale, and it is the same discipline a GPU
front end would need.

---

## 3. Core subsystems

### Text

`ropey` behind a `TextStore` trait. It already tracks UTF-16 code units in its
node info (`char_to_utf16_cu` is O(log n)), which is the thing LSP needs and the
thing that is painful to retrofit — verified, not assumed. Undo is a **tree**,
not a stack, and it is per-buffer state in the core so `u` works identically in
Codex and in Video.

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

### Syntax and semantics

Tree-sitter for highlighting, folds, text objects and structural selection; LSP
for everything semantic; DAP for the debug rail. Both language servers and
debug adapters run out of process, so a crash costs a squiggle rather than the
editor. Grammars load as wasm through the same sandbox as plugins, which means a
plugin can add a language without a native build step.

### Plugins

Wasmtime, Component Model, WIT-defined interfaces. Two things configured on day
one, not later:

- **`Config::epoch_interruption`** with a per-call deadline. Without it the
  "a plugin can't hog the thread" guarantee is false — an infinite loop in wasm
  spins forever by default.
- **A coarse API.** Host calls are not free, so there is no per-keystroke hook.
  Plugins get grammars, themes, LSP adapters, commands, and mode contributions.

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
  is exactly right for Proton: "the user switched to Video mode", "the trace
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

That is the whole contract. Chat, Codex, Design, Modeling, Game, Video and Data
are the seven that ship; nothing about them is privileged, so an eighth is an
extension rather than a fork.

**Every mode's document is a text file that diffs in Git.** This is the load-
bearing rule of the entire project:

| Mode | Document |
|---|---|
| Chat | `.proton/chats/*.json` — messages, tool calls, artifact refs |
| Codex | Source files |
| Design | Component source + `theme.css` tokens |
| Modeling | `scene.proton.json` |
| Game | Engine-native scene files |
| Video | `.edl` + sidecar `.srt` |
| Data | `.ipynb` + `.sql` |

It is not tidiness. It is what lets the agent edit a scene or a timeline by
writing a patch instead of driving a GUI, and what makes any of it reviewable.

---

## 5. The shell

The prototype now lives in `ui/` and is the real chrome; its stylesheet is
already the design system. Three notes for the build:

- **The mode switcher is a vertical rail**, outboard of the panel rail — see the
  note in `DESIGN.md` § The shell. The prototype still shows it in the titlebar;
  build it vertical.
- **The editor pane is virtualised** from day one: visible window plus one
  screen of overscan, syntax highlighting off the main thread, deltas over the
  bridge. This is the pane that decides whether the webview bet holds.
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
symbols and diagnostics will need no new matching code.

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
into a mailbox. Hand-written rather than imported: the response shapes that
matter are three, each with an `untagged` subtlety worth stating explicitly
instead of inheriting.

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

*Exit: met. `␣ff` and `␣fg` are faster than reaching for the mouse;
`crates/proton-core/tests/live_server.rs` drives a real rust-analyzer through
the same seam the shell uses and gets back a completion list, a `gd` that lands
on the declaration, and a type error in the gutter.*

Still open, and deliberately deferred: document sync is full-text rather than
incremental. Incremental ships less over the pipe, but it means getting ranges
right on both sides of every edit, and a server that disagrees with the editor
about the text produces diagnostics on the wrong lines — the worst kind of
wrong. Revisit when edits are batched. The language table is hard-coded until
it becomes user config in M4.

**M3 — Agent. ◐ in progress.** The runtime ships: `proton-agent` is the
Messages API, layered the way `proton-lsp` is, and for the same reasons. `sse`
frames bytes and knows nothing about their meaning; `stream` assembles events
into a turn; `wire` gives both directions one set of types, because a response
has to round-trip into the next request; `client` owns the connection and hands
back a mailbox drained on the same 50 ms heartbeat the language servers use.

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

*Exit: half met.* `cache_read_input_tokens` is non-zero across turns and
asserted end-to-end in `crates/proton-core/tests/agent.rs`, which drives the
whole loop — question, tool call against the real index, result, answer —
against a scripted API on a loopback socket. **Applicable hunks are the half
that is not done**: there is no `edit_file` tool and no hunk staging yet, so the
agent can read the workspace and explain it but cannot yet propose a change to
it.

Still open, and the shape of the rest of M3: an `edit_file` tool whose output
is staged rather than applied, per-hunk Refer/Apply over that staging, and the
task queue. The shadow worktree is deliberately *not* a prerequisite for the
first two — a staged diff held against the buffer is enough to review and
apply, and a real git worktree earns its place later, when proposed changes
need to be run against tests before they are accepted.

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

**M5 — Plugins.** Wasmtime host, WIT interfaces, epoch interruption, wasm
grammars, themes.
*Exit: a third-party grammar and theme install without a native toolchain, and a
deliberately infinite plugin is killed rather than hanging the app.*

**M6 — Data mode.** Closest to Codex; the first real exercise of the mode
contract by something that is not a text editor.
*Exit: a notebook runs, a dataframe renders, and "open sat 17 in Codex" jumps to
the right line.*

**M7 — Design mode.** Two-way binding between artboard and component source.
The hardest of the seven, because the binding has to survive a human editing
both sides.
*Exit: editing a token in the inspector changes `theme.css`, and editing the
component in Codex updates the artboard.*

**Later — Modeling, Game, Video.** Each needs a renderer and each is a real
project. Do not start one until M0–M7 are shipped and stable.

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
`bridge.js` falls back to a local stand-in when Tauri is absent, so the page
runs either way — but the stand-in is a demo, not a second implementation, and
must never grow past keeping the design work honest. The real state machine
lives in `crates/proton-core/src/vim.rs`.

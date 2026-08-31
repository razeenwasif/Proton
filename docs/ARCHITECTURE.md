# Proton — architecture

The build document. **M0 and M0.5 are implemented**; everything from M1 on is a
plan. The design it serves is in [`../design/DESIGN.md`](../design/DESIGN.md), and the
reasoning behind the tool choices is in [`STACK.md`](STACK.md). This file says
**what we build and in what order**, and is the one to argue with before writing
code.

```bash
cargo test -p proton-core     # 23 tests, headless
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
| `proton-syntax` | Tree-sitter parsing, highlight queries, folds, structural selection | `tree-sitter` |
| `proton-lsp` | LSP client, DAP client, server supervision, capability negotiation | `lsp-types`, `tokio` |
| `proton-vcs` | Status, gutter, blame, staging, the shadow worktree | `gix` |
| `proton-index` | Symbol + embedding index behind Telescope's `␣fa` | `tantivy` |
| `proton-agent` | Claude client, tool dispatch, task queue, diff staging | `tokio`, `reqwest`, `eventsource-stream` |
| `proton-trace` | Code Visualizer client + process supervision | `tokio` |
| `proton-plugin` | Wasmtime host, WIT interfaces, capability grants | `wasmtime`, `wit-bindgen` |
| `proton-core` | Workspace state, mode registry, command/event bus, config | all of the above |
| `proton-shell` | Window, webview, IPC bridge, system backdrop, packaging | `tauri` v2, `window-vibrancy` |
| `proton-cli` | `proton trace`, `proton fmt`, headless smoke tests | `clap` |

**M0/M0.5 ship `proton-core` and `proton-shell` only.** The core today holds the
bus, the mode registry and the modal engine; the rest of the table is the shape
it grows into. `proton-vim` and `proton-text` split out of `proton-core` when M1
gives them enough substance to be worth their own crates — splitting earlier
would be structure without content.

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
*Exit met: `[shell] backdrop: mica` on this machine, with the chrome
translucent and the stage near-opaque so code never depends on a wallpaper.*

**M1 — The editor.** `proton-text`, `proton-syntax`, virtualised rendering, and
the text objects / marks / macros the M0 engine was shaped to take. Real
buffers, real editing, real highlighting.
*Exit: Proton edits its own source, and `2dd`, `ciw`, `.`, `u` and a recorded
macro all behave. Measure input-to-photon here and write the number down.*

**M2 — Telescope + LSP.** Files, grep, buffers, symbols, diagnostics. The spine
of the whole interaction model; get it right before adding anything.
*Exit: `␣ff` and `␣fg` are faster than reaching for the mouse, and
rust-analyzer drives completion and go-to-definition.*

**M3 — Agent.** `proton-agent`, streaming, tool dispatch, the shadow worktree,
per-hunk Refer/Apply, the task queue. **Chat mode ships here** — it is this
runtime given the whole window instead of a sidebar, and it is the cheapest way
to make the agent useful before any non-code mode exists.
*Exit: "fix the jank" produces applicable hunks the user reviews, and
`cache_read_input_tokens` is non-zero across turns.*

**M4 — Code Visualizer.** Sidecar, then the native pane. First real proof of the
"second consumer of a document format" pattern every later mode needs — and the
toolchain is already working on this machine, so it is cheap.
*Exit: `␣ct` traces `trajectory.c` and the dangling read is visible at step 212.*

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

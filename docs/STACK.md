# Proton — stack

Status: **proposed**, nothing built. This supersedes the "Rust + native webview"
line in [`ARCHITECTURE.md`](ARCHITECTURE.md), which was written before the
renderer question was taken seriously.

---

## Settled

These are not close calls. Take them and move on.

| Layer | Choice | |
|---|---|---|
| Core language | **Rust** | Zed and Lapce are the proof. The concurrency story is the real prize: background indexing, tracing and agent I/O without a class of data race. |
| Text storage | **Rope** (B-tree with summaries) | Not "rope *or* piece table" — see below. |
| Parsing | **Tree-sitter** | Incremental, error-tolerant, grammars for everything. Nothing else is close. |
| Semantics | **LSP** + **DAP** | Out-of-process, so a crashing rust-analyzer costs you a squiggle, not the editor. |
| Plugins | **WebAssembly / Wasmtime**, Component Model | Sandboxed, near-native, language-agnostic. Zed validated this. |

### Rope vs piece table — they are not alternatives

A rope gives O(log n) edit *and* O(log n) positional lookup. A piece table gives
cheap undo and keeps the original file untouched, but a naive one is O(pieces)
to find an offset, which degrades as you edit. VS Code's "piece table" is
actually a piece *tree* — a red-black tree over the pieces — which is a rope
whose leaves happen to be spans of two buffers. So: build a B-tree, and the
piece-vs-rope question dissolves.

What matters far more is **what the tree summarises**. Each node should carry,
at minimum: byte length, UTF-8 char count, **UTF-16 code-unit count**, and
newline count. The UTF-16 one is not optional — LSP positions are UTF-16 offsets,
and if your summary lacks it you will scan the line on every request, every
completion, every diagnostic. Zed's `SumTree` is the shape to copy.

**Correction to an earlier draft of this file:** `ropey` *does* carry UTF-16 in
its node info — `len_utf16_cu()`, `char_to_utf16_cu()`, `utf16_cu_to_char()`
are all O(log n) (verified against ropey 1.6.1's `src/rope.rs`). So start with
ropey behind a `TextStore` trait and do not hand-roll a tree. You only need a
custom `SumTree` later if you want *additional* summary dimensions — folded
height, diagnostic counts, per-line syntax state — and that is a measurable
optimisation, not a prerequisite.

### One correction worth making early

> "opening a 5GB log file won't crash or slow down the editor"

A rope does not give you this. You still have 5GB resident. What gives you large
files is refusing to do work: memory-map the file, index newlines lazily, skip
tree-sitter above a size threshold, and never ask the LSP about it. Those are
policy decisions in the file-open path, unrelated to the rope. Worth separating
so the rope does not get blamed — or credited — for the wrong thing.

### Wasm plugins: two things to configure on day one

- **Preemption is opt-in.** By default an infinite loop in a plugin spins
  forever and the "can't hog the thread" guarantee evaporates. Turn on
  `Config::epoch_interruption` (or fuel) and give plugins a deadline.
- **Keep the API coarse.** Host calls are not free, so a per-keystroke plugin
  hook is a performance bug waiting to happen. Zed's extension surface is
  deliberately narrow — grammars, themes, LSP adapters, slash commands — because
  wasm has no DOM and no cheap chatter.

Nice side effect: tree-sitter grammars compile to wasm, so plugin-provided
languages load through the same sandbox as everything else.

**LuaJIT** is a real option *alongside*, not instead: wasm for extensions, Lua
for config and keymaps. It is what nvim users expect, and LuaJIT being frozen at
5.1 semantics is a feature there. But two runtimes is two runtimes — only take
it if the config story demands it.

---

## The actual decision: the renderer

This is where the proposal and the design collide, and it deserves a straight
answer rather than a list of options.

**GPUI is the right tool for a code editor and the wrong tool for six of
Proton's eleven modes.**

The case for it is real. Text at the monitor's refresh rate, a design-driven
layout model, and it is the only Rust GUI framework built by people shipping a
production editor with it. If Codex mode were the whole product, this section
would say "use GPUI" and stop.

But look at what the other modes need:

| Mode | Needs |
|---|---|
| Design | Nested box layout, an artboard with its own cascade, drag-resize, breakpoints |
| Modeling | 3D viewport, gizmos, a node graph |
| Game | 2D/3D renderer, its own simulation as WebAssembly, profiler charts |
| Video | Decoded frames on a timeline, scrubbing, waveforms |
| Data | Virtualised tables, charts, notebook cells with rich output |
| Chat | Markdown, code blocks, streaming reflow |
| PDF | Pages drawn as they are reached, a text layer to select, overlays for comments and form fields |

In GPUI, every one of those is written from scratch. There is no table widget,
no markdown renderer, no chart primitive, no rich-text engine. And the surface
this project has already committed to — layered backdrop blur with saturation,
specular edges, `color-mix` theming, hairline dividers — is roughly a hundred
lines of CSS and a multi-pass downsample-blur shader chain plus a token system
in GPUI.

Two more things about GPUI specifically, worth knowing before committing:

- It is **not a stable public API**. It ships with Zed, for Zed. The
  documentation is Zed's source. You will be reading it, and you will be pinned
  to a commit.
- **Windows is its newest backend.** This machine is Windows 11. Being an early
  Windows GPUI adopter means finding its Windows bugs.

Neither is disqualifying. Both are cost, and they land on the platform you
actually develop on.

### What to do instead

**Do not pick the renderer yet. Pick the seam, and make the renderer swappable.**

```
        ┌──────────────────────────────────────────────┐
        │  proton-core   (pure Rust, zero UI deps)     │
        │  rope · tree-sitter · LSP/DAP · git · agent  │
        │  trace · index · wasm host · vim engine      │
        └───────────────────────┬──────────────────────┘
                                │  typed command/event API
                ┌───────────────┴───────────────┐
                │                               │
        ┌───────▼────────┐             ┌────────▼─────────┐
        │  webview shell │             │  GPU editor      │
        │  chrome + all  │   later →   │  surface, if     │
        │  eleven modes  │             │  measurement     │
        └────────────────┘             │  demands it      │
                                       └──────────────────┘
```

The vim engine belongs in the core, not the UI — it already dispatches resolved
actions (`Delete·Line`) rather than keys, which is exactly the boundary that
makes the front end replaceable.

Ship the webview shell first. It is what the prototype already is, so the design
work is not thrown away, and it is the only way one person gets eleven modes
built this decade. Then **measure the editor pane**: input-to-photon latency,
frame time at 4k lines with inlay hints and semantic tokens. If it fails, you
replace one pane — with a GPU surface driven by the same core — instead of
rewriting the app.

Be honest that this has a real cost: a DOM editor will not match Zed's latency,
and if that specific number is the thing you care about most, take GPUI and
accept a much smaller product. That is a legitimate choice. It is just a
different product than the one in `DESIGN.md`.

### If you go GPU-native anyway

The piece everyone underestimates is **text shaping**, not rasterisation.
Ligatures, Nerd Font glyphs, fallback chains, CJK, emoji, combining marks,
per-glyph colour from semantic tokens. Use `cosmic-text` (rustybuzz + swash +
fontdb) rather than writing it. Budget weeks, not days.

---

## Missing from the proposal

Five things Proton needs that a generic editor stack does not cover:

| | |
|---|---|
| **Agent runtime** | `tokio` + `reqwest` with SSE streaming, a tool-dispatch layer, and the shadow worktree. Use `gix` for git — it is pure Rust and does not drag in libgit2's threading model. |
| **Code Visualizer** | Upstream is Python and needs `gdb` with Python support. Proton has to supervise that process and ship a runtime — [`CODE-VISUALIZER.md`](CODE-VISUALIZER.md) covers it. On this machine, detect MSYS2 UCRT64 rather than asking. |
| **Video decode** | `ffmpeg` bindings. There is no pure-Rust path that handles real footage. |
| **3D** | `wgpu` directly for Modeling. For Game, this once said to embed an engine; it now has its own, because an embedded engine's runs are opaque to the agent and the niches it is for are narrow enough to build for: a fixed-point simulation in Rust (native and WebAssembly) and a WebGL2 renderer in the window. |
| **PDF** | pdf.js to draw (vendored; it already drew the viewer), `lopdf` to change — the version `pdf-extract` brought in, so nothing new was downloaded — and `tesseract` with Poppler's `pdftoppm` for OCR, run as processes in the sandbox. PDFium would draw faster but is a C++ binary per platform; pdfcraft (a Rust Acrobat) was the feature list, not a dependency — it patches its own lopdf and is not on crates.io. |

## The one-line version

Rust core, rope with UTF-16 summaries, tree-sitter, LSP/DAP, wasm plugins with
epoch interruption — all correct, take them. Keep the core UI-free so the
renderer stays a decision you can defer, ship the webview shell because seven
modes demand it, and hold GPUI in reserve for the editor pane once you have a
latency number that justifies it.

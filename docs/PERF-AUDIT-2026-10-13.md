# Proton performance audit — 13 October 2026

Proton is fast when it is idle and slow where it does heavy work. An idle heartbeat costs half a microsecond. Three kinds of work are slow, and in each case the cost grows with the size of what is open rather than with the size of the change:

- every PDF edit, which re-serialises and re-reads the whole document;
- every streamed agent token, which re-sends and repaints the whole conversation, four times over;
- a few subsystems that repeat a whole read or rebuild on every heartbeat.

Most of these are fixed by sending what changed rather than everything. None of them needs a rewrite.

## Method

Four read-only reviews ran in parallel:

- the core's event loop and the bridge to the window;
- the window's rendering;
- the heavy work paths;
- startup, memory and the build.

The claims that ranked highest were then checked against the code and, where possible, measured. Measurements used:

- a release build;
- a copy of a real 38 MB, 1,230-page textbook (*Computer Vision: Algorithms and Applications*), never the original;
- pdf.js in Chromium, through the preview;
- direct timing of the startup probes.

The machine was partly loaded by another build during the core measurements, so treat them as indicative.

Each finding says how sure it is:

- **Measured**: timed here.
- **Verified**: the cause was confirmed in the code, file and line given.
- **Reported**: found by code reading during the review, and not re-checked.

## The numbers

| What | Cost | How sure |
|---|---|---|
| One idle heartbeat (`Session::poll`), Codex or PDF mode | 0.4–0.6 µs, no events | Measured |
| Opening the 38 MB book (core) | 176 ms | Measured |
| One highlight or note (core, before the window does anything) | 150–260 ms each, flat over 10 notes | Measured |
| Undo / redo (core) | 164 / 147 ms | Measured |
| Saving as an update appended to the file | 174 ms | Measured |
| pdf.js re-opening the book after an edit (parse + first page) | 47 + 13 ms | Measured |
| The window rescanning every page's text after each edit (OCR's "pages with no text") | **2.05 s** on 1,230 pages | Measured |
| Walking the PDF library (27 books, 508 MB, warm cache) | 0.36 s on a worker | Measured |
| Six real covers, Poppler in the sandbox | 0.43 s on a worker | Measured |
| Login-shell PATH probe before the window opens | 0.13–0.15 s | Measured |
| `git status` of `~/Courses` (1,356 entries, warm) | 0.16 s | Measured (with the git CLI; the app uses gix) |

## Findings, most costly first

### 1. A PDF edit re-does the whole document, twice in the core and once in the window

**Measured. The cause was verified.**

Every highlight, note, rotation and form value runs through `Doc::apply`. Each one does the following.

**In the core:**
- It deep-clones the whole lopdf document (`crates/proton-pdf/src/doc.rs:104`).
- It serialises the whole file again as the view (`redrawn_as` → `view_bytes`, `crates/proton-core/src/pdf/mod.rs` ~474). The incremental path copies the file (`disk.to_vec()`), clones the document as it was on disk, and compares every object to find what changed (`crates/proton-pdf/src/save.rs:12-31`).
- It serialises the whole file **a second time**, only to say how large it would be (`info_event`, `pdf/mod.rs:874`, `d.bytes()`).
- All of this happens inside the session lock, so the terminal, the agent and every other command wait for it (reported).

**In the window:**
- pdf.js fetches and parses the new bytes. That part is cheap: 60 ms.
- Then `read()` → `reader.look(doc)` (`ui/pdf.js:513`, `ui/pdf-ocr.js`) runs `getTextContent` on **every page** to find pages with no text. That is 2 s on this book, on the same worker that draws pages, after every edit.
- An open search runs again too.

**Undo** clones the document as first opened and replays the whole log (`doc.rs:122-125`). Its cost grows with the number of edits since opening; ops that bring in another PDF or OCR's words re-parse their bytes each time.

**Form fields** pay the full core cost after every pause in typing, although the window ignores that view (`quiet`) (reported).

**Memory**: each open PDF holds about 4–5× its file size (`disk`, `origin`, `on_disk` and `current`, plus the view), with peaks near 9× during an edit (reported).

**The agent's PDF tools** drop their page-text cache on every version (`crates/proton-core/src/pdf/agent.rs:54`). A highlight followed by a search therefore re-extracts the whole book. The extraction also re-walks the page tree for each page (reported).

**Fixes, cheapest first:**
1. In `info_event`, use the view's length, which is already in hand, or leave the "as saved" size until Clean up asks for it. This halves the core's cost at once.
2. Keep OCR's "pages with no text" across versions. Only OCR, insert, delete, move and flatten change it.
3. Keep the agent's page text across versions unless the op touched page content or page order.
4. Build view bytes lazily and outside the lock: hand `read_document` an `Arc` of the document and serialise there. Quiet changes then cost nothing.
5. Make each `Op` return the object IDs it touched. Write the incremental update from that set (disk bytes, changed objects, xref with `/Prev`) instead of cloning and comparing everything.
6. Replace clone-and-replay undo with a snapshot every N ops, or the inverse patch of each op.
7. Draw comments as the window's own overlay (they already are, for choosing them) and render pages without their annotations. A comment then needs no reload at all.

### 2. A streaming agent re-sends and repaints the whole conversation, four times

**Verified.**

- **The heartbeat never rests during a turn.** While the agent is busy, `poll_agent` pushes `tasks_event()` on **every** poll (`crates/proton-core/src/workspace.rs:4497-4499`), changed or not. The shell's loop treats any non-empty batch as activity (`crates/proton-shell/src/main.rs:321-325`), so it runs at 60 Hz for the whole turn, including long stretches of model "thinking".
- **Every change sends the whole transcript.** Each one clones the entire transcript (`agent_event`, `workspace.rs:5453`).
- **The window redoes all of it.** It then `JSON.stringify`s every entry (`ui/app.js:6608`) and paints **all four** `[data-agent-thread]` copies: Codex's dock, Chat, LaTeX and Data. Each paint re-runs markdown, highlighting and KaTeX on the growing answer and forces a layout read and write per thread (`app.js:6609-6615`).
- **Cost:** total work is quadratic in the length of the answer, and the conversation's length multiplies it. Estimated at about 5 MB/s moved through IPC for a 40-entry chat (reported).

**Fixes:**
- Push Tasks only when its note or tool count changes.
- Send `AgentDelta { index, entry }` for the growing entry, and the whole transcript only when its length changes.
- In the window, build an entry's HTML once and paint only the visible thread, marking the others stale.
- Key entries by index, text length and state instead of `JSON.stringify`.

### 3. Things re-read or re-sent on every heartbeat

All reported, except where marked.

- **The Lab run being shown** (Research). While it runs, `poll_runs` sets `moved = true` whether or not it printed anything (`crates/proton-core/src/research/mod.rs:699-704`; the trigger is **verified**). `runs_event` then re-reads the investigation, every run's `run.json`, and the whole of `stdout.txt`/`stderr.txt` (`proton-lab/src/run.rs:489-494`), 60 times a second, under the lock. Over a long run this is quadratic.
  - **Fix:** send only when the logs' length or mtime moves, build only the `ResearchRun` event, and tail by seeking from the end.
- **Notebook cells.** Each kernel message for a running cell sends the whole notebook, base64 images included (`proton-core/src/data.rs:337-380`). The window rebuilds every cell's HTML (`app.js:3472`). While the agent waits on a cell, every poll clones every cell just to read a running flag (`workspace.rs:4959`).
  - **Fix:** per-cell events, outputs behind `Arc`, and a running set to check.
- **The terminal.** Every chunk of output rebuilds the whole screen as one `String` per cell and sends it (`workspace.rs:6200-6224`, `proton-term/src/lib.rs:357-401`). The window paints it synchronously, then forces a layout (`app.js:3679-3758`).
  - **Fix:** send changed rows only, and coalesce painting to one per animation frame.
- **The Notebook tutor's citations.** Every streamed poll resolves every citation again, reading each cited source's whole snapshot from disk (`proton-study/src/cite.rs:79`, `snapshot.rs:150`).
  - **Fix:** cache sources' words in the tutor, and resolve only new citations.

### 4. The window's rendering

- **One wheel listener for the whole window, not passive** (**verified**, `app.js:3858-3871`). It exists for the terminal. Marked `passive: false` on `document`, it makes WebKit send *all* scrolling (editor, PDF, chat, notebook) through the main thread, so scrolling stutters whenever 2 or 3 above are busy.
  - **Fix:** attach it to `[data-term-screen]`.
- **No frame coalescing.** Each batch is painted as it arrives (`app.js:4846`). For latest-wins events (agent, terminal, notebook, research), keep the last one and paint once per frame.
- **The PDF library rebuilds every shelf on every key typed in its search** (verified, `ui/pdf-library.js`, `render`). Every cover is re-inserted as a data URL and decoded again.
  - **Fix:** keep one node per book and toggle `hidden`.
- **PDF thumbnails are never released.** About 300 KB of canvas each at 2× scale, so roughly 200 MB after scrolling a 700-page sidebar (`ui/pdf.js:84`, `pdf-pages.js:74`).
  - **Fix:** keep about 150, least recently used, and let go of those that leave the screen.
- **PDF scrolling** recomputes the current page by reading every page's offsets each frame (`pdf-view.js:553-601`).
  - **Fix:** compute page tops from page sizes and binary-search them.
- **Smaller costs on every key:**
  - **Telescope** rebuilds all its rows on ↑/↓.
  - **The editor** reads `getBoundingClientRect` on every paint.
  - **The file tree** re-renders whole, with an O(n²) `indexOf`.
  - **Research's board** rebuilds whole on every round event.
- **Idle timers:**
  - the caret blink runs in every mode;
  - the quiz clock runs from boot;
  - Chat's status line rewrites its `innerHTML` every second, which restarts its shimmer.
- **CSS:**
  - In solid mode, a full-window `backdrop-filter: blur(30px) saturate(180%)` sits over four `blur(110px)` blobs, so any spinner recomposites a blurred window.
  - `ui.blur = 0` still installs `blur(0px)`, which keeps the layer.
  - No `contain` or `content-visibility` anywhere. Add `contain: strict` to the editor, terminal and agent threads, and `content-visibility: auto` to cells, messages and thumbnails.

### 5. Search indexes rebuilt from scratch

All reported, except where marked.

- **Rebuilt for every question.** Every tutor question in Passages mode, and every studio job, re-reads and re-indexes every source (`snapshot::library`, `proton-study/src/snapshot.rs:159`). Research does the same per call (`research/driver.rs:765`).
  - **Fix:** cache the `Library` keyed by `sources.json` and the sources' digests. `proton-mcp/src/notebook.rs:50-57` already does this.
- **The workspace library rebuilds whole on any note change** (checked every 20 s): it re-reads every note and every cached PDF text, then re-tokenises (`proton-library/src/shelf.rs:724`).
- **The embedding cache:**
  - with embeddings on, it is read and rewritten whole on each refresh;
  - while first embedding, it is rewritten after every batch (estimated at GB of writes);
  - `wanted_keys.contains` is O(n²) (`embed.rs:100-128`).
  - **Fix:** keep passages per document by (path, size, mtime), make the vector cache append-only, and use a `HashSet`.
- **The PDF library reads every book whole on its first walk** (**verified**): lopdf's `load_metadata` does `read_to_end` (`lopdf-0.42.0/src/reader.rs:176`). That was 508 MB for 27 books. It runs on a worker and is cached after, but a large library's first walk reads all of it.
  - **Fix:** memory-map the file and call `load_metadata_mem`.

### 6. Startup

- **Before the window exists**, the login-shell PATH probe runs `zsh -l` (**measured 0.13–0.15 s**; `proton-shell/src/main.rs:486`).
  - **Fix:** cache the PATH, keyed by the shell's rc files' mtimes, and refresh it in the background.
- **`Session::restore` builds every saved workspace eagerly** before `.build()`. Each does a `gix::discover`, about six JSON reads and a `Research::new` (reported).
  - **Fix:** build the active one; keep the rest as stubs until focused.
- **The first `Sync` runs a full git status of the workspace's repository** under the lock (`workspace.rs:6300`, `proton-vcs/src/lib.rs:134-150`). Four of your saved workspaces are subfolders of the `~/Courses` repository, so each status walks all of it (the git CLI takes 0.16 s warm; 1,356 entries). It runs again on mode switches.
  - **Fix:** send the branch at once, compute the changes on a worker, and pass the workspace's path as a pathspec.
- **The window parses 53 modules and 1.3 MB of JavaScript at boot**, and builds every mode. Notebook, PDF, Chat, Research, LaTeX, Lab and the GL terminal (about 800 KB) could be `import()`ed on first entry, as pdf.js, KaTeX and three already are.
  - The terminal also starts a login shell at boot.

### 7. The build

- **No `[profile.release]`** (**verified**, `Cargo.toml`): no LTO, 16 codegen units, symbols kept. The binary is 74 MB, 16 MB of it symbols (reported).
  - **Fix:** `strip = true` plus thin LTO and `codegen-units = 1`.
  - **Expected effect:** about 20 MB off, and some speed.
  - **Cost:** slower release builds.
  - **Keep unwinding:** `catch_unwind` in proton-pdf and the poisoned-lock recovery depend on it, so do not set `panic = "abort"`.
- **The Wasm toolchain** (cranelift, wasmtime, wasmparser: about 6.9 MB) is compiled in for plugins, and there are none installed.
  - **Fix:** put the plugin host behind a cargo feature.
- **Tree-sitter grammar tables** take about 12 MB.
- **rustls** adds about 2.6 MB where `native-tls` would use the system's.
- **Dev-only files** (bindings, preview stand-ins) ship in the bundle, because `frontendDist` is the whole `ui/`.

## What is already right

These need no work:

- the idle heartbeat;
- the shell's adaptive cadence (16 ms → 50 ms → 250 ms);
- the agent event's fingerprint;
- fuzzy file search narrowing as you type;
- grep on a worker;
- tree-sitter parsing incrementally and highlighting only the viewport;
- pdf.js, KaTeX and three loaded lazily;
- the terminal's 2,000-line cap;
- the PDF library and covers on workers, kept on disk.

## Suggested order

**First: quick, low-risk, and each removes most of its cost.**
1. PDF: the cheap size in `info_event`; OCR's blank pages kept across versions; the agent's page text kept across versions.
2. Agent: Tasks only when it changes; one thread painted, the rest marked stale.
3. Research: the Lab run's event only when its logs grow.
4. The wheel listener on the terminal only.
5. The PDF library's search toggling `hidden`; thumbnails least-recently-used.
6. `[profile.release]` with `strip = true` and thin LTO.

**Then: the larger changes.**
1. PDF engine: view bytes built lazily outside the lock; updates written from touched objects; undo by snapshots; comments as overlays.
2. Agent transcript deltas.
3. Notebook per-cell events.
4. Terminal changed rows; frame coalescing in the window.
5. BM25 libraries cached; the embedding cache append-only.

**Last: startup.**
1. Cached PATH.
2. Lazy workspaces.
3. Git status on a worker with a pathspec.
4. Modes imported on first entry.
5. The plugin host behind a feature.

## Measurements still worth taking

Take these in the installed app:

- **Lock and heartbeat timing.** Log the time spent under the lock in `dispatch` and `poll_once`, and the event types per batch. Do it with a Lab run shown, during an agent turn, and with a notebook cell printing a progress bar.
- **Push size.** Log `serde_json::to_vec(&events).len()` before `app.emit` during a long agent stream.
- **Memory.** Run `footprint $(pgrep proton-shell)` before and after opening a 100 MB PDF, and after one rotation.
- **Window traces.** Record a Web Inspector timeline during an agent stream: expect four markdown passes and four forced layouts per event.

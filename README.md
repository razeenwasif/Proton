<img src="brand/proton-icon.svg" width="88" align="right" alt="">

# Proton

A code editor that is also a chat client, a design tool, a 3D workspace, a game
editor, a video editor and a data notebook — one window, seven modes, Neovim
underneath all of them, and an agent that can act in any of them.

**M0 through M7 are built**, and Modeling has its document, outliner,
viewport and shader graph. The editor, Telescope and language servers, the
agent, the Code Visualizer, the plugin host, Data mode and Design mode all
ship, and Game reads and writes a Godot project's scenes. Video is the one
still to come, and running a game engine is a real project of its own.

```bash
cargo run -p proton-shell      # the app
cargo test --workspace         # 971 tests, headless
cargo tauri build              # the installer
node ui/serve.mjs              # the front end alone → http://localhost:5178
```

Best at 1440px or wider. Keys: `⌃1`–`⌃7` switch mode · `␣` leader · `⌘K` Telescope ·
`␣ff` files · `␣fg` grep · `␣fl` your library · `:` command line · `i` `v` `⎋` vim modes ·
`␣w` splits · `⌘,` settings. Bare digits are vim counts, not mode switches — that
collision is why the mode chord is `⌃`.

---

## What's here

| Path | |
|---|---|
| `crates/proton-core/` | The headless core — command/event bus, mode registry, modal input engine, editor |
| `crates/proton-text/` | Rope buffers, the undo tree, motions and text objects |
| `crates/proton-index/` | Telescope's index — file walking, fuzzy matching, grep |
| `crates/proton-syntax/` | Tree-sitter parsing and viewport-scoped highlighting |
| `crates/proton-lsp/` | The language-server client, and the supervision around it |
| `crates/proton-agent/` | The model client — Claude and Gemini, streaming, tool use |
| `crates/proton-library/` | Your own notes and PDFs — passages, BM25, and answers that cite the note and line or the paper and page |
| `crates/proton-mcp/` | The library, served over MCP to any client you already pay for |
| `crates/proton-data/` | Notebooks and the Jupyter kernel behind Data mode |
| `crates/proton-vcs/` | The gutter, and what git considers changed |
| `crates/proton-dap/` | The debug-adapter client |
| `crates/proton-term/` | The pseudo-terminal, and the screen it paints |
| `crates/proton-plugin/` | The Wasmtime host, WIT interfaces and capability grants |
| `crates/proton-trace/` | The Code Visualizer client |
| `crates/proton-design/` | Theme tokens and component markup, for Design mode |
| `crates/proton-game/` | Godot `.tscn` scenes, for Game mode |
| `crates/proton-shell/` | The Tauri v2 desktop shell and the IPC bridge |
| `brand/` | App icon — SVG master, PNGs 16→1024, `proton.ico`, and the monochrome mark. [Details](brand/README.md) |
| `ui/` | The front end — the app's real chrome, and the interactive prototype — all seven modes, four live-switchable themes, the AI dock, Telescope, which-key, settings, and the Code Visualizer pane |
| `design/DESIGN.md` | The design system, the mode-by-mode spec, and which reference each idea came from |
| `docs/ARCHITECTURE.md` | The build document — crate layout, the core/UI seam, the mode contract, milestones |
| `docs/STACK.md` | Language, renderer, parser, plugin runtime — what is settled and what is still a real decision |
| `docs/CODE-VISUALIZER.md` | Integration plan for [Code-Visualizer](https://github.com/razeenwasif/Code-Visualizer) |
| `assets/` | The reference designs the whole thing was drawn from |

## The seven modes

**Chat** · a full conversation with the model — threads on the left, a centred
reading column, and **Send to Codex** when it is time to act on what you decided.
**Codex** · the editor — buffers, LSP, splits, the agent dock, and the mobile
target's device / layout-inspector / Logcat panel.
**Design** · a web builder with a layer tree and an artboard, two-way bound to
real component source and design tokens.
**Modeling** · 3D — outliner, viewport and shader graph over a scene
serialised to diffable JSON. Objects are drawn as proxies; there is no mesh
loader yet, and the modifier stack is still to come.
**Game** · a Godot project's scene tree and an inspector that edits the
`.tscn` without reformatting it. The play transport, profiler, hot reload and
the agent that verifies changes by replaying recorded input all need an engine
process, and wait on one.
**Video** · media bin, program monitor, multitrack timeline, with generated
assets always marked as generated.
**Data** · notebook + SQL + dataframe grid + charts, with real cell lineage back
into the codebase.

## The three things that hold it together

**Modal editing is an application service, not an editor feature.** The vim
engine dispatches resolved actions — `Delete·Line` — and every mode implements
the same verb set. `dd` deletes a line in Codex, a layer in Design and a clip in
Video. Leader is `␣`, and Telescope is the spine: one fuzzy finder over files,
symbols, artboards, scene objects, cells and traces.

**Every document is a text file that diffs in Git.** Scenes, timelines and
artboards included. That is what lets the agent edit them by writing a patch
instead of driving a GUI, and what makes any of it reviewable.

**The agent proposes, you apply.** An edit it asks for is staged in memory and
surfaced as a diff, hunk by hunk — nothing is written to disk until you accept
it, so "I changed that file" is never something it can say truthfully before
you have looked. It shows its tool calls with results and timings, and what it
says about your notes carries a citation you can open.

## Your own library

The one piece that is not an editor feature. Point Proton at directories of your
own writing — a vault, course notes, a papers folder — and it cuts them into
passages, ranks them with BM25, and answers with a citation you can click:

```
[[01 - Systems/COMP2310 - The Memory Hierarchy.md § 4. Caching L124-143]]
[[Computer Systems A Programmer's Perspective.pdf § Chapter 6 p. 701]]
```

Notes are cited by line and PDFs by page, because a page is the only place in a
PDF a reader can be sent. `␣fl` searches it, the agent searches it, and it is
checked for changes while the editor runs.

Nothing is read until you list it in `.proton/settings.json`, and nothing outside
a listed directory is ever read — no symlink is followed out of one. A library
that could reach anywhere under your home directory is one that can put a medical
record in a prompt by accident.

**It also works outside Proton.** `proton-mcp` serves the same library over MCP,
so the CLI you already have a subscription to can search and cite your notes:

```bash
claude mcp add proton -- <path>/proton-mcp.exe
```

Retrieval is BM25 over local files, so it needs no key, costs nothing per query,
and works on a train.

## Status

**M0–M7 done.** Real: buffers, splits, Telescope, tree-sitter highlighting,
language servers, git gutters, the debugger, the terminal, the agent with tool
use and per-hunk review, the Code Visualizer, the Wasm plugin host, Data mode
with a live Jupyter kernel, the library above, and Design mode — a layer tree
read out of your component, an artboard that renders it in a sandboxed frame,
and edits that go both ways: a token or a layer changed here rewrites the file
by splicing the bytes that one thing occupies, and a save in Codex comes back
the other way.

Modeling is real too, up to a point worth stating: `scene.proton.json`, an
outliner over it, an inspector that writes it, a viewport you can orbit and
click, and a shader graph whose boxes and wires are the material in the file.
What it draws are *proxies* — a box where a mesh is, a frustum for a camera —
because the format holds transforms and no geometry. There is no mesh loader
and nothing renders a shader.

Game reads a Godot project: `project.godot` says which scene starts, the
hierarchy is that `.tscn`'s node tree, and the inspector edits it by splicing
the file Godot will open next. What it cannot do is run one — the play
transport, hot reload, the profiler and replaying recorded input all need an
engine process on the other end of a pipe, and none of them is mimed.

Not yet real: the Video mode, which is still a hand-built screen behind a live
statusline.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the milestones.

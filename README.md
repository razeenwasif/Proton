<img src="brand/proton-icon.svg" width="88" align="right" alt="">

# Proton

A code editor that is also a chat client, a design tool, a 3D workspace, a game
editor, a video editor and a data notebook — one window, seven modes, Neovim
underneath all of them, and an agent that can act in any of them.

**M0 is built.** The window opens, the modal engine is real, and every keystroke
round-trips through `proton-core`. Everything from M1 on is still a plan.

```bash
cargo run -p proton-shell     # the app
cargo test --workspace         # 86 tests, headless
node ui/serve.mjs             # the front end alone → http://localhost:5178
```

Best at 1440px or wider. Keys: `⌃1`–`⌃7` switch mode · `␣` leader · `⌘K` Telescope ·
`:` command line · `i` `v` `⎋` vim modes · `␣w` splits · `⌘,` settings. Bare digits are vim
counts, not mode switches — that collision is why the mode chord is `⌃`.

---

## What's here

| Path | |
|---|---|
| `crates/proton-core/` | The headless core — command/event bus, mode registry, modal input engine, editor |
| `crates/proton-text/` | Rope buffers, the undo tree, motions and text objects |
| `crates/proton-index/` | Telescope's index — file walking, fuzzy matching, grep |
| `crates/proton-library/` | Your own notes and PDFs — passages, BM25, and answers that cite the note and line or the paper and page |
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
**Modeling** · 3D — outliner, viewport, shader graph, modifier stack, scene
serialised to diffable JSON.
**Game** · hierarchy, play transport, inspector, profiler, hot reload, and an
agent that verifies changes by replaying recorded input.
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

**The agent proposes, you apply.** It works in a shadow worktree and surfaces
per-hunk diffs with Refer / Apply. Its scope is a chip you can point at, and it
shows its tool calls with results and timings so its claims can be checked.

## Status

**M0.5.** Real: the window (Mica-backed on Windows 11), the command/event seam,
the vertical mode rail, and the modal engine — counts,
operators, doubled operators, registers, visual mode, the leader, the command
line. Not yet real: buffers, language servers, model calls, renderers. The seven
modes are still hand-built screens behind a live statusline.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the milestones.

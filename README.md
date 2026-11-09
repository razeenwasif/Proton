<img src="brand/proton-icon.svg" width="88" align="right" alt="">

# Proton

A code editor that is also a data notebook, a research workbench, a
spreadsheet, a logic simulator, a logic workbench and a CAD program — one window,
seven modes, a shared Vim-style
modal engine underneath all of them, and an agent that can act in any of them.

Every mode in the window reads and writes a real document: the editor,
Telescope and language servers, the agent, the Code Visualizer, the plugin
host, Data's notebooks, Research's investigations,
Excel's workbooks, Circuit's projects and Logic's files all ship. The creative modes that were here — Design, Model, Game, Canvas, Diffuse
and Video — moved to an app of their own in October 2026, and LaTeX and the
Reader to Quark.

```bash
git submodule update --init    # the Code Visualizer, vendored underneath M4
cargo run -p proton-shell      # the app
cargo test --workspace         # headless tests; some integrations need external tools
cargo tauri build              # the installer
node ui/serve.mjs              # the front end alone → http://localhost:5178
npm ci && npm run check        # checked UI modules and generated command contract
npm run test:unit              # transport and text-row regression tests
npx playwright install chromium
npm run test:browser           # isolated browser harness and preview smoke test
```

`cmake` has to be on `PATH` first — tree-sitter builds its wasm runtime against
wasmtime's C API, and that half of it is CMake, not Cargo. `brew install cmake`
on macOS; MSYS2's `mingw-w64-*-cmake` on Windows; most Linux package managers
already call it `cmake`.

Panels can be collapsed from the rail, or in Codex from the title bar; text size, density and file tabs are in Settings. Keys: `⌃1`–`⌃7` switch mode · `␣` leader · `⌘K` Telescope ·
`␣ff` files · `␣fg` grep · `␣fl` your library · `:` command line · `i` `v` `⎋` vim modes ·
`␣w` splits · `` ⌃` `` terminal · `⌘,` settings. Bare digits are vim counts, not mode switches — that
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
| `crates/proton-research/` | Research's investigations — the files each is, and the co-scientist round that grows it |
| `crates/proton-literature/` | Research's literature — arXiv, Semantic Scholar and OpenAlex asked politely, citations checked, open PDFs kept |
| `crates/proton-lab/` | Research's Lab — the Python its code runs in, and Lean with Mathlib, made with your say-so |
| `crates/proton-vcs/` | The gutter, and what git considers changed |
| `crates/proton-dap/` | The debug-adapter client |
| `crates/proton-term/` | The pseudo-terminal, and the screen it paints |
| `crates/proton-plugin/` | The Wasmtime host, WIT interfaces and capability grants |
| `crates/proton-trace/` | The Code Visualizer client |
| `crates/proton-shell/` | The Tauri v2 desktop shell and the IPC bridge |
| `brand/` | App icon — SVG master, PNGs 16→1024, `proton.ico`, and the monochrome mark. [Details](brand/README.md) |
| `ui/` | The front end — the app's real chrome, and the interactive prototype — all eight modes, six live-switchable themes, the AI dock, Telescope, which-key, settings, and the Code Visualizer pane |
| `design/DESIGN.md` | The design system, the mode-by-mode spec, and which reference each idea came from |
| `docs/ARCHITECTURE.md` | The build document — crate layout, the core/UI seam, the mode contract, milestones |
| `docs/STACK.md` | Language, renderer, parser, plugin runtime — what is settled and what is still a real decision |
| `docs/CODE-VISUALIZER.md` | Integration plan for [Code-Visualizer](https://github.com/razeenwasif/Code-Visualizer) |
| `assets/` | The reference designs the whole thing was drawn from |

## The seven modes

Every mode keeps projects of its own: the folder open in Codex is not
carried into Data, Research or any other, and going back to a mode brings
back the one it had. The first time a mode is entered with nothing open in
it, it is given a folder of its own, `~/Proton/<Mode>`; opening a folder
there makes it that mode's. A folder open
in several modes is one project, listed by each.

See the [current capability matrix](docs/ARCHITECTURE.md) for implemented documents and runtime limitations.

**Codex** · the editor — buffers, LSP, splits, the agent dock, and the mobile
target's device / layout-inspector / Logcat panel. C and C++ are left as
written on save unless **Format C and C++ too** is on in Settings; then, and
with `␣lf` whenever asked, they are formatted by clangd, or by `clang-format`
— Xcode's, Homebrew's or LLVM's — where there is no clangd, with the
project's `.clang-format`. ⌘V pastes the clipboard as vim's `"+` would: as
copied in insert mode, before the cursor in normal mode, over the selection
in visual mode. Brackets and quotes close as
they are typed; Enter, `o` and `O` indent as the code calls for, and Tab is
as wide as the file's own indent — from its `.editorconfig`, else from its
lines. Saves are whole or not at all, and quitting with unsaved changes asks
first.
**Data** · Jupyter notebooks, drawn as Codex is: cells run in a kernel,
dataframes come back as tables beside them, and an Agent tab shares the side
with the table — it reads the notebook, runs its cells in your kernel, and
proposes changes you accept or reject on the cells themselves. A notebook's
unsaved changes are kept: saved when you open another, close the project or
quit. Code cells are coloured as you type them, and prose cells are drawn as
the agent's answers are, TeX included. SQL cells, charts and lineage back
into the code are not built yet.

Cells run in a Jupyter kernel, in the first of these that has one:
`PROTON_PYTHON`; the project's `.venv`, `venv` or `.env`; the Jupyter kernel
the notebook names (`metadata.kernelspec`), as `jupyter kernelspec list`
shows it; then the first Python on your login shell's PATH that has
`ipykernel`. A kernel that stops because its Python has no ipykernel says
how to add it. A cell's `!pip` is the kernel's own, so it installs where
`import` looks (`%pip` works too). For a project of its own:

```bash
python3 -m venv .venv && .venv/bin/python -m pip install ipykernel
```

**Research** · investigations, after Google's AI co-scientist, put to Claude
Code, Codex and Antigravity together — physics and mathematics first, and
chemistry, biology and computer science beside them. In a round each proposes
hypotheses, another model family reviews each one, a tournament judged blind
ranks them by Elo, the best are evolved, and an overview rewrites the plan.
An investigation is a folder, `research/<slug>/`: the brief, the plan, each
hypothesis and review, the tournament one match to a line — files that diff
in Git and can be edited by hand. Research works in the project Data has open,
or, to keep every investigation in one place whichever project is open, in a
folder of their own — chosen under the list with Change, and kept in your own
settings, `~/.config/proton/settings.json`, as
`{"research": {"folder": "~/Github/Research"}}` — each investigation a folder in
it beside whatever else is there.
Run round starts one — about fourteen agent calls, never more than twenty — and
the Activity tab follows it call by call, each one opening into what it said;
what you type goes to the team, `/h Title: statement` adds a hypothesis of
your own, and `/p` with a DOI or an arXiv id adds a paper. Fork copies an
investigation to take another direction — from the end of any round, or as it
stands — with the direction you give as its team's first word; the copy says
where it came from, and the original is left as it is. The Literature tab
searches arXiv, Semantic Scholar and OpenAlex — told only your queries, never
your files — and keeps each paper as `literature/P001.md`, with its open PDF
beside it and out of Git. Each round searches it first, with the plan's or the
last overview's queries, and every seat proposes its hypotheses knowing what
was found — Claude Code and Codex searching the papers' full text through
Proton's own MCP server, Antigravity given the passages that matter — and a
citation of a page, `[[literature/P003.pdf p. 4]]`, opens the PDF there. Every
DOI and arXiv id the team cites is checked with arXiv, Crossref and the DOI
system, and one that cannot be found is marked, said, and told to the team; Semantic Scholar and OpenAlex give more to a free key,
read from your shell's `SEMANTIC_SCHOLAR_API_KEY` and `OPENALEX_API_KEY` and
never stored. The Lab's Python — Python 3.12 with numpy, scipy, sympy, mpmath,
matplotlib, networkx and pandas, from conda-forge, apart from your own
environments; or, with no conda, a virtual environment of a Python 3.9 or newer
already here, the same packages from PyPI — is made with your say-so twice:
once to plan it, and once to make exactly that plan, every package and its
download in front of you. Code an agent proposes waits in the Lab for you to read; run, it is
in a sandbox — Seatbelt on macOS, bubblewrap on Linux: no network, nothing
written outside its own folder, nothing of your home folder read — and its
folder keeps what it printed, the numbers and pictures it saved, and a record
of the code and environment by digest and of who approved it. Its pictures are
drawn in the Lab, and its 3D scenes — spheres and bonds, points, lines, arrows
and meshes, in frames for anything that moves — open there in three.js to be
turned and played. So do the molecules it writes: XYZ (a trajectory's frames
too), PDB, mmCIF, MOL, SDF and SMILES, laid out in 3D from its graph — in ball
and stick, sticks or spacefill, and a protein as a cartoon of helices and
strands; click an atom to say what it is, and another for how far apart. What a run showed goes back to the team: a seat of another
model family reviews it against its hypothesis, in the round or the next. In
mathematics, the leading hypothesis is stated in Lean 4 with Mathlib each round
and proved — or its negation — by a seat of another family, Lean's errors going
back to it until it is proved or its three calls are spent; Lean checks every
file in the sandbox, a proof counts only on Lean's standard axioms, and the
hypothesis shows Lean's own statement of what was proved beside its own. Lean and
Mathlib are Proton's, in `~/.proton/lean` (about 15 GB), made with your say-so
twice like the Lab's Python.
Design, Model, Game, Canvas, Diffuse and Video, once modes here, are an app
of their own now, LaTeX and the Reader are Quark's, and Chat, once the first
mode here, and Notebook have given way to Fairy. Codex
still edits `.tex` and `.bib` files, with texlab when it is on `PATH`, and
PDFs still open in the viewer from any mode.

**Excel** · a spreadsheet on ⌃4, after Google Sheets and Excel. A workbook
is a `Name.sheet/` folder of text that diffs — `workbook.json` and a
`.cells` file a sheet, a line a cell — and `.xlsx`, `.csv` and `.tsv` open
and save as themselves. Formulas are worked out by Proton's own engine
(`crates/proton-sheet`): 140 functions, Excel's grammar, references
across sheets and names, and only what a change reaches worked out again,
cycles shown as `#CYCLE!`. The grid draws the rows on screen and no more;
Vim's keys move and act (`v`, `V`, `y`, `d`, `p`, `dd`, `o`, `:sort`,
`:filter`, `:freeze`), and a spreadsheet's keys and mouse do too — typing
starts a cell, the fill handle fills, a letter's edge widens its column.
`/` or `⌘F` finds text in what cells hold or show, its cells marked and
walked with `n` and `N`, and `⌘⌥F` replaces it, one cell or all as one undo;
a formula it would leave unreadable is left as it was, and said.
Number formats, bold and colour, frozen panes, sorting, filters, colour
rules, and line, bar, scatter and pie charts drawn over the cells and kept
in `.xlsx`. The agent reads, writes and charts the workbook with its sheet
tools, `@sheet` sends what is selected, and a range goes to Data as a
dataframe and a notebook's table comes back as a sheet. Edits are held until
⌘S, and quitting with an unsaved workbook asks first.

**Circuit** · digital logic on ⌃5, after hneemann's
[Digital](https://github.com/hneemann/Digital). A project is a
`Name.circuit/` folder of text — a `.circ` file a circuit, a line a part or
a wire, test tables, memory contents as `.hex` and state machines — and
Digital's `.dig` files open as projects of their own. Gates, flip-flops,
registers, counters, memories, multiplexers, arithmetic, splitters, tunnels
and displays are placed, wired, turned and copied with the keys or the
mouse, and circuits placed in one another. Proton's simulator
(`crates/proton-circuit`) runs them as Digital does, a gate's delay at a time
when asked, the wires coloured by their values and inputs clicked; a quick
run clocks a CPU at hundreds of kilohertz until a Break rises. Test tables in
Digital's language run against a circuit and its signals are graphed; truth
tables, minimised expressions (Quine–McCluskey) and Karnaugh maps come from
a circuit, and circuits from expressions or from a state machine written as
text. Out go Verilog, VHDL and SVG. The agent reads, edits, tests and
analyses circuits with its circuit tools, and `@circuit` sends the circuit on
screen. Edits are held until ⌘S.

**Logic** · on ⌃6, a calculator after
[logic-calculator.com](https://logic-calculator.com/) and a model finder
after John Slaney's [Logic for Fun](https://logic4fun.comp.anu.edu.au/), with
proofs. A `.logic` file is a worksheet or a theory in Proton's language, a
`.l4f` file Logic4Fun's, and a `.proof` file a proof; all are text. The line at
the cursor is answered as you type: a formula's truth table, whether it is a
tautology, its normal forms, least sum of products and Karnaugh map; an
argument's validity, with a countermodel or its semantic tableau drawn; two
formulas' equivalence. A theory's models are found and counted
(`crates/proton-logic`, eight queens' 92 in 16 ms), and when there is none,
the fewest lines that cannot all hold are marked. Proofs in natural
deduction (Fitch), the sequent calculus and resolution are checked line by
line, each mistake named, and found by Proton — always for propositional
logic. A formula's table can be built in Circuit. The agent asks, solves,
proves and writes with its logic tools, and `@logic` sends the file and the line
at the cursor. Edits are held until ⌘S.

**CAD** · on ⌃7, parametric parts on a kernel of Proton's own
(`crates/proton-cad`, planned in [`docs/CAD.md`](docs/CAD.md)). A `.part` file
is text: parameters, sketches of lines, arcs and circles held by constraints
and dimensions, and features made from them — extrudes and revolves joined
to, cut from or met with a body, holes tapped, counterbored or countersunk,
patterns and mirrors, edges rounded or chamfered, bodies shelled, profiles
swept along paths and lofted — on booleans of Proton's own, each body
checked closed and its volume integrated exactly. The steps are
listed on the left, each with its mistake; the part is drawn in 3D; a sketch
opens flat on its plane, where the mouse draws and holds shapes, a dimension
is typed over in place and a point dragged with the solver holding the rest,
and the solver says how much is still free; in 3D an edge under the pointer
is picked to round or chamfer. Every edit is a line of the file,
which the Text tab shows and takes typing. Bodies are written as STL or 3MF
for a printer, or as STEP, exact, for a machinist or another CAD program.
An `.assembly` places parts by mates — two faces picked and held flat,
flush, on one axis, a gap or an angle apart — solved by the same solver,
which says what can still move; it checks where parts overlap or come too
near, lists the parts in a bill of materials, and takes itself apart for an
exploded view. A library makes parts from published standards — a Mini-ITX
board on ATX's holes, SFX supplies, graphics cards, fans, radiators, pumps,
G1/4 fittings, ISO screws, the FormD T1 and its limits — each written into
the folder as a part file like any other. Routes run hard or soft tube
between fittings' sockets, bent to the tube's radius, with the length to
cut and where to bend it, and draw the loop in the order the coolant goes.
A `.drawing` lays a part or an assembly out on a sheet: views projected with
the edges behind dashed, sections hatched, details, dimensions measured on
the model, a title block; for an assembly, its blueprint — four sides, an
exploded view, a section, every part ballooned to the bill of materials and
the tightest gaps measured — on paper or white on blue. Views are dragged
where they go, and the sheet is written as PDF, SVG or DXF. The agent, on
CAD's Agent tab or anywhere with `@cad`, reads, writes, measures and checks
parts, assemblies and drawings — a build against the FormD T1's published
limits too — and what it writes is held like your own. A `.schematic` is
the start of a custom board: parts from a library that has the PC's own
connectors (ATX, EPS, PCIe, SATA, fan, ARGB and the front-panel USB
headers), nets that say the current they carry, the electrical rules
checked, the sheet drawn with each pin's net, and a netlist for KiCad. Edits are held until
⌘S.

## The three things that hold it together

**Modal editing is an application service, not an editor feature.** The vim
engine dispatches resolved actions — `Delete·Line` — and every mode implements
the same verb set. `dd` deletes a line in Codex and a cell in Data. Leader is
`␣`, and Telescope is the spine: one fuzzy finder over files, symbols, cells
and traces.

**Every document is a text file that diffs in Git.** Notebooks,
investigations and conversations included. That is what lets the agent edit them by writing a patch
instead of driving a GUI, and what makes any of it reviewable.

**The agent writes, and you can take it back.** A file it changes or creates
is written in the project at once, and its row in the pane has an Undo that
puts it back as it was — if nothing has changed it since. With "Let the agent
write files" off, its edits are staged as a diff, hunk by hunk, for you to
accept; a file with unsaved changes of yours, or outside the project, is always
staged. A command it wants to run — a build, a test, an install — waits on its
row for you to Run or Skip, and what it printed goes back to it. Asked in Codex, a question carries the file you have open — once, then only
when it has changed — named by a chip you can click to stop it. It shows its tool calls with results and timings, and what it
says about your notes carries a citation you can open. Its answers are
markdown: code comes in blocks coloured the way the editor colours that
language, with a button to copy it, and TeX (`$…$`, `\(…\)`, `$$…$$`, `\[…\]`)
is drawn as equations by [KaTeX](https://katex.org), vendored in
`ui/vendor/katex/` and loaded the first time an answer has one. Each project's
threads are files in its `.proton/chats/`; the clock in the agent header lists
them, and the one open comes back after a restart.

## Codex agent connection

In **Settings → AI & models**, select **Codex** and **gpt-6-astra** (also the
Codex default). Proton uses your existing Codex sign-in; credentials are managed
by Codex and are never copied into project settings. On macOS it prefers the
Codex runtime bundled with the desktop app. Other installations can supply a
current `codex` executable on PATH or set `PROTON_CODEX_BIN` to its full path.

The connection uses the [Codex app-server protocol](https://learn.chatgpt.com/docs/app-server).
Streaming replies and tool calls appear in Proton. Workspace tools retain
Proton's file boundaries and patch review. Codex runs without an attached
environment; unsupported permission requests fail visibly instead of being
approved automatically. A recent runtime with experimental dynamic tools is
required. Each question starts an ephemeral Codex thread with Proton's saved
conversation; this adapter does not resume Codex desktop tasks.

## The engines

Research runs on the real CLIs, each on its own sign-in,
driven over the protocol it offers hosts — your account, your settings and your
subscription, not an API key:

| Agent | Runs | Sign in with |
| --- | --- | --- |
| Claude Code | `claude`, stream-json mode | `claude` |
| Codex | `codex app-server` | `codex login` (ChatGPT plan) |
| Gemini | `gemini --experimental-acp` | `gemini` (Google) |
| Antigravity | `agy`, stream-json print mode | `agy` |

Proton finds each in `~/.local/bin`, on your login shell's PATH, or at
`PROTON_CLAUDE_BIN`, `PROTON_CODEX_BIN`, `PROTON_GEMINI_BIN` or `PROTON_AGY_BIN`.

## Antigravity agent connection

A Google AI subscription has no API key, so the **Gemini** provider cannot use
it. **Antigravity** can: in **Settings → AI & models**, select it, and Proton
runs Antigravity's `agy` command on your own Google sign-in. Run `agy` once in a
terminal to sign in. Proton finds it in `~/.local/bin`, where the Antigravity
installer puts it, or on PATH, or inside the Gemini desktop app
(`Gemini.app/Contents/Helpers/agy`), or at
`PROTON_AGY_BIN`. Gemini 4 Argon (High)
is the default, and `agy models` lists the rest. Research gives its
Antigravity seat the newest Gemini Antigravity offers, at its highest
thinking level.

Unlike Codex, `agy` cannot be handed Proton's tools for a single run, so it
works in the project with its own. Its steps appear in the agent pane as they
happen. It may always read and search. It may not run commands. It may not
write files either, until **Let Antigravity edit files** is turned on. That
switch is the one exception to *the agent proposes, you apply*: with it on,
edits go straight to disk, not through the diff review. Open files it changed
are re-read when the turn ends, unless they have unsaved work in them.
`agy` also reads your own global instructions, `~/.gemini/GEMINI.md`, as it does
anywhere else.

## Your own library

The one piece that is not an editor feature. Point Proton at directories of your
own writing — a vault, course notes, a papers folder — and it cuts them into
passages, ranks them with BM25, and answers with a citation you can click:

```
[[01 - Systems/COMP2310 - The Memory Hierarchy.md § 4. Caching L124-143]]
[[Computer Systems A Programmer's Perspective.pdf § Chapter 6 p. 701]]
```

Notes are cited by line and PDFs by page, because a page is the only place in a
PDF a reader can be sent — and a citation opens Proton's own viewer at it.
`␣fl` searches it, the agent searches it, and it is checked for changes while
the editor runs. Any PDF opens in the viewer, from the tree or `␣ff` in any
mode: its pages drawn as you reach them, its text selectable, `j`/`k` and
Space to read, `+`/`−` and `0` to zoom and fit, Escape to put it away. To
annotate or rearrange one, open it in Quark.

Nothing is read until you list it in `.proton/settings.json`, and nothing outside
a listed directory is ever read — no symlink is followed out of one. A library
that could reach anywhere under your home directory is one that can put a medical
record in a prompt by accident.

**It also works outside Proton.** `proton-mcp` serves the same library over MCP,
so the CLI you already have a subscription to can search and cite your notes. It
ships with the app — in `Proton.app/Contents/Resources/` on a Mac, beside
`Proton.exe` on Windows, in `/usr/lib/Proton/` on Linux:

```bash
claude mcp add proton -- <path>/proton-mcp
```

Retrieval is BM25 over local files, so it needs no key, costs nothing per query,
and works on a train.

## Status

**M0–M7 done.** Real: buffers, splits, Telescope, tree-sitter highlighting,
language servers, git gutters, the debugger, the terminal, the agent with tool
use and per-hunk review, the Code Visualizer, the Wasm plugin host, Data mode
with a live Jupyter kernel, and the library above.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the milestones.

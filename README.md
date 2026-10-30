<img src="brand/proton-icon.svg" width="88" align="right" alt="">

# Proton

A code editor that is also a chat client, a data notebook, a research
workbench, a study notebook, a spreadsheet, a logic simulator and a logic
workbench — one window, eight modes, a shared Vim-style
modal engine underneath all of them, and an agent that can act in any of them.

Every mode in the window reads and writes a real document: the editor,
Telescope and language servers, the agent, the Code Visualizer, the plugin
host, Data's notebooks, Research's investigations, Notebook's sources,
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

Panels can be collapsed from the rail, or in Codex from the title bar; text size, density and file tabs are in Settings. Keys: `⌃1`–`⌃5` switch mode · `␣` leader · `⌘K` Telescope ·
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

## The eight modes

Every mode keeps projects of its own: the folder open in Codex is not
carried into Data, Research or any other, and going back to a mode brings
back the one it had. The first time a mode is entered with nothing open in
it, it is given a folder of its own, `~/Proton/<Mode>`; opening a folder
there makes it that mode's. A folder open
in several modes is one project, listed by each.

See the [current capability matrix](docs/ARCHITECTURE.md) for implemented documents and runtime limitations.

**Chat** · Claude Code itself — your `claude`, its tools, permissions,
sessions and Artifact tool — drawn as Claude Code's app draws it, with what it
publishes in a pane beside the conversation. Codex, Gemini and Antigravity
answer in the same tab.
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
**Notebook** · Gemini Notebook for studying, being built: notebooks of your own
sources — a vault, a courses folder, an iCloud archive — a tutor on Claude Code,
Codex, Gemini or Antigravity that answers from them and cites them, and a
studio that makes study guides, flashcards, quizzes and exams of them. So far:
notebooks, each a folder in `~/Github/Notebooks`; sources chosen from your
folders in a picker — browsed, searched by name and by word, a folder's files
ticked all at once (its subfolders' too, if you like), with what is left
out and why; a lab folder added whole as one source — its README, code and
Makefiles read together and cited by file and line, from the picker or by its
path, read again when it changes — or found for you as what is like the notebook, or found on the
web by Discover or Deep Research, which writes a report of what it read — and read once,
their words kept in the notebook: notes, PDFs, notebooks, LaTeX, captions, Word
documents and EPUBs (with pandoc), PowerPoints, pictures' words (with
tesseract), pages of the web kept as their articles, your artifacts — read as
their articles, KaTeX's formulas read back as TeX, and opened as themselves —
YouTube videos' captions (with yt-dlp), and recordings, transcribed by Fish Audio when you
choose, told what it costs; a reader, with a guide to each source on asking;
sources labelled by topic, and ticked by label; slides with few words read
from their pictures; notes;
and the tutor, which searches the sources you tick through Proton's own MCP
server and cites them — each citation a number whose passage shows over it and
opens at its place — as a plain tutor, a learning guide, or a role you write,
in any language; a studio that makes study guides, briefings, FAQs, glossaries,
timelines, mind maps, tables and learning objectives of the sources ticked,
each citing them; flashcards, scheduled by FSRS so they are remembered by the
exam, reviewed with the keys 1–4, and exported for Anki; and quizzes and
practice problems — choices, blanks, numbers and written answers — marked as you
answer, a written answer by another family of model against its rubric, each
mark quoting the words that earned it and open to dispute, with how you did by
learning objective; and exams — a new paper set like your past papers, never one
of theirs reworded, or a past paper itself marked by its solutions — sat against
the clock with every answer saved as you write it, and printed; and your
progress by learning objective, with a diagnostic quiz, a lesson on any gap
with the tutor as a guide, and a Learning Overview that takes the course topic by
topic; and audio overviews — a deep dive, a brief, a critique, a debate or a
lecture — spoken by Fish Audio in voices you choose from its library, your own
clones, or one by its link (with `FISH_API_KEY` exported in your shell, which
Proton reads and never keeps), with a transcript that follows along; slide decks with speaker notes, revised a slide at a time,
presented, and written out for PowerPoint; infographics and mind maps saved
as pictures; and coding practice — problems in Python, C, C++, Java,
JavaScript or Rust, whichever this machine has the compilers for, each solved in
an editor beside it and built and run against its tests, sandboxed, with hints
and a solution when you ask; every problem's own solution is run against its
tests before you see it. Python's problems run with the Python you choose — a
conda environment, a venv — and may use the libraries it has, NumPy, SciPy or
PuLP and its solver among them. Your folders are named in your own settings,
`{"study": {"roots": ["~/Notes", "~/Courses"], "exclude": ["Notes/Private/"]}}`,
and catalogued in under a second without opening a file. A file still in
iCloud is listed under a cloud and never read until you choose to download it,
having been told its size.
Design, Model, Game, Canvas, Diffuse and Video, once modes here on ⌃6–⌃-,
are an app of their own now, and LaTeX and the Reader are Quark's. Codex
still edits `.tex` and `.bib` files, with texlab when it is on `PATH`, and
PDFs still open in the viewer from any mode.

**Excel** · a spreadsheet on ⌃6, after Google Sheets and Excel. A workbook
is a `Name.sheet/` folder of text that diffs — `workbook.json` and a
`.cells` file a sheet, a line a cell — and `.xlsx`, `.csv` and `.tsv` open
and save as themselves. Formulas are worked out by Proton's own engine
(`crates/proton-sheet`): 140 functions, Excel's grammar, references
across sheets and names, and only what a change reaches worked out again,
cycles shown as `#CYCLE!`. The grid draws the rows on screen and no more;
Vim's keys move and act (`v`, `V`, `y`, `d`, `p`, `dd`, `o`, `:sort`,
`:filter`, `:freeze`), and a spreadsheet's keys and mouse do too — typing
starts a cell, the fill handle fills, a letter's edge widens its column.
Number formats, bold and colour, frozen panes, sorting, filters, colour
rules, and line, bar, scatter and pie charts drawn over the cells and kept
in `.xlsx`. The agent reads, writes and charts the workbook with its sheet
tools, `@sheet` sends what is selected, and a range goes to Data as a
dataframe and a notebook's table comes back as a sheet. Edits are held until
⌘S, and quitting with an unsaved workbook asks first.

**Circuit** · digital logic on ⌃7, after hneemann's
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
analyses circuits with its circuit tools, `@circuit` sends the circuit on
screen, and Notebook makes circuit practice from your sources, each problem
built and checked by the simulator before you see it. Edits are held until
⌘S.

**Logic** · on ⌃8, a calculator after
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
proves and writes with its logic tools, `@logic` sends the file and the line
at the cursor, and Notebook makes logic practice, each problem checked by
the engine before you see it. Edits are held until ⌘S.

## The three things that hold it together

**Modal editing is an application service, not an editor feature.** The vim
engine dispatches resolved actions — `Delete·Line` — and every mode implements
the same verb set. `dd` deletes a line in Codex and a cell in Data. Leader is
`␣`, and Telescope is the spine: one fuzzy finder over files, symbols, cells
and traces.

**Every document is a text file that diffs in Git.** Notebooks,
investigations and conversations included. That is what lets the agent edit them by writing a patch
instead of driving a GUI, and what makes any of it reviewable.

**The agent proposes, you apply.** An edit it asks for is staged in memory and
surfaced as a diff, hunk by hunk — nothing is written to disk until you accept
it, so "I changed that file" is never something it can say truthfully before
you have looked. It shows its tool calls with results and timings, and what it
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

## Claude Code, in the Chat tab

The Chat tab runs the real Claude Code. Proton starts your own `claude`
command in the project, in the stream-json mode Claude Code's SDKs use, so it
is your account, your settings and CLAUDE.md, your skills and connectors, and
every tool Claude Code has. Run `claude` once in a terminal to sign in. Proton
finds it in `~/.local/bin`, where Claude Code's installer puts it, on your login
shell's PATH, or at `PROTON_CLAUDE_BIN`. Replies are drawn as the agent pane's
answers are, coloured code and equations included, and so is the code in what
its tools did: a file read, an edit's diff, a command.

What Claude Code would ask in a terminal — may it run this command, edit this
file, go ahead with this plan, which of these did you mean — is asked as a card
in the conversation. The permission mode and the model are pills on the
composer; ⇧⇥ steps the mode. The sessions on the left are every session Claude
Code keeps for the folder, including ones started in a terminal or in the
desktop app, and any of them can be picked up again. Under them, **Claude
desktop** lists the desktop app's sessions from its other folders; one opens in
its own folder, and one the desktop app has open right now (marked with a ring)
carries on as a copy, so two programs never write to one session.

**Folders.** Group chats into folders, as in Claude's own apps: the folder
button beside **New session** makes one and puts its name up for editing,
and a chat moves into a folder by dragging it there or from its **⋯** menu
(right-click works too). A folder folds, and still shows a spinner or a dot
when something in it is working or waiting on you. Folders are shared by
every project, and hold chats from any of them — Claude Code's, the other
engines', the desktop app's. Deleting a folder puts its chats back in the
list; it never deletes them.

**Deleting a chat.** **Delete chat…** in a chat's **⋯** menu asks first, then
stops it if it is running and moves its transcript to the Trash — with the
folder beside it where Claude Code keeps its subagents' logs — so it can be
put back. A chat open in another Claude Code can't be deleted here until it is
closed there, since that program would only write it back.

**Codex, Gemini and Antigravity.** The pill at the left of the composer picks
who answers a new conversation. Each is the real CLI on its own sign-in,
driven over the protocol it offers hosts, and drawn with the same rows, cards
and pills:

| Agent | Runs | Sign in with | Asks before acting | Picks a conversation up again |
| --- | --- | --- | --- | --- |
| Codex | `codex app-server` | `codex login` (ChatGPT plan) | yes, as cards | yes, its own thread |
| Gemini | `gemini --experimental-acp` | `gemini` (Google) | yes, as cards | told what was said, as the CLI cannot load a session |
| Antigravity | `agy`, stream-json print mode | `agy` | no: its mode decides, and refusals are listed when it finishes | yes, its own conversation |

Their conversations are kept in Proton's data folder (`dev.razeen.proton/chats`,
or `PROTON_CHATS`) and listed with the project's sessions. `PROTON_CODEX_BIN`,
`PROTON_GEMINI_BIN` and `PROTON_AGY_BIN` point at the commands if they are
somewhere unusual.

**Artifacts.** When Claude publishes one with its Artifact tool, it opens in the
pane on the right. Each chat has its own pane: switch chats and the pane
shows what that chat had open — or nothing — and switch back to find yours
as you left it. One a chat publishes while you are in another is waiting open
when you go back to it, and the list puts the chat's own artifacts first. Proton keeps a copy of every version it publishes in
`~/Library/Application Support/dev.razeen.proton/artifacts` (`PROTON_ARTIFACTS`
moves it) — with the files each page brought along, its clips and pictures
found where the Artifact tool found them, and carried into each new version as
claude.ai carries them — and serves them from `artifact://`, sandboxed: a page
can load what a claude.ai artifact can, play its own videos (answered a range
of bytes at a time, as a player asks) and go full screen, and reach nothing of
the editor's. **Open on claude.ai** goes to the published page. **Import from
Claude**, in the artifacts list, brings in what Claude Code published from this
machine before Proton was keeping it — found in the desktop app's session
records and in every session's own file, with the files each page brought
along — and gives a version kept before without its files the ones it names.
One whose page has since been deleted comes in as its address, and opens on
claude.ai. Notebook takes artifacts as sources too.

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
`Proton.exe` on Windows, in `/usr/lib/Proton/` on Linux — and `/council` hands
it to each member whose CLI can take one for a single run:

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

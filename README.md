<img src="brand/proton-icon.svg" width="88" align="right" alt="">

# Proton

A code editor that is also a chat client, a data notebook, a research
workbench, a design tool, a 3D workspace, a game editor and a video editor —
one window, nine modes, a shared Vim-style modal engine underneath all of
them, and an agent that can act in any of them.

**M0 through M7 are built**, and every mode in the window now reads and writes
a real document. The editor, Telescope and language servers, the agent, the
Code Visualizer, the plugin host, Data mode and Design mode all ship; Modeling
has its scene, outliner, viewport and shader graph; Game reads and writes a
Godot project's scenes; Video reads and writes a cut and its captions. What is
still to come is the machinery underneath two of them — a game engine and
ffmpeg — and each of those is a real project of its own.

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

Panels can be collapsed from the rail, or in Codex from the title bar; text size, density and file tabs are in Settings. Keys: `⌃1`–`⌃8` switch mode · `␣` leader · `⌘K` Telescope ·
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
| `crates/proton-design/` | Theme tokens and component markup, for Design mode |
| `crates/proton-game/` | Godot `.tscn` scenes, for Game mode |
| `crates/proton-shell/` | The Tauri v2 desktop shell and the IPC bridge |
| `brand/` | App icon — SVG master, PNGs 16→1024, `proton.ico`, and the monochrome mark. [Details](brand/README.md) |
| `ui/` | The front end — the app's real chrome, and the interactive prototype — all nine modes, six live-switchable themes, the AI dock, Telescope, which-key, settings, and the Code Visualizer pane |
| `design/DESIGN.md` | The design system, the mode-by-mode spec, and which reference each idea came from |
| `docs/ARCHITECTURE.md` | The build document — crate layout, the core/UI seam, the mode contract, milestones |
| `docs/STACK.md` | Language, renderer, parser, plugin runtime — what is settled and what is still a real decision |
| `docs/CODE-VISUALIZER.md` | Integration plan for [Code-Visualizer](https://github.com/razeenwasif/Code-Visualizer) |
| `assets/` | The reference designs the whole thing was drawn from |

## The nine modes

See the [current capability matrix](docs/ARCHITECTURE.md) for implemented documents and runtime limitations.

**Chat** · Claude Code itself — your `claude`, its tools, permissions,
sessions and Artifact tool — drawn as Claude Code's app draws it, with what it
publishes in a pane beside the conversation. Codex, Gemini and Antigravity
answer in the same tab.
**Codex** · the editor — buffers, LSP, splits, the agent dock, and the mobile
target's device / layout-inspector / Logcat panel. C and C++ are formatted by
clangd, or by `clang-format` — Xcode's, Homebrew's or LLVM's — where there is
no clangd, with the project's `.clang-format`.
**Data** · notebook + SQL + dataframe grid + charts, with real cell lineage back
into the codebase, drawn as Codex is. Data keeps projects of its own, apart
from Codex's: the folder open in Codex is not carried into it, and going back
to Data brings back the one it had. Code cells are coloured as you type them,
and prose cells are drawn as the agent's answers are, TeX included.

Cells run in a Jupyter kernel, in the first of these that has one:
`PROTON_PYTHON`; the project's `.venv`, `venv` or `.env`; the Jupyter kernel
the notebook names (`metadata.kernelspec`), as `jupyter kernelspec list`
shows it; then the first Python on your login shell's PATH that has
`ipykernel`. A kernel that stops because its Python has no ipykernel says
how to add it. For a project of its own:

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
your own, and `/p` with a DOI or an arXiv id adds a paper. The Literature tab
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
turned and played. What a run showed goes back to the team: a seat of another
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
folders in a picker — browsed, searched by name and by word, with what is left
out and why — and read once, their words kept in the notebook; a reader; notes;
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
lecture — spoken by Fish Audio in voices you choose, with a transcript that
follows along; slide decks with speaker notes, revised a slide at a time,
presented, and written out for PowerPoint; and infographics and mind maps saved
as pictures. Your folders are named in your own settings,
`{"study": {"roots": ["~/Notes", "~/Courses"], "exclude": ["Notes/Private/"]}}`,
and catalogued in under a second without opening a file. A file still in
iCloud is listed under a cloud and never read until you choose to download it,
having been told its size.
**Design** · a web builder with a layer tree and an artboard, two-way bound to
real component source and design tokens.
**Modeling** · 3D — outliner, viewport, modifier stack and shader graph over a
scene serialised to diffable JSON. An object names the `.obj` its geometry is
in, a stack of operations says what to do to it, and the material graph is
compiled into a program a WebGL renderer runs on every pixel.
**Game** · a Godot project's scene tree and an inspector that edits the
`.tscn` without reformatting it. The play transport, profiler, hot reload and
the agent that verifies changes by replaying recorded input all need an engine
process, and wait on one.
**Video** · a multitrack timeline over a CMX3600 edit decision list, with the
captions from the `.srt` beside it. Both files are spliced, never re-printed.
There is no program monitor: a frame of the programme is a decoded frame of
video, and decoding needs ffmpeg.

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
moves it) and serves them from `artifact://`, sandboxed: a page can load what a
claude.ai artifact can and reach nothing of the editor's. **Open on claude.ai**
goes to the published page. **Import from Claude**, in the artifacts list,
brings in what Claude Code published from this machine before Proton was
keeping it — found in the desktop app's session records and in every session's
own file, with the files each page brought along. One whose page has since
been deleted comes in as its address, and opens on claude.ai.

## Antigravity agent connection

A Google AI subscription has no API key, so the **Gemini** provider cannot use
it. **Antigravity** can: in **Settings → AI & models**, select it, and Proton
runs Antigravity's `agy` command on your own Google sign-in. Run `agy` once in a
terminal to sign in. Proton finds it in `~/.local/bin`, where the Antigravity
installer puts it, or on PATH, or at `PROTON_AGY_BIN`. Gemini 3.8 Flash (High)
is the default, and `agy models` lists the rest.

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
Space to read, `+`/`−` and `0` to zoom and fit, Escape to put it away.

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
with a live Jupyter kernel, the library above, and Design mode — a layer tree
read out of your component, an artboard that renders it in a sandboxed frame,
and edits that go both ways: a token or a layer changed here rewrites the file
by splicing the bytes that one thing occupies, and a save in Codex comes back
the other way.

Modeling is real too, up to a point worth stating: `scene.proton.json`, an
outliner over it, an inspector that writes it, a viewport you can orbit and
click, and a shader graph whose boxes and wires are the material in the file.
Geometry lives beside the scene rather than in it: an object names the `.obj`
its surface is in, one relative path, and the viewport draws that surface —
flat shaded, sorted back to front, back faces dropped only for a mesh that is
actually closed, because culling a sheet would make a wall vanish when you
orbited past its edge. A mesh nobody copied, one that will not parse, or one
too big to draw falls back to a box — its *real* box, from the file, not a unit
cube standing in for the loader's ignorance — and the inspector says which of
those it is.

How big is too big was measured rather than guessed. A 2D canvas charges about
0.8 µs per drawn triangle, because a solid is one `fill()` each, and 80,000
triangles is where orbiting stops holding 30 fps; the table is in
`proton-core::modeling`. Past that the core sends the bounding box and not the
vertices. The fix is not a larger number, it is WebGL — which is also what a
painter's sort wants, since it is not a depth buffer and two triangles that
intersect will show through one another.

A mesh path is confined to the scene's own folder, checked both when this
writes one and when it reads one back. A scene arrives from somewhere — an
asset pack, a colleague, the agent — and one naming `../../.ssh/id_rsa` as a
mesh would otherwise have this editor open that file and put it on screen.

The stack is non-destructive, which is the whole point of one: the `.obj` is
read and never written, so mirroring a character does not double the vertices
in anybody's export and switching the mirror off gets the original back
exactly. It lives in `scene.proton.json` as a handful of lines describing a
change to a file it does not touch. Mirror, array and subdivide, each exact
— Loop subdivision, boundary rule and all, so the rim of a plane is refined
along itself rather than dragged toward the middle. Order is editable because
order is the point: mirror then array is two rows of paired objects, array
then mirror is one row reflected into two.

How far a stack grows a mesh is known exactly before it runs — a mirror
doubles, an array multiplies by its count, a level of subdivision is four
times the last — so a stack that would make 1.6 million triangles is refused
with that number in the message, rather than by running out of memory finding
out. A modifier of a kind this build has never heard of is kept in the file,
shown in the stack, and marked *not applied*: a bevel that quietly does
nothing is a worse answer than one that says it did nothing.

The material graph is compiled, not evaluated, in the core: what a graph means
is the file's business, and a surface property that varies across a mesh is one
value per triangle — eighty thousand colours crossing the seam on every nudge
of a slider. A dozen instructions cross instead, and the viewport runs them.

Those instructions become GLSL, and the GPU runs them per pixel. What it runs
them through is the terms a metal/rough workflow is made of: a Lambert diffuse
and a GGX specular with Smith and Schlick, metals taking their reflection
colour from the base and losing their diffuse. What it has no way to do is the
rest of a renderer — one light and no environment (the ambient is a flat grey
room standing in for one, which is what makes a metal legible rather than a
silhouette), no shadows, and nothing bouncing. A node kind the build cannot
render is kept in the file, and the surface draws plain grey rather than a
guess.

The 2D canvas this replaced had said for four revisions that it was the wrong
tool and should be *replaced rather than extended*, and it was right about all
three of its limits. Occlusion is now decided per pixel, so two things that
interpenetrate do; the material runs per pixel, so a noise is no longer as
coarse as the mesh; and a million triangles draws in 0.3 ms, where filling
paths ran out at ninety thousand. About four hundred lines went with it — a
painter's sort, a scratch-buffer pool, a colour quantiser, a batch-size
constant tuned to a rasteriser's quirk, and a software BSDF.

Normals are worked out after the modifier stack rather than read from the
file's `vn`, which every modifier would make wrong. Each vertex averages the
faces around it by the angle they make there — not by area, which let how a
quad happened to be triangulated tilt its corners — and faces are only
averaged together across an edge gentler than 40°. A cube keeps its corners;
a subdivided sphere comes out round, within 3° of radial on an actual sphere.
Geometry crosses the seam as bytes, not JSON. The event that says a mesh
changed carries its bounds and triangle count and nothing else; the page then
asks for the mesh and gets one buffer back — a 48-byte header, then positions,
normals and indices — and lays typed arrays straight over it, with no parse and
no copy. On a 270,400-triangle sphere that is 6.4 MB against 13.3 MB of JSON,
1.5 ms to write against 25 ms, and nothing to read against another 25 ms.
Bytes from a different build are refused in words rather than drawn as garbage
shaped like a mesh. What is left to pay for is reading the OBJ and working out
its normals, about 0.4 µs a triangle, so the budget is 500,000 triangles.
Each file is read once and kept for as long as an object wears it, so an edit
to a stack runs only the stack and the normals: 56 ms on that sphere, down
from 122 ms when every edit re-read the file.

Game reads a Godot project: `project.godot` says which scene starts, the
hierarchy is that `.tscn`'s node tree, and the inspector edits it by splicing
the file Godot will open next. What it cannot do is run one — the play
transport, hot reload, the profiler and replaying recorded input all need an
engine process on the other end of a pipe, and none of them is mimed.

Video reads a cut: an `.edl` is one line per event and four timecodes per line,
plain text that has diffed in Git since before Git, and the timeline draws each
clip where its record timecode puts it. The captions come from the `.srt`
beside it, and the media bin says which of the sources the cut refers to are
actually on disk. An edit — a timecode, a reel, a line of dialogue — replaces
the bytes that one field occupies and re-aligns no column, because these are
interchange files and something else opens them next.

The rate is the interesting part. An EDL does not record the rate its timecodes
are counted at; `FCM:` says only whether the count drops frame *numbers*. So
the rate is shown on the bar and can be changed there, and changing it writes
nothing — the file is the same file, and what moves is what its timecodes mean.
An assumption nobody can see is indistinguishable from a fact.

What Video cannot do is show you the picture. The program monitor, waveforms,
thumbnails, durations read from the media itself and any kind of render all
need ffmpeg in a process of its own — which the process model already accounts
for and which is not installed here. None of it is mimed.

So: no screen in this window claims to do something it cannot. What remains is
two real projects — an engine process for Game and an ffmpeg sidecar for
Video.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the milestones.

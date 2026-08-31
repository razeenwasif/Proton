# Proton — design

Proton is one workspace with seven modes, a modal editor underneath all of them,
and an agent that can act in any of them. It is not a code editor with extra
tabs bolted on; the tabs are the point.

Run the prototype:

```bash
node ui/serve.mjs
```

Then open <http://localhost:5178>. Keys: `⌃1`–`⌃7` switch mode, `␣` opens the
leader menu, `⌘K` Telescope, `:` the command line, `i` / `v` / `⎋` change vim
mode, `⌘,` settings. The layout targets 1440px and wider.

Bare digits are **counts**, not mode switches — `2dd` has to mean two lines.
The prototype used `1`–`7` before the modal engine was real; the mode chord is
now `⌃`.

---

## Where each idea came from

The five references in `assets/` each contribute something specific. Nothing was
copied wholesale — each one answers a different question about what Proton is.

| Reference | What it contributes |
|---|---|
| `8ed638a0…webp` — Layout Guide builder | **Design mode.** The Style / Layout / Inspect / Layers / Pagepilot rail, the layer tree of primitives (Container, Grid, Image, Video, Button, Avatar, Link, Code, Spacer, Stepper, Divider), the artboard with breakpoint toggles, the Draft badge. |
| `original-29d1…webp` — dark node editor | **Modeling mode's graph**, the right-hand Events / States / Props / Styles inspector, live collaborator cursors labelled by role, the console with an error count, and the Cube / Sphere / Plane primitive menu. |
| `original-006b…webp` — Elipsee settings | **The settings surface**: three columns (section → subsection → cards), and the frosted dark chrome that the whole app inherits. |
| `original-81e3…webp` — Copilot editor | **The AI dock**: tabs across the top of the panel, a context scope chip, numbered suggestions with inline diff blocks and a **Refer / Apply** pair on every hunk, file tabs annotated with a role tag, and scope toggles in the composer footer. |
| `large-thumbnail…mp4` — Cicada | **The mode switcher itself** — Codex · Design · Modeling · Game · Video — plus the agent task queue (In Progress / Ready, streaming thoughts, generated-file chips) and the composer with model picker, mode picker and voice. |

Added on top, from the brief: a **Chat** mode and a **Data** mode, the
Android-Studio-shaped mobile target, full Neovim modality, and the Code
Visualizer.

---

## Surface language

Four rules carry the whole surface. Everything else follows from them.

### 1. One window, no cards

Proton is a single macOS-style window: rounded, inset from the desktop so its
glass has something to refract, with the traffic lights and a unified toolbar
that does not divide itself from the content below.

Inside it, **nothing that holds content has a border of its own.** The editor is
not a panel — it is simply the part of the window where the code is. Regions are
separated by 1px hairlines and by space, never by a card sitting on a
background. The only things that get an edge and a shadow are the things that
genuinely float: Telescope, which-key, the command line, settings, popovers,
graph nodes, the composer.

### 2. Glass is layered by elevation

Liquid glass is not "add blur". It is three properties moving together — fill,
specular edge, and shadow — and elevation is the only thing that changes them.

| Layer | Fill | Blur | Specular |
|---|---|---|---|
| Window | `bg` at 74% | `blur(30px) saturate(180%)` | `inset 0 1px 0 white 11%` |
| Chrome (rail, sidebars) | black at 7–11% | inherits the window | — |
| Controls | white at 3.5 / 6 / 9% | — | same inset highlight |
| Floating | `bg` at 62% | `blur(46px) saturate(200%)` | `inset 0 1px 0 white 18%` |

The saturation boost is what separates this from frosted grey: the blurred
wallpaper behind the window keeps its colour, so the glass picks up a tint that
shifts as you move across the app. The specular top edge is what makes a surface
read as a physical sheet rather than a translucent rectangle.

Blur is a setting, not a constant. Drop it to zero on a remote session and every
surface falls back to a flat fill — no layout moves.

**And there is a hard limit worth stating plainly: `backdrop-filter` can only
blur page content. It can never reach the desktop.** Only the compositor can put
the wallpaper behind Proton, which is what the next rule is for.

### 3. The ground comes from the compositor

Windows 11 offers two materials through `DWMWA_SYSTEMBACKDROP_TYPE`, and Proton
asks for one at startup:

| Material | Samples | Live | Cost |
|---|---|---|---|
| **Mica** (default) | The wallpaper only — a static, heavily blurred sample that re-tints as the window moves across the desktop | No | Nearly free |
| **Mica Alt** | Same, stronger tint | No | Nearly free |
| **Acrylic** | Everything behind the window, other apps included | Yes | Real GPU and battery |

macOS gets vibrancy; Linux has no standard equivalent, so it paints its own
ground. So does Windows when the user turns transparency effects off or enters
battery saver — **`solid` is a supported state, not a failure**, and it has to
look deliberate. The setting reports what the compositor actually did rather
than what was asked for.

The trade this makes on purpose: the ground is now the user's wallpaper, which
we do not control, and contrast becomes wallpaper-dependent. So every surface
gets a **veil** — a flat tint of `--bg` over the material — and the veils form a
ladder from the outside in. The further from the text, the more wallpaper:

| Surface | Veil | |
|---|---|---|
| Mode rail, panel rail | 34% | Outermost; almost pure material |
| Window, statusline | 38–40% | |
| Sidebars, docks, builder rail | 46% | |
| **Stage** | **64%** | Frosted, not opaque — but always the most solid |

Three steps ship — Subtle, Medium (default), Heavy — and they scale the whole
ladder together, so the ordering never inverts. Heavy is where legibility starts
to depend on the wallpaper, which is why it is a choice rather than the default.

The stage gets no `backdrop-filter` of its own, and that is not an oversight:
there is no page content beneath it to blur, only transparency down to DWM,
which has already blurred the wallpaper itself. Internal floating layers *do*
keep their CSS glass, because blurring the editor behind Telescope is page
content and therefore still the right tool.

### 4. Colour is a token set

A theme sets sixteen custom properties on `<html>` and touches no layout rule.
Four ship, switchable live in Settings → Appearance:

| Theme | Ground | Accent | |
|---|---|---|---|
| **Tokyo Night** (default) | `#16161e` | `#7aa2f7` | The nvim standard; its blue-violet ground is the best fit for glass |
| **One Dark** | `#282c34` | `#61afef` | The classic |
| **Catppuccin Mocha** | `#1e1e2e` | `#89b4fa` | Softest contrast of the four |
| **Nord** | `#2e3440` | `#88c0d0` | Coolest, lowest saturation |

The tokens each theme defines:

| Token | Role |
|---|---|
| `--bg` / `--bg-deep` | Window ground · desktop behind it |
| `--text` / `--text-hi` | Body · emphasis |
| `--accent` / `--accent-2` | Active mode, cursor, primary action · AI, keywords |
| `--ok` `--warn` `--err` `--info` `--tone` | Pass · warn · error · pointer · numbers |
| `--wall-a/b/c` | The three wallpaper blobs the glass refracts |

Everything else is derived: `--ink-2/3/4` are `color-mix` steps off `--text`,
elevation fills are `--raise` at fixed alphas, and every accent wash is a
`color-mix` against `--accent`. That is why adding a fifth theme is a paste of
one block, and why the accent carries through the entire window rather than
being applied per-component.

### Type

| | |
|---|---|
| UI | **Inter** — falls back to SF Pro, then Segoe UI Variable |
| Code | **CaskaydiaMono Nerd Font** → CaskaydiaCove NF → Cascadia Mono |

The Nerd Font patch matters: devicons in the file tree, the git branch glyph in
the statusline and the powerline separators all come from that glyph set. Where
only `CaskaydiaCove NF` is installed (the Code-width patch, which is the common
one) the fallback catches it and only the character advance changes.

---

## The shell

```
╭──────────────────────────────────────────────────────────────────────╮
│ ●●●  PROTON   orbital-dashboard · main            ⋯   Share   Build │  46px
├────┬──┬────────────────┬────────────────────┬──────────────────────┤
│Chat│  │                │                    │                      │
│Code│ p│                │                    │                      │
│Desn│ a│  context pane  │       stage        │         dock         │
│Modl│ n│                │                    │                      │
│Game│ e│                │                    │                      │
│Vide│ l│                │                    │                      │
│Data│  │                │                    │                      │
├────┴──┴────────────────┴────────────────────┴──────────────────────┤
│ NORMAL │  main +2 ~1 │ path │ lsp │      │ ✦ agent │ ln 11, col 62  │  26px
╰──────────────────────────────────────────────────────────────────────╯
```

Those are hairlines, not panel edges — the diagram overstates them.

**Every divider is a drag handle.** The hairline between panes is a real 1px
grid track rather than a border, which means a pane's width is one number a drag
can write and nothing has to be measured. Drag to resize, double-click to reset,
and widths persist per mode. Two clamps: no pane below 172px, and the stage
never below 320px. Widths are remembered as *what you asked for* rather than
what currently fits, so narrowing the window and widening it again gives your
layout back instead of leaving it clamped.

The mode switcher is the outermost column, *outboard* of the panel rail — so
left to right the order is mode rail, panel rail, context pane, stage, dock.
Two reasons it is a column rather than pills across the titlebar. Seven pills do
not survive an eighth, and a column does; and it puts the two rails in a real
hierarchy — the outer one chooses *which application you are in*, the inner one
chooses *what you are looking at inside it*. That reads as macOS (Finder's
sidebar, Xcode's navigator) rather than as a browser tab strip, and it leaves
the titlebar to the project breadcrumb and the collaborators, which is where
those belong.

Five regions never move, in any mode:

- **The mode rail** — which application you are in.
- **The panel rail** — panels, not modes. Explorer, Telescope, Git, Debug, Visualizer,
  Database, Plugins. The same rail in Video mode as in Codex.
- **The context pane** — what you are working *on*. Files in Codex, layers in
  Design, the outliner in Modeling, hierarchy in Game, the media bin in Video,
  sources in Data, threads in Chat.
- **The stage** — the thing itself.
- **The dock** — the agent, plus whatever inspector that mode needs. Always
  where the AI lives, so there is one place to look for it.

The statusline is the Neovim statusline, in every mode. In Video mode it still
says `NORMAL`, and `d d` still deletes the selected clip.

---

## Modal editing everywhere

Vim is not an emulation layer in the editor pane. It is the input model for the
application, and each mode binds the same verbs to its own nouns.

| | Codex | Design | Modeling | Game | Video | Data |
|---|---|---|---|---|---|---|
| `j` `k` | line | layer | outliner item | node | clip | cell |
| `d d` | delete line | delete layer | delete object | delete node | ripple-delete clip | delete cell |
| `y` `p` | yank / put | copy / paste layer | duplicate object | duplicate node | copy / paste clip | copy / paste cell |
| `/` | search buffer | find layer | find object | find node | search transcript | search cells |
| `u` | undo | undo | undo | undo | undo | undo |
| visual | text object | multi-select | box-select | box-select | range select | cell range |

Chat is the one mode where the verbs mostly go quiet — `j` `k` walk threads,
`/` searches the transcript, and everything else defers to the composer, because
the noun there is prose.

Leader is `␣`, and which-key shows the tree after ~200ms:

```
␣f  +find      ␣g  +git      ␣l  +lsp        ␣d  +debug / data
␣c  +code      ␣a  +ai       ␣w  +window     ␣b  +buffer
␣t  +terminal  ␣m  mode      ␣p  +plugins    ␣e  explorer
␣/  live grep  ␣?  keymap    ␣␣  last buffer
```

**Telescope is the spine.** One fuzzy finder over everything the workspace
holds, not just files:

| Picker | Finds |
|---|---|
| `␣ff` `␣fg` `␣fb` `␣fh` | files · live grep · buffers · help tags |
| `␣fs` `␣fd` | LSP symbols · diagnostics |
| `␣fm` | artboards and components (Design) |
| `␣fo` | scene objects and materials (Modeling / Game) |
| `␣fc` | notebook cells and SQL history (Data) |
| `␣ft` | recorded traces (Visualizer) |
| `␣fa` | ✦ semantic — "where the fleet grid re-renders", answered over the embedding index |

Every picker uses the same window: prompt, results, live preview. `␣fa` returns
code spans instead of filenames, and is the only picker that costs a model call.

The command line is real. `:ProtonTrace native/trajectory.c`, `:ProtonMode
video`, `:ProtonAgent fix the jank`, `:vsplit`, `:%s/…/…/g`. Anything reachable
by a menu is reachable by a command, and the command palette is `:` — not a
second, different palette.

---

## The agent

Three things make the AI integration more than a chat sidebar.

**One agent, six modes.** It sees the whole workspace. The same task can trace a
C program, change a React component, retime a video clip and re-run a notebook
cell, because those are all just tools it holds. The Codex dock's **Tasks** tab
is the Cicada queue: In Progress / Ready, each task streaming its tool calls
(`telescope.grep`, `lsp.references`, `profile.react`) with timings, and finished
tasks carrying their artifacts as chips.

**Diffs, never edits.** Every proposal arrives as one or more hunks in the dock,
each with **Refer** (jump to it) and **Apply**. Hunks land independently — you
can take the memoisation and leave the identity change. The agent writes to a
shadow worktree first; nothing touches your tree until you accept. Autonomy is a
setting with the stop point named explicitly: *plan → propose diffs → wait*.

**Scope is explicit and visible.** The composer carries `@repo`, `@file`,
`@selection`, `@trace`, `@artboard`, `@scene`, `@dataframe`. What the model can
see is a chip you can point at, which is the part the Copilot reference gets
right and most tools get wrong.

Inline, `⌘K` is an edit-in-place prompt over the selection, and ghost text is
completion. `⇥` accepts, `⌥⇥` cycles.

---

## The modes

### Chat

A full conversation with the model, given the whole window rather than a 370px
sidebar. Threads on the left — pinned, today, yesterday — and the conversation
in a centred 780px column, because a reading measure is a reading measure
whether or not the app around it is an IDE.

It is not a separate product bolted on. The distinction from the Codex dock is
where the work lands:

|  | Chat mode | Codex dock |
|---|---|---|
| For | Thinking, explaining, deciding | Acting on the buffer in front of you |
| Output | Prose, tables, charts, artifact chips | Diffs, applied in place |
| Scope | The whole workspace | Usually one file or selection |

The same agent and the same tools underneath, so a chat can end with **Send to
Codex** and the diffs it staged are already there. The conversation in the
prototype does exactly that: it profiles the web target, reads Logcat from the
Android target, concludes both symptoms have one cause in `orbit.ts`, and hands
back two staged hunks.

### Codex

The editor. Buffer tabs carry a role tag (`route`, `lib`, `native`) and a
modified dot. Relative line numbers, inlay hints, a code lens above each symbol,
current-line crimson wash. Splits are vim splits. Harpoon-style pinned buffers
sit under the tree.

The dock has five tabs — **Agent**, **Tasks**, **Visualizer**, **Device**,
**Terminal** — which is where the mobile story lives. A project declares its
targets in `proton.toml`; `android · pixel 8` turns the Device tab into the
Android-Studio layout: a device frame, a layout inspector that outlines the
selected view over the running app, and Logcat filtered to the package. The
insight that makes it worth having in the same pane as the agent is on screen
in the prototype — Choreographer reports skipped frames on Android for the same
reason the web target janks, and one fix covers both.

### Design

The web builder. Layers tree on the left, artboard in the middle with
desktop/tablet/phone breakpoints and a Design / Code / Preview toggle, style
inspector on the right. Two things make it not-Webflow:

- **Two-way binding to real files.** The tree shows which layers are bound to
  `Dashboard.tsx` and `theme.css`; editing tokens in the inspector rewrites the
  Tailwind theme, and editing the component in Codex updates the artboard. The
  generated code is shown, not hidden.
- **Pagepilot** reviews the layout rather than generating it — "this grid drops
  to one column below 640px but the video keeps a 16:9 box; it will overflow by
  24px."

Collaborators appear as labelled cursors on the artboard.

### Modeling

3D. Outliner, viewport with a transform gizmo and Object / Edit / Sculpt /
Shading modes, primitives (Cube · Sphere · Plane), a shader node graph docked
under the viewport, and a modifier stack on the right. The whole scene
serialises to `scene.proton.json` so it diffs and merges in Git — which is what
lets the agent edit a scene by writing a patch instead of driving a GUI.

### Game

Engine-shaped: hierarchy, play/pause/step/stop transport, a game view with
collision shapes and navmesh overlays, an inspector bound to the selected node,
and Output / Debugger / Profiler / Network below. Hot reload preserves state.
The agent's leverage here is **replay** — it changes a jump curve, then replays
30 seconds of recorded input against both builds and reports 7 missed inputs
became 0. That is a claim you can check.

### Video

An NLE. Media bin, program monitor, and a timeline with text, two video tracks
and audio. Generated assets are tagged in the bin and coloured differently on
the timeline, so you always know what the model made. The agent's work is
listed as the operations it actually ran — `scene.detect`, `transcribe`,
`rough.cut`, `generate` — with counts, and it hands back an `.edl` and a
sidecar `.srt` rather than a flattened render.

### Data

Notebook, SQL, grid and charts over the same kernel. Cells record their inputs,
so lineage is real: the analyst cell can say "open sat 17 in Codex" and land on
the scheduler code that emits `SAFE_HOLD`. The right pane profiles the selected
column — nulls, σ, outliers, a histogram. The analyst is held to the same
standard as the rest of the agent: it names the test it ran and the p-value,
and it distinguishes "the fleet is degrading" from "two satellites on one ground
station are degrading".

---

## Code Visualizer

Docked in Codex as a first-class pane, not an embedded page. Stack frames and
heap blocks, one box per object, and **every pointer drawn as an edge** — the
rule the upstream project is built around. In the prototype the edges are
measured from the live boxes rather than hand-placed, so they stay correct when
the pane resizes; chain pointers route down a lane instead of crossing the block
they point at.

The scrubber is the time-travel index: step 212 of 558, tracking `cur->next`,
with the dangling read drawn in red. `Memory / Structure / Calls / Layout`
match upstream's panes.

See [`docs/CODE-VISUALIZER.md`](../docs/CODE-VISUALIZER.md) for how it plugs in.

---

## What the prototype does not do

It is a design prototype, not a thin slice of the product. There is no editor
buffer, no language server, no model call, no renderer. Text does not edit,
`d d` does not delete, and the seven modes are seven hand-built screens. Its job
is to settle the layout, the surface language and the interaction model before
any of that gets written.

What *is* real: the theme system (four palettes, live-switchable, sixteen tokens
each), the elevation model, and the Visualizer's measured pointer edges.

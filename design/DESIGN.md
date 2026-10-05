# Proton — design

Proton is one workspace with eight modes, a modal editor underneath all of them,
and an agent that can act in any of them. It is not a code editor with extra
tabs bolted on; the tabs are the point.

Run the prototype:

```bash
node ui/serve.mjs
```

Then open <http://localhost:5178>. Keys: `⌃1`–`⌃8` switch mode, `␣` opens the
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
Six ship, switchable live in Settings → Appearance:

| Theme | Ground | Accent | |
|---|---|---|---|
| **Aura Dark** (default) | `#15141b` | `#a277ff` | Aura's ground and terminal with Gruvbox's syntax — one Zed setup, which Codex is drawn after |
| **Tokyo Night** | `#16161e` | `#7aa2f7` | The nvim standard; its blue-violet ground is the best fit for glass |
| **One Dark** | `#282c34` | `#61afef` | The classic |
| **Catppuccin Mocha** | `#1e1e2e` | `#89b4fa` | Softest contrast of the lot |
| **Nord** | `#2e3440` | `#88c0d0` | Coolest, lowest saturation |
| **Gruvbox Dark** | `#282828` | `#83a598` | The one warm ground, which is what proves the rules are theme-blind |

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
`color-mix` against `--accent`. That is why adding a theme is a paste of
one block, and why the accent carries through the entire window rather than
being applied per-component.

Beneath the sixteen sit **optional tokens** a palette may set and need not:
the syntax colours (`--syn-keyword` … `--syn-link`, and `--code` for plain
text), the terminal's sixteen ANSI colours (`--ansi-0` … `--ansi-15`), and the
editor's own surfaces — `--muted`, `--line-number`, `--cursor`,
`--selection`, `--row-on`, `--head` and the rest. Unset, each is derived from
the sixteen exactly as the rules drew it before the token existed, so the
older themes look as they always did. Aura sets them all, because "Aura's
ground with Gruvbox's code" is a choice, not something a derivation can reach.

### Type

| | |
|---|---|
| UI | **Inter** — falls back to SF Pro, then Segoe UI Variable |
| Code | **CaskaydiaMono Nerd Font** → CaskaydiaCove NF → Cascadia Mono |
| Codex, chrome and code alike | **JetBrainsMono Nerd Font** → JetBrains Mono (Google Fonts) → the code stack above |

The Nerd Font patch is worth having for the powerline separators and the git
branch glyph in the statusline, but nothing structural depends on it. Icons are
drawn, not typed — see below. Where only `CaskaydiaCove NF` is installed (the
Code-width patch, which is the common one) the fallback catches it and only the
character advance changes.

### Icons

Ninety symbols, one sprite, `currentColor` throughout. Drawn on a 24 grid with
a 1.7 stroke and round caps, so the whole set shares a weight and a corner
radius and reads as one family rather than as a collection.

**Nothing is an emoji.** They arrive in colour, they change shape with the
platform, and a yellow folder in a window whose entire palette is sixteen
tokens is a hole in the design. Every one that was in the prototype is now a
symbol that inherits the theme, which means a theme switch takes the icons with
it and a `prefers-reduced-motion` reader gets a server spinner that does not
spin.

Two rules do most of the work:

**Shape carries the meaning; colour is a second, redundant channel.** In the
file tree the silhouette says what kind of thing a file is — page, image,
waveform, sliders — and the tint says which language. A tree where every row is
the same grey outline is still readable, just slower; one where the shapes are
identical and only the hue differs is not readable at all to a good fraction of
people.

**Size is set where the icon sits, not on the icon.** The same mark is 11px in
a completion list and 17px on the mode rail, and a 1.7 stroke that looks right
at 17 is a hairline at 11. The context rules tune the stroke along with the box.

Two places deliberately keep glyphs rather than icons. **Keycaps** — `⌘ ⌃ ⎋ ⇥
␣` — are typography: they are what is printed on the key, and drawing them
would be redrawing a font badly. And **literal terminal output** in the mock
panes keeps its `✓` and `❯`, because that is text a program actually wrote.

**Files and folders are the exception to the sprite.** They wear Charmed
Icons in its Warm palette — the set the Zed setup behind Codex uses — from
`ui/file-icons.js`, a curated sixty-one of them under the set's MIT licence.
They carry their own colours rather than taking the theme's, so they have
their own class, `.fi`, with nothing for a path to inherit. The matching is
the set's own and case-sensitive, as Zed's is: `README.md` gets the Markdown
mark, because the set only names `readme.md`.

---

## The shell

```
╭────┬──┬─────────────────┬───────────────────┬──────────────────────╮
│ ●  │  │                 │                   │                      │
│ ●  │  │                 │                   │                      │
│ ●  │  │                 │                   │                      │
│ ⬡  │  │                 │                   │                      │
│Chat│  │                 │                   │                      │
│Code│ p│                 │      stage        │                      │
│Desn│ a│  context pane   │  ┌───────┬──────┐ │        dock          │
│Modl│ n│                 │  │ split │ split│ │                      │
│Game│ e│                 │  └───────┴──────┘ │                      │
│Vide│ l│                 │                   │                      │
│Data│  │                 │                   │                      │
│    │  │                 │                   │                      │
│ 👥 │  │                 │                   │                      │
│Buld│  │                 │                   │                      │
├────┴──┴─────────────────┴───────────────────┴──────────────────────┤
│NORMAL│ ⬢ project │  main +2 ~1 │ path │ ✦ agent │ ln 11, col 62    │  26px
╰────────────────────────────────────────────────────────────────────╯
```

Those are hairlines, not panel edges — the diagram overstates them.

**There is no titlebar.** Window controls, the mark, the modes and the project
actions are all on the same vertical edge. Two reasons: it buys the editor a
full row of height, which is the dimension a code editor is always short of;
and splitting chrome across a top bar *and* a left rail was asking the eye to
track two different homes for the same class of thing. The project name moves
to the statusline, next to the branch — which is where a vim user already looks
for it. Empty rail is the window's drag surface.

**Every divider is a drag handle.** The hairline between panes is a real 1px
grid track rather than a border, which means a pane's width is one number a drag
can write and nothing has to be measured. Drag to resize, double-click to reset,
and widths persist per mode. Two clamps: no pane below 172px, and the stage
never below 320px. Widths are remembered as *what you asked for* rather than
what currently fits, so narrowing the window and widening it again gives your
layout back instead of leaving it clamped.

**The stage splits.** `:vsplit` and `:split` — and `␣wv` / `␣ws` / `␣wq` /
`␣wo` — divide the stage into up to four views, side by side or stacked. The
active split carries an accent edge and the others dim, because with more than
one view nothing else on screen says where the cursor is.

A split shows *the same buffer twice*, which is what `:vsplit` has always meant:
two views onto one document, not two documents. That is why the prototype clones
the pane rather than inventing a second file — it is the honest model, and the
thing that has to become real in M1 is the buffer behind each view, not the
splitting.

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

The statusline is the Neovim statusline, in every mode but Codex. In Video
mode it still says `NORMAL`, and `d d` still deletes the selected clip.

---

## Codex, drawn after Zed

Codex is the first mode drawn to a second language: one Zed setup, reproduced
— Aura Dark with Gruvbox syntax, JetBrains Mono for everything, and nothing
that setup turns off. Chat and Data are drawn this way too; the other four
keep the glass until they are, so every rule for it hangs off `data-mode` on
`<html>` or the workspace it is for (the blocks near the end of
`ui/styles.css`), never replacing a rule the glass relies on.

```
╭──────────┬──────────────────────────────┬────────────────┬──────────┬───╮
│ ▸ crates │ crates/proton-core/src/vim.rs│ ❯ cargo test   │ thread ⋯ │ ● │  ← darker
│   src    │                              │                │          │ ● │
│ ▣ vim.rs │  1  //! The modal input …    │                │ Thinking │ ● │
│          │  2  //!                      │                │ Read file│ C │
│          │                              │                │          │ o │
│          │                              │                │          │ d │
│ project  │  code                        │  terminal      │ agent    │ … │
│          │                              │                │          │   │
│          │                              │                │          │ ◫ │
│          │                              │                │          │ ⚙ │
╰──────────┴──────────────────────────────┴────────────────┴──────────┴───╯
                                                                       40px
```

No line in that diagram is drawn on screen. Every panel is the editor's
colour, so the window reads as one surface broken only by space and by the
agent's darker header.

**What the setup turns off stays off.** No rails, no file tabs, no statusline,
no scrollbars, no git gutter, no current-line wash, no borders. What the rails
held that Codex still needs moved to a title bar: the window controls, the
modes as quiet words — mouse users need a way out of Codex, and ⌃1–8 is not
discoverable — and the panel toggles a hidden statusline would otherwise have
taken with it.

**The title bar stands on its end down the right edge**, 40px wide, which is
where Codex parts from the setup on purpose: the code keeps the window's full
height, the dimension an editor is always short of, as the other modes' rail
gives it theirs. The traffic lights stack at its top, the mode names read down
it the way a side panel's labels do, the toggles sit at its foot, and the
empty stretch between is the window's drag surface.
The vim mode is the caret's shape, which is where the eye already is — and so
is where the keys are going: hollow and still while the project panel, the
shell or a field has them, as Zed draws an editor without the focus. A click
in the code takes them back, and so does a file clicked in the tree. A
refused `:w` is still the banner over the code. File tabs come back from
Settings → Appearance; until then the row above the code is the open file's
path, as Zed's breadcrumb is with its tab bar off, and `␣fb` lists the buffers.

**The terminal sits beside the code**, the way a Zed pane group splits one to
the right of the editor, rather than in the dock — so the shell and the agent
can be on screen together, which is the reason Zed users arrange it so. It is
view state: open or shut and how wide are remembered in the window, never
more than half the stage, and hiding it does not end the shell. `` ⌃` `` toggles it,
and `␣tt` or `:terminal` opens it. Each project has its own shell, started in
its root — as a login shell on macOS, as Terminal starts one, so Homebrew's
PATH is there — and the pane shows the project you are in, starting one there
if it has none.

**Settings → Appearance → Terminal draws it with the GPU**, as Zed does: each
glyph is rasterised once into a sheet on the GPU, and a screen is two
instanced draws, the cells' colours and then their glyphs, with nothing laid
out. Text stays the default while it is new. The rows are still written
underneath, invisibly, once the screen has been still for 150 ms — so the
mouse selects, the clipboard copies and a screen reader reads exactly as it
did — and a press brings them up to date at once. On a 52 × 100 screen
scrolling a coloured build log a line a frame, it cost 1.8 ms of the web
process and 0.6 ms of WebKit's GPU process a frame against 4.7 ms and 3.5 ms
for text, with memory the same; `tests/ui/terminal-bench.html` measures it.

**The agent's header is Zed's**: the thread's name, fading where it runs long,
then new, expand and `⋯`. Expand goes to the Chat tab, which is Claude Code,
and `⋯` holds the panels that used to be a row of tabs, since
one is open at a time and it is usually the agent. Tool calls read as lines —
an icon, what it did, and to what — and the composer has no box: what you
type is set like code, on a double line, under a placeholder in faded lilac.

The numbers were measured off a screenshot of the setup at 1512pt: 16px code
on 32px lines, 17.5px chrome, 32px tree rows stepping in 20px a level, a 39px
agent header, a 12px traffic light every 20px. They scale with Interface text,
as Zed's scale with its UI font size, and the code with Editor text.

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

Chat is the one mode where the verbs go quiet: the keyboard is the
composer's, because the noun there is a conversation with Claude Code, and its
own keys — ⇧⇥, `/`, Escape — are the ones that mean something.

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

**Threads are kept.** Every conversation is a file in the project's
`.proton/chats/`, written after each turn. The clock in the agent header
lists the project's threads, newest first, to reopen one or delete it (after
asking, in the row); the one open comes back after a restart.

**Answers are documents.** An answer is markdown, drawn by the renderer the
Chat tab uses (`ui/markdown.js`). A fenced block is coloured with the editor's
own token classes — Aura's lilac keywords in Aura, Gruvbox's red in Gruvbox —
by a small lexer per family of languages (`ui/highlight.js`): the core's
tree-sitter colours buffers, not a few lines that arrive a word at a time. TeX
— `$…$`, `\(…\)`, `$$…$$`, `\[…\]`, a ```` ```math ```` fence — is drawn by
KaTeX, vendored and loaded the first time an equation appears, with `trust`
off so no command in one can become a link or markup. Until it has loaded, and
while an equation is still being written, the source shows dimmed rather than
as an error. `$…$` follows Pandoc's rule, so "$5 and $10" stays two prices.

Inline, `⌘K` is an edit-in-place prompt over the selection, and ghost text is
completion. `⇥` accepts, `⌥⇥` cycles.

---

## The modes

### Chat

**Claude Code itself, in a tab.** Not a harness written to resemble it: the
`claude` command, driven over the stream-json protocol Claude Code's own SDKs
use (`proton-claude`). Every tool, permission rule, CLAUDE.md, skill, subagent,
compaction and claude.ai connector is the one Claude Code has — and so is its
Artifact tool. Proton is the host: it draws the session, asks the questions
Claude Code would ask in a terminal, and keeps a copy of everything published.

The layout is Claude Code's app. Sessions run down the left — every session
Claude Code keeps for the folder, whether it was started here, in a terminal
or in the desktop app, each resumable — and under them the desktop app's
sessions from its other folders, each named by its folder. One the desktop
app has open right now wears a ring, and what is sent to it continues a fork,
because two programs writing one session would interleave.
Above them all are your folders, as Claude's own apps have them: a head that
folds — still showing a spinner or a dot when something inside is working or
waiting — a **⋯** to rename or delete it, and the chats it holds indented
under it. A chat goes into one by being dragged there or from its own **⋯**
(or a right click), and a new folder is named in place the moment it exists.
Folders belong to you rather than to a project: one file in Proton's data
folder, read again whenever another project's tab has changed it, holding
chats from every project and engine, each opened in its own folder. A chat's
**⋯** also deletes it, after asking: the transcript goes to the Trash rather
than away, because a session is history someone may want back, and one
another Claude Code has open is refused, because that program would only
write it back. The conversation is a centred column,
your messages as bubbles and Claude's as prose. Each tool is one line — a dot
for its state, a verb, what it acted on, a count — that opens into its diff,
its output or the lines it read. The questions it stops for are cards answered
in place: yes, yes and remember the rule it suggests, or no with what to do
instead; a plan is a card to approve, and AskUserQuestion is a form. Its task
list sits over the composer, whose pills are the permission mode and the model
and effort; ⇧⇥ steps the mode, `/` offers Claude Code's commands, Escape stops
the turn, and an image pastes in. A session left running carries on while
another is looked at.

It is worn as Codex is worn: the theme's ground and accent, JetBrains Mono at
Zed's sizes, the darker strip for a header, rows lit as the tree lights them,
and Codex's title bar down the right instead of the rails.

**Each chat has its own artifacts pane, as in Cowork.** Switching chats puts
back what the chat on screen had open — an artifact at its version, the list,
or a closed pane — and one published while another chat was on screen is
waiting open when its own comes back. The list puts the chat's own artifacts
first. What each pane holds is the window's to remember, so it lives in the
window's storage, not the project's.

**Artifacts open in the right pane the moment they are published.** The page
is copied, a version at a time, into Proton's data folder
(`dev.razeen.proton/artifacts`), so the version published is the one shown,
offline. It is served from `artifact://` — a scheme the shell answers only from
that folder, under the policy artifacts are written for (inline script, the
CDNs claude.ai allows) and with nothing of the editor's in it — and framed
sandboxed, with an origin of its own, so a page can draw itself and reach
nothing of Proton's. The pane lists every artifact kept, every version of each,
and opens the published page on claude.ai. What was published before Proton
kept it is imported from what this machine remembers — the desktop app's
records and the sessions' own files — page and files as they are now, or the
address alone when the page has gone.

**Codex, Gemini and Antigravity answer here too.** The first pill on the
composer says who is answering; a new conversation can be with any of them.
Each is its own CLI on its own sign-in — `codex app-server` over JSON-RPC, the
Gemini CLI over the Agent Client Protocol, `agy` over its stream-json print
mode — and each is folded into the rows Claude Code's are: a command is a Bash
row, a patch an edit with its hunks, a refused tool a refused row. The pills
are the engine's own — its models, its efforts, its permission modes under
their own names (Codex's Auto and Read only, Antigravity's Review) — and so is
the account box. What differs is said where it differs: Antigravity cannot stop
to ask, so what its mode refused is listed when it finishes, with the mode that
would allow it; the Gemini CLI cannot load a session, so a conversation picked
up again is told what was said. Their conversations are kept by Proton, as the
rows it drew, and listed with the project's sessions, tagged with who had them.

The Codex dock keeps Proton's own agent: the one that stages edits for review
and also speaks to Codex, Gemini and Antigravity. The council lists under the
sessions, and `/council <question>` convenes it.

### Codex

The editor, drawn after Zed — see [Codex, drawn after Zed](#codex-drawn-after-zed).
The path of the open file sits above the code, and buffer tabs with a modified
dot come back from Settings. Splits are vim splits, and the terminal splits off
to the right of them.

The dock's panels — **Agent**, **Tasks**, **Debug**, **Visualizer**,
**Device**, **Git** — sit behind the agent header's `⋯`, and the Device panel
is where the mobile story lives. A project declares its
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

Drawn as Codex is: the title bar down the right, the project panel as
Codex's, one ground from edge to edge. The notebook is a column of cells on
it, each a little raised from the ground as Zed raises a tool call in its
agent, the one the keys are on outlined; the frame beside it is a table in the
same type, its numbers in the syntax's colour for numbers.

**Its own projects.** Notebooks live in a research folder more often than in
the codebase, so Data keeps projects apart from Codex: its own list in the
title bar, and its own project on screen. Going to Data brings back the one
Data had, whatever Codex has open since, and leaving it brings back Codex's.
Research works in Data's project too, for the same reason. Every other
mode — Chat, Design, Modeling, Game, Video — works on the checkout the code
is in, and shares Codex's. The first visit starts on
Codex's project, because a side has to show something; open another from
Data's switcher and the two are separate from then on, across restarts.
Opening a notebook from Codex takes its project to Data, since that is where
the notebook is; closing a project on one side leaves it open on the other.

A code cell is coloured as it is typed, by the lexer the agent's answers use:
the colours are the source drawn again behind the cell, whose own text is
clear, so what you type into stays plain text with the browser's own undo.
Prose cells go through the answers' renderer too — code blocks, tables and
TeX, which notebooks are full of.

Notebook, SQL, grid and charts over the same kernel. Cells record their inputs,
so lineage is real: the analyst cell can say "open sat 17 in Codex" and land on
the scheduler code that emits `SAFE_HOLD`. The right pane profiles the selected
column — nulls, σ, outliers, a histogram. The analyst is held to the same
standard as the rest of the agent: it names the test it ran and the p-value,
and it distinguishes "the fleet is degrading" from "two satellites on one ground
station are degrading".

### Research

Investigations, after Google's AI co-scientist, with the roles of the
user's own AGENTS.md: Claude Code the Principal Investigator, Codex the Lab
Engineer, Antigravity the Validation Sandbox. A round frames a plan (the
first round only), has every seat propose hypotheses, has each reviewed by
a model of another family, verifies the leader's derivation, ranks them in a
tournament judged blind — A and B in random order, by a judge whose family
wrote neither — evolves the best, and ends with an overview that rewrites
the plan. Light on purpose: about fourteen agent calls, never more than
twenty, and nothing starts without a click.

**Drawn as Chat is.** The investigations down the left as Chat lists
sessions; the board in a column in the middle, under a header that is the
investigation's — where a round's controls go — with its tabs in a strip of
their own: Overview (the brief, the overview, the plan, and folded under
them the verdicts and the sandbox log) and Hypotheses (the leaderboard by
Elo, the retired last, a narrow board putting each one's name on a line of
its own). Each section names its file, because each is one: an
investigation is a folder, `research/<slug>/`, and everything it knows diffs
in Git. A hypothesis opens whole — statement, rationale, predictions, its
reviews and their scores, the matches it won and lost with the judge's
reasons, and what it was evolved from and into as links.

**The Lab** is the pane on the right: what an investigation proposes to run
is shown there before it runs, and runs with no network, nothing of your
home folder to read and nowhere to write but its own folder. Its figures and
3D scenes are drawn there too.

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

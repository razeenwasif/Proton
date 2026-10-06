/* Proton front end.
 *
 * Two kinds of state live here, and keeping them apart is the whole point:
 *
 *   - Core state — the vim mode, the active workspace mode, pending commands.
 *     The page never decides these. It sends a Command and renders the Events
 *     that come back.
 *   - View state — which dock tab is open, which tree row is selected, which
 *     theme is applied. Purely local; the core neither knows nor cares.
 *
 * Anything that becomes a document (a real selection, a real buffer) graduates
 * from the second list to the first.
 */

import {escapeHtml, icon} from "./html.js";
import {lineHtml} from "./editor-row.js";
import { HAS_SHELL, applyBackdrop, dispatch, dispatchMany, onEvents, onCommand, readGeometry, readLab, sendKey, setBackdrop, setOpaque,
  setMode as coreSetMode, sync, wireWindowChrome } from "./bridge.js";
import { closeDocument, documentIsOpen, documentKey, documentShown, fitDocument, openDocument, zoomDocument } from "./document.js";
import { decodeMesh } from "./meshbytes.js";
import { createOverlays } from "./overlays.js";
import { createPanels } from "./panels.js";
import { createDisplaySettings } from "./display-settings.js";
import { createFileTree } from "./file-tree.js";
import { fileIconHtml, folderIconHtml, installFileIcons } from "./file-icons.js";
import { createBufferTabs } from "./buffer-tabs.js";
import { createSplitResize } from "./split-resize.js";
import { createEditorFeedback } from "./editor-feedback.js";
import { createEditorState, createEditorPaints } from "./editor-state.js";
import { performanceReview, editorPainted } from "./performance.js";
import { createTerminalGL, xtermHex } from "./term-gl.js";
import { createClaude } from "./claude.js";
import { createResearch } from "./research.js";
import { createStudy } from "./study.js";
import { createLab } from "./lab.js";
import { createLean } from "./lean.js";
import { markdown } from "./markdown.js";
// As `highlightCode`: `highlight` here is the picker's, for the letters a query matched.
import { highlight as highlightCode } from "./highlight.js";
import { keyCommand, changesContext } from "./transport.js";
import { historyCommand, historyShortcut } from "./history-controls.js";
import {
  clearGeometry,
  drawViewport,
  frameAll,
  hasGeometry,
  hasSurface,
  renderProblem,
  renderer,
  setGeometry,
  setShaders,
  startViewport,
} from "./viewport.js";

const $  = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];

const token = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

/* ── theme ────────────────────────────────────────────────────────────
   A theme sets sixteen custom properties on <html> — and may set the
   optional ones beneath them, as Aura does for its syntax and terminal —
   and nothing else. No rule below the palette block knows which one is
   active, which is why switching is instant and why adding another is a
   paste, not a port. */
const THEMES = ["aura-dark", "tokyo-night", "one-dark", "catppuccin", "nord", "gruvbox-dark"];

/* Palettes a plugin brought, by id.
 *
 * A built-in theme is a `[data-theme]` block in the stylesheet; a plugin's is
 * the same tokens set on `<html>` from JavaScript. Both end up as the same
 * custom properties, which is the whole reason a plugin can ship a theme
 * without shipping any code: the design's rule that colour is a token set is
 * what makes an extension point out of it. */
const plugged = new Map();

/// The tokens a theme may set. An allow-list, because these arrive from a
/// file somebody else wrote: without it a "theme" could set any custom
/// property the stylesheet reads, which is a stylesheet rather than a palette.
const TOKENS = new Set([
  "bg", "bg-deep", "raise", "text", "text-hi",
  "accent", "accent-2", "ok", "warn", "err", "info", "tone",
  "wall-a", "wall-b", "wall-c",
]);

/* ── settings ─────────────────────────────────────────────────────────

   `.proton/settings.json` is the truth; `localStorage` is a cache in front of
   it. The cache earns its place by being readable *before* the core answers:
   without it the window paints in the default theme and then switches, which
   is a flash on every launch.

   `applying` is the loop guard. Applying a value the core just sent must not
   turn round and tell the core about it — that is an echo, and with a file
   write on the other end it is an echo that touches the disk. */
let lastSettings = {};
let applying = false;

/** Remember a choice, in the file and in the cache in front of it. */
function keep(key, value) {
  if (applying) return;
  // Only the two the window needs before the core answers are cached. Caching
  // the rest would mean two places to disagree about what a setting is, and
  // the file is the one that decides.
  if (key === "theme" || key === "translucency") {
    try {
      localStorage.setItem(`proton.${key}`, String(value));
    } catch {
      /* private window, blocked storage — it just won't survive a restart */
    }
  }
  dispatch({ type: "setSetting", key, value });
}

/** One value out of the settings, by dotted key. */
function setting(key) {
  return key.split(".").reduce((node, part) => node?.[part], lastSettings);
}

function renderSettings(event) {
  lastSettings = event.values ?? {};

  const where = $("[data-settings-path]");
  if (where) where.textContent = event.path || "";

  applying = true;
  try {
    if (typeof lastSettings.theme === "string") setTheme(lastSettings.theme);
    if (typeof lastSettings.translucency === "string") {
      setTranslucency(lastSettings.translucency);
    }

    displaySettings.apply(lastSettings);
    // A new font size is a new cell size: the panel is the same size and
    // holds a different number of rows, which no resize would announce.
    requestAnimationFrame(() => reportTerminalSize());

    // The controls show what is set, and a control with nothing set shows the
    // default — which is why each one carries its own.
    $$("[data-set-bool]").forEach((box) => {
      const value = setting(box.dataset.setBool);
      box.checked = typeof value === "boolean" ? value : box.defaultChecked;
    });
    $$("[data-set-text]").forEach((field) => {
      if (document.activeElement === field) return; // do not type over them
      field.value = setting(field.dataset.setText) ?? "";
    });
    $$("[data-set-choice]").forEach((group) => {
      const chosen = setting(group.dataset.setChoice);
      const buttons = $$("button", group);
      // First option is the default when nothing has been chosen.
      const want = chosen ?? buttons[0]?.dataset.value;
      buttons.forEach((b) => b.classList.toggle("on", b.dataset.value === want));
    });
  } finally {
    applying = false;
  }
}

/* ── the library ──────────────────────────────────────────────────────
   The roots are the whole permission model: a directory listed here is one
   the agent may read and cite, and one that is not listed is unreachable.
   So it is drawn as a list you can see and remove from, not as a switch. */

/** Which sources the library is narrowed to, as the last library event said.
 *  Kept here so the `␣fl` picker can say so in its header: a scope set days
 *  ago is the quietest possible reason for a thin answer, and the settings
 *  panel is not where you are looking when you get one. */
let libraryScope = [];

/** The roots as the settings file has them, which is what an edit rewrites.
 *  Index-aligned with the resolved ones the core reports back. */
function libraryRoots() {
  const roots = setting("library.roots");
  return Array.isArray(roots) ? roots.filter((r) => typeof r === "string") : [];
}

function renderLibrary(event) {
  const list = $("[data-library-roots]");
  if (list) {
    const missing = new Set(event.missing ?? []);
    list.innerHTML = event.roots.length
      ? event.roots
          .map(
            (root, i) =>
              `<div class="lib-root"><span class="path" title="${escapeHtml(root)}">${escapeHtml(root)}</span>` +
              (missing.has(root) ? `<span class="gone">not found</span>` : "") +
              `<button data-library-drop="${i}" title="Stop reading this directory">&times;</button></div>`,
          )
          .join("")
      : `<div class="lib-root"><span class="path" style="color:var(--ink-3)">Nothing listed — the agent cannot read any of your notes.</span></div>`;
  }

  // The sources, and which are ticked. Rebuilt only when the list itself
  // changes — a walk reports progress many times a second, and replacing the
  // markup under a pointer mid-click loses the click.
  const picks = $("[data-library-sources]");
  const scope = new Set(event.scope ?? []);
  // Remembered, because the picker's header needs it and the picker's own
  // event has no business carrying a field about the library.
  libraryScope = [...scope];
  if (picks) {
    const shape = (event.sources ?? []).map((s) => `${s.path}:${s.documents}`).join("|");
    if (picks.dataset.built !== shape) {
      picks.dataset.built = shape;
      picks.innerHTML = (event.sources ?? []).length
        ? event.sources
            .map(
              (s) =>
                `<label class="lib-source"><input type="checkbox" data-library-pick="${escapeHtml(s.path)}">` +
                `<span class="name">${escapeHtml(s.label)}</span>` +
                `<span class="size">${s.documents.toLocaleString()} doc${s.documents === 1 ? "" : "s"} · ${s.passages.toLocaleString()}</span></label>`,
            )
            .join("")
        : `<p style="color:var(--ink-4)">Nothing read yet.</p>`;
    }
    // Ticks are set every time, since they change without the list changing.
    for (const box of $$("[data-library-pick]", picks)) {
      box.checked = scope.has(box.dataset.libraryPick);
    }
  }
  // Where the questions are asked, not only where they are configured: a
  // narrowed library that says so nowhere is one you forget you narrowed.
  for (const chip of $$("[data-scope-chip]")) {
    chip.hidden = scope.size === 0;
    chip.textContent =
      scope.size === 1 ? [...scope][0] : `${scope.size} sources`;
    chip.title = `Answers are drawn from ${[...scope].join(", ")}. Click to use everything.`;
  }

  // "matching by word" -> "Matching by word". The core writes these lowercase
  // because it does not know whether they start a sentence; here they do.
  const sentence = (text) => (text ? text[0].toUpperCase() + text.slice(1) : "");

  const status = $("[data-library-status]");
  if (!status) return;
  const n = (x) => x.toLocaleString();
  const held =
    `${n(event.passages)} passage${event.passages === 1 ? "" : "s"} from ` +
    `${n(event.documents)} document${event.documents === 1 ? "" : "s"}`;

  if (!event.roots.length) {
    status.textContent = "No roots listed.";
  } else if (event.learning) {
    // Reading has finished and the vectors are being made. Said separately
    // because the old text called this "Reading", which was wrong twice over:
    // the documents were all in — it would have read "1,307 of 1,307" — and
    // the thing actually taking the time was not reading at all.
    //
    // The second sentence is the one that matters. A search works right now
    // and answers by word, which is a good answer, and nothing in the results
    // reveals that the other half is missing.
    status.textContent =
      `${held}. Learning what they mean — searches answer by word until it finishes.`;
  } else if (event.indexing) {
    // A first walk over a corpus of PDFs runs for minutes, so it says how far
    // it has got rather than "reading" — which after two minutes reads as a
    // hang. What is already in is searchable while the rest arrives.
    const far = event.found ? ` — ${n(event.read)} of ${n(event.found)} documents` : "";
    status.textContent = `Reading${far}. ${held} so far.`;
  } else {
    // "Nothing found" and "not read yet" are different answers, and the
    // count is what tells them apart.
    const took = event.elapsedMs ? `, read in ${n(event.elapsedMs)} ms` : "";
    status.textContent = `${held}${took}. ${sentence(event.meaning)}.`;
  }
  // Only worth colouring when something is wrong: a library matching by word
  // because nobody asked for more is not a warning.
  status.classList.toggle("st-warn", /—/.test(event.meaning || ""));
}

/** What is ticked now, read off the panel rather than kept in a second place
 *  that could disagree with it. */
function pickedSources() {
  return $$("[data-library-pick]")
    .filter((box) => box.checked)
    .map((box) => box.dataset.libraryPick);
}

document.addEventListener("change", (e) => {
  const pick = e.target.closest?.("[data-library-pick]");
  if (pick) dispatch({ type: "setLibraryScope", sources: pickedSources() });
});

document.addEventListener("click", (e) => {
  // The chip in the composer and the button in the panel do the same thing:
  // stop narrowing. An empty selection is "no restriction".
  if (e.target.closest?.("[data-library-all]") || e.target.closest?.("[data-scope-chip]")) {
    dispatch({ type: "setLibraryScope", sources: [] });
    return;
  }
  if (e.target.closest?.("[data-library-refresh]")) {
    dispatch({ type: "refreshLibrary" });
    return;
  }
  const add = e.target.closest?.("[data-library-add]");
  if (add) {
    const field = $("[data-library-new]");
    const path = field?.value.trim();
    if (!path) return;
    keep("library.roots", [...libraryRoots(), path]);
    field.value = "";
    return;
  }
  const drop = e.target.closest?.("[data-library-drop]");
  if (drop) {
    const roots = libraryRoots();
    roots.splice(Number(drop.dataset.libraryDrop), 1);
    // An emptied list is written as an empty array rather than removed: the
    // difference between "no roots" and "no opinion" does not matter to the
    // core, and a key that vanishes reads as a settings file that lost it.
    keep("library.roots", roots);
  }
});

// Enter in the field is the same as clicking Add — nobody reaches for the
// mouse after typing a path.
document.addEventListener("keydown", (e) => {
  if (e.key !== "Enter") return;
  if (!e.target.closest?.("[data-library-new]")) return;
  e.preventDefault();
  $("[data-library-add]")?.click();
});

// A citation in an answer opens the note it names. The core resolves it,
// because turning those words back into a file needs the roots — and needs
// to refuse a citation that points outside them.
document.addEventListener("click", (e) => {
  const cite = e.target.closest?.("[data-cite]");
  if (cite) dispatch({ type: "openCitation", cite: cite.dataset.cite });
});

// In an answer or a notebook's prose, a code block's button copies the code,
// and a link opens in the browser rather than in the editor's own window.
// (Chat's answers have the same two, handled in claude.js.)
document.addEventListener("click", (e) => {
  if (!e.target.closest?.("[data-agent-thread], [data-cells]")) return;
  const link = e.target.closest("a[data-link]");
  if (link) {
    e.preventDefault();
    dispatch({ type: "openLink", url: link.getAttribute("href") });
    return;
  }
  const copy = e.target.closest("[data-copy]");
  if (!copy) return;
  const code = copy.closest(".md-code")?.querySelector("code")?.textContent ?? "";
  navigator.clipboard?.writeText(code).then(() => {
    copy.textContent = "Copied";
    setTimeout(() => { copy.textContent = "Copy"; }, 1200);
  }).catch(() => {});
});

/* The settings page. Each nav row shows one panel; nothing else is hidden or
   shown, so a panel that has not been built cannot be reached. */
document.addEventListener("click", (e) => {
  const nav = e.target.closest?.("[data-st-nav]");
  if (!nav) return;
  const want = nav.dataset.stNav;
  $$("[data-st-nav]").forEach((r) => r.classList.toggle("sel", r === nav));
  $$("[data-st-panel]").forEach((panel) => {
    panel.hidden = panel.dataset.stPanel !== want;
  });
});

/* The controls themselves. Each writes one key and nothing else knows which
   key that was — the markup says. */
document.addEventListener("change", (e) => {
  const box = e.target.closest?.("[data-set-bool]");
  if (box) {
    keep(box.dataset.setBool, box.checked);
    return;
  }
  const field = e.target.closest?.("[data-set-text]");
  if (field) {
    // An emptied field means "back to the default", which is a removal
    // rather than a stored empty string.
    const value = field.value.trim();
    keep(field.dataset.setText, value === "" ? null : value);
  }
});

document.addEventListener("click", (e) => {
  const choice = e.target.closest?.("[data-set-choice] button");
  if (!choice) return;
  const group = choice.closest("[data-set-choice]");
  $$("button", group).forEach((b) => b.classList.toggle("on", b === choice));
  if (group.dataset.setChoice === "agent.provider" && setting("agent.provider") !== choice.dataset.value) {
    dispatchMany([
      {type: "setSetting", key: "agent.model", value: null},
      {type: "setSetting", key: "agent.provider", value: choice.dataset.value},
    ]);
    return;
  }
  keep(group.dataset.setChoice, choice.dataset.value);
});

function setTheme(name) {
  const plugin = plugged.get(name);
  if (!plugin && !THEMES.includes(name)) return;

  // A plugin's tokens are set on the element and a built-in's come from the
  // stylesheet, so switching between them has to clear whatever the last one
  // put there — otherwise a plugin's accent survives into Tokyo Night.
  TOKENS.forEach((t) => document.documentElement.style.removeProperty(`--${t}`));

  if (plugin) {
    // A built-in stays underneath as the base, so a palette that sets five
    // tokens gets sensible values for the other ten rather than the browser's
    // idea of an unset custom property. The plugin's own tokens are set on
    // the element, which beats any stylesheet block.
    document.documentElement.dataset.theme = "tokyo-night";
    document.documentElement.dataset.pluginTheme = plugin.id;
    for (const [key, value] of plugin.colours) {
      if (TOKENS.has(key)) document.documentElement.style.setProperty(`--${key}`, value);
    }
    // `--raise` is what "lighter" means on this ground, and on a light one
    // lighter is black. A palette may say so itself; most will not.
    if (!plugin.colours.some(([k]) => k === "raise")) {
      document.documentElement.style.setProperty(
        "--raise",
        plugin.dark ? "255, 255, 255" : "0, 0, 0",
      );
    }
  } else {
    document.documentElement.dataset.theme = name;
    delete document.documentElement.dataset.pluginTheme;
  }

  $$("[data-theme-set]").forEach((c) => c.classList.toggle("on", c.dataset.themeSet === name));
  // A new theme is a new ground for Codex's opaque window, and new colours
  // for a terminal the GPU draws.
  renderWindowGround();
  termGL?.restyle();
  keep("theme", name);
  requestAnimationFrame(drawPointerEdges); // edges are painted, not tokenised
}

/* Delegated rather than bound per card: the plugin themes' cards do not exist
   until the core says what is installed, and a listener attached at load
   would miss every one of them. */
document.addEventListener("click", (e) => {
  const card = e.target.closest("[data-theme-set]");
  if (card) setTheme(card.dataset.themeSet);
});

/* ── translucency ─────────────────────────────────────────────────────
   Three steps over one set of veil percentages. Like the theme, it is a
   token change: no rule below the veil block knows which step is live. */
function setTranslucency(step) {
  if (!["subtle", "medium", "heavy"].includes(step)) return;
  document.documentElement.dataset.translucency = step;
  $$("[data-translucency-set]").forEach((b) =>
    b.classList.toggle("on", b.dataset.translucencySet === step),
  );
  keep("translucency", step);
}

$$("[data-translucency-set]").forEach((b) =>
  b.addEventListener("click", () => setTranslucency(b.dataset.translucencySet)),
);

/* ── workspace modes ──────────────────────────────────────────────────
   The mode bar asks the core to switch; the core's ModeChanged event is
   what actually moves the UI. That indirection looks pointless with one
   front end and is the reason a second one is possible. */
/** The raster pictures a notebook output may be, as the core chooses them. */
const RASTER = new Set(["image/png", "image/jpeg", "image/gif", "image/webp"]);

const MODES = ["chat", "codex", "data", "research", "study", "design", "modeling", "game", "video"];

/** Which mode is on screen. The core owns it; this is a copy for the handful
 *  of places that have to behave differently in one of them. */
let activeMode = "codex";

/* ── the window's ground ───────────────────────────────────────────────
   Codex paints every pixel of its own, so the window under it need not be
   see-through: an opaque one spares the compositor a blur of the desktop that
   Codex's ground hides completely. The modes drawn as glass get their
   material back. Asked for only when the answer changes. */
let groundAsked;

/** A CSS colour as `#rrggbb`, whatever form the theme wrote it in. */
function hexOf(css) {
  const probe = document.createElement("i");
  probe.style.color = css;
  document.body.append(probe);
  const channels = getComputedStyle(probe).color.match(/\d+(\.\d+)?/g);
  probe.remove();
  if (!channels) return null;
  return "#" + channels.slice(0, 3).map((v) => Math.round(Number(v)).toString(16).padStart(2, "0")).join("");
}

function renderWindowGround(again = false) {
  if (!HAS_SHELL) return;
  // Chat, Data, Research and Notebook are drawn as Codex is — every pixel
  // the theme's ground — so they get the same opaque window.
  const want = ["codex", "chat", "data", "research", "study"].includes(activeMode) ? hexOf(token("--bg")) : null;
  if (!again && want === groundAsked) return;
  groundAsked = want;
  setOpaque(want);
}

function renderMode(mode) {
  activeMode = mode;
  // Codex, and the modes drawn as it is, wear a different chrome from the
  // rest — a title bar in place of the rails, no statusline — and the
  // stylesheet keys all of it off this.
  document.documentElement.dataset.mode = mode;
  // After the mode is set, not before: Codex has a ground of its own, and
  // reading the token any earlier gets the mode being left.
  renderWindowGround();
  termGL?.restyle();
  $$(".mode").forEach((b) => b.setAttribute("aria-selected", String(b.dataset.mode === mode)));
  $$(".workspace").forEach((w) => w.classList.toggle("on", w.dataset.ws === mode));
  closeAll();
  panels.refresh();
  // The container that was just revealed is empty — the splits were drawn
  // into the mode we came from. Redraw them here, and the notebook with them.
  if (lastSplits) {
    renderSplits(lastSplits.views, lastSplits.focused, lastSplits.vertical);
  }
  if (mode === "data" && lastNotebook) renderNotebook(lastNotebook);
  requestAnimationFrame(reportEveryViewport);
}

$$(".mode").forEach((b) => b.addEventListener("click", () => coreSetMode(b.dataset.mode)));
$$("[data-mode-go]").forEach((el) =>
  el.addEventListener("click", () => coreSetMode(el.dataset.modeGo)),
);

/* ── right dock ───────────────────────────────────────────────────────── */

/** What the dock's header says while each panel is open. The agent's is the
 *  conversation's own title, from `renderChats`. */
const DOCK_TITLES = { tasks: "Tasks", debug: "Debug", visualizer: "Visualizer", device: "Device", git: "Git" };
let dockPanel = "agent";

function setDock(panel) {
  const dock = $("#codex-dock");
  if (!dock) return;
  showDockMenu(false);
  // The terminal left the dock for the stage, beside the code. Asking the
  // dock for it still gets it — that is where it is now.
  if (panel === "term") {
    if (activeMode !== "codex") coreSetMode("codex");
    showTerminal(true, { focus: true });
    return;
  }
  dockPanel = panel;
  panels.show("right", dock.closest(".workspace"));
  $$(".dock-tab", dock).forEach((t) => t.classList.toggle("on", t.dataset.dockpanel === panel));
  $$(".dock-panel", dock).forEach((p) => {
    const on = p.dataset.panel === panel;
    p.classList.toggle("on", on);
    p.style.display = on ? "flex" : "none";
  });
  dock.dataset.showing = panel;
  renderDockTitle();
  requestAnimationFrame(drawPointerEdges);

  // Git's answer moves for reasons the core never sees — a commit in the
  // terminal panel, a branch switched outside — so opening the panel asks
  // again rather than showing whatever was true at the last sync.
  if (panel === "git") dispatch({ type: "refreshRepo" });
}

function renderDockTitle() {
  const title = $("[data-dock-title]");
  if (!title) return;
  const open = lastChats.conversations.find((c) => c.path === lastChats.current);
  title.textContent = dockPanel === "agent" ? open?.title || "New Thread" : DOCK_TITLES[dockPanel] ?? "";
  title.title = title.textContent;
}

/** The panel list behind the header's `⋯`. A menu rather than a row of tabs:
 *  one panel is open at a time, and it is usually the agent. */
function showDockMenu(on) {
  const menu = $("#codex-dock .dock-tabs");
  const button = $("[data-dock-menu]");
  if (!menu || !button) return;
  menu.hidden = !on;
  button.setAttribute("aria-expanded", String(on));
  if (on) ($(".dock-tab.on", menu) ?? $(".dock-tab", menu))?.focus();
}

$$("#codex-dock .dock-tab").forEach((t) =>
  t.addEventListener("click", () => setDock(t.dataset.dockpanel)),
);

document.addEventListener("click", (e) => {
  const menu = $("#codex-dock .dock-tabs");
  if (!menu) return;
  if (e.target.closest?.("[data-dock-menu]")) showDockMenu(menu.hidden);
  else if (!menu.hidden && !e.target.closest?.("#codex-dock .dock-tabs")) showDockMenu(false);
});

// Arrows walk the menu and Escape puts it away, back on the button that
// opened it — the keys every menu answers to.
$("#codex-dock .dock-tabs")?.addEventListener("keydown", (e) => {
  const items = $$("#codex-dock .dock-tab");
  const at = items.indexOf(document.activeElement);
  if (e.key === "Escape") {
    e.preventDefault();
    e.stopPropagation();
    showDockMenu(false);
    $("[data-dock-menu]")?.focus();
  } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    e.stopPropagation();
    const step = e.key === "ArrowDown" ? 1 : -1;
    items[(at + step + items.length) % items.length]?.focus();
  }
});

// Rail and tree shortcuts that jump straight to a dock panel.
$$("[data-dock]").forEach((el) =>
  el.addEventListener("click", () => {
    coreSetMode("codex");
    setDock(el.dataset.dock);
  }),
);

// Dock tab groups elsewhere are view state only.
$$(".dock-tabs").forEach((group) => {
  if (group.closest("#codex-dock")) return;
  $$(".dock-tab", group).forEach((t) =>
    t.addEventListener("click", () => {
      $$(".dock-tab", group).forEach((o) => o.classList.remove("on"));
      t.classList.add("on");
    }),
  );
});

$$(".seg, .builder-rail").forEach((group) => {
  // Both pickers own their own selected state — the backdrop one because it
  // reflects what the compositor answered rather than what was clicked, the
  // translucency one because it restores from storage on boot. Neither wants
  // the optimistic view-state handler.
  if (group.id === "backdrop-picker" || group.id === "translucency-picker") return;
  $$("button", group).forEach((b) =>
    b.addEventListener("click", () => {
      $$("button", group).forEach((o) => o.classList.remove("on"));
      b.classList.add("on");
    }),
  );
});

// Design's left panel: two things that exist, and a rail that switches
// between them. The generic handler above has already moved the `on` class;
// this is what makes the click mean something.
// Clicking an object in the viewport selects it, exactly as clicking its row
// does — one selection, owned by the core, however it was chosen.
startViewport(
  $("[data-viewport]"),
  (index) => {
    if (index >= 0) dispatch({ type: "selectNode", index });
  },
  showViewportCount,
);

$$("[data-design-panel]").forEach((button) => {
  button.addEventListener("click", () => {
    panels.show("left");
    const want = button.dataset.designPanel;
    $$("[data-design-side]").forEach((el) => {
      el.hidden = el.dataset.designSide !== want;
    });
  });
});

$$(".tree").forEach((tree) => {
  $$(".row", tree).forEach((r) =>
    r.addEventListener("click", () => {
      $$(".row", tree).forEach((o) => o.classList.remove("sel"));
      r.classList.add("sel");
    }),
  );
});

/* ── splits ───────────────────────────────────────────────────────────
   A split is a *view* onto a buffer, and the core owns every one of them:
   which buffer it shows, where its cursor is, where it is scrolled to. This
   builds the panes the core describes and reports each one's height back.

   It used to clone the markup, which made two panes onto one document and
   two panes onto two copies indistinguishable — and neither of them a split. */
function splits() {
  // One split layout in the core, drawn into whichever mode is on screen.
  // Codex splits a buffer and Data splits a notebook, but the arithmetic of
  // how many panes there are and which has the focus is the same for both,
  // so there is one of it.
  return activeMode === "data" ? $("#data-splits") : $("#codex-splits");
}

/** The pane that has the focus, in Data mode. */
function focusedCells() {
  const view = lastSplits?.focused ?? 0;
  return $(`[data-cells][data-view="${view}"]`) ?? $("[data-cells]");
}

/** The markup of one pane. Everything in it is addressed by view index. */
function splitHtml(view, focused) {
  const actions =
    `<span class="split-actions">` +
    (activeMode === "codex" ? `<button data-history="undo" title="Undo" aria-label="Undo"><svg class="i" aria-hidden="true"><use href="#i-undo"/></svg></button><button data-history="redo" title="Redo" aria-label="Redo"><svg class="i" aria-hidden="true"><use href="#i-redo"/></svg></button>` : "") +
    `<button data-split="v" title="Split right — :vsplit or ␣wv"><svg class="i"><use href="#i-split-v"/></svg></button>` +
    `<button data-split="s" title="Split down — :split or ␣ws"><svg class="i"><use href="#i-split-h"/></svg></button>` +
    `<button data-split="close" title="Close this split — :close or ␣wq"><svg class="i"><use href="#i-close"/></svg></button>` +
    `</span>`;
  const on = view === focused ? " on" : "";

  // There is one notebook behind every Data pane, so there are no buffer tabs
  // to draw — the panes differ only in which cell they are on.
  if (activeMode === "data") {
    return (
      `<section class="split${on}" data-view="${view}">` +
      `<div class="buffers"><span class="grow"></span>${actions}</div>` +
      `<div class="scroll" tabindex="0" data-cells data-view="${view}"></div>` +
      `</section>`
    );
  }
  return (
    `<section class="split${on}" data-view="${view}">` +
    `<div class="buffers">` +
    // Where the file is, in place of the tabs while they are hidden — what
    // Zed's breadcrumb shows with its tab bar turned off.
    `<span class="crumbs" data-crumbs></span>` +
    `<span data-buffers></span>` +
    actions +
    `</div>` +
    `<div class="scroll code" role="tabpanel" tabindex="0" id="editor-view-${view}" data-editor data-view="${view}"></div>` +
    `</section>`
  );
}

/**
 * Rebuild the panes from the core's layout.
 *
 * Only when the shape actually changed. A `Splits` event also arrives when
 * the focus moves, and tearing down the DOM for that would throw away every
 * pane's scroll position and make switching splits scroll them all to the top.
 */
function renderSplits(views, focused, vertical) {
  // Kept because each mode draws this same layout into its own container, and
  // a mode that has just come on screen has never drawn it.
  lastSplits = { views, focused, vertical };
  const box = splits();
  if (!box) return;

  const shapeChanged =
    $$(".split", box).length !== views.length ||
    box.classList.contains("stacked") === vertical;

  if (shapeChanged) {
    restorePaneFocus ||= box.contains(document.activeElement);
    $$("[data-editor]", box).forEach(pane => viewportObserver.unobserve(pane));
    // A divider between each pair. Rebuilt with the panes rather than kept,
    // because splitting and closing renumber everything either side of them.
    box.innerHTML = views
      .map((_, i) => (i ? `<div class="sp-gutter" data-sp="${i}"></div>` : "") + splitHtml(i, focused))
      .join("");
    box.classList.toggle("stacked", !vertical);
    box.classList.toggle("multi", views.length > 1);
    // The panes' markup has just been thrown away, so what each of them
    // remembers having painted describes rows that no longer exist. The
    // renderer would notice anyway — it rebuilds when it finds no window —
    // but a cache that outlives the thing it describes should be cleared
    // where the thing is destroyed, not left to be caught downstream.
    for (const pane of panes.values()) pane.painted = null;
    // Splitting resets the shares. Two panes from one are even, which is what
    // you asked for; carrying the old ratios over would give the new pane a
    // width nobody chose.
    $$(".split", box).forEach((pane) => pane.style.removeProperty("--share"));
    $$("[data-editor]", box).forEach((pane) => viewportObserver.observe(pane));
    // Each pane has its own height, and the core has to be told all of them.
    requestAnimationFrame(reportEveryViewport);
  } else {
    $$(".split", box).forEach((pane) =>
      pane.classList.toggle("on", Number(pane.dataset.view) === focused),
    );
  }
  $$("[data-split=close]", box).forEach(button => { button.disabled = views.length <= 1; });
  splitResize.refresh();
  view.focused = focused;
  // Which buffer each pane is showing, so its strip can mark the right tab.
  bufferBar.panes = views.map((v) => v.buffer);
  renderBufferBars();
  requestAnimationFrame(drawPointerEdges);
}

/** Which pane a node is in, or the focused one. */
function viewOf(node) {
  const pane = node?.closest?.("[data-view]");
  return pane ? Number(pane.dataset.view) : view.focused;
}

document.addEventListener("click", (e) => {
  const action = e.target.closest("[data-split]");
  if (action) {
    e.stopPropagation();
    if (action.disabled) return;
    action.focus();
    const index = viewOf(action);
    const kind = action.dataset.split;
    if (kind === "v") dispatchMany([{ type: "focusSplit", index }, { type: "split", vertical: true }]);
    else if (kind === "s") dispatchMany([{ type: "focusSplit", index }, { type: "split", vertical: false }]);
    else dispatch({ type: "closeSplit", index });
    return;
  }
  // Clicking a panes chrome — its buffer bar, its empty space — focuses it.
  // A click in the *text* is handled on mousedown, which fires first and has
  // to focus before it places the cursor.
  const pane = e.target.closest(".split");
  if (pane && splits()?.contains(pane)) {
    if (e.target.closest('[data-editor], [data-history], [data-buffer], [data-buffer-close]')) return;
    const index = Number(pane.dataset.view);
    dispatch({ type: "focusSplit", index });
  }
});

/* ── resizing the editor splits ───────────────────────────────────────
   Two panes, one divider, and the only thing that moves is how the pair
   share the space between them. Everything further out keeps its size, so
   dragging the divider between panes 2 and 3 cannot shove pane 1 about.

   The shares live in the DOM as `--share` rather than in the core: the core
   owns which buffer a view shows and where its cursor is, and `Command` and
   `Event` deliberately name no pixels. A second front end would size its
   panes its own way. */
const splitResize = createSplitResize(document, reportEveryViewport);
let restorePaneFocus = false;

/* Panel geometry is view state, independent of the document renderers. */
const panels = createPanels(document, () => {
  requestAnimationFrame(() => {
    if (!$("[data-term-screen]")?.offsetParent) focusTerminal(false);
    reportEveryViewport();
    reportTerminalSize();
    drawPointerEdges();
  });
});
const displaySettings = createDisplaySettings(document, keep, () => {
  // Not in the frame below: nothing here waits on layout, and a renderer
  // switched on should not wait for the next frame to exist.
  renderTerminalRenderer();
  requestAnimationFrame(() => {
    measureLineHeight();
    if (activeMode === "codex") $$(".workspace.on [data-editor]").forEach((pane) => renderEditor(Number(pane.dataset.view)));
    panels.refresh();
  });
});

/* Chat: Claude Code. The module draws what the core sends; the council it
   hands back here, because the council is Proton's, not Claude Code's. */
const claude = createClaude({
  root: document,
  dispatch,
  panels,
  shell: HAS_SHELL,
  convene: (question) => dispatch({ type: "convene", question }),
});

/* Research: the project's investigations. Drawn from what the core reads
   out of research/; nothing is kept here that the files do not say. */
const research = createResearch({ root: document, dispatch });
/* The Lab: the Python an investigation's code runs in, made with your say-so. */
const lab = createLab({ root: document, dispatch, readLab });
/* Lean and Mathlib, in which a mathematics investigation's proofs are checked. */
const lean = createLean({ root: document, dispatch });
/* Notebook: notebooks of your own sources, and the knowledge base they are
   chosen from. Drawn from what the core reads of them. */
const study = createStudy({ root: document, dispatch });

/* ── overlays ─────────────────────────────────────────────────────────── */
const OVERLAYS = ["telescope", "whichkey", "cmdline", "settings"];
const overlays = createOverlays(document, OVERLAYS);

function closeAll() {
  overlays.hide();
}

async function dismissOverlay() {
  const coreOwned = pickerOpen || (overlays.active && overlays.active !== "settings");
  pickerOpen = false;
  closeAll();
  if (coreOwned) await sendKey({ key: "Escape" });
}

/**
 * Toggle an overlay. For buttons: clicking Settings twice closes it.
 *
 * Never call this from an event handler. An event that fires per keystroke —
 * the command line, the picker — would toggle the overlay off on every second
 * key, which looks exactly like the panel flickering. Use `show`.
 */
async function open(id) {
  const wasOpen = overlays.active === id;
  if (overlays.active || pickerOpen) await dismissOverlay();
  if (wasOpen) return;
  if (id === "telescope") {
    // Own input immediately, including while IPC is opening the picker.
    pickerOpen = true;
    $("[data-picker-query]").value = "";
    $("[data-picker-results]").innerHTML = '<div class="empty">Opening search…</div>';
    show(id);
    await dispatch({ type: "openPicker", kind: "files" });
    return;
  }
  show(id);
}

/** Open an overlay, idempotently. Safe to call on every keystroke. */
function show(id) {
  overlays.show(id);
}

$$("[data-open]").forEach((el) => el.addEventListener("click", () => open(el.dataset.open)));
$$("[data-close]").forEach((el) => el.addEventListener("click", dismissOverlay));
$("#scrim").addEventListener("click", dismissOverlay);

function openFileAction(label, query) {
  return `<div class="empty-actions"><button class="btn" data-find-file="${query}">${label}</button></div>`;
}
document.addEventListener("click", async (e) => {
  const button = e.target.closest?.("[data-find-file]");
  if (!button) return;
  await open("telescope");
  dispatch({ type: "pickerQuery", text: button.dataset.findFile });
});

/* ── the editor view ──────────────────────────────────────────────────
   The core owns the text; this only draws what the last Viewport event
   said. Nothing here computes a line number, a length or a position —
   if it did, the two sides could disagree, and the one with the rope
   would be right. */

const view = {
  // Which split has the focus. The panes are drawn from the core's layout and
  // addressed by index; this is the one the keyboard is in.
  focused: 0,
  firstLine: 0,
  lines: [],
  totalLines: 0,
  cursor: { line: 0, column: 0 },
  /// The far end of the selection, or null. The near end is the cursor.
  selection: null,
  /// Which visual mode is live, because the same two endpoints describe a
  /// run, a set of lines or a rectangle depending on it.
  vim: "NORMAL",
};

/**
 * What each split is showing, keyed by view index.
 *
 * A split is a view onto a buffer, so every one of them has its own scroll,
 * its own cursor and its own lines — that is the difference between a split
 * and a second copy of the same pane, and it has to be true here as well as
 * in the core.
 */
const editorState = createEditorState();
const panes = editorState.panes;
const paneState = editorState.pane;
const editorPaints = createEditorPaints(paintEditor);
function renderEditor(index = view.focused) { editorPaints.request(index); }

/** The focused pane's state, for the statusline and the completion popup. */
const focusedPane = () => paneState(view.focused);

/* Diagnostics for the active buffer, indexed by line.
 *
 * Held here rather than merged into `view.lines` because the two arrive on
 * different clocks: text comes back with the keystroke, diagnostics turn up
 * whenever the server finishes thinking. Keeping them apart means a late
 * diagnostic never has to wait for a redraw of the text, and a redraw of the
 * text never has to wait for a server. */
const marks = { byLine: new Map(), errors: 0, warnings: 0 };

/** Whether the popup is showing. The core is the authority; this mirrors it
 * so the keydown handler can suppress the browser default before the key
 * makes the round trip. */
let completionOpen = false;

/* ── icons ────────────────────────────────────────────────────────────
   Two sprites. The `.i` set lives in index.html and takes the theme's
   colours; files and folders wear Charmed Icons, the set the Zed this
   window is drawn after uses, which bring colours of their own — see
   file-icons.js. Installed before anything renders a row. */
installFileIcons(document);

/** What a file gets in the tree, the buffer bar and a task's artifacts. */
const fileIcon = fileIconHtml;

/* ── scrolling ────────────────────────────────────────────────────────
   The core sends one screen of lines, so the pane cannot simply scroll —
   there is nothing below the last line to scroll into. Instead the document
   is represented by a spacer of its full height, and the loaded window is
   translated to sit at the right place inside it. The scrollbar is then the
   real one for the file, and scrolling asks the core for a different window.

   `first_line` stays the core's to decide. The page proposes one when the
   user scrolls; everything that moves the window for its own reasons — `G`,
   a grep hit, the cursor leaving the screen — comes back as a viewport event
   like any other, and the pane follows it. */

/** Height of one rendered line. Measured, because the theme sets it. */
let lineHeight = 20;

function measureLineHeight() {
  const line = $(".workspace.on [data-editor] .ln");
  const measured = line?.getBoundingClientRect().height;
  if (measured > 0) lineHeight = measured;
  else {
    const target = $(".workspace.on [data-editor]");
    if (target) lineHeight = parseFloat(getComputedStyle(target).lineHeight) || 20;
  }
  return lineHeight;
}

function paintEditor(index) {
  const target = $(`[data-editor][data-view="${index}"]`);
  if (!target) return;
  editorPainted(paneState(index).lines.length);
  editorFeedback.empty(index, bufferBar.names[bufferBar.panes[index] ?? bufferBar.active]?.name, paneState(index).lines);
  const drawing = paneState(index);
  // Measured before building, because the spacer's height depends on it. With
  // no line rendered yet this falls back to the pane's computed line-height,
  // which is the same number.
  measureLineHeight();
  // How many digits the last line number has. The gutter is as wide as the
  // file needs and no wider, as Zed's is — a fixed width is too much for a
  // ten-line file and too little for a hundred-thousand-line one.
  const digits = String(Math.max(drawing.totalLines, 1)).length;
  if (target.style.getPropertyValue("--digits") !== String(digits)) {
    target.style.setProperty("--digits", String(digits));
  }

  const cursorRow = drawing.cursor.line - drawing.firstLine;
  const rows = drawing.lines.map((text, i) =>
    lineHtml(
      drawing.firstLine + i,
      text,
      i === cursorRow ? drawing.cursor.column : undefined,
      drawing.highlights[i],
      drawing.matches[i],
      drawing.git[i],
      drawing.stops.get(drawing.firstLine + i),
      {pane: drawing, vim: view.vim, diagnostics: marks.byLine},
    ),
  );

  // A spacer the height of the whole file, with the loaded window positioned
  // inside it. `translate` rather than `top` so moving the window is a
  // compositor job and never a reflow.
  const height = `${Math.max(1, drawing.totalLines) * lineHeight}px`;
  const shift = `translateY(${drawing.firstLine * lineHeight}px)`;
  let win = target.querySelector(".win");

  if (!win) {
    target.innerHTML =
      `<div class="doc" style="height:${height}">` +
      `<div class="win" style="transform:${shift}">${rows.join("")}</div></div>`;
    win = target.querySelector(".win");
    drawing.painted = rows;
  } else {
    // Only the rows that actually differ. The core re-sends the whole visible
    // window on every edit — which is right, it is the thing that knows what
    // the window is — but typing changes one line of sixty, and replacing the
    // pane's markup for it costs a parse and a full layout every keystroke.
    // Measured in the preview: 1.85ms for a rebuild against 0.32ms for this.
    //
    // Compared against the strings from last time rather than against the
    // DOM: reading `outerHTML` back to compare it serialises every row, which
    // is most of the work this exists to avoid.
    const before = drawing.painted ?? [];
    const kids = win.children;
    for (let i = 0; i < rows.length; i++) {
      if (before[i] === rows[i] && kids[i]) continue;
      const holder = document.createElement("div");
      holder.innerHTML = rows[i];
      const fresh = holder.firstElementChild;
      if (!fresh) continue;
      if (kids[i]) kids[i].replaceWith(fresh);
      else win.appendChild(fresh);
    }
    // The window can shrink — at the end of a file, or on a smaller pane.
    while (kids.length > rows.length) win.lastElementChild.remove();
    drawing.painted = rows;

    const doc = target.querySelector(".doc");
    if (doc && doc.style.height !== height) doc.style.height = height;
    if (win.style.transform !== shift) win.style.transform = shift;
  }

  // The window moved for a reason of the core's — `G`, a grep hit, the cursor
  // leaving the screen — so the pane has to follow. A user scroll is already
  // within one line of where it should be, which is what tells the two apart.
  const want = drawing.firstLine * lineHeight;
  if (Math.abs(target.scrollTop - want) > lineHeight) target.scrollTop = want;

  // The caret element the popup was anchored to has just been replaced, so
  // an open popup has to be told where the new one is or it stays where the
  // cursor used to be.
  const popup = $("#completion");
  if (popup && !popup.hidden) anchorCompletion(popup);
  const hover = $("#hover");
  if (hover && !hover.hidden && hoverAt) anchorHover(hover, hoverAt);
}

/* ── clicking in the text ─────────────────────────────────────────────
   A line is not one text node: syntax spans, diagnostic spans and the caret
   split it into as many as the highlighter felt like. So the column is found
   by asking the browser where the click landed and then counting characters
   from the start of the line to that point, which is correct however the row
   happens to be cut up — and stays correct if the highlighter changes. */

/** Character offset of a point within `.ln`, or null if it missed. */
function columnAt(line, x, y) {
  const hit =
    document.caretPositionFromPoint?.(x, y) ??
    (document.caretRangeFromPoint
      ? (() => {
          const r = document.caretRangeFromPoint(x, y);
          return r && { offsetNode: r.startContainer, offset: r.startOffset };
        })()
      : null);
  if (!hit || !line.contains(hit.offsetNode)) return null;

  // Everything before the hit, in document order, minus the gutter — which is
  // markup rather than text and must not count towards the column.
  const walker = document.createTreeWalker(line, NodeFilter.SHOW_TEXT);
  let column = 0;
  let node;
  while ((node = walker.nextNode())) {
    if (node.parentElement.closest(".n, .s")) continue;
    if (node === hit.offsetNode) return column + hit.offset;
    column += node.textContent.length;
  }
  return column;
}

/** Where in the document a point is, or null if it is not on a line. */
function pointAt(x, y) {
  const line = document.elementFromPoint(x, y)?.closest("[data-editor] .ln");
  if (!line) return null;
  const number = Number(line.querySelector(".n")?.textContent);
  if (!Number.isFinite(number)) return null;

  // The gutter is markup, not text. Landing there would otherwise walk past
  // every run and put the cursor at the end of the line, which is the least
  // likely thing anybody clicking a line number wanted.
  const gutter = document.elementFromPoint(x, y)?.closest(".n, .s");
  const column = gutter ? 0 : columnAt(line, x, y);
  return column === null ? null : { line: number - 1, column };
}

/* A drag is a visual selection, which is what makes it leave you somewhere
   `y` and `d` mean what you expect. The selection is drawn from core state,
   so the browser's own is suppressed — two selections disagreeing about what
   is selected is worse than either. */
let dragging = false;
let dragTo = null;

document.addEventListener("mousedown", (e) => {
  if (e.button !== 0) return;
  const at = pointAt(e.clientX, e.clientY);
  if (!at) return;

  // Stops the browser painting a second, different selection over ours.
  e.preventDefault();
  // ...which stops it moving the focus too, so that is done here. A click in
  // the text is a request to type in it; left where it was — in the project
  // panel, usually — the keyboard sent `i` to find a file instead. The shell
  // lets go here too rather than on `click`: placing the cursor redraws the
  // line under the pointer, and a press whose element is gone by the release
  // may never become a click at all.
  e.target.closest?.("[data-editor]")?.focus({ preventScroll: true });
  focusTerminal(false);
  dragging = true;

  // The core clamps: a click past the end of a short line is the ordinary
  // case, and the front end does not hold the document to check against.
  const place = { type: "placeCursor", ...at };

  // Clicking into a different split focuses it *first*. `PlaceCursor` moves
  // the focused view's cursor, so the other order would put the cursor in the
  // pane you just left.
  const index = viewOf(e.target);
  // Always focus: a previously queued click may not have painted yet, so the
  // locally rendered focus is not a safe way to omit this command.
  dispatchMany([{ type: "focusSplit", index }, place]);
});

document.addEventListener("mousemove", (e) => {
  if (!dragging) return;
  const at = pointAt(e.clientX, e.clientY);
  if (!at) return;
  // Coalesced to one per frame: a drag fires far more moves than the screen
  // can show, and each one would otherwise be a round trip.
  const queued = dragTo !== null;
  dragTo = at;
  if (queued) return;
  requestAnimationFrame(() => {
    const to = dragTo;
    dragTo = null;
    if (to) dispatch({ type: "selectTo", ...to });
  });
});

document.addEventListener("mouseup", () => {
  dragging = false;
});

/* The open buffers, and which one each pane is showing.
 *
 * Two events feed one strip per pane: `Buffers` says what is open, `Splits`
 * says which of them each pane has. Kept here so either can arrive first and
 * redraw with what the other last said. */
const bufferBar = { names: [], active: 0, panes: [] };
const bufferTabs = createBufferTabs(document, dispatchMany, fileIcon);
const editorFeedback = createEditorFeedback(document);
onCommand(editorFeedback.command);
onCommand((command, active) => {
  if (active && changesContext(command)) renderStatus("");
  if (!active && changesContext(command)) requestAnimationFrame(reportEveryViewport);
});

function renderBuffers(names, active) {
  bufferBar.names = names;
  bufferBar.active = active;
  renderBufferBars();
}

/**
 * Draw every pane's tab strip.
 *
 * Every pane, not the first one — `querySelector` finds one `[data-buffers]`
 * and there is one per split, so writing to it put every file you opened in
 * the leftmost pane's strip however many panes were open and whichever one
 * you were in.
 *
 * And each strip marks *its own* buffer, not the editor's active one: a pane
 * showing a file it is not highlighting is a pane disagreeing with itself.
 */
function renderBufferBars() {
  $$("[data-buffers]").forEach(target => {
    const index = viewOf(target);
    bufferTabs.render(target, bufferBar.names, bufferBar.panes[index] ?? bufferBar.active, index);
  });
  $$("[data-crumbs]").forEach(target => {
    const buffer = bufferBar.names[bufferBar.panes[viewOf(target)] ?? bufferBar.active];
    const path = buffer ? projectRelative(buffer.path ?? buffer.name) : "";
    if (target.textContent !== path) target.textContent = path;
    target.title = buffer?.path ?? "";
  });
}

/** The project the window is in, from Event::Workspaces, so a path can be
 *  shown from its root the way the tree shows it. */
let projectRoot = "";

/** A buffer's path as the tree would spell it: from the project's root, with
 *  forward slashes on every platform. A file outside the project keeps its
 *  whole path, because that is the only honest thing to call it. */
function projectRelative(path) {
  const slashed = path.replaceAll("\\", "/");
  const root = projectRoot.replaceAll("\\", "/").replace(/\/+$/, "");
  return root && slashed.startsWith(`${root}/`) ? slashed.slice(root.length + 1) : slashed;
}

/* ── the file tree ────────────────────────────────────────────────────
   The core owns it: what is in it, which folders are open, and which row
   is the file you are looking at. This draws rows and sends clicks back —
   it does not read a directory or decide what to hide, because then the
   tree and `␣ff` could disagree about what is in the project. */

const fileTree = createFileTree(document, dispatch, fileIcon, folderIconHtml);
function renderTree(rows) { fileTree.render(rows); }

/* ── the terminal ─────────────────────────────────────────────────────
   The core owns the pseudo-terminal and the screen it paints. This draws
   rows of runs and sends keys back — it does not interpret an escape
   sequence or decide where the cursor is, because the thing holding the
   shell already knows. */

/** Whether the terminal has the keyboard. */
let terminalFocused = false;

/* ── notebooks ────────────────────────────────────────────────────────
   The core owns the notebook, the kernel and the choice of which MIME
   type is worth drawing. This draws cells and sends the buttons back —
   it never decides what an output *is*, because a dataframe arriving as
   both HTML and plain text is a judgement about data rather than pixels,
   and that judgement is made once, in Rust. */

/** The cells drawn last time, so only the ones that moved are redrawn. */
let notebookPainted = new Map();
/** The last split layout the core sent, so any mode can redraw it. */
let lastSplits = null;
/** What the table dock last drew, so a heartbeat does not rebuild it. */
let dockPainted = null;
/** The last theme the core sent, so a row can say which token it is. */
let lastTheme = null;

/** The last notebook the core sent, so a re-render can be asked for without
 * waiting for another event — `i` has to turn prose into source before there
 * is anything to put a caret in. */
let lastNotebook = { path: "", cells: [], problem: "", language: "python" };

/** Which markdown cell is showing its source, if any.
 *
 * View state rather than the core's: whether you are looking at prose or at
 * the `#` that made it is a question about this window, and a second window
 * on the same notebook should be free to disagree.
 */
let markdownEditing = -1;

/** One output. Everything here is text or a table; no kernel markup. */
function outputHtml(out) {
  switch (out.type) {
    case "stream":
      // stderr is not an error — warnings and progress bars come this way —
      // but it is worth telling apart from what the cell meant to print.
      return `<pre class="out-stream${out.name === "stderr" ? " err" : ""}">${escapeHtml(out.text)}</pre>`;

    case "text":
      return `<pre class="out-stream">${escapeHtml(out.text)}</pre>`;

    case "table":
      return tableHtml(out.table);

    case "image": {
      // A data URL, which is the one place a kernel's bytes reach the page —
      // as an image, where they can only ever be an image. The type is
      // written as itself, not percent-encoded: a data URL's type is not
      // decoded, so `image%2Fpng` is read as text and only sniffing saved it.
      const mime = RASTER.has(out.mime) ? out.mime : "image/png";
      return `<img class="out-img" alt="output" src="data:${mime};base64,${out.base64.replace(/[^A-Za-z0-9+/=]/g, "")}">`;
    }

    case "svg":
      // SVG is text, not base64: percent-encoded, which leaves nothing that
      // could close the attribute. In an <img> its scripts do not run and its
      // styles stay its own — inlined, a figure's `*{…}` would restyle the
      // window and its scripts would be the page's.
      return `<img class="out-img out-svg" alt="output" src="data:image/svg+xml;charset=utf-8,${encodeURIComponent(out.svg)}">`;

    case "error":
      // The traceback carries ANSI colour from the kernel; it is stripped
      // rather than interpreted, because a traceback is worth reading and
      // colouring it is not worth an escape-sequence parser here.
      return (
        `<div class="out-err"><b>${escapeHtml(out.ename)}</b>: ${escapeHtml(out.evalue)}` +
        `<pre>${escapeHtml(out.traceback.map(stripAnsi).join("\n"))}</pre></div>`
      );

    default:
      return "";
  }
}

/** ANSI escapes out of a traceback. */
const ANSI = /\[[0-9;]*m/g;
const stripAnsi = (text) => text.replace(ANSI, "");

/** A table the core extracted, drawn with our own borders.
 *
 * Every cell is a jump target: a value in a dataframe is usually a thing that
 * exists in the code too, and "open sat 17 in Codex" is the whole reason the
 * cross-mode jump exists.
 */
function tableHtml(table) {
  const head = table.columns
    .map((c) => `<th>${escapeHtml(c)}</th>`)
    .join("");
  const body = table.rows
    .map(
      (row) =>
        `<tr>${row
          .map((cell, i) => {
            const numeric = /^-?[\d.,]+$/.test(cell.trim()) && cell.trim() !== "";
            return (
              `<td class="${numeric ? "n" : ""}" data-df-cell="${escapeHtml(cell)}"` +
              ` title="${escapeHtml(table.columns[i] || "")} — click to find it in the code">` +
              `${escapeHtml(cell)}</td>`
            );
          })
          .join("")}</tr>`,
    )
    .join("");
  // What the frame said it had, when it said. Showing thirty of a million
  // without mentioning the million is the one thing a table must not do.
  const more =
    table.total && table.total > table.rows.length
      ? `<div class="df-more">${table.total.toLocaleString()} rows in all · showing ${table.rows.length}</div>`
      : "";
  return `<div class="out" style="padding:0"><table class="df"><thead><tr>${head}</tr></thead><tbody>${body}</tbody></table>${more}</div>`;
}

/** What the open notebook's code is in, to colour its cells by. */
function notebookLanguage() {
  return lastNotebook?.language || "python";
}

function cellHtml(cell, index) {
  const number = cell.running
    ? `<span class="spin"></span>`
    : `[${cell.executionCount ?? " "}]`;
  const outputs = cell.outputs.map(outputHtml).join("");
  const run =
    cell.kind === "code"
      ? `<button class="hbtn" data-run-cell="${index}" title="Run this cell"><svg class="i"><use href="#i-play"/></svg></button>`
      : "";

  return (
    `<div class="cell${cell.running ? " running" : ""}${cell.selected ? " on" : ""}" data-cell="${index}">` +
    `<header>${number} ${escapeHtml(cell.kind)}<span class="grow"></span>${run}` +
    `<button class="hbtn" data-drop-cell="${index}" title="Remove this cell"><svg class="i"><use href="#i-close"/></svg></button>` +
    `</header>` +
    // Prose is rendered; code is typed in. A markdown cell showing its own
    // `#` and `**` is a notebook pretending to be a text file — but it still
    // has to be editable, so double-clicking it puts the source back.
    (cell.kind === "markdown" && index !== markdownEditing
      ? `<div class="cell-md" data-md="${index}" title="Double-click to edit">${markdown(cell.source)}</div>`
      : cell.kind === "code"
        ? `<div class="cell-code"><pre class="cell-hl" aria-hidden="true">${highlightCode(cell.source, notebookLanguage())}</pre>` +
          `<pre class="cell-src" contenteditable="plaintext-only" spellcheck="false" data-src="${index}">${escapeHtml(cell.source)}</pre></div>`
        : `<pre class="cell-src" contenteditable="plaintext-only" spellcheck="false" data-src="${index}">${escapeHtml(cell.source)}</pre>`) +
    (outputs ? `<div class="out-wrap">${outputs}</div>` : "") +
    `</div>`
  );
}

/* Insert mode, in a notebook.
 *
 * `i` and `a` put you in the selected cell's source, and `Esc` leaves it —
 * which is what the rest of the editor does and what fingers expect. Until
 * this, a cell could only be entered by clicking it, which is the one gesture
 * a modal editor should not require.
 *
 * The core does the modal half already: the engine enters insert on `i`, `a`,
 * `o` and `O` whatever the mode, and Data's verb table has meanwhile added a
 * cell for the last two. All that is missing is the caret, which lives here —
 * the core names no widget and cannot focus one.
 */
function enterCell(object) {
  const box = focusedCells();
  const cell = box?.querySelector(".cell.on");
  if (!cell) return;

  // Prose has to become source before there is anything to type in.
  if (cell.querySelector("[data-md]")) {
    markdownEditing = Number(cell.dataset.cell);
    notebookPainted.clear();
    renderNotebook(lastNotebook);
  }
  const src = focusedCells()?.querySelector(".cell.on .cell-src");
  if (!src) return;
  src.focus();

  // `i` lands at the start and `a` after — the same distinction they make in
  // text, applied to a cell. `o` and `O` made an empty cell, so either end is
  // the same place.
  const range = document.createRange();
  range.selectNodeContents(src);
  range.collapse(object === "Here" || object === "LineStart");
  const selection = getSelection();
  selection.removeAllRanges();
  selection.addRange(range);
}

// Double-click rendered prose to get its source back. Single-click selects,
// which is what a click on any other cell does, so editing needs the stronger
// gesture — the same bargain every notebook makes.
document.addEventListener("dblclick", (e) => {
  const md = e.target.closest?.("[data-md]");
  if (!md) return;
  markdownEditing = Number(md.dataset.md);
  // The cache describes markup that is about to be replaced.
  notebookPainted.clear();
  dispatch({ type: "selectCell", index: markdownEditing });
});

/** A cell's source as typed. A browser ends a line typed into a cell with a
 *  `<br>` as readily as with a newline — Chrome always does — and keeps a
 *  last `<br>` only so an empty last line has somewhere for the caret, so
 *  `textContent` would run the lines together. */
function cellSource(el) {
  let text = "";
  const nodes = [...el.childNodes];
  nodes.forEach((node, i) => {
    if (node.nodeName === "BR") {
      if (i < nodes.length - 1) text += "\n";
    } else if (node.nodeName === "DIV") {
      // A line some engines wrap in a block of its own.
      text += (text && !text.endsWith("\n") ? "\n" : "") + cellSource(node);
    } else {
      text += node.textContent;
    }
  });
  return text;
}

// A code cell is coloured as it is typed. The colours are the source drawn
// again behind it, whose own text is clear; the source itself stays as the
// browser typed it, so it keeps its undo and nothing moves the caret.
document.addEventListener("input", (e) => {
  const src = e.target.closest?.("[data-src]");
  const behind = src?.previousElementSibling;
  if (behind?.classList.contains("cell-hl")) behind.innerHTML = highlightCode(cellSource(src), notebookLanguage());
});

// A cell's source goes back to the core when you leave it, not on every
// keystroke: the core answers an edit by re-sending the notebook, and
// re-rendering the box you are typing in would move the caret to the start
// of it on every character.
document.addEventListener(
  "blur",
  (e) => {
    const src = e.target.closest?.("[data-src]");
    if (!src) return;
    dispatch({
      type: "setCell",
      index: Number(src.dataset.src),
      source: cellSource(src),
    });
  },
  true,
);

// The keys a cell claims for itself.
//
// ⇧⏎ runs it, which is what it does in every notebook there has ever been;
// plain ⏎ is a newline, because a cell is code. Escape leaves — the global
// handler yields to editable elements now, so this is the only thing that can
// hand the keyboard back to the modal engine.
document.addEventListener("keydown", (e) => {
  if (e.defaultPrevented) return;
  const src = e.target.closest?.("[data-src]");
  if (!src) return;
  const index = Number(src.dataset.src);

  if (e.key === "Escape") {
    e.preventDefault();
    dispatch({ type: "setCell", index, source: cellSource(src) });
    src.blur();
    // The core still thinks the engine is in insert mode, and only a key
    // it sees will change that.
    sendKey(e);
    return;
  }
  if (e.key === "Enter" && e.shiftKey) {
    e.preventDefault();
    dispatch({ type: "setCell", index, source: cellSource(src) });
    dispatch({ type: "runCell", index });
    // Back to normal mode, so `j` moves to the next cell rather than typing
    // a `j` into the one that just ran.
    src.blur();
    sendKey(new KeyboardEvent("keydown", { key: "Escape" }));
  }
});

function renderNotebook(event) {
  lastNotebook = event;
  const name = $("[data-nb-name]");
  if (name) {
    name.textContent = event.path ? event.path.split(/[\\/]/).pop() : "no notebook";
    name.className = "chip" + (event.path ? " accent" : "");
  }
  const kernel = $("[data-nb-kernel]");
  if (kernel) {
    // Only when there is something wrong. A kernel that is working is not
    // news, and a chip that is always there is a chip nobody reads.
    kernel.hidden = !event.problem;
    kernel.className = "chip warn";
    kernel.textContent = event.problem;
  }

  const boxes = $$("[data-cells]");
  if (!boxes.length) return;
  for (const box of boxes) paintCells(box, event);
  renderTableDock(event);
}

/* ── Design mode's artboard ───────────────────────────────────────────

   What the theme actually looks like.

   The milestone's exit is that editing a token in the inspector changes
   `theme.css`. That is only half of what makes it usable: the other half is
   seeing the change land on something. So the artboard renders the *theme* —
   every colour token as a live swatch, and a specimen built from them.

   It is not a mockup of a component. Rendering somebody's `Dashboard.tsx`
   needs a bundler and a JSX runtime, and a hand-drawn approximation of one
   would be the same lie Data mode's sidebar used to tell: plausible, and
   about nothing. Everything drawn here is a value read out of the file.

   The palette is applied as inline custom properties on the artboard element
   and the specimen inside it uses `var()`, so the cascade resolves it exactly
   as a browser would. No colour is computed here.  */

/** Which scope the artboard is rendering, or null for the first one. */
let artboardScope = null;
/** Artboard width in px; 0 fills the pane. */
let artboardWidth = 0;
let artboardPainted = null;
/** The component the artboard is drawing, kept because the palette and the
    markup arrive in separate events and the frame needs both. */
let lastArtboard = null;
/** The frame currently on the artboard, so a posted message can be checked
    against its window rather than merely believed. */
let artboardFrame = null;

/* Clicking the picture selects the layer.
 *
 * Everything arriving here is untrusted. The frame has an opaque origin, so
 * `event.origin` is the string "null" and proves nothing; what does prove
 * something is `event.source`, which is a window reference the sender cannot
 * forge. Anything not from the frame currently on the artboard is dropped,
 * and the shape is checked before the path is used — the path itself is only
 * ever compared against paths the core computed, never executed or injected. */
window.addEventListener("message", (e) => {
  if (!artboardFrame || e.source !== artboardFrame.contentWindow) return;
  const said = e.data;
  if (!said || said.proton !== "pick" || typeof said.path !== "string") return;

  const index = lastArtboard?.layers?.findIndex((l) => l.path === said.path);
  // -1 is an element the parser never produced — a `tbody` the browser
  // inserted, most likely. Selecting nothing is right; selecting a neighbour
  // would outline the wrong thing and look deliberate.
  if (index === undefined || index < 0) return;
  dispatch({ type: "selectLayer", index });
});

/**
 * The tokens in force for a scope.
 *
 * A theme states `--accent` once per palette, and a palette is the base
 * declarations with that scope's on top — which is what the cascade does when
 * `[data-theme="x"]` matches. Resolving it the same way here is why a swatch
 * shows the colour the app would actually use.
 */
function resolveScope(tokens, scope) {
  const base = tokens.filter((t) => t.scope === (tokens[0]?.scope ?? ""));
  const over = scope ? tokens.filter((t) => t.scope === scope) : [];
  const merged = new Map();
  for (const token of [...base, ...over]) merged.set(token.name, token.value);
  return merged;
}

/** The scopes a theme declares, in the order it declares them. */
function scopesOf(tokens) {
  const seen = [];
  for (const token of tokens) {
    if (!seen.includes(token.scope)) seen.push(token.scope);
  }
  return seen;
}

/**
 * The palettes a theme declares, as Design's structure.
 *
 * One row per scope, with how many tokens it states and a strip of the
 * colours it overrides — which is what tells the difference between a palette
 * that changes everything and one that changes an accent.
 */
function renderPalettes(event) {
  const box = $("[data-palettes]");
  if (!box) return;
  const count = $("[data-pal-count]");

  const scopes = scopesOf(event.tokens);
  if (count) count.textContent = scopes.length ? `${scopes.length}` : "";

  if (!scopes.length) {
    box.innerHTML = `<div class="empty">${escapeHtml(
      event.problem || "No theme.",
    )}</div>`;
    return;
  }

  box.innerHTML = scopes
    .map((scope) => {
      const mine = event.tokens.filter((t) => t.scope === scope);
      const strip = mine
        .filter((t) => swatchable(t.value))
        .slice(0, 6)
        .map((t) => `<i style="background:${escapeHtml(t.value)}"></i>`)
        .join("");
      return (
        `<div class="row pal${scope === artboardScope ? " sel" : ""}"` +
        ` data-palette="${escapeHtml(scope)}" title="${escapeHtml(scope)}">` +
        `<span class="pal-name">${escapeHtml(scope || "(top level)")}</span>` +
        `<span class="pal-strip">${strip}</span>` +
        `<span class="chip">${mine.length}</span>` +
        `</div>`
      );
    })
    .join("");
}

/* ── The component on the artboard ───────────────────────────────

   ## Why this is an iframe and not a div

   The source is somebody's file. It arrives from disk, it may have come from
   a teammate or a template, and `tauri.conf.json` sets `withGlobalTauri`,
   which puts `window.__TAURI__` — the filesystem, the shell — in this
   document. Writing that markup into this document with `innerHTML` would
   hand anything in it those keys. That is not a hypothetical about malice; a
   component that pulls in an analytics snippet is enough.

   A frame with an opaque origin cannot reach any of it. It also stops
   Proton's own stylesheet leaking into the preview, which a div cannot: the
   whole point of the artboard is to show what the component looks like with
   *its* theme, not with the editor's.

   ## Scripts do not run, deliberately

   `sandbox=""` — no flags at all — is the strictest setting: no scripts, no
   forms, no navigation, opaque origin. A preview of layout and palette does
   not need behaviour, and starting closed and opening deliberately is the
   right order for a thing that renders other people's files. The CSP inside
   the document is the second lock: no network, so a preview cannot phone
   home or leak the fact that a file was opened.

   Both are worth revisiting when the artboard grows interaction. Neither is
   worth loosening quietly. */

/** A fresh nonce for one render. Random per render so a component cannot
 *  carry a `<script nonce=…>` that guesses it. */
function artboardNonce() {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** The document to put in the frame: the component, wrapped in its palette.
 *
 * `pick` is the selected layer's CSS path, or empty. It is applied as a rule
 * rather than by marking up the element, because the frame must be given the
 * author's markup unaltered — the moment this starts editing the source on
 * its way to the screen, the artboard stops showing the file. */
function artboardDoc(source, isDocument, vars, pick, nonce) {
  // ## What this CSP is doing, and why it is a nonce
  //
  // The frame needs one script — the one below, which reports what was
  // clicked. Allowing `'unsafe-inline'` would run that *and* every script the
  // component happens to contain, and those could post the same messages this
  // one does, so the panel could be told a different element was clicked.
  //
  // A nonce, fresh for each render, admits exactly the script carrying it.
  // The component's own scripts have no nonce and do not run. `default-src
  // 'none'` still means no network of any kind, so nothing in the frame can
  // fetch, beacon, or load a remote font — a preview cannot phone home or
  // report that a file was opened.
  const csp =
    `<meta http-equiv="Content-Security-Policy" content="default-src 'none'; ` +
    `style-src 'unsafe-inline'; img-src data:; font-src data:; ` +
    `script-src 'nonce-${nonce}'">`;
  const style =
    `<style>:root{${vars}}` +
    `html,body{margin:0}` +
    `body{background:var(--bg,#16161e);color:var(--ink,#c0caf5);` +
    `font:14px/1.5 system-ui,sans-serif;padding:18px}` +
    // `outline` rather than `border`, because a border would move the element
    // it is drawn on and the artboard would stop showing the real layout.
    (pick
      ? `${pick}{outline:2px solid var(--accent,#7aa2f7);outline-offset:2px}`
      : "") +
    `</style>`;

  // Reports what was clicked, as the same `nth-child` path proton-design
  // builds from the source, so the two can be compared without either side
  // trusting the other's numbering.
  //
  // It walks to `body` for a fragment and to the root for a whole page,
  // matching where the parser starts counting. Where a browser inserts
  // elements the source never had — `tbody` inside a table is the usual one —
  // the two disagree and the click selects nothing rather than the wrong
  // thing. Saying so plainly beats a highlight that is quietly off by one.
  const picker =
    `<script nonce="${nonce}">document.addEventListener("click",function(e){` +
    `var el=e.target,parts=[];` +
    `while(el&&el.nodeType===1){` +
    `if(el===document.body&&${!isDocument})break;` +
    `var p=el.parentNode,n=1;` +
    // Among *all* element siblings, not among same-tag ones: the parser
    // numbers children the same way, and `nth-of-type` here against
    // `nth-child` there disagrees the moment an element has a sibling of
    // another tag — which is nearly always.
    `if(p){var k=p.children;for(var i=0;i<k.length;i++){` +
    `if(k[i]===el)break;n++;}}` +
    `parts.unshift(el.tagName.toLowerCase()+":nth-child("+n+")");` +
    `el=el.parentElement;}` +
    `parent.postMessage({proton:"pick",path:parts.join(" > ")},"*");` +
    `e.preventDefault();},true);<\/script>`;

  if (!isDocument) {
    return `<!doctype html><html><head><meta charset="utf-8">${csp}${style}</head>` +
      `<body>${source}${picker}</body></html>`;
  }
  // A whole page brings its own head, so the palette goes in at the end of it
  // and wins over what the page set. A page with no head at all is rendered as
  // written rather than guessed at.
  const at = source.search(/<\/head>/i);
  return at === -1
    ? source + style + picker
    : source.slice(0, at) + csp + style + source.slice(at) + picker;
}

/* ── Video's timeline ─────────────────────────────────

   Every clip on the timeline is an event in the project's `.edl`, placed by
   its record timecode. The inspector edits that file by splicing it, and the
   captions come from the `.srt` beside it.

   There is no program monitor: a frame of the programme is a decoded frame of
   video, and nothing here can decode one. */

let lastCut = null;

/** `HH:MM:SS:FF` from a frame count, which is what the ruler counts in. */
function asTimecode(frames, rate) {
  const fps = Math.max(1, rate?.fps ?? 24);
  // Drop-frame puts back the numbers it skipped — two a minute, given back
  // every tenth minute — before the count is split into fields.
  let n = frames;
  if (rate?.drop) {
    const d = Math.floor(fps / 15);
    const perTen = fps * 600 - 9 * d;
    const perOne = fps * 60 - d;
    const tens = Math.floor(frames / perTen);
    const rest = frames % perTen;
    n += 9 * d * tens;
    if (rest >= d) n += d * Math.floor((rest - d) / perOne);
  }
  const pad = (v) => String(v).padStart(2, "0");
  return (
    `${pad(Math.floor(n / (fps * 3600)))}:` +
    `${pad(Math.floor(n / (fps * 60)) % 60)}:` +
    `${pad(Math.floor(n / fps) % 60)}:${pad(n % fps)}`
  );
}

/** The sources the cut refers to, and whether they are on disk. */
function renderMediaBin(event) {
  const box = $("[data-media]");
  if (!box) return;
  const count = $("[data-media-count]");
  const media = event.media ?? [];
  if (count) count.textContent = media.length ? `${media.length}` : "";

  if (!media.length) {
    box.innerHTML = `<div class="empty">${escapeHtml(
      event.problem || "No edit decision list in this project.",
    )}${openFileAction("Open edit decision list", ".edl")}</div>`;
    return;
  }
  box.innerHTML = media
    .map((item) => {
      // Missing is the useful state to shout about: a cut handed over on its
      // own almost always arrives with its media somewhere else.
      const where = item.found
        ? `<span class="chip ok" title="${escapeHtml(item.path)}">on disk</span>`
        : `<span class="chip warn">missing</span>`;
      const icon = item.audio ? "i-audio" : "i-play";
      const tint = item.audio ? " ft-audio" : "";
      return (
        `<div class="row media" title="${escapeHtml(item.reel)} · ${item.uses} event${
          item.uses === 1 ? "" : "s"
        }">` +
        `<span class="ic"><svg class="i${tint}"><use href="#${icon}"/></svg></span>` +
        `${escapeHtml(item.name)}<span class="grow"></span>${where}</div>`
      );
    })
    .join("");
}

/** The timeline: lanes of clips, positioned by record timecode. */
function renderTimeline(event) {
  const box = $("[data-timeline]");
  if (!box) return;
  const clips = event.clips ?? [];
  const captions = event.captions ?? [];
  const length = Math.max(1, Number(event.length ?? 0));
  const rate = event.rate;

  if (!clips.length) {
    box.innerHTML = "";
    return;
  }

  // Six marks, on whole seconds, because a ruler labelled in frames is a ruler
  // nobody reads.
  const fps = Math.max(1, rate?.fps ?? 24);
  const seconds = Math.max(1, Math.round(length / fps / 6));
  const marks = [];
  for (let at = 0; at < length; at += seconds * fps) {
    marks.push(
      `<span style="left:${(at / length) * 100}%">${escapeHtml(asTimecode(at, rate))}</span>`,
    );
  }

  const lane = (name, icon, tint, inner) =>
    `<div class="track"><div class="name"><svg class="i${tint}"><use href="#${icon}"/></svg> ${escapeHtml(
      name,
    )}</div><div class="lane">${inner}</div></div>`;

  const tracks = (event.tracks ?? []).map((name) => {
    const kind = name.startsWith("A") ? "a" : "v";
    const inner = clips
      .map((clip, i) => [clip, i])
      .filter(([clip]) => (clip.tracks ?? []).includes(name))
      .map(([clip, i]) => {
        // An out point is the first frame *not* included, so a clip's width is
        // the difference and two touching clips share an edge exactly.
        const left = (clip.start / length) * 100;
        const width = Math.max(0.4, ((clip.end - clip.start) / length) * 100);
        const label = clip.name || clip.reel || clip.event;
        return (
          `<div class="clip ${kind}${i === event.selected ? " sel" : ""}" data-clip="${i}"` +
          ` style="left:${left}%;width:${width}%"` +
          ` title="${escapeHtml(clip.event)} · ${escapeHtml(clip.transition)}${
            clip.duration ? " " + escapeHtml(clip.duration) : ""
          } · ${escapeHtml(clip.recordIn)} → ${escapeHtml(clip.recordOut)}"` +
          `>${escapeHtml(label)}</div>`
        );
      })
      .join("");
    return lane(name, kind === "a" ? "i-audio" : "i-play", kind === "a" ? " ft-audio" : "", inner);
  });

  // The captions are a second file, so they get a lane of their own rather
  // than being drawn as if they were events in the cut.
  if (captions.length) {
    const inner = captions
      .map((cue) => {
        const from = ((cue.startMs / 1000) * fps) / length;
        const to = ((cue.endMs / 1000) * fps) / length;
        return (
          `<div class="clip t" style="left:${from * 100}%;width:${Math.max(
            0.4,
            (to - from) * 100,
          )}%" title="${escapeHtml(cue.start)} → ${escapeHtml(cue.end)}">${escapeHtml(
            cue.text.replace(/\n/g, " "),
          )}</div>`
        );
      })
      .join("");
    tracks.push(lane("Captions", "i-file-doc", " ft-doc", inner));
  }

  box.innerHTML = `<div class="tl-ruler">${marks.join("")}</div>${tracks.join("")}`;
}

/** The list's own text, which is what Video mode can honestly show. */
function renderCutSource(event) {
  const box = $("[data-cut-source]");
  if (!box) return;
  if (!event.path) {
    box.innerHTML = `<div class="empty">${escapeHtml(
      event.problem || "No edit decision list in this project.",
    )}${openFileAction("Open edit decision list", ".edl")}</div>`;
    return;
  }
  box.innerHTML =
    `<div class="level-note">The cut itself. A program monitor, waveforms and ` +
    `any kind of render need ffmpeg in a process of its own; there is not one.</div>` +
    `<pre class="level-src">${escapeHtml(event.source ?? "")}</pre>`;
}

/** The selected event's fields, exactly as the list wrote them. */
function renderCutInspector(event) {
  const box = $("[data-cut-props]");
  const nothing = $("[data-cut-empty]");
  if (!box || !nothing) return;

  const clip = event.clips?.[event.selected];
  const picked = $("[data-cut-selected]");
  if (picked) picked.textContent = clip ? `${clip.event} · ${clip.channel}` : "";

  box.hidden = !clip;
  nothing.hidden = !!clip;
  if (!clip) return;

  // Read-only above, editable below. A transition and its length change what
  // an event *is*, not merely when it happens, and neither is a text box.
  const said = (label, value) =>
    `<div class="prop-row"><span class="lbl">${escapeHtml(label)}</span>` +
    `<span class="val">${escapeHtml(value)}</span></div>`;
  const field = (label, name, value) =>
    `<div class="prop-row"><span class="lbl">${escapeHtml(label)}</span>` +
    // Left-aligned: `.num` is right-aligned for the numbers Modeling's
    // inspector holds, and a clip name reading toward the right edge is a
    // field that looks broken.
    `<input class="num" data-clip-field="${name}" spellcheck="false" style="text-align:left"` +
    ` value="${escapeHtml(value)}"></div>`;

  box.innerHTML =
    said("channel", clip.channel) +
    said(
      "transition",
      clip.duration ? `${clip.transition} ${clip.duration}` : clip.transition,
    ) +
    field("clip", "name", clip.name) +
    field("reel", "reel", clip.reel) +
    field("source in", "sourceIn", clip.sourceIn) +
    field("source out", "sourceOut", clip.sourceOut) +
    field("record in", "recordIn", clip.recordIn) +
    field("record out", "recordOut", clip.recordOut);
}

/** The captions, editable, from the file beside the cut. */
function renderCaptions(event) {
  const box = $("[data-captions]");
  const head = $("[data-captions-head]");
  if (!box || !head) return;
  const captions = event.captions ?? [];
  head.hidden = !captions.length;
  if (!captions.length) {
    box.innerHTML = "";
    return;
  }
  const file = $("[data-captions-file]");
  if (file) file.textContent = (event.captionsPath || "").split(/[\\/]/).pop();

  box.innerHTML = captions
    .map(
      (cue, i) =>
        `<div class="cue"><div class="cue-at">${escapeHtml(cue.start)} → ${escapeHtml(
          cue.end,
        )}</div>` +
        `<textarea class="cue-text" data-caption="${i}" spellcheck="false" rows="2">${escapeHtml(
          cue.text,
        )}</textarea></div>`,
    )
    .join("");
}

/** The bar above the timeline, including the rate that is an assumption. */
function renderCutBar(event) {
  const file = $("[data-cut-file]");
  if (file) {
    file.textContent = event.path ? event.path.split(/[\\/]/).pop() : "no cut";
    file.className = "chip" + (event.path ? " accent" : "");
  }
  const title = $("[data-cut-title]");
  if (title) title.textContent = event.title ?? "";
  const length = $("[data-cut-length]");
  if (length) {
    length.textContent = event.path
      ? `${asTimecode(Number(event.length ?? 0), event.rate)}`
      : "";
  }

  const rate = $("[data-cut-rate]");
  if (!rate) return;
  const fps = event.rate?.fps ?? 24;
  const RATES = [24, 25, 30, 48, 50, 60];
  // The list's own rate is offered even when it is not one of the usual ones,
  // so that changing something else does not silently change this too.
  const offer = RATES.includes(fps) ? RATES : [...RATES, fps].sort((a, b) => a - b);
  rate.innerHTML = offer
    .map(
      (one) =>
        `<option value="${one}"${one === fps ? " selected" : ""}>${one} fps${
          one === fps && event.rate?.drop ? " · drop" : ""
        }</option>`,
    )
    .join("");
  rate.disabled = !event.path;
}

document.addEventListener("click", (e) => {
  const clip = e.target.closest?.("[data-clip]");
  if (!clip) return;
  dispatch({ type: "selectClip", index: Number(clip.dataset.clip) });
});

document.addEventListener("change", (e) => {
  const field = e.target.closest?.("[data-clip-field]");
  if (field) {
    const index = lastCut?.selected;
    if (index === undefined || index < 0) return;
    dispatch({
      type: "setClipField",
      index,
      field: field.dataset.clipField,
      value: field.value,
    });
    return;
  }

  const cue = e.target.closest?.("[data-caption]");
  if (cue) {
    dispatch({
      type: "setCaptionField",
      index: Number(cue.dataset.caption),
      field: "text",
      value: cue.value,
    });
    return;
  }

  const rate = e.target.closest?.("[data-cut-rate]");
  if (rate) dispatch({ type: "setTimelineRate", fps: Number(rate.value) });
});

/* ── Game's hierarchy ────────────────────────────────

   Every row is a node really in the project's `.tscn`. The inspector edits
   Godot's file by splicing it; the pane in the middle shows the file's text,
   which is the honest thing to show until there is an engine on the other end
   of a pipe to actually run the scene. */

let lastLevel = null;

function renderHierarchy(event) {
  const box = $("[data-hierarchy]");
  if (!box) return;
  const count = $("[data-level-count]");
  const nodes = event.nodes ?? [];
  if (count) count.textContent = nodes.length ? `${nodes.length}` : "";

  const file = $("[data-level-file]");
  if (file) {
    file.textContent = event.path ? event.path.split(/[\\/]/).pop() : "no scene";
    file.className = "chip" + (event.path ? " accent" : "");
  }

  if (!nodes.length) {
    box.innerHTML = `<div class="empty">${escapeHtml(
      event.problem || "No Godot scene in this project.",
    )}${openFileAction("Open scene", ".tscn")}</div>`;
    return;
  }

  box.innerHTML = nodes
    .map((node, i) => {
      const kids = node.children
        ? `<span class="node-kids">${node.children}</span>`
        : "";
      // The Godot class beside the name, quiet, because in a Godot hierarchy
      // that is how you tell a Sprite2D from an AnimatedSprite2D when both
      // are called "Sprite".
      return (
        `<div class="row scene-node${i === event.selected ? " sel" : ""}" data-level-node="${i}"` +
        ` style="padding-left:${8 + node.depth * 12}px"` +
        ` title="${escapeHtml(node.kind || "instance")} · line ${node.line}">` +
        `${escapeHtml(node.name)}<span class="lay-text">${escapeHtml(node.kind)}</span>` +
        `<span class="grow"></span>${kids}</div>`
      );
    })
    .join("");
}

/** The selected node's properties, exactly as Godot wrote their values. */
function renderLevelInspector(event) {
  const box = $("[data-level-props]");
  const nothing = $("[data-level-empty]");
  if (!box || !nothing) return;

  const node = event.nodes?.[event.selected];
  const picked = $("[data-level-selected]");
  if (picked) picked.textContent = node ? `${node.kind} · ${node.name}` : "";
  const kind = $("[data-level-kind]");
  if (kind) kind.textContent = node?.kind ?? "";

  box.hidden = !node;
  nothing.hidden = !!node;
  if (!node) return;

  if (!node.properties.length) {
    box.innerHTML = `<div class="empty">This node sets no properties.</div>`;
    return;
  }
  // One text box per value, because a `.tscn` value is an expression —
  // `Vector2(196, 286)`, `ExtResource("1_x8k")` — and a widget per type would
  // have to understand a language this deliberately does not parse.
  box.innerHTML = node.properties
    .map(
      (p) =>
        `<div class="prop-row"><span class="lbl" title="${escapeHtml(p.name)}">${escapeHtml(p.name)}</span>` +
        `<input class="num" data-level-prop="${escapeHtml(p.name)}" spellcheck="false"` +
        ` style="text-align:left" value="${escapeHtml(p.value)}"></div>`,
    )
    .join("");
}

/** The scene file itself, which is what Game mode can honestly show. */
function renderLevelSource(event) {
  const box = $("[data-level-source]");
  if (!box) return;
  if (!event.path) {
    box.innerHTML = `<div class="empty">${escapeHtml(
      event.problem || "No Godot scene in this project.",
    )}${openFileAction("Open scene", ".tscn")}</div>`;
    return;
  }
  box.innerHTML =
    `<div class="level-note">Godot's file. Running the scene, hot reload and the ` +
    `profiler need an engine process; there is not one.</div>` +
    `<pre class="level-src">${escapeHtml(event.source ?? "")}</pre>`;
}

document.addEventListener("click", (e) => {
  const row = e.target.closest?.("[data-level-node]");
  if (!row) return;
  dispatch({ type: "selectGameNode", index: Number(row.dataset.levelNode) });
});

document.addEventListener("change", (e) => {
  const field = e.target.closest?.("[data-level-prop]");
  if (!field) return;
  const index = lastLevel?.selected;
  if (index === undefined || index < 0) return;
  dispatch({
    type: "setGameProperty",
    index,
    name: field.dataset.levelProp,
    value: field.value,
  });
});

/* ── Modeling's outliner ────────────────────────────────

   Every row is an object really in `scene.proton.json`. Clicking one selects
   it; the inspector edits it; editing writes the file. Nothing here is
   invented, which is the difference between this and what it replaced. */

/** Which glyph a kind gets. Unknown kinds get the generic one rather than
    nothing, because a scene from a later version may hold a kind this build
    has never heard of and an empty column reads as a broken row. */
const NODE_ICON = {
  mesh: "i-mesh",
  light: "i-light",
  camera: "i-camera",
  empty: "i-sphere",
};

let lastScene = null;

/** Whether the viewport has been pointed at a scene yet, so the camera is
    framed once rather than on every event — a repaint that moved the camera
    would fight whoever is dragging it. */
let framed = false;

function renderViewport(event) {
  // The materials, already compiled by the core. Given to the viewport before
  // the geometry is asked for, so the first frame of a mesh is shaded rather
  // than grey for a moment.
  setShaders(event.shaders ?? []);
  // Ask for the geometry this scene names and the viewport does not already
  // hold. Asked for rather than pushed: a mesh is the one thing on this seam
  // measured in megabytes, and the front end is the end that knows what it has.
  for (const path of event.meshes ?? []) {
    if (!hasGeometry(path)) dispatch({ type: "loadMesh", path });
  }

  // Framed first, then drawn — `frameAll` only moves the camera, and returning
  // here left the renderer with the empty node list it started with, so the
  // grid appeared and the objects did not.
  if (!framed && event.nodes?.length) {
    framed = true;
    frameAll(event.nodes);
  }
  drawViewport(event.nodes ?? [], event.selected);
}

/** What the picture is made of, and what is drawing it.
 *
 * The triangle count is what the scene *has*, not what survived being drawn.
 * It used to be the latter, because a painter's sort knew exactly how many
 * polygons it filled; a depth buffer decides how much of a triangle you can
 * see per pixel, and there is no honest count of "drawn" to report any more.
 *
 * The renderer's name is there because it is the one thing that would explain
 * a slow viewport and cannot be guessed from inside: the same code on a real
 * GPU and on a software fallback are two different programs. */
function showViewportCount(tris, objects) {
  const badge = $("[data-vp-count]");
  if (!badge) return;
  const stopped = renderProblem();
  if (stopped) {
    badge.textContent = stopped;
    badge.className = "vp-badge warn";
    return;
  }
  badge.className = "vp-badge";
  const said = [
    objects ? `${objects} object${objects === 1 ? "" : "s"}` : "",
    tris ? `${tris.toLocaleString()} triangle${tris === 1 ? "" : "s"}` : "",
    renderer(),
  ].filter(Boolean);
  badge.textContent = said.join(" · ");
}

/* ── The shader graph ──────────────────────────────────

   The material of whichever object is selected. Every box is a node really in
   `scene.proton.json` and every wire a link in it; editing a value writes the
   file, dragging a box writes where it moved to.

   Boxes are DOM and wires are one SVG behind them, which is what the mock
   this replaces did and is right for a handful of nodes: the boxes get text
   selection, focus and inputs for free, and the wires need curves that DOM
   cannot draw. */

/** Which ports a kind has, so a box can show its sockets. Unknown kinds get
    whatever the links mention, because a graph from a later version has to
    keep working rather than lose its wires. */
const GRAPH_PORTS = {
  noise: { in: ["scale", "detail"], out: ["fac", "color"] },
  ramp: { in: ["fac"], out: ["color"] },
  principled: { in: ["baseColor", "metallic", "roughness"], out: ["bsdf"] },
  output: { in: ["surface", "displacement"], out: [] },
};

/** A colour param as CSS, for the swatch. Linear 0..1 to sRGB 0..255 is a
    gamma curve, not a multiply — 0.5 linear is mid-*light*, not mid-grey, and
    showing it as #808080 would misreport every colour in the file. */
function paramColor([r, g, b]) {
  const to8 = (v) => {
    const c = Math.max(0, Math.min(1, v));
    const s = c <= 0.0031308 ? c * 12.92 : 1.055 * Math.pow(c, 1 / 2.4) - 0.055;
    return Math.round(s * 255);
  };
  return `rgb(${to8(r)}, ${to8(g)}, ${to8(b)})`;
}

/** The material the selected object wears, or null. */
function materialFor(event) {
  const node = event.nodes?.[event.selected];
  if (!node?.material) return null;
  return event.materials?.find((m) => m.id === node.material) ?? null;
}

function renderGraph(event) {
  const box = $("[data-graph]");
  const wires = $("[data-graph-wires]");
  const name = $("[data-graph-name]");
  if (!box || !wires) return;

  const material = materialFor(event);
  if (name) {
    name.textContent = material
      ? material.name
      : event.nodes?.[event.selected]
        ? "No material on this object"
        : "No material";
  }
  if (!material) {
    box.innerHTML = "";
    wires.innerHTML = "";
    return;
  }

  const ports = (node) => {
    const known = GRAPH_PORTS[node.kind];
    if (known) return known;
    // Whatever the wires mention, so an unfamiliar node still shows sockets.
    const mine = (end) =>
      material.links
        .map((l) => l[end])
        .filter((p) => p.startsWith(`${node.id}:`))
        .map((p) => p.slice(node.id.length + 1));
    return { in: mine("to"), out: mine("from") };
  };

  box.innerHTML = material.nodes
    .map((node) => {
      const { in: ins, out: outs } = ports(node);
      const rows = node.params
        .map((p) => {
          const value =
            "color" in p.value
              ? `<i class="sw" style="background:${paramColor(p.value.color)}"></i>` +
                `<span class="rt">${p.value.color.map((n) => n.toFixed(2)).join(" ")}</span>`
              : `<input class="rt num" data-gp="${escapeHtml(node.id)}|${escapeHtml(p.name)}"` +
                ` value="${escapeHtml(String(p.value.number))}" inputmode="decimal">`;
          return `<div class="port"><i></i>${escapeHtml(p.name)}${value}</div>`;
        })
        .join("");
      const sockets = (list, side) =>
        list
          .map(
            (port) =>
              `<span class="sock ${side}" data-sock="${escapeHtml(node.id)}:${escapeHtml(port)}"` +
              ` title="${escapeHtml(port)}"></span>`,
          )
          .join("");
      return (
        `<div class="node" data-gn="${escapeHtml(node.id)}"` +
        ` style="left:${node.at[0]}px;top:${node.at[1]}px">` +
        `<div class="nh">${escapeHtml(node.kind)}</div>` +
        `<div class="nb">${rows}</div>` +
        `<div class="socks in">${sockets(ins, "in")}</div>` +
        `<div class="socks out">${sockets(outs, "out")}</div>` +
        `</div>`
      );
    })
    .join("");

  drawWires(material, box, wires);
}

/** The wires, drawn from where the sockets actually ended up.
 *
 * Measured from the DOM rather than computed from the node positions, so a
 * box that grew a row — an extra param, a longer name — keeps its wires
 * attached instead of having them land where the socket used to be. */
function drawWires(material, box, wires) {
  const frame = box.getBoundingClientRect();
  const at = (port) => {
    const el = box.querySelector(`[data-sock="${CSS.escape(port)}"]`);
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return [r.left + r.width / 2 - frame.left, r.top + r.height / 2 - frame.top];
  };

  wires.innerHTML = material.links
    .map((link) => {
      const a = at(link.from);
      const b = at(link.to);
      // A wire to a socket the node does not have is a fact about the file,
      // not something to draw a guess for.
      if (!a || !b) return "";
      const bend = Math.max(30, Math.abs(b[0] - a[0]) * 0.5);
      return (
        `<path d="M${a[0]} ${a[1]} C ${a[0] + bend} ${a[1]}, ${b[0] - bend} ${b[1]}, ${b[0]} ${b[1]}"` +
        ` stroke="var(--accent-2)" stroke-width="1.6" fill="none"/>`
      );
    })
    .join("");
}

document.addEventListener("change", (e) => {
  const field = e.target.closest?.("[data-gp]");
  if (!field) return;
  const material = materialFor(lastScene ?? {});
  if (!material) return;
  const [node, param] = field.dataset.gp.split("|");
  const number = Number(field.value);
  if (!Number.isFinite(number)) {
    renderGraph(lastScene);
    renderStatus(`${param} needs a number`, "warn");
    return;
  }
  dispatch({
    type: "setMaterialParam",
    material: material.id,
    node,
    param,
    value: { number },
  });
});

/* Dragging a wire between sockets.
 *
 * A wire runs from an output to an input, and the gesture works from either
 * end because that is what every node editor does and nobody remembers which
 * way round they are supposed to start.
 *
 * Dragging *out* of an input that already has a wire picks that wire up rather
 * than starting a second one: an input takes one wire, so beginning a new one
 * there would only ever end in replacing what is already attached, and picking
 * it up lets the same gesture move it somewhere else or drop it in empty space
 * to remove it. */

/** The socket nearest a point, within reach, or null.
 *
 * By nearest-within-radius rather than by what is exactly under the pointer: a
 * socket is nine pixels across and a wire is a gesture people make quickly.
 * `elementFromPoint` alone would miss by two pixels and do nothing, which
 * reads as the feature being broken rather than as a near miss. */
function socketNear(x, y, box) {
  let best = null;
  let bestD = 18;
  for (const el of $$("[data-sock]", box)) {
    const r = el.getBoundingClientRect();
    const d = Math.hypot(r.left + r.width / 2 - x, r.top + r.height / 2 - y);
    if (d < bestD) {
      bestD = d;
      best = el;
    }
  }
  return best;
}

(() => {
  /** The drag in progress: which output it comes from, and — when a wire was
      picked up — which input it used to feed. */
  let wiring = null;

  const box = () => $("[data-graph]");
  const wires = () => $("[data-graph-wires]");

  document.addEventListener("pointerdown", (e) => {
    const sock = e.target.closest?.("[data-sock]");
    if (!sock) return;
    const material = materialFor(lastScene ?? {});
    if (!material) return;
    e.preventDefault();
    e.stopPropagation();

    const port = sock.dataset.sock;
    if (sock.classList.contains("out")) {
      wiring = { material, from: port, was: null };
      return;
    }
    // An input. If something feeds it, pick that wire up; otherwise there is
    // nothing here to drag.
    const existing = material.links.find((l) => l.to === port);
    if (!existing) return;
    wiring = { material, from: existing.from, was: port };
  });

  document.addEventListener("pointermove", (e) => {
    if (!wiring) return;
    const frame = box().getBoundingClientRect();
    const start = $(`[data-sock="${CSS.escape(wiring.from)}"]`, box());
    if (!start) return;
    const r = start.getBoundingClientRect();
    const a = [r.left + r.width / 2 - frame.left, r.top + r.height / 2 - frame.top];
    const b = [e.clientX - frame.left, e.clientY - frame.top];

    // The socket the drop would land on, marked so the target is obvious
    // before the button comes up.
    const over = socketNear(e.clientX, e.clientY, box());
    for (const el of $$("[data-sock]", box())) el.classList.toggle("hot", el === over);

    const bend = Math.max(30, Math.abs(b[0] - a[0]) * 0.5);
    // Drawn on top of the settled wires, dashed, so it reads as a thing in
    // flight rather than as a wire that already exists.
    wires().insertAdjacentHTML(
      "beforeend",
      `<path data-ghost d="M${a[0]} ${a[1]} C ${a[0] + bend} ${a[1]}, ${b[0] - bend} ${b[1]}, ${b[0]} ${b[1]}"` +
        ` stroke="var(--accent)" stroke-width="1.6" stroke-dasharray="4 3" fill="none"/>`,
    );
    // One ghost, not one per move.
    for (const old of $$("[data-ghost]", wires()).slice(0, -1)) old.remove();
  });

  document.addEventListener("pointerup", (e) => {
    if (!wiring) return;
    const { material, from, was } = wiring;
    wiring = null;
    for (const el of $$("[data-sock]", box())) el.classList.remove("hot");
    for (const ghost of $$("[data-ghost]", wires())) ghost.remove();

    const over = socketNear(e.clientX, e.clientY, box());
    // Dropped on nothing. A wire that was picked up is let go — that is how
    // one is removed — and a wire that was only ever being drawn is abandoned.
    if (!over || !over.classList.contains("in")) {
      if (was) dispatch({ type: "disconnectPort", material: material.id, to: was });
      return;
    }
    const to = over.dataset.sock;
    if (to === was) {
      // Put back exactly where it came from: nothing happened.
      return;
    }
    // A wire that was picked up *moves*. Sending a connect on its own would
    // leave the old one attached and make one gesture into two wires — which
    // is what this did until somebody dragged one and counted.
    if (was) {
      dispatch({ type: "rewirePort", material: material.id, fromInput: was, toInput: to });
      return;
    }
    dispatch({ type: "connectPorts", material: material.id, from, to });
  });
})();

/* Dragging a box. The position is written to the file when the drag ends, not
   while it moves — a write per pointermove would be a hundred saves for one
   nudge, and a hundred entries in the undo history. */
(() => {
  let dragging = null;
  document.addEventListener("pointerdown", (e) => {
    const head = e.target.closest?.(".node > .nh");
    if (!head) return;
    const node = head.parentElement;
    dragging = {
      id: node.dataset.gn,
      el: node,
      x: e.clientX,
      y: e.clientY,
      at: [parseFloat(node.style.left) || 0, parseFloat(node.style.top) || 0],
    };
    e.preventDefault();
  });

  document.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    dragging.at = [
      dragging.at[0] + (e.clientX - dragging.x),
      dragging.at[1] + (e.clientY - dragging.y),
    ];
    dragging.x = e.clientX;
    dragging.y = e.clientY;
    dragging.el.style.left = `${dragging.at[0]}px`;
    dragging.el.style.top = `${dragging.at[1]}px`;
    const material = materialFor(lastScene ?? {});
    if (material) drawWires(material, $("[data-graph]"), $("[data-graph-wires]"));
  });

  document.addEventListener("pointerup", () => {
    if (!dragging) return;
    const material = materialFor(lastScene ?? {});
    const { id, at } = dragging;
    dragging = null;
    if (material) {
      dispatch({
        type: "moveGraphNode",
        material: material.id,
        node: id,
        at: [Math.round(at[0]), Math.round(at[1])],
      });
    }
  });
})();

function renderOutliner(event) {
  const box = $("[data-outliner]");
  if (!box) return;
  const count = $("[data-scene-count]");
  const nodes = event.nodes ?? [];
  if (count) count.textContent = nodes.length ? `${nodes.length}` : "";

  if (!nodes.length) {
    box.innerHTML = `<div class="empty">${escapeHtml(
      event.problem || "No scene in this project.",
    )}${openFileAction("Open scene", ".proton.json")}</div>`;
    return;
  }

  box.innerHTML = nodes
    .map((node, i) => {
      const icon = NODE_ICON[node.kind] ?? "i-sphere";
      // The count is worth showing because a collapsed branch is the next
      // thing this panel will want, and because "3" beside a name answers
      // "did my delete take the children with it".
      const kids = node.children
        ? `<span class="node-kids">${node.children}</span>`
        : "";
      return (
        `<div class="row scene-node${i === event.selected ? " sel" : ""}" data-node="${i}"` +
        ` style="padding-left:${8 + node.depth * 12}px"` +
        ` title="${escapeHtml(node.kind)} · ${escapeHtml(node.id)}">` +
        `<span class="ic"><svg class="i"><use href="#${icon}"/></svg></span>` +
        `${escapeHtml(node.name)}<span class="grow"></span>${kids}</div>`
      );
    })
    .join("");
}

/** The inspector for the selected node. */
function renderNodeInspector(event) {
  // The bar over the viewport: which file, and what is selected. Both are
  // facts from this event rather than the invented statistics that were here.
  const file = $("[data-scene-file]");
  if (file) {
    file.textContent = event.path ? event.path.split(/[\/]/).pop() : "no scene";
    file.className = "chip" + (event.path ? " accent" : "");
  }

  const form = $("[data-node-form]");
  const nothing = $("[data-node-empty]");
  const chosen = event.nodes?.[event.selected];
  const picked = $("[data-scene-selected]");
  if (picked) picked.textContent = chosen ? `${chosen.kind} · ${chosen.name}` : "";
  const kind = $("[data-node-kind]");
  if (kind) kind.textContent = chosen?.kind ?? "";
  if (!form || !nothing) return;

  const node = chosen;
  form.hidden = !node;
  nothing.hidden = !!node;
  if (!node) return;

  // Not while it is being typed in: replacing the value under the caret moves
  // it to the end, on every event the core sends.
  const put = (box, value) => {
    if (box && document.activeElement !== box) box.value = value;
  };
  put($("[data-node-name]"), node.name);
  for (const box of $$("[data-node-xyz]")) {
    put(box, String(node[box.dataset.nodeXyz][Number(box.dataset.axis)]));
  }
  // Only a mesh has geometry. Showing the field on a light would invite
  // somebody to fill it in and wonder why nothing happened.
  const meshRow = $("[data-mesh-row]");
  if (meshRow) meshRow.hidden = node.kind !== "mesh";
  if (node.kind === "mesh") put($("[data-node-mesh]"), node.mesh ?? "");

  const meshNote = $("[data-mesh-note]");
  if (meshNote) {
    const said = node.kind === "mesh" ? meshProblem(node.geometry || node.mesh) : "";
    meshNote.textContent = said;
    meshNote.className = "st-hint" + (said ? " warn" : "");
  }

  $("[data-node-note]").textContent = `${node.kind} · ${node.id}`;
  renderStack(event);
}

/** The modifier stack of whichever object is selected.
 *
 * Every row is a modifier really in `scene.proton.json`; the arrows change the
 * order it runs in, the dot switches it off without removing it, and every
 * field writes the file. A kind this build cannot apply is shown greyed with
 * what it is, because a bevel that quietly does nothing is a worse answer than
 * one that says it did nothing.
 */
function renderStack(event) {
  const box = $("[data-stack]");
  const head = $("[data-stack-head]");
  if (!box || !head) return;

  const node = event.nodes?.[event.selected];
  // Only a mesh has geometry to modify. Offering the panel on a light would
  // invite somebody to fill it in and wonder why nothing happened.
  const shown = node?.kind === "mesh";
  head.hidden = !shown;
  if (!shown) {
    box.innerHTML = "";
    return;
  }

  const stack = node.modifiers ?? [];
  if (!stack.length) {
    box.innerHTML = `<div class="empty">No modifiers.</div>`;
    return;
  }

  const num = (label, field, value, row) =>
    `<div class="prop-row"><span class="lbl">${escapeHtml(label)}</span>` +
    `<input class="num" data-mod-field="${field}" data-mod-row="${row}"` +
    ` inputmode="decimal" value="${escapeHtml(String(value))}"></div>`;

  box.innerHTML = stack
    .map((one, row) => {
      const bar =
        `<div class="mod-bar">` +
        `<button class="mod-dot${one.off ? "" : " on"}" data-mod-off="${row}"` +
        ` title="${one.off ? "Switch on" : "Switch off"}"></button>` +
        `<span class="mod-kind">${escapeHtml(one.kind)}</span>` +
        (one.known ? "" : `<span class="chip warn">not applied</span>`) +
        `<span class="grow"></span>` +
        `<button class="mod-btn" data-mod-up="${row}" title="Earlier in the stack">\u2191</button>` +
        `<button class="mod-btn" data-mod-down="${row}" title="Later in the stack">\u2193</button>` +
        `<button class="mod-btn" data-mod-del="${row}" title="Remove">\u00d7</button>` +
        `</div>`;

      let body = "";
      if (!one.known) {
        body =
          `<p class="st-hint">This build cannot apply a ` +
          `${escapeHtml(one.kind)}. It is kept in the file exactly as it was.</p>`;
      } else if (one.kind === "mirror") {
        body =
          `<div class="prop-row"><span class="lbl">Axis</span>` +
          `<select class="num" data-mod-field="axis" data-mod-row="${row}">` +
          ["x", "y", "z"]
            .map(
              (axis) =>
                `<option value="${axis}"${axis === one.axis ? " selected" : ""}>${axis}</option>`,
            )
            .join("") +
          `</select></div>` +
          `<div class="prop-row"><span class="lbl">Weld</span>` +
          `<label class="mod-check"><input type="checkbox" data-mod-field="weld"` +
          ` data-mod-row="${row}"${one.weld ? " checked" : ""}>` +
          `<span>merge the seam</span></label></div>`;
      } else if (one.kind === "array") {
        body =
          num("Count", "count", one.count, row) +
          `<div class="prop-row"><span class="lbl">Offset</span>` +
          ["offsetX", "offsetY", "offsetZ"]
            .map(
              (field, axis) =>
                `<input class="num" data-mod-field="${field}" data-mod-row="${row}"` +
                ` inputmode="decimal" value="${escapeHtml(String(one.offset[axis]))}">`,
            )
            .join("") +
          `</div>`;
      } else if (one.kind === "subdivide") {
        body = num("Levels", "levels", one.levels, row);
      }

      return `<div class="mod${one.off ? " off" : ""}">${bar}${body}</div>`;
    })
    .join("");
}

document.addEventListener("click", (e) => {
  const index = lastScene?.selected;
  if (index === undefined || index < 0) return;
  const hit = (what) => {
    const button = e.target.closest?.(`[data-mod-${what}]`);
    return button ? Number(button.dataset[`mod${what[0].toUpperCase()}${what.slice(1)}`]) : null;
  };

  const off = hit("off");
  if (off !== null) {
    const now = lastScene.nodes[index].modifiers[off]?.off ?? false;
    dispatch({
      type: "setModifierField",
      index,
      row: off,
      field: "off",
      value: String(!now),
    });
    return;
  }
  const up = hit("up");
  if (up !== null) {
    dispatch({ type: "moveNodeModifier", index, row: up, to: Math.max(0, up - 1) });
    return;
  }
  const down = hit("down");
  if (down !== null) {
    dispatch({ type: "moveNodeModifier", index, row: down, to: down + 1 });
    return;
  }
  const gone = hit("del");
  if (gone !== null) dispatch({ type: "removeNodeModifier", index, row: gone });
});

document.addEventListener("change", (e) => {
  const index = lastScene?.selected;
  if (index === undefined || index < 0) return;

  const add = e.target.closest?.("[data-add-modifier]");
  if (add) {
    if (add.value) dispatch({ type: "setNodeModifier", index, kind: add.value });
    // Back to the prompt, so the control reads as an action and not as a
    // setting that is now stuck on "Mirror".
    add.value = "";
    return;
  }

  const field = e.target.closest?.("[data-mod-field]");
  if (!field) return;
  const value = field.type === "checkbox" ? String(field.checked) : field.value;
  dispatch({
    type: "setModifierField",
    index,
    row: Number(field.dataset.modRow),
    field: field.dataset.modField,
    value,
  });
});

/* Geometry arrives in two parts: an event that says what a mesh is, as JSON
   like everything else, and then the mesh itself, as bytes. Every request is
   numbered per key, so when two edits in a row send two answers and they come
   back in the other order, the older one is dropped rather than drawn over the
   newer. */
const asked = new Map();
let tickets = 0;

async function receiveGeometry(event) {
  // Numbered before anything else, so an event with no mesh also supersedes
  // one still travelling: a stack pushed over the budget must not have the
  // mesh from before it land afterwards and be drawn as current.
  const ticket = ++tickets;
  asked.set(event.path, ticket);

  // The description goes in straight away, so a first load shows the object's
  // real box while the bytes are on their way — and so a second scene event in
  // the meantime does not ask for the same mesh again. A surface already on
  // screen stays there instead, or every modifier edit would blink to a box.
  if (!event.ready || !hasSurface(event.path)) setGeometry({ ...event, mesh: null });
  if (!event.ready) return;

  const scene = lastScene?.path;
  let mesh;
  try {
    mesh = decodeMesh(await readGeometry(event.path));
  } catch (err) {
    // Only the newest request's failure is news; an older one failing after a
    // newer one succeeded is not.
    if (asked.get(event.path) !== ticket) return;
    meshProblems.set(event.path, String(err?.message ?? err));
    if (lastScene) renderNodeInspector(lastScene);
    return;
  }
  // Superseded while it was travelling: a newer answer for this key is on its
  // way, or the whole scene changed under it.
  if (asked.get(event.path) !== ticket || lastScene?.path !== scene) return;
  setGeometry({ ...event, mesh });
}

/** What is wrong with a node's geometry, if anything is.
 *
 * The empty string covers both "it is fine" and "it has not arrived yet",
 * which is right: a mesh still loading is not a fault to report, and the box
 * on screen already says the surface is not there.
 */
const meshProblems = new Map();
function meshProblem(path) {
  if (!path) return "";
  return meshProblems.get(path) ?? "";
}

document.addEventListener("change", (e) => {
  const box = e.target.closest?.("[data-node-mesh]");
  if (!box) return;
  const index = lastScene?.selected;
  if (index === undefined || index < 0) return;
  dispatch({ type: "setNodeMesh", index, path: box.value });
});

document.addEventListener("click", (e) => {
  const row = e.target.closest?.("[data-node]");
  if (row) {
    dispatch({ type: "selectNode", index: Number(row.dataset.node) });
    return;
  }
  const add = e.target.closest?.("[data-add-node]");
  if (add) {
    const kind = add.dataset.addNode;
    dispatch({
      type: "addNode",
      kind,
      // Capitalised, because it is a name a person will read in a tree.
      name: kind[0].toUpperCase() + kind.slice(1),
    });
  }
});

// Written on `change` rather than on every keystroke, for the reason the layer
// inspector is: `input` would put the disk in the typing loop and fill the
// undo history with one entry per letter.
document.addEventListener("change", (e) => {
  const index = lastScene?.selected;
  if (index === undefined || index < 0 || index === null) return;

  if (e.target.matches?.("[data-node-name]")) {
    dispatch({ type: "renameNode", index, name: e.target.value });
    return;
  }
  const box = e.target.closest?.("[data-node-xyz]");
  if (!box) return;

  const part = box.dataset.nodeXyz;
  const to = [...$$(`[data-node-xyz="${part}"]`)].map((one) => Number(one.value));
  // A box somebody typed a word into would otherwise send NaN, which JSON
  // cannot hold and the core would have to guess about. Put back instead.
  if (to.some((n) => !Number.isFinite(n))) {
    renderNodeInspector(lastScene);
    renderStatus(`${part} needs three numbers`, "warn");
    return;
  }
  dispatch({ type: "setNodeTransform", index, part, to });
});

/* ── The layer tree ──────────────────────────────────────────────

   Every row is an element that is really in the file on the artboard, read
   out of it by proton-design::markup. Clicking one selects it and outlines it
   in the frame; double-clicking opens the line that wrote it, in Codex.

   Selection is sent to the core rather than kept here, because the outline is
   drawn from the layer's CSS path and the core is what knows the tree. Two
   copies of "which layer" is two things to disagree. */

function renderLayers(event) {
  const box = $("[data-layers]");
  if (!box) return;
  const count = $("[data-layer-count]");
  const layers = event.layers ?? [];
  if (count) count.textContent = layers.length ? `${layers.length}` : "";

  if (!layers.length) {
    box.innerHTML = `<div class="empty">${escapeHtml(
      event.problem || "No component on the artboard.",
    )}</div>`;
    return;
  }

  box.innerHTML = layers
    .map((layer, i) => {
      // What the element is called, in the order somebody looks for it: the
      // id if it has one, then classes, then its text. A tree of bare `div`
      // is a tree nobody can navigate.
      const name = layer.id
        ? `<b>#${escapeHtml(layer.id)}</b>`
        : layer.classes.length
          ? `<i>.${escapeHtml(layer.classes[0])}</i>`
          : "";
      const text = layer.text
        ? `<span class="lay-text">${escapeHtml(layer.text)}</span>`
        : "";
      return (
        `<div class="row lay${i === event.selected ? " sel" : ""}" data-layer="${i}"` +
        ` style="padding-left:${8 + layer.depth * 11}px"` +
        ` title="${escapeHtml(layer.tag)} — line ${layer.line}">` +
        `<span class="lay-tag">${escapeHtml(layer.tag)}</span>${name}${text}` +
        `<span class="grow"></span><span class="lay-line">${layer.line}</span>` +
        `</div>`
      );
    })
    .join("");
}

/* ── The selected layer's inspector ──────────────────────────────

   The reverse binding: what is typed here is spliced into the component file,
   replacing the bytes that one thing occupies and moving nothing else. There
   is no save button, for the same reason the token rows have none — a
   component with unsaved changes is one the artboard disagrees with.

   Written on `change` rather than on every keystroke. `input` would put the
   disk in the typing loop and fill the undo history with one entry per letter;
   `change` fires when the box is left or Enter is pressed, which is when
   somebody has decided what they meant. */

function renderLayerInspector(event) {
  const head = $("[data-layer-head]");
  const form = $("[data-layer-form]");
  if (!head || !form) return;

  const layer = event.layers?.[event.selected];
  head.hidden = !layer;
  form.hidden = !layer;
  if (!layer) return;

  $("[data-layer-tag]").textContent = layer.tag;
  const text = $("[data-layer-text]");
  const id = $("[data-layer-id]");
  const cls = $("[data-layer-class]");

  // Not while it is being typed in: replacing the value under the caret moves
  // it to the end, on every repaint the core sends.
  const put = (box, value) => {
    if (document.activeElement !== box) box.value = value;
  };
  put(text, layer.text);
  put(id, layer.id);
  put(cls, layer.classes.join(" "));

  // An element with layers inside it cannot have its text replaced — doing so
  // would delete them. Said here rather than only refused on the way in, so
  // the box does not look like it will work.
  text.disabled = !layer.leaf;
  $("[data-layer-note]").textContent = layer.leaf
    ? `line ${layer.line}`
    : `line ${layer.line} — has elements inside it, so its text is edited in Codex`;
}

document.addEventListener("change", (e) => {
  const box = e.target;
  const index = lastArtboard?.selected;
  if (index === undefined || index < 0) return;
  if (box.matches?.("[data-layer-text]")) {
    dispatch({ type: "setLayerText", index, text: box.value });
  } else if (box.matches?.("[data-layer-id]")) {
    dispatch({ type: "setLayerAttribute", index, name: "id", value: box.value.trim() });
  } else if (box.matches?.("[data-layer-class]")) {
    dispatch({ type: "setLayerAttribute", index, name: "class", value: box.value.trim() });
  }
});

function renderArtboard(event) {
  const board = $("[data-artboard]");
  if (!board) return;

  const file = $("[data-ab-file]");
  if (file) {
    file.textContent = event.path ? event.path.split(/[\\/]/).pop() : "no theme";
    file.className = "chip" + (event.path ? " accent" : "");
  }

  const scopes = scopesOf(event.tokens);
  if (artboardScope === null || !scopes.includes(artboardScope)) {
    artboardScope = scopes[0] ?? null;
  }

  // The chooser is rebuilt only when the scopes themselves change — replacing
  // it on every token edit would close it while somebody is choosing.
  const picker = $("[data-ab-scope]");
  if (picker && picker.dataset.built !== scopes.join("|")) {
    picker.dataset.built = scopes.join("|");
    picker.innerHTML = scopes
      .map(
        (scope) =>
          `<option value="${escapeHtml(scope)}"${scope === artboardScope ? " selected" : ""}>` +
          `${escapeHtml(scope || "(top level)")}</option>`,
      )
      .join("");
  }

  if (!event.tokens.length) {
    board.innerHTML = `<div class="empty">${escapeHtml(
      event.problem || "Open a stylesheet to preview its colors and components.",
    )}${openFileAction("Open stylesheet", ".css")}</div>`;
    board.removeAttribute("style");
    artboardPainted = null;
    return;
  }

  const palette = resolveScope(event.tokens, artboardScope);
  const shown = lastArtboard || {};
  const key = JSON.stringify([
    [...palette],
    artboardWidth,
    shown.path,
    shown.source,
    shown.problem,
    shown.selected,
  ]);
  // Repainted only when something it draws actually changed. The frame is
  // reloaded by writing `srcdoc`, and reloading it on every heartbeat would
  // reset a scroll position somebody was reading from.
  if (key === artboardPainted) return;
  artboardPainted = key;

  // The palette, as custom properties. Everything below reads them through
  // `var()`, so the browser resolves the theme rather than this code.
  const vars = [...palette]
    .map(([name, value]) => `${name}: ${value}`)
    .join("; ");
  board.setAttribute(
    "style",
    `${vars}; background: var(--bg, #16161e); color: var(--ink, #c0caf5)` +
      (artboardWidth ? `; width: ${artboardWidth}px; max-width: 100%` : ""),
  );

  // A component, if one is on the artboard. The swatches below are what a
  // project with no component gets — a palette is still worth looking at.
  if (shown.source) {
    const frame = document.createElement("iframe");
    frame.className = "ab-frame";
    // Loosened by exactly one flag, and deliberately. The frame needs to run
    // one script to report a click; it still has no `allow-same-origin`, so
    // its origin stays opaque, it cannot read this document, and
    // `window.__TAURI__` is not reachable from it. No forms, no navigation,
    // no popups. The CSP nonce above is what keeps the *component's* own
    // scripts from running alongside.
    frame.setAttribute("sandbox", "allow-scripts");
    frame.setAttribute("title", "component preview");
    // Kept so a message can be checked against the frame that is actually on
    // screen, not merely against "some frame".
    artboardFrame = frame;
    frame.srcdoc = artboardDoc(
      shown.source,
      shown.document,
      vars,
      shown.layers?.[shown.selected]?.path ?? "",
      artboardNonce(),
    );
    board.replaceChildren(frame);
    return;
  }
  if (shown.problem) {
    board.innerHTML = `<div class="empty">${escapeHtml(shown.problem)}</div>`;
    return;
  }

  const colours = [...palette].filter(([, value]) => swatchable(value));
  const swatches = colours
    .map(
      ([name, value]) =>
        `<div class="ab-swatch" title="${escapeHtml(name)}: ${escapeHtml(value)}">` +
        `<i style="background:${escapeHtml(value)}"></i>` +
        `<b>${escapeHtml(name.replace(/^--/, ""))}</b>` +
        `<code>${escapeHtml(value)}</code>` +
        `</div>`,
    )
    .join("");

  // The specimen names tokens with fallbacks throughout, because it has to
  // render somebody else's theme as well as this one — a project that calls
  // its accent something else still gets a readable artboard.
  board.innerHTML =
    `<div class="ab-head">` +
    `<h3>${escapeHtml(artboardScope || "(top level)")}</h3>` +
    `<span>${colours.length} colours · ${palette.size} tokens</span>` +
    `</div>` +
    `<div class="ab-swatches">${swatches}</div>` +
    `<div class="ab-specimen">` +
    `<div class="ab-card">` +
    `<div class="ab-title">Heading</div>` +
    `<p>Body text, set in the theme's own ink over its own ground.</p>` +
    `<div class="ab-row">` +
    `<span class="ab-btn">Primary</span>` +
    `<span class="ab-btn ghost">Secondary</span>` +
    `</div>` +
    `<div class="ab-row">` +
    `<span class="ab-pill ok">ok</span>` +
    `<span class="ab-pill warn">warn</span>` +
    `<span class="ab-pill err">error</span>` +
    `</div>` +
    `</div>` +
    `</div>`;
}

/* ── Design mode's inspector ─────────────────────────────────────

   Every custom property in the theme, grouped by the selector that declares
   it. Editing a row writes the file — there is no save, because a palette
   with unsaved changes is one the artboard disagrees with. */

/** A value a colour input can actually hold. Everything else gets a text box. */
function hexish(value) {
  return /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(value.trim());
}

/** Something to put in a swatch, or nothing if it is not a colour at all. */
function swatchable(value) {
  return /^(#|rgb|hsl|oklch|oklab|lab|lch|color\()/i.test(value.trim());
}

let themePainted = null;

function renderTheme(event) {
  lastTheme = event;
  const name = $("[data-theme-name]");
  if (name) {
    const file = event.path ? event.path.split(/[\/]/).pop() : "no theme";
    name.textContent = event.problem || file;
    name.className = "chip" + (event.problem ? " warn" : event.path ? " accent" : "");
  }

  const box = $("[data-tokens]");
  if (!box) return;

  // Rebuilt only when something actually differs. A colour input fires while
  // it is being dragged, and replacing the markup under it would close the
  // picker on the first move.
  const key = JSON.stringify([event.tokens, event.selected]);
  if (key === themePainted) return;
  themePainted = key;

  if (!event.tokens.length) {
    box.innerHTML = `<div class="empty">${escapeHtml(
      event.problem || "No theme. Open a stylesheet with custom properties.",
    )}</div>`;
    return;
  }

  let html = "";
  let scope = null;
  event.tokens.forEach((token, index) => {
    if (token.scope !== scope) {
      scope = token.scope;
      html += `<div class="sec-head">${escapeHtml(scope || "(top level)")}</div>`;
    }
    // Only for colours the picker cannot hold. A hex value gets an
    // `<input type="color">`, which *is* a swatch — drawing a second one
    // beside it puts two squares of the same colour on every row.
    const swatch =
      swatchable(token.value) && !hexish(token.value)
        ? `<span class="swatch" style="background:${escapeHtml(token.value)}"></span>`
        : "";
    // A colour gets the native picker; anything else — a shadow, a font
    // stack, a `var()` — gets a text box, because converting those to a
    // colour to show them would lose what they are.
    const editor = hexish(token.value)
      ? `<input class="tok-color" type="color" value="${escapeHtml(token.value.length === 4
          ? "#" + token.value.slice(1).split("").map((c) => c + c).join("")
          : token.value)}" data-token="${index}" title="${escapeHtml(token.value)}">`
      : "";
    html +=
      `<div class="prop-row token${index === event.selected ? " on" : ""}" data-token-row="${index}">` +
      swatch +
      `<span class="lbl" title="${escapeHtml(token.name)}">${escapeHtml(token.name)}</span>` +
      editor +
      `<input class="val tok-value" value="${escapeHtml(token.value)}" data-token="${index}" spellcheck="false">` +
      `</div>`;
  });
  box.innerHTML = html;
}

/** Send a token's new value, if it really is new. */
function writeToken(index, value) {
  const token = lastTheme?.tokens?.[index];
  if (!token || value === token.value) return;
  dispatch({
    type: "setToken",
    name: token.name,
    scope: token.scope,
    value,
  });
}

/**
 * The dock: the selected cell's dataframe, given room to be read.
 *
 * A frame is already drawn under its own cell, but in a column the width of
 * the stage and among everything else the cell printed. The dock is the same
 * table with the width to read it in — which is the whole reason `Slot::Table`
 * is what Data's dock holds rather than an inspector.
 *
 * It follows the selection rather than the last frame produced: a panel that
 * showed whatever ran most recently would change under you while you read it.
 */
function renderTableDock(event) {
  const box = $("[data-df-dock]");
  if (!box) return;
  const shape = $("[data-df-shape]");

  const cell = event.cells.find((it) => it.selected);
  const table = cell?.outputs.find((out) => out.type === "table")?.table;

  // Cheap, and it matters: a running cell sends a notebook every heartbeat,
  // and rebuilding a table of thirty rows each time throws away the text
  // selection of anyone reading it.
  const key = table ? JSON.stringify(table) : cell ? "none" : "shut";
  if (key === dockPainted) return;
  dockPainted = key;

  if (!table) {
    box.innerHTML = `<div class="empty">${
      cell
        ? "No table in this cell. Run one that ends in a dataframe."
        : "Open a .ipynb to start"
    }</div>`;
    if (shape) shape.hidden = true;
    return;
  }

  if (shape) {
    shape.hidden = false;
    // Rows as the frame counted them, not as many as were sent.
    const rows = table.total ?? table.rows.length;
    shape.textContent = `${rows.toLocaleString()} × ${table.columns.length}`;
  }
  box.innerHTML = tableHtml(table);
}

/**
 * Paint one pane.
 *
 * Every pane shows the same notebook — there is one document and one kernel
 * behind all of them — so they differ only in which cell is marked. The
 * focused pane takes that from the notebook itself, where the core keeps the
 * live selection; the others take it from the split layout, which is where a
 * pane's cell is parked while it is not focused.
 */
function paintCells(box, event) {
  const view = Number(box.dataset.view ?? 0);
  if (!event.cells.length) {
    box.innerHTML = `<div class="empty">Open a notebook to start.${openFileAction("Open notebook", ".ipynb")}</div>`;
    notebookPainted.delete(view);
    return;
  }

  const mine =
    view === (lastSplits?.focused ?? 0)
      ? event.cells.findIndex((cell) => cell.selected)
      : (lastSplits?.views?.[view]?.cell ?? 0);

  // Cell by cell, for two reasons. The cheap one is the same as the editor
  // pane's: a running cell sends a notebook every heartbeat and almost all of
  // it is unchanged. The one that matters is that these are editable — and
  // replacing the markup underneath somebody who is typing takes the caret
  // with it, on every keystroke of output that arrives while they think.
  const rows = event.cells.map((cell, i) =>
    cellHtml({ ...cell, selected: i === mine }, i),
  );
  const painted = notebookPainted.get(view) ?? [];
  // Only this pane's caret matters here. A caret in the *other* pane must not
  // stop this one from being redrawn.
  const held = document.activeElement?.closest?.("[data-cell]");
  const editing = held && box.contains(held) ? Number(held.dataset.cell) : -1;

  const kids = box.children;
  for (let i = 0; i < rows.length; i++) {
    if (i === editing && kids[i]) {
      // The cell holding the caret is never replaced — that would take the
      // caret with it. But it must still be able to gain and lose the
      // selection, or moving off a focused cell leaves it looking selected
      // and moving onto one never marks it. Class only; the text is yours.
      kids[i].classList.toggle("on", i === mine);
      kids[i].classList.toggle("running", event.cells[i].running);
      continue;
    }
    if (painted[i] === rows[i] && kids[i]) continue;
    const holder = document.createElement("div");
    holder.innerHTML = rows[i];
    const fresh = holder.firstElementChild;
    if (!fresh) continue;
    if (kids[i]) kids[i].replaceWith(fresh);
    else box.appendChild(fresh);
  }
  while (kids.length > rows.length) box.lastElementChild.remove();
  // The cell being typed in was skipped, so what is on screen there is not
  // what was just painted. Forgetting that is what would leave it stale after
  // the caret moves on.
  if (editing >= 0) rows[editing] = null;
  notebookPainted.set(view, rows);
}


/* ── git ──────────────────────────────────────────────────────────────
   The core asks git; this draws the answer. A row is a path, so clicking
   one opens it — the core spells these the way `openBuffer` does, which
   is what makes that possible without translating anything here. */

/** What git calls each state, for people who do not read git. */
const GIT_STATE = { M: "modified", A: "added", D: "deleted", "?": "untracked" };

function renderRepo(event) {
  const status = $("#sl-git");
  status.hidden = !event.branch && !event.files.length;
  status.textContent = [event.branch, event.files.length ? `${event.files.length} changed` : ""]
    .filter(Boolean).join(" · ");
  const branch = $("[data-git-branch]");
  if (branch) {
    // No branch is three ordinary situations — no repository, a detached
    // HEAD, a repository with no commits — and none of them is an error.
    branch.textContent = event.branch ?? "no branch";
    branch.className = "chip" + (event.branch ? " accent" : "");
  }

  const count = $("[data-git-count]");
  if (count) {
    const n = event.files.length;
    count.textContent = n === 1 ? "1 change" : `${n} changes`;
    count.hidden = false;
  }

  const box = $("[data-git-files]");
  if (!box) return;
  box.innerHTML = event.files.length
    ? event.files
        .map((file) => {
          const state = GIT_STATE[file.state] ?? "changed";
          const name = file.path.split("/").pop();
          // Without the trailing separator: it is the only character that
          // would be left once the folder is ellipsised, and a lone slash
          // is not context.
          const folder = file.path.slice(0, -name.length).replace(/\/$/, "");
          return (
            `<div class="row" data-git-open="${escapeHtml(file.path)}" title="${escapeHtml(file.path)} — ${state}">` +
            `<span class="gst g-${state}">${escapeHtml(file.state)}</span>` +
            `<span class="gname">${escapeHtml(name)}</span>` +
            `<span class="gdir">${escapeHtml(folder)}</span></div>`
          );
        })
        .join("")
    : `<div class="empty">Nothing changed</div>`;
}

/** The rows drawn last time, so only the ones that moved are redrawn. */
let terminalPainted = [];
/** The last screen the core sent, to draw again without being sent it. */
let lastTerminal = null;
/** The GPU renderer, while Settings asks for one and there is a GPU to ask. */
let termGL = null;
/** A catch-up of the rows under the GPU's picture, waiting for a still screen. */
let termRowsLater = 0;
/** How long the screen has to be still before those rows catch up, in ms. */
const TERM_ROWS_SETTLE = 150;

/** Give the terminal the keyboard, or take it back.
 *
 *  The focus goes to the hidden field the terminal types through, because a
 *  paste only lands in something that takes text — unless a mouse has just
 *  selected some of the screen, which moving the focus would throw away. */
function focusTerminal(on) {
  const was = terminalFocused;
  terminalFocused = on;
  $("[data-term-screen]")?.classList.toggle("on", on);
  // The caret is drawn solid or hollow by who has the keyboard, and the GPU
  // draws only when asked.
  if (termGL && was !== on) termGL.redraw(on);
  if (on && window.getSelection()?.isCollapsed !== false) {
    $("[data-term-input]")?.focus({ preventScroll: true });
  }
  renderKeyboardOwner();
}

// A paste while the shell has the keyboard is the shell's: the core sends it
// as a terminal pastes — Return for line breaks, and bracketed when the shell
// asked, so a pasted command waits for Enter rather than running.
document.addEventListener("paste", (e) => {
  if (!terminalFocused) return;
  const text = e.clipboardData?.getData("text/plain") ?? "";
  e.preventDefault();
  if (text) dispatch({ type: "terminalPaste", text });
});

// What an input method composes — an accent, a character from another
// script — arrives as text in the field rather than as a key, and is the
// shell's too. The field is emptied every time, so nothing builds up in it.
$("[data-term-input]")?.addEventListener("input", (e) => {
  const field = /** @type {HTMLTextAreaElement} */ (e.target);
  if (/** @type {InputEvent} */ (e).isComposing) return;
  const text = field.value;
  field.value = "";
  if (text && terminalFocused) dispatch({ type: "terminalInput", text });
});
$("[data-term-input]")?.addEventListener("compositionend", (e) => {
  const field = /** @type {HTMLTextAreaElement} */ (e.target);
  const text = field.value;
  field.value = "";
  if (text && terminalFocused) dispatch({ type: "terminalInput", text });
});

function runHtml(run) {
  const classes = [];
  // The theme names sixteen colours; the other 240 are xterm's, and go inline.
  const inline = [];
  if (run.fg != null) {
    if (run.fg < 16) classes.push(`tf${run.fg}`);
    else inline.push(`color:${xtermHex(run.fg)}`);
  }
  if (run.bg != null) {
    if (run.bg < 16) classes.push(`tb${run.bg}`);
    else inline.push(`background:${xtermHex(run.bg)}`);
  }
  if (run.bold) classes.push("tbold");
  if (run.italic) classes.push("titalic");
  if (run.underline) classes.push("tunder");
  if (run.inverse) classes.push("tinv");
  // A true colour has no palette name either.
  if (run.rgb) inline.push(`color:${run.rgb}`);
  const style = inline.length ? ` style="${inline.join(";")}"` : "";
  const attrs = `${classes.length ? ` class="${classes.join(" ")}"` : ""}${style}`;
  return attrs ? `<span${attrs}>${escapeHtml(run.text)}</span>` : escapeHtml(run.text);
}

function renderTerminal(event) {
  terminalRunning = event.running;
  // No shell at all — not one that exited, which keeps its last screen. Every
  // project has its own, so this is what switching to one that has never had
  // one looks like; with the pane open, that is the request for one, exactly
  // as opening the pane is.
  if (!event.running && !event.rows.length && termPane && !termPane.hidden) askForTerminal();
  // Written only when they change: a busy shell sends twenty screens a
  // second, and the same words written again are still a change to lay out.
  const state = $("[data-term-state]");
  if (state) {
    const text = event.running ? "ready" : event.rows.length ? "exited" : "starting";
    if (state.textContent !== text) state.textContent = text;
    state.className = "chip" + (event.running ? " accent" : "");
  }
  // A shell you exited out of is the one case where there is something to
  // press. While one is running there is nothing to do, so nothing is shown.
  const restart = $("[data-term-restart]");
  if (restart) restart.hidden = event.running || !event.rows.length;

  const back = $("[data-term-back]");
  if (back) {
    back.hidden = !event.scrollback;
    const behind = $("[data-term-back-rows]");
    if (event.scrollback && behind.textContent !== String(event.scrollback)) behind.textContent = String(event.scrollback);
  }

  lastTerminal = event;
  if (termGL) {
    termGL.draw(event, terminalFocused);
    // The rows are still written underneath, unseen: the mouse selects them,
    // the clipboard copies them and a screen reader reads them. None of those
    // needs every frame of a build streaming past, which is the work the GPU
    // is here to save — so they catch up once the screen is still.
    clearTimeout(termRowsLater);
    termRowsLater = setTimeout(() => {
      termRowsLater = 0;
      paintTerminalRows();
    }, TERM_ROWS_SETTLE);
    return;
  }
  paintTerminalRows();
}

/** Bring the rows under the GPU's picture up to date now, rather than when
 *  the screen is next still: a selection starts on what is shown. */
function settleTerminalRows() {
  if (!termRowsLater) return;
  clearTimeout(termRowsLater);
  termRowsLater = 0;
  paintTerminalRows();
}

/** Draw the terminal with the GPU or as text, as Settings → Appearance says. */
function renderTerminalRenderer() {
  const screen = $("[data-term-screen]");
  if (!screen) return;
  const want = document.documentElement.dataset.termRenderer === "gpu";
  if (want && !termGL) {
    termGL = createTerminalGL(screen, {
      // A driver reset or a long sleep can take the GPU's context away. The
      // text renderer is always there to fall back on.
      onLost: () => {
        termGL?.destroy();
        termGL = null;
        screen.classList.remove("gpu");
        settleTerminalRows();
      },
    });
    if (termGL && lastTerminal) termGL.draw(lastTerminal, terminalFocused);
  } else if (!want && termGL) {
    termGL.destroy();
    termGL = null;
    settleTerminalRows();
  } else {
    // A new font size is a new cell.
    termGL?.restyle();
  }
  screen.classList.toggle("gpu", Boolean(termGL));
}

/** Write the last screen out as rows of text: the picture itself for the
 *  text renderer, and what lies under the GPU's for the mouse and clipboard. */
function paintTerminalRows() {
  const event = lastTerminal;
  const screen = $("[data-term-screen]");
  if (!screen || !event) return;
  const [cursorRow, cursorCol] = event.cursor;

  const rows = event.rows.map((row, i) => {
    const body = row.map((run) => runHtml(run)).join("");
    // The cursor is drawn as a class on its row plus a marker span, rather
    // than by splitting a run: the core said which cell it is on, and
    // cutting runs to find it would be re-deriving what it already sent.
    const here = event.cursorVisible && i === cursorRow;
    return `<div class="trow${here ? " tcur" : ""}"${
      here ? ` style="--tcol:${cursorCol}"` : ""
    }>${body || "&nbsp;"}</div>`;
  });

  // Row by row, for the reason the editor pane is: a shell that is printing
  // sends a screen twenty times a second, and almost all of it is the same
  // screen as last time. Replacing the panel's markup for each one is a parse
  // and a full layout per frame, which is most of what makes a build feel
  // like it is dragging the window along behind it. Even a blinking cursor
  // changes exactly two rows.
  const before = terminalPainted;
  const kids = screen.children;
  for (let i = 0; i < rows.length; i++) {
    if (before[i] === rows[i] && kids[i]) continue;
    const holder = document.createElement("div");
    holder.innerHTML = rows[i];
    const fresh = holder.firstElementChild;
    if (!fresh) continue;
    if (kids[i]) kids[i].replaceWith(fresh);
    else screen.appendChild(fresh);
  }
  while (kids.length > rows.length) screen.lastElementChild.remove();
  terminalPainted = rows;

  screen.scrollTop = screen.scrollHeight;
}

/** What a keystroke means to a shell. */
function terminalBytes(e) {
  if (e.key.length === 1 && !e.ctrlKey && !e.altKey && !e.metaKey) return e.key;
  switch (e.key) {
    case "Enter":
      return "\r";
    case "Backspace":
      // A shell expects DEL here, not backspace — the difference between
      // deleting a character and printing `^H`.
      return "\x7f";
    case "Tab":
      return "\t";
    case "Escape":
      return "\x1b";
    case "ArrowUp":
      return "\x1b[A";
    case "ArrowDown":
      return "\x1b[B";
    case "ArrowRight":
      return "\x1b[C";
    case "ArrowLeft":
      return "\x1b[D";
    case "Home":
      return "\x1b[H";
    case "End":
      return "\x1b[F";
    case "Delete":
      return "\x1b[3~";
    default:
      // `⌃c`, `⌃d` and the rest are the letter with the top three bits off.
      if (e.ctrlKey && /^[a-z]$/i.test(e.key)) {
        return String.fromCharCode(e.key.toUpperCase().charCodeAt(0) - 64);
      }
      return null;
  }
}

/** How many rows the panel is showing, for a page-sized scroll. */
function terminalRows() {
  return lastTerminal?.rows.length || $("[data-term-screen]")?.querySelectorAll(".trow").length || 24;
}

/** The size of one character cell, measured rather than assumed.
 *
 * A row cannot be measured for this. `.trow` is a block, so its width is the
 * container's whatever is written in it — dividing that by the row's character
 * count gave a cell width that depended on how long the first line happened to
 * be. A one-character prompt made the terminal twenty columns wide in a panel
 * that fits fifty.
 *
 * So: a hidden span of known length, in the terminal's own font, divided by
 * that length. `pre` because a run of spaces would otherwise collapse and the
 * answer would be too small.
 */
function cellSize(screen) {
  const probe = document.createElement("span");
  probe.className = "term";
  probe.style.cssText = "position:absolute;visibility:hidden;white-space:pre;padding:0";
  probe.textContent = "0".repeat(100);
  screen.appendChild(probe);
  const box = probe.getBoundingClientRect();
  probe.remove();
  return { width: box.width / 100 || 7, height: box.height || 17 };
}

/** How many cells fit, so the shell can be told. */
/** The size last sent to the shell. Every size sent is answered with a
 *  screen, and every screen used to send the size again — once a frame, for as
 *  long as the panel was open. Only a size that changed goes now. */
let termSize = { rows: 0, cols: 0 };

/** Tell the shell how big the panel is, if that is news. `force` sends it
 *  regardless, for a shell that may never have been told: one just started,
 *  or another project's. */
function reportTerminalSize(force = false) {
  const screen = $("[data-term-screen]");
  if (!screen || !screen.offsetParent) return;
  const cell = cellSize(screen);
  // Padding is inside `clientWidth`/`clientHeight`, so it has to come off or
  // the answer is one row and a couple of columns too many — and the bottom
  // row of the shell is drawn where it cannot be seen.
  const style = getComputedStyle(screen);
  const inner = (a, b) => parseFloat(style[a]) + parseFloat(style[b]) || 0;
  const height = screen.clientHeight - inner("paddingTop", "paddingBottom");
  const width = screen.clientWidth - inner("paddingLeft", "paddingRight");

  const rows = Math.max(1, Math.floor(height / cell.height));
  const cols = Math.max(20, Math.floor(width / cell.width));
  if (force !== true && rows === termSize.rows && cols === termSize.cols) return;
  termSize = { rows, cols };
  dispatch({ type: "terminalSize", rows, cols });
}

/** How many rows a wheel notch moves. */
const TERM_WHEEL_ROWS = 3;

// The wheel over the screen looks back through the history. The panel renders
// exactly the rows the core sent, so there is nothing here for the browser to
// scroll — without this the wheel does nothing at all.
document.addEventListener(
  "wheel",
  (e) => {
    if (!e.target.closest("[data-term-screen]")) return;
    e.preventDefault();
    // Up the page is back in time, which is the direction a wheel already
    // means: deltaY is negative going up.
    const lines = e.deltaY < 0 ? TERM_WHEEL_ROWS : -TERM_WHEEL_ROWS;
    dispatch({ type: "terminalScroll", lines });
  },
  { passive: false },
);

/** The mode's four slots. The core says what belongs where; this shows it.
 *
 * Only the mode switch itself today — the workspaces were already built one
 * per mode, so the layout event confirms what is on screen rather than
 * rebuilding it. What it adds is the *reason*: the front end no longer has
 * its own opinion about which mode has a dock.
 */
function renderLayout(layout) {
  document.documentElement.dataset.stage = layout.stage;
  document.documentElement.dataset.document = layout.document;
  // A mode with nothing for its dock gets the space back rather than an
  // empty panel — Chat is the one that means it.
  const workspace = $(`.workspace[data-ws="${layout.mode}"]`);
  if (workspace) workspace.dataset.dock = layout.dock;
}

// The artboard's palette chooser and its breakpoints. Both only change what
// is drawn, so neither goes near the core.
document.addEventListener("change", (e) => {
  const picker = e.target.closest?.("[data-ab-scope]");
  if (!picker) return;
  artboardScope = picker.value;
  artboardPainted = null;
  if (lastTheme) renderArtboard(lastTheme);
});

// A layer: click to select and outline it, double-click to open the line that
// wrote it. Both go to the core, which owns which layer is selected — the
// outline is drawn from the layer's CSS path, and the core is what has the
// tree that path came from.
document.addEventListener("click", (e) => {
  const row = e.target.closest?.("[data-layer]");
  if (!row) return;
  dispatch({ type: "selectLayer", index: Number(row.dataset.layer) });
});

document.addEventListener("dblclick", (e) => {
  const row = e.target.closest?.("[data-layer]");
  if (!row) return;
  dispatch({ type: "revealLayer", index: Number(row.dataset.layer) });
});

// Choosing a palette from the list is the same act as choosing it from the
// artboard's own selector, so it goes through the same state.
document.addEventListener("click", (e) => {
  const row = e.target.closest?.("[data-palette]");
  if (!row || !lastTheme) return;
  artboardScope = row.dataset.palette;
  artboardPainted = null;
  const picker = $("[data-ab-scope]");
  if (picker) picker.value = artboardScope;
  renderArtboard(lastTheme);
  renderPalettes(lastTheme);
});

document.addEventListener("click", (e) => {
  const size = e.target.closest?.("[data-ab-width]");
  if (!size) return;
  $$("[data-ab-size] button").forEach((b) => b.classList.toggle("on", b === size));
  artboardWidth = Number(size.dataset.abWidth);
  artboardPainted = null;
  if (lastTheme) renderArtboard(lastTheme);
});

/* A token's new value. `change` rather than `input` on the colour picker:
   `input` fires continuously while it is dragged, and every one of those would
   be a write to the stylesheet. */
document.addEventListener("change", (e) => {
  const field = e.target.closest?.(".tok-color, .tok-value");
  if (!field) return;
  writeToken(Number(field.dataset.token), field.value.trim());
});

// Enter commits a text value without waiting for the field to lose focus.
document.addEventListener("keydown", (e) => {
  const field = e.target.closest?.(".tok-value");
  if (!field || e.key !== "Enter") return;
  e.preventDefault();
  writeToken(Number(field.dataset.token), field.value.trim());
  field.blur();
});

// Clicking a token row selects it. Deliberately its own listener rather than a
// branch in the one below: the click is usually on the row's own input, which
// has to go on working, so this must not swallow the event.
document.addEventListener("click", (e) => {
  const row = e.target.closest?.("[data-token-row]");
  if (row) dispatch({ type: "selectToken", index: Number(row.dataset.tokenRow) });
});

document.addEventListener("click", (e) => {
  /* ── notebooks ────────────────────────────────────────────────── */
  const run = e.target.closest("[data-run-cell]");
  if (run) {
    dispatch({ type: "runCell", index: Number(run.dataset.runCell) });
    return;
  }
  const drop = e.target.closest("[data-drop-cell]");
  if (drop) {
    dispatch({ type: "removeCell", index: Number(drop.dataset.dropCell) });
    return;
  }
  if (e.target.closest("[data-nb-add]")) {
    dispatch({ type: "addCell", code: true });
    return;
  }
  if (e.target.closest("[data-nb-save]")) {
    dispatch({ type: "saveNotebook" });
    return;
  }
  if (e.target.closest("[data-nb-runall]")) {
    dispatch({ type: "runAll" });
    return;
  }
  // Clicking a cell puts the cursor on it. Without this the caret and the
  // selection disagree, and `dd` deletes a cell you are not looking at.
  const inCell = e.target.closest("[data-cell]");
  if (inCell) {
    const index = Number(inCell.dataset.cell);
    // Leaving a rendered markdown cell puts its prose back.
    if (markdownEditing >= 0 && markdownEditing !== index) {
      markdownEditing = -1;
      notebookPainted.clear();
    }
    // A click is also a way of choosing a pane. Without this, clicking a cell
    // in the pane that does not have the focus moves the *other* pane's
    // selection, and the cell you clicked stays unmarked.
    const pane = inCell.closest("[data-cells]");
    const view = pane ? Number(pane.dataset.view ?? 0) : 0;
    if (lastSplits && view !== lastSplits.focused) {
      dispatch({ type: "focusSplit", index: view });
    }
    dispatch({ type: "selectCell", index });
    return;
  }
  // A value in a dataframe is usually a thing that exists in the code too.
  // This is the cross-mode jump from the other end: the table says what to
  // look for, Telescope finds where, and Codex is where you land.
  const df = e.target.closest("[data-df-cell]");
  if (df) {
    const value = df.dataset.dfCell.trim();
    if (value) {
      dispatch({ type: "openPicker", kind: "grep" });
      dispatch({ type: "pickerQuery", text: value });
    }
    return;
  }

  if (e.target.closest("[data-doc-close]")) {
    closeDocument();
    return;
  }
  if (e.target.closest("[data-doc-fit]")) {
    fitDocument();
    return;
  }
  const zoom = e.target.closest("[data-doc-zoom]");
  if (zoom) {
    zoomDocument(Number(zoom.dataset.docZoom));
    return;
  }

  // A changed file is a file: clicking one opens it, which is the only thing
  // anybody wants from a list of them.
  const changed = e.target.closest("[data-git-open]");
  if (changed) {
    dispatch({ type: "openBuffer", path: changed.dataset.gitOpen });
    return;
  }

  // The indicator is also the way out of history.
  if (e.target.closest("[data-term-back]")) {
    dispatch({ type: "terminalToBottom" });
    return;
  }

  // A fresh shell, for when the last one was exited out of.
  if (e.target.closest("[data-term-restart]")) {
    dispatch({ type: "openTerminal" });
    focusTerminal(true);
    return;
  }
  // Clicking the screen gives it the keyboard; clicking anywhere else takes
  // it back, or every keystroke in the editor would go to the shell.
  focusTerminal(Boolean(e.target.closest("[data-term-screen]")));
});

// The shell is told the panel's size, and the panel changes size for reasons
// that have nothing to do with the shell: the window, the dock, a split. An
// observer is the only thing that hears all of them.
const termScreen = $("[data-term-screen]");
if (termScreen) new ResizeObserver(() => reportTerminalSize()).observe(termScreen);
termScreen?.addEventListener("mousedown", settleTerminalRows);

/* ── the terminal pane ────────────────────────────────────────────────
   Beside the code rather than in the dock, the way a Zed pane group splits
   a terminal to the right of the editor — so the shell and the agent can be
   on screen together. Open or shut, and how wide, are view state and live
   here; the shell is the core's, and hiding the pane does not end it. */
const termPane = $("[data-term-pane]");
const termGutter = $("[data-term-resize]");
/** Whether the core has a live shell, from the last Event::Terminal. */
let terminalRunning = false;
const TERM_WIDTH = 392;

/** Ask the core for a shell in the active project — once at a time, because
 *  the pane opening and the project's first empty screen both ask, and they
 *  usually arrive together. */
let terminalAsked = false;
function askForTerminal() {
  if (terminalAsked) return;
  terminalAsked = true;
  dispatch({ type: "openTerminal" }).finally(() => { terminalAsked = false; });
}

function showTerminal(on, { focus = false } = {}) {
  if (!termPane || !termGutter) return;
  termPane.hidden = !on;
  termGutter.hidden = !on;
  try {
    localStorage.setItem("proton.terminal", on ? "open" : "closed");
  } catch {
    /* storage blocked — it just won't persist */
  }
  $$("[data-term-toggle]").forEach((b) => {
    b.classList.toggle("on", on);
    b.setAttribute("aria-pressed", String(on));
  });
  if (on) {
    // Opening the pane is the request for a shell; there is no other button.
    if (!terminalRunning) askForTerminal();
    requestAnimationFrame(() => {
      reportTerminalSize();
      if (focus) focusTerminal(true);
    });
  } else if (terminalFocused || termPane.contains(document.activeElement)) {
    // The keyboard was in the shell that just went away, so it goes back to
    // the code rather than to nowhere.
    focusTerminal(false);
    $(".workspace.on .split.on [data-editor]")?.focus();
  }
}

function toggleTerminal() {
  if (activeMode !== "codex") coreSetMode("codex");
  const on = Boolean(termPane?.hidden);
  showTerminal(on, { focus: on });
}

$$("[data-term-toggle]").forEach((b) => b.addEventListener("click", toggleTerminal));

/** How wide the terminal is, as asked for. The stylesheet caps it at half the
 *  stage, so a narrow window keeps the code readable without forgetting the
 *  width you chose. */
function setTerminalWidth(px, keep = true) {
  if (!termPane) return;
  const width = Math.max(220, Math.min(1400, Math.round(px)));
  termPane.style.setProperty("--term-w", `${width}px`);
  termGutter?.setAttribute("aria-valuenow", String(width));
  if (!keep) return;
  try {
    localStorage.setItem("proton.terminal.width", String(width));
  } catch {
    /* storage blocked — it just won't persist */
  }
}

if (termGutter) {
  termGutter.tabIndex = 0;
  termGutter.setAttribute("role", "separator");
  termGutter.setAttribute("aria-orientation", "vertical");
  termGutter.setAttribute("aria-label", "Resize terminal");
  termGutter.title = "Drag or use arrow keys to resize · double-click or Home to reset";
  const width = () => termPane.getBoundingClientRect().width;
  termGutter.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    termGutter.setPointerCapture(e.pointerId);
    termGutter.classList.add("dragging");
    document.documentElement.classList.add("resizing");
  });
  termGutter.addEventListener("pointermove", (e) => {
    if (!termGutter.classList.contains("dragging")) return;
    setTerminalWidth(termPane.parentElement.getBoundingClientRect().right - e.clientX);
  });
  const stop = () => {
    termGutter.classList.remove("dragging");
    document.documentElement.classList.remove("resizing");
  };
  termGutter.addEventListener("pointerup", stop);
  termGutter.addEventListener("pointercancel", stop);
  termGutter.addEventListener("dblclick", () => setTerminalWidth(TERM_WIDTH));
  // The divider is on the terminal's left edge, so left widens it. Every
  // other key stays with the divider rather than reaching the modal engine,
  // as the split dividers do.
  termGutter.addEventListener("keydown", (e) => {
    if (e.key === "Tab" || e.metaKey || e.ctrlKey || e.altKey) return;
    e.preventDefault();
    e.stopPropagation();
    const step = e.shiftKey ? 50 : 20;
    if (e.key === "ArrowLeft") setTerminalWidth(width() + step);
    else if (e.key === "ArrowRight") setTerminalWidth(width() - step);
    else if (e.key === "Home") setTerminalWidth(TERM_WIDTH);
    else if (e.key === "Escape") $(".workspace.on .split.on [data-editor]")?.focus();
  });
}

/* ── the debugger ─────────────────────────────────────────────────────
   The core owns where it is, what the stack looks like and what is in
   scope. This draws it and sends the buttons back. */

function renderDebug(event) {
  const phase = $("[data-debug-phase]");
  if (phase) {
    phase.textContent = event.phase;
    phase.className = "chip" + (event.phase === "stopped" ? " warn" : event.phase === "running" ? " accent" : "");
  }

  const frames = $("[data-debug-frames]");
  if (frames) {
    frames.innerHTML = event.frames.length
      ? event.frames
          .map(
            (f, i) =>
              `<div class="row${i === 0 ? " sel" : ""}"` +
              // A frame with a path can be jumped to; one in a library cannot.
              `${f.path ? ` data-path="${escapeHtml(f.path)}"` : ""}>` +
              `<span class="ic">${icon("k-function")}</span>${escapeHtml(f.name)}` +
              `<span class="dir" style="margin-left:auto">` +
              `${f.path ? escapeHtml(f.path) + ":" + f.line : "—"}</span></div>`,
          )
          .join("")
      : `<div class="row empty">not stopped</div>`;
  }

  const variables = $("[data-debug-variables]");
  if (variables) {
    variables.innerHTML = event.variables.length
      ? event.variables
          .map(
            (v) =>
              `<div class="row"><span class="ic">${icon(
                v.expandable ? "k-type" : "k-variable",
              )}</span>${escapeHtml(v.name)}` +
              `<span class="dir" style="margin-left:auto">${escapeHtml(v.value)}</span></div>`,
          )
          .join("")
      : `<div class="row empty">nothing in scope</div>`;
  }

  const output = $("[data-debug-output]");
  if (output) {
    output.textContent = event.output.join("\n");
    output.scrollTop = output.scrollHeight;
  }
}

document.addEventListener("click", (e) => {
  const button = e.target.closest("[data-debug]");
  if (!button) return;
  const what = button.dataset.debug;
  dispatch({
    type: { start: "debugStart", stop: "debugStop", resume: "debugResume", stepOver: "debugStepOver" }[what],
  });
});

/* ── the workspace rail ───────────────────────────────────────────────
   One tile per open project. The core decides what they are called — it is
   the only thing that can, since telling two projects called `src` apart
   needs to know about both of them. */

/** Up to two letters, one per part of a name the core had to qualify. */
function initials(name) {
  return name
    .split("/")
    .map((part) => part[0] ?? "")
    .join("")
    .slice(0, 2)
    .toUpperCase();
}

function renderWorkspaces(items, active) {
  const current = items[active];
  $("#sl-project").textContent = current?.name ?? "No workspace";
  $("#sl-project").title = current?.path ?? "";
  projectRoot = current?.path ?? "";
  claude?.setRoot(projectRoot);
  renderBufferBars();
  // Every rail that lists them: the chrome rail's, and Codex's title bar.
  const html = items
    .map(
      (ws, i) =>
        `<button class="ws" role="tab" aria-selected="${i === active}"` +
        ` data-workspace="${i}" title="${escapeHtml(ws.path)}">` +
        `${escapeHtml(initials(ws.name))}` +
        `${ws.modified ? '<span class="mod"></span>' : ""}` +
        `<span class="x" data-workspace-close="${i}" title="Close ${escapeHtml(ws.name)}">` +
        `<svg class="i"><use href="#i-close"/></svg></span></button>`,
    )
    .join("");
  for (const rail of $$("[data-workspaces]")) rail.innerHTML = html;
}

/**
 * Ask the core for the window the pane can currently show.
 *
 * `first_line` is exactly the line at the top of the pane, never an overscan
 * start. It has to mean one thing, because the core sets it too — `G`, a grep
 * hit, the cursor leaving the screen — and the pane positions itself from it
 * either way. Offsetting it here would leave the two disagreeing by the
 * overscan, and the correction would scroll the user back a little on every
 * frame they scrolled. The overscan is extra rows below instead.
 */
const OVERSCAN = 4;

function reportViewport(index = view.focused) {
  const target = $(`.workspace.on [data-editor][data-view="${index}"]`);
  if (!target || target.clientHeight <= 0) return;
  const first = Math.max(0, Math.floor(target.scrollTop / lineHeight));
  const rows = Math.max(1, Math.ceil(target.clientHeight / lineHeight) + OVERSCAN);
  dispatch({ type: "viewport", view: index, firstLine: first, lines: rows });
}

/** Every pane has its own height, and the core has to be told all of them. */
function reportEveryViewport() {
  $$(".workspace.on [data-editor]").forEach((pane) => reportViewport(Number(pane.dataset.view)));
}

const viewportObserver = new ResizeObserver(() => reportEveryViewport());

// Scroll events arrive far faster than frames, and every one of them would
// otherwise be a round trip to the core. One per frame is enough to keep up
// with a trackpad and is the most the screen can show anyway.
const viewportQueued = new Set();

function reportViewportSoon(index) {
  if (viewportQueued.has(index)) return;
  viewportQueued.add(index);
  requestAnimationFrame(() => {
    viewportQueued.delete(index);
    reportViewport(index);
  });
}

document.addEventListener(
  "scroll",
  (e) => {
    // Per pane: two splits scrolling at once must not coalesce into one
    // report, or one of them stops where the other left it.
    if (e.target?.matches?.("[data-editor]")) {
      reportViewportSoon(Number(e.target.dataset.view));
    }
  },
  // Scroll does not bubble, so the listener has to be capturing to see it on
  // a descendant of `document`.
  true,
);

document.addEventListener("click", (e) => {
  const history = e.target.closest("[data-history]");
  if (history && activeMode === "codex") {
    const index = Number(history.closest("[data-view]").dataset.view);
    dispatchMany([{ type: "focusSplit", index }, historyCommand(history.dataset.history === "redo")]);
    return;
  }
  const pick = e.target.closest("[data-pick]");
  if (pick) {
    // Select then accept: the click says which row, and Enter means the same
    // thing however the row got chosen.
    dispatchMany([{ type: "selectPick", index: Number(pick.dataset.pick) }, keyCommand({key: "Enter"})]);
    return;
  }
  const candidate = e.target.closest("[data-complete]");
  if (candidate) {
    // Select then accept, exactly as the picker does: the click says which
    // row, and accepting means the same thing however the row got chosen.
    dispatchMany([{ type: "selectCompletion", index: Number(candidate.dataset.complete) }, { type: "acceptCompletion" }]);
    return;
  }
  // Closing is checked before focusing: the × sits inside the tile, so a
  // click on it matches both and only one of them is what was meant.
  const shut = e.target.closest("[data-workspace-close]");
  if (shut) {
    e.stopPropagation();
    dispatch({ type: "closeWorkspace", index: Number(shut.dataset.workspaceClose) });
    return;
  }
  const project = e.target.closest("[data-workspace]");
  if (project) {
    dispatch({ type: "focusWorkspace", index: Number(project.dataset.workspace) });
    return;
  }
  if (e.target.closest("[data-open-workspace]")) {
    dispatch({ type: "openWorkspacePicker" });
    return;
  }

  // Checked before focusing, because the × sits inside the tab and a click on
  // it matches both — only one of them is what was meant.
  if (e.target.closest("[data-reveal]")) {
    dispatch({ type: "revealInTree" });
    return;
  }

  const row = e.target.closest("[data-path]");
  if (row) dispatch({ type: "openBuffer", path: row.dataset.path });

});

/* ── telescope ────────────────────────────────────────────────────────
   The core owns the picker: it holds the query, does the matching and
   decides what a row opens. This draws the answer and nothing else —
   including the matched character positions, so the highlight can never
   disagree with the score that produced the ordering. */

let pickerOpen = false;
let pickerKind = "files";

$("[data-picker-query]").addEventListener("input", (e) => {
  if (pickerOpen) dispatch({ type: "pickerQuery", text: e.target.value });
});

function highlight(label, positions) {
  if (!positions?.length) return escapeHtml(label);
  const hit = new Set(positions);
  return [...label]
    .map((c, i) => (hit.has(i) ? `<mark>${escapeHtml(c)}</mark>` : escapeHtml(c)))
    .join("");
}

function renderPicker(event) {
  pickerOpen = true;
  pickerKind = event.kind;
  show("telescope");

  const query = $("[data-picker-query]");
  query.setAttribute("aria-label", event.kind === "workspaces" ? "Search folders" : "Search");
  if (query.value !== event.query) query.value = event.query;
  $("[data-picker-title]").textContent = event.title;
  // An empty list while the tree is still being walked means "not yet", not
  // "no matches". Saying so is the difference between waiting a beat and
  // deciding the file isn't there.
  //
  // A list cut short is the same trap from the other side. Over a home folder
  // or anything broader the core lists only so many files, and one it never
  // reached looks exactly like one that isn't there.
  const count = $("[data-picker-count]");
  count.textContent = event.scanning
    ? "reading…"
    : `${event.items.length}/${event.total}${event.cutShort ? " · cut short" : ""} · ${event.elapsedMs.toFixed(1)} ms`;
  count.title = event.cutShort
    ? `This folder has too many files to list them all. Only ${event.total} were listed, and only those are searched; a project opened on its own is listed whole.`
    : "";

  // What the library is narrowed to, said where the question is being typed.
  const narrowed = $("[data-picker-scope]");
  if (narrowed) {
    const on = event.kind === "library" && libraryScope.length > 0;
    narrowed.hidden = !on;
    if (on) {
      narrowed.textContent =
        libraryScope.length === 1 ? libraryScope[0] : `${libraryScope.length} sources`;
      narrowed.title = `Only ${libraryScope.join(", ")} is being searched. Settings › Library changes it.`;
    }
  }

  // A folder prompt lists folders, and a row that looks like a file in a list
  // of projects is a row you have to read twice.
  const leader =
    event.kind === "workspaces" ? '<svg class="i"><use href="#i-folder"/></svg> ' : "";

  const results = $("[data-picker-results]");
  results.innerHTML = event.items
    .map((item, i) => {
      const on = i === event.selected ? " on" : "";
      // A library row is two lines, because its two halves are both prose and
      // both long: the citation says which passage, and the passage itself
      // says whether it is the one you wanted. Squeezing them onto one line
      // the way a file row does would truncate both.
      if (event.kind === "library") {
        const [first = ""] = item.detail.split("\n");
        return (
          `<div class="res lib${on}" data-pick="${i}">` +
          `<span class="cite">${escapeHtml(item.label)}</span>` +
          `<span class="line">${escapeHtml(first)}</span></div>`
        );
      }
      // A symbol row says what kind of thing it is, using the marks the
      // completion popup already teaches — a second alphabet for the same
      // nine categories would be one to learn twice.
      const mark = item.kind
        ? `<span class="k">${icon(KIND_ICON[item.kind] ?? "k-other")}</span>`
        : leader;
      return (
        `<div class="res${on}" data-pick="${i}">` +
        `<span>${mark}${highlight(item.label, item.positions)}</span>` +
        `<span class="dir" style="margin-left:auto">${escapeHtml(item.detail)}</span></div>`
      );
    })
    .join("");
  if (!event.items.length) results.innerHTML = `<div class="empty">${event.scanning
    ? "Reading files…" : event.cutShort ? `No matches in the ${event.total} files listed. The rest of this folder was not searched.`
    : "No matches. Try a different search."}</div>`;

  results.querySelector(".res.on")?.scrollIntoView({ block: "nearest" });
  previewPassage(event);
}

/** The selected passage, in the pane beside the list.
 *
 *  Only the library fills this. The other pickers leave the pane's standing
 *  contents alone — a passage is several lines and worth reading before you
 *  open it, which is not true of a filename. */
function previewPassage(event) {
  const pane = $("#telescope .preview");
  if (!pane) return;
  if (event.kind !== "library") {
    const item = event.items[event.selected];
    pane.innerHTML = item
      ? `<div class="pv-cite">${escapeHtml(item.label)}</div>` +
        `<div class="pv-passage">${escapeHtml(item.detail || item.path || "")}</div>` +
        `<div class="pv-empty">Enter to open · ↑ ↓ to choose · Esc to close</div>`
      : `<div class="pv-empty">${event.scanning ? "Reading files…" : "No item selected"}</div>`;
    return;
  }

  const item = event.items[event.selected];
  if (!item) {
    pane.innerHTML =
      `<div class="pv-head">${event.query.trim() ? "nothing found" : "your library"}</div>` +
      `<div class="pv-empty">${
        event.query.trim()
          ? "No passage in your notes matches those words. Matching is exact, so try the words the note would use."
          : "Ask a question. Results are passages from the roots listed in Settings › Library, each citing where it came from."
      }</div>`;
    return;
  }
  // Its own class rather than `pv-head`: that one uppercases and tracks out
  // its text, which is right for a label and wrong for a citation — a file
  // path in capitals is a file path you have to read twice.
  pane.innerHTML =
    `<div class="pv-cite">${escapeHtml(item.label)}</div>` +
    `<div class="pv-passage">${escapeHtml(item.detail)}</div>`;
}

function closePicker() {
  pickerOpen = false;
  closeAll();
}

/* ── language servers ─────────────────────────────────────────────────
   Diagnostics, completion and server status. All three arrive without
   being asked for — the shell's poll thread pushes them — so they are
   rendered the same way as everything else and only differ in what woke
   them up. */

function renderDiagnostics(event) {
  marks.byLine = new Map();
  marks.errors = event.errors;
  marks.warnings = event.warnings;
  for (const d of event.items) {
    // A multi-line diagnostic is registered on every line it touches, so the
    // sign column marks all of them rather than just where it started.
    for (let line = d.line; line <= d.endLine; line++) {
      const at = marks.byLine.get(line) ?? [];
      at.push(d);
      marks.byLine.set(line, at);
    }
  }
  // Most serious first, so the sign column shows the worst thing on the line.
  const RANK = { error: 3, warning: 2, info: 1, hint: 0 };
  for (const at of marks.byLine.values()) {
    at.sort((a, b) => RANK[b.severity] - RANK[a.severity]);
  }

  const el = $("#sl-diagnostics");
  if (el) {
    el.innerHTML =
      event.errors || event.warnings
        ? `<span class="e">${icon("d-error", "solid")} ${event.errors}</span>` +
          `<span class="w">${icon("d-warning", "solid")} ${event.warnings}</span>`
        : "";
  }
  renderEditor();
}

function renderServers(states) {
  const el = $("#sl-servers");
  if (!el) return;
  el.innerHTML = states
    .map((s) => {
      // A server that is busy says what it is busy with. Half a minute of
      // silence while rust-analyzer indexes is indistinguishable from a
      // broken editor, and that is the impression people keep.
      if (s.status === "busy") {
        return `<span class="busy">${icon("srv-busy", "spin")} ${escapeHtml(s.note ?? s.language)}</span>`;
      }
      if (s.status === "starting") {
        return `<span class="busy">${icon("srv-busy", "spin")} ${escapeHtml(s.language)}</span>`;
      }
      if (s.status === "ready") return `<span>${icon("srv-ready", "solid")} ${escapeHtml(s.language)}</span>`;
      // Not having gopls installed is a normal state, not a fault, so it is
      // dimmed rather than flagged.
      return `<span class="off">${icon("srv-off")} ${escapeHtml(s.language)}</span>`;
    })
    .join(" ");
}

/* Twelve LSP categories collapsed onto nine marks. Drawn at 11px in a
   list the eye scans vertically, which rules out pictograms — these are
   category marks, learned once, the way a diamond comes to mean keyword. */
const KIND_ICON = {
  function: "k-function", method: "k-method", field: "k-field",
  variable: "k-variable", type: "k-type", module: "k-module",
  keyword: "k-keyword", constant: "k-constant", snippet: "scissors",
  other: "k-other",
};

function renderCompletion(event) {
  const popup = $("#completion");
  const list = $("[data-completion-list]");
  if (!popup || !list) return;

  list.innerHTML = event.items
    .map(
      (item, i) =>
        `<div class="cmp-row${i === event.selected ? " on" : ""}" data-complete="${i}">` +
        `<span class="k">${icon(KIND_ICON[item.kind] ?? "k-other")}</span>` +
        `<span class="l">${escapeHtml(item.label)}</span>` +
        (item.hint ? `<span class="h">${escapeHtml(item.hint)}</span>` : "") +
        `</div>`,
    )
    .join("");

  popup.hidden = false;
  completionOpen = true;
  anchorCompletion(popup);
  list.querySelector(".cmp-row.on")?.scrollIntoView({ block: "nearest" });
}

/** Put the popup under the caret, and above it when there is no room below. */
function anchorCompletion(popup) {
  const caret = $("[data-editor] .caret");
  if (!caret) return;
  const at = caret.getBoundingClientRect();
  const box = popup.getBoundingClientRect();
  const gap = 6;

  const below = at.bottom + gap;
  const fits = below + box.height <= window.innerHeight - 8;
  popup.style.top = `${fits ? below : Math.max(8, at.top - gap - box.height)}px`;
  // Nudged left so a popup near the right edge does not hang off it.
  popup.style.left = `${Math.min(at.left, window.innerWidth - box.width - 8)}px`;
}

function closeCompletion() {
  completionOpen = false;
  const popup = $("#completion");
  if (popup) popup.hidden = true;
}

/* `K`. The core sends a panel only when there is something in it, and takes
   it away on the next action, so this end has no state beyond a flag for
   whether Escape should be spent dismissing it. */
let hoverOpen = false;
let hoverAt = null;

function renderHover(event) {
  const panel = $("#hover");
  const body = $("[data-hover-text]");
  if (!panel || !body) return;

  // Plain text with its fences already off. `textContent` rather than markup
  // both because there is no markup left to honour and because a signature
  // full of generics is a page of angle brackets to escape.
  body.textContent = event.text;
  panel.hidden = false;
  hoverOpen = true;
  hoverAt = event;
  anchorHover(panel, event);
}

/** Above the caret by preference — the code you are reading is below it. */
function anchorHover(panel, event) {
  const caret = $("[data-editor] .caret");
  // No caret means the pane is drawing something else entirely; the event
  // says which line it was about, so the row still gives somewhere to point.
  const row = caret ?? rowFor(event.line);
  if (!row) return;

  const at = row.getBoundingClientRect();
  const box = panel.getBoundingClientRect();
  const gap = 6;

  const above = at.top - gap - box.height;
  panel.style.top = `${above >= 8 ? above : Math.min(at.bottom + gap, window.innerHeight - box.height - 8)}px`;
  panel.style.left = `${Math.max(8, Math.min(at.left, window.innerWidth - box.width - 8))}px`;
}

/** The line element carrying a buffer line number, if it is on screen. */
function rowFor(line) {
  return (
    [...document.querySelectorAll("[data-editor] .ln")].find(
      (el) => Number(el.querySelector(".n")?.textContent) === line + 1,
    ) ?? null
  );
}

function closeHover() {
  hoverOpen = false;
  hoverAt = null;
  const panel = $("#hover");
  if (panel) panel.hidden = true;
}

/* ── which-key ────────────────────────────────────────────────────────
   What each leader key does, as the core resolves it: the pickers in
   picker.rs, the debug, lsp, window and terminal families in workspace.rs,
   and the project keys session.rs claims. Mirrored here only to be shown —
   a key pressed still goes to the core, which is the authority on what it
   means, so a row here that drifted would be a wrong label rather than a
   wrong action. Every row is a binding that does something. */
const LEADER = {
  "": [
    ["f", "+find"], ["p", "+project · plugins"], ["g", "+git"], ["l", "+lsp"],
    ["d", "+debug"], ["w", "+window"], ["c", "+code"], ["t", "+terminal"],
    ["e", "reveal this file in the tree"],
  ],
  f: [["f", "files"], ["g", "live grep"], ["b", "open buffers"], ["p", "projects — open a folder"], ["l", "your library"]],
  p: [["n", "next project"], ["p", "previous project"], ["c", "plugin commands"]],
  g: [["g", "changed files"]],
  l: [["s", "symbols"], ["r", "rename"], ["a", "code actions"], ["f", "format"]],
  d: [["b", "toggle breakpoint"], ["r", "run"], ["c", "continue"], ["n", "step over"], ["i", "step into"], ["o", "step out"], ["q", "stop"]],
  w: [["v", "split right"], ["s", "split down"], ["h j k l", "go to a split"], ["w", "next split"], ["o", "only this split"], ["q", "close this split"]],
  c: [["t", "trace in the Code Visualizer"]],
  t: [["t", "open the terminal"]],
};

function renderWhichKey(prefix) {
  const head = $("[data-wk-head]");
  const grid = $("[data-wk-grid]");
  if (!head || !grid) return;
  const rows = LEADER[prefix];
  const keys = ["␣", ...prefix].map((k) => escapeHtml(k)).join(" ");
  const name = prefix ? LEADER[""].find(([k]) => k === prefix)?.[1] ?? "" : "— leader";
  head.innerHTML =
    `<span style="color:var(--accent)">${keys}</span> <span style="color:var(--ink-4)">${escapeHtml(name)}</span>`;
  grid.innerHTML = rows
    ? rows
        .map(([key, label]) =>
          `<div class="wk"><b>${escapeHtml(key)}</b><span class="arrow">→</span>` +
          `<span${label.startsWith("+") ? ' class="grp"' : ""}>${escapeHtml(label)}</span></div>`)
        .join("")
    : `<div class="wk"><span>nothing is bound under ${escapeHtml(keys)} — ⎋ to cancel</span></div>`;
}

/* ── the message line ─────────────────────────────────────────────────
   Codex has no statusline, so what one would have said in passing — `:pwd`,
   a write, a command nobody knows — appears for a moment at the foot of the
   code instead, where vim says it. Refusals and errors are not routed here:
   they have the banner, which stays until it is dismissed. */
const messageLine = document.createElement("div");
messageLine.className = "msgline";
messageLine.setAttribute("role", "status");
messageLine.setAttribute("aria-live", "polite");
messageLine.hidden = true;
$(".stage-code")?.append(messageLine);
let messageTimer;

function flashMessage(text, level = "info") {
  if (activeMode !== "codex" || !text) return;
  messageLine.textContent = text;
  messageLine.dataset.level = level;
  messageLine.hidden = false;
  clearTimeout(messageTimer);
  messageTimer = setTimeout(() => { messageLine.hidden = true; }, 4000);
}

/* ── rendering core events ────────────────────────────────────────────── */

function renderVim(label) {
  view.vim = label;
  const el = $("#sl-mode");
  el.className = "sl sl-mode " + (label === "NORMAL" ? "" : label.toLowerCase().replace("-", ""));
  el.textContent = label;

  // The caret is a block in every mode but insert, where you sit *between*
  // characters rather than on one. That difference is the clearest signal of
  // which mode you are in, and it belongs on the text rather than only in the
  // statusline nobody is looking at while they type.
  $$("[data-editor]").forEach((pane) => pane.classList.toggle("insert", label === "INSERT"));
}

/**
 * The statusline message.
 *
 * `level` is carried through because a refusal has to look like one. `:w`
 * declining to overwrite a file something else rewrote is the same shape of
 * message as `:w` succeeding, and if the two read identically the guard may as
 * well not be there.
 */
function renderStatus(text, level = "info") {
  const el = $("#sl-status");
  if (!el) return;
  el.textContent = text ?? "";
  el.classList.toggle("warn", level === "warn");
  el.classList.toggle("err", level === "error");
}

onEvents((events) => {
  editorPaints.begin();
  try { events.forEach(renderEvent); }
  finally {
    editorPaints.flush();
    if (restorePaneFocus && !overlays.active) {
      const target = splits()?.querySelector(`.split.on [data-editor], .split.on [data-cells]`);
      target?.focus();
    }
    restorePaneFocus = false;
  }
  // `␣tt` and `:terminal` answer with the screen in the same batch that puts
  // the leader or the command line away; a shell printing never does. That
  // is the one way to tell "show me the terminal" from "the terminal
  // changed", and only the first should open a pane somebody shut. Moving to
  // another project — `␣pn`, `:cd` — sends that project's screen the same
  // way, and says so with the project list, which a terminal key never does.
  if (termPane?.hidden && events.some((e) => e.type === "terminal" && e.running) &&
      !events.some((e) => e.type === "workspaces") &&
      events.some((e) => (e.type === "leader" && e.prefix === null) || e.type === "vimMode")) {
    showTerminal(true, { focus: true });
  }
});

/** @param {import('./protocol').WireEvent} event */
function renderEvent(event) {
  switch (event.type) {
    case "state":
      renderMode(event.mode);
      renderVim(event.vimLabel);
      break;

    case "modeChanged":
      renderMode(event.mode);
      break;

    case "vimMode":
      renderVim(event.label);
      // Leaving the command line has to take the scrim with it. Removing only
      // the panel's own class left the dimming layer over the whole window
      // after every `:` command that ran.
      if (event.mode !== "Command" && !pickerOpen) closeAll();
      renderStatus("");
      break;

    case "pending":
      renderStatus(event.keys);
      if (!event.keys) closeAll();
      break;

    case "leader":
      if (event.prefix === null) {
        closeAll();
      } else {
        renderWhichKey(event.prefix);
        show("whichkey");
        // ␣ff is the one leader sequence M0 resolves, so Telescope is reachable
        // the way it will be in M2.
        if (event.prefix === "ff") show("telescope");
      }
      break;

    case "cmdline":
      if (event.text === null) {
        closeAll();
      } else {
        show("cmdline");
        const el = $("#cmdline-text");
        if (el) el.textContent = event.text;
        // `:` and `/` share the line, and the character is the only thing
        // telling you which one you are typing into.
        const prompt = $("#cmdline-prompt");
        if (prompt) prompt.textContent = event.prompt;
      }
      break;

    case "cmdlineRun": {
      // The core hands back the line it collected; the front end decides what
      // Only what the core did not recognise arrives here. It owns `:w`,
      // `:e`, `:q` and the splits now, so what is left is genuinely unknown —
      // and saying so beats swallowing a typo.
      if (event.text.trim()) {
        renderStatus(`:${event.text.trim()} — not a command yet`, "warn");
        flashMessage(`:${event.text.trim()} — not a command`, "warn");
      }
      break;
    }

    case "action":
      renderStatus(event.description);
      // `i`, `a`, `o` and `O` in Data mode put the caret in a cell. The core
      // has already switched the engine to insert and, for `o`/`O`, added the
      // cell; this is the half it cannot do.
      if (activeMode === "data" && event.action.verb === "Insert") {
        requestAnimationFrame(() => enterCell(event.action.object));
      }
      break;

    case "unsupported":
      // The mode has no meaning for this verb, and says so. Silently doing
      // nothing is the failure users cannot debug.
      renderStatus(`${event.description} — not in ${event.mode}`);
      break;

    case "input":
      renderStatus(`insert ${JSON.stringify(event.text)}`);
      break;

    case "viewport":
    case "cursor": {
      editorState.update(event);
      renderEditor(event.view);
      if (event.view === view.focused) renderPosition();
      break;
    }

    case "splits":
      renderSplits(event.views, event.focused, event.vertical);
      break;

    case "buffers": {
      renderBuffers(event.names, event.active);
      const active = event.names[event.active];
      const path = $("#sl-path");
      if (path && active) path.textContent = active.path ?? active.name;
      if (path) path.title = active?.path ?? "";
      const extension = active?.name.match(/\.([^.]+)$/)?.[1];
      $("#sl-filetype").textContent = extension ?? "";
      break;
    }

    case "tree":
      renderTree(event.rows);
      break;

    case "repo":
      renderRepo(event);
      break;

    case "notebook":
      renderNotebook(event);
      break;

    case "claudeSessions":
      claude?.sessions(event);
      break;

    case "claudeItems":
      claude?.items(event);
      break;

    case "claudeStatus":
      claude?.status(event.status);
      break;

    case "claudeCapabilities":
      claude?.capabilities(event.capabilities);
      study?.capabilities(event.capabilities);
      break;

    case "artifacts":
      claude?.artifacts(event);
      break;

    case "researchList":
      research?.list(event);
      break;

    case "researchBoard":
      research?.board(event);
      lean?.discipline(event.discipline);
      break;

    case "researchHypothesis":
      research?.hypothesis(event);
      break;

    case "researchRound":
      research?.round(event);
      break;

    case "researchTranscript":
      research?.transcript(event);
      break;

    case "researchLiterature":
      research?.literature(event);
      break;

    case "researchPaper":
      research?.paper(event);
      break;

    case "researchResults":
      research?.results(event);
      break;

    case "labEnv":
      lab?.env(event);
      break;

    case "studyList":
      study?.list(event);
      break;

    case "studyNotebook":
      study?.notebook(event);
      break;

    case "studyClosed":
      study?.closed();
      break;

    case "studyKb":
      study?.kb(event);
      break;

    case "studyKbFolder":
      study?.kbFolder(event);
      break;

    case "studyKbResults":
      study?.kbResults(event);
      break;

    case "studySource":
      study?.source(event);
      break;

    case "studyNote":
      study?.note(event);
      break;

    case "studyChat":
      study?.chat(event);
      break;

    case "studyChatLive":
      study?.chatLive(event);
      break;

    case "studyStudio":
      study?.studio(event);
      break;

    case "studyMadeOpen":
      study?.made(event);
      break;

    case "studyTask":
      study?.task(event);
      break;

    case "leanEnv":
      lean?.env(event);
      break;

    case "researchLab":
      lab?.experiments(event);
      break;

    case "researchRun":
      lab?.run(event);
      break;

    case "chats":
      renderChats(event);
      break;

    case "settings":
      renderSettings(event);
      break;

    case "library":
      renderLibrary(event);
      break;

    case "council":
      renderCouncil(event);
      break;

    case "cut":
      lastCut = event;
      renderCutBar(event);
      renderMediaBin(event);
      renderTimeline(event);
      renderCutInspector(event);
      renderCaptions(event);
      renderCutSource(event);
      break;

    case "level":
      lastLevel = event;
      renderHierarchy(event);
      renderLevelInspector(event);
      renderLevelSource(event);
      break;

    case "geometry":
      // One mesh, described — asked for when a scene names it, and sent again
      // by itself when the file is re-exported or its stack changes. The mesh
      // follows as bytes; see `receiveGeometry`.
      receiveGeometry(event);
      meshProblems.set(event.path, event.problem ?? "");
      if (lastScene) renderNodeInspector(lastScene);
      break;

    case "scene":
      // A different scene means different files behind the same relative
      // paths. Everything held for the last one has to go.
      if (lastScene && lastScene.path !== event.path) clearGeometry();
      lastScene = event;
      renderOutliner(event);
      renderNodeInspector(event);
      renderViewport(event);
      renderGraph(event);
      break;

    case "artboard":
      lastArtboard = event;
      renderLayers(event);
      renderLayerInspector(event);
      // Drawn from the theme event, which carries the palette. Nothing to
      // paint on until one has arrived; the next one will pick this up.
      if (lastTheme) renderArtboard(lastTheme);
      break;

    case "theme":
      renderTheme(event);
      renderArtboard(event);
      renderPalettes(event);
      break;

    case "layout":
      renderLayout(event.layout);
      break;

    case "document":
      openDocument(event);
      break;

    case "debug":
      renderDebug(event);
      break;

    case "terminal":
      renderTerminal(event);
      // The shell has to be told how big the panel is, and only the panel
      // knows once it has a row to measure — but only when it is not that
      // size already. A screen with a different number of rows is a shell
      // that was never told: just started, or another project's.
      if (event.rows.length && event.rows.length !== termSize.rows) {
        requestAnimationFrame(() => reportTerminalSize(true));
      }
      break;

    case "workspaces":
      renderWorkspaces(event.items, event.active);
      // Every project has its own shell, and a size told to one was not told
      // to the others.
      termSize = { rows: 0, cols: 0 };
      requestAnimationFrame(() => reportTerminalSize());
      break;

    case "picker":
      renderPicker(event);
      break;

    case "pickerClosed":
      closePicker();
      break;

    case "diagnostics":
      renderDiagnostics(event);
      break;

    case "completion":
      renderCompletion(event);
      break;

    case "hover":
      renderHover(event);
      break;

    case "hoverClosed":
      closeHover();
      break;

    case "completionClosed":
      closeCompletion();
      break;

    case "servers":
      renderServers(event.states);
      break;

    case "agent":
      renderAgent(event);
      break;

    case "review":
      renderReview(event);
      break;

    case "tasks":
      renderTasks(event);
      break;

    case "plugins":
      renderPlugins(event);
      break;

    case "trace":
      renderTrace(event);
      break;

    case "notice":
      editorFeedback.notice(event);
      renderStatus(event.text, event.level);
      if (event.level === "info") flashMessage(event.text);
      break;
  }
}

function renderPosition() {
  const el = $("#sl-position");
  const at = focusedPane().cursor;
  if (el) el.textContent = `ln ${at.line + 1}, col ${at.column + 1}`;
  const total = focusedPane().totalLines;
  $("#sl-progress").textContent = total ? `${Math.min(100, Math.round((at.line + 1) / total * 100))}%` : "";
}

/* ── the caret's blink ────────────────────────────────────────────────
   On a timer, as Zed's is, rather than as a CSS animation. Animating a colour
   means styling the page again on every frame the animation runs — sixty
   times a second, for as long as an editor was open, which made an idle
   window the busiest thing on the machine's main thread. This changes it
   twice a second, holds it solid while you type, and stops altogether while
   the window is in the background or motion is turned down. */
const BLINK_MS = 530;
const reducedMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)");
let blinkTimer = 0;
let blinkOff = false;

function showBlink(off) {
  blinkOff = off;
  for (const caret of document.querySelectorAll("[data-editor] .caret")) {
    caret.classList.toggle("off", off);
  }
}

/** Solid now, and blinking again from here — the moment anything happens. */
function restartBlink() {
  clearInterval(blinkTimer);
  blinkTimer = 0;
  if (blinkOff) showBlink(false);
  if (document.hidden || !document.hasFocus() || reducedMotion?.matches) return;
  blinkTimer = setInterval(() => showBlink(!blinkOff), BLINK_MS);
}

document.addEventListener("keydown", restartBlink, true);
document.addEventListener("mousedown", restartBlink, true);
document.addEventListener("visibilitychange", restartBlink);
window.addEventListener("focus", restartBlink);
window.addEventListener("blur", restartBlink);
reducedMotion?.addEventListener?.("change", restartBlink);
restartBlink();

/* ── who has the keyboard ─────────────────────────────────────────────
   The code, unless something else has asked for it: the project panel, the
   shell, a field being typed in. The caret says which — solid while keys go
   to the code, hollow and still while they go elsewhere, as Zed draws an
   editor without the focus — because a key that lands somewhere unseen reads
   as a key that did nothing. */
function renderKeyboardOwner() {
  const elsewhere = terminalFocused || Boolean(document.activeElement?.closest?.(
    "[data-tree], input, textarea, select, [contenteditable]:not([contenteditable=false])",
  ));
  document.documentElement.dataset.keys = elsewhere ? "elsewhere" : "code";
}

// Out of one element and into the next is two events, and between them the
// focus is on the page itself — which is "the code" — so both are heard.
document.addEventListener("focusin", renderKeyboardOwner);
document.addEventListener("focusout", renderKeyboardOwner);

// The shell lets go of the keyboard when the focus goes anywhere outside its
// pane — not only when something else is clicked. A file chosen from ⌘K puts
// the focus back in the code without a click, and the keys went on to the
// shell, Tab and all.
document.addEventListener("focusin", (e) => {
  if (terminalFocused && !(e.target instanceof Element && e.target.closest("[data-term-pane]"))) {
    focusTerminal(false);
  }
});

/* ── keys ─────────────────────────────────────────────────────────────
   Two layers, and the split matters. Shell chords (⌘K, ⌘,, ⌃1-9) are the
   window's own bindings and never reach the core. Everything else is a
   keystroke the core interprets — including the digits, which are vim
   counts, not mode switches. */
document.addEventListener("keydown", (e) => {
  // Modal controls own input before either the terminal or the Vim engine.
  if (e.defaultPrevented) return;
  if (overlays.active === "settings") {
    if (e.key === "Escape" || (e.metaKey && e.key === ",")) {
      e.preventDefault();
      dismissOverlay();
    } else overlays.tab(e);
    return;
  }
  if (pickerOpen) {
    if (e.key === "Tab" && pickerKind === "workspaces") {
      e.preventDefault();
      sendKey(e);
      return;
    }
    if (overlays.tab(e)) return;
    if (e.key === "Escape" || (e.metaKey && e.key.toLowerCase() === "k")) {
      e.preventDefault();
      dismissOverlay();
    } else if (e.metaKey && e.key === ",") {
      e.preventDefault();
      open("settings");
    } else if (e.target?.matches?.("[data-picker-query]") &&
      !["Enter", "ArrowUp", "ArrowDown"].includes(e.key) &&
      !(e.ctrlKey && ["n", "p"].includes(e.key))) {
      // Native text editing supplies the entire query, including paste/IME.
      return;
    } else if (!e.metaKey) {
      e.preventDefault();
      sendKey(e);
    }
    return;
  }
  if (e.target?.closest?.("[data-tree]") && !e.metaKey && !e.ctrlKey) return;
  if (e.target?.matches?.("[data-resize]") && !e.metaKey && !e.ctrlKey) return;
  // The terminal takes the whole keyboard while it has the focus, exactly as
  // the picker does — `d` types a `d` at the shell rather than starting an
  // operator, and `⌃c` interrupts a program rather than doing nothing.
  if (terminalFocused) {
    // An input method is composing in the hidden field; what it makes
    // arrives there as text, not as these keys.
    if (e.isComposing || e.key === "Dead" || e.key === "Process") return;
    // ⌘V (⌃⇧V elsewhere) pastes, and the paste lands in whatever has the
    // focus — so the hidden field takes it first, in case a mouse selection
    // on the screen had kept it there. The paste handler does the rest.
    const mac = /Mac/.test(navigator.platform);
    if (e.key.toLowerCase() === "v" && (mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && e.shiftKey)) {
      $("[data-term-input]")?.focus({ preventScroll: true });
      return;
    }
    // Shift+PageUp/PageDown look through the history, which is the pairing
    // every terminal uses — the unshifted keys belong to whatever is running
    // in the shell, and taking those would break a pager.
    if (e.shiftKey && (e.key === "PageUp" || e.key === "PageDown")) {
      e.preventDefault();
      const page = Math.max(1, terminalRows() - 1);
      dispatch({ type: "terminalScroll", lines: e.key === "PageUp" ? page : -page });
      return;
    }
    const bytes = terminalBytes(e);
    if (bytes !== null) {
      e.preventDefault();
      dispatch({ type: "terminalInput", text: bytes });
      return;
    }
  }

  // ⌃1-9 and ⌘K/⌘, are deliberately two different modifiers, not
  // interchangeable ones — aliasing them here used to let ⌃K and ⌃,
  // trigger the shell chords too, which stole them from the modal engine
  // below (⌃-prefixed commands are exactly what a Vim keymap is made of,
  // and on macOS ⌃K in particular already means "kill to end of line"
  // everywhere else on the system).
  if (e.ctrlKey && /^[1-9]$/.test(e.key)) {
    e.preventDefault();
    coreSetMode(MODES[Number(e.key) - 1]);
    return;
  }

  // ⌃` is Zed's terminal toggle. It means nothing to a Vim keymap, and it has
  // to work from inside the shell too, or the pane could not be put away from
  // where the keyboard usually is when you want to.
  if (e.ctrlKey && !e.metaKey && !e.altKey && e.code === "Backquote") {
    e.preventDefault();
    toggleTerminal();
    return;
  }

  if (e.metaKey && e.key.toLowerCase() === "k") {
    e.preventDefault();
    open("telescope");
    return;
  }

  // ⌘O opens a folder, as it does in Zed — the same picker as ␣fp and a bare
  // :cd, which takes a project's name or a path, with Tab to complete it.
  if (e.metaKey && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "o") {
    e.preventDefault();
    closeAll();
    dispatch({ type: "openWorkspacePicker" });
    return;
  }

  if (e.metaKey && e.key === ",") {
    e.preventDefault();
    open("settings");
    return;
  }

  // A document on screen takes the keyboard, as the picker does: its own keys
  // scroll, page and zoom it and Escape or q puts it away, and nothing else
  // reaches the buffer or the cell underneath — `dd` over a PDF must not
  // delete a line nobody can see. ⌘ chords, ⌃1–9 and a focused shell are
  // answered above, and the text it lays over each page can still be copied.
  if (documentShown() && !terminalFocused && !e.metaKey) {
    e.preventDefault();
    documentKey(e);
    return;
  }

  // ⌃Space asks for completions outright. ⌃n and ⌃p walk the list, and are
  // sent through as keys because the core is what knows whether a popup is
  // open — in normal mode those keys mean something else entirely.
  if (e.ctrlKey && e.key === " ") {
    e.preventDefault();
    dispatch({ type: "complete" });
    return;
  }
  if (e.ctrlKey && completionOpen && (e.key === "n" || e.key === "p")) {
    e.preventDefault();
    sendKey(e);
    return;
  }

  if (activeMode === "codex" && !terminalFocused && !overlays.active && !documentIsOpen()
      && historyShortcut(e, dispatch, /Mac/.test(navigator.platform))) return;

  if (e.metaKey) return; // leave the rest of the browser/OS chords alone

  // Somewhere you can actually type keeps its own keys.
  //
  // Everything below assumes the keyboard belongs to the modal engine, and
  // presses `preventDefault` on anything that would otherwise reach the page.
  // That is right for the editor, whose text is drawn rather than edited in
  // place — and wrong for a notebook cell, which is a real editable element:
  // it made every character vanish, and made ⇧⏎ unreachable because the cell
  // never held the caret long enough to be typed in.
  //
  // The picker is checked above and returns before this, so a picker that
  // owns the keyboard still does.
  const typing = e.target?.closest?.(
    "input, textarea, select, [contenteditable]:not([contenteditable=false])",
  );
  if (typing) return;
  // Native buttons retain Tab, Space and Enter even in normal mode.
  if (["Tab", "Enter", " "].includes(e.key) && e.target?.closest?.("button, a[href], [role=tab]")) return;

  // The popup does not own the keyboard — you are still in insert mode — but
  // the few keys it does claim must not also do what the browser thinks they
  // do. Tab moving focus out of the editor mid-completion is the worst of
  // them; the window never comes back on its own.
  if (completionOpen && ["Tab", "Enter", "ArrowUp", "ArrowDown"].includes(e.key)) {
    e.preventDefault();
    sendKey(e);
    return;
  }

  // Overlays are view state, so Escape closes them here as well as unwinding
  // the modal state in the core.
  if (e.key === "Escape") closeAll();
  // The hover panel is not an overlay and Escape in normal mode resolves to
  // nothing, so the core would never hear that it should go away.
  if (e.key === "Escape" && hoverOpen) dispatch({ type: "closeHover" });

  // Tab is the code's, as it is in every editor: an indent in insert mode,
  // and nothing the browser does with it. Left to the browser it moved the
  // focus to the next control, so a Tab typed into a function walked off
  // into the panes. A control that has the focus — a button, a field —
  // keeps its own Tab above; so does the terminal, which completes with it.

  // Nothing that moves the cursor may also move the page.
  //
  // The single-character test above misses every key whose name is a word,
  // which is all of the ones the browser scrolls with — so the core moved the
  // cursor down a line *and* the browser scrolled, which reads as the page
  // sliding away underneath you on every press of `↓`.
  if (e.key === " " || e.key.length === 1 || SCROLLS_THE_PAGE.has(e.key)) {
    e.preventDefault();
  }

  sendKey(e);
});

/// Keys the browser scrolls with. Backspace is here for a different reason —
/// in some browsers it is still "back" — but it belongs to the buffer either
/// way.
const SCROLLS_THE_PAGE = new Set([
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "PageUp",
  "PageDown",
  "Home",
  "End",
  "Backspace",
  "Tab",
]);

/* ── Code Visualizer: a pointer is an edge, never an inline expansion ──
   Every pointer slot carries data-ptr; every heap block carries data-addr.
   The edge is measured from the live boxes, so it survives any relayout. */
function drawPointerEdges() {
  const canvas = $("#cv-canvas");
  const svg = $("#cv-edges");
  if (!canvas || !svg) return;
  if (!canvas.offsetParent) return; // dock panel is hidden — nothing to measure

  [...svg.querySelectorAll("path")].forEach((p) => p.remove());
  const origin = canvas.getBoundingClientRect();

  /* Where the arrows come from.
   *
   * A live trace supplies them already resolved: the core matched each
   * pointer's target address to an object — exact first, then the smallest
   * range containing it — and that rule is the whole design of the format,
   * so it is not re-implemented here. The static mock has no edge list, and
   * falls back to matching `data-ptr` against `data-addr`. */
  const pairs = trace.view
    ? trace.view.edges
        .filter((edge) => edge.kind === "points" || edge.kind === "dangling")
        .map((edge) => ({
          slot: $(`[data-node="${CSS.escape(edge.from)}"]`, canvas),
          target: $(`[data-node="${CSS.escape(edge.to)}"]`, canvas),
          dangling: edge.kind === "dangling",
        }))
    : $$("[data-ptr]", canvas).map((slot) => ({
        slot,
        target: $(`[data-addr="${slot.dataset.ptr}"]`, canvas),
        dangling: slot.classList.contains("dangling"),
      }));

  pairs.forEach(({ slot, target, dangling }) => {
    if (!slot) return;

    /* A pointer into freed memory has nothing drawn at the other end —
     * that is the whole fact. Skipping it would delete the evidence: the
     * arrow simply would not be there, and an arrow that is not there
     * looks like a pointer that was never followed rather than one that
     * points at memory which is gone. So it gets a stub that visibly
     * stops in open space. */
    if (!target) {
      if (!dangling) return;
      const a = slot.getBoundingClientRect();
      const dx = canvas.scrollLeft - origin.left;
      const dy = canvas.scrollTop - origin.top;
      const x1 = a.right + dx;
      const y1 = a.top + a.height / 2 + dy;

      const stub = document.createElementNS("http://www.w3.org/2000/svg", "path");
      stub.setAttribute("d", `M${x1} ${y1} L${x1 + 34} ${y1}`);
      stub.setAttribute("fill", "none");
      stub.setAttribute("stroke", token("--err"));
      stub.setAttribute("stroke-width", "1.6");
      stub.setAttribute("stroke-dasharray", "4 3");
      svg.appendChild(stub);

      // A cross where the object would have been.
      const gone = document.createElementNS("http://www.w3.org/2000/svg", "path");
      gone.setAttribute("d", `M${x1 + 38} ${y1 - 4} l8 8 M${x1 + 46} ${y1 - 4} l-8 8`);
      gone.setAttribute("fill", "none");
      gone.setAttribute("stroke", token("--err"));
      gone.setAttribute("stroke-width", "1.6");
      gone.setAttribute("stroke-linecap", "round");
      svg.appendChild(gone);
      return;
    }

    const a = slot.getBoundingClientRect();
    const b = target.getBoundingClientRect();
    const dx = canvas.scrollLeft - origin.left;
    const dy = canvas.scrollTop - origin.top;
    const y1 = a.top + a.height / 2 + dy;
    const xTo = b.left + dx - 5;
    const yTo = b.top + 11 + dy;

    // Every arrow lands on the target's left edge. A pointer that already sits
    // inside the heap column — the next link in a chain — leaves to the *left*
    // and drops down the routing lane, so it never crosses the block it points
    // at. A pointer from the stack simply reaches across.
    const sameColumn = a.left + dx > xTo;
    let d;
    if (sameColumn) {
      const lane = xTo - 15;
      const x1 = a.left + dx - 5;
      d = `M${x1} ${y1} C ${lane} ${y1}, ${lane} ${yTo}, ${xTo} ${yTo}`;
    } else {
      const x1 = a.right + dx;
      const bend = (xTo - x1) * 0.55;
      d = `M${x1} ${y1} C ${x1 + bend} ${y1}, ${xTo - bend} ${yTo}, ${xTo} ${yTo}`;
    }

    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    path.setAttribute("d", d);
    path.setAttribute("fill", "none");
    path.setAttribute("stroke-width", "1.6");
    path.setAttribute("stroke", dangling ? token("--err") : token("--info"));
    path.setAttribute("marker-end", dangling ? "url(#ahr)" : "url(#ah)");
    if (dangling) path.setAttribute("stroke-dasharray", "4 3");
    svg.appendChild(path);
  });
}

new ResizeObserver(drawPointerEdges).observe(document.body);
window.addEventListener("resize", drawPointerEdges);
$("#cv-canvas")?.addEventListener("scroll", drawPointerEdges);

/* ── the keyboard hint ────────────────────────────────────────────────
   A prototype affordance parked on top of real controls, so it has to be
   dismissible — and stay dismissed. */
function setHint(collapsed) {
  $("#hint")?.classList.toggle("collapsed", collapsed);
  try {
    localStorage.setItem("proton.hint", collapsed ? "collapsed" : "open");
  } catch {
    /* storage blocked — it just won't persist */
  }
}

$(".hint-toggle")?.addEventListener("click", () =>
  setHint(!$("#hint").classList.contains("collapsed")),
);

/* ── boot ─────────────────────────────────────────────────────────────── */

// Cached appearance is only an initial paint, never a project-settings write.
applying = true;
try {
  const theme = localStorage.getItem("proton.theme");
  if (theme) setTheme(theme);
  setTranslucency(localStorage.getItem("proton.translucency") ?? "medium");
  setHint(localStorage.getItem("proton.hint") === "collapsed");
} catch {
  /* nothing stored, or storage is blocked — the defaults stand */
  setTranslucency("medium");
} finally {
  applying = false;
}

wireWindowChrome();
document.documentElement.classList.toggle("shell", HAS_SHELL);
/* The backdrop picker reports what the compositor *did*, not what was asked
   for. Windows turns Mica off under battery saver or with transparency effects
   disabled, and the setting has to be honest when that happens. */
function renderBackdrop(applied, wanted) {
  const mac = /Mac/.test(navigator.platform);
  $$("[data-backdrop-set]").forEach((button) => {
    const kind = button.dataset.backdropSet;
    button.hidden = kind !== "solid" && (mac ? kind !== "vibrancy" : kind === "vibrancy");
  });
  $("[data-backdrop-description]").textContent = mac
    ? "Vibrancy softly blurs the desktop behind your window. Solid uses an opaque background."
    : "Choose a desktop-tinted background or Solid for an opaque window.";
  $$("[data-backdrop-set]").forEach((b) =>
    b.classList.toggle("on", b.dataset.backdropSet === applied),
  );
  const note = $("#backdrop-note");
  if (!note) return;
  if (!HAS_SHELL) {
    note.textContent = "Browser preview — the page paints its own ground here.";
  } else if (wanted && wanted !== applied) {
    note.textContent = `${wanted} is unavailable. Using ${applied}.`;
  } else {
    note.textContent = "";
  }
}

$$("[data-backdrop-set]").forEach((b) =>
  b.addEventListener("click", async () => {
    const wanted = b.dataset.backdropSet;
    renderBackdrop(await setBackdrop(wanted), wanted);
    // Applying a material puts it back behind Codex's opaque window.
    renderWindowGround(true);
    requestAnimationFrame(drawPointerEdges);
  }),
);

applyBackdrop().then((applied) => {
  renderBackdrop(applied);
  requestAnimationFrame(drawPointerEdges);
});
sync();
drawPointerEdges();

// The terminal pane as it was left: open unless somebody shut it, because a
// fresh window is drawn with the shell beside the code.
try {
  const width = Number(localStorage.getItem("proton.terminal.width"));
  if (width > 0) setTerminalWidth(width, false);
  showTerminal(localStorage.getItem("proton.terminal") !== "closed");
} catch {
  showTerminal(true);
}

// The shell keeps its restored buffers. Only the browser preview opens a fixture.
//
// Not inside requestAnimationFrame: rAF does not fire while the window is
// hidden, so booting there means an app started minimised never loads
// anything and shows an empty editor when you finally look at it.
function boot() {
  reportEveryViewport();
  if (!HAS_SHELL) dispatch({ type: "openBuffer", path: "crates/proton-core/src/vim.rs" });
}

if (document.readyState === "complete") boot();
else window.addEventListener("load", boot, { once: true });

viewportObserver.observe(document.body);

// Exposed for the browser preview's console and for driving screenshots.
// Splitting is a command now, not a function here — `dispatch` covers it.
Object.assign(window, {
  setTheme, setTranslucency, setMode: coreSetMode, setDock, toggleTerminal, open, closeAll,
  dispatch, drawPointerEdges, setHint, enterCell, protonPerformance: performanceReview,
});

/* ── the council ───────────────────────────────────
   A debate is a question put to every model with a subscription: each
   answers alone, then having read the others, then one writes the brief.
   It takes about two minutes and spends three subscriptions, so the rounds
   are drawn as they land rather than behind a spinner. */

function renderCouncil(event) {
  const list = $("[data-council-list]");
  if (list) {
    const running = event.running
      ? `<div class="row council on"><span class="ic spin"><svg class="i"><use href="#i-users"/></svg></span>` +
        `<span class="chat-name">${escapeHtml(event.running)}</span>` +
        `<span class="chat-when">${
          event.round === 3 ? "writing the brief" : `round ${event.round}`
        }</span></div>`
      : "";
    const past = event.held
      .map(
        (one) =>
          `<div class="row council${one.name === event.showing ? " sel" : ""}" data-debate="${escapeHtml(one.name)}">` +
          `<span class="chat-name">${escapeHtml(one.question)}</span>` +
          `<span class="chat-when">${one.finished ? "brief" : "no brief"}</span></div>`,
      )
      .join("");
    list.innerHTML =
      running + past ||
      `<div class="empty-note">No debates yet. Type a question and press Convene.</div>`;
  }

  // The stage shows a debate when there is one to show, and the conversation
  // otherwise — one place to look rather than two panes competing.
  const view = $("[data-council-view]");
  const thread = $("[data-claude-thread]");
  if (!view || !thread) return;
  const showing = Boolean(event.running || event.brief || event.problem);
  view.hidden = !showing;
  thread.hidden = showing;
  if (!showing) return;

  const problem = event.problem
    ? `<p class="council-problem">${escapeHtml(event.problem)}</p>`
    : "";
  // Progress is a list of what each member did, so a member that failed is as
  // visible as one that answered — a round that quietly loses a voice
  // reads as agreement.
  const progress = event.progress.length
    ? `<ul class="council-progress">${event.progress
        .map((line) => `<li>${escapeHtml(line)}</li>`)
        .join("")}</ul>`
    : "";
  const heading = event.running
    ? `<div class="council-head">${escapeHtml(event.running)}<span class="chip spin-chip">${
        event.round === 3 ? "writing the brief" : `round ${event.round} of 2`
      }</span></div>`
    : "";

  view.innerHTML = heading + problem + progress + proseHtml(event.brief);
}

/* Convene, from the button or the side pane's +. The question is whatever is
   in the composer, because that is where a question already is. */
document.addEventListener("click", (e) => {
  if (e.target.closest?.("[data-council-new]")) {
    const question = (claude?.question() ?? "").replace(/^\/council\s+/, "").trim();
    if (!question) {
      flashMessage?.("Type the question in the composer, then press the council button.");
      claude?.focus();
      return;
    }
    dispatch({ type: "convene", question });
    claude?.clearQuestion();
    return;
  }
  const row = e.target.closest?.("[data-debate]");
  if (row) dispatch({ type: "openDebate", name: row.dataset.debate });
});

/* ── the agent ────────────────────────────────────────────────────────
   The core owns the conversation; this draws it. The whole transcript
   arrives on every change rather than as deltas — a turn is at most a few
   dozen entries, and a delta protocol for something that small is a
   synchronisation bug waiting to be written. */

/** Which tool gets which icon. Unknown tools still get a mark. */
const TOOL_ICON = {
  read_file: "file-code",
  grep: "search",
  find_files: "files",
  open_buffer: "split-v",
  search_library: "book",
  diagnostics: "d-warning",
  // Antigravity's own tools, which it runs in the project itself.
  view_file: "file-code",
  list_dir: "folder",
  find_by_name: "files",
  grep_search: "search",
  write_to_file: "pen",
  replace_file_content: "pen",
  multi_replace_file_content: "pen",
  sed_file: "pen",
  run_command: "term",
  command_status: "term",
  search_web: "globe",
  read_url_content: "globe",
};

const CALL_STATE = { starting: "", running: "spin", done: "", failed: "" };

/** What each tool is called in a sentence. A tool this list does not know
 *  keeps its own name, which is still the truth about what ran. */
const TOOL_LABEL = {
  read_file: "Read file",
  grep: "Search",
  find_files: "Find files",
  open_buffer: "Open",
  search_library: "Search library",
  diagnostics: "Diagnostics",
  view_file: "Read file",
  list_dir: "List",
  find_by_name: "Find files",
  grep_search: "Search",
  write_to_file: "Write file",
  replace_file_content: "Edit",
  multi_replace_file_content: "Edit",
  sed_file: "Edit",
  run_command: "Run",
  command_status: "Command status",
  search_web: "Search the web",
  read_url_content: "Read page",
};

function toolHtml(call) {
  const spin = CALL_STATE[call.state] === "spin" ? " spin" : "";
  const mark = TOOL_ICON[call.name] ?? "k-other";
  const right =
    call.state === "failed"
      ? `<span class="t" style="color:var(--err)">${escapeHtml(call.result)}</span>`
      : call.result
        ? `<span class="t">${escapeHtml(call.result)}</span>`
        : "";
  const detail = call.detail ? ` <span class="d" title="${escapeHtml(call.detail)}">${escapeHtml(call.detail)}</span>` : "";
  return (
    `<div class="tool" title="${escapeHtml(call.name)}"><svg class="i${spin}"><use href="#i-${mark}"/></svg> ` +
    `<b>${escapeHtml(TOOL_LABEL[call.name] ?? call.name)}</b>${detail}${right}</div>`
  );
}

/** Minimal inline markup: backticks, citations and paragraphs — for what you
 *  wrote, and for the council's brief. An answer is markdown (`answerHtml`).
 *
 *  A citation is `[[path § heading L12-34]]`, which is what `search_library`
 *  hands the model and what it is asked to quote back. The brackets are worth
 *  the trouble: a path can contain spaces and an en dash, so there is no way
 *  to find where one starts in a sentence otherwise — and they are also
 *  Obsidian's own link syntax, so an answer pasted into the vault is already
 *  a working link. Text that is not a citation stays text. */
function proseHtml(text) {
  return escapeHtml(text)
    .split(/\n{2,}/)
    .filter((p) => p.trim())
    .map(
      (p) =>
        `<p>${p
          .replace(/`([^`]+)`/g, "<code>$1</code>")
          .replace(
            /\[\[([^[\]\n]+)\]\]/g,
            (_, cite) => `<span class="cite" data-cite="${cite}" title="Open this note">${cite}</span>`,
          )
          .replace(/\n/g, "<br>")}</p>`,
    )
    .join("");
}

/** An answer as the agent wrote it: markdown, its code coloured the way the
 *  editor colours code and its TeX drawn as equations, with citations kept.
 *  The same renderer as the Chat tab's, so an answer reads alike in both. */
function answerHtml(text, streaming) {
  return text.trim() ? `<div class="md">${markdown(text, {streaming, cite: true})}</div>` : "";
}

/** The model that is answering, for the transcript's byline. */
let answeringModel = "";

function entryHtml(entry) {
  const you = entry.speaker === "you";
  const who = you
    ? `<svg class="i" style="color:var(--accent)"><use href="#i-proton"/></svg> You`
    : `<svg class="i ft-ai"><use href="#i-spark"/></svg> Proton${
        answeringModel ? ` · ${escapeHtml(answeringModel)}` : ""
      }`;

  const thinking = entry.thinking
    ? `<details class="thought"><summary><svg class="i"><use href="#i-bulb"/></svg>Thinking</summary>${answerHtml(entry.thinking, entry.streaming && !entry.text)}</details>`
    : "";
  const calls = entry.calls.map(toolHtml).join("");
  // A caret while the text is still arriving, so an empty answer that is on
  // its way does not look like an empty answer that arrived.
  const caret = entry.streaming ? `<span class="cursor"></span>` : "";

  return (
    `<div class="msg${you ? " you" : ""}"><div class="who">${who}</div>` +
    `${thinking}${calls}${you ? proseHtml(entry.text) : answerHtml(entry.text, entry.streaming)}${caret}</div>`
  );
}

/* ── what is installed ────────────────────────────────────────────────
   Sent once, when the front end connects: plugins are loaded when the
   session is built and there is no way to install one without restarting,
   so there is nothing later to redraw from. */
function renderPlugins(event) {
  // Palettes first, so a theme a plugin brought is choosable before anyone
  // reads the list of what brought it.
  plugged.clear();
  event.themes.forEach((t) => plugged.set(t.id, t));

  const strip = $(".themes");
  if (strip) {
    $$(".theme-card[data-from]", strip).forEach((c) => c.remove());
    strip.insertAdjacentHTML(
      "beforeend",
      event.themes.map(themeCardHtml).join(""),
    );
    // The stored choice may name a plugin's theme, which did not exist when
    // the page loaded and so could not be applied then.
    const wanted = (() => {
      try {
        return localStorage.getItem("proton.theme");
      } catch {
        return null;
      }
    })();
    if (wanted && plugged.has(wanted)) setTheme(wanted);
  }

  const box = $("[data-plugins]");
  const setting = $("[data-plugins-setting]");
  if (!box || !setting) return;
  setting.hidden = event.items.length === 0;
  box.innerHTML = event.items.map(pluginRowHtml).join("");
}

/** Five swatches, the way the built-in cards show one. */
function themeCardHtml(theme) {
  const of = (name) => theme.colours.find(([k]) => k === name)?.[1];
  const swatches = ["bg", "accent", "accent-2", "ok", "warn"]
    .map((n) => of(n) ?? of("text") ?? "#888")
    .map((c) => `<i style="background:${escapeHtml(c)}"></i>`)
    .join("");
  return (
    `<button class="theme-card" data-theme-set="${escapeHtml(theme.id)}" ` +
    `data-from="${escapeHtml(theme.from)}" title="from ${escapeHtml(theme.from)}">` +
    `<span class="sw">${swatches}</span>` +
    `<span class="nm">${escapeHtml(theme.name)}</span></button>`
  );
}

function pluginRowHtml(plugin) {
  // A plugin that failed says why, in its own row. One that failed silently
  // is one you uninstall in confusion.
  const right = plugin.problem
    ? `<span class="dir" style="margin-left:auto;padding-left:16px;color:var(--err)">${escapeHtml(plugin.problem)}</span>`
    : `<span class="dir" style="margin-left:auto;padding-left:16px">${escapeHtml(plugin.provides)}</span>`;
  const version = plugin.version ? ` <span class="dim">${escapeHtml(plugin.version)}</span>` : "";
  return (
    `<div class="res" style="cursor:default;align-items:flex-start">` +
    `<span>${escapeHtml(plugin.name)}${version}` +
    (plugin.description ? `<br><span class="dim">${escapeHtml(plugin.description)}</span>` : "") +
    `</span>${right}</div>`
  );
}

/* ── the task queue ───────────────────────────────────────────────────
   Two groups, as the design has always drawn them: what is happening and
   what is waiting on you. The core owns the queue — which one is running,
   what each is doing, and what it left behind. */

const RING = { running: "run", queued: "", ready: "done", failed: "fail" };

function renderTasks(event) {
  const badge = $("[data-task-badge]");
  badge.textContent = String(event.tasks.length);
  badge.hidden = !event.tasks.length;
  // The Tasks entry is behind the agent header's `⋯` now, so the count needs
  // a mark that can be seen with the menu shut.
  const dot = $("[data-task-dot]");
  if (dot) dot.hidden = !event.tasks.length;
  const box = $("[data-tasks]");
  if (!box) return;

  const doing = event.tasks.filter((t) => t.state === "running" || t.state === "queued");
  const done = event.tasks.filter((t) => t.state === "ready" || t.state === "failed");

  const count = $("[data-task-count]");
  if (count) {
    count.textContent = event.tasks.length
      ? `${doing.length} running · ${done.length} ready`
      : "nothing queued";
  }

  const group = (title, list) =>
    list.length
      ? `<div class="group">${title} · ${list.length}</div>` + list.map(taskHtml).join("")
      : "";

  box.innerHTML =
    group("In progress", doing) + group("Ready for review", done) ||
    `<div class="empty">Nothing queued. Ask Proton something.</div>`;
}

function taskHtml(task) {
  // A queued one can be taken off; a running one cannot, so it is not
  // offered — a button that refuses is worse than no button.
  const drop =
    task.state === "running"
      ? ""
      : `<span class="x" data-task-drop="${task.id}" title="Remove"><svg class="i"><use href="#i-close"/></svg></span>`;
  const files = task.files.length
    ? `<div class="sub">${task.files
        .map((f) => `<span class="artifact">${fileIcon(f)} ${escapeHtml(f)}</span>`)
        .join("")}</div>`
    : "";
  const note = task.note
    ? `<div class="sub">${escapeHtml(task.note)}${
        task.tools ? ` <span class="dim">· ${task.tools} tool${task.tools === 1 ? "" : "s"}</span>` : ""
      }</div>`
    : "";
  return (
    `<div class="task" data-task="${task.id}">` +
    `<div class="hd"><span class="ring ${RING[task.state] ?? ""}"></span>` +
    `${escapeHtml(task.prompt)}${drop}</div>${note}${files}</div>`
  );
}

document.addEventListener("click", (e) => {
  const drop = e.target.closest("[data-task-drop]");
  if (drop) {
    e.stopPropagation();
    dispatch({ type: "dropTask", id: Number(drop.dataset.taskDrop) });
    return;
  }
  if (e.target.closest("[data-tasks-clear]")) dispatch({ type: "clearTasks" });
});

/* Changes the agent has proposed and nobody has accepted yet.
   The hunks are the live diff between each buffer and what was proposed, so
   this redraws whole on every change — there is no stored list of them a
   delta could describe. */
function renderReview(event) {
  const box = $("[data-staged]");
  if (!box) return;

  if (!event.files.length) {
    box.hidden = true;
    box.innerHTML = "";
    return;
  }

  const total = event.files.reduce((n, f) => n + f.hunks.length, 0);
  box.innerHTML =
    `<div class="head">${total} hunk${total === 1 ? "" : "s"} staged` +
    `<span class="acts">` +
    `<button data-staged-reject>Discard all</button>` +
    `<button class="go" data-staged-apply>Apply all</button>` +
    `</span></div>` +
    event.files
      .map((file) =>
        file.hunks
          .map(
            (hunk, i) =>
              `<div class="diff"><header>` +
              `${escapeHtml(file.path)} · L${hunk.line}` +
              (file.creates ? ` <span class="new">new file</span>` : "") +
              `<span class="acts">` +
              `<button data-refer="${i}" data-path="${escapeHtml(file.path)}">Refer</button>` +
              `<button class="go" data-apply="${i}" data-path="${escapeHtml(file.path)}">Apply</button>` +
              `</span></header><pre>${hunk.lines.map(diffLineHtml).join("")}</pre></div>`,
          )
          .join(""),
      )
      .join("");
  box.hidden = false;
}

const DIFF_MARK = { added: "p", removed: "m", context: "c" };

function diffLineHtml(line) {
  const sign = { added: "+ ", removed: "- ", context: "  " }[line.mark];
  return `<span class="${DIFF_MARK[line.mark]}">${escapeHtml(sign + line.text)}</span>`;
}

/* One hunk at a time, addressed by file and index. The index is only valid
   for the list as it is right now — the core refuses one that has moved on
   rather than applying it to whatever is at that position. */
document.addEventListener("click", (e) => {
  const apply = e.target.closest("[data-apply]");
  if (apply) {
    dispatch({ type: "applyHunk", path: apply.dataset.path, hunk: Number(apply.dataset.apply) });
    return;
  }
  const refer = e.target.closest("[data-refer]");
  if (refer) {
    dispatch({ type: "rejectHunk", path: refer.dataset.path, hunk: Number(refer.dataset.refer) });
    return;
  }
  if (e.target.closest("[data-staged-apply]")) dispatch({ type: "applyStaged", path: null });
  if (e.target.closest("[data-staged-reject]")) dispatch({ type: "rejectStaged", path: null });
});

/* What each provider offers, for the composer's picker.
   A short list rather than every model either of them has ever shipped: this
   is a control you glance at mid-question, and the settings panel takes a
   typed name for anything else. The one in use is always shown even when it
   is not on this list. */
const MODELS = {
  codex: ["gpt-6-astra"],
  claude: ["claude-opus-5", "claude-sonnet-5", "claude-haiku-4-5-20251001"],
  // Aliases first, then the pinned ids they currently resolve to. Every
  // one of these was checked against `:streamGenerateContent` — the list
  // this replaced held three ids and all three were dead.
  gemini: [
    "gemini-pro-latest",
    "gemini-flash-latest",
    "gemini-3.1-pro-preview",
    "gemini-3.8-flash",
  ],
  // `agy models` ids: the thinking level is part of the name.
  antigravity: [
    "gemini-3.8-flash-high",
    "gemini-3.8-flash-medium",
    "gemini-3.8-flash-low",
    "gemini-3.1-pro-high",
    "gemini-3.1-pro-low",
  ],
};

// Choosing one writes the setting, which is the same thing the settings panel
// writes — one place decides which model answers.
document.addEventListener("change", (e) => {
  const pick = e.target.closest?.("[data-model-pick]");
  if (!pick) return;
  dispatch({ type: "setSetting", key: "agent.model", value: pick.value });
});

/* ── conversations ────────────────────────────────────────────────────

   Chat's explorer. Files in `.proton/chats`, most recently changed first,
   from the core rather than from anything this end remembers. */

let lastChats = { conversations: [], current: "" };

function renderChats(event) {
  lastChats = event;
  // The agent's header in Codex names the conversation, as Zed's names the
  // thread.
  renderDockTitle();

  const title = $("[data-chat-title]");
  if (title) {
    const open = event.conversations.find((c) => c.path === event.current);
    title.textContent = open?.title || "New conversation";
    title.className = "chip" + (open ? " accent" : "");
  }
  // Nothing to rename or delete until there is something saved.
  for (const button of [$("[data-chat-rename]"), $("[data-chat-delete]")]) {
    if (button) button.disabled = !event.current;
  }

  const box = $("[data-chats]");
  if (!box) return;
  if (!event.conversations.length) {
    box.innerHTML = `<div class="empty">No threads yet. Ask something.</div>`;
    return;
  }

  box.innerHTML = event.conversations
    .map((chat) => {
      const path = escapeHtml(chat.path);
      // Deleting asks first, in the row, because the file has no undo.
      if (chat.path === forgetting) {
        return (
          `<div class="row chat ask"><span class="chat-name">Delete “${escapeHtml(chat.title)}”?</span>` +
          `<span class="chat-acts"><button data-chat-keep>Cancel</button>` +
          `<button class="danger" data-chat-forget-yes="${path}">Delete</button></span></div>`
        );
      }
      // The turn count is what distinguishes a question you abandoned from one
      // you worked through, which is the thing you are scanning for.
      const turns = chat.turns === 1 ? "1 turn" : `${chat.turns} turns`;
      const who = PROVIDER_NAMES[chat.provider] ? ` · ${PROVIDER_NAMES[chat.provider]}` : "";
      return (
        `<div class="row chat${chat.path === event.current ? " sel" : ""}" role="menuitem" tabindex="-1"` +
        ` data-chat="${path}" title="${path}">` +
        `<span class="chat-name">${escapeHtml(chat.title)}</span>` +
        `<span class="chat-meta">${turns}${escapeHtml(who)}</span>` +
        `<button class="chat-x" data-chat-forget="${path}" title="Delete this thread" aria-label="Delete ${escapeHtml(chat.title)}">` +
        `<svg class="i"><use href="#i-close"/></svg></button></div>`
      );
    })
    .join("");
}

/** Which providers a thread can say it is from. */
const PROVIDER_NAMES = /** @type {Record<string, string>} */ ({
  claude: "Claude", gemini: "Gemini", codex: "Codex", antigravity: "Antigravity",
});

/** The thread a row is asking whether to delete, if one is. */
let forgetting = "";

/** Show or put away the agent's past threads. */
function showThreads(on) {
  const list = $("[data-chat-threads]");
  const button = $("[data-chat-history]");
  if (!list || !button) return;
  if (!on && forgetting) {
    forgetting = "";
    renderChats(lastChats);
  }
  list.hidden = !on;
  button.setAttribute("aria-expanded", String(on));
  if (on) ($(".row.chat.sel", list) ?? $(".row.chat", list))?.focus();
}

$("[data-chat-threads]")?.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    e.preventDefault();
    e.stopPropagation();
    showThreads(false);
    $("[data-chat-history]")?.focus();
  } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    const rows = $$("[data-chat-threads] .row.chat[data-chat]");
    const at = rows.indexOf(document.activeElement);
    rows[(at + (e.key === "ArrowDown" ? 1 : -1) + rows.length) % rows.length]?.focus();
  } else if (e.key === "Enter" && document.activeElement?.matches?.("[data-chat]")) {
    e.preventDefault();
    document.activeElement.click();
  }
});

document.addEventListener("click", (e) => {
  if (e.target.closest?.("[data-chat-new]")) {
    showThreads(false);
    dispatch({ type: "newChat" });
    return;
  }

  // The past threads: open, close, and delete after asking. Checked before
  // the row itself, because the buttons sit inside it.
  const threads = $("[data-chat-threads]");
  if (e.target.closest?.("[data-chat-history]")) {
    showThreads(Boolean(threads?.hidden));
    return;
  }
  const forget = e.target.closest?.("[data-chat-forget]");
  if (forget) {
    forgetting = forget.dataset.chatForget;
    renderChats(lastChats);
    return;
  }
  if (e.target.closest?.("[data-chat-keep]")) {
    forgetting = "";
    renderChats(lastChats);
    return;
  }
  const gone = e.target.closest?.("[data-chat-forget-yes]");
  if (gone) {
    forgetting = "";
    dispatch({ type: "deleteChat", path: gone.dataset.chatForgetYes });
    return;
  }
  if (threads && !threads.hidden && !e.target.closest?.("[data-chat-threads]")) showThreads(false);

  if (e.target.closest?.("[data-chat-rename]")) {
    const open = lastChats.conversations.find((c) => c.path === lastChats.current);
    if (!open) return;
    const title = prompt("Name this conversation", open.title);
    // `null` is cancel; an empty string would silently un-name it.
    if (title && title.trim()) {
      dispatch({ type: "renameChat", path: open.path, title: title.trim() });
    }
    return;
  }

  const row = e.target.closest?.("[data-chat]");
  if (row) {
    showThreads(false);
    dispatch({ type: "openChat", path: row.dataset.chat });
  }
});

function renderAgent(event) {
  answeringModel = event.model ?? "";
  // Every thread: Codex's dock and Chat's stage show the same conversation,
  // and one of them is always off screen. Painting only the visible one would
  // leave the other showing whatever it had when it was last looked at.
  const html = event.transcript.length ? event.transcript.map(entryHtml).join("") : null;
  for (const thread of $$("[data-agent-thread]")) {
    if (html === null) continue;
    const atBottom = thread.scrollHeight - thread.scrollTop - thread.clientHeight < 60;
    thread.innerHTML = html;
    // Follow the answer only if the reader was already at the bottom —
    // yanking someone back down while they are reading is worse than
    // making them scroll.
    if (atBottom) thread.scrollTop = thread.scrollHeight;
  }
  // A conversation that was cleared has to clear the panes too, or `New chat`
  // leaves the last one on screen.
  if (!event.transcript.length) {
    for (const thread of $$("[data-agent-thread]")) thread.innerHTML = "";
  }

  // Which service answered. Worth a permanent chip rather than a setting you
  // have to go and look up: the two give different answers to the same
  // question, and a transcript you cannot attribute is one you cannot reason
  // about a week later.
  for (const who of $$("[data-agent-provider]")) {
    who.textContent = {claude: "Claude", gemini: "Gemini", codex: "Codex", antigravity: "Antigravity"}[event.provider] ?? event.provider;
    who.title = `${event.model} — change it in Settings > AI & models`;
    who.className = "chip" + (event.provider === "gemini" ? " ok" : " accent");
  }

  // The model picker: what this provider offers, plus whatever is actually
  // set even when it is not on the list — a model typed into settings must
  // still show as the one in use rather than silently reading as the default.
  for (const pick of $$("[data-model-pick]")) {
    const known = MODELS[event.provider] ?? [];
    const options = known.includes(event.model) ? known : [event.model, ...known];
    const want = options.join("|");
    if (pick.dataset.built !== want) {
      pick.dataset.built = want;
      pick.innerHTML = options
        .map((m) => `<option value="${escapeHtml(m)}">${escapeHtml(m)}</option>`)
        .join("");
    }
    pick.value = event.model;
  }

  // Where the key comes from, in the settings panel. Not a promise that a
  // request will succeed — only that one will be attempted with something.
  const auth = $("[data-agent-auth]");
  if (auth) auth.textContent = event.credential || "";

  for (const box of $$(".composer .box")) {
    box.classList.toggle("busy", event.activity !== "idle");
  }

  for (const send of $$("[data-agent-send]")) {
    send.classList.toggle("stop", event.activity !== "idle");
    send.setAttribute("aria-label", event.activity === "idle" ? "Send" : "Stop");
    // Codex's composer sends with Zed's paper plane; Chat keeps its arrow.
    const idle = send.closest("#codex-dock") ? "send" : "arrow-up";
    send.innerHTML = `<svg class="i"><use href="#i-${
      event.activity === "idle" ? idle : "stop"
    }"/></svg>`;
  }

  const meter = $("#sl-agent");
  if (meter) {
    // The cached figure earns its place: a prefix that stops matching is a
    // cost regression rather than an error, and nothing else reports it.
    meter.innerHTML = event.inputTokens
      ? `<svg class="i ft-ai"><use href="#i-spark"/></svg> ${fmt(event.inputTokens)} in · ` +
        `${fmt(event.outputTokens)} out · <span class="cached">${fmt(event.cachedTokens)} cached</span>`
      : `<svg class="i ft-ai"><use href="#i-spark"/></svg> ${escapeHtml(
          event.model || "agent",
        )}`;
  }
}

const fmt = (n) => (n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n));

/* Clicking a scope chip writes the mention into the question, rather than
   toggling some state the sentence does not mention. The design's line is that
   what the model can see is a chip you can point at — so the chip puts a word
   in the text you can see, read back, and delete. */
document.addEventListener("click", (e) => {
  const chip = e.target.closest?.("[data-mention]");
  if (!chip) return;
  const input = chip.closest(".composer")?.querySelector("[data-agent-input]");
  if (!input) return;

  const word = chip.dataset.mention;
  const words = input.textContent.split(' ').filter(Boolean);
  // The second click takes it out again: a chip that only ever adds is one
  // you cannot undo without editing the sentence by hand.
  const has = words.includes(word);
  input.textContent = (has ? words.filter((w) => w !== word) : [...words, word]).join(' ');

  input.focus();
  // Caret to the end, or typing continues in front of what was just added.
  const range = document.createRange();
  range.selectNodeContents(input);
  range.collapse(false);
  const selection = getSelection();
  selection.removeAllRanges();
  selection.addRange(range);
  chip.classList.toggle("on", !has);
});

/* There is more than one composer — Codex has one in its dock and Chat has
   one as its whole stage — and they are the same conversation seen twice. So
   everything here works from the composer the event came from rather than
   from the first one in the document, which would send Chat's question from
   Codex's empty box. */

function askAgent(from) {
  const input = from?.querySelector("[data-agent-input]") ?? $("[data-agent-input]");
  if (!input) return;
  const text = input.textContent.trim();
  if (!text) return;
  input.textContent = "";
  dispatch({ type: "ask", text });
}

document.addEventListener("click", (e) => {
  const button = e.target.closest("[data-agent-send]");
  if (!button) return;
  // The same button stops a turn that is running — the thing you want when
  // an answer is going the wrong way is always in the same place.
  if (button.classList.contains("stop")) dispatch({ type: "cancelTurn" });
  else askAgent(button.closest(".composer"));
});

// Enter sends, Shift-Enter makes a newline. The composer is not the buffer, so
// these keys never reach the modal engine. Delegated rather than bound to one
// input, because Chat's composer does not exist when this file is read.
document.addEventListener("keydown", (e) => {
  const input = e.target.closest?.("[data-agent-input]");
  if (!input) return;
  e.stopPropagation();
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    askAgent(input.closest(".composer"));
  }
});

/* ── the Visualizer ───────────────────────────────────────────────────
   The core owns the trace and does the pointer resolution — exact match
   first, then the smallest containing range — because that rule is the
   whole design of the format and it is tested in Rust. The pane draws
   what it is told and re-resolves nothing. */

const trace = { view: null, step: 0, steps: 0, state: "empty" };

/** One value row. Pointers carry the resolved edge id, not an address. */
function slotHtml(slot, depth = 0) {
  const pad = depth ? ` style="padding-left:${10 + depth * 9}px"` : "";
  const value = slot.pointsTo
    ? `<span class="vl ptr">${escapeHtml(slot.repr)}</span>`
    : `<span class="vl">${escapeHtml(slot.repr)}</span>`;
  const row =
    `<div class="slot" data-node="${escapeHtml(slot.id)}"${pad}>` +
    `<span class="nm">${escapeHtml(slot.name)}</span>${slot.repr ? value : ""}</div>`;
  return row + slot.children.map((c) => slotHtml(c, depth + 1)).join("");
}

function frameHtml(frame) {
  return (
    `<div class="frame" data-node="${escapeHtml(frame.id)}">` +
    `<div class="fh">${escapeHtml(frame.func)}<span class="dim" style="margin-left:auto">` +
    `L${frame.line}</span></div>` +
    frame.slots.map((s) => slotHtml(s)).join("") +
    `</div>`
  );
}

function blockHtml(block) {
  const freed = block.freed ? " freed" : "";
  const tag = block.freed
    ? `<span class="chip" style="margin-left:6px;color:var(--err)">freed</span>`
    : "";
  return (
    `<div class="block${freed}" data-node="${escapeHtml(block.id)}" ` +
    `data-addr="${escapeHtml(block.addr)}">` +
    `<div class="fh">${escapeHtml(block.addr)} · ${escapeHtml(block.typeName)}${tag}</div>` +
    block.slots.map((s) => slotHtml(s)).join("") +
    `</div>`
  );
}

function renderTrace(event) {
  Object.assign(trace, {
    view: event.view,
    step: event.step,
    steps: event.steps,
    state: event.state,
  });

  const subject = $("[data-trace-subject]");
  if (subject) subject.textContent = event.subject || "no trace";
  const status = $("[data-trace-status]");
  if (status) {
    status.textContent = event.message;
    status.className = "chip" + (event.state === "failed" ? " warn" : "");
  }

  const scrub = $("[data-trace-scrub]");
  if (scrub) {
    scrub.max = String(Math.max(0, event.steps - 1));
    scrub.value = String(event.step);
    scrub.disabled = event.steps === 0;
  }
  const counter = $("[data-trace-step]");
  if (counter) {
    counter.textContent = event.steps ? `${event.step + 1} / ${event.steps}` : "—";
  }

  const stack = $("[data-trace-stack]");
  const heap = $("[data-trace-heap]");
  const out = $("[data-trace-stdout]");
  if (!stack || !heap) return;

  const view = event.view;
  if (!view) {
    stack.innerHTML = "";
    heap.innerHTML = "";
    if (out) out.textContent = "";
    drawPointerEdges();
    return;
  }

  // Innermost frame first is how the format sends it, and how a call stack
  // reads — the frame you are in belongs at the top.
  stack.innerHTML =
    `<div class="mem-h">Stack</div>` +
    view.frames.map(frameHtml).join("") +
    (view.globals.length
      ? `<div class="frame"><div class="fh">globals</div>` +
        view.globals.map((s) => slotHtml(s)).join("") +
        `</div>`
      : "");

  heap.innerHTML =
    `<div class="mem-h">Heap</div>` +
    (view.heap.length ? view.heap.map(blockHtml).join("") : `<div class="dim">nothing allocated</div>`);

  if (out) out.textContent = view.stdout;
  drawPointerEdges();
}

document.addEventListener("input", (e) => {
  const scrub = e.target.closest("[data-trace-scrub]");
  if (scrub) dispatch({ type: "traceSeek", step: Number(scrub.value) });
});

document.addEventListener("click", (e) => {
  const step = e.target.closest("[data-trace-nudge]");
  if (step) dispatch({ type: "traceStep", by: Number(step.dataset.traceNudge) });
});

/* Stepping keys, scoped to the pane.
 *
 * Upstream binds the arrows globally and Space to play/pause. Proton cannot:
 * the arrows are motions and Space is the leader. So they work when the
 * Visualizer has focus and nowhere else, which is the resolution the
 * integration notes settled on. */
document.addEventListener("keydown", (e) => {
  const canvas = $("#cv-canvas");
  if (!canvas || !canvas.contains(document.activeElement)) return;
  const by = { ArrowLeft: -1, ArrowRight: 1, "[": -1, "]": 1 }[e.key];
  if (by === undefined) return;
  e.preventDefault();
  e.stopPropagation();
  dispatch({ type: "traceStep", by: e.shiftKey ? by * 10 : by });
}, true);

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

import { HAS_SHELL, applyBackdrop, dispatch, onEvent, sendKey, setBackdrop,
  setMode as coreSetMode, sync, wireWindowChrome } from "./bridge.js";
import { closeDocument, documentIsOpen, openDocument, zoomDocument } from "./document.js";

const $  = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];

const token = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

/* ── theme ────────────────────────────────────────────────────────────
   A theme sets sixteen custom properties on <html> and nothing else. No
   rule below the palette block knows which one is active, which is why
   switching is instant and why adding a fifth is a paste, not a port. */
const THEMES = ["tokyo-night", "one-dark", "catppuccin", "nord", "gruvbox-dark"];

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
const MODES = ["chat", "codex", "design", "modeling", "game", "video", "data"];

/** Which mode is on screen. The core owns it; this is a copy for the handful
 *  of places that have to behave differently in one of them. */
let activeMode = "codex";

function renderMode(mode) {
  activeMode = mode;
  $$(".mode").forEach((b) => b.setAttribute("aria-selected", String(b.dataset.mode === mode)));
  $$(".workspace").forEach((w) => w.classList.toggle("on", w.dataset.ws === mode));
  closeAll();
  // The container that was just revealed is empty — the splits were drawn
  // into the mode we came from. Redraw them here, and the notebook with them.
  if (lastSplits) {
    renderSplits(lastSplits.views, lastSplits.focused, lastSplits.vertical);
  }
  if (mode === "data" && lastNotebook) renderNotebook(lastNotebook);
}

$$(".mode").forEach((b) => b.addEventListener("click", () => coreSetMode(b.dataset.mode)));
$$("[data-mode-go]").forEach((el) =>
  el.addEventListener("click", () => coreSetMode(el.dataset.modeGo)),
);

/* ── right dock ───────────────────────────────────────────────────────── */
function setDock(panel) {
  const dock = $("#codex-dock");
  if (!dock) return;
  $$(".dock-tab", dock).forEach((t) => t.classList.toggle("on", t.dataset.dockpanel === panel));
  $$(".dock-panel", dock).forEach((p) => {
    const on = p.dataset.panel === panel;
    p.classList.toggle("on", on);
    p.style.display = on ? "flex" : "none";
  });
  requestAnimationFrame(drawPointerEdges);

  // A terminal panel with no terminal in it is a panel that does nothing until
  // you find the button. Showing it is the request.
  if (panel === "term") {
    dispatch({ type: "openTerminal" });
    requestAnimationFrame(() => {
      reportTerminalSize();
      focusTerminal(true);
    });
  }
  // Git's answer moves for reasons the core never sees — a commit in the
  // terminal panel, a branch switched outside — so opening the panel asks
  // again rather than showing whatever was true at the last sync.
  if (panel === "git") dispatch({ type: "refreshRepo" });
}

$$("#codex-dock .dock-tab").forEach((t) =>
  t.addEventListener("click", () => setDock(t.dataset.dockpanel)),
);

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

$$(".tree").forEach((tree) => {
  $$(".row", tree).forEach((r) =>
    r.addEventListener("click", () => {
      $$(".row", tree).forEach((o) => o.classList.remove("sel"));
      r.classList.add("sel");
    }),
  );
});

// Delegated, because splitting clones buffer bars and a cloned node brings no
// listeners with it.
document.addEventListener("click", (e) => {
  const buf = e.target.closest(".buf");
  if (!buf) return;
  $$(".buf", buf.parentElement).forEach((o) => o.classList.remove("on"));
  buf.classList.add("on");
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
      `<div class="scroll" data-cells data-view="${view}"></div>` +
      `</section>`
    );
  }
  return (
    `<section class="split${on}" data-view="${view}">` +
    `<div class="buffers">` +
    `<span data-buffers></span>` +
    actions +
    `</div>` +
    `<div class="scroll code" data-editor data-view="${view}"></div>` +
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
    // Each pane has its own height, and the core has to be told all of them.
    requestAnimationFrame(reportEveryViewport);
  } else {
    $$(".split", box).forEach((pane) =>
      pane.classList.toggle("on", Number(pane.dataset.view) === focused),
    );
  }
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
    const index = viewOf(action);
    const kind = action.dataset.split;
    if (kind === "v") dispatch({ type: "split", vertical: true });
    else if (kind === "s") dispatch({ type: "split", vertical: false });
    else dispatch({ type: "closeSplit", index });
    return;
  }
  // Clicking a panes chrome — its buffer bar, its empty space — focuses it.
  // A click in the *text* is handled on mousedown, which fires first and has
  // to focus before it places the cursor.
  const pane = e.target.closest(".split");
  if (pane && splits()?.contains(pane)) {
    const index = Number(pane.dataset.view);
    if (index !== view.focused) dispatch({ type: "focusSplit", index });
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
const SPLIT_MIN = 120;

/** The pane before a divider and the pane after it. */
function neighbours(gutter) {
  return [gutter.previousElementSibling, gutter.nextElementSibling];
}

function resizeSplit(gutter, at) {
  const [before, after] = neighbours(gutter);
  if (!before || !after) return;

  const stacked = gutter.parentElement.classList.contains("stacked");
  const a = before.getBoundingClientRect();
  const b = after.getBoundingClientRect();
  const size = (r) => (stacked ? r.height : r.width);
  const start = stacked ? a.top : a.left;

  // The pair's own span, shared out by where the pointer is inside it. Only
  // the two of them: a share is relative to its siblings, so leaving the rest
  // alone is what keeps a drag local.
  const span = size(a) + size(b);
  if (span <= SPLIT_MIN * 2) return;
  const want = Math.max(SPLIT_MIN, Math.min(at - start, span - SPLIT_MIN));

  // Scaled to keep the pair's total share the same, whatever it was — the
  // two may already be an uneven part of a row of four.
  const total =
    (parseFloat(before.style.getPropertyValue("--share")) || 1) +
    (parseFloat(after.style.getPropertyValue("--share")) || 1);
  const share = (want / span) * total;
  before.style.setProperty("--share", share.toFixed(4));
  after.style.setProperty("--share", (total - share).toFixed(4));
}

let splitDrag = null;

document.addEventListener("pointerdown", (e) => {
  const gutter = e.target.closest(".sp-gutter");
  if (!gutter) return;
  e.preventDefault();
  splitDrag = gutter;
  gutter.classList.add("dragging");
  document.body.style.cursor = gutter.parentElement.classList.contains("stacked")
    ? "row-resize"
    : "col-resize";
});

window.addEventListener("pointermove", (e) => {
  if (!splitDrag) return;
  const stacked = splitDrag.parentElement.classList.contains("stacked");
  resizeSplit(splitDrag, stacked ? e.clientY : e.clientX);
});

window.addEventListener("pointerup", () => {
  if (!splitDrag) return;
  splitDrag.classList.remove("dragging");
  splitDrag = null;
  document.body.style.cursor = "";
  // A pane that changed size can show a different number of lines, and the
  // core hands out exactly the ones asked for.
  reportEveryViewport();
});

/* Double-clicking a divider evens the pair up again — the way back from a
   drag you did not mean, without having to aim. */
document.addEventListener("dblclick", (e) => {
  const gutter = e.target.closest(".sp-gutter");
  if (!gutter) return;
  neighbours(gutter).forEach((pane) => pane?.style.removeProperty("--share"));
  reportEveryViewport();
});

/* ── resizable panes ──────────────────────────────────────────────────
   Each gutter writes one custom property on its workspace, and the grid
   does the rest. Widths are remembered as *what you asked for* rather
   than what currently fits, so narrowing the window and widening it again
   gives your layout back instead of leaving it clamped. */
const PANE_MIN = 172;
const STAGE_MIN = 320;

function paneWidths(ws) {
  try {
    return JSON.parse(localStorage.getItem(`proton.panes.${ws}`) ?? "{}");
  } catch {
    return {};
  }
}

function rememberPane(ws, prop, px) {
  try {
    localStorage.setItem(`proton.panes.${ws}`, JSON.stringify({ ...paneWidths(ws), [prop]: px }));
  } catch {
    /* storage blocked — the drag still works, it just won't survive a reload */
  }
}

function defaultPane(section, prop) {
  // The value the markup shipped with, before any drag.
  return parseFloat(section.dataset[prop === "--col-l" ? "defL" : "defR"]);
}

$$(".workspace").forEach((section) => {
  section.dataset.defL = parseFloat(section.style.getPropertyValue("--col-l")) || 0;
  section.dataset.defR = parseFloat(section.style.getPropertyValue("--col-r")) || 0;

  const saved = paneWidths(section.dataset.ws);
  for (const [prop, px] of Object.entries(saved)) section.style.setProperty(prop, `${px}px`);
});

function resizePane(section, gutter, clientX) {
  const prop = gutter.dataset.resize;
  const box = section.getBoundingClientRect();
  const other = prop === "--col-l"
    ? parseFloat(getComputedStyle(section).getPropertyValue("--col-r")) || 0
    : parseFloat(getComputedStyle(section).getPropertyValue("--col-l")) || 0;
  // Design mode has a fixed 66px tool rail ahead of the resizable column.
  const fixed = section.dataset.ws === "design" ? 66 : 0;

  const raw = prop === "--col-l" ? clientX - box.left - fixed : box.right - clientX;
  const room = box.width - fixed - other - STAGE_MIN - 2;
  const px = Math.round(Math.max(PANE_MIN, Math.min(raw, room)));

  section.style.setProperty(prop, `${px}px`);
  rememberPane(section.dataset.ws, prop, px);
}

/* The drag lives on `window`, not on the gutter. Pointer capture would be
   tidier, but it can throw and it drops the drag the moment the pointer
   outruns the 1px track — and losing a drag mid-gesture is worse than the
   extra listener. */
let drag = null;

$$(".gutter").forEach((gutter) => {
  const section = gutter.closest(".workspace");

  gutter.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    drag = { gutter, section };
    gutter.classList.add("dragging");
    document.documentElement.classList.add("resizing");
  });

  gutter.addEventListener("dblclick", () => {
    const prop = gutter.dataset.resize;
    const px = defaultPane(section, prop);
    section.style.setProperty(prop, `${px}px`);
    rememberPane(section.dataset.ws, prop, px);
    requestAnimationFrame(drawPointerEdges);
  });
});

window.addEventListener("pointermove", (e) => {
  if (!drag) return;
  e.preventDefault();
  resizePane(drag.section, drag.gutter, e.clientX);
});

function endDrag() {
  if (!drag) return;
  drag.gutter.classList.remove("dragging");
  document.documentElement.classList.remove("resizing");
  drag = null;
  requestAnimationFrame(drawPointerEdges);
}

window.addEventListener("pointerup", endDrag);
window.addEventListener("pointercancel", endDrag);
// A drag that ends outside the window still has to end.
window.addEventListener("blur", endDrag);

/* ── overlays ─────────────────────────────────────────────────────────── */
const OVERLAYS = ["telescope", "whichkey", "cmdline", "settings"];

function closeAll() {
  OVERLAYS.forEach((id) => $("#" + id).classList.remove("on"));
  $("#scrim").classList.remove("on");
}

/**
 * Toggle an overlay. For buttons: clicking Settings twice closes it.
 *
 * Never call this from an event handler. An event that fires per keystroke —
 * the command line, the picker — would toggle the overlay off on every second
 * key, which looks exactly like the panel flickering. Use `show`.
 */
function open(id) {
  const wasOpen = $("#" + id).classList.contains("on");
  closeAll();
  if (wasOpen) return;
  show(id);
}

/** Open an overlay, idempotently. Safe to call on every keystroke. */
function show(id) {
  closeAll();
  $("#" + id).classList.add("on");
  $("#scrim").classList.add("on");
}

$$("[data-open]").forEach((el) => el.addEventListener("click", () => open(el.dataset.open)));
$$("[data-close]").forEach((el) => el.addEventListener("click", closeAll));
$("#scrim").addEventListener("click", closeAll);

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
const panes = new Map();

function paneState(index) {
  if (!panes.has(index)) {
    panes.set(index, {
      firstLine: 0,
      lines: [],
      totalLines: 0,
      cursor: { line: 0, column: 0 },
      /// The far end of the selection, or null. The near end is the cursor.
      selection: null,
      /// Coloured runs, parallel to `lines` — empty for a file with no grammar.
      highlights: [],
      // Search hits per line. A separate layer from `highlights`: a match is a
      // background and a scope is a colour, and a match inside a string has to
      // stay string-coloured.
      matches: [],
      // Git marks per line — added, modified, or lines removed here.
      git: [],
      // Breakpoints on visible lines, keyed by line number. A map because
      // there are usually none and a screenful of nulls to say so is waste.
      stops: new Map(),
    });
  }
  return panes.get(index);
}

/**
 * The pane being drawn right now.
 *
 * An ambient rather than a parameter threaded through `lineHtml`,
 * `selectedOn` and `runsHtml` — they already read the view they are drawing
 * from a module-level object, and this is that object, now one per split.
 */
let drawing = paneState(0);

/** The focused pane's state, for the statusline and the completion popup. */
const focusedPane = () => paneState(view.focused);

/* Quotes are escaped as well as the angle brackets, because most of what this
   wraps ends up inside an attribute — a `title`, a `data-df-cell`, an
   `<option value>`. A value holding a `"` closes the attribute early and the
   rest of it becomes markup: a scope like `:root[data-theme="dark"]` arrived
   at the browser as `:root[data-theme=`, and a dataframe cell containing a
   quote broke the click that searches for it. Harmless in text content, where
   `&quot;` renders as the quote it is. */
const escapeHtml = (s) =>
  String(s).replace(
    /[&<>"']/g,
    (c) =>
      ({
        "&": "&amp;",
        "<": "&lt;",
        ">": "&gt;",
        '"': "&quot;",
        "'": "&#39;",
      })[c],
  );

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
   The sprite lives in index.html; this is how the rendered parts of the
   app reach it. `class="i"` is what makes a `<use>` draw at all — see the
   comment on that rule in styles.css. */
const icon = (id, cls = "") =>
  `<svg class="i${cls ? " " + cls : ""}"><use href="#i-${id}"/></svg>`;

/* Diagnostic signs are solid and told apart by silhouette as much as by
   colour — a dot, a triangle, a square and a bar stay distinct for a
   reader who cannot tell red from amber. */
const SIGN = { error: "d-error", warning: "d-warning", info: "d-info", hint: "d-hint" };

/* What each file gets in the buffer bar. A table rather than a switch
   because it is read far more often than it is changed; the static
   explorer markup in index.html mirrors it. */
const FILE_ICONS = [
  [/\.rs$/i, "file-code", "ft-rust"],
  [/\.(tsx?|mts|cts)$/i, "file-code", "ft-ts"],
  [/\.(jsx?|mjs|cjs)$/i, "file-code", "ft-js"],
  [/\.(py|pyi)$/i, "file-code", "ft-py"],
  [/\.go$/i, "file-code", "ft-go"],
  [/\.(c|h|cpp|hpp|cc)$/i, "file-code", "ft-c"],
  [/\.(gd|lua|rb|sh|ps1)$/i, "file-code", "ft-script"],
  [/\.(css|scss)$/i, "file-code", "ft-css"],
  [/\.(html?|svelte|vue)$/i, "file-code", "ft-html"],
  [/\.(json|jsonc)$/i, "sliders", "ft-data"],
  [/\.(toml|ya?ml|ini|cfg|conf)$/i, "sliders", "ft-config"],
  [/\.lock$/i, "sliders", "ft-lock"],
  [/\.(md|mdx|txt|rst)$/i, "file-doc", "ft-doc"],
  [/\.(png|jpe?g|gif|webp|svg|ico|bmp)$/i, "image", "ft-image"],
  [/\.(wav|mp3|ogg|flac|aac)$/i, "audio", "ft-audio"],
  [/\.(mp4|mov|mkv|webm)$/i, "film", "ft-video"],
];

function fileIcon(name) {
  const [, id, tint] = FILE_ICONS.find(([re]) => re.test(name)) ?? [0, "file", "ft-plain"];
  return icon(id, tint);
}

/** Wrap the diagnostic spans on one line, innermost work on a character array. */
function markUp(chars, line) {
  const here = marks.byLine.get(line);
  if (!here?.length) return null;

  // A per-character severity, so overlapping diagnostics resolve by
  // seriousness rather than by whichever the server happened to send last.
  const RANK = { error: 3, warning: 2, info: 1, hint: 0 };
  const worst = new Array(chars.length).fill(null);
  for (const d of here) {
    const from = d.line === line ? d.column : 0;
    // A diagnostic can span lines; on every line but its last it runs to the
    // end. A zero-width range still gets one character, or it draws nothing.
    const to = d.endLine === line ? Math.max(d.endColumn, from + 1) : chars.length;
    for (let i = from; i < Math.min(to, chars.length); i++) {
      if (!worst[i] || RANK[d.severity] > RANK[worst[i]]) worst[i] = d.severity;
    }
  }
  return worst;
}

/* Scope names from the core to the theme's class names. The core says what a
   run *is*; the stylesheet decides what that looks like, and neither knows
   the other's vocabulary. */
const SCOPE = {
  keyword: "k", function: "fn", type: "ty", string: "str",
  number: "num", comment: "cm", constant: "cn", variable: "va",
  property: "pr", operator: "op", punctuation: "pn", attribute: "at",
  // Prose. Two of these are a weight and a slope rather than a colour,
  // because that is how the text itself says them.
  heading: "md-h", emphasis: "md-em", strong: "md-b", link: "md-a",
};

/// Emit `[from, to)` as merged runs of identical colour and severity.
///
/// Merged, because one element per character is a few thousand nodes on a
/// wide line and the browser feels every one of them. Two channels at once,
/// because a keyword inside a diagnostic needs to be both coloured and
/// underlined and splitting them would need twice the elements.
function runsHtml(chars, scope, severity, picked, hit, from, to) {
  const same = (i, j) =>
    (scope?.[i] ?? null) === (scope?.[j] ?? null) &&
    (severity?.[i] ?? null) === (severity?.[j] ?? null) &&
    Boolean(picked?.[i]) === Boolean(picked?.[j]) &&
    Boolean(hit?.[i]) === Boolean(hit?.[j]);

  let html = "";
  let i = from;
  while (i < to) {
    let end = i + 1;
    while (end < to && same(i, end)) end++;

    const text = escapeHtml(chars.slice(i, end).join(""));
    const classes = [];
    const colour = scope?.[i];
    if (colour && SCOPE[colour]) classes.push(SCOPE[colour]);
    if (severity?.[i]) classes.push("sq", "d-" + severity[i]);
    if (picked?.[i]) classes.push("sel");
    // Last, so a match reads over a scope rather than under it — the point of
    // painting it is that you can find it.
    if (hit?.[i]) classes.push("hit");
    html += classes.length ? `<span class="${classes.join(" ")}">${text}</span>` : text;
    i = end;
  }
  return html;
}

/* Which characters on a line are selected.
 *
 * Three shapes, and they are genuinely different questions. Charwise runs
 * from one point to another and only the first and last lines are partial.
 * Linewise takes whole lines. A block takes the same columns from every line,
 * which is the one shape a range of offsets cannot describe — and the reason
 * the mode has to know which of the three it is rather than just holding two
 * endpoints. */
function selectedOn(line, width) {
  const anchor = drawing.selection;
  if (!anchor) return null;
  const head = drawing.cursor;

  const top = anchor.line <= head.line ? anchor : head;
  const bottom = anchor.line <= head.line ? head : anchor;
  if (line < top.line || line > bottom.line) return null;

  if (view.vim === "V-LINE") return [0, width];
  if (view.vim === "V-BLOCK") {
    const left = Math.min(anchor.column, head.column);
    const right = Math.max(anchor.column, head.column) + 1;
    return [Math.min(left, width), Math.min(right, width)];
  }
  const from = line === top.line ? top.column : 0;
  const to = line === bottom.line ? bottom.column + 1 : width;
  return [Math.min(from, width), Math.min(to, width)];
}

function lineHtml(number, text, cursorColumn, highlights, hits, git, stop) {
  // A breakpoint takes the sign column: it is a thing you put there, and a
  // diagnostic is a thing that happened. Yours wins.
  const here = marks.byLine.get(number);
  const sign = stop
    ? `<span class="s bp${stop.verified ? "" : " unset"}${stop.here ? " at" : ""}"></span>`
    : here?.length
    ? `<span class="s d-${here[0].severity}">${icon(SIGN[here[0].severity], "solid")}</span>`
    : `<span class="s"></span>`;
  // A thin bar between the number and the diagnostic sign. Its own strip
  // rather than sharing one, because a line can be both changed and broken
  // and you need to see both.
  const change = git ? `<span class="g ${git}"></span>` : `<span class="g"></span>`;
  const gutter = `<span class="n">${number + 1}</span>${change}${sign}`;

  const chars = [...text];
  const severity = markUp(chars, number);

  // Spans are character columns, already merged and non-overlapping by the
  // time they arrive, so this only has to expand them.
  const scope = new Array(chars.length).fill(null);
  for (const span of highlights ?? []) {
    for (let i = span.start; i < Math.min(span.end, chars.length); i++) {
      scope[i] = span.scope;
    }
  }

  // Search hits, expanded the same way. `null` when nothing is being
  // searched for, so the merge below has nothing to compare and no cost.
  let hit = null;
  if (hits?.length) {
    hit = new Array(chars.length).fill(false);
    for (const h of hits) {
      for (let i = h.start; i < Math.min(h.end, chars.length); i++) hit[i] = true;
    }
  }

  const range = selectedOn(number, chars.length);
  const picked = range && new Array(chars.length).fill(false);
  if (range) {
    for (let i = range[0]; i < range[1]; i++) picked[i] = true;
  }
  // A selected empty line has nothing to paint, so the row says so instead.
  const empty = range && chars.length === 0 ? " selected" : "";

  // The text goes inside one element, not straight into the row.
  //
  // The row is a flex container so the gutter columns line up, and a flex
  // container **drops text nodes that are only whitespace** — they never
  // become anonymous flex items. Highlighting cuts a line into a span per
  // token with bare spaces between them, so `from typing import List` laid
  // out as flex items reads `fromtypingimportList`. One element around the
  // whole line makes its interior ordinary inline flow again.
  if (cursorColumn === undefined) {
    const body = runsHtml(chars, scope, severity, picked, hit, 0, chars.length);
    return `<div class="ln${empty}">${gutter}<span class="tx">${body}</span></div>`;
  }
  // The caret sits *on* a character in normal mode, so it takes the one it
  // covers with it rather than being drawn between two.
  const under = escapeHtml(chars[cursorColumn] ?? " ") || " ";
  return (
    `<div class="ln cur${empty}">${gutter}<span class="tx">` +
    `${runsHtml(chars, scope, severity, picked, hit, 0, cursorColumn)}` +
    `<span class="caret">${under}</span>` +
    `${runsHtml(chars, scope, severity, picked, hit, cursorColumn + 1, chars.length)}` +
    `</span></div>`
  );
}

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
  const line = $("[data-editor] .ln");
  const measured = line?.getBoundingClientRect().height;
  if (measured > 0) lineHeight = measured;
  else {
    const target = $("[data-editor]");
    if (target) lineHeight = parseFloat(getComputedStyle(target).lineHeight) || 20;
  }
  return lineHeight;
}

function renderEditor(index = view.focused) {
  const target = $(`[data-editor][data-view="${index}"]`);
  if (!target) return;
  // The ambient the drawing functions read. Set for the length of this call
  // and never left pointing at a pane nobody is drawing.
  drawing = paneState(index);
  // Measured before building, because the spacer's height depends on it. With
  // no line rendered yet this falls back to the pane's computed line-height,
  // which is the same number.
  measureLineHeight();

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
  dragging = true;

  // The core clamps: a click past the end of a short line is the ordinary
  // case, and the front end does not hold the document to check against.
  const place = () => dispatch({ type: "placeCursor", ...at });

  // Clicking into a different split focuses it *first*. `PlaceCursor` moves
  // the focused view's cursor, so the other order would put the cursor in the
  // pane you just left.
  const index = viewOf(e.target);
  if (index !== view.focused) {
    dispatch({ type: "focusSplit", index }).then(place);
  } else {
    place();
  }
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
  $$("[data-buffers]").forEach((target) => {
    const index = viewOf(target);
    // The pane's own buffer when the layout has said, the editor's active one
    // until it has — which is the case with a single pane and no splits.
    const mine = bufferBar.panes[index] ?? bufferBar.active;
    target.innerHTML = bufferBar.names
      .map(
        (b, i) =>
          `<span class="buf${i === mine ? " on" : ""}" data-buffer="${i}">` +
          `${fileIcon(b.name)} ${escapeHtml(b.name)}` +
          // The unsaved dot and the close button share a slot, as they do in
          // every tab bar: two glyphs competing for the same corner reads as
          // clutter, and hovering is exactly when you stop needing the dot.
          `${b.modified ? '<span class="mod"></span>' : ""}` +
          `<span class="x" data-buffer-close="${i}" title="Close ${escapeHtml(b.name)}">` +
          `<svg class="i"><use href="#i-close"/></svg></span></span>`,
      )
      .join("");

    // The strip scrolls rather than wrapping, so the tab you just switched to
    // has to be brought into view — otherwise `:bn` past the edge looks like
    // it did nothing. Only the focused pane, or three of them fight over it.
    if (index === view.focused) {
      target.querySelector(".buf.on")?.scrollIntoView({ block: "nearest", inline: "nearest" });
    }
  });
}

/* ── the file tree ────────────────────────────────────────────────────
   The core owns it: what is in it, which folders are open, and which row
   is the file you are looking at. This draws rows and sends clicks back —
   it does not read a directory or decide what to hide, because then the
   tree and `␣ff` could disagree about what is in the project. */

function renderTree(rows) {
  // Every mode with an explorer, not just the one on screen: they draw the
  // same tree from the same event, and a mode that only rendered while
  // visible would come up empty on the first switch into it.
  const targets = $$("[data-tree]");
  if (!targets.length) return;

  const html = rows
    .map((row) => {
      // Indentation is inline rather than a class per level, because a class
      // per level puts a ceiling on how deep a project may be nested.
      const pad = `padding-left:${8 + row.depth * 13}px`;

      if (row.truncated) {
        return `<div class="row more" style="${pad}">${escapeHtml(row.name)}</div>`;
      }
      if (row.dir) {
        return (
          `<div class="row dir${row.expanded ? " open" : ""}" style="${pad}"` +
          ` data-tree-dir="${escapeHtml(row.path)}">` +
          `<span class="ic"><svg class="i chev"><use href="#i-chev"/></svg></span>` +
          `<svg class="i ft-folder"><use href="#i-${row.expanded ? "folder-open" : "folder"}"/></svg>` +
          ` ${escapeHtml(row.name)}</div>`
        );
      }
      return (
        `<div class="row${row.active ? " sel" : ""}" style="${pad}"` +
        ` data-path="${escapeHtml(row.path)}">` +
        `<span class="ic">${fileIcon(row.name)}</span>${escapeHtml(row.name)}</div>`
      );
    })
    .join("");

  for (const target of targets) {
    target.innerHTML = html;
    target.querySelector(".row.sel")?.scrollIntoView({ block: "nearest" });
  }
}

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
let lastNotebook = { path: "", cells: [], problem: "" };

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

    case "image":
      // A data URL, which is the one place a kernel's bytes reach the page —
      // as an image, where they can only ever be an image.
      return `<img class="out-img" alt="output" src="data:${encodeURIComponent(out.mime)};base64,${out.base64.replace(/[^A-Za-z0-9+/=]/g, "")}">`;

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

/** Markdown, for a cell that holds prose rather than code.
 *
 * A small subset on purpose — headings, emphasis, code, links, lists, rules —
 * because a notebook is a file somebody downloaded and a full renderer is a
 * large surface to point at one. **Everything is escaped first and markup is
 * applied to the escaped text**, so nothing in the document can become a tag:
 * the same rule the dataframe table follows.
 *
 * A link's text is kept but its target is dropped. A notebook cell offering a
 * clickable URL into the editor's own window is not something this needs to
 * support to be useful.
 */
function markdownHtml(source) {
  const inline = (text) =>
    escapeHtml(text)
      .replace(/`([^`]+)`/g, "<code>$1</code>")
      .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
      .replace(/(^|\W)\*([^*]+)\*/g, "$1<em>$2</em>")
      .replace(/(^|\W)_([^_]+)_/g, "$1<em>$2</em>")
      .replace(/\[([^\]]+)\]\([^)]*\)/g, "<u>$1</u>");

  const out = [];
  let list = false;
  const closeList = () => {
    if (list) out.push("</ul>");
    list = false;
  };

  for (const line of source.split("\n")) {
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    const item = /^\s*[-*+]\s+(.*)$/.exec(line);

    if (heading) {
      closeList();
      const level = Math.min(heading[1].length, 6);
      out.push(`<h${level}>${inline(heading[2])}</h${level}>`);
    } else if (item) {
      if (!list) out.push("<ul>");
      list = true;
      out.push(`<li>${inline(item[1])}</li>`);
    } else if (/^\s*(---+|\*\*\*+)\s*$/.test(line)) {
      closeList();
      out.push("<hr>");
    } else if (line.trim() === "") {
      closeList();
    } else {
      closeList();
      out.push(`<p>${inline(line)}</p>`);
    }
  }
  closeList();
  return out.join("");
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
      ? `<div class="cell-md" data-md="${index}" title="Double-click to edit">${markdownHtml(cell.source)}</div>`
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
      source: src.textContent,
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
  const src = e.target.closest?.("[data-src]");
  if (!src) return;
  const index = Number(src.dataset.src);

  if (e.key === "Escape") {
    e.preventDefault();
    dispatch({ type: "setCell", index, source: src.textContent });
    src.blur();
    // The core still thinks the engine is in insert mode, and only a key
    // it sees will change that.
    sendKey(e);
    return;
  }
  if (e.key === "Enter" && e.shiftKey) {
    e.preventDefault();
    dispatch({ type: "setCell", index, source: src.textContent });
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
      event.problem || "No theme to render.",
    )}</div>`;
    board.removeAttribute("style");
    artboardPainted = null;
    return;
  }

  const palette = resolveScope(event.tokens, artboardScope);
  const key = JSON.stringify([[...palette], artboardWidth]);
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
    box.innerHTML = `<div class="empty">Open a .ipynb to start</div>`;
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

/** Give the terminal the keyboard, or take it back. */
function focusTerminal(on) {
  terminalFocused = on;
  $("[data-term-screen]")?.classList.toggle("on", on);
}

function runHtml(run, row, column, cursor) {
  const classes = [];
  if (run.fg != null) classes.push(`tf${run.fg}`);
  if (run.bg != null) classes.push(`tb${run.bg}`);
  if (run.bold) classes.push("tbold");
  if (run.italic) classes.push("titalic");
  if (run.underline) classes.push("tunder");
  if (run.inverse) classes.push("tinv");
  // A true colour has no palette name, so it goes inline. Nothing else does.
  const style = run.rgb ? ` style="color:${run.rgb}"` : "";
  const attrs = `${classes.length ? ` class="${classes.join(" ")}"` : ""}${style}`;
  return attrs ? `<span${attrs}>${escapeHtml(run.text)}</span>` : escapeHtml(run.text);
}

function renderTerminal(event) {
  const state = $("[data-term-state]");
  if (state) {
    state.textContent = event.running ? "ready" : event.rows.length ? "exited" : "starting";
    state.className = "chip" + (event.running ? " accent" : "");
  }
  // A shell you exited out of is the one case where there is something to
  // press. While one is running there is nothing to do, so nothing is shown.
  const restart = $("[data-term-restart]");
  if (restart) restart.hidden = event.running || !event.rows.length;

  const back = $("[data-term-back]");
  if (back) {
    back.hidden = !event.scrollback;
    if (event.scrollback) $("[data-term-back-rows]").textContent = String(event.scrollback);
  }

  const screen = $("[data-term-screen]");
  if (!screen) return;
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
  return $("[data-term-screen]")?.querySelectorAll(".trow").length || 24;
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
function reportTerminalSize() {
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
if (termScreen) new ResizeObserver(reportTerminalSize).observe(termScreen);

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
  const rail = $("[data-workspaces]");
  if (!rail) return;
  rail.innerHTML = items
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
  const target = $(`[data-editor][data-view="${index}"]`);
  if (!target) return;
  const first = Math.max(0, Math.floor(target.scrollTop / lineHeight));
  const rows = Math.max(1, Math.ceil(target.clientHeight / lineHeight) + OVERSCAN);
  dispatch({ type: "viewport", view: index, firstLine: first, lines: rows });
}

/** Every pane has its own height, and the core has to be told all of them. */
function reportEveryViewport() {
  $$("[data-editor]").forEach((pane) => reportViewport(Number(pane.dataset.view)));
}

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
  const pick = e.target.closest("[data-pick]");
  if (pick) {
    // Select then accept: the click says which row, and Enter means the same
    // thing however the row got chosen.
    dispatch({ type: "selectPick", index: Number(pick.dataset.pick) })
      .then(() => sendKey({ key: "Enter" }));
    return;
  }
  const candidate = e.target.closest("[data-complete]");
  if (candidate) {
    // Select then accept, exactly as the picker does: the click says which
    // row, and accepting means the same thing however the row got chosen.
    dispatch({ type: "selectCompletion", index: Number(candidate.dataset.complete) })
      .then(() => dispatch({ type: "acceptCompletion" }));
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
  const shutBuffer = e.target.closest("[data-buffer-close]");
  if (shutBuffer) {
    e.stopPropagation();
    dispatch({
      type: "closeBuffer",
      index: Number(shutBuffer.dataset.bufferClose),
      // Never forced from a click. Losing unsaved work must cost a deliberate
      // `:q!`, not a mis-aimed pointer.
      force: false,
    });
    return;
  }

  const folder = e.target.closest("[data-tree-dir]");
  if (folder) {
    dispatch({ type: "toggleTreeNode", path: folder.dataset.treeDir });
    return;
  }
  if (e.target.closest("[data-reveal]")) {
    dispatch({ type: "revealInTree" });
    return;
  }

  const row = e.target.closest("[data-path]");
  if (row) dispatch({ type: "openBuffer", path: row.dataset.path });
  const tab = e.target.closest("[data-buffer]");
  if (tab) dispatch({ type: "focusBuffer", index: Number(tab.dataset.buffer) });
});

/* ── telescope ────────────────────────────────────────────────────────
   The core owns the picker: it holds the query, does the matching and
   decides what a row opens. This draws the answer and nothing else —
   including the matched character positions, so the highlight can never
   disagree with the score that produced the ordering. */

let pickerOpen = false;

function highlight(label, positions) {
  if (!positions?.length) return escapeHtml(label);
  const hit = new Set(positions);
  return [...label]
    .map((c, i) => (hit.has(i) ? `<mark>${escapeHtml(c)}</mark>` : escapeHtml(c)))
    .join("");
}

function renderPicker(event) {
  pickerOpen = true;
  show("telescope");

  $("[data-picker-query]").textContent = event.query;
  $("[data-picker-title]").textContent = event.title;
  // An empty list while the tree is still being walked means "not yet", not
  // "no matches". Saying so is the difference between waiting a beat and
  // deciding the file isn't there.
  $("[data-picker-count]").textContent = event.scanning
    ? "scanning…"
    : `${event.items.length}/${event.total} · ${event.elapsedMs.toFixed(1)} ms`;

  // A folder prompt lists folders, and a row that looks like a file in a list
  // of projects is a row you have to read twice.
  const leader =
    event.kind === "workspaces" ? '<svg class="i"><use href="#i-folder"/></svg> ' : "";

  const results = $("[data-picker-results]");
  results.innerHTML = event.items
    .map((item, i) => {
      // A symbol row says what kind of thing it is, using the marks the
      // completion popup already teaches — a second alphabet for the same
      // nine categories would be one to learn twice.
      const mark = item.kind
        ? `<span class="k">${icon(KIND_ICON[item.kind] ?? "k-other")}</span>`
        : leader;
      return (
        `<div class="res${i === event.selected ? " on" : ""}" data-pick="${i}">` +
        `<span>${mark}${highlight(item.label, item.positions)}</span>` +
        `<span class="dir" style="margin-left:auto">${escapeHtml(item.detail)}</span></div>`
      );
    })
    .join("");

  results.querySelector(".res.on")?.scrollIntoView({ block: "nearest" });
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

onEvent((event) => {
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
      }
      break;
    }

    case "action":
      renderStatus(event.description);
      break;

    case "unsupported":
      // The mode has no meaning for this verb, and says so. Silently doing
      // nothing is the failure users cannot debug.
      renderStatus(`${event.description} — not in ${event.mode}`);
      break;

    case "input":
      renderStatus(`insert ${JSON.stringify(event.text)}`);
      break;

    case "viewport": {
      const pane = paneState(event.view);
      pane.firstLine = event.firstLine;
      pane.lines = event.lines;
      pane.totalLines = event.totalLines;
      pane.highlights = event.highlights ?? [];
      pane.matches = event.matches ?? [];
      pane.git = event.git ?? [];
      pane.stops = new Map((event.stops ?? []).map((s) => [s.line, s]));
      renderEditor(event.view);
      break;
    }

    case "cursor": {
      const pane = paneState(event.view);
      pane.cursor = { line: event.line, column: event.column };
      pane.selection = event.selection;
      renderEditor(event.view);
      // The statusline speaks for the split you are in, not whichever one
      // happened to redraw.
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

    case "action":
      // `i`, `a`, `o` and `O` in Data mode put the caret in a cell. The core
      // has already switched the engine to insert and, for `o`/`O`, added the
      // cell; this is the half it cannot do.
      if (activeMode === "data" && event.action.verb === "Insert") {
        requestAnimationFrame(() => enterCell(event.action.object));
      }
      break;

    case "settings":
      renderSettings(event);
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
      // knows once it has a row to measure.
      requestAnimationFrame(reportTerminalSize);
      break;

    case "workspaces":
      renderWorkspaces(event.items, event.active);
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
      renderStatus(event.text, event.level);
      break;
  }
});

function renderPosition() {
  const el = $("#sl-position");
  const at = focusedPane().cursor;
  if (el) el.textContent = `ln ${at.line + 1}, col ${at.column + 1}`;
}

/* ── keys ─────────────────────────────────────────────────────────────
   Two layers, and the split matters. Shell chords (⌘K, ⌘,, ⌃1-7) are the
   window's own bindings and never reach the core. Everything else is a
   keystroke the core interprets — including the digits, which are vim
   counts, not mode switches. */
document.addEventListener("keydown", (e) => {
  // The terminal takes the whole keyboard while it has the focus, exactly as
  // the picker does — `d` types a `d` at the shell rather than starting an
  // operator, and `⌃c` interrupts a program rather than doing nothing.
  // A document is over the code, so Escape closes it rather than reaching the
  // modal engine underneath — which is what Escape means over anything else
  // that covers the stage.
  if (documentIsOpen() && e.key === "Escape") {
    e.preventDefault();
    closeDocument();
    return;
  }

  if (terminalFocused) {
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

  const meta = e.metaKey || e.ctrlKey;

  if (meta && /^[1-7]$/.test(e.key)) {
    e.preventDefault();
    coreSetMode(MODES[Number(e.key) - 1]);
    return;
  }

  if (meta && e.key.toLowerCase() === "k") {
    e.preventDefault();
    if (HAS_SHELL) dispatch({ type: "openPicker", kind: "files" });
    else open("telescope"); // browser preview has no file system
    return;
  }

  if (meta && e.key === ",") {
    e.preventDefault();
    open("settings");
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

  if (meta) return; // leave the rest of the browser/OS chords alone

  // The picker owns the keyboard while it is open: every key goes to the core
  // and none of the page-level bindings fire.
  if (pickerOpen) {
    e.preventDefault();
    sendKey(e);
    return;
  }

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

try {
  const theme = localStorage.getItem("proton.theme");
  if (theme) setTheme(theme);
  setTranslucency(localStorage.getItem("proton.translucency") ?? "medium");
  setHint(localStorage.getItem("proton.hint") === "collapsed");
} catch {
  /* nothing stored, or storage is blocked — the defaults stand */
  setTranslucency("medium");
}

wireWindowChrome();
document.documentElement.classList.toggle("shell", HAS_SHELL);
/* The backdrop picker reports what the compositor *did*, not what was asked
   for. Windows turns Mica off under battery saver or with transparency effects
   disabled, and the setting has to be honest when that happens. */
function renderBackdrop(applied, wanted) {
  $$("[data-backdrop-set]").forEach((b) =>
    b.classList.toggle("on", b.dataset.backdropSet === applied),
  );
  const note = $("#backdrop-note");
  if (!note) return;
  if (!HAS_SHELL) {
    note.textContent = "Browser preview — the page paints its own ground here.";
  } else if (wanted && wanted !== applied) {
    note.textContent = `${wanted} is unavailable — Windows is compositing "${applied}" instead. `
      + "Check Settings › Personalisation › Colours › Transparency effects.";
  } else {
    note.textContent = "";
  }
}

$$("[data-backdrop-set]").forEach((b) =>
  b.addEventListener("click", async () => {
    const wanted = b.dataset.backdropSet;
    renderBackdrop(await setBackdrop(wanted), wanted);
    requestAnimationFrame(drawPointerEdges);
  }),
);

applyBackdrop().then((applied) => {
  renderBackdrop(applied);
  requestAnimationFrame(drawPointerEdges);
});
sync();
drawPointerEdges();

// The stage measures itself, tells the core how many lines it can show, and
// opens a real file — M1's exit criterion is Proton editing its own source.
//
// Not inside requestAnimationFrame: rAF does not fire while the window is
// hidden, so booting there means an app started minimised never loads
// anything and shows an empty editor when you finally look at it.
function boot() {
  reportViewport();
  dispatch({ type: "openBuffer", path: "crates/proton-core/src/vim.rs" });
}

if (document.readyState === "complete") boot();
else window.addEventListener("load", boot, { once: true });

new ResizeObserver(reportViewport).observe(document.body);

// Exposed for the browser preview's console and for driving screenshots.
// Splitting is a command now, not a function here — `dispatch` covers it.
Object.assign(window, {
  setTheme, setTranslucency, setMode: coreSetMode, setDock, open, closeAll,
  dispatch, drawPointerEdges, setHint, enterCell,
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
  diagnostics: "d-warning",
};

const CALL_STATE = { starting: "", running: "spin", done: "", failed: "" };

function toolHtml(call) {
  const spin = CALL_STATE[call.state] === "spin" ? " spin" : "";
  const mark = TOOL_ICON[call.name] ?? "k-other";
  const right =
    call.state === "failed"
      ? `<span class="t" style="color:var(--err)">${escapeHtml(call.result)}</span>`
      : call.result
        ? `<span class="t">${escapeHtml(call.result)}</span>`
        : "";
  return (
    `<div class="tool"><svg class="i${spin}"><use href="#i-${mark}"/></svg> ` +
    `<b>${escapeHtml(call.name)}</b> ${escapeHtml(call.detail)}${right}</div>`
  );
}

/** Minimal inline markup: backticks and paragraphs. Nothing more — this is a
 *  side panel, not a document viewer. */
function proseHtml(text) {
  return escapeHtml(text)
    .split(/\n{2,}/)
    .filter((p) => p.trim())
    .map((p) => `<p>${p.replace(/`([^`]+)`/g, "<code>$1</code>").replace(/\n/g, "<br>")}</p>`)
    .join("");
}

function entryHtml(entry) {
  const you = entry.speaker === "you";
  const who = you
    ? `<svg class="i" style="color:var(--accent)"><use href="#i-proton"/></svg> You`
    : `<svg class="i ft-ai"><use href="#i-spark"/></svg> Proton · Opus 5`;

  const thinking = entry.thinking
    ? `<details class="thought"><summary>thought</summary>${proseHtml(entry.thinking)}</details>`
    : "";
  const calls = entry.calls.map(toolHtml).join("");
  // A caret while the text is still arriving, so an empty answer that is on
  // its way does not look like an empty answer that arrived.
  const caret = entry.streaming ? `<span class="cursor"></span>` : "";

  return (
    `<div class="msg"><div class="who">${who}</div>` +
    `${thinking}${calls}${proseHtml(entry.text)}${caret}</div>`
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

function renderAgent(event) {
  const thread = $("[data-agent-thread]");
  if (thread && event.transcript.length) {
    const atBottom = thread.scrollHeight - thread.scrollTop - thread.clientHeight < 60;
    thread.innerHTML = event.transcript.map(entryHtml).join("");
    // Follow the answer only if the reader was already at the bottom —
    // yanking someone back down while they are reading is worse than
    // making them scroll.
    if (atBottom) thread.scrollTop = thread.scrollHeight;
  }

  // Which service answered. Worth a permanent chip rather than a setting you
  // have to go and look up: the two give different answers to the same
  // question, and a transcript you cannot attribute is one you cannot reason
  // about a week later.
  const who = $("[data-agent-provider]");
  if (who) {
    who.textContent = event.provider === "gemini" ? "Gemini" : "Claude";
    who.title = `${event.model} — change it in Settings > AI & models`;
    who.className = "chip" + (event.provider === "gemini" ? " ok" : " accent");
  }

  const box = $(".composer .box");
  if (box) box.classList.toggle("busy", event.activity !== "idle");

  const send = $("[data-agent-send]");
  if (send) {
    send.classList.toggle("stop", event.activity !== "idle");
    send.setAttribute("aria-label", event.activity === "idle" ? "Send" : "Stop");
    send.innerHTML = `<svg class="i"><use href="#i-${
      event.activity === "idle" ? "arrow-up" : "stop"
    }"/></svg>`;
  }

  const meter = $("#sl-agent");
  if (meter) {
    // The cached figure earns its place: a prefix that stops matching is a
    // cost regression rather than an error, and nothing else reports it.
    meter.innerHTML = event.inputTokens
      ? `<svg class="i ft-ai"><use href="#i-spark"/></svg> ${fmt(event.inputTokens)} in · ` +
        `${fmt(event.outputTokens)} out · <span class="cached">${fmt(event.cachedTokens)} cached</span>`
      : `<svg class="i ft-ai"><use href="#i-spark"/></svg> Opus 5 · agent`;
  }
}

const fmt = (n) => (n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n));

function askAgent() {
  const input = $("[data-agent-input]");
  if (!input) return;
  const text = input.textContent.trim();
  if (!text) return;
  input.textContent = "";
  dispatch({ type: "ask", text });
}

document.addEventListener("click", (e) => {
  if (!e.target.closest("[data-agent-send]")) return;
  // The same button stops a turn that is running — the thing you want when
  // an answer is going the wrong way is always in the same place.
  const busy = $("[data-agent-send]")?.classList.contains("stop");
  if (busy) dispatch({ type: "cancelTurn" });
  else askAgent();
});

// Enter sends, Shift-Enter makes a newline. The composer is not the buffer,
// so these keys never reach the modal engine.
$("[data-agent-input]")?.addEventListener("keydown", (e) => {
  e.stopPropagation();
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    askAgent();
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

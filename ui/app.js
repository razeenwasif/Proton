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

const $  = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];

const token = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();

/* ── theme ────────────────────────────────────────────────────────────
   A theme sets sixteen custom properties on <html> and nothing else. No
   rule below the palette block knows which one is active, which is why
   switching is instant and why adding a fifth is a paste, not a port. */
const THEMES = ["tokyo-night", "one-dark", "catppuccin", "nord"];

function setTheme(name) {
  if (!THEMES.includes(name)) return;
  document.documentElement.dataset.theme = name;
  $$("[data-theme-set]").forEach((c) => c.classList.toggle("on", c.dataset.themeSet === name));
  try {
    localStorage.setItem("proton.theme", name);
  } catch {
    /* private window, blocked storage — the theme just doesn't persist */
  }
  requestAnimationFrame(drawPointerEdges); // edges are painted, not tokenised
}

$$("[data-theme-set]").forEach((c) =>
  c.addEventListener("click", () => setTheme(c.dataset.themeSet)),
);

/* ── translucency ─────────────────────────────────────────────────────
   Three steps over one set of veil percentages. Like the theme, it is a
   token change: no rule below the veil block knows which step is live. */
function setTranslucency(step) {
  if (!["subtle", "medium", "heavy"].includes(step)) return;
  document.documentElement.dataset.translucency = step;
  $$("[data-translucency-set]").forEach((b) =>
    b.classList.toggle("on", b.dataset.translucencySet === step),
  );
  try {
    localStorage.setItem("proton.translucency", step);
  } catch {
    /* storage blocked — it just won't persist */
  }
}

$$("[data-translucency-set]").forEach((b) =>
  b.addEventListener("click", () => setTranslucency(b.dataset.translucencySet)),
);

/* ── workspace modes ──────────────────────────────────────────────────
   The mode bar asks the core to switch; the core's ModeChanged event is
   what actually moves the UI. That indirection looks pointless with one
   front end and is the reason a second one is possible. */
const MODES = ["chat", "codex", "design", "modeling", "game", "video", "data"];

function renderMode(mode) {
  $$(".mode").forEach((b) => b.setAttribute("aria-selected", String(b.dataset.mode === mode)));
  $$(".workspace").forEach((w) => w.classList.toggle("on", w.dataset.ws === mode));
  closeAll();
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
  return $("#codex-splits");
}

/** The markup of one pane. Everything in it is addressed by view index. */
function splitHtml(view, focused) {
  return (
    `<section class="split${view === focused ? " on" : ""}" data-view="${view}">` +
    `<div class="buffers">` +
    `<span data-buffers></span>` +
    `<span class="split-actions">` +
    `<button data-split="v" title="Split right — :vsplit or ␣wv"><svg class="i"><use href="#i-split-v"/></svg></button>` +
    `<button data-split="s" title="Split down — :split or ␣ws"><svg class="i"><use href="#i-split-h"/></svg></button>` +
    `<button data-split="close" title="Close this split — :close or ␣wq"><svg class="i"><use href="#i-close"/></svg></button>` +
    `</span></div>` +
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

const escapeHtml = (s) =>
  s.replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);

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
  const rows = drawing.lines
    .map((text, i) =>
      lineHtml(
        drawing.firstLine + i,
        text,
        i === cursorRow ? drawing.cursor.column : undefined,
        drawing.highlights[i],
        drawing.matches[i],
        drawing.git[i],
        drawing.stops.get(drawing.firstLine + i),
      ),
    )
    .join("");

  // A spacer the height of the whole file, with the loaded window positioned
  // inside it. `translate` rather than `top` so moving the window is a
  // compositor job and never a reflow.
  target.innerHTML =
    `<div class="doc" style="height:${Math.max(1, drawing.totalLines) * lineHeight}px">` +
    `<div class="win" style="transform:translateY(${drawing.firstLine * lineHeight}px)">` +
    `${rows}</div></div>`;

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
  const target = $("[data-tree]");
  if (!target) return;

  target.innerHTML = rows
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

  target.querySelector(".row.sel")?.scrollIntoView({ block: "nearest" });
}

/* ── the terminal ─────────────────────────────────────────────────────
   The core owns the pseudo-terminal and the screen it paints. This draws
   rows of runs and sends keys back — it does not interpret an escape
   sequence or decide where the cursor is, because the thing holding the
   shell already knows. */

/** Whether the terminal has the keyboard. */
let terminalFocused = false;

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
    state.textContent = event.running ? "running" : event.rows.length ? "exited" : "closed";
    state.className = "chip" + (event.running ? " accent" : "");
  }

  const screen = $("[data-term-screen]");
  if (!screen) return;
  const [cursorRow, cursorCol] = event.cursor;

  screen.innerHTML = event.rows
    .map((row, i) => {
      const body = row.map((run) => runHtml(run)).join("");
      // The cursor is drawn as a class on its row plus a marker span, rather
      // than by splitting a run: the core said which cell it is on, and
      // cutting runs to find it would be re-deriving what it already sent.
      const here = event.cursorVisible && i === cursorRow;
      return `<div class="trow${here ? " tcur" : ""}"${
        here ? ` style="--tcol:${cursorCol}"` : ""
      }>${body || "&nbsp;"}</div>`;
    })
    .join("");
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

/** How many cells fit, so the shell can be told. */
function reportTerminalSize() {
  const screen = $("[data-term-screen]");
  if (!screen || !screen.offsetParent) return;
  const probe = screen.querySelector(".trow");
  const lineHeight = probe?.getBoundingClientRect().height || 17;
  // Measured from a real row rather than assumed: the font is the theme's.
  const charWidth = probe ? probe.getBoundingClientRect().width / Math.max(1, probe.textContent.length) : 7;
  const rows = Math.max(1, Math.floor(screen.clientHeight / lineHeight));
  const cols = Math.max(20, Math.floor(screen.clientWidth / Math.max(4, charWidth)));
  dispatch({ type: "terminalSize", rows, cols });
}

document.addEventListener("click", (e) => {
  const button = e.target.closest("[data-term]");
  if (button) {
    dispatch({ type: button.dataset.term === "open" ? "openTerminal" : "closeTerminal" });
    return;
  }
  // Clicking the screen gives it the keyboard; clicking anywhere else takes
  // it back, or every keystroke in the editor would go to the shell.
  terminalFocused = Boolean(e.target.closest("[data-term-screen]"));
  $("[data-term-screen]")?.classList.toggle("on", terminalFocused);
});

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
  if (terminalFocused) {
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
  dispatch, drawPointerEdges, setHint,
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

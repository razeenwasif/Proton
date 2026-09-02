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
   `:vsplit` shows the same buffer twice — two views onto one document —
   so cloning the pane is the honest model, not a shortcut.

   This is view state today. It becomes core state in M1, when buffers
   exist and a split has to mean "which buffer, scrolled where" rather
   than "another copy of the same markup". */
function splits() {
  return $("#codex-splits");
}

function activeSplit() {
  const box = splits();
  return box?.querySelector(".split.on") ?? box?.firstElementChild ?? null;
}

function syncSplits() {
  const box = splits();
  if (!box) return;
  const all = $$(".split", box);
  box.classList.toggle("multi", all.length > 1);
  if (!all.some((s) => s.classList.contains("on"))) all[0]?.classList.add("on");
  requestAnimationFrame(drawPointerEdges);
}

function splitEditor(direction) {
  const box = splits();
  const source = activeSplit();
  if (!box || !source || $$(".split", box).length >= 4) return;

  box.classList.toggle("stacked", direction === "stacked");
  const clone = source.cloneNode(true);
  source.classList.remove("on");
  clone.classList.add("on");
  source.after(clone);
  syncSplits();
}

function closeSplit(which = activeSplit()) {
  const box = splits();
  if (!box || !which || $$(".split", box).length < 2) return;
  const next = which.nextElementSibling ?? which.previousElementSibling;
  which.remove();
  next?.classList.add("on");
  syncSplits();
}

function onlySplit() {
  const box = splits();
  const keep = activeSplit();
  if (!box || !keep) return;
  $$(".split", box).forEach((s) => s !== keep && s.remove());
  syncSplits();
}

document.addEventListener("click", (e) => {
  const action = e.target.closest("[data-split]");
  if (action) {
    e.stopPropagation();
    const split = action.closest(".split");
    $$(".split", splits()).forEach((s) => s.classList.toggle("on", s === split));
    if (action.dataset.split === "v") splitEditor("side");
    else if (action.dataset.split === "s") splitEditor("stacked");
    else closeSplit(split);
    return;
  }
  // Clicking anywhere in a split focuses it.
  const split = e.target.closest(".split");
  if (split && splits()?.contains(split)) {
    $$(".split", splits()).forEach((s) => s.classList.toggle("on", s === split));
  }
});

/* The commands vim already uses for this, wired to the real command line. */
const SPLIT_COMMANDS = {
  vsplit: () => splitEditor("side"),
  vs: () => splitEditor("side"),
  vsp: () => splitEditor("side"),
  split: () => splitEditor("stacked"),
  sp: () => splitEditor("stacked"),
  // `:q` is not here any more — it closes the buffer, in the core. Splits are
  // still view state, so they keep `:close` and `␣wq`.
  close: () => closeSplit(),
  clo: () => closeSplit(),
  only: () => onlySplit(),
  on: () => onlySplit(),
};

const SPLIT_LEADER = {
  wv: () => splitEditor("side"),
  ws: () => splitEditor("stacked"),
  wq: () => closeSplit(),
  wo: () => onlySplit(),
};

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
  firstLine: 0,
  lines: [],
  totalLines: 0,
  cursor: { line: 0, column: 0 },
  /// The far end of the selection, or null. The near end is the cursor.
  selection: null,
  /// Which visual mode is live, because the same two endpoints describe a
  /// run, a set of lines or a rectangle depending on it.
  vim: "NORMAL",
  /// Coloured runs, parallel to `lines` — or empty for a file with no grammar.
  highlights: [],
};

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
};

/// Emit `[from, to)` as merged runs of identical colour and severity.
///
/// Merged, because one element per character is a few thousand nodes on a
/// wide line and the browser feels every one of them. Two channels at once,
/// because a keyword inside a diagnostic needs to be both coloured and
/// underlined and splitting them would need twice the elements.
function runsHtml(chars, scope, severity, picked, from, to) {
  const same = (i, j) =>
    (scope?.[i] ?? null) === (scope?.[j] ?? null) &&
    (severity?.[i] ?? null) === (severity?.[j] ?? null) &&
    Boolean(picked?.[i]) === Boolean(picked?.[j]);

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
  const anchor = view.selection;
  if (!anchor) return null;
  const head = view.cursor;

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

function lineHtml(number, text, cursorColumn, highlights) {
  const here = marks.byLine.get(number);
  const sign = here?.length
    ? `<span class="s d-${here[0].severity}">${icon(SIGN[here[0].severity], "solid")}</span>`
    : `<span class="s"></span>`;
  const gutter = `<span class="n">${number + 1}</span>${sign}`;

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

  const range = selectedOn(number, chars.length);
  const picked = range && new Array(chars.length).fill(false);
  if (range) {
    for (let i = range[0]; i < range[1]; i++) picked[i] = true;
  }
  // A selected empty line has nothing to paint, so the row says so instead.
  const empty = range && chars.length === 0 ? " selected" : "";

  if (cursorColumn === undefined) {
    const body = runsHtml(chars, scope, severity, picked, 0, chars.length);
    return `<div class="ln${empty}">${gutter}${body}</div>`;
  }
  // The caret sits *on* a character in normal mode, so it takes the one it
  // covers with it rather than being drawn between two.
  const under = escapeHtml(chars[cursorColumn] ?? " ") || " ";
  return (
    `<div class="ln cur${empty}">${gutter}` +
    `${runsHtml(chars, scope, severity, picked, 0, cursorColumn)}` +
    `<span class="caret">${under}</span>` +
    `${runsHtml(chars, scope, severity, picked, cursorColumn + 1, chars.length)}</div>`
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

function renderEditor() {
  const target = $("[data-editor]");
  if (!target) return;
  // Measured before building, because the spacer's height depends on it. With
  // no line rendered yet this falls back to the pane's computed line-height,
  // which is the same number.
  measureLineHeight();

  const cursorRow = view.cursor.line - view.firstLine;
  const rows = view.lines
    .map((text, i) =>
      lineHtml(
        view.firstLine + i,
        text,
        i === cursorRow ? view.cursor.column : undefined,
        view.highlights[i],
      ),
    )
    .join("");

  // A spacer the height of the whole file, with the loaded window positioned
  // inside it. `translate` rather than `top` so moving the window is a
  // compositor job and never a reflow.
  target.innerHTML =
    `<div class="doc" style="height:${Math.max(1, view.totalLines) * lineHeight}px">` +
    `<div class="win" style="transform:translateY(${view.firstLine * lineHeight}px)">` +
    `${rows}</div></div>`;

  // The window moved for a reason of the core's — `G`, a grep hit, the cursor
  // leaving the screen — so the pane has to follow. A user scroll is already
  // within one line of where it should be, which is what tells the two apart.
  const want = view.firstLine * lineHeight;
  if (Math.abs(target.scrollTop - want) > lineHeight) target.scrollTop = want;

  // The caret element the popup was anchored to has just been replaced, so
  // an open popup has to be told where the new one is or it stays where the
  // cursor used to be.
  const popup = $("#completion");
  if (popup && !popup.hidden) anchorCompletion(popup);
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
  dispatch({ type: "placeCursor", ...at });
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

function renderBuffers(names, active) {
  const target = $("[data-buffers]");
  if (!target) return;
  target.innerHTML = names
    .map(
      (b, i) =>
        `<span class="buf${i === active ? " on" : ""}" data-buffer="${i}">` +
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
  // has to be brought into view — otherwise `:bn` past the edge looks like it
  // did nothing.
  target.querySelector(".buf.on")?.scrollIntoView({ block: "nearest", inline: "nearest" });
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

function reportViewport() {
  const target = $("[data-editor]");
  if (!target) return;
  const first = Math.max(0, Math.floor(target.scrollTop / lineHeight));
  const rows = Math.max(1, Math.ceil(target.clientHeight / lineHeight) + OVERSCAN);
  dispatch({ type: "viewport", firstLine: first, lines: rows });
}

// Scroll events arrive far faster than frames, and every one of them would
// otherwise be a round trip to the core. One per frame is enough to keep up
// with a trackpad and is the most the screen can show anyway.
let viewportQueued = false;

function reportViewportSoon() {
  if (viewportQueued) return;
  viewportQueued = true;
  requestAnimationFrame(() => {
    viewportQueued = false;
    reportViewport();
  });
}

document.addEventListener(
  "scroll",
  (e) => {
    if (e.target?.matches?.("[data-editor]")) reportViewportSoon();
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
    .map(
      (item, i) =>
        `<div class="res${i === event.selected ? " on" : ""}" data-pick="${i}">` +
        `<span>${leader}${highlight(item.label, item.positions)}</span>` +
        `<span class="dir" style="margin-left:auto">${escapeHtml(item.detail)}</span></div>`,
    )
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
      } else if (SPLIT_LEADER[event.prefix]) {
        closeAll();
        SPLIT_LEADER[event.prefix]();
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
      // Only what the core did not recognise arrives here — `:w` and `:e` are
      // its own now. What is left is the split commands, which are still view
      // state, plus the residue of a compound: `:wq` writes in the core and
      // hands the `q` on to be run here.
      const run = SPLIT_COMMANDS[event.text.trim().replace(/^:/, "")];
      if (run) {
        run();
      } else if (event.text.trim()) {
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

    case "viewport":
      view.firstLine = event.firstLine;
      view.lines = event.lines;
      view.totalLines = event.totalLines;
      view.highlights = event.highlights ?? [];
      renderEditor();
      break;

    case "cursor":
      view.cursor = { line: event.line, column: event.column };
      view.selection = event.selection;
      renderEditor();
      renderPosition();
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

    case "completionClosed":
      closeCompletion();
      break;

    case "servers":
      renderServers(event.states);
      break;

    case "agent":
      renderAgent(event);
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
  if (el) el.textContent = `ln ${view.cursor.line + 1}, col ${view.cursor.column + 1}`;
}

/* ── keys ─────────────────────────────────────────────────────────────
   Two layers, and the split matters. Shell chords (⌘K, ⌘,, ⌃1-7) are the
   window's own bindings and never reach the core. Everything else is a
   keystroke the core interprets — including the digits, which are vim
   counts, not mode switches. */
document.addEventListener("keydown", (e) => {
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

  // Space and the vim keys must not scroll or type into the page.
  if (e.key === " " || e.key.length === 1) e.preventDefault();

  sendKey(e);
});

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
syncSplits();
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
Object.assign(window, {
  setTheme, setTranslucency, setMode: coreSetMode, setDock, open, closeAll,
  dispatch, drawPointerEdges, splitEditor, closeSplit, onlySplit, setHint,
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

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
  q: () => closeSplit(),
  close: () => closeSplit(),
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

function open(id) {
  const wasOpen = $("#" + id).classList.contains("on");
  closeAll();
  if (wasOpen) return;
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

const SIGN = { error: "●", warning: "▲", info: "■", hint: "·" };

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
    ? `<span class="s d-${here[0].severity}">${SIGN[here[0].severity]}</span>`
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

function renderEditor() {
  const target = $("[data-editor]");
  if (!target) return;
  const cursorRow = view.cursor.line - view.firstLine;
  target.innerHTML = view.lines
    .map((text, i) =>
      lineHtml(
        view.firstLine + i,
        text,
        i === cursorRow ? view.cursor.column : undefined,
        view.highlights[i],
      ),
    )
    .join("");

  // The caret element the popup was anchored to has just been replaced, so
  // an open popup has to be told where the new one is or it stays where the
  // cursor used to be.
  const popup = $("#completion");
  if (popup && !popup.hidden) anchorCompletion(popup);
}

function renderBuffers(names, active) {
  const target = $("[data-buffers]");
  if (!target) return;
  target.innerHTML = names
    .map(
      (b, i) =>
        `<span class="buf${i === active ? " on" : ""}" data-buffer="${i}">◈ ${escapeHtml(b.name)}` +
        `${b.modified ? ' <span class="mod"></span>' : ""}</span>`,
    )
    .join("");
}

/** How many lines the stage can actually show, so the core sends no more. */
function reportViewport() {
  const target = $("[data-editor]");
  if (!target) return;
  const lineHeight = parseFloat(getComputedStyle(target).lineHeight) || 20;
  const rows = Math.max(1, Math.ceil(target.clientHeight / lineHeight) + 2);
  dispatch({ type: "viewport", firstLine: view.firstLine, lines: rows });
}

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
  open("telescope");

  $("[data-picker-query]").textContent = event.query;
  $("[data-picker-title]").textContent = event.title;
  // An empty list while the tree is still being walked means "not yet", not
  // "no matches". Saying so is the difference between waiting a beat and
  // deciding the file isn't there.
  $("[data-picker-count]").textContent = event.scanning
    ? "scanning…"
    : `${event.items.length}/${event.total} · ${event.elapsedMs.toFixed(1)} ms`;

  const results = $("[data-picker-results]");
  results.innerHTML = event.items
    .map(
      (item, i) =>
        `<div class="res${i === event.selected ? " on" : ""}" data-pick="${i}">` +
        `<span>${highlight(item.label, item.positions)}</span>` +
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
        ? `<span class="e">✖ ${event.errors}</span><span class="w">⚠ ${event.warnings}</span>`
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
        return `<span class="busy">◐ ${escapeHtml(s.note ?? s.language)}</span>`;
      }
      if (s.status === "starting") return `<span class="busy">◌ ${escapeHtml(s.language)}</span>`;
      if (s.status === "ready") return `<span>◆ ${escapeHtml(s.language)}</span>`;
      // Not having gopls installed is a normal state, not a fault, so it is
      // dimmed rather than flagged.
      return `<span class="off">◇ ${escapeHtml(s.language)}</span>`;
    })
    .join(" ");
}

const KIND_MARK = {
  function: "ƒ", method: "ƒ", field: "▪", variable: "α", type: "T",
  module: "▤", keyword: "◆", constant: "π", snippet: "✂", other: "·",
};

function renderCompletion(event) {
  const popup = $("#completion");
  const list = $("[data-completion-list]");
  if (!popup || !list) return;

  list.innerHTML = event.items
    .map(
      (item, i) =>
        `<div class="cmp-row${i === event.selected ? " on" : ""}" data-complete="${i}">` +
        `<span class="k">${KIND_MARK[item.kind] ?? "·"}</span>` +
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
}

function renderStatus(text) {
  const el = $("#sl-status");
  if (el) el.textContent = text ?? "";
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
      // Leaving normal mode closes which-key and the pending display.
      if (event.mode !== "Command") $("#cmdline").classList.remove("on");
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
        open("whichkey");
        // ␣ff is the one leader sequence M0 resolves, so Telescope is reachable
        // the way it will be in M2.
        if (event.prefix === "ff") open("telescope");
      }
      break;

    case "cmdline":
      if (event.text === null) {
        closeAll();
      } else {
        open("cmdline");
        const el = $("#cmdline-text");
        if (el) el.textContent = event.text;
      }
      break;

    case "cmdlineRun": {
      // The core hands back the line it collected; the front end decides what
      // it means. In M1 that dispatch moves into the core with the buffers.
      const run = SPLIT_COMMANDS[event.text.trim().replace(/^:/, "")];
      if (run) {
        run();
        renderStatus(`:${event.text.trim()}`);
      } else if (event.text.trim()) {
        renderStatus(`:${event.text.trim()} — not a command yet`);
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

    case "notice":
      renderStatus(event.text);
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

  $$("[data-ptr]", canvas).forEach((slot) => {
    const target = $(`[data-addr="${slot.dataset.ptr}"]`, canvas);
    if (!target) return;

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

    const dangling = slot.classList.contains("dangling");
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

/* ── ambient life: the trace scrubber creeps forward ───────────────────
   Placeholder until proton-trace streams real steps in M4. */
const scrub = $(".cv-scrub .bar");
if (scrub) {
  let at = 38;
  setInterval(() => {
    at = at >= 92 ? 12 : at + 0.35;
    scrub.querySelector("i").style.width = at + "%";
    scrub.querySelector("u").style.left = at + "%";
  }, 220);
}

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

/* Excel: a workbook's sheets as a grid, its workbooks when none is open.
 *
 * The grid draws the rows on screen and no others, so a million rows scroll
 * as ten do: it asks the core for the rectangle in view, a little more each
 * way, and keeps what it is sent by cell. The core answers an edit with the
 * cells it moved that are in that rectangle, which replace those kept.
 *
 * What a cell shows — its number formatted, its formula worked out — is the
 * core's; this lays it out, keeps the selection, and turns typing into
 * `sheetSet`. A workbook is `generation`-numbered by the core, and anything
 * about another one is dropped. */

import { escapeHtml as esc, icon } from "./html.js";

/** @typedef {import("./bindings/SheetBookView").SheetBookView} BookView */
/** @typedef {import("./bindings/SheetCellView").SheetCellView} CellView */
/** @typedef {import("./bindings/SheetFound").SheetFound} Found */
/** @typedef {import("./bindings/SheetStyle").SheetStyle} Style */
/** @typedef {{generation: number, version: number, sheet: number, rows: [number, number], cols: [number, number], cells: CellView[], partial: boolean}} CellsEvent */

/** Pixels: a column as wide as Excel's default, a row a line of text. */
const COL = 72;
const ROW = 24;
/** The row numbers' column and the letters' row. */
const HEAD_W = 52;
const HEAD_H = 24;
/** How much more than is on screen is asked for, each way. */
const MARGIN_ROWS = 60;
const MARGIN_COLS = 12;
/** The grid's extent past the last cell in use. */
const SPARE_ROWS = 200;
const SPARE_COLS = 26;
const MAX_ROW = 1_048_575;
const MAX_COL = 16_383;

/** `A`, `Z`, `AA` for 0, 25, 26. @param {number} col */
export function colName(col) {
  let s = "";
  for (let n = col + 1; n > 0; n = Math.floor((n - 1) / 26)) s = String.fromCharCode(65 + ((n - 1) % 26)) + s;
  return s;
}

/** `B7` for row 6, column 1. @param {number} row @param {number} col */
export const cellName = (row, col) => `${colName(col)}${row + 1}`;

/** A cell's name read back, or null. @param {string} text @returns {[number, number] | null} */
export function parseCell(text) {
  const m = /^\$?([A-Za-z]{1,3})\$?(\d{1,7})$/.exec(text.trim());
  if (!m) return null;
  let col = 0;
  for (const ch of m[1].toUpperCase()) col = col * 26 + (ch.charCodeAt(0) - 64);
  const row = Number(m[2]) - 1;
  if (col < 1 || col - 1 > MAX_COL || row < 0 || row > MAX_ROW) return null;
  return [row, col - 1];
}

/** @param {{root: Document, dispatch: (command: any) => unknown}} host */
export function createSheet(host) {
  const found = /** @type {HTMLElement | null} */ (host.root.querySelector('.workspace[data-ws="sheet"]'));
  if (!found) return null;
  const ws = found;
  const q = (/** @type {string} */ sel) => /** @type {HTMLElement} */ (ws.querySelector(sel));
  const els = {
    list: q("[data-sheet-list]"),
    home: q("[data-sheet-home]"),
    book: q("[data-sheet-book]"),
    title: q("[data-sheet-title]"),
    name: /** @type {HTMLInputElement} */ (q("[data-sheet-name]")),
    bar: /** @type {HTMLInputElement} */ (q("[data-sheet-bar]")),
    scroll: q("[data-sheet-scroll]"),
    space: q("[data-sheet-space]"),
    cells: q("[data-sheet-cells]"),
    colHead: q("[data-sheet-colhead]"),
    rowHead: q("[data-sheet-rowhead]"),
    tabs: q("[data-sheet-tabs]"),
    editor: /** @type {HTMLInputElement} */ (q("[data-sheet-editor]")),
  };

  const state = {
    /** @type {Found[]} */
    workbooks: [],
    /** @type {BookView | null} */
    book: null,
    /** Cells kept for the sheet on screen, by `row,col`. @type {Map<string, CellView>} */
    cells: new Map(),
    /** The sheet they are of, and the rectangle last asked for. */
    sheet: -1,
    /** @type {{rows: [number, number], cols: [number, number]} | null} */
    asked: null,
    /** The selected cell, and the far corner of a range dragged from it. */
    at: /** @type {[number, number]} */ ([0, 0]),
    to: /** @type {[number, number]} */ ([0, 0]),
    /** Where the grid ends for now: grown as it is scrolled toward. */
    extent: /** @type {[number, number]} */ ([SPARE_ROWS, SPARE_COLS]),
    /** Editing a cell: in it, or in the formula bar. */
    editing: /** @type {"" | "cell" | "bar"} */ (""),
    drawn: 0,
  };

  const key = (/** @type {number} */ r, /** @type {number} */ c) => `${r},${c}`;
  const tab = () => state.book?.sheets[state.book.active] ?? null;
  /** Column widths in pixels, where they are not the default. */
  let widths = /** @type {Map<number, number>} */ (new Map());
  const width = (/** @type {number} */ c) => widths.get(c) ?? COL;
  /** Where a column starts, from the grid's left edge past the row numbers. @param {number} c */
  function colX(c) {
    let x = c * COL;
    for (const [k, w] of widths) if (k < c) x += w - COL;
    return x;
  }
  /** The column at a distance across. @param {number} x */
  function colAt(x) {
    let c = Math.max(0, Math.floor(x / COL));
    while (c > 0 && colX(c) > x) c--;
    while (colX(c + 1) <= x && c < MAX_COL) c++;
    return c;
  }

  /* ── the home page ─────────────────────────────────────────────────── */

  const KIND = { folder: "Workbook", xlsx: "Excel", csv: "CSV", tsv: "TSV" };

  function renderList() {
    const open = state.book?.path;
    const rows = state.workbooks
      .map((w) => `<button class="xl-row${w.path === open ? " on" : ""}" data-sheet-open="${esc(w.path)}" title="${esc(w.path)}">
        ${icon("table")}<span class="xl-row-name">${esc(w.name)}</span><span class="xl-kind">${KIND[w.kind]}</span></button>`)
      .join("");
    els.list.innerHTML = rows || `<p class="xl-empty">No workbooks in this project yet.</p>`;
    els.home.querySelector("[data-sheet-found]")?.replaceChildren();
    const found = els.home.querySelector("[data-sheet-found]");
    if (found) found.innerHTML = rows;
  }

  /* ── the workbook ──────────────────────────────────────────────────── */

  function renderBook() {
    const b = state.book;
    els.home.hidden = !!b;
    els.book.hidden = !b;
    renderList();
    if (!b) return;
    els.title.textContent = b.name;
    els.title.title = b.path;
    ws.classList.toggle("dirty", b.dirty);
    q("[data-sheet-dirty]").hidden = !b.dirty;
    const t = tab();
    widths = new Map((t?.widths ?? []).map(([c, pt]) => [c, Math.round((pt * 4) / 3)]));
    els.tabs.innerHTML =
      b.sheets
        .map((s, i) => `<button class="xl-tab${i === b.active ? " on" : ""}" role="tab" aria-selected="${i === b.active}" data-sheet-tab="${i}" title="${esc(s.name)} — double-click to rename">${esc(s.name)}</button>`)
        .join("") +
      `<button class="xl-tab-add" data-sheet-add title="Add a sheet">${icon("plus")}</button>` +
      (b.sheets.length > 1 ? `<button class="xl-tab-remove" data-sheet-remove title="Remove this sheet (undo brings it back)">${icon("close")}</button>` : "");
    if (b.active !== state.sheet) {
      state.sheet = b.active;
      state.cells.clear();
      state.asked = null;
      state.at = [0, 0];
      state.to = [0, 0];
      els.scroll.scrollTop = 0;
      els.scroll.scrollLeft = 0;
    }
    state.extent = [Math.max(state.extent[0], (t?.rows ?? 0) + SPARE_ROWS), Math.max(state.extent[1], (t?.cols ?? 0) + SPARE_COLS)];
    layout();
    ask(true);
    draw();
    showSelected();
  }

  function layout() {
    els.space.style.width = `${HEAD_W + colX(Math.min(state.extent[1], MAX_COL + 1))}px`;
    els.space.style.height = `${HEAD_H + Math.min(state.extent[0], MAX_ROW + 1) * ROW}px`;
  }

  /** The rows and columns on screen. @returns {{rows: [number, number], cols: [number, number]}} */
  function visible() {
    const s = els.scroll;
    const r0 = Math.max(0, Math.floor(s.scrollTop / ROW));
    const r1 = Math.min(MAX_ROW, r0 + Math.ceil((s.clientHeight || 600) / ROW) + 1);
    const c0 = colAt(s.scrollLeft);
    const c1 = Math.min(MAX_COL, colAt(s.scrollLeft + (s.clientWidth || 900)) + 1);
    return { rows: [r0, r1], cols: [c0, c1] };
  }

  /** Ask for what is on screen when it is not already what was asked for. @param {boolean} [again] */
  function ask(again = false) {
    const b = state.book;
    if (!b) return;
    const v = visible();
    const a = state.asked;
    const covered = a && v.rows[0] >= a.rows[0] && v.rows[1] <= a.rows[1] && v.cols[0] >= a.cols[0] && v.cols[1] <= a.cols[1];
    if (covered && !again) return;
    const rows = /** @type {[number, number]} */ ([Math.max(0, v.rows[0] - MARGIN_ROWS), Math.min(MAX_ROW, v.rows[1] + MARGIN_ROWS)]);
    const cols = /** @type {[number, number]} */ ([Math.max(0, v.cols[0] - MARGIN_COLS), Math.min(MAX_COL, v.cols[1] + MARGIN_COLS)]);
    state.asked = { rows, cols };
    host.dispatch({ type: "sheetView", generation: b.generation, sheet: b.active, rows, cols });
  }

  /** The style a cell names. @param {number} n @returns {Style | null} */
  const styleOf = (n) => (n ? state.book?.styles[n] ?? null : null);

  function draw() {
    state.drawn = 0;
    if (!state.book) return;
    const v = visible();
    const sel = selection();
    let cols = "", rows = "", cells = "";
    for (let c = v.cols[0]; c <= v.cols[1]; c++) {
      const on = c >= sel.c0 && c <= sel.c1 ? " on" : "";
      cols += `<div class="xl-ch${on}" style="left:${HEAD_W + colX(c)}px;width:${width(c)}px">${colName(c)}</div>`;
    }
    for (let r = v.rows[0]; r <= v.rows[1]; r++) {
      const on = r >= sel.r0 && r <= sel.r1 ? " on" : "";
      rows += `<div class="xl-rh${on}" style="top:${HEAD_H + r * ROW}px">${r + 1}</div>`;
    }
    for (const cell of state.cells.values()) {
      if (cell.row < v.rows[0] || cell.row > v.rows[1] || cell.col < v.cols[0] || cell.col > v.cols[1]) continue;
      if (!cell.text && !cell.style) continue;
      const st = styleOf(cell.style);
      const align = st?.align ?? (cell.kind === "n" ? "right" : cell.kind === "b" || cell.kind === "e" ? "center" : "left");
      let css = `left:${HEAD_W + colX(cell.col)}px;top:${HEAD_H + cell.row * ROW}px;width:${width(cell.col)}px;text-align:${align};`;
      if (st?.bold) css += "font-weight:700;";
      if (st?.italic) css += "font-style:italic;";
      if (st?.underline) css += "text-decoration:underline;";
      const color = cell.color ?? st?.color;
      if (color) css += `color:${color};`;
      if (st?.fill) css += `background:${st.fill};`;
      const cls = `xl-c${cell.kind === "e" ? " err" : ""}${st?.wrap ? " wrap" : ""}`;
      cells += `<div class="${cls}" style="${css}" data-cell="${cell.row},${cell.col}">${esc(cell.text)}</div>`;
    }
    els.colHead.innerHTML = cols;
    els.rowHead.innerHTML = rows;
    els.cells.innerHTML = cells + selectionBox(sel);
    els.colHead.style.transform = `translateX(${-els.scroll.scrollLeft}px)`;
    els.rowHead.style.transform = `translateY(${-els.scroll.scrollTop}px)`;
  }

  /** The selected range, its corners in order. */
  function selection() {
    const [r, c] = state.at, [r2, c2] = state.to;
    return { r0: Math.min(r, r2), r1: Math.max(r, r2), c0: Math.min(c, c2), c1: Math.max(c, c2) };
  }

  /** @param {{r0: number, r1: number, c0: number, c1: number}} sel */
  function selectionBox(sel) {
    const x = HEAD_W + colX(sel.c0), y = HEAD_H + sel.r0 * ROW;
    const w = colX(sel.c1 + 1) - colX(sel.c0), h = (sel.r1 - sel.r0 + 1) * ROW;
    const [ar, ac] = state.at;
    const range = sel.r0 !== sel.r1 || sel.c0 !== sel.c1;
    return (range ? `<div class="xl-range" style="left:${x}px;top:${y}px;width:${w}px;height:${h}px"></div>` : "") +
      `<div class="xl-cursor" style="left:${HEAD_W + colX(ac)}px;top:${HEAD_H + ar * ROW}px;width:${width(ac)}px;height:${ROW}px"></div>`;
  }

  let frame = 0;
  function redraw() {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      draw();
    });
  }

  /** The cell as kept, or an empty one. @param {number} r @param {number} c */
  const cellAt = (r, c) => state.cells.get(key(r, c));

  /** Show what the selected cell holds in the name box and formula bar. */
  function showSelected() {
    const [r, c] = state.at;
    const sel = selection();
    els.name.value = sel.r0 === sel.r1 && sel.c0 === sel.c1 ? cellName(r, c) : `${cellName(sel.r0, sel.c0)}:${cellName(sel.r1, sel.c1)}`;
    if (state.editing !== "bar") els.bar.value = cellAt(r, c)?.input ?? "";
  }

  /** Move the selection, keeping it on screen. @param {number} r @param {number} c @param {boolean} [extend] */
  function select(r, c, extend = false) {
    r = Math.max(0, Math.min(MAX_ROW, r));
    c = Math.max(0, Math.min(MAX_COL, c));
    state.to = [r, c];
    if (!extend) state.at = [r, c];
    if (r + 20 > state.extent[0] || c + 5 > state.extent[1]) {
      state.extent = [Math.max(state.extent[0], r + SPARE_ROWS), Math.max(state.extent[1], c + SPARE_COLS)];
      layout();
    }
    const s = els.scroll;
    const [tr, tc] = extend ? state.to : state.at;
    const top = tr * ROW, left = colX(tc);
    const viewH = s.clientHeight - HEAD_H, viewW = s.clientWidth - HEAD_W;
    if (top < s.scrollTop) s.scrollTop = top;
    else if (top + ROW > s.scrollTop + viewH) s.scrollTop = top + ROW - viewH;
    if (left < s.scrollLeft) s.scrollLeft = left;
    else if (left + width(tc) > s.scrollLeft + viewW) s.scrollLeft = left + width(tc) - viewW;
    showSelected();
    redraw();
    ask();
  }

  /* ── editing ───────────────────────────────────────────────────────── */

  /** Start editing the selected cell, in place, with what is there or `text`. @param {string | null} text */
  function editCell(text) {
    if (!state.book) return;
    const [r, c] = state.at;
    state.editing = "cell";
    const e = els.editor;
    e.hidden = false;
    e.style.left = `${HEAD_W + colX(c) - els.scroll.scrollLeft}px`;
    e.style.top = `${HEAD_H + r * ROW - els.scroll.scrollTop}px`;
    e.style.minWidth = `${width(c)}px`;
    e.value = text ?? cellAt(r, c)?.input ?? "";
    e.focus({ preventScroll: true });
    const end = e.value.length;
    e.setSelectionRange(end, end);
    els.bar.value = e.value;
  }

  /** Put what was typed into the cell, and move on. @param {string} value @param {[number, number]} [move] */
  function commit(value, move = [1, 0]) {
    const b = state.book;
    const was = state.editing;
    stopEditing();
    if (!b || !was) return;
    const [r, c] = state.at;
    if ((cellAt(r, c)?.input ?? "") !== value) {
      host.dispatch({ type: "sheetSet", generation: b.generation, sheet: b.active, cells: [[cellName(r, c), value]] });
    }
    select(r + move[0], c + move[1]);
    els.scroll.focus({ preventScroll: true });
  }

  function stopEditing() {
    state.editing = "";
    els.editor.hidden = true;
    els.editor.blur();
    showSelected();
  }

  /** The selection emptied, as one step. */
  function clearSelection() {
    const b = state.book;
    if (!b) return;
    const sel = selection();
    /** @type {[string, string][]} */
    const cells = [];
    for (const cell of state.cells.values()) {
      if (cell.row >= sel.r0 && cell.row <= sel.r1 && cell.col >= sel.c0 && cell.col <= sel.c1 && cell.input) cells.push([cellName(cell.row, cell.col), ""]);
    }
    if (cells.length) host.dispatch({ type: "sheetSet", generation: b.generation, sheet: b.active, cells });
  }

  els.editor.addEventListener("input", () => {
    els.bar.value = els.editor.value;
  });
  els.editor.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      commit(els.editor.value, e.shiftKey ? [-1, 0] : [1, 0]);
    } else if (e.key === "Tab") {
      e.preventDefault();
      commit(els.editor.value, e.shiftKey ? [0, -1] : [0, 1]);
    } else if (e.key === "Escape") {
      e.preventDefault();
      stopEditing();
      els.scroll.focus({ preventScroll: true });
    }
    e.stopPropagation();
  });
  els.editor.addEventListener("blur", () => {
    if (state.editing === "cell") commit(els.editor.value, [0, 0]);
  });

  els.bar.addEventListener("focus", () => {
    if (state.book) state.editing = "bar";
  });
  els.bar.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      commit(els.bar.value, [1, 0]);
    } else if (e.key === "Escape") {
      e.preventDefault();
      stopEditing();
      els.scroll.focus({ preventScroll: true });
    }
    e.stopPropagation();
  });
  els.bar.addEventListener("blur", () => {
    if (state.editing === "bar") stopEditing();
  });

  // The name box: a cell's name goes there.
  els.name.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      const [a, b] = els.name.value.split(":");
      const from = parseCell(a ?? ""), to = b ? parseCell(b) : null;
      if (from) {
        select(from[0], from[1]);
        if (to) select(to[0], to[1], true);
      }
      els.scroll.focus({ preventScroll: true });
    } else if (e.key === "Escape") {
      showSelected();
      els.scroll.focus({ preventScroll: true });
    }
    e.stopPropagation();
  });

  /* ── keys and the mouse ────────────────────────────────────────────── */

  els.scroll.addEventListener("scroll", () => {
    const s = els.scroll;
    // Near the end, the grid goes on.
    if (s.scrollTop + s.clientHeight > (state.extent[0] - 40) * ROW) {
      state.extent[0] = Math.min(MAX_ROW + 1, state.extent[0] + SPARE_ROWS);
      layout();
    }
    if (s.scrollLeft + s.clientWidth > colX(state.extent[1] - 4)) {
      state.extent[1] = Math.min(MAX_COL + 1, state.extent[1] + SPARE_COLS);
      layout();
    }
    if (state.editing === "cell") commit(els.editor.value, [0, 0]);
    redraw();
    ask();
  }, { passive: true });

  /** The cell under a point in the grid. @param {MouseEvent} e @returns {[number, number]} */
  function cellUnder(e) {
    const box = els.scroll.getBoundingClientRect();
    const x = e.clientX - box.left + els.scroll.scrollLeft - HEAD_W;
    const y = e.clientY - box.top + els.scroll.scrollTop - HEAD_H;
    return [Math.max(0, Math.floor(y / ROW)), colAt(Math.max(0, x))];
  }

  let dragging = false;
  els.scroll.addEventListener("mousedown", (e) => {
    if (e.button !== 0 || !state.book) return;
    const [r, c] = cellUnder(e);
    if (state.editing) commit(state.editing === "cell" ? els.editor.value : els.bar.value, [0, 0]);
    select(r, c, e.shiftKey);
    dragging = true;
    els.scroll.focus({ preventScroll: true });
    e.preventDefault();
  });
  window.addEventListener("mousemove", (e) => {
    if (!dragging) return;
    const [r, c] = cellUnder(e);
    if (r !== state.to[0] || c !== state.to[1]) select(r, c, true);
  });
  window.addEventListener("mouseup", () => {
    dragging = false;
  });
  els.scroll.addEventListener("dblclick", (e) => {
    if (!state.book) return;
    const [r, c] = cellUnder(e);
    select(r, c);
    editCell(null);
  });

  ws.addEventListener("keydown", (e) => {
    const b = state.book;
    const mod = e.metaKey || e.ctrlKey;
    // ⌘S saves, ⌘Z and ⌘⇧Z undo and redo, wherever the focus is in the mode.
    if (b && e.metaKey && !e.altKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      if (state.editing) commit(state.editing === "cell" ? els.editor.value : els.bar.value, [0, 0]);
      host.dispatch({ type: "sheetSave" });
      return;
    }
    if (e.target !== els.scroll) return;
    if (!b) return;
    if (e.metaKey && e.key.toLowerCase() === "z") {
      e.preventDefault();
      host.dispatch({ type: e.shiftKey ? "sheetRedo" : "sheetUndo" });
      return;
    }
    if (e.ctrlKey && /^[0-9]$/.test(e.key)) return; // modes, the window's
    const [r, c] = state.at;
    const [tr, tc] = state.to;
    const ext = e.shiftKey;
    const from = ext ? [tr, tc] : [r, c];
    // The arrows, and Vim's h j k l; with ⇧ they stretch the selection.
    /** @type {Record<string, [number, number]>} */
    const moves = { ArrowDown: [1, 0], ArrowUp: [-1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1], j: [1, 0], k: [-1, 0], h: [0, -1], l: [0, 1] };
    const named = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    const move = mod ? undefined : moves[named];
    if (move) {
      e.preventDefault();
      select(from[0] + move[0], from[1] + move[1], ext);
      return;
    }
    if (e.key === "Enter" || e.key === "F2" || (!mod && (e.key === "i" || e.key === "a"))) {
      e.preventDefault();
      editCell(null);
      return;
    }
    if (e.key === "Tab") {
      e.preventDefault();
      select(r, c + (e.shiftKey ? -1 : 1));
      return;
    }
    if (e.key === "Delete" || e.key === "Backspace" || (!mod && e.key === "x")) {
      e.preventDefault();
      clearSelection();
      return;
    }
    if (!mod && e.key === "u") {
      e.preventDefault();
      host.dispatch({ type: "sheetUndo" });
      return;
    }
    if (e.ctrlKey && e.key === "r") {
      e.preventDefault();
      host.dispatch({ type: "sheetRedo" });
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      select(r, c);
      return;
    }
    // Anything else typed starts the cell over with it, as in every
    // spreadsheet; `=` starts a formula.
    if (!mod && !e.altKey && e.key.length === 1 && !"hjklHJKLaiux".includes(e.key)) {
      e.preventDefault();
      editCell(e.key);
    }
  });

  /* ── tabs, the home page, the header ───────────────────────────────── */

  ws.addEventListener("click", (e) => {
    const t = /** @type {HTMLElement | null} */ (e.target instanceof Element ? e.target.closest("button, [data-sheet-tab]") : null);
    if (!t) return;
    if (t.dataset.sheetOpen !== undefined) host.dispatch({ type: "sheetOpen", path: t.dataset.sheetOpen });
    else if (t.dataset.sheetTab !== undefined) {
      if (state.editing) commit(state.editing === "cell" ? els.editor.value : els.bar.value, [0, 0]);
      host.dispatch({ type: "sheetTab", sheet: Number(t.dataset.sheetTab) });
    } else if (t.dataset.sheetAdd !== undefined) host.dispatch({ type: "sheetAddSheet", name: null });
    else if (t.dataset.sheetRemove !== undefined && state.book) host.dispatch({ type: "sheetRemoveSheet", sheet: state.book.active });
    else if (t.dataset.sheetClose !== undefined) host.dispatch({ type: "sheetClose", discard: false });
    else if (t.dataset.sheetSave !== undefined) host.dispatch({ type: "sheetSave" });
    else if (t.dataset.sheetNew !== undefined) newBook();
  });

  ws.addEventListener("dblclick", (e) => {
    const t = e.target instanceof HTMLElement ? e.target.closest("[data-sheet-tab]") : null;
    if (!(t instanceof HTMLElement) || !state.book) return;
    const i = Number(t.dataset.sheetTab);
    const field = document.createElement("input");
    field.className = "xl-tab-name";
    field.value = state.book.sheets[i]?.name ?? "";
    t.replaceChildren(field);
    field.focus();
    field.select();
    let done = false;
    const finish = (/** @type {boolean} */ keep) => {
      if (done) return;
      done = true;
      const name = field.value.trim();
      if (keep && name && name !== state.book?.sheets[i]?.name) host.dispatch({ type: "sheetRenameSheet", sheet: i, name });
      else renderBook();
    };
    field.addEventListener("keydown", (k) => {
      if (k.key === "Enter") finish(true);
      else if (k.key === "Escape") finish(false);
      k.stopPropagation();
    });
    field.addEventListener("blur", () => finish(true));
  });

  /** The home page's form: a name, and what to keep it as. */
  function newBook() {
    const form = /** @type {HTMLFormElement | null} */ (ws.querySelector("[data-sheet-form]"));
    if (!form) return;
    els.home.hidden = false;
    els.book.hidden = true;
    form.hidden = false;
    /** @type {HTMLInputElement | null} */ (form.querySelector("input"))?.focus();
  }

  ws.querySelector("[data-sheet-form]")?.addEventListener("submit", (e) => {
    e.preventDefault();
    const form = /** @type {HTMLFormElement} */ (e.target);
    const name = /** @type {HTMLInputElement} */ (form.querySelector("input")).value.trim();
    const as = /** @type {HTMLSelectElement} */ (form.querySelector("select")).value;
    if (!name) return;
    const slug = name.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "-").replace(/^-|-$/g, "") || "workbook";
    host.dispatch({ type: "sheetNew", name, folder: as === "sheet" ? null : `${slug}.${as}` });
    form.reset();
  });

  return {
    /** @param {{workbooks: Found[]}} event */
    home(event) {
      state.workbooks = event.workbooks;
      renderList();
    },
    /** @param {{book: BookView | null}} event */
    book(event) {
      const was = state.book?.generation;
      state.book = event.book;
      if (!event.book || event.book.generation !== was) {
        state.cells.clear();
        state.asked = null;
        state.sheet = -1;
        state.extent = [SPARE_ROWS, SPARE_COLS];
        state.editing = "";
        els.editor.hidden = true;
      }
      renderBook();
    },
    /** @param {CellsEvent} event */
    cells(event) {
      const b = state.book;
      if (!b || event.generation !== b.generation || event.sheet !== b.active) return;
      if (!event.partial) {
        for (const k of [...state.cells.keys()]) {
          const [r, c] = k.split(",").map(Number);
          if (r >= event.rows[0] && r <= event.rows[1] && c >= event.cols[0] && c <= event.cols[1]) state.cells.delete(k);
        }
      }
      for (const cell of event.cells) {
        if (cell.input || cell.style || cell.text) state.cells.set(key(cell.row, cell.col), cell);
        else state.cells.delete(key(cell.row, cell.col));
      }
      showSelected();
      redraw();
    },
    /** The mode is on screen: the grid takes the keys. */
    shown() {
      requestAnimationFrame(() => {
        if (state.book) {
          layout();
          ask(true);
          draw();
          if (!state.editing) els.scroll.focus({ preventScroll: true });
        }
      });
    },
    /** For tests: what is selected and kept. */
    debug() {
      return { at: state.at, to: state.to, cells: state.cells.size, editing: state.editing, asked: state.asked };
    },
  };
}

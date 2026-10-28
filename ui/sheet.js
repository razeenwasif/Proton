/* Excel: a workbook's sheets as a grid, its workbooks when none is open.
 *
 * The grid draws the rows on screen and no others, so a million rows scroll
 * as ten do: it asks the core for the rectangle in view, a little more each
 * way, and keeps what it is sent by cell. The core answers an edit with the
 * cells it moved that are in that rectangle, which replace those kept.
 * Frozen rows and columns come with every rectangle, and are drawn over the
 * cells scrolled under them.
 *
 * What a cell shows — its number formatted, its formula worked out — is the
 * core's; this lays it out, keeps the selection, and turns keys and the
 * mouse into commands: Vim's keys to move and act, a spreadsheet's to type,
 * `:` for the rest. A workbook is `generation`-numbered by the core, and
 * anything about another one is dropped. */

import { escapeHtml as esc, icon } from "./html.js";
import { chartSvg } from "./sheet-chart.js";

/** @typedef {import("./bindings/SheetBookView").SheetBookView} BookView */
/** @typedef {import("./bindings/SheetCellView").SheetCellView} CellView */
/** @typedef {import("./bindings/SheetFound").SheetFound} Found */
/** @typedef {import("./bindings/SheetStyle").SheetStyle} Style */
/** @typedef {import("./bindings/SheetStylePatch").SheetStylePatch} Patch */
/** @typedef {{generation: number, version: number, sheet: number, rows: [number, number], cols: [number, number], cells: CellView[], partial: boolean}} CellsEvent */
/** @typedef {{r0: number, r1: number, c0: number, c1: number}} Sel */

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

/** The number formats offered, as Excel writes them. */
export const FORMATS = /** @type {[string, string][]} */ ([
  ["", "General"], ["0", "0"], ["0.00", "0.00"], ["#,##0", "#,##0"], ["#,##0.00", "#,##0.00"], ["0%", "0%"], ["0.00%", "0.00%"],
  ["$#,##0.00", "$#,##0.00"], ["0.00E+00", "Scientific"], ["yyyy-mm-dd", "Date"], ["yyyy-mm-dd hh:mm", "Date and time"], ["h:mm", "Time"], ["@", "Text"],
]);

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

/** `B7:D9` read, or null. @param {string} text @returns {Sel | null} */
export function parseRange(text) {
  const [a, b = a] = text.split(":");
  const from = parseCell(a ?? ""), to = parseCell(b ?? "");
  if (!from || !to) return null;
  return { r0: Math.min(from[0], to[0]), r1: Math.max(from[0], to[0]), c0: Math.min(from[1], to[1]), c1: Math.max(from[1], to[1]) };
}

/** A selection as the core reads a range. @param {Sel} s */
export const rangeName = (s) => (s.r0 === s.r1 && s.c0 === s.c1 ? cellName(s.r0, s.c0) : `${cellName(s.r0, s.c0)}:${cellName(s.r1, s.c1)}`);

/** @param {{root: Document, dispatch: (command: any) => unknown, paste?: () => Promise<string | null>}} host */
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
    cmd: /** @type {HTMLInputElement} */ (q("[data-sheet-cmd]")),
    scroll: q("[data-sheet-scroll]"),
    space: q("[data-sheet-space]"),
    cells: q("[data-sheet-cells]"),
    colHead: q("[data-sheet-colhead]"),
    rowHead: q("[data-sheet-rowhead]"),
    tabs: q("[data-sheet-tabs]"),
    tools: q("[data-sheet-tools]"),
    editor: /** @type {HTMLInputElement} */ (q("[data-sheet-editor]")),
    charts: q("[data-sheet-charts]"),
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
    /** The selected cell, and the far corner of a range from it. */
    at: /** @type {[number, number]} */ ([0, 0]),
    to: /** @type {[number, number]} */ ([0, 0]),
    /** `v` stretches the selection as the cursor moves; `V` takes whole rows. */
    visual: /** @type {"" | "v" | "V"} */ (""),
    /** A key waiting for the next: `g`, `d`, `y`. */
    pending: "",
    /** Where the grid ends for now: grown as it is scrolled toward. */
    extent: /** @type {[number, number]} */ ([SPARE_ROWS, SPARE_COLS]),
    /** Editing a cell: in it, or in the formula bar. */
    editing: /** @type {"" | "cell" | "bar"} */ (""),
  };

  const key = (/** @type {number} */ r, /** @type {number} */ c) => `${r},${c}`;
  const tab = () => state.book?.sheets[state.book.active] ?? null;
  const frozen = () => /** @type {[number, number]} */ (tab()?.frozen ?? [0, 0]);
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
  /** The rows a filter hides, as runs: they take no room. */
  const hiddenRuns = () => tab()?.hidden ?? [];
  /** Whether a filter hides a row. @param {number} r */
  const isHidden = (r) => hiddenRuns().some(([a, b]) => r >= a && r <= b);
  /** Where a row starts, down from the letters, hidden rows taking no room. @param {number} r */
  function rowY(r) {
    let skip = 0;
    for (const [a, b] of hiddenRuns()) {
      if (a >= r) break;
      skip += Math.min(b, r - 1) - a + 1;
    }
    return (r - skip) * ROW;
  }
  /** The row at a distance down. @param {number} y */
  function rowAt(y) {
    let r = Math.max(0, Math.floor(y / ROW));
    for (const [a, b] of hiddenRuns()) {
      if (a > r) break;
      r += b - a + 1;
    }
    return Math.min(MAX_ROW, r);
  }
  /** The next row shown from `r` going `by`, past hidden ones. @param {number} r @param {number} by */
  function step(r, by) {
    let n = r + by;
    while (by && n > 0 && n < MAX_ROW && isHidden(n)) n += Math.sign(by);
    return n;
  }
  /** Where a cell is drawn in the scrolled space: a frozen one moves with
   *  the scroll, so it stays where it is on screen. @param {number} r @param {number} c */
  function pos(r, c) {
    const [fr, fc] = frozen();
    return {
      x: HEAD_W + colX(c) + (c < fc ? els.scroll.scrollLeft : 0),
      y: HEAD_H + rowY(r) + (r < fr ? els.scroll.scrollTop : 0),
    };
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
    const list = els.home.querySelector("[data-sheet-found]");
    if (list) list.innerHTML = rows;
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
      state.visual = "";
      els.scroll.scrollTop = 0;
      els.scroll.scrollLeft = 0;
    }
    state.extent = [Math.max(state.extent[0], (t?.rows ?? 0) + SPARE_ROWS), Math.max(state.extent[1], (t?.cols ?? 0) + SPARE_COLS)];
    layout();
    ask(true);
    draw();
    renderCharts();
    showSelected();
  }

  /** A chart's corner on the cells it sits on. @param {string} at */
  function chartCorner(at) {
    const [r, c] = parseCell(at) ?? [0, 0];
    return { x: HEAD_W + colX(c), y: HEAD_H + rowY(r) };
  }

  function renderCharts() {
    const charts = tab()?.charts ?? [];
    els.charts.innerHTML = charts
      .map((view, i) => {
        const { x, y } = chartCorner(view.chart.at);
        const { width: w, height: h } = view.chart;
        const kinds = ["line", "bar", "scatter", "pie"].map((k) => `<option value="${k}"${k === view.chart.kind ? " selected" : ""}>${k[0].toUpperCase()}${k.slice(1)}</option>`).join("");
        return `<div class="xl-chart" data-chart="${i}" style="left:${x}px;top:${y}px;width:${w}px;height:${h}px">
          <header class="xl-chart-head" data-chart-drag="${i}" title="Drag to move · double-click the title to rename">
            <span class="xl-chart-title" data-chart-title="${i}">${esc(view.chart.title || view.chart.range)}</span>
            <select data-chart-kind="${i}" aria-label="Kind of chart">${kinds}</select>
            <button data-chart-remove="${i}" title="Remove the chart">✕</button>
          </header>
          ${chartSvg(view, w, h - 28)}
          <i class="xl-chart-size" data-chart-size="${i}" title="Drag to resize"></i>
        </div>`;
      })
      .join("");
  }

  /** A chart changed: moved, resized, retitled, another kind. @param {number} i @param {Partial<import("./bindings/SheetChart").SheetChart>} change */
  function setChart(i, change) {
    const view = tab()?.charts[i];
    if (view) send({ type: "sheetChartSet", index: i, chart: { ...view.chart, ...change } });
  }

  /** A chart over the selection (or the rows in use), to its right. @param {string} kind */
  function addChart(kind = "bar") {
    const r = region(true);
    const at = cellName(r.r0, r.c1 + 2);
    send({ type: "sheetChartAdd", chart: { kind, range: rangeName(r), title: "", at, width: 480, height: 300 } });
  }

  function layout() {
    els.space.style.width = `${HEAD_W + colX(Math.min(state.extent[1], MAX_COL + 1))}px`;
    els.space.style.height = `${HEAD_H + rowY(Math.min(state.extent[0], MAX_ROW + 1))}px`;
  }

  /** The rows and columns on screen. @returns {{rows: [number, number], cols: [number, number]}} */
  function visible() {
    const s = els.scroll;
    const r0 = rowAt(s.scrollTop);
    const r1 = Math.min(MAX_ROW, rowAt(s.scrollTop + (s.clientHeight || 600)) + 1);
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

  /** A cell drawn. @param {CellView} cell @param {string} [extra] */
  function cellHtml(cell, extra = "") {
    const st = styleOf(cell.style);
    const align = st?.align ?? (cell.kind === "n" ? "right" : cell.kind === "b" || cell.kind === "e" ? "center" : "left");
    const p = pos(cell.row, cell.col);
    let css = `left:${p.x}px;top:${p.y}px;width:${width(cell.col)}px;text-align:${align};`;
    if (st?.bold || cell.bold) css += "font-weight:700;";
    if (st?.italic) css += "font-style:italic;";
    if (st?.underline) css += "text-decoration:underline;";
    const color = cell.color ?? st?.color;
    if (color) css += `color:${color};`;
    const fill = cell.fill ?? st?.fill;
    if (fill) css += `background:${fill};`;
    const cls = `xl-c${cell.kind === "e" ? " err" : ""}${st?.wrap ? " wrap" : ""}${extra}`;
    return `<div class="${cls}" style="${css}" data-cell="${cell.row},${cell.col}">${esc(cell.text)}</div>`;
  }

  function draw() {
    if (!state.book) return;
    const s = els.scroll;
    const v = visible();
    const sel = selection();
    const [fr, fc] = frozen();
    const viewW = s.clientWidth || 900, viewH = s.clientHeight || 600;
    let cols = "", rows = "", cells = "", lines = "";
    // The frozen ones, then those on screen and not scrolled under them.
    const colList = [];
    for (let c = 0; c < fc; c++) colList.push(c);
    for (let c = Math.max(v.cols[0], fc); c <= v.cols[1]; c++) if (!fc || colX(c + 1) > s.scrollLeft + colX(fc)) colList.push(c);
    const rowList = [];
    for (let r = 0; r < fr; r++) rowList.push(r);
    for (let r = Math.max(v.rows[0], fr); r <= v.rows[1]; r++) if (!isHidden(r) && (!fr || rowY(r) + ROW > s.scrollTop + fr * ROW)) rowList.push(r);
    for (const c of colList) {
      const on = c >= sel.c0 && c <= sel.c1 ? " on" : "";
      const fz = c < fc ? " fz" : "";
      const left = HEAD_W + colX(c) + (c < fc ? s.scrollLeft : 0);
      cols += `<div class="xl-ch${on}${fz}" style="left:${left}px;width:${width(c)}px" data-col="${c}">${colName(c)}<i class="xl-resize" data-resize-col="${c}"></i></div>`;
      // The line after each column, as tall as the screen.
      lines += `<div class="xl-vl${fz}" style="left:${left + width(c) - 1}px;top:${s.scrollTop + HEAD_H}px;height:${viewH}px"></div>`;
    }
    for (const r of rowList) {
      const on = r >= sel.r0 && r <= sel.r1 ? " on" : "";
      const top = HEAD_H + rowY(r) + (r < fr ? s.scrollTop : 0);
      // A row after hidden ones says so, as Excel's numbers do.
      const gap = r > 0 && isHidden(r - 1) ? " gap" : "";
      rows += `<div class="xl-rh${on}${r < fr ? " fz" : ""}${gap}" style="top:${top}px" data-row="${r}">${r + 1}</div>`;
    }
    // What is frozen hides what scrolls under it.
    if (fr) cells += `<div class="xl-fzband" style="left:${s.scrollLeft + HEAD_W}px;top:${s.scrollTop + HEAD_H}px;width:${viewW}px;height:${fr * ROW}px"></div>`;
    if (fc) cells += `<div class="xl-fzband" style="left:${s.scrollLeft + HEAD_W}px;top:${s.scrollTop + HEAD_H}px;width:${colX(fc)}px;height:${viewH}px"></div>`;
    for (const cell of state.cells.values()) {
      const inRows = (cell.row >= v.rows[0] && cell.row <= v.rows[1]) || cell.row < fr;
      const inCols = (cell.col >= v.cols[0] && cell.col <= v.cols[1]) || cell.col < fc;
      if (!inRows || !inCols || (!cell.text && !cell.style)) continue;
      // Wholly under a frozen pane and not in it: not drawn. One partly
      // under it is, and the pane covers that part.
      if (isHidden(cell.row)) continue;
      if (fr && cell.row >= fr && rowY(cell.row) + ROW <= s.scrollTop + fr * ROW) continue;
      if (fc && cell.col >= fc && colX(cell.col + 1) <= s.scrollLeft + colX(fc)) continue;
      cells += cellHtml(cell, cell.row < fr || cell.col < fc ? " fz" : "");
    }
    cells += filterButtons(v);
    els.colHead.innerHTML = cols;
    els.rowHead.innerHTML = rows;
    els.cells.innerHTML = lines + cells + selectionBox(sel);
    els.colHead.style.transform = `translateX(${-s.scrollLeft}px)`;
    els.rowHead.style.transform = `translateY(${-s.scrollTop}px)`;
  }

  /** The filter's ▾ on each header cell in view. @param {{rows: [number, number], cols: [number, number]}} v */
  function filterButtons(v) {
    const f = tab()?.filter;
    const r = f ? parseRange(f.range) : null;
    if (!f || !r) return "";
    let out = "";
    for (let c = Math.max(r.c0, v.cols[0]); c <= Math.min(r.c1, v.cols[1]); c++) {
      const p = pos(r.r0, c);
      const on = f.columns.includes(c) ? " on" : "";
      out += `<button class="xl-filter${on}" data-filter-col="${c}" style="left:${p.x + width(c) - 20}px;top:${p.y + 3}px" title="Filter ${colName(c)}">▾</button>`;
    }
    return out;
  }

  /** The selected range, its corners in order. @returns {Sel} */
  function selection() {
    const [r, c] = state.at, [r2, c2] = state.to;
    if (state.visual === "V") return { r0: Math.min(r, r2), r1: Math.max(r, r2), c0: 0, c1: Math.max(0, (tab()?.cols ?? 1) - 1) };
    return { r0: Math.min(r, r2), r1: Math.max(r, r2), c0: Math.min(c, c2), c1: Math.max(c, c2) };
  }

  /** @param {Sel} sel */
  function selectionBox(sel) {
    const a = pos(sel.r0, sel.c0), b = pos(sel.r1, sel.c1);
    const w = b.x + width(sel.c1) - a.x, h = b.y + ROW - a.y;
    const [ar, ac] = state.at;
    const cur = pos(ar, ac);
    const range = sel.r0 !== sel.r1 || sel.c0 !== sel.c1;
    return (range ? `<div class="xl-range" style="left:${a.x}px;top:${a.y}px;width:${w}px;height:${h}px"></div>` : "") +
      `<div class="xl-cursor" style="left:${cur.x}px;top:${cur.y}px;width:${width(ac)}px;height:${ROW}px"></div>` +
      // The fill handle, at the selection's far corner.
      `<div class="xl-handle" data-fill-handle style="left:${a.x + w - 4}px;top:${a.y + h - 4}px"></div>`;
  }

  let frame = 0;
  function redraw() {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      draw();
    });
  }

  /** The cell as kept. @param {number} r @param {number} c */
  const cellAt = (r, c) => state.cells.get(key(r, c));

  /** The core is told what is selected, for `@sheet`, a moment after it settles. */
  let told = "";
  let telling = 0;
  function tellSelection() {
    clearTimeout(telling);
    telling = window.setTimeout(() => {
      const b = state.book;
      const range = rangeName(selection());
      const key = `${b?.generation}:${b?.active}:${range}`;
      if (!b || key === told) return;
      told = key;
      host.dispatch({ type: "sheetSelect", generation: b.generation, sheet: b.active, range });
    }, 150);
  }

  /** Show what the selected cell holds in the name box, the formula bar and the format bar. */
  function showSelected() {
    const [r, c] = state.at;
    tellSelection();
    els.name.value = rangeName(selection());
    if (state.editing !== "bar") els.bar.value = cellAt(r, c)?.input ?? "";
    const st = styleOf(cellAt(r, c)?.style ?? 0);
    for (const b of els.tools.querySelectorAll("[data-sheet-style]")) {
      const k = /** @type {HTMLElement} */ (b).dataset.sheetStyle ?? "";
      const on = k === "bold" ? st?.bold : k === "italic" ? st?.italic : k === "underline" ? st?.underline : k === "wrap" ? st?.wrap : st?.align === k;
      b.classList.toggle("on", !!on);
    }
    const fmt = /** @type {HTMLSelectElement | null} */ (els.tools.querySelector("[data-sheet-format]"));
    if (fmt) fmt.value = FORMATS.some(([f]) => f === (st?.format ?? "")) ? st?.format ?? "" : "";
  }

  /** Move the cursor, keeping it on screen; with `extend`, the range's far corner.
   *  @param {number} r @param {number} c @param {boolean} [extend] */
  function select(r, c, extend = false) {
    r = Math.max(0, Math.min(MAX_ROW, r));
    c = Math.max(0, Math.min(MAX_COL, c));
    const stretch = extend || !!state.visual;
    state.to = [r, c];
    if (!stretch) state.at = [r, c];
    if (r + 20 > state.extent[0] || c + 5 > state.extent[1]) {
      state.extent = [Math.max(state.extent[0], r + SPARE_ROWS), Math.max(state.extent[1], c + SPARE_COLS)];
      layout();
    }
    reveal(r, c);
    showSelected();
    redraw();
    ask();
  }

  /** Scroll a cell into view, past any frozen pane. @param {number} r @param {number} c */
  function reveal(r, c) {
    const s = els.scroll;
    const [fr, fc] = frozen();
    const top = rowY(r), left = colX(c);
    const viewH = s.clientHeight - HEAD_H, viewW = s.clientWidth - HEAD_W;
    if (r >= fr) {
      if (top < s.scrollTop + fr * ROW) s.scrollTop = top - fr * ROW;
      else if (top + ROW > s.scrollTop + viewH) s.scrollTop = top + ROW - viewH;
    }
    if (c >= fc) {
      if (left < s.scrollLeft + colX(fc)) s.scrollLeft = left - colX(fc);
      else if (left + width(c) > s.scrollLeft + viewW) s.scrollLeft = left + width(c) - viewW;
    }
  }

  /* ── commands ──────────────────────────────────────────────────────── */

  /** A command about the sheet shown, with the workbook's numbers. @param {Record<string, any>} command */
  function send(command) {
    const b = state.book;
    if (!b) return;
    host.dispatch({ generation: b.generation, sheet: b.active, ...command });
  }

  /** @param {Partial<Patch>} patch */
  const format = (patch) => send({ type: "sheetFormat", range: rangeName(selection()), patch: { bold: null, italic: null, underline: null, wrap: null, color: null, fill: null, align: null, format: null, ...patch } });

  /** Bold, italic, underline or wrap turned the other way from the cursor's. @param {"bold" | "italic" | "underline" | "wrap"} k */
  function toggle(k) {
    const st = styleOf(cellAt(state.at[0], state.at[1])?.style ?? 0);
    format({ [k]: !st?.[k] });
  }

  function copy(cut = false) {
    send({ type: "sheetCopy", range: rangeName(selection()), cut });
    leaveVisual();
  }

  async function paste() {
    const text = (await host.paste?.()) ?? null;
    if (text) send({ type: "sheetPaste", at: cellName(state.at[0], state.at[1]), text });
  }

  /** The selection emptied, as one step. */
  function clearSelection() {
    const sel = selection();
    /** @type {[string, string][]} */
    const cells = [];
    for (const cell of state.cells.values()) {
      if (cell.row >= sel.r0 && cell.row <= sel.r1 && cell.col >= sel.c0 && cell.col <= sel.c1 && cell.input) cells.push([cellName(cell.row, cell.col), ""]);
    }
    if (cells.length) send({ type: "sheetSet", cells });
    leaveVisual();
  }

  function leaveVisual() {
    if (!state.visual && state.at[0] === state.to[0] && state.at[1] === state.to[1]) return;
    state.visual = "";
    state.to = [...state.at];
    showSelected();
    redraw();
  }

  /** Rows (or columns) put in before the selection, or after it with `after`. @param {boolean} cols @param {boolean} after */
  function insertLines(cols, after) {
    const sel = selection();
    const n = cols ? sel.c1 - sel.c0 + 1 : 1;
    const at = cols ? (after ? sel.c1 + 1 : sel.c0) : after ? sel.r1 + 1 : sel.r0;
    send({ type: "sheetInsert", cols, at, n });
    if (!cols && after) select(state.at[0] + 1, state.at[1]);
  }

  /** The selection's rows (or columns) taken out. @param {boolean} cols */
  function deleteLines(cols) {
    const sel = selection();
    send({ type: "sheetDelete", cols, at: cols ? sel.c0 : sel.r0, n: cols ? sel.c1 - sel.c0 + 1 : sel.r1 - sel.r0 + 1 });
    leaveVisual();
  }

  /** ⌘D and ⌘R: the selection's first row (or column) down (or across) it. @param {boolean} across */
  function fillFrom(across) {
    const sel = selection();
    if (across ? sel.c0 === sel.c1 : sel.r0 === sel.r1) return;
    const from = across ? { ...sel, c1: sel.c0 } : { ...sel, r1: sel.r0 };
    send({ type: "sheetFill", from: rangeName(from), to: rangeName(sel) });
  }

  /** What a sort or a filter is over: the selection when it is more than a
   *  cell; else the filter's rows; else the rows in use under the first. */
  function region(withHeader = false) {
    const sel = selection();
    if (sel.r0 !== sel.r1 || sel.c0 !== sel.c1) return sel;
    const f = tab()?.filter ? parseRange(tab()?.filter?.range ?? "") : null;
    if (f && sel.r0 >= f.r0 && sel.r0 <= f.r1 && sel.c0 >= f.c0 && sel.c0 <= f.c1) return withHeader ? f : { ...f, r0: f.r0 + 1 };
    const rows = Math.max(1, tab()?.rows ?? 1), cols = Math.max(1, tab()?.cols ?? 1);
    return { r0: withHeader ? 0 : Math.min(1, rows - 1), r1: rows - 1, c0: 0, c1: cols - 1 };
  }

  /** The rows sorted by a column, the cursor's unless one is named. @param {boolean} down @param {number} [col] */
  function sortBy(down, col = state.at[1]) {
    const r = region();
    if (col < r.c0 || col > r.c1) return;
    send({ type: "sheetSort", range: rangeName(r), keys: [[col, !down]] });
  }

  /** A filter put on, or taken off when there is one. */
  function toggleFilter() {
    if (tab()?.filter) send({ type: "sheetFilter", range: null });
    else send({ type: "sheetFilter", range: rangeName(region(true)) });
  }

  /** A `:` command. @param {string} line */
  function run(line) {
    const [word = "", ...rest] = line.trim().split(/\s+/);
    const arg = rest.join(" ");
    const [r, c] = state.at;
    switch (word) {
      case "w": host.dispatch({ type: "sheetSave" }); return;
      case "q": host.dispatch({ type: "sheetClose", discard: false }); return;
      case "q!": host.dispatch({ type: "sheetClose", discard: true }); return;
      case "wq": host.dispatch({ type: "sheetSave" }); host.dispatch({ type: "sheetClose", discard: false }); return;
      case "freeze": send({ type: "sheetFreeze", rows: r, cols: c }); return;
      case "unfreeze": send({ type: "sheetFreeze", rows: 0, cols: 0 }); return;
      case "width": {
        const sel = selection();
        for (let col = sel.c0; col <= sel.c1; col++) send({ type: "sheetWidth", col, width: arg ? Number(arg) : null });
        return;
      }
      case "format": format({ format: arg }); return;
      case "bold": toggle("bold"); return;
      case "italic": toggle("italic"); return;
      case "wrap": toggle("wrap"); return;
      case "align": format({ align: arg }); return;
      case "color": format({ color: arg }); return;
      case "fill": format({ fill: arg }); return;
      case "insert": insertLines(arg.startsWith("col"), false); return;
      case "sort": {
        // `:sort`, `:sort desc`, `:sort B`, `:sort B desc`.
        const words = arg.split(/\s+/).filter(Boolean);
        const named = words.find((w) => /^[A-Za-z]{1,3}$/.test(w) && !/^(asc|desc)$/i.test(w));
        const col = named ? parseCell(`${named}1`)?.[1] : undefined;
        sortBy(words.some((w) => /^desc$/i.test(w)), col);
        return;
      }
      case "filter": toggleFilter(); return;
      case "nofilter": send({ type: "sheetFilter", range: null }); return;
      case "rules": openRules(); return;
      case "chart": addChart(["line", "bar", "scatter", "pie"].includes(arg) ? arg : "bar"); return;
      case "data": send({ type: "sheetToData", range: arg || rangeName(region(true)) }); return;
      case "delete": deleteLines(arg.startsWith("col")); return;
      default: {
        // `:B7` goes there.
        const to = parseCell(word);
        if (to) select(to[0], to[1]);
        else host.dispatch({ type: "notice", text: `:${word} is not a sheet command`, level: "warn" });
      }
    }
  }

  /* ── editing ───────────────────────────────────────────────────────── */

  /** Start editing the selected cell, in place, with what is there or `text`. @param {string | null} text */
  function editCell(text) {
    if (!state.book) return;
    const [r, c] = state.at;
    state.editing = "cell";
    leaveVisual();
    const e = els.editor;
    const p = pos(r, c);
    e.hidden = false;
    e.style.left = `${p.x - els.scroll.scrollLeft}px`;
    e.style.top = `${p.y - els.scroll.scrollTop}px`;
    e.style.minWidth = `${width(c)}px`;
    e.value = text ?? cellAt(r, c)?.input ?? "";
    e.focus({ preventScroll: true });
    const end = e.value.length;
    e.setSelectionRange(end, end);
    els.bar.value = e.value;
  }

  /** Put what was typed into the cell, and move on. @param {string} value @param {[number, number]} [move] */
  function commit(value, move = [1, 0]) {
    const was = state.editing;
    stopEditing();
    if (!state.book || !was) return;
    const [r, c] = state.at;
    if ((cellAt(r, c)?.input ?? "") !== value) send({ type: "sheetSet", cells: [[cellName(r, c), value]] });
    select(r + move[0], c + move[1]);
    els.scroll.focus({ preventScroll: true });
  }

  function stopEditing() {
    state.editing = "";
    els.editor.hidden = true;
    els.editor.blur();
    showSelected();
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

  // The name box: a cell's name, or a range's, goes there.
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

  // `:` and a command, run on Enter.
  els.cmd.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      const line = els.cmd.value;
      els.cmd.hidden = true;
      els.cmd.value = "";
      els.scroll.focus({ preventScroll: true });
      run(line);
    } else if (e.key === "Escape" || (e.key === "Backspace" && !els.cmd.value)) {
      e.preventDefault();
      els.cmd.hidden = true;
      els.scroll.focus({ preventScroll: true });
    }
    e.stopPropagation();
  });
  els.cmd.addEventListener("blur", () => {
    els.cmd.hidden = true;
  });

  /* ── the format bar ────────────────────────────────────────────────── */

  els.tools.innerHTML = `
    <button class="xl-tool" data-sheet-style="bold" title="Bold  ⌘B"><b>B</b></button>
    <button class="xl-tool" data-sheet-style="italic" title="Italic  ⌘I"><i>I</i></button>
    <button class="xl-tool" data-sheet-style="underline" title="Underline  ⌘U"><u>U</u></button>
    <span class="xl-tool-sep"></span>
    <button class="xl-tool" data-sheet-style="left" title="Align left">⇤</button>
    <button class="xl-tool" data-sheet-style="center" title="Centre">↔</button>
    <button class="xl-tool" data-sheet-style="right" title="Align right">⇥</button>
    <button class="xl-tool" data-sheet-style="wrap" title="Wrap text">↵</button>
    <span class="xl-tool-sep"></span>
    <select class="xl-tool-format" data-sheet-format title="Number format">${FORMATS.map(([f, label]) => `<option value="${esc(f)}">${esc(label)}</option>`).join("")}</select>
    <label class="xl-tool xl-swatch" title="Text colour"><span>A</span><input type="color" data-sheet-color value="#e06c75"></label>
    <label class="xl-tool xl-swatch" title="Fill"><span class="xl-fill-icon"></span><input type="color" data-sheet-fill value="#3d375e"></label>
    <button class="xl-tool" data-sheet-clear-style title="Clear formatting">⌫</button>
    <span class="xl-tool-sep"></span>
    <button class="xl-tool" data-sheet-sort="up" title="Sort A→Z by this column  :sort">A↓</button>
    <button class="xl-tool" data-sheet-sort="down" title="Sort Z→A by this column  :sort desc">Z↓</button>
    <button class="xl-tool" data-sheet-filter title="Filter  :filter">▾</button>
    <button class="xl-tool" data-sheet-rules title="Conditional colour  :rules">◐</button>
    <button class="xl-tool" data-sheet-chart title="A chart of the selection  :chart">▥</button>
    <button class="xl-tool" data-sheet-to-data title="Send the selection to Data as a dataframe  :data">→ Data</button>`;
  els.tools.addEventListener("mousedown", (e) => {
    // Clicking a tool leaves the keyboard with the grid.
    if (e.target instanceof Element && e.target.closest("button")) e.preventDefault();
  });
  els.tools.addEventListener("click", (e) => {
    const b = e.target instanceof Element ? /** @type {HTMLElement | null} */ (e.target.closest("[data-sheet-style], [data-sheet-clear-style], [data-sheet-sort], [data-sheet-filter], [data-sheet-rules], [data-sheet-chart], [data-sheet-to-data]")) : null;
    if (!b || !state.book) return;
    if (b.dataset.sheetSort) return sortBy(b.dataset.sheetSort === "down");
    if (b.dataset.sheetFilter !== undefined) return toggleFilter();
    if (b.dataset.sheetRules !== undefined) return openRules();
    if (b.dataset.sheetChart !== undefined) return addChart();
    if (b.dataset.sheetToData !== undefined) return send({ type: "sheetToData", range: rangeName(region(true)) });
    const k = b.dataset.sheetStyle;
    if (b.dataset.sheetClearStyle !== undefined) format({ bold: false, italic: false, underline: false, wrap: false, color: "", fill: "", align: "", format: "" });
    else if (k === "bold" || k === "italic" || k === "underline" || k === "wrap") toggle(k);
    else if (k) format({ align: b.classList.contains("on") ? "" : k });
  });
  els.tools.querySelector("[data-sheet-format]")?.addEventListener("change", (e) => {
    format({ format: /** @type {HTMLSelectElement} */ (e.target).value });
    els.scroll.focus({ preventScroll: true });
  });
  for (const [sel, k] of /** @type {const} */ ([["[data-sheet-color]", "color"], ["[data-sheet-fill]", "fill"]])) {
    els.tools.querySelector(sel)?.addEventListener("change", (e) => {
      format({ [k]: /** @type {HTMLInputElement} */ (e.target).value });
      els.scroll.focus({ preventScroll: true });
    });
  }

  /* ── the filter's list, and the colour rules ───────────────────────── */

  /** A panel over the grid; one at a time, gone with Escape or a click elsewhere. */
  let popover = /** @type {HTMLElement | null} */ (null);
  function closePopover() {
    popover?.remove();
    popover = null;
  }
  /** @param {string} html @param {number} x @param {number} y */
  function openPopover(html, x, y) {
    closePopover();
    const el = document.createElement("div");
    el.className = "xl-pop";
    el.innerHTML = html;
    const grid = /** @type {HTMLElement} */ (els.scroll.parentElement);
    grid.append(el);
    el.style.left = `${Math.max(4, Math.min(x, grid.clientWidth - el.offsetWidth - 8))}px`;
    el.style.top = `${Math.max(4, Math.min(y, grid.clientHeight - el.offsetHeight - 8))}px`;
    el.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        e.preventDefault();
        closePopover();
        els.scroll.focus({ preventScroll: true });
      }
      e.stopPropagation();
    });
    popover = el;
    return el;
  }
  document.addEventListener("mousedown", (e) => {
    if (popover && e.target instanceof Node && !popover.contains(e.target)) closePopover();
  }, true);

  /** The values a filtered column has, to tick, and a condition.
   *  @param {{col: number, values: [string, number][], ticked: string[] | null, criteria: string}} event */
  function showFilter(event) {
    const f = tab()?.filter ? parseRange(tab()?.filter?.range ?? "") : null;
    if (!f) return;
    const ticked = event.ticked ? new Set(event.ticked) : null;
    const p = pos(f.r0, event.col);
    const rows = event.values
      .map(([v, n], i) => `<label class="xl-pop-row"><input type="checkbox" data-value="${i}" ${!ticked || ticked.has(v) ? "checked" : ""}>
        <span>${v === "" ? "<i>(empty)</i>" : esc(v)}</span><span class="xl-pop-n">${n}</span></label>`)
      .join("");
    const el = openPopover(`<div class="xl-pop-title">Filter ${colName(event.col)}</div>
      <label class="xl-pop-row xl-pop-all"><input type="checkbox" data-all ${!ticked ? "checked" : ""}><span>Select all</span></label>
      <div class="xl-pop-list">${rows}</div>
      <input class="xl-pop-field" data-criteria placeholder="Condition: >10, <>0, ap*" value="${esc(event.criteria)}">
      <div class="xl-pop-actions"><button data-clear>Clear</button><button class="primary" data-apply>Apply</button></div>`,
      p.x - els.scroll.scrollLeft, p.y - els.scroll.scrollTop + ROW);
    const boxes = () => /** @type {HTMLInputElement[]} */ ([...el.querySelectorAll("[data-value]")]);
    el.querySelector("[data-all]")?.addEventListener("change", (e) => {
      for (const b of boxes()) b.checked = /** @type {HTMLInputElement} */ (e.target).checked;
    });
    el.querySelector("[data-apply]")?.addEventListener("click", () => {
      const all = boxes().every((b) => b.checked);
      const values = all ? null : boxes().filter((b) => b.checked).map((b) => event.values[Number(b.dataset.value)][0]);
      const criteria = /** @type {HTMLInputElement} */ (el.querySelector("[data-criteria]")).value;
      send({ type: "sheetFilterSet", col: event.col, values, criteria });
      closePopover();
      els.scroll.focus({ preventScroll: true });
    });
    el.querySelector("[data-clear]")?.addEventListener("click", () => {
      send({ type: "sheetFilterSet", col: event.col, values: null, criteria: "" });
      closePopover();
      els.scroll.focus({ preventScroll: true });
    });
  }

  const WHEN = /** @type {[string, string][]} */ ([
    ["greater", "Greater than"], ["less", "Less than"], ["between", "Between"], ["equal", "Equal to"], ["contains", "Text contains"],
    ["duplicates", "Duplicates"], ["top", "Top"], ["bottom", "Bottom"], ["scale", "Colour scale"],
  ]);

  /** A rule in words. @param {import("./bindings/SheetRuleView").SheetRuleView} r */
  function ruleWords(r) {
    const w = r.when;
    switch (w.when) {
      case "greater": return `> ${w.value}`;
      case "less": return `< ${w.value}`;
      case "between": return `${w.low} to ${w.high}`;
      case "equal": return `= ${w.value}`;
      case "contains": return `contains “${w.text}”`;
      case "duplicates": return "duplicates";
      case "top": return `top ${w.n}`;
      case "bottom": return `bottom ${w.n}`;
      default: return "colour scale";
    }
  }

  /** The sheet's conditional colours, and a form to add one over the selection. */
  function openRules() {
    const t = tab();
    if (!t) return;
    const list = t.rules
      .map((r, i) => `<div class="xl-pop-row"><i class="xl-pop-swatch" style="background:${esc(r.fill ?? "transparent")};color:${esc(r.color ?? "inherit")}">${r.bold ? "B" : "a"}</i>
        <span>${esc(r.range)} · ${esc(ruleWords(r))}</span><button class="xl-pop-x" data-remove-rule="${i}" title="Remove">✕</button></div>`)
      .join("") || `<p class="xl-pop-none">No colour rules on this sheet.</p>`;
    const el = openPopover(`<div class="xl-pop-title">Conditional colour</div><div class="xl-pop-list">${list}</div>
      <div class="xl-pop-form">
        <input class="xl-pop-field" data-rule-range value="${esc(rangeName(selection()))}" aria-label="Range">
        <select data-rule-when>${WHEN.map(([k, label]) => `<option value="${k}">${label}</option>`).join("")}</select>
        <input class="xl-pop-field" data-rule-a placeholder="Value" aria-label="Value">
        <input class="xl-pop-field" data-rule-b placeholder="and" aria-label="Second value" hidden>
        <label class="xl-pop-row">Fill <input type="color" data-rule-fill value="#f6c1c1"> Text <input type="color" data-rule-color value="#000000"><input type="checkbox" data-rule-text> <span>colour the text</span></label>
        <label class="xl-pop-row"><input type="checkbox" data-rule-bold><span>Bold</span></label>
      </div>
      <div class="xl-pop-actions"><button class="primary" data-add-rule>Add</button></div>`, 80, 8);
    const when = /** @type {HTMLSelectElement} */ (el.querySelector("[data-rule-when]"));
    const a = /** @type {HTMLInputElement} */ (el.querySelector("[data-rule-a]"));
    const b = /** @type {HTMLInputElement} */ (el.querySelector("[data-rule-b]"));
    const shape = () => {
      const k = when.value;
      a.hidden = k === "duplicates";
      b.hidden = k !== "between" && k !== "scale";
      a.placeholder = k === "scale" ? "Low colour #hex" : k === "top" || k === "bottom" ? "How many" : "Value";
      b.placeholder = k === "scale" ? "High colour #hex" : "and";
      if (k === "scale" && !a.value) { a.value = "#f8696b"; b.value = "#63be7b"; }
    };
    when.addEventListener("change", shape);
    shape();
    el.addEventListener("click", (e) => {
      const t = e.target instanceof HTMLElement ? e.target : null;
      if (t?.dataset.removeRule !== undefined) {
        send({ type: "sheetRuleRemove", index: Number(t.dataset.removeRule) });
        closePopover();
      } else if (t?.dataset.addRule !== undefined) {
        const k = when.value, num = (/** @type {string} */ v) => Number(v);
        /** @type {any} */
        const w = k === "greater" || k === "less" ? { when: k, value: num(a.value) }
          : k === "between" ? { when: k, low: num(a.value), high: num(b.value) }
          : k === "equal" ? { when: k, value: a.value }
          : k === "contains" ? { when: k, text: a.value }
          : k === "top" || k === "bottom" ? { when: k, n: Math.max(1, Math.round(num(a.value) || 10)) }
          : k === "scale" ? { when: k, low: a.value, high: b.value } : { when: "duplicates" };
        const text = /** @type {HTMLInputElement} */ (el.querySelector("[data-rule-text]")).checked;
        const rule = {
          range: /** @type {HTMLInputElement} */ (el.querySelector("[data-rule-range]")).value.trim(),
          when: w,
          fill: k === "scale" ? null : /** @type {HTMLInputElement} */ (el.querySelector("[data-rule-fill]")).value,
          color: text ? /** @type {HTMLInputElement} */ (el.querySelector("[data-rule-color]")).value : null,
          bold: /** @type {HTMLInputElement} */ (el.querySelector("[data-rule-bold]")).checked,
        };
        if ((w.value !== undefined && Number.isNaN(w.value)) || (w.low !== undefined && typeof w.low === "number" && (Number.isNaN(w.low) || Number.isNaN(w.high)))) return;
        send({ type: "sheetRuleAdd", rule });
        closePopover();
        els.scroll.focus({ preventScroll: true });
      }
    });
  }

  /* ── keys and the mouse ────────────────────────────────────────────── */

  els.scroll.addEventListener("scroll", () => {
    const s = els.scroll;
    // Near the end, the grid goes on.
    if (s.scrollTop + s.clientHeight > rowY(state.extent[0] - 40)) {
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

  /** The cell under a point in the grid, frozen panes counted. @param {MouseEvent} e @returns {[number, number]} */
  function cellUnder(e) {
    const box = els.scroll.getBoundingClientRect();
    const [fr, fc] = frozen();
    const vx = e.clientX - box.left - HEAD_W, vy = e.clientY - box.top - HEAD_H;
    const row = vy < fr * ROW ? Math.floor(vy / ROW) : rowAt(vy + els.scroll.scrollTop);
    const col = vx < colX(fc) ? colAt(Math.max(0, vx)) : colAt(Math.max(0, vx + els.scroll.scrollLeft));
    return [Math.max(0, row), col];
  }

  /** What a drag is doing: selecting, filling from the handle, or widening a column. */
  let drag = /** @type {null | {kind: "select"} | {kind: "fill", from: Sel} | {kind: "width", col: number, x: number, w: number}
    | {kind: "chart" | "chartSize", i: number, x: number, y: number, el: HTMLElement, left: number, top: number, w: number, h: number}} */ (null);
  els.charts.addEventListener("mousedown", (e) => {
    const t = e.target instanceof HTMLElement ? e.target : null;
    const el = /** @type {HTMLElement | null} */ (t?.closest(".xl-chart") ?? null);
    if (!t || !el || e.button !== 0) return;
    if (t.closest("select, button")) return;
    const i = Number(el.dataset.chart);
    const move = t.closest("[data-chart-drag]") && !t.closest("[data-chart-title]") ? "chart" : t.dataset.chartSize !== undefined ? "chartSize" : null;
    if (!move) return;
    drag = { kind: move, i, x: e.clientX, y: e.clientY, el, left: el.offsetLeft, top: el.offsetTop, w: el.offsetWidth, h: el.offsetHeight };
    e.preventDefault();
  });
  els.charts.addEventListener("change", (e) => {
    const t = e.target instanceof HTMLSelectElement ? e.target : null;
    if (t?.dataset.chartKind !== undefined) setChart(Number(t.dataset.chartKind), { kind: /** @type {any} */ (t.value) });
  });
  els.charts.addEventListener("click", (e) => {
    const t = e.target instanceof HTMLElement ? e.target : null;
    if (t?.dataset.chartRemove !== undefined) send({ type: "sheetChartRemove", index: Number(t.dataset.chartRemove) });
  });
  els.charts.addEventListener("dblclick", (e) => {
    const t = e.target instanceof HTMLElement ? e.target : null;
    if (t?.dataset.chartTitle === undefined) return;
    e.stopPropagation();
    const i = Number(t.dataset.chartTitle);
    const field = document.createElement("input");
    field.className = "xl-chart-rename";
    field.value = tab()?.charts[i]?.chart.title ?? "";
    t.replaceChildren(field);
    field.focus();
    let done = false;
    const finish = (/** @type {boolean} */ keep) => {
      if (done) return;
      done = true;
      if (keep) setChart(i, { title: field.value.trim() });
      else renderCharts();
    };
    field.addEventListener("keydown", (k) => {
      if (k.key === "Enter") finish(true);
      else if (k.key === "Escape") finish(false);
      k.stopPropagation();
    });
    field.addEventListener("blur", () => finish(true));
  });
  els.scroll.addEventListener("mousedown", (e) => {
    if (e.button !== 0 || !state.book) return;
    const t = e.target instanceof HTMLElement ? e.target : null;
    if (t?.closest(".xl-chart")) return;
    if (t?.dataset.filterCol !== undefined) {
      e.preventDefault();
      send({ type: "sheetFilterValues", col: Number(t.dataset.filterCol) });
      return;
    }
    if (state.editing) commit(state.editing === "cell" ? els.editor.value : els.bar.value, [0, 0]);
    if (t?.dataset.fillHandle !== undefined) {
      drag = { kind: "fill", from: selection() };
    } else {
      const [r, c] = cellUnder(e);
      if (!e.shiftKey) state.visual = "";
      select(r, c, e.shiftKey);
      drag = { kind: "select" };
    }
    els.scroll.focus({ preventScroll: true });
    e.preventDefault();
  });
  // The letters and numbers: a click takes the column or the row, a
  // letter's edge widens its column, and a double-click there resets it.
  const heads = /** @type {HTMLElement} */ (els.colHead.parentElement);
  heads.addEventListener("mousedown", (e) => {
    const t = e.target instanceof HTMLElement ? e.target : null;
    if (!t || !state.book || e.button !== 0) return;
    if (t.dataset.resizeCol !== undefined) {
      const col = Number(t.dataset.resizeCol);
      drag = { kind: "width", col, x: e.clientX, w: width(col) };
      e.preventDefault();
      return;
    }
    const head = t.closest("[data-col], [data-row]");
    if (!(head instanceof HTMLElement)) return;
    if (head.dataset.col !== undefined) {
      const c = Number(head.dataset.col);
      state.visual = "";
      state.at = [0, c];
      select(Math.max(0, (tab()?.rows ?? 1) - 1), c, true);
    } else {
      const r = Number(head.dataset.row);
      state.visual = "V";
      state.at = [r, 0];
      select(r, 0, true);
    }
    els.scroll.focus({ preventScroll: true });
    e.preventDefault();
  });
  heads.addEventListener("dblclick", (e) => {
    const t = e.target instanceof HTMLElement ? e.target : null;
    if (t?.dataset.resizeCol !== undefined) send({ type: "sheetWidth", col: Number(t.dataset.resizeCol), width: null });
  });
  window.addEventListener("mousemove", (e) => {
    if (!drag) return;
    if (drag.kind === "chart") {
      drag.el.style.left = `${Math.max(HEAD_W, drag.left + e.clientX - drag.x)}px`;
      drag.el.style.top = `${Math.max(HEAD_H, drag.top + e.clientY - drag.y)}px`;
      return;
    }
    if (drag.kind === "chartSize") {
      drag.el.style.width = `${Math.max(160, drag.w + e.clientX - drag.x)}px`;
      drag.el.style.height = `${Math.max(120, drag.h + e.clientY - drag.y)}px`;
      return;
    }
    if (drag.kind === "width") {
      widths.set(drag.col, Math.max(16, drag.w + e.clientX - drag.x));
      layout();
      redraw();
      return;
    }
    const [r, c] = cellUnder(e);
    if (drag.kind === "select") {
      if (r !== state.to[0] || c !== state.to[1]) select(r, c, true);
    } else if (drag.kind === "fill") {
      // The fill goes the way the pointer went furthest: down or across.
      const f = drag.from;
      const down = Math.abs(r - f.r1) >= Math.abs(c - f.c1);
      state.at = [f.r0, f.c0];
      select(down ? Math.max(r, f.r1) : f.r1, down ? f.c1 : Math.max(c, f.c1), true);
    }
  });
  window.addEventListener("mouseup", () => {
    const d = drag;
    drag = null;
    if (!d || !state.book) return;
    if (d.kind === "chart") {
      // It sits on the cell its corner was dropped on.
      const r = rowAt(d.el.offsetTop - HEAD_H), c = colAt(d.el.offsetLeft - HEAD_W);
      setChart(d.i, { at: cellName(r, c) });
    } else if (d.kind === "chartSize") {
      setChart(d.i, { width: d.el.offsetWidth, height: d.el.offsetHeight });
    } else if (d.kind === "width") {
      send({ type: "sheetWidth", col: d.col, width: Math.round(width(d.col) * 75) / 100 });
    } else if (d.kind === "fill") {
      const to = selection();
      if (to.r1 !== d.from.r1 || to.c1 !== d.from.c1) send({ type: "sheetFill", from: rangeName(d.from), to: rangeName(to) });
    }
  });
  els.scroll.addEventListener("dblclick", (e) => {
    if (!state.book) return;
    const [r, c] = cellUnder(e);
    select(r, c);
    editCell(null);
  });

  ws.addEventListener("keydown", (e) => {
    const b = state.book;
    // ⌘S saves wherever the focus is in the mode.
    if (b && e.metaKey && !e.altKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      if (state.editing) commit(state.editing === "cell" ? els.editor.value : els.bar.value, [0, 0]);
      host.dispatch({ type: "sheetSave" });
      return;
    }
    if (e.target !== els.scroll || !b) return;
    if (e.ctrlKey && /^[0-9]$/.test(e.key)) return; // modes, the window's
    const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    if (e.metaKey) {
      /** @type {Record<string, () => void>} */
      const chords = {
        z: () => host.dispatch({ type: e.shiftKey ? "sheetRedo" : "sheetUndo" }),
        c: () => copy(false),
        x: () => copy(true),
        v: () => void paste(),
        d: () => fillFrom(false),
        r: () => fillFrom(true),
        b: () => toggle("bold"),
        i: () => toggle("italic"),
        u: () => toggle("underline"),
        a: () => {
          state.at = [0, 0];
          select(Math.max(0, (tab()?.rows ?? 1) - 1), Math.max(0, (tab()?.cols ?? 1) - 1), true);
        },
      };
      if (chords[k]) {
        e.preventDefault();
        chords[k]();
      }
      return;
    }
    const [r, c] = state.at;
    const [tr, tc] = state.to;
    const ext = e.shiftKey;
    const from = ext || state.visual ? [tr, tc] : [r, c];
    const pending = state.pending;
    state.pending = "";
    // The arrows, and Vim's h j k l; with ⇧ or in visual they stretch the selection.
    /** @type {Record<string, [number, number]>} */
    const moves = { ArrowDown: [1, 0], ArrowUp: [-1, 0], ArrowLeft: [0, -1], ArrowRight: [0, 1], j: [1, 0], k: [-1, 0], h: [0, -1], l: [0, 1] };
    const move = e.ctrlKey || e.altKey ? undefined : moves[k];
    if (move) {
      e.preventDefault();
      select(step(from[0], move[0]), from[1] + move[1], ext);
      return;
    }
    if (e.ctrlKey && k === "r") {
      e.preventDefault();
      host.dispatch({ type: "sheetRedo" });
      return;
    }
    if (e.ctrlKey || e.altKey) return;
    /** @type {Record<string, () => void>} */
    const keys = {
      // The ends: the first column, the last in use, the top, the last row in use.
      "0": () => select(from[0], 0, ext),
      "^": () => select(from[0], 0, ext),
      "$": () => select(from[0], Math.max(0, (tab()?.cols ?? 1) - 1), ext),
      G: () => select(Math.max(0, (tab()?.rows ?? 1) - 1), from[1], ext),
      g: () => {
        if (pending === "g") select(0, from[1], ext);
        else state.pending = "g";
      },
      Enter: () => editCell(null),
      F2: () => editCell(null),
      i: () => editCell(null),
      a: () => editCell(null),
      c: () => editCell(""),
      Tab: () => select(r, c + (ext ? -1 : 1)),
      Delete: () => clearSelection(),
      Backspace: () => clearSelection(),
      x: () => clearSelection(),
      u: () => host.dispatch({ type: "sheetUndo" }),
      v: () => {
        state.visual = state.visual === "v" ? "" : "v";
        if (!state.visual) state.to = [...state.at];
        showSelected();
        redraw();
      },
      V: () => {
        state.visual = state.visual === "V" ? "" : "V";
        if (!state.visual) state.to = [...state.at];
        showSelected();
        redraw();
      },
      y: () => {
        if (state.visual || pending === "y") copy(false);
        else state.pending = "y";
      },
      d: () => {
        if (state.visual === "V" || pending === "d") deleteLines(false);
        else if (state.visual) copy(true);
        else state.pending = "d";
      },
      p: () => void paste(),
      o: () => insertLines(false, true),
      O: () => insertLines(false, false),
      ":": () => {
        els.cmd.hidden = false;
        els.cmd.value = "";
        els.cmd.focus();
      },
      Escape: () => {
        state.visual = "";
        select(r, c);
      },
    };
    const name = e.key === "G" || e.key === "V" || e.key === "O" ? e.key : k;
    const act = keys[name];
    if (act) {
      e.preventDefault();
      act();
      return;
    }
    // Anything else typed starts the cell over with it, as in every
    // spreadsheet; `=` starts a formula.
    if (e.key.length === 1) {
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
        state.visual = "";
        els.editor.hidden = true;
      }
      renderBook();
    },
    /** @param {CellsEvent} event */
    cells(event) {
      const b = state.book;
      if (!b || event.generation !== b.generation || event.sheet !== b.active) return;
      if (!event.partial) {
        const [fr, fc] = frozen();
        for (const k of [...state.cells.keys()]) {
          const [r, c] = k.split(",").map(Number);
          const inRows = (r >= event.rows[0] && r <= event.rows[1]) || r < fr;
          const inCols = (c >= event.cols[0] && c <= event.cols[1]) || c < fc;
          if (inRows && inCols) state.cells.delete(k);
        }
      }
      for (const cell of event.cells) {
        if (cell.input || cell.style || cell.text) state.cells.set(key(cell.row, cell.col), cell);
        else state.cells.delete(key(cell.row, cell.col));
      }
      showSelected();
      redraw();
    },
    /** @param {{col: number, values: [string, number][], ticked: string[] | null, criteria: string}} event */
    filterValues(event) {
      showFilter(event);
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
  };
}

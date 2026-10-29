/* Circuit: a project's circuits drawn and edited on their grid, its projects
 * when none is open.
 *
 * The core sends a circuit with every part's pins already placed — where
 * the simulator joins them — so a pin drawn here is a pin that connects.
 * Each part is drawn in its own frame: turned and mirrored as it is, the
 * pins taken back into that frame to size its body, so a part the window
 * has no shape for is still a box exactly around its pins.
 *
 * One 2D canvas, redrawn on a frame when anything moves. The wheel pans,
 * ⌘-wheel and a pinch zoom about the pointer, `=` fits the circuit to the
 * window. A project is `generation`-numbered by the core; the view is kept
 * while the same one is redrawn, and fitted when another opens.
 *
 * Editing is the core's: every change goes to it as ops (`circuitEdit`) and
 * comes back as the circuit redrawn, so the pins of a part placed or turned
 * are the simulator's. Here are only the gestures: a cursor on the grid
 * moved by h j k l, a selection, and drags — from a pin or a wire's end to
 * draw a wire, from a part to move what is selected, from nothing to select
 * a box of it. ⌥-drag pans.
 *
 * Running is the core's too: ⌘R starts it, and it sends every wire's value
 * after each change, which colours the wires — high, low, high-Z — and
 * labels buses in hex. A click on an input flips it (a wider one asks for a
 * number), a button is held while pressed, `s` is a clock edge and `.` a
 * gate delay; the clock can be left running at a few hertz, sent from here
 * an edge at a time, or quick-run by the core as fast as it goes.
 *
 * Below the drawing, `t` opens the circuit's test tables — written here,
 * kept by the core as held edits, run there, the rows that failed shown —
 * and a timing graph of what the run has recorded, which can go to Data. */

import { escapeHtml as esc, icon } from "./html.js";

/** @typedef {import("./bindings/CircuitProjectView").CircuitProjectView} ProjectView */
/** @typedef {import("./bindings/CircuitPartView").CircuitPartView} PartView */
/** @typedef {import("./bindings/CircuitFound").CircuitFound} Found */
/** @typedef {import("./bindings/CircuitRunView").CircuitRunView} RunView */
/** @typedef {import("./bindings/CircuitValue").CircuitValue} Value */
/** @typedef {import("./bindings/CircuitTestResult").CircuitTestResult} TestResult */
/** @typedef {import("./bindings/CircuitAnalysisView").CircuitAnalysisView} Analysis */
/** @typedef {import("./bindings/CircuitMachineView").CircuitMachineView} MachineView */

/** No more rows of a truth table than this are drawn. */
const MOST_TRUTH_ROWS = 256;

/** The Gray code of `n` bits, in order: 00 01 11 10 for two. @param {number} n */
export function gray(n) {
  return Array.from({ length: 1 << n }, (_, i) => i ^ (i >> 1));
}

/** A Karnaugh map's layout for `n` variables: the first half (rounded
 *  down) down the side, the rest across, each in Gray order; and which row
 *  of the table each cell is. @param {number} n */
export function kmap(n) {
  const rv = Math.floor(n / 2), cv = n - rv;
  const rows = gray(rv), cols = gray(cv);
  return { rv, cv, rows, cols, row: (/** @type {number} */ r, /** @type {number} */ c) => (rows[r] << cv) | cols[c] };
}

/** Runs of consecutive positions, as [first, last]: a group that wraps is
 *  two. @param {number[]} at */
function runs(at) {
  const out = /** @type {[number, number][]} */ ([]);
  for (const p of [...at].sort((a, b) => a - b)) {
    const last = out.at(-1);
    if (last && p === last[1] + 1) last[1] = p;
    else out.push([p, p]);
  }
  return out;
}

/** Pixels: a sample's width in the graph, a signal's row, the names' column. */
const SAMPLE_W = 16;
const SIGNAL_H = 26;
const NAMES_W = 72;

/** A value as a part shows it: a bit as 0 or 1, a bus in hex, Z as Z.
 *  @param {Value | undefined} v */
export function shownValue(v) {
  if (!v) return "";
  if (v.z) return "Z";
  return v.bits === 1 ? v.hex : `${v.hex.toUpperCase()}`;
}

/** Zoom, in pixels a grid step. */
const MIN_SCALE = 3;
const MAX_SCALE = 80;
const FIT_SCALE = 22;

/** The gates drawn as their shapes: their body's width in steps. */
const GATES = new Set(["and", "nand", "or", "nor", "xor", "xnor"]);
const INVERTED = new Set(["nand", "nor", "xnor", "not"]);

/** Short names for the boxes drawn as boxes. */
const BOX_NAMES = /** @type {Record<string, string>} */ ({
  mux: "MUX", demux: "DEMUX", decoder: "DEC", priority: "PRI", add: "+", subtract: "−", multiply: "×", divide: "÷",
  compare: "CMP", negate: "−x", shifter: "SHIFT", register: "REG", dff: "D", tff: "T", jkff: "JK", rsff: "RS", ram2: "RAM",
  ram: "RAM", rom: "ROM", lut: "LUT", counter: "CTR", driver: "EN", driverinv: "EN̄", bitext: "EXT", bitsel: "BIT",
});
/** The displays: drawn as a face, lit from their value while running. */
const DISPLAYS = new Set(["seg7", "hexdisplay", "rgbled"]);
/** A hex digit's segments, a to g as bits 0 to 6. */
const DIGITS = [0x3f, 0x06, 0x5b, 0x4f, 0x66, 0x6d, 0x7d, 0x07, 0x7f, 0x6f, 0x77, 0x7c, 0x39, 0x5e, 0x79, 0x71];

/** A part's setting. @param {PartView} p @param {string} k */
const setting = (p, k) => p.settings.find(([key]) => key === k)?.[1] ?? "";

/** A pin taken back into its part's own frame: the turn undone, then the
 *  mirror. The core turns (x, y) to (y, −x) a quarter at a time.
 *  @param {PartView} p @param {number} x @param {number} y @returns {[number, number]} */
export function local(p, x, y) {
  let dx = x - p.x, dy = y - p.y;
  for (let i = 0; i < p.rot % 4; i++) [dx, dy] = [-dy, dx];
  if (p.mirror) dy = -dy;
  return [dx, dy];
}

/** The box around a part's pins in its own frame, at least a step each way.
 *  @param {PartView} p */
export function frame(p) {
  const pts = p.pins.map((pin) => local(p, pin.x, pin.y));
  pts.push([0, 0]);
  const xs = pts.map((q) => q[0]), ys = pts.map((q) => q[1]);
  return { x0: Math.min(...xs), x1: Math.max(...xs), y0: Math.min(...ys), y1: Math.max(...ys) };
}

/** Where everything is, in grid steps: parts' frames turned back out, and
 *  wires. @param {ProjectView} v */
export function extent(v) {
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  const take = (/** @type {number} */ x, /** @type {number} */ y) => {
    x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y);
  };
  for (const w of v.wires) { take(w[0], w[1]); take(w[2], w[3]); }
  for (const p of v.parts) {
    take(p.x, p.y);
    for (const pin of p.pins) take(pin.x, pin.y);
  }
  // Room for the labels drawn beside inputs and outputs.
  return Number.isFinite(x0) ? { x0: x0 - 4, y0: y0 - 2, x1: x1 + 4, y1: y1 + 2 } : null;
}

/** A point of a part's own frame put out on the grid: mirrored, then turned
 *  a quarter at a time as the core turns it. @param {PartView} p @param {number} lx @param {number} ly @returns {[number, number]} */
export function world(p, lx, ly) {
  let x = lx, y = p.mirror ? -ly : ly;
  for (let i = 0; i < p.rot % 4; i++) [x, y] = [y, -x];
  return [p.x + x, p.y + y];
}

/** What a part covers in its own frame: its body, and its pins.
 *  @param {PartView} p */
export function body(p) {
  const f = frame(p);
  switch (p.kind) {
    case "in": case "clock": return { x0: -1.5, x1: 0, y0: -0.5, y1: 0.5 };
    case "out": case "led": case "probe": return { x0: 0, x1: 1.5, y0: -0.5, y1: 0.5 };
    case "const": return { x0: -1, x1: 0, y0: -0.5, y1: 0.5 };
    case "tunnel": return { x0: 0, x1: 0.8, y0: -0.4, y1: 0.4 };
    case "ground": case "vdd": return { x0: -0.5, x1: 0.5, y0: -0.9, y1: 0.9 };
    case "text": return { x0: 0, x1: 3, y0: -0.5, y1: 0.5 };
    case "seg7": case "hexdisplay": case "rgbled": return { x0: 0, x1: 3, y0: -0.5, y1: Math.max(f.y1, 2) + 0.5 };
    case "pullup": return { x0: -0.4, x1: 0.4, y0: -1.2, y1: 0 };
    case "pulldown": return { x0: -0.4, x1: 0.4, y0: 0, y1: 1.2 };
    default: return { x0: f.x0, x1: Math.max(f.x1, f.x0 + 1), y0: f.y0 - 0.5, y1: f.y1 + 0.5 };
  }
}

/** The box a part covers on the grid. @param {PartView} p */
export function box(p) {
  if (p.problem) return { x0: p.x - 0.5, y0: p.y - 0.5, x1: p.x + 2.5, y1: p.y + 1.5 };
  const b = body(p);
  const pts = [world(p, b.x0, b.y0), world(p, b.x1, b.y0), world(p, b.x0, b.y1), world(p, b.x1, b.y1)];
  for (const pin of p.pins) pts.push([pin.x, pin.y]);
  const xs = pts.map((q) => q[0]), ys = pts.map((q) => q[1]);
  return { x0: Math.min(...xs), y0: Math.min(...ys), x1: Math.max(...xs), y1: Math.max(...ys) };
}

/** How far a point is from a wire, in grid steps. @param {number[]} w @param {number} x @param {number} y */
export function wireDistance(w, x, y) {
  const [ax, ay, bx, by] = w;
  const len = (bx - ax) ** 2 + (by - ay) ** 2;
  const t = len === 0 ? 0 : Math.max(0, Math.min(1, ((x - ax) * (bx - ax) + (y - ay) * (by - ay)) / len));
  return Math.hypot(x - (ax + t * (bx - ax)), y - (ay + t * (by - ay)));
}

/** The legs of a wire from `a` to `b`: across, then down. @param {[number, number]} a @param {[number, number]} b @returns {[number, number][]} */
export function legs(a, b) {
  if (a[0] === b[0] || a[1] === b[1]) return [a, b];
  return [a, [b[0], a[1]], b];
}

/** Where `.circ` text starts on the grid: its least x and y, for a paste to
 *  land at the cursor. @param {string} text @returns {[number, number] | null} */
export function origin(text) {
  let x = Infinity, y = Infinity;
  for (const m of text.matchAll(/(?:\bat |^wire |\s)(-?\d+),(-?\d+)/gm)) {
    x = Math.min(x, Number(m[1]));
    y = Math.min(y, Number(m[2]));
  }
  return Number.isFinite(x) ? [x, y] : null;
}

/** The parts a palette offers first, in its order. */
const COMMON = ["in", "out", "clock", "const", "not", "and", "or", "xor", "nand", "nor", "mux", "add", "register", "dff", "tunnel", "split"];

/** @param {{root: Document, dispatch: (command: any) => unknown, paste?: () => Promise<string | null>}} host */
export function createCircuit(host) {
  const found = /** @type {HTMLElement | null} */ (host.root.querySelector('.workspace[data-ws="circuit"]'));
  if (!found) return null;
  const ws = found;
  const q = (/** @type {string} */ sel) => /** @type {HTMLElement} */ (ws.querySelector(sel));
  const els = {
    list: q("[data-circuit-list]"),
    home: q("[data-circuit-home]"),
    project: q("[data-circuit-project]"),
    title: q("[data-circuit-title]"),
    shown: q("[data-circuit-shown]"),
    stage: q("[data-circuit-stage]"),
    canvas: /** @type {HTMLCanvasElement} */ (q("[data-circuit-canvas]")),
    empty: q("[data-circuit-empty]"),
  };

  const state = {
    /** @type {Found[]} */
    projects: [],
    /** @type {ProjectView | null} */
    project: null,
    /** Pixels a grid step, and where step (0, 0) is on the canvas. */
    view: { scale: FIT_SCALE, ox: 40, oy: 40 },
    /** The project and circuit the view was fitted to. */
    fitted: "",
    /** A drag under way: panning, moving the selection, drawing a wire, or
     *  selecting a box. `x, y` are where it started on the screen, `from`
     *  on the grid, `to` where it is now on the grid.
     *  @type {null | {kind: "pan" | "move" | "wire" | "box", x: number, y: number, ox: number, oy: number, from: [number, number], to: [number, number], add: boolean, moved: boolean}} */
    drag: null,
    /** Naming a new circuit in the list. */
    adding: false,
    /** The parts and wires selected: ids, and wires as `ax,ay,bx,by`. */
    parts: /** @type {Set<string>} */ (new Set()),
    wires: /** @type {Set<string>} */ (new Set()),
    /** The cursor on the grid. */
    cursor: /** @type {[number, number]} */ ([0, 0]),
    /** A wire begun from the keyboard with `w`: where it starts. @type {[number, number] | null} */
    wiring: null,
    /** The kind `space` places. */
    last: "and",
    /** The circuit running, as the core last said. @type {RunView | null} */
    run: null,
    /** The clock left running: the timer sending its edges. @type {ReturnType<typeof setInterval> | null} */
    live: null,
    /** A click on an input waiting to see whether it is a click or a drag. @type {{id: string, x: number, y: number} | null} */
    press: null,
    /** The panel below: open, and which tab. */
    panel: false,
    tab: /** @type {"tests" | "graph" | "analysis" | "machines"} */ ("tests"),
    /** The machine shown, as the core last read it, and which is chosen. @type {MachineView | null} */
    machine: null,
    machineName: "",
    namingMachine: false,
    /** The last analysis, for this project. @type {Analysis | null} */
    analysis: null,
    /** The table shown in it, and the last results by table, for this
     *  project and circuit. */
    table: "",
    /** @type {Map<string, TestResult>} */
    results: new Map(),
    resultsOf: "",
    /** Naming a new table. */
    naming: false,
  };
  const panelEls = {
    panel: q("[data-circuit-panel]"),
    tools: q("[data-circuit-tools]"),
    tests: q("[data-circuit-tests]"),
    tables: q("[data-circuit-tables]"),
    text: /** @type {HTMLTextAreaElement} */ (q("[data-circuit-table-text]")),
    results: q("[data-circuit-results]"),
    graph: q("[data-circuit-graph]"),
    graphCanvas: /** @type {HTMLCanvasElement} */ (q("[data-circuit-graph-canvas]")),
    graphEmpty: q("[data-circuit-graph-empty]"),
    analysis: q("[data-circuit-analysis]"),
    analyseForm: /** @type {HTMLFormElement} */ (q("[data-circuit-analyse-form]")),
    truth: q("[data-circuit-truth]"),
    forms: q("[data-circuit-forms]"),
    machines: q("[data-circuit-machines]"),
    machineList: q("[data-circuit-machine-list]"),
    machineText: /** @type {HTMLTextAreaElement} */ (q("[data-circuit-machine-text]")),
    diagram: /** @type {HTMLCanvasElement} */ (q("[data-circuit-machine-diagram]")),
    machineTable: q("[data-circuit-machine-table]"),
  };
  const runEls = {
    status: q("[data-circuit-status]"),
    run: q("[data-circuit-run]"),
    running: q("[data-circuit-running]"),
    hold: /** @type {HTMLInputElement} */ (q("[data-circuit-hold]")),
    hz: /** @type {HTMLSelectElement} */ (q("[data-circuit-hz]")),
    live: q("[data-circuit-live]"),
  };
  /** The run, if it is of the circuit on screen. */
  const runOf = () => {
    const r = state.run, p = state.project;
    return r && p && r.generation === p.generation && r.circuit === p.shown ? r : null;
  };
  /** A part's value while running. @param {string} id */
  const valueOf = (id) => runOf()?.values.find(([k]) => k === id)?.[1];

  const wireKey = (/** @type {number[]} */ w) => w.join(",");
  /** Changes to the circuit shown, sent as one step. @param {any[]} ops */
  function send(ops) {
    const p = state.project;
    if (p && ops.length) host.dispatch({ type: "circuitEdit", generation: p.generation, circuit: p.shown, ops });
  }
  const selectedWires = () => (state.project?.wires ?? []).filter((w) => state.wires.has(wireKey(w)));
  const nothingSelected = () => state.parts.size === 0 && state.wires.size === 0;
  /** The part at a grid point, the topmost. @param {number} x @param {number} y */
  function partAt(x, y) {
    const parts = state.project?.parts ?? [];
    for (let i = parts.length - 1; i >= 0; i--) {
      const b = box(parts[i]);
      if (x >= b.x0 - 0.15 && x <= b.x1 + 0.15 && y >= b.y0 - 0.15 && y <= b.y1 + 0.15) return parts[i];
    }
    return null;
  }
  /** The wire at a grid point, if one passes within reach. @param {number} x @param {number} y */
  function wireAt(x, y) {
    const reach = Math.max(0.25, 6 / state.view.scale);
    return (state.project?.wires ?? []).find((w) => wireDistance(w, x, y) <= reach) ?? null;
  }
  /** Whether a wire can start at a grid point: a pin or a wire's end there. @param {number} x @param {number} y */
  function wireEnd(x, y) {
    const p = state.project;
    if (!p) return false;
    return p.parts.some((part) => part.pins.some((pin) => pin.x === x && pin.y === y)) || p.wires.some((w) => (w[0] === x && w[1] === y) || (w[2] === x && w[3] === y));
  }
  /** The parts the keys act on: the selection, or the part at the cursor. */
  function targets() {
    if (state.parts.size) return [...state.parts];
    const at = partAt(state.cursor[0], state.cursor[1]);
    return at ? [at.id] : [];
  }

  /* ── the side list ─────────────────────────────────────────────────── */

  function renderList() {
    const p = state.project;
    // A Digital file is imported when it is opened.
    const projects = state.projects
      .map((f) => f.kind === "dig"
        ? `<button class="xl-row" data-circuit-import="${esc(f.path)}" title="${esc(f.path)} — import it as a .circuit project beside it">
          ${icon("file")}<span class="xl-row-name">${esc(f.name)}</span><span class="xl-kind">Digital</span></button>`
        : `<button class="xl-row${f.path === p?.path ? " on" : ""}" data-circuit-open="${esc(f.path)}" title="${esc(f.path)}">
          ${icon("bolt")}<span class="xl-row-name">${esc(f.name)}</span></button>`)
      .join("");
    const found = els.home.querySelector("[data-circuit-found]");
    if (found) found.innerHTML = projects;
    if (!p) {
      els.list.innerHTML = projects || `<p class="xl-empty">No circuit projects in this folder yet.</p>`;
      return;
    }
    const circuits = p.circuits
      .map((c) => `<button class="xl-row${c === p.shown ? " on" : ""}" data-circuit-show="${esc(c)}" title="${esc(c)}.circ${c === p.top ? " — runs when the project does" : ""}">
        ${icon("node")}<span class="xl-row-name">${esc(c)}</span>${c === p.top ? `<span class="xl-kind">top</span>` : `<span class="xl-kind ci-make-top" data-circuit-top="${esc(c)}" title="Make it the top circuit">top?</span>`}</button>`)
      .join("");
    els.list.innerHTML = `<div class="ci-section">Circuits</div>${circuits}` +
      (state.adding
        ? `<form class="ci-add" data-circuit-add-form><input type="text" placeholder="Name" aria-label="New circuit's name" spellcheck="false"></form>`
        : `<button class="xl-row ci-add-row" data-circuit-add>${icon("plus")}<span class="xl-row-name">Add a circuit</span></button>`) +
      `<div class="ci-section">Parts</div><div class="ci-palette">${palette(p)}</div>` +
      `<div class="ci-section">Projects</div>${projects}`;
    if (state.adding) {
      const input = /** @type {HTMLInputElement | null} */ (els.list.querySelector("[data-circuit-add-form] input"));
      input?.focus();
    }
  }

  /** The parts to place: the common ones, then subcircuits, then the rest.
   *  @param {ProjectView} p */
  function palette(p) {
    const offered = new Set(p.palette);
    const subs = p.palette.filter((k) => k.startsWith("sub:"));
    const rest = p.palette.filter((k) => !k.startsWith("sub:") && !COMMON.includes(k));
    return [...COMMON.filter((k) => offered.has(k)), ...subs, ...rest]
      .map((k) => `<button class="ci-kind${k === state.last ? " on" : ""}" data-circuit-place="${esc(k)}" title="Place ${esc(k)} at the cursor — space places the last one again">${esc(k.startsWith("sub:") ? k.slice(4) : k)}</button>`)
      .join("");
  }

  /* ── the project ───────────────────────────────────────────────────── */

  function renderProject() {
    const p = state.project;
    els.home.hidden = !!p;
    els.project.hidden = !p;
    renderList();
    if (!p) return;
    els.title.textContent = p.name;
    els.title.title = p.path;
    els.shown.textContent = p.shown === p.top ? `${p.shown}` : `${p.shown}  ·  top is ${p.top}`;
    q("[data-circuit-dirty]").hidden = !p.dirty;
    els.empty.hidden = p.parts.length > 0 || p.wires.length > 0;
    renderRun();
    renderPanel();
    // What was selected and is still there stays so; what an edit made is
    // selected instead.
    const ids = new Set(p.parts.map((part) => part.id));
    const wires = new Set(p.wires.map(wireKey));
    if (p.made.length) {
      state.parts = new Set(p.made.filter((id) => ids.has(id)));
      state.wires = new Set();
    } else {
      state.parts = new Set([...state.parts].filter((id) => ids.has(id)));
      state.wires = new Set([...state.wires].filter((w) => wires.has(w)));
    }
    if (`${p.generation}/${p.shown}` !== state.fitted) requestAnimationFrame(fit);
    else redraw();
  }

  /* ── tests and the graph ───────────────────────────────────────────── */

  function renderPanel() {
    const p = state.project;
    panelEls.panel.hidden = !state.panel || !p;
    if (!p || !state.panel) return;
    const key = `${p.generation}/${p.shown}`;
    if (state.resultsOf !== key) {
      state.resultsOf = key;
      state.results.clear();
      state.table = "";
    }
    for (const b of panelEls.panel.querySelectorAll("[data-circuit-tab]")) {
      b.setAttribute("aria-selected", String(/** @type {HTMLElement} */ (b).dataset.circuitTab === state.tab));
    }
    panelEls.tests.hidden = state.tab !== "tests";
    panelEls.graph.hidden = state.tab !== "graph";
    panelEls.analysis.hidden = state.tab !== "analysis";
    panelEls.machines.hidden = state.tab !== "machines";
    if (state.tab === "machines") {
      renderMachines();
      return;
    }
    if (state.tab === "analysis") {
      panelEls.tools.innerHTML = "";
      renderAnalysis();
      return;
    }
    if (state.tab === "tests") {
      panelEls.tools.innerHTML = `<button class="ci-tool" data-circuit-run-all${p.tables.length ? "" : " disabled"}>Run all</button>`;
      if (!state.table || !p.tables.some(([l]) => l === state.table)) {
        if (state.table !== "" || !state.naming) state.table = p.tables[0]?.[0] ?? "";
      }
      const badge = (/** @type {string} */ label) => {
        const r = state.results.get(label);
        if (!r) return "";
        if (r.error) return `<span class="ci-badge bad">!</span>`;
        return r.failures.length ? `<span class="ci-badge bad">✗</span>` : `<span class="ci-badge ok">✓</span>`;
      };
      panelEls.tables.innerHTML = p.tables
        .map(([label]) => `<button class="xl-row${label === state.table ? " on" : ""}" data-circuit-table="${esc(label)}">${badge(label)}<span class="xl-row-name">${esc(label)}</span></button>`)
        .join("") +
        (state.naming
          ? `<form class="ci-add" data-circuit-table-form><input type="text" placeholder="Name" aria-label="New table's name" spellcheck="false"></form>`
          : `<button class="xl-row" data-circuit-table-new>${icon("plus")}<span class="xl-row-name">New table</span></button>`);
      if (state.naming) /** @type {HTMLInputElement | null} */ (panelEls.tables.querySelector("input"))?.focus();
      const text = p.tables.find(([l]) => l === state.table)?.[1];
      if (document.activeElement !== panelEls.text) panelEls.text.value = text ?? "";
      panelEls.text.disabled = text === undefined;
      const r = state.results.get(state.table);
      const head = text === undefined ? "" : `<div class="ci-results-head">
        <button class="ci-tool" data-circuit-table-run title="Save and run it  ⌘↩">Run</button>
        <button class="ci-tool" data-circuit-table-save title="Keep what is written, a held edit">Keep</button>
        <button class="ci-tool" data-circuit-table-delete title="Take the table away (undo brings it back)">Remove</button>
        <span class="ci-result-sum${r && (r.error || r.failures.length) ? " bad" : ""}">${r ? (r.error ? esc(r.error) : r.failures.length ? `${r.failures.length === 10 ? "10 or more" : r.failures.length} of ${r.rows} rows failed` : `all ${r.rows} rows pass`) : ""}</span></div>`;
      panelEls.results.innerHTML = head + (r ? r.failures.map((f) => `<div class="ci-failure">${esc(f)}</div>`).join("") : "");
    } else {
      panelEls.tools.innerHTML = `<button class="ci-tool" data-circuit-trace-data${runOf() ? "" : " disabled"} title="The recording as a dataframe in Data">To Data</button>
        <button class="ci-tool" data-circuit-trace-clear${runOf() ? "" : " disabled"}>Clear</button>`;
      drawGraph();
    }
  }

  /** The truth table, the least forms of each output, and a Karnaugh map
   *  of each for six variables or fewer, its groups outlined. */
  function renderAnalysis() {
    const a = state.analysis;
    if (!a) {
      panelEls.truth.innerHTML = `<p class="ci-hint">Analyse this circuit — a circuit without flip-flops, of 16 input bits or fewer — or expressions, to see its truth table, its least sums of products and products of sums, and a Karnaugh map.</p>`;
      panelEls.forms.innerHTML = "";
      return;
    }
    if (a.error) {
      panelEls.truth.innerHTML = `<p class="ci-failure">${esc(a.error)}</p>`;
      panelEls.forms.innerHTML = "";
      return;
    }
    const n = a.inputs.length, total = 1 << n, shown = Math.min(total, MOST_TRUTH_ROWS);
    let rows = "";
    for (let r = 0; r < shown; r++) {
      const ins = a.inputs.map((_, i) => `<td>${(r >> (n - 1 - i)) & 1}</td>`).join("");
      const outs = a.columns.map((c) => `<td class="v${c[r]}">${c[r]}</td>`).join("");
      rows += `<tr>${ins}<td class="sep"></td>${outs}</tr>`;
    }
    panelEls.truth.innerHTML = `<table class="ci-truth-table"><thead><tr>${a.inputs.map((v) => `<th>${esc(v)}</th>`).join("")}<th class="sep"></th>${a.outputs.map((v) => `<th>${esc(v)}</th>`).join("")}</tr></thead><tbody>${rows}</tbody></table>` +
      (shown < total ? `<p class="ci-hint">The first ${shown} of ${total.toLocaleString()} rows.</p>` : "");
    panelEls.forms.innerHTML = a.outputs.map((o, k) => `<section class="ci-form">
        <div class="ci-form-line"><b>${esc(o)}</b> = ${esc(a.sop[k])}</div>
        <div class="ci-form-line muted">${esc(o)} = ${esc(a.pos[k])}</div>
        ${n >= 2 && n <= 6 ? `<canvas class="ci-kmap" data-circuit-kmap="${k}"></canvas>` : ""}
      </section>`).join("") + (a.exact ? "" : `<p class="ci-hint">Some covers were chosen greedily: they may not be the least.</p>`);
    for (const c of panelEls.forms.querySelectorAll("[data-circuit-kmap]")) drawKmap(/** @type {HTMLCanvasElement} */ (c), a, Number(/** @type {HTMLElement} */ (c).dataset.circuitKmap));
  }

  /** @param {HTMLCanvasElement} c @param {Analysis} a @param {number} k */
  function drawKmap(c, a, k) {
    const n = a.inputs.length, m = kmap(n), cell = 30, left = 44, top = 30;
    const w = left + m.cols.length * cell + 8, h = top + m.rows.length * cell + 8;
    const dpr = window.devicePixelRatio || 1;
    c.width = w * dpr; c.height = h * dpr;
    c.style.width = `${w}px`; c.style.height = `${h}px`;
    const g = /** @type {CanvasRenderingContext2D} */ (c.getContext("2d"));
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    const col = colours();
    g.font = `11px ui-monospace, "SF Mono", Menlo, monospace`;
    g.textAlign = "center";
    g.textBaseline = "middle";
    g.fillStyle = col.muted;
    const bits = (/** @type {number} */ v, /** @type {number} */ len) => v.toString(2).padStart(len, "0");
    g.fillText(`${a.inputs.slice(0, m.rv).join("")}\\${a.inputs.slice(m.rv).join("")}`, left / 2, top / 2);
    m.cols.forEach((v, i) => g.fillText(m.cv ? bits(v, m.cv) : "", left + i * cell + cell / 2, top / 2));
    m.rows.forEach((v, i) => g.fillText(m.rv ? bits(v, m.rv) : "", left / 2, top + i * cell + cell / 2));
    for (let r = 0; r < m.rows.length; r++) for (let q = 0; q < m.cols.length; q++) {
      const v = a.columns[k][m.row(r, q)];
      g.strokeStyle = col.line;
      g.strokeRect(left + q * cell, top + r * cell, cell, cell);
      g.fillStyle = v === "1" ? col.text : col.muted;
      g.fillText(v === "x" ? "x" : v, left + q * cell + cell / 2, top + r * cell + cell / 2);
    }
    // Each group: the rows and columns its cells are in, in runs.
    const hues = ["#f97316", "#22c55e", "#3b82f6", "#e879f9", "#eab308", "#14b8a6", "#f43f5e", "#a78bfa"];
    a.groups[k].forEach(([value, care], gi) => {
      const covers = (/** @type {number} */ row) => (row & care) === (value & care);
      const rs = m.rows.map((_, r) => r).filter((r) => m.cols.some((_, q) => covers(m.row(r, q))));
      const cs = m.cols.map((_, q) => q).filter((q) => m.rows.some((_, r) => covers(m.row(r, q))));
      g.strokeStyle = hues[gi % hues.length];
      g.lineWidth = 2;
      const inset = 3 + (gi % 4) * 2;
      for (const [r0, r1] of runs(rs)) for (const [c0, c1] of runs(cs)) {
        g.beginPath();
        g.roundRect(left + c0 * cell + inset, top + r0 * cell + inset, (c1 - c0 + 1) * cell - 2 * inset, (r1 - r0 + 1) * cell - 2 * inset, 6);
        g.stroke();
      }
      g.lineWidth = 1;
    });
  }

  /* The machines: a list, the text of the one chosen, and its diagram and
     state table as the core reads what is written, a moment after typing. */

  function renderMachines() {
    const p = state.project;
    if (!p) return;
    if (!state.machineName || !p.machines.some(([n]) => n === state.machineName)) {
      if (!state.namingMachine) state.machineName = p.machines[0]?.[0] ?? "";
    }
    panelEls.tools.innerHTML = `<button class="ci-tool" data-circuit-machine-keep${state.machineName ? "" : " disabled"}>Keep</button>
      <select class="ci-hz" data-circuit-machine-ff aria-label="Flip-flops"><option value="d">D flip-flops</option><option value="jk">JK flip-flops</option></select>
      <input class="ci-build-name ci-tool-input" data-circuit-machine-circuit placeholder="circuit name" aria-label="Name of the circuit to build" spellcheck="false">
      <button class="ci-tool" data-circuit-machine-build${state.machineName ? "" : " disabled"}>Build</button>`;
    panelEls.machineList.innerHTML = p.machines.map(([n]) => `<button class="xl-row${n === state.machineName ? " on" : ""}" data-circuit-machine="${esc(n)}"><span class="xl-row-name">${esc(n)}</span></button>`).join("") +
      (state.namingMachine
        ? `<form class="ci-add" data-circuit-machine-form><input type="text" placeholder="Name" aria-label="New machine's name" spellcheck="false"></form>`
        : `<button class="xl-row" data-circuit-machine-new>${icon("plus")}<span class="xl-row-name">New machine</span></button>`);
    if (state.namingMachine) /** @type {HTMLInputElement | null} */ (panelEls.machineList.querySelector("input"))?.focus();
    const text = p.machines.find(([n]) => n === state.machineName)?.[1];
    if (document.activeElement !== panelEls.machineText) panelEls.machineText.value = text ?? "";
    panelEls.machineText.disabled = text === undefined;
    if (text !== undefined && state.machine?.name !== state.machineName && document.activeElement !== panelEls.machineText) {
      runCommand("circuitMachineShow", { name: state.machineName, text: null });
    }
    drawMachine();
  }

  let machineTimer = /** @type {ReturnType<typeof setTimeout> | null} */ (null);
  panelEls.machineText.addEventListener("input", () => {
    if (machineTimer) clearTimeout(machineTimer);
    machineTimer = setTimeout(() => runCommand("circuitMachineShow", { name: null, text: panelEls.machineText.value }), 250);
  });
  panelEls.machineText.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Escape") els.stage.focus({ preventScroll: true });
    if (e.metaKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      keepMachine();
      host.dispatch({ type: "circuitSave" });
    }
  });
  function keepMachine() {
    const p = state.project;
    if (!p || !state.machineName) return;
    const now = p.machines.find(([n]) => n === state.machineName)?.[1];
    const text = panelEls.machineText.value;
    if (now !== text && now !== `${text}\n`) host.dispatch({ type: "circuitMachineSet", generation: p.generation, name: state.machineName, text });
  }

  /** The states on a ring, the first marked as where it starts; each
   *  transition an arrow — a loop for one to itself — labelled with its
   *  condition and outputs; and the state table beside it. */
  function drawMachine() {
    const m = state.machine;
    const c = panelEls.diagram;
    const box = c.parentElement?.getBoundingClientRect();
    const w = Math.max(200, Math.round((box?.width ?? 400) * 0.55)), h = Math.max(160, Math.round(box?.height ?? 200) - 8);
    const dpr = window.devicePixelRatio || 1;
    c.width = w * dpr; c.height = h * dpr;
    c.style.width = `${w}px`; c.style.height = `${h}px`;
    const g = /** @type {CanvasRenderingContext2D} */ (c.getContext("2d"));
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, h);
    const col = colours();
    if (!m) {
      panelEls.machineTable.innerHTML = `<p class="ci-hint">A machine is written as its inputs, its outputs, its states (with a Moore output, <code>state S1 Z=1</code>) and its transitions (<code>S0 -> S1 when X</code>, a Mealy output after <code>/ Z=1</code>). It starts in its first state; at a clock edge it takes the first transition whose condition holds, or stays.</p>`;
      return;
    }
    if (m.error) {
      panelEls.machineTable.innerHTML = `<p class="ci-failure">${esc(m.error)}</p>`;
      return;
    }
    const n = m.states.length, cx = w / 2, cy = h / 2, ring = Math.min(w, h) / 2 - 34, r = 20;
    const at = m.states.map((_, i) => n === 1 ? [cx, cy] : [cx + ring * Math.cos((2 * Math.PI * i) / n - Math.PI / 2), cy + ring * Math.sin((2 * Math.PI * i) / n - Math.PI / 2)]);
    g.font = `11px ui-monospace, "SF Mono", Menlo, monospace`;
    g.textAlign = "center";
    g.textBaseline = "middle";
    g.lineWidth = 1.3;
    const arrowHead = (/** @type {number} */ x, /** @type {number} */ y, /** @type {number} */ angle) => {
      g.beginPath();
      g.moveTo(x, y);
      g.lineTo(x - 8 * Math.cos(angle - 0.4), y - 8 * Math.sin(angle - 0.4));
      g.lineTo(x - 8 * Math.cos(angle + 0.4), y - 8 * Math.sin(angle + 0.4));
      g.closePath();
      g.fill();
    };
    m.transitions.forEach(([from, to, when, outs]) => {
      const label = [when, outs && `/ ${outs}`].filter(Boolean).join(" ");
      g.strokeStyle = col.muted;
      g.fillStyle = col.muted;
      const [x0, y0] = at[from], [x1, y1] = at[to];
      if (from === to) {
        // A loop, outward from the ring's centre.
        const a = Math.atan2(y0 - cy, x0 - cx) || -Math.PI / 2;
        const lx = x0 + Math.cos(a) * (r + 10), ly = y0 + Math.sin(a) * (r + 10);
        g.beginPath();
        g.arc(lx, ly, 11, 0, Math.PI * 2);
        g.stroke();
        g.fillStyle = col.text;
        g.fillText(label, x0 + Math.cos(a) * (r + 34), y0 + Math.sin(a) * (r + 30));
        return;
      }
      // Bowed outward from the ring's middle, so labels stay apart; where
      // both A→B and B→A are drawn, the second bows the other way.
      const mx = (x0 + x1) / 2, my = (y0 + y1) / 2, dx = x1 - x0, dy = y1 - y0, len = Math.hypot(dx, dy) || 1;
      const bow = 24;
      const out = (/** @type {number} */ sign) => [mx - sign * (dy / len) * bow, my + sign * (dx / len) * bow];
      const [ax, ay] = out(1), [bx, by] = out(-1);
      let outward = Math.hypot(ax - cx, ay - cy) >= Math.hypot(bx - cx, by - cy) ? 1 : -1;
      if (from > to && m.transitions.some(([f, t2]) => f === to && t2 === from)) outward = -outward;
      const [qx, qy] = out(outward);
      const a0 = Math.atan2(qy - y0, qx - x0), a1 = Math.atan2(y1 - qy, x1 - qx);
      const sx = x0 + Math.cos(a0) * r, sy = y0 + Math.sin(a0) * r, ex = x1 - Math.cos(a1) * r, ey = y1 - Math.sin(a1) * r;
      g.beginPath();
      g.moveTo(sx, sy);
      g.quadraticCurveTo(qx, qy, ex, ey);
      g.stroke();
      arrowHead(ex, ey, a1);
      g.fillStyle = col.text;
      g.fillText(label, qx - outward * (dy / len) * 8, qy + outward * (dx / len) * 8);
    });
    m.states.forEach((name, i) => {
      const [x, y] = at[i];
      g.fillStyle = col.bg;
      g.strokeStyle = i === 0 ? col.accent : col.text;
      g.beginPath();
      g.arc(x, y, r, 0, Math.PI * 2);
      g.fill();
      g.stroke();
      g.fillStyle = col.text;
      g.fillText(name, x, m.stateOutputs[i] ? y - 5 : y);
      if (m.stateOutputs[i]) {
        g.fillStyle = col.high;
        g.fillText(m.stateOutputs[i], x, y + 7);
      }
    });
    // Where it starts.
    const [sx, sy] = at[0];
    g.strokeStyle = col.accent;
    g.fillStyle = col.accent;
    g.beginPath();
    g.moveTo(sx - r - 22, sy);
    g.lineTo(sx - r - 2, sy);
    g.stroke();
    arrowHead(sx - r - 1, sy, 0);
    const code = (/** @type {number} */ s) => s.toString(2).padStart(m.bits, "0");
    panelEls.machineTable.innerHTML = `<table class="ci-truth-table"><thead><tr><th>state</th><th>${esc(m.inputs.join(" ") || "—")}</th><th class="sep"></th><th>next</th><th>${esc(m.outputs.join(" ") || "—")}</th></tr></thead><tbody>${
      m.table.map(([s, ins, next, outs]) => `<tr><td>${esc(m.states[s])} <span class="muted">${code(s)}</span></td><td>${ins || "—"}</td><td class="sep"></td><td>${esc(m.states[next])} <span class="muted">${code(next)}</span></td><td>${outs || "—"}</td></tr>`).join("")
    }</tbody></table><p class="ci-hint">${m.mealy ? "Mealy: outputs on the transitions." : "Moore: outputs on the states."} ${m.states.length} states in ${m.bits} bit${m.bits === 1 ? "" : "s"}.</p>`;
  }

  /** The written table kept, as a held edit. */
  function keepTable() {
    const p = state.project;
    if (!p || !state.table) return;
    const now = p.tables.find(([l]) => l === state.table)?.[1];
    const text = panelEls.text.value;
    if (now !== text && now !== `${text}\n`) host.dispatch({ type: "circuitTestSet", generation: p.generation, circuit: p.shown, label: state.table, text });
  }
  /** @param {string | null} label */
  function runTables(label) {
    const p = state.project;
    if (!p) return;
    keepTable();
    host.dispatch({ type: "circuitTestRun", generation: p.generation, circuit: p.shown, label });
  }

  /** The recorded signals, a row each, the newest sample at the right. */
  function drawGraph() {
    const run = runOf();
    const trace = run?.trace;
    panelEls.graphEmpty.hidden = !!trace && trace.samples.length > 0;
    const c = panelEls.graphCanvas;
    const r = panelEls.graph.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    c.width = Math.max(1, Math.round(r.width * dpr));
    c.height = Math.max(1, Math.round(r.height * dpr));
    c.style.width = `${r.width}px`;
    c.style.height = `${r.height}px`;
    const g = /** @type {CanvasRenderingContext2D} */ (c.getContext("2d"));
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, r.width, r.height);
    if (!trace || !trace.samples.length) return;
    const col = colours();
    const fit = Math.max(1, Math.floor((r.width - NAMES_W - 8) / SAMPLE_W));
    const samples = trace.samples.slice(-fit);
    const first = trace.total - samples.length;
    g.font = `11px ui-monospace, "SF Mono", Menlo, monospace`;
    g.textBaseline = "middle";
    trace.signals.forEach(([name, bits], row) => {
      const y0 = 8 + row * SIGNAL_H, hi = y0 + 4, lo = y0 + SIGNAL_H - 6, mid = (hi + lo) / 2;
      g.fillStyle = col.muted;
      g.textAlign = "left";
      g.fillText(name, 4, mid);
      samples.forEach((sample, i) => {
        const v = sample[row];
        const x0 = NAMES_W + i * SAMPLE_W, x1 = x0 + SAMPLE_W;
        const prev = i > 0 ? samples[i - 1][row] : v;
        g.lineWidth = 1.5;
        if (v === null) {
          g.strokeStyle = col.z;
          g.beginPath(); g.moveTo(x0, mid); g.lineTo(x1, mid); g.stroke();
        } else if (bits === 1) {
          const y = v === "1" ? hi : lo;
          g.strokeStyle = v === "1" ? col.high : col.low;
          g.beginPath();
          if (prev !== v && prev !== null) { g.moveTo(x0, prev === "1" ? hi : lo); g.lineTo(x0, y); } else g.moveTo(x0, y);
          g.lineTo(x1, y);
          g.stroke();
        } else {
          // A bus: a band, crossed where its value changes, its value in it.
          g.strokeStyle = col.bus;
          g.beginPath();
          const cross = prev !== v ? 3 : 0;
          g.moveTo(x0, mid); g.lineTo(x0 + cross, hi); g.lineTo(x1, hi);
          g.moveTo(x0, mid); g.lineTo(x0 + cross, lo); g.lineTo(x1, lo);
          g.stroke();
          if (prev !== v || i === 0) {
            g.fillStyle = col.text;
            g.textAlign = "left";
            g.fillText(v.toUpperCase(), x0 + 4, mid);
          }
        }
      });
    });
    // Sample numbers along the bottom, every tenth.
    g.fillStyle = col.muted;
    g.textAlign = "center";
    const yb = 8 + trace.signals.length * SIGNAL_H + 6;
    samples.forEach((_, i) => {
      const n = first + i;
      if (n % 10 === 0) g.fillText(String(n), NAMES_W + i * SAMPLE_W, yb);
    });
  }

  /* ── running ───────────────────────────────────────────────────────── */

  function renderRun() {
    const r = state.run, p = state.project;
    const on = !!r && !!p && r.generation === p.generation;
    runEls.run.innerHTML = icon(on ? "stop" : "play");
    runEls.run.title = on ? "Stop  ⌘R" : "Run  ⌘R";
    runEls.run.setAttribute("aria-label", on ? "Stop" : "Run");
    runEls.running.hidden = !on;
    runEls.live.classList.toggle("on", !!state.live);
    let status = "";
    if (on && r) {
      const q = r.quick;
      if (r.error) status = r.error;
      else if (q?.running) status = `quick run · ${q.cycles.toLocaleString()} cycles · ${hertz(q.hz)}`;
      else if (q) status = `${q.cycles.toLocaleString()} cycles at ${hertz(q.hz)} — ${q.ended ?? "ended"}`;
      else if (r.waiting) status = `${r.waiting} waiting — . steps`;
      else status = "running";
    }
    runEls.status.textContent = status;
    if (state.panel && state.tab === "graph") renderPanel();
    runEls.status.classList.toggle("bad", !!r?.error);
    if (!on && state.live) setLive(false);
  }

  /** @param {number} hz */
  const hertz = (hz) => (hz >= 1e6 ? `${(hz / 1e6).toFixed(2)} MHz` : hz >= 1e3 ? `${(hz / 1e3).toFixed(1)} kHz` : `${hz.toFixed(0)} Hz`);

  /** Commands about the run, for the project open. @param {string} type @param {Record<string, unknown>} more */
  function runCommand(type, more = {}) {
    const p = state.project;
    if (p) host.dispatch({ type, generation: p.generation, ...more });
  }
  const running = () => !!runOf() && !runOf()?.quick?.running && !runOf()?.error;
  function toggleRun() {
    if (state.run && state.run.generation === state.project?.generation) {
      setLive(false);
      host.dispatch({ type: "circuitStop" });
    } else {
      runCommand("circuitStart");
    }
  }
  const clockEdge = () => runCommand("circuitClock", { hold: runEls.hold.checked });
  /** The clock left running, an edge each half period, or stopped. @param {boolean} on */
  function setLive(on) {
    if (state.live) clearInterval(state.live);
    state.live = null;
    if (on && running()) {
      const hz = Number(runEls.hz.value) || 2;
      state.live = setInterval(() => (running() ? clockEdge() : setLive(false)), 500 / hz);
    }
    runEls.live.classList.toggle("on", !!state.live);
  }

  /** An input clicked while running: a bit flips, a wider one is asked for.
   *  @param {PartView} part */
  function setInput(part) {
    const v = valueOf(part.id);
    const bits = v?.bits ?? 1;
    if (bits === 1) {
      runCommand("circuitSet", { id: part.id, value: v && !v.z && v.hex === "1" ? "0" : "1", hold: runEls.hold.checked });
      return;
    }
    popup(`<label class="ci-pop-row"><span>${esc(part.id)} · ${bits} bits</span><input name="value" value="0x${esc(v?.hex ?? "0")}" aria-label="Value" spellcheck="false"></label>`,
      [part.x, part.y], (form) => {
        const value = /** @type {HTMLInputElement} */ (form.elements.namedItem("value")).value.trim();
        if (value) runCommand("circuitSet", { id: part.id, value, hold: runEls.hold.checked });
      });
  }

  /** The circuit fitted to the stage, no closer than a comfortable zoom. */
  function fit() {
    const p = state.project;
    const r = els.stage.getBoundingClientRect();
    // Not laid out yet: fitted when the stage has its size.
    if (!p || r.width === 0 || r.height === 0) return;
    state.fitted = `${p.generation}/${p.shown}`;
    const e = extent(p);
    if (!e) {
      state.view = { scale: FIT_SCALE, ox: 40, oy: 40 };
    } else {
      const scale = Math.max(MIN_SCALE, Math.min(FIT_SCALE * 1.5, r.width / (e.x1 - e.x0), r.height / (e.y1 - e.y0)));
      state.view = { scale, ox: (r.width - (e.x1 - e.x0) * scale) / 2 - e.x0 * scale, oy: (r.height - (e.y1 - e.y0) * scale) / 2 - e.y0 * scale };
    }
    redraw();
  }

  let frameAsked = false;
  function redraw() {
    if (frameAsked) return;
    frameAsked = true;
    requestAnimationFrame(() => {
      frameAsked = false;
      draw();
    });
  }

  /** The theme's colours, read when drawn so a theme change shows. */
  function colours() {
    const css = getComputedStyle(ws);
    const get = (/** @type {string} */ n, /** @type {string} */ d) => css.getPropertyValue(n).trim() || d;
    return { text: get("--text", "#ddd"), muted: get("--muted", "#888"), accent: get("--accent", "#6af"), bg: get("--bg", "#111"), danger: get("--danger", "#e55"), line: get("--xl-line", "#333"),
      high: get("--ci-high", "#4ade80"), low: get("--ci-low", "#166534"), z: get("--ci-z", "#60a5fa"), bus: get("--ci-bus", "#a3e635") };
  }

  function draw() {
    const c = els.canvas;
    const r = els.stage.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const w = Math.max(1, Math.round(r.width * dpr)), h = Math.max(1, Math.round(r.height * dpr));
    if (c.width !== w || c.height !== h) {
      c.width = w;
      c.height = h;
      c.style.width = `${r.width}px`;
      c.style.height = `${r.height}px`;
    }
    const g = /** @type {CanvasRenderingContext2D} */ (c.getContext("2d"));
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, r.width, r.height);
    const p = state.project;
    if (!p) return;
    const col = colours();
    const { scale: s, ox, oy } = state.view;

    // The grid: a dot a step, when the steps are far enough apart to see.
    if (s >= 8) {
      g.fillStyle = col.line;
      const x0 = Math.floor(-ox / s), x1 = Math.ceil((r.width - ox) / s);
      const y0 = Math.floor(-oy / s), y1 = Math.ceil((r.height - oy) / s);
      const d = Math.max(1, s / 14);
      for (let x = x0; x <= x1; x++) for (let y = y0; y <= y1; y++) g.fillRect(ox + x * s - d / 2, oy + y * s - d / 2, d, d);
    }

    g.save();
    g.translate(ox, oy);
    g.scale(s, s);
    g.lineCap = "round";
    g.lineJoin = "round";
    // Wires, and a dot where three or more ends meet.
    const d = state.drag;
    const shift = d?.kind === "move" && d.moved ? [d.to[0] - d.from[0], d.to[1] - d.from[1]] : [0, 0];
    g.lineWidth = Math.max(1.5 / s, 0.09);
    /** @type {Map<string, number>} */
    const ends = new Map();
    const run = runOf();
    /** A wire's colour: selected, then its value when running. @param {number} i */
    const wireColour = (i) => {
      if (state.wires.has(wireKey(p.wires[i]))) return col.accent;
      const v = run?.wires[i];
      if (!v) return col.muted;
      if (v.z) return col.z;
      if (v.bits > 1) return col.bus;
      return v.hex === "1" ? col.high : col.low;
    };
    const byColour = /** @type {Map<string, number[]>} */ (new Map());
    p.wires.forEach((w, i) => {
      const c = wireColour(i);
      byColour.set(c, [...(byColour.get(c) ?? []), i]);
      if (!state.wires.has(wireKey(w)) || !d?.moved) for (const k of [`${w[0]},${w[1]}`, `${w[2]},${w[3]}`]) ends.set(k, (ends.get(k) ?? 0) + 1);
    });
    for (const [colour, list] of byColour) {
      g.strokeStyle = colour;
      g.lineWidth = Math.max(1.5 / s, 0.09) * (run && colour !== col.muted ? 1.6 : 1);
      g.beginPath();
      for (const i of list) {
        const w = p.wires[i];
        const [dx, dy] = state.wires.has(wireKey(w)) ? shift : [0, 0];
        g.moveTo(w[0] + dx, w[1] + dy);
        g.lineTo(w[2] + dx, w[3] + dy);
      }
      g.stroke();
    }
    g.lineWidth = Math.max(1.5 / s, 0.09);
    // A bus says its value, at the middle of each wire long enough to hold it.
    if (run) {
      p.wires.forEach((w, i) => {
        const v = run.wires[i];
        if (!v || v.bits === 1 || Math.abs(w[2] - w[0]) + Math.abs(w[3] - w[1]) < 2) return;
        text(g, shownValue(v), (w[0] + w[2]) / 2, (w[1] + w[3]) / 2 - 0.4, v.z ? col.z : col.bus, 0.5, "center");
      });
    }
    for (const part of p.parts) for (const pin of part.pins) {
      const k = `${pin.x},${pin.y}`;
      if (ends.has(k)) ends.set(k, /** @type {number} */ (ends.get(k)) + 1);
    }
    g.fillStyle = col.muted;
    for (const [k, n] of ends) {
      if (n < 3) continue;
      const [x, y] = k.split(",").map(Number);
      g.beginPath();
      g.arc(x, y, 0.16, 0, Math.PI * 2);
      g.fill();
    }

    for (const part of p.parts) {
      const on = state.parts.has(part.id);
      if (on && (shift[0] || shift[1])) {
        g.save();
        g.translate(shift[0], shift[1]);
        drawPart(g, part, col, s, true);
        g.restore();
      } else {
        drawPart(g, part, col, s, on);
      }
    }

    // While running: inputs and displays show their values; a high LED lights.
    if (run) {
      for (const part of p.parts) {
        const v = valueOf(part.id);
        if (!v) continue;
        const b = body(part);
        const [x, y] = world(part, (b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2);
        const high = v.bits === 1 && v.hex === "1" && !v.z;
        if (DISPLAYS.has(part.kind)) {
          face(g, part, v, col);
        } else if (part.kind === "led") {
          g.fillStyle = high ? col.high : col.bg;
          g.beginPath();
          g.arc(x, y, 0.42, 0, Math.PI * 2);
          g.fill();
        } else {
          if (high) {
            g.fillStyle = col.high;
            g.globalAlpha = 0.3;
            g.fillRect(x - 0.5, y - 0.45, 1, 0.9);
            g.globalAlpha = 1;
          }
          text(g, shownValue(v), x, y, v.z ? col.z : col.text, 0.6, "center");
        }
      }
    }

    // A wire being drawn, by the mouse or from the keyboard.
    const from = d?.kind === "wire" ? d.from : state.wiring;
    const to = d?.kind === "wire" ? d.to : state.cursor;
    if (from && (from[0] !== to[0] || from[1] !== to[1])) {
      g.save();
      g.strokeStyle = col.accent;
      g.setLineDash([0.3, 0.2]);
      g.beginPath();
      const pts = legs(from, to);
      g.moveTo(pts[0][0], pts[0][1]);
      for (const q of pts.slice(1)) g.lineTo(q[0], q[1]);
      g.stroke();
      g.restore();
    }
    // A box being dragged out to select what is in it.
    if (d?.kind === "box" && d.moved) {
      g.save();
      g.strokeStyle = col.accent;
      g.fillStyle = col.accent;
      g.globalAlpha = 0.12;
      const x = Math.min(d.from[0], d.to[0]), y = Math.min(d.from[1], d.to[1]);
      const w = Math.abs(d.to[0] - d.from[0]), h = Math.abs(d.to[1] - d.from[1]);
      g.fillRect(x, y, w, h);
      g.globalAlpha = 1;
      g.setLineDash([0.2, 0.15]);
      g.strokeRect(x, y, w, h);
      g.restore();
    }
    // The cursor: a cross on its grid point.
    const [cx, cy] = state.cursor;
    g.strokeStyle = col.accent;
    g.lineWidth = Math.max(1.5 / s, 0.07);
    g.beginPath();
    g.moveTo(cx - 0.35, cy); g.lineTo(cx + 0.35, cy);
    g.moveTo(cx, cy - 0.35); g.lineTo(cx, cy + 0.35);
    g.stroke();
    g.restore();
  }

  /** @param {CanvasRenderingContext2D} g @param {PartView} p @param {ReturnType<typeof colours>} colours_ @param {number} s @param {boolean} on selected: drawn in the accent */
  function drawPart(g, p, colours_, s, on) {
    const col = on ? { ...colours_, text: colours_.accent } : colours_;
    const line = Math.max(1.5 / s, 0.08) * (on ? 1.5 : 1);
    if (p.problem) {
      g.save();
      g.strokeStyle = col.danger;
      g.setLineDash([0.2, 0.15]);
      g.lineWidth = line;
      g.strokeRect(p.x - 0.5, p.y - 0.5, 3, 2);
      g.restore();
      text(g, p.kind, p.x + 1, p.y + 0.5, col.danger, 0.55, "center");
      return;
    }
    g.save();
    g.translate(p.x, p.y);
    g.rotate((-Math.PI / 2) * (p.rot % 4));
    if (p.mirror) g.scale(1, -1);
    g.lineWidth = line;
    g.strokeStyle = col.text;
    g.fillStyle = col.bg;
    const f = frame(p);
    const label = setting(p, "label");
    const kind = p.kind;
    if (GATES.has(kind)) {
      gate(g, kind, f);
    } else if (kind === "not") {
      const x1 = f.x1 - 0.5;
      g.beginPath();
      g.moveTo(0, -0.7); g.lineTo(x1 - 0.3, 0); g.lineTo(0, 0.7); g.closePath();
      g.fill(); g.stroke();
      bubble(g, x1 - 0.15, 0);
    } else if (kind === "in" || kind === "clock") {
      g.beginPath();
      g.rect(-1.4, -0.5, 1.1, 1);
      g.fill(); g.stroke();
      if (kind === "clock") {
        g.beginPath();
        g.moveTo(-1.2, 0.2); g.lineTo(-1.0, 0.2); g.lineTo(-1.0, -0.2); g.lineTo(-0.7, -0.2); g.lineTo(-0.7, 0.2); g.lineTo(-0.5, 0.2);
        g.stroke();
      }
      text(g, label, -1.6, 0, col.text, 0.6, "right");
    } else if (kind === "out" || kind === "led" || kind === "probe") {
      g.beginPath();
      if (kind === "led") g.arc(0.85, 0, 0.55, 0, Math.PI * 2);
      else g.rect(0.3, -0.5, 1.1, 1);
      g.fill(); g.stroke();
      text(g, label, 1.6, 0, col.text, 0.6, "left");
    } else if (kind === "const") {
      text(g, setting(p, "value") || "1", -0.25, 0, col.text, 0.6, "right");
    } else if (kind === "ground" || kind === "vdd") {
      g.beginPath();
      const dir = kind === "ground" ? 1 : -1;
      g.moveTo(0, 0); g.lineTo(0, 0.4 * dir);
      g.moveTo(-0.5, 0.4 * dir); g.lineTo(0.5, 0.4 * dir);
      if (kind === "ground") { g.moveTo(-0.3, 0.6); g.lineTo(0.3, 0.6); g.moveTo(-0.12, 0.8); g.lineTo(0.12, 0.8); }
      g.stroke();
    } else if (kind === "tunnel") {
      g.beginPath();
      g.moveTo(0, 0); g.lineTo(0.6, -0.35); g.lineTo(0.6, 0.35); g.closePath();
      g.fill(); g.stroke();
      text(g, setting(p, "net"), 0.8, 0, col.muted, 0.55, "left");
    } else if (kind === "split") {
      g.fillStyle = col.text;
      g.fillRect(f.x0 + 0.35, f.y0 - 0.3, 0.3, f.y1 - f.y0 + 0.6);
    } else if (DISPLAYS.has(kind)) {
      const b = body(p);
      g.beginPath();
      g.rect(b.x0, b.y0, b.x1 - b.x0, b.y1 - b.y0);
      g.fill(); g.stroke();
    } else if (kind === "pullup" || kind === "pulldown") {
      const dir = kind === "pullup" ? -1 : 1;
      g.beginPath();
      g.moveTo(0, 0); g.lineTo(0, 0.3 * dir);
      g.rect(-0.18, Math.min(0.3 * dir, 1 * dir), 0.36, 0.7);
      g.stroke();
      if (kind === "pullup") { g.beginPath(); g.moveTo(-0.35, -1.15); g.lineTo(0.35, -1.15); g.stroke(); }
    } else if (kind === "text") {
      text(g, setting(p, "text"), 0, 0, col.muted, 0.6, "left");
    } else {
      // A box exactly around its pins; a subcircuit is named for its circuit.
      const x0 = f.x0, x1 = Math.max(f.x1, f.x0 + 2);
      g.beginPath();
      g.rect(x0, f.y0 - 0.5, x1 - x0, f.y1 - f.y0 + 1);
      g.fill(); g.stroke();
      const name = kind.startsWith("sub:") ? kind.slice(4) : BOX_NAMES[kind] ?? kind;
      text(g, name, (x0 + x1) / 2, (f.y0 + f.y1) / 2, col.text, 0.55, "center");
      if (label) text(g, label, (x0 + x1) / 2, f.y0 - 0.95, col.muted, 0.5, "center");
    }
    // Pins: inputs hollow, outputs filled.
    for (const pin of p.pins) {
      const [x, y] = local(p, pin.x, pin.y);
      g.beginPath();
      g.arc(x, y, 0.12, 0, Math.PI * 2);
      g.fillStyle = pin.input ? col.bg : col.text;
      g.fill();
      g.stroke();
    }
    if (label && GATES.has(kind)) text(g, label, (f.x0 + f.x1) / 2, f.y0 - 1, col.muted, 0.5, "center");
    g.restore();
  }

  /** A display lit from its value: segments a–g and the point; a digit and
   *  its point; or a colour, R, G and B each as wide as a third of the bus.
   *  Drawn upright at the middle of the part's body.
   *  @param {CanvasRenderingContext2D} g @param {PartView} p @param {Value} v @param {ReturnType<typeof colours>} col */
  function face(g, p, v, col) {
    const b = body(p);
    const [cx, cy] = world(p, (b.x0 + b.x1) / 2, (b.y0 + b.y1) / 2);
    const n = BigInt(`0x${v.hex || "0"}`);
    if (p.kind === "rgbled") {
      const w = BigInt(Math.max(1, Math.floor(v.bits / 3)));
      const top = (1n << w) - 1n;
      const ch = (/** @type {bigint} */ i) => Number(((n >> (i * w)) & top) * 255n / (top || 1n));
      g.fillStyle = `rgb(${ch(0n)}, ${ch(1n)}, ${ch(2n)})`;
      g.beginPath();
      g.arc(cx, cy, 0.9, 0, Math.PI * 2);
      g.fill();
      return;
    }
    const segs = p.kind === "seg7" ? Number(n & 0xffn) : (DIGITS[Number(n & 0xfn)] ?? 0) | (Number((n >> 4n) & 1n) << 7);
    const h = Math.min(b.y1 - b.y0, 3) * 0.38, w = 0.55, t = 0.14;
    /** a b c d e f g as lines from the digit's centre. */
    const lines = [[-w, -h, w, -h], [w, -h, w, 0], [w, 0, w, h], [-w, h, w, h], [-w, 0, -w, h], [-w, -h, -w, 0], [-w, 0, w, 0]];
    g.save();
    g.lineWidth = t;
    g.lineCap = "round";
    lines.forEach(([x0, y0, x1, y1], i) => {
      g.strokeStyle = segs >> i & 1 ? col.danger : col.line;
      g.beginPath();
      g.moveTo(cx - 0.15 + x0, cy + y0);
      g.lineTo(cx - 0.15 + x1, cy + y1);
      g.stroke();
    });
    g.fillStyle = segs >> 7 & 1 ? col.danger : col.line;
    g.beginPath();
    g.arc(cx + w + 0.15, cy + h, t, 0, Math.PI * 2);
    g.fill();
    g.restore();
  }

  /** An AND's D or an OR's shield, from its inputs' column to its output.
   *  @param {CanvasRenderingContext2D} g @param {string} kind @param {{x0: number, x1: number, y0: number, y1: number}} f */
  function gate(g, kind, f) {
    const inv = INVERTED.has(kind);
    const x1 = f.x1 - (inv ? 1 : 0);
    const top = f.y0 - 0.5, bot = f.y1 + 0.5, mid = (top + bot) / 2, half = (bot - top) / 2;
    g.beginPath();
    if (kind === "and" || kind === "nand") {
      const flat = Math.max(f.x0, x1 - half);
      g.moveTo(f.x0, top); g.lineTo(flat, top);
      g.arc(flat, mid, half, -Math.PI / 2, Math.PI / 2);
      g.lineTo(f.x0, bot); g.closePath();
    } else {
      const back = f.x0 + (kind.includes("xor") || kind.includes("xnor") ? 0.35 : 0);
      g.moveTo(back, top);
      g.quadraticCurveTo(back + (x1 - back) * 0.6, top, x1, mid);
      g.quadraticCurveTo(back + (x1 - back) * 0.6, bot, back, bot);
      g.quadraticCurveTo(back + 0.6, mid, back, top);
      g.closePath();
    }
    g.fill(); g.stroke();
    if (kind === "xor" || kind === "xnor") {
      g.beginPath();
      g.moveTo(f.x0, top);
      g.quadraticCurveTo(f.x0 + 0.6, mid, f.x0, bot);
      g.stroke();
    }
    if (inv) bubble(g, x1 + 0.3, mid);
  }

  /** @param {CanvasRenderingContext2D} g @param {number} x @param {number} y */
  function bubble(g, x, y) {
    g.beginPath();
    g.arc(x, y, 0.28, 0, Math.PI * 2);
    g.fill(); g.stroke();
  }

  /** Text at a point of the part's frame, drawn in screen pixels so it is
   *  crisp at any zoom and upright however the part is turned.
   *  @param {CanvasRenderingContext2D} g @param {string} t @param {number} x @param {number} y @param {string} colour @param {number} size @param {CanvasTextAlign} align */
  function text(g, t, x, y, colour, size, align) {
    if (!t) return;
    const at = g.getTransform().transformPoint(new DOMPoint(x, y));
    const px = size * state.view.scale * (window.devicePixelRatio || 1);
    if (px < 5) return;
    g.save();
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.fillStyle = colour;
    g.font = `${px}px ui-monospace, "SF Mono", Menlo, monospace`;
    g.textAlign = align;
    g.textBaseline = "middle";
    g.fillText(t, at.x, at.y);
    g.restore();
  }

  /* ── pan and zoom ──────────────────────────────────────────────────── */

  /** Zoom by `k` about a point on the stage. @param {number} k @param {number} px @param {number} py */
  function zoom(k, px, py) {
    const v = state.view;
    const scale = Math.max(MIN_SCALE, Math.min(MAX_SCALE, v.scale * k));
    const real = scale / v.scale;
    state.view = { scale, ox: px - (px - v.ox) * real, oy: py - (py - v.oy) * real };
    redraw();
  }

  els.stage.addEventListener("wheel", (e) => {
    if (!state.project) return;
    e.preventDefault();
    const r = els.stage.getBoundingClientRect();
    if (e.ctrlKey || e.metaKey) {
      zoom(Math.exp(-e.deltaY / 120), e.clientX - r.left, e.clientY - r.top);
    } else {
      state.view = { ...state.view, ox: state.view.ox - e.deltaX, oy: state.view.oy - e.deltaY };
      redraw();
    }
  }, { passive: false });

  // WebKit's pinch.
  let pinchFrom = 1;
  els.stage.addEventListener("gesturestart", (e) => { e.preventDefault(); pinchFrom = 1; });
  els.stage.addEventListener("gesturechange", (e) => {
    e.preventDefault();
    const ge = /** @type {any} */ (e);
    const r = els.stage.getBoundingClientRect();
    zoom(ge.scale / pinchFrom, ge.clientX - r.left, ge.clientY - r.top);
    pinchFrom = ge.scale;
  });

  /** A screen point on the grid: exact, and snapped to the nearest step.
   *  @param {PointerEvent | MouseEvent} e */
  function gridOf(e) {
    const r = els.stage.getBoundingClientRect();
    const x = (e.clientX - r.left - state.view.ox) / state.view.scale, y = (e.clientY - r.top - state.view.oy) / state.view.scale;
    return { x, y, snap: /** @type {[number, number]} */ ([Math.round(x), Math.round(y)]) };
  }

  els.stage.addEventListener("pointerdown", (e) => {
    const p = state.project;
    // A popup over the drawing keeps its own clicks.
    if (/** @type {HTMLElement} */ (e.target).closest(".ci-pop, .ci-empty")) return;
    if (!p || (e.button !== 0 && e.button !== 1)) return;
    closeForms();
    els.stage.focus({ preventScroll: true });
    els.stage.setPointerCapture(e.pointerId);
    const at = gridOf(e);
    const base = { x: e.clientX, y: e.clientY, ox: state.view.ox, oy: state.view.oy, from: at.snap, to: at.snap, add: e.shiftKey, moved: false };
    state.cursor = at.snap;
    state.wiring = null;
    if (e.button === 1 || e.altKey) {
      state.drag = { kind: "pan", ...base };
      return;
    }
    // From a pin or a wire's end, close by: a wire.
    if (Math.hypot(at.x - at.snap[0], at.y - at.snap[1]) < 0.3 && wireEnd(at.snap[0], at.snap[1])) {
      state.drag = { kind: "wire", ...base };
      redraw();
      return;
    }
    const part = partAt(at.x, at.y);
    // Running: an input under the pointer is to be clicked — a button at
    // once, held while pressed; the others when the press ends without a drag.
    state.press = null;
    if (part && running() && !e.shiftKey && ["in", "clock", "button"].includes(part.kind)) {
      if (part.kind === "button") {
        runCommand("circuitSet", { id: part.id, value: "1", hold: runEls.hold.checked });
        state.press = { id: part.id, x: e.clientX, y: e.clientY };
        state.drag = null;
        return;
      }
      state.press = { id: part.id, x: e.clientX, y: e.clientY };
    }
    const wire = part ? null : wireAt(at.x, at.y);
    if (part || wire) {
      const key = part ? part.id : wireKey(/** @type {number[]} */ (wire));
      const set = part ? state.parts : state.wires;
      if (e.shiftKey) {
        if (set.has(key)) set.delete(key);
        else set.add(key);
      } else if (!set.has(key)) {
        state.parts = new Set(part ? [key] : []);
        state.wires = new Set(part ? [] : [key]);
      }
      state.drag = { kind: "move", ...base };
    } else {
      if (!e.shiftKey) {
        state.parts.clear();
        state.wires.clear();
      }
      state.drag = { kind: "box", ...base };
    }
    redraw();
  });
  els.stage.addEventListener("pointermove", (e) => {
    const d = state.drag;
    if (!d) return;
    if (d.kind === "pan") {
      state.view = { ...state.view, ox: d.ox + e.clientX - d.x, oy: d.oy + e.clientY - d.y };
    } else {
      const at = gridOf(e);
      d.to = d.kind === "box" ? [at.x, at.y] : at.snap;
      d.moved ||= Math.hypot(e.clientX - d.x, e.clientY - d.y) > 3;
    }
    redraw();
  });
  els.stage.addEventListener("pointerup", () => {
    const d = state.drag;
    state.drag = null;
    const press = state.press;
    state.press = null;
    const pressed = press && state.project?.parts.find((q) => q.id === press.id);
    if (pressed?.kind === "button") {
      runCommand("circuitSet", { id: pressed.id, value: "0", hold: runEls.hold.checked });
      return;
    }
    if (pressed && !d?.moved) {
      setInput(pressed);
      redraw();
      return;
    }
    if (!d) return;
    if (d.kind === "move" && d.moved) {
      const dx = d.to[0] - d.from[0], dy = d.to[1] - d.from[1];
      if (dx || dy) send([{ op: "move", ids: [...state.parts], wires: selectedWires(), dx, dy }]);
    } else if (d.kind === "wire" && (d.to[0] !== d.from[0] || d.to[1] !== d.from[1])) {
      send([{ op: "wire", points: legs(d.from, d.to) }]);
      state.cursor = d.to;
    } else if (d.kind === "box" && d.moved) {
      // Everything wholly inside the box.
      const x0 = Math.min(d.from[0], d.to[0]), x1 = Math.max(d.from[0], d.to[0]);
      const y0 = Math.min(d.from[1], d.to[1]), y1 = Math.max(d.from[1], d.to[1]);
      const inBox = (/** @type {number} */ x, /** @type {number} */ y) => x >= x0 && x <= x1 && y >= y0 && y <= y1;
      for (const part of state.project?.parts ?? []) {
        const b = box(part);
        if (inBox(b.x0, b.y0) && inBox(b.x1, b.y1)) state.parts.add(part.id);
      }
      for (const w of state.project?.wires ?? []) if (inBox(w[0], w[1]) && inBox(w[2], w[3])) state.wires.add(wireKey(w));
    }
    redraw();
  });
  els.stage.addEventListener("pointercancel", () => { state.drag = null; redraw(); });
  els.stage.addEventListener("dblclick", (e) => {
    const at = gridOf(e);
    const part = partAt(at.x, at.y);
    if (part && running() && (part.kind === "ram" || part.kind === "ram2")) saveMemory(part);
    else if (part) editSettings(part);
  });

  /* ── settings, the part picker, the clipboard ─────────────────────── */

  /** A form over the stage, at a grid point; gone on Escape or a click away.
   *  @param {string} html @param {[number, number]} at @param {(form: HTMLFormElement) => void} done */
  function popup(html, at, done) {
    closeForms();
    const form = document.createElement("form");
    form.className = "ci-pop";
    form.innerHTML = html;
    const r = els.stage.getBoundingClientRect();
    form.style.left = `${Math.max(8, Math.min(r.width - 330, state.view.ox + at[0] * state.view.scale + 12))}px`;
    form.style.top = `${Math.max(8, Math.min(r.height - 90, state.view.oy + at[1] * state.view.scale + 12))}px`;
    form.addEventListener("submit", (e) => {
      e.preventDefault();
      done(form);
      form.remove();
      els.stage.focus({ preventScroll: true });
    });
    form.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        form.remove();
        els.stage.focus({ preventScroll: true });
      }
      e.stopPropagation();
    });
    els.stage.append(form);
    /** @type {HTMLInputElement | null} */ (form.querySelector("input"))?.focus();
    return form;
  }
  function closeForms() {
    for (const f of els.stage.querySelectorAll(".ci-pop")) f.remove();
  }

  /** A part's id and settings, as `.circ` writes them, to edit and set.
   *  @param {PartView} part */
  function editSettings(part) {
    const quote = (/** @type {string} */ v) => (/^[\w.,*+:/#-]+$/.test(v) ? v : `"${v.replace(/["\\]/g, "\\$&")}"`);
    const settings = part.settings.map(([k, v]) => `${k}=${quote(v)}`).join(" ");
    popup(`<label class="ci-pop-row"><span>${esc(part.kind)}</span><input name="id" value="${esc(part.id)}" aria-label="Id" spellcheck="false"></label>
      <input name="settings" value="${esc(settings)}" placeholder="bits=8 label=A" aria-label="Settings" spellcheck="false">
      <p class="ci-pop-hint">Settings as key=value: bits, inputs, label, sel, value, net, in, out, wide, flip, signed… Enter sets, Escape leaves.</p>`,
    [part.x, part.y], (form) => {
      const id = /** @type {HTMLInputElement} */ (form.elements.namedItem("id")).value.trim();
      const text = /** @type {HTMLInputElement} */ (form.elements.namedItem("settings")).value;
      /** @type {[string, string][]} */
      const pairs = [];
      for (const m of text.matchAll(/([\w]+)=("((?:[^"\\]|\\.)*)"|\S*)/g)) pairs.push([m[1], m[3] !== undefined ? m[3].replace(/\\(.)/g, "$1") : m[2]]);
      const renamed = id && id !== part.id ? id : null;
      // The selection follows the part to its new name.
      if (renamed && state.parts.delete(part.id)) state.parts.add(renamed);
      send([{ op: "set", id: part.id, newId: renamed, settings: pairs }]);
    });
    /** @type {HTMLInputElement | null} */ (els.stage.querySelector(".ci-pop [name=settings]"))?.focus();
  }

  /** A running RAM's contents kept as a data file, for a ROM to read.
   *  @param {PartView} part */
  function saveMemory(part) {
    popup(`<label class="ci-pop-row"><span>Keep ${esc(part.id)}'s contents as data/</span><input name="name" value="${esc(part.id.toLowerCase())}" aria-label="Data file" spellcheck="false"><span>.hex</span></label>
      <p class="ci-pop-hint">A held edit, saved with ⌘S; a ROM reads it with file=&lt;name&gt;.</p>`,
    [part.x, part.y], (form) => {
      const name = /** @type {HTMLInputElement} */ (form.elements.namedItem("name")).value.trim();
      if (name) runCommand("circuitSaveMemory", { id: part.id, name });
    });
  }

  /** `a`: a part picked by name, placed at the cursor. */
  function pick() {
    const p = state.project;
    if (!p) return;
    popup(`<input name="kind" list="ci-kinds" placeholder="Part — and, mux, sub:alu…" aria-label="Part to place" spellcheck="false">
      <datalist id="ci-kinds">${p.palette.map((k) => `<option value="${esc(k)}">`).join("")}</datalist>`,
    state.cursor, (form) => {
      const kind = /** @type {HTMLInputElement} */ (form.elements.namedItem("kind")).value.trim();
      if (kind) place(kind);
    });
  }

  /** A part placed at the cursor, and remembered for `space`. @param {string} kind */
  function place(kind) {
    state.last = kind;
    send([{ op: "place", kind, x: state.cursor[0], y: state.cursor[1], rot: 0, mirror: false, settings: [] }]);
  }

  function copy() {
    const p = state.project;
    if (!p || nothingSelected()) return;
    host.dispatch({ type: "circuitCopy", generation: p.generation, circuit: p.shown, ids: [...state.parts], wires: selectedWires() });
  }

  /** What is on the clipboard, put in with its top left at the cursor. */
  async function paste() {
    const text = (await host.paste?.()) ?? null;
    const at = text && origin(text);
    if (!text || !at) return;
    send([{ op: "paste", text, dx: state.cursor[0] - at[0], dy: state.cursor[1] - at[1] }]);
  }

  /** The cursor moved, and the view after it if it would leave the stage.
   *  @param {number} dx @param {number} dy */
  function moveCursor(dx, dy) {
    state.cursor = [state.cursor[0] + dx, state.cursor[1] + dy];
    const r = els.stage.getBoundingClientRect();
    const v = state.view;
    const x = v.ox + state.cursor[0] * v.scale, y = v.oy + state.cursor[1] * v.scale;
    const margin = 3 * v.scale;
    if (x < margin) v.ox += margin - x;
    if (x > r.width - margin) v.ox -= x - (r.width - margin);
    if (y < margin) v.oy += margin - y;
    if (y > r.height - margin) v.oy -= y - (r.height - margin);
    redraw();
  }

  new ResizeObserver(() => {
    const p = state.project;
    if (!p) return;
    if (`${p.generation}/${p.shown}` !== state.fitted) fit();
    else redraw();
  }).observe(els.stage);

  /* ── keys and clicks ───────────────────────────────────────────────── */

  ws.addEventListener("keydown", (e) => {
    const p = state.project;
    // ⌘S saves wherever the focus is in the mode.
    if (p && e.metaKey && !e.altKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      host.dispatch({ type: "circuitSave" });
      return;
    }
    if (p && e.metaKey && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "r") {
      e.preventDefault();
      toggleRun();
      return;
    }
    if (e.target !== els.stage || !p) return;
    const r = els.stage.getBoundingClientRect();
    const k = e.key;
    /** @type {Record<string, () => void>} */
    const chords = {
      r: toggleRun,
      z: () => host.dispatch({ type: e.shiftKey ? "circuitRedo" : "circuitUndo" }),
      c: copy,
      x: () => { copy(); remove(); },
      v: () => void paste(),
      a: () => {
        state.parts = new Set(p.parts.map((part) => part.id));
        state.wires = new Set(p.wires.map(wireKey));
        redraw();
      },
    };
    if (e.metaKey && !e.altKey) {
      const run = chords[k.toLowerCase()];
      if (run) {
        e.preventDefault();
        run();
      }
      return;
    }
    if (e.ctrlKey && k === "r") {
      e.preventDefault();
      host.dispatch({ type: "circuitRedo" });
      return;
    }
    if (e.ctrlKey || e.altKey) return;
    const step = e.shiftKey ? 5 : 1;
    /** @type {Record<string, () => void>} */
    const keys = {
      "=": fit,
      "+": () => zoom(1.25, r.width / 2, r.height / 2),
      "-": () => zoom(0.8, r.width / 2, r.height / 2),
      h: () => moveCursor(-step, 0), H: () => moveCursor(-step, 0), ArrowLeft: () => moveCursor(-step, 0),
      l: () => moveCursor(step, 0), L: () => moveCursor(step, 0), ArrowRight: () => moveCursor(step, 0),
      k: () => moveCursor(0, -step), K: () => moveCursor(0, -step), ArrowUp: () => moveCursor(0, -step),
      j: () => moveCursor(0, step), J: () => moveCursor(0, step), ArrowDown: () => moveCursor(0, step),
      " ": () => place(state.last),
      a: pick,
      w: () => {
        // `w` starts a wire at the cursor, and again ends it there.
        const from = state.wiring;
        if (!from) {
          state.wiring = state.cursor;
        } else {
          state.wiring = null;
          if (from[0] !== state.cursor[0] || from[1] !== state.cursor[1]) send([{ op: "wire", points: legs(from, state.cursor) }]);
        }
        redraw();
      },
      Enter: () => {
        if (state.wiring) return keys.w();
        const ids = targets();
        const part = p.parts.find((q) => q.id === ids[0]);
        if (part && running() && (part.kind === "ram" || part.kind === "ram2")) return saveMemory(part);
        if (part) editSettings(part);
      },
      e: () => keys.Enter(),
      r: () => send(targets().length ? [{ op: "rotate", ids: targets() }] : []),
      m: () => send(targets().length ? [{ op: "mirror", ids: targets() }] : []),
      x: remove,
      Delete: remove,
      Backspace: remove,
      y: copy,
      p: () => void paste(),
      u: () => host.dispatch({ type: "circuitUndo" }),
      s: () => { if (running()) clockEdge(); },
      ".": () => { if (running()) runCommand("circuitStep"); },
      c: () => setLive(!state.live),
      t: () => {
        state.panel = !state.panel;
        renderPanel();
        redraw();
      },
      Escape: () => {
        // A quick run, or the clock left running, stops first.
        if (runOf()?.quick?.running) return void host.dispatch({ type: "circuitStop" });
        if (state.live) return void setLive(false);
        state.wiring = null;
        state.parts.clear();
        state.wires.clear();
        redraw();
      },
    };
    const run = keys[k];
    if (run) {
      e.preventDefault();
      run();
    }
  });

  /** The selection, or the part at the cursor, taken away. */
  function remove() {
    const ids = targets(), wires = selectedWires();
    if (!ids.length && !wires.length) {
      const w = wireAt(state.cursor[0], state.cursor[1]);
      if (w) wires.push(w);
    }
    if (ids.length || wires.length) send([{ op: "delete", ids, wires }]);
  }

  ws.addEventListener("click", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    const top = t.closest("[data-circuit-top]");
    if (top) {
      e.stopPropagation();
      host.dispatch({ type: "circuitSetTop", circuit: /** @type {HTMLElement} */ (top).dataset.circuitTop });
      return;
    }
    const imported = t.closest("[data-circuit-import]");
    if (imported) return void host.dispatch({ type: "circuitImport", path: /** @type {HTMLElement} */ (imported).dataset.circuitImport });
    if (t.closest("[data-circuit-export]")) {
      const p = state.project;
      if (!p) return;
      const r = els.stage.getBoundingClientRect();
      return void popup(`<div class="ci-pop-row"><span>Write ${esc(p.shown)} as</span></div>
        <div class="ci-pop-choices">
          <button type="button" class="ci-tool" data-circuit-export-as="verilog">Verilog</button>
          <button type="button" class="ci-tool" data-circuit-export-as="vhdl">VHDL</button>
          <button type="button" class="ci-tool" data-circuit-export-as="svg">SVG</button>
        </div>
        <p class="ci-pop-hint">Into ${esc(p.path)}/export/. Verilog and VHDL are of the circuit with its subcircuits inline; SVG is the drawing.</p>`,
      [(r.width - 340 - state.view.ox) / state.view.scale, (8 - state.view.oy) / state.view.scale], () => {});
    }
    const as = t.closest("[data-circuit-export-as]");
    if (as) {
      closeForms();
      return void runCommand("circuitExport", { circuit: state.project?.shown, format: /** @type {HTMLElement} */ (as).dataset.circuitExportAs });
    }
    const open = t.closest("[data-circuit-open]");
    if (open) return void host.dispatch({ type: "circuitOpen", path: /** @type {HTMLElement} */ (open).dataset.circuitOpen });
    const show = t.closest("[data-circuit-show]");
    if (show) return void host.dispatch({ type: "circuitShow", circuit: /** @type {HTMLElement} */ (show).dataset.circuitShow });
    if (t.closest("[data-circuit-add]")) {
      state.adding = true;
      renderList();
      return;
    }
    const placing = t.closest("[data-circuit-place]");
    if (placing) {
      place(/** @type {HTMLElement} */ (placing).dataset.circuitPlace ?? "and");
      els.stage.focus({ preventScroll: true });
      return;
    }
    if (t.closest("[data-circuit-new]")) return void newProject();
    if (t.closest("[data-circuit-save]")) return void host.dispatch({ type: "circuitSave" });
    if (t.closest("[data-circuit-fit]")) return void fit();
    const tab = t.closest("[data-circuit-tab]");
    if (tab) {
      state.tab = /** @type {"tests" | "graph" | "analysis" | "machines"} */ (/** @type {HTMLElement} */ (tab).dataset.circuitTab);
      return void renderPanel();
    }
    if (t.closest("[data-circuit-panel-toggle]")) {
      state.panel = !state.panel;
      renderPanel();
      return void redraw();
    }
    const table = t.closest("[data-circuit-table]");
    if (table) {
      keepTable();
      state.table = /** @type {HTMLElement} */ (table).dataset.circuitTable ?? "";
      panelEls.text.blur();
      return void renderPanel();
    }
    if (t.closest("[data-circuit-table-new]")) {
      state.naming = true;
      return void renderPanel();
    }
    if (t.closest("[data-circuit-run-all]")) return void runTables(null);
    if (t.closest("[data-circuit-table-run]")) return void runTables(state.table);
    if (t.closest("[data-circuit-table-save]")) return void keepTable();
    if (t.closest("[data-circuit-table-delete]")) {
      const p = state.project;
      if (p && state.table) host.dispatch({ type: "circuitTestSet", generation: p.generation, circuit: p.shown, label: state.table, text: null });
      return;
    }
    const machine = t.closest("[data-circuit-machine]");
    if (machine) {
      keepMachine();
      state.machineName = /** @type {HTMLElement} */ (machine).dataset.circuitMachine ?? "";
      state.machine = null;
      panelEls.machineText.blur();
      return void renderPanel();
    }
    if (t.closest("[data-circuit-machine-new]")) {
      state.namingMachine = true;
      return void renderPanel();
    }
    if (t.closest("[data-circuit-machine-keep]")) return void keepMachine();
    if (t.closest("[data-circuit-machine-build]")) {
      const nameIn = /** @type {HTMLInputElement | null} */ (panelEls.tools.querySelector("[data-circuit-machine-circuit]"));
      const ff = /** @type {HTMLSelectElement | null} */ (panelEls.tools.querySelector("[data-circuit-machine-ff]"))?.value ?? "d";
      const circuit = nameIn?.value.trim() ?? "";
      if (!circuit) return void nameIn?.focus();
      keepMachine();
      return void runCommand("circuitMachineBuild", { name: state.machineName, flipflop: ff, circuit });
    }
    if (t.closest("[data-circuit-build]")) {
      const f = panelEls.analyseForm;
      const expression = /** @type {HTMLInputElement} */ (f.elements.namedItem("expression")).value.trim();
      const name = /** @type {HTMLInputElement} */ (f.elements.namedItem("name")).value.trim();
      const style = /** @type {HTMLSelectElement} */ (f.elements.namedItem("style")).value;
      if (!name) return void /** @type {HTMLInputElement} */ (f.elements.namedItem("name")).focus();
      return void runCommand("circuitSynthesise", { circuit: expression ? null : state.project?.shown, expression: expression || null, style, name });
    }
    if (t.closest("[data-circuit-trace-data]")) return void runCommand("circuitTraceToData");
    if (t.closest("[data-circuit-trace-clear]")) return void runCommand("circuitTraceClear");
    if (t.closest("[data-circuit-run]")) return void toggleRun();
    if (t.closest("[data-circuit-clock]")) return void clockEdge();
    if (t.closest("[data-circuit-gate]")) return void runCommand("circuitStep");
    if (t.closest("[data-circuit-live]")) return void setLive(!state.live);
    if (t.closest("[data-circuit-quick]")) {
      setLive(false);
      return void runCommand(runOf()?.quick?.running ? "circuitStop" : "circuitQuick");
    }
    if (t.closest("[data-circuit-close]")) {
      const p = state.project;
      if (p?.dirty && !window.confirm(`${p.name} has unsaved changes. Close it and lose them?`)) return;
      host.dispatch({ type: "circuitClose", discard: true });
    }
  });

  els.list.addEventListener("submit", (e) => {
    e.preventDefault();
    const input = /** @type {HTMLInputElement | null} */ (els.list.querySelector("[data-circuit-add-form] input"));
    const name = input?.value.trim() ?? "";
    state.adding = false;
    if (name) host.dispatch({ type: "circuitAdd", name });
    else renderList();
  });
  els.list.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && state.adding) {
      state.adding = false;
      renderList();
    }
    e.stopPropagation();
  });
  els.list.addEventListener("focusout", (e) => {
    const form = els.list.querySelector("[data-circuit-add-form]");
    if (state.adding && form && !form.contains(/** @type {Node | null} */ (e.relatedTarget))) {
      state.adding = false;
      renderList();
    }
  });

  /** The home page's form, shown with its name field ready. */
  function newProject() {
    const form = /** @type {HTMLFormElement | null} */ (ws.querySelector("[data-circuit-form]"));
    if (!form) return;
    els.home.hidden = false;
    els.project.hidden = true;
    /** @type {HTMLInputElement | null} */ (form.querySelector("input"))?.focus();
  }

  ws.querySelector("[data-circuit-form]")?.addEventListener("submit", (e) => {
    e.preventDefault();
    const form = /** @type {HTMLFormElement} */ (e.target);
    const name = /** @type {HTMLInputElement} */ (form.querySelector("input")).value.trim();
    if (!name) return;
    host.dispatch({ type: "circuitNew", name });
    form.reset();
  });

  runEls.hz.addEventListener("change", () => { if (state.live) setLive(true); });

  panelEls.machineList.addEventListener("submit", (e) => {
    e.preventDefault();
    const p = state.project;
    const name = /** @type {HTMLInputElement | null} */ (panelEls.machineList.querySelector("input"))?.value.trim() ?? "";
    state.namingMachine = false;
    if (!p || !name) return void renderPanel();
    state.machineName = name;
    state.machine = null;
    host.dispatch({ type: "circuitMachineSet", generation: p.generation, name, text: "inputs X\noutputs Z\nstate S0\nstate S1 Z=1\nS0 -> S1 when X\nS1 -> S0 when X'\n" });
  });
  panelEls.machineList.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && state.namingMachine) {
      state.namingMachine = false;
      renderPanel();
    }
    e.stopPropagation();
  });
  panelEls.tools.addEventListener("keydown", (e) => e.stopPropagation());

  panelEls.analyseForm.addEventListener("submit", (e) => {
    e.preventDefault();
    const expression = /** @type {HTMLInputElement} */ (panelEls.analyseForm.elements.namedItem("expression")).value.trim();
    runCommand("circuitAnalyse", { circuit: expression ? null : state.project?.shown, expression: expression || null });
  });
  panelEls.analyseForm.addEventListener("keydown", (e) => e.stopPropagation());

  // A new table: named, then made with a header to fill in.
  panelEls.tables.addEventListener("submit", (e) => {
    e.preventDefault();
    const p = state.project;
    const name = /** @type {HTMLInputElement | null} */ (panelEls.tables.querySelector("input"))?.value.trim() ?? "";
    state.naming = false;
    if (!p || !name) return void renderPanel();
    const ins = p.parts.filter((q) => ["in", "clock", "button"].includes(q.kind)).map((q) => setting(q, "label") || q.id);
    const outs = p.parts.filter((q) => q.kind === "out").map((q) => setting(q, "label") || q.id);
    state.table = name;
    host.dispatch({ type: "circuitTestSet", generation: p.generation, circuit: p.shown, label: name, text: `${[...ins, ...outs].join(" ") || "A Y"}\n` });
  });
  panelEls.tables.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && state.naming) {
      state.naming = false;
      renderPanel();
    }
    e.stopPropagation();
  });
  panelEls.text.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.metaKey && e.key === "Enter") {
      e.preventDefault();
      runTables(state.table);
    } else if (e.metaKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      keepTable();
      host.dispatch({ type: "circuitSave" });
    } else if (e.key === "Escape") {
      keepTable();
      els.stage.focus({ preventScroll: true });
    }
  });
  panelEls.text.addEventListener("blur", keepTable);
  new ResizeObserver(() => { if (state.panel && state.tab === "graph") drawGraph(); }).observe(panelEls.graph);

  return {
    /** @param {{generation: number, machine: MachineView}} event */
    machine(event) {
      if (event.generation !== state.project?.generation) return;
      state.machine = { ...event.machine, name: event.machine.name ?? state.machineName };
      if (state.panel && state.tab === "machines") drawMachine();
    },
    /** @param {{generation: number, analysis: Analysis}} event */
    analysis(event) {
      if (event.generation !== state.project?.generation) return;
      state.analysis = event.analysis;
      state.panel = true;
      state.tab = "analysis";
      renderPanel();
    },
    /** @param {{generation: number, circuit: string, results: TestResult[]}} event */
    tests(event) {
      const p = state.project;
      if (!p || event.generation !== p.generation || event.circuit !== p.shown) return;
      state.resultsOf = `${p.generation}/${p.shown}`;
      for (const r of event.results) state.results.set(r.label, r);
      if (event.results.length === 1) state.table = event.results[0].label;
      state.panel = true;
      state.tab = "tests";
      renderPanel();
    },
    /** @param {{run: RunView | null}} event */
    run(event) {
      state.run = event.run;
      renderRun();
      redraw();
    },
    /** @param {{projects: Found[]}} event */
    home(event) {
      state.projects = event.projects;
      renderList();
    },
    /** @param {{project: ProjectView | null}} event */
    project(event) {
      state.project = event.project;
      if (!event.project) {
        state.fitted = "";
        state.run = null;
      }
      renderProject();
    },
    /** The mode is on screen: the drawing takes the keys. */
    shown() {
      requestAnimationFrame(() => {
        const p = state.project;
        if (p) {
          if (`${p.generation}/${p.shown}` !== state.fitted) fit();
          else draw();
          els.stage.focus({ preventScroll: true });
        }
      });
    },
    /** For the tests: the view, and the parts as drawn. */
    debug() {
      return { view: { ...state.view }, project: state.project, run: state.run, live: !!state.live };
    },
  };
}

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
 * an edge at a time, or quick-run by the core as fast as it goes. */

import { escapeHtml as esc, icon } from "./html.js";

/** @typedef {import("./bindings/CircuitProjectView").CircuitProjectView} ProjectView */
/** @typedef {import("./bindings/CircuitPartView").CircuitPartView} PartView */
/** @typedef {import("./bindings/CircuitFound").CircuitFound} Found */
/** @typedef {import("./bindings/CircuitRunView").CircuitRunView} RunView */
/** @typedef {import("./bindings/CircuitValue").CircuitValue} Value */

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
});

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
    const projects = state.projects
      .map((f) => `<button class="xl-row${f.path === p?.path ? " on" : ""}" data-circuit-open="${esc(f.path)}" title="${esc(f.path)}">
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
        if (part.kind === "led") {
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
    if (part) editSettings(part);
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

  return {
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

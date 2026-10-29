/* Circuit: a project's circuits drawn on their grid, its projects when none
 * is open.
 *
 * The core sends a circuit with every part's pins already placed — where
 * the simulator joins them — so a pin drawn here is a pin that connects.
 * Each part is drawn in its own frame: turned and mirrored as it is, the
 * pins taken back into that frame to size its body, so a part the window
 * has no shape for is still a box exactly around its pins.
 *
 * One 2D canvas, redrawn on a frame when anything moves: dragging pans,
 * the wheel and a pinch zoom about the pointer, `=` fits the circuit to the
 * window. A project is `generation`-numbered by the core; the view is kept
 * while the same one is redrawn, and fitted when another opens. */

import { escapeHtml as esc, icon } from "./html.js";

/** @typedef {import("./bindings/CircuitProjectView").CircuitProjectView} ProjectView */
/** @typedef {import("./bindings/CircuitPartView").CircuitPartView} PartView */
/** @typedef {import("./bindings/CircuitFound").CircuitFound} Found */

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

/** @param {{root: Document, dispatch: (command: any) => unknown}} host */
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
    /** A drag panning the view: where it started. @type {{x: number, y: number, ox: number, oy: number} | null} */
    drag: null,
    /** Naming a new circuit in the list. */
    adding: false,
  };

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
      `<div class="ci-section">Projects</div>${projects}`;
    if (state.adding) {
      const input = /** @type {HTMLInputElement | null} */ (els.list.querySelector("[data-circuit-add-form] input"));
      input?.focus();
    }
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
    if (`${p.generation}/${p.shown}` !== state.fitted) requestAnimationFrame(fit);
    else redraw();
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
    return { text: get("--text", "#ddd"), muted: get("--muted", "#888"), accent: get("--accent", "#6af"), bg: get("--bg", "#111"), danger: get("--danger", "#e55"), line: get("--xl-line", "#333") };
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
    g.strokeStyle = col.muted;
    g.lineWidth = Math.max(1.5 / s, 0.09);
    g.beginPath();
    /** @type {Map<string, number>} */
    const ends = new Map();
    for (const [ax, ay, bx, by] of p.wires) {
      g.moveTo(ax, ay);
      g.lineTo(bx, by);
      for (const k of [`${ax},${ay}`, `${bx},${by}`]) ends.set(k, (ends.get(k) ?? 0) + 1);
    }
    g.stroke();
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

    for (const part of p.parts) drawPart(g, part, col, s);
    g.restore();
  }

  /** @param {CanvasRenderingContext2D} g @param {PartView} p @param {ReturnType<typeof colours>} col @param {number} s */
  function drawPart(g, p, col, s) {
    const line = Math.max(1.5 / s, 0.08);
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

  els.stage.addEventListener("pointerdown", (e) => {
    if (!state.project || e.button !== 0) return;
    els.stage.focus({ preventScroll: true });
    els.stage.setPointerCapture(e.pointerId);
    state.drag = { x: e.clientX, y: e.clientY, ox: state.view.ox, oy: state.view.oy };
  });
  els.stage.addEventListener("pointermove", (e) => {
    const d = state.drag;
    if (!d) return;
    state.view = { ...state.view, ox: d.ox + e.clientX - d.x, oy: d.oy + e.clientY - d.y };
    redraw();
  });
  const endDrag = () => { state.drag = null; };
  els.stage.addEventListener("pointerup", endDrag);
  els.stage.addEventListener("pointercancel", endDrag);

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
    if (e.target !== els.stage || !p) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const r = els.stage.getBoundingClientRect();
    /** @type {Record<string, () => void>} */
    const keys = {
      "=": fit,
      "+": () => zoom(1.25, r.width / 2, r.height / 2),
      "-": () => zoom(0.8, r.width / 2, r.height / 2),
      h: () => { state.view.ox += 60; redraw(); },
      l: () => { state.view.ox -= 60; redraw(); },
      k: () => { state.view.oy += 60; redraw(); },
      j: () => { state.view.oy -= 60; redraw(); },
      ArrowLeft: () => { state.view.ox += 60; redraw(); },
      ArrowRight: () => { state.view.ox -= 60; redraw(); },
      ArrowUp: () => { state.view.oy += 60; redraw(); },
      ArrowDown: () => { state.view.oy -= 60; redraw(); },
    };
    const run = keys[e.key];
    if (run) {
      e.preventDefault();
      run();
    }
  });

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
    if (t.closest("[data-circuit-new]")) return void newProject();
    if (t.closest("[data-circuit-save]")) return void host.dispatch({ type: "circuitSave" });
    if (t.closest("[data-circuit-fit]")) return void fit();
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

  return {
    /** @param {{projects: Found[]}} event */
    home(event) {
      state.projects = event.projects;
      renderList();
    },
    /** @param {{project: ProjectView | null}} event */
    project(event) {
      state.project = event.project;
      if (!event.project) state.fitted = "";
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
      return { view: { ...state.view }, project: state.project };
    },
  };
}

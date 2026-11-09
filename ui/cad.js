/* CAD: a project's parts (docs/CAD.md). The part open has its steps on
 * the left — parameters, sketches and features, each with its mistake —
 * the part in 3D in the middle, or the sketch being drawn flat on its
 * plane, and the chosen step's settings on the right with the part's text.
 *
 * Every change is a line of the .part language sent to the core, which
 * holds the file until ⌘S, builds it, and sends the part back drawn: the
 * window keeps no model of its own. Typing in the Text tab is sent a
 * moment after it stops. */

import { escapeHtml as esc, icon } from "./html.js";
import { createSketcher, fresh } from "./cad-sketch.js";
import { asmText, historyHtml, libraryHtml, libraryParams, lineIndex, offsets, points, portOf, relPath, settingsHtml } from "./cad-assembly.js";
import { boxAt, drawText, drawingHistoryHtml, drawingSettingsHtml, freshName, itemOfBox, moved, scaleOf } from "./cad-drawing.js";
import { netLine, partLine, referenceFor, schematicHistoryHtml, schematicItemOfBox, schematicMoved, schematicSettingsHtml } from "./cad-schematic.js";
import { boardHistoryHtml, boardItemOfBox, boardMoved, boardSettingsHtml, layersOf, snap, trackLine, viaLine } from "./cad-board.js";

/** @typedef {import("./bindings/CadFound").CadFound} Found */
/** @typedef {import("./bindings/CadPartView").CadPartView} PartView */
/** @typedef {import("./bindings/CadStep").CadStep} Step */
/** @typedef {import("./bindings/CadSketchView").CadSketchView} SketchView */
/** @typedef {import("./bindings/CadOp").CadOp} Op */
/** @typedef {import("./bindings/CadAssemblyView").CadAssemblyView} AssemblyView */
/** @typedef {import("./bindings/CadDrawingView").CadDrawingView} DrawingView */
/** @typedef {import("./bindings/CadSchematicView").CadSchematicView} SchematicView */
/** @typedef {import("./bindings/CadBoardView").CadBoardView} BoardView */

/** How long typing must stop before the text is sent. */
const TEXT_WAIT = 300;

/** A step's line, from its settings. */
export const stepText = {
  /** @param {string} name @param {string} on @param {string} [offset] */
  sketch: (name, on, offset) => `sketch ${name} on ${on}${offset?.trim() ? ` offset ${offset.trim()}` : ""}`,
  /** @param {string} name @param {string} sketch @param {string} distance @param {boolean} symmetric
   *  @param {boolean} [through] @param {string} [how] `join`, `cut`, `intersect`, or none for a body of its own @param {string} [body] */
  extrude: (name, sketch, distance, symmetric, through = false, how = "", body = "") =>
    `extrude ${name} from ${sketch} ${through ? "through" : `distance ${distance.trim() || "10"}${symmetric ? " symmetric" : ""}`}${how && body ? ` ${how} ${body}` : ""}`,
  /** @param {string} name @param {string} sketch @param {string} axis @param {string} [angle] @param {string} [how] @param {string} [body] */
  revolve: (name, sketch, axis, angle, how = "", body = "") =>
    `revolve ${name} from ${sketch} about ${axis}${angle?.trim() ? ` angle ${angle.trim()}` : ""}${how && body ? ` ${how} ${body}` : ""}`,
  /** @param {string} name @param {string[]} edges each as written: `a.top & a.side(r.left)` @param {string} radius */
  fillet: (name, edges, radius) => `fillet ${name} on ${edges.join(", ")} radius ${radius.trim() || "1"}`,
  /** @param {string} name @param {string[]} edges @param {string} distance @param {string} [other] the second face's */
  chamfer: (name, edges, distance, other) => `chamfer ${name} on ${edges.join(", ")} distance ${distance.trim() || "1"}${other?.trim() ? ` and ${other.trim()}` : ""}`,
  /** @param {string} name @param {string} body @param {string} thickness @param {string[]} [open] faces taken away */
  shell: (name, body, thickness, open = []) => `shell ${name} of ${body} thickness ${thickness.trim() || "2"}${open.length ? ` open ${open.join(", ")}` : ""}`,
  /** @param {string} name @param {string} sketch @param {string} path @param {string} [how] @param {string} [body] */
  sweep: (name, sketch, path, how = "", body = "") => `sweep ${name} from ${sketch} along ${path}${how && body ? ` ${how} ${body}` : ""}`,
  /** @param {string} name @param {string} from @param {string} to @param {string} [how] @param {string} [body] */
  loft: (name, from, to, how = "", body = "") => `loft ${name} from ${from} to ${to}${how && body ? ` ${how} ${body}` : ""}`,
};

/** A list of names as typed: one a line, blank lines left out. @param {string} text */
export const lines = (text) => text.split("\n").map((x) => x.trim()).filter(Boolean);

/** A pick taken out of a list if it is in it, put in if not.
 *  @param {string[]} list @param {string} x */
export const toggled = (list, x) => (list.includes(x) ? list.filter((y) => y !== x) : [...list, x]);

/** A face a sketch or a hole can be put on by name: an extrude's or a
 *  revolve's cap, or a side named for the shape that swept it. @param {string} name */
export const placeable = (name) => /^[A-Za-z_][A-Za-z0-9_]*\.(top|bottom|start|end|side\([A-Za-z_][A-Za-z0-9_.]*\))$/.test(name);

/** Millimetres in one of the part's units. @param {string} units */
export const unitScale = (units) => ({ mm: 1, cm: 10, m: 1000, in: 25.4 })[units] ?? 1;

/** A point on a flat face in the face's own coordinates, in the part's
 *  units: its plane is origin, outward normal and x, nine numbers.
 *  @param {number[]} plane @param {number[]} p @param {number} scale @returns {[number, number]} */
export function onFace(plane, p, scale) {
  const [ox, oy, oz, nx, ny, nz, xx, xy, xz] = plane;
  const [yx, yy, yz] = [ny * xz - nz * xy, nz * xx - nx * xz, nx * xy - ny * xx];
  const d = [p[0] - ox, p[1] - oy, p[2] - oz];
  const r = (/** @type {number} */ v) => Math.round((v / scale) * 1000) / 1000;
  return [r(d[0] * xx + d[1] * xy + d[2] * xz), r(d[0] * yx + d[1] * yy + d[2] * yz)];
}

/** Cubic millimetres as words. @param {number} v */
export function volumeText(v) {
  if (Math.abs(v) >= 1000) return `${(v / 1000).toFixed(v >= 1e6 ? 0 : 2)} cm³`;
  return `${v.toFixed(1)} mm³`;
}

/** What a step is, in a few words. @param {Step} s */
function summary(s) {
  if (s.kind === "sketch") return `on ${s.on}${s.offset ? `, offset ${s.offset}` : ""}`;
  const into = s.how && s.body && !["union", "cut", "intersect"].includes(s.kind) ? ` · ${s.how} ${s.body}` : "";
  if (s.kind === "extrude") return `${s.sketch} · ${s.through ? "through" : s.distance}${s.symmetric ? " each way" : ""}${into}`;
  if (s.kind === "revolve") return `${s.sketch} · about ${s.axis}${s.angle ? ` · ${s.angle}°` : ""}${into}`;
  if (s.kind === "hole") return `on ${s.on}`;
  // A pick by a body's or face's edges reads as it is; two faces' edges count.
  const n = (/** @type {number} */ k) => (k === 1 && s.edges[0]?.includes(".edges") ? s.edges[0] : `${k} edge${k === 1 ? "" : "s"}`);
  if (s.kind === "fillet") return `${n(s.edges.length)} · r ${s.value}`;
  if (s.kind === "chamfer") return `${n(s.edges.length)} · ${s.value}${s.other ? ` × ${s.other}` : ""}`;
  if (s.kind === "shell") return `${s.body} · ${s.value} thick${s.open.length ? ` · ${s.open.length} open` : ""}`;
  if (s.kind === "sweep") return `${s.sketch} along ${s.path}${into}`;
  if (s.kind === "loft") return `${s.sketch} to ${s.path}${into}`;
  if (["union", "cut", "intersect"].includes(s.kind)) return `with ${s.body}`;
  return s.note ?? "";
}

/** @param {{root: Document | HTMLElement, dispatch: (cmd: any) => any}} host */
export function createCad(host) {
  const ws = /** @type {HTMLElement} */ (host.root.querySelector('.workspace[data-ws="cad"]'));
  if (!ws) return null;
  const q = (/** @type {string} */ s) => /** @type {HTMLElement} */ (ws.querySelector(s));
  const els = {
    adds: q("[data-cad-adds]"),
    history: q("[data-cad-history]"),
    list: q("[data-cad-list]"),
    home: q("[data-cad-home]"),
    found: q("[data-cad-found]"),
    library: q("[data-cad-library]"),
    form: /** @type {HTMLFormElement} */ (q("[data-cad-form]")),
    part: q("[data-cad-part]"),
    title: q("[data-cad-title]"),
    dirty: q("[data-cad-dirty]"),
    crumb: q("[data-cad-crumb]"),
    tools: q("[data-cad-tools]"),
    undo: /** @type {HTMLButtonElement} */ (q("[data-cad-undo]")),
    redo: /** @type {HTMLButtonElement} */ (q("[data-cad-redo]")),
    canvas: /** @type {HTMLCanvasElement} */ (q("[data-cad-canvas]")),
    sheet: q("[data-cad-sheet]"),
    pick: q("[data-cad-sheet-pick]"),
    sketch: /** @type {SVGSVGElement} */ (/** @type {unknown} */ (q("[data-cad-sketch]"))),
    value: /** @type {HTMLInputElement} */ (q("[data-cad-value]")),
    banner: q("[data-cad-banner]"),
    status: q("[data-cad-status]"),
    step: q("[data-cad-settings]"),
    textpane: q("[data-cad-textpane]"),
    agentpane: q("[data-cad-agentpane]"),
    text: /** @type {HTMLTextAreaElement} */ (q("[data-cad-text]")),
    diag: q("[data-cad-diag]"),
  };
  els.sketch.setAttribute("tabindex", "0");
  const state = {
    /** @type {Found[]} */ parts: [],
    /** @type {PartView | null} */ part: null,
    /** The step chosen, by its place among the part's items. @type {number | null} */ step: null,
    /** The sketch being drawn, by its place. @type {number | null} */ sketching: null,
    /** The face chosen in the view, and where on it — in its own
     *  coordinates — for a flat one. @type {{body: string, face: number, at?: [number, number]} | null} */ face: null,
    tab: /** @type {"step" | "text" | "agent"} */ ("step"),
    /** When the text was last typed: the core's copy is not put over it. */
    typedAt: 0,
    /** A step just added, chosen when it comes back; a sketch, opened. @type {{name: string, open: boolean} | null} */ want: null,
    /** The part last fitted in view. */ fitted: 0,
    /** Why there is no 3D view, if there is none. @type {string | null} */ noView: null,
    /** Edges picked in the view to round or chamfer, by body and name. @type {{body: string, pick: string}[]} */ edges: [],
    /** The radius or distance they get. */ edgeSize: "1",
    /** The assembly open, if one is. @type {AssemblyView | null} */ asm: null,
    /** Faces picked in an assembly to mate, by reference. @type {string[]} */ picks: [],
    /** The exploded view shown, by name. @type {string | null} */ exploded: null,
    /** A mate's value as typed. */ mateSize: "",
    /** What the 3D view last drew of an assembly. */ drawn: "",
    /** The library's families. @type {import("./bindings/CadFamily").CadFamily[]} */ families: [],
    /** The family chosen in the library, and its values as typed. @type {string | null} */ family: null,
    /** @type {Record<string, string>} */ libValues: {},
    /** The drawing open, if one is. @type {DrawingView | null} */ drw: null,
    /** The schematic open, if one is. @type {SchematicView | null} */ sch: null,
    /** The kind of part chosen to add to a schematic. */ schKind: "resistor",
    /** The board open, if one is. @type {BoardView | null} */ brd: null,
    /** What a click on the board draws, and with what. @type {"track" | "via" | null} */ tool: null,
    /** The track being drawn: its points on the board, mm. @type {number[][]} */ route: [],
    brdNet: "", brdLayer: "F.Cu", brdWidth: "",
    /** Where the pointer is, snapped, while a track is drawn. @type {number[] | null} */ hover: null,
    /** The sheet's zoom, and where its corner is in the stage, in pixels. */
    sheet: { zoom: 1, x: 0, y: 0, fitted: 0, shown: "" },
    /** A view being dragged on the sheet, or the sheet panned.
     *  @type {{box: import("./bindings/CadSheetBox").CadSheetBox, from: number[], by: number[]} | {pan: number[], at: number[]} | null} */ dragging: null,
  };
  /** @type {Awaited<ReturnType<typeof import("./cad-view.js").mountView>> | null} */
  let view = null;
  /** @type {Promise<void> | null} */
  let viewing = null;
  let textTimer = 0;

  const generation = () => state.part?.generation ?? state.asm?.generation ?? state.drw?.generation ?? state.sch?.generation ?? state.brd?.generation ?? 0;
  /** What is open: a part, an assembly, a drawing, a schematic or a board. */
  const doc = () => state.part ?? state.asm ?? state.drw ?? state.sch ?? state.brd;
  /** What is shown flat, as a sheet: a drawing, a schematic, a board. */
  const sheetDoc = () => state.drw ?? state.sch ?? state.brd;
  /** The statement a box on the sheet is. @param {string} name */
  const boxItem = (name) => (state.drw ? itemOfBox(state.drw, name) : state.sch ? schematicItemOfBox(state.sch, name) : state.brd ? boardItemOfBox(state.brd, name) : null);
  /** @param {Op[]} ops */
  const edit = (ops) => {
    if (doc()) host.dispatch({ type: "cadEdit", generation: generation(), ops });
  };
  const status = (/** @type {string} */ text) => (els.status.textContent = text);
  const sketchView = () => state.part?.sketches.find((s) => s.index === state.sketching) ?? null;

  const sketcher = createSketcher({
    svg: els.sketch,
    input: els.value,
    edit,
    drag: (point, x, y, done) => {
      const s = sketchView();
      if (s && state.part) host.dispatch({ type: "cadDrag", generation: generation(), sketch: s.name, point, x, y, done });
    },
    status,
    done: () => leaveSketch(),
  });

  /* ── the home page and the list ───────────────────────────────── */

  function renderList() {
    const rows = state.parts
      .map((f) => `<button class="xl-row${f.path === doc()?.path ? " on" : ""}" data-cad-open="${esc(f.path)}" title="${esc(f.path)}">${icon(f.kind === "assembly" ? "pattern" : f.kind === "drawing" ? "frame" : f.kind === "schematic" ? "node" : f.kind === "board" ? "layers" : "cad")}<span class="xl-row-name">${esc(f.name)}</span><span class="xl-kind">${esc(f.path.includes("/") ? f.path.slice(0, f.path.lastIndexOf("/")) : "")}</span></button>`)
      .join("");
    els.list.innerHTML = rows || `<p class="xl-empty">No .part files in this folder yet.</p>`;
    els.found.innerHTML = rows;
    if (els.library) els.library.innerHTML = state.families.length ? `<h3>Library</h3><p class="cd-muted">Parts made from published standards — boards, power, graphics, cooling, fasteners and the FormD T1 — each written into the folder's <code>library/</code> as a part like any other.</p>${libraryHtml(state.families, state.family, state.libValues, false)}` : "";
  }

  /* ── the part ─────────────────────────────────────────────────── */

  const names = () => [
    ...(state.part?.steps ?? []).map((s) => s.name),
    ...(state.part?.params ?? []).map((p) => p.name),
    ...(state.asm?.instances ?? []).map((i) => i.name),
    ...(state.asm?.mates ?? []).map((m) => m.name),
    ...(state.asm?.explodes ?? []).map((e) => e.name),
    ...(state.asm?.routes ?? []).map((r) => r.name),
    ...(state.drw?.items ?? []).map((i) => i.name),
    ...(state.sch?.items ?? []).map((i) => i.name),
  ];
  /** A point on screen as a point on the board, mm. @param {number} cx @param {number} cy */
  const toBoard = (cx, cy) => {
    const at = toSheet(cx, cy), o = state.brd?.origin ?? [0, 0];
    return [at[0] - o[0], at[1] - o[1]];
  };

  function renderHistory() {
    if (state.brd) {
      els.history.innerHTML = boardHistoryHtml(state.brd, state.step);
      return;
    }
    if (state.sch) {
      els.history.innerHTML = schematicHistoryHtml(state.sch, state.step);
      return;
    }
    if (state.drw) {
      els.history.innerHTML = drawingHistoryHtml(state.drw, state.step);
      return;
    }
    if (state.asm) {
      els.history.innerHTML = historyHtml(state.asm, state.step);
      return;
    }
    const p = state.part;
    if (!p) {
      els.history.innerHTML = "";
      return;
    }
    const params = p.params
      .map((x) => `<div class="cd-param" data-cad-param="${esc(x.name)}"><span class="cd-pname">${esc(x.name)}</span><input class="cd-pvalue" value="${esc(x.value)}" aria-label="${esc(x.name)}" data-cad-param-value="${esc(x.name)}"><span class="cd-now">${x.now == null ? "?" : x.value === String(x.now) ? "" : `= ${+x.now.toFixed(6)}`}</span></div>`)
      .join("");
    const steps = p.steps
      .map((s) => {
        const mark = s.error ? `<i class="cd-bad" title="${esc(s.error)}">!</i>` : s.note ? `<i class="cd-later" title="${esc(s.note)}">·</i>` : "";
        const kind = /** @type {Record<string, string>} */ ({ sketch: "sketch", revolve: "revolve", extrude: "extrude", hole: "hole", pattern: "pattern", mirror: "mirror", union: "extrude", cut: "extrude", intersect: "extrude", fillet: "fillet", chamfer: "chamfer", shell: "shell", sweep: "sweep", loft: "loft" })[s.kind] ?? "file";
        return `<button class="cd-row${s.index === state.step ? " on" : ""}${s.error ? " bad" : ""}${s.note ? " later" : ""}${s.index === state.sketching ? " drawing" : ""}" data-cad-step="${s.index}" title="${esc(s.error ?? s.note ?? s.text)}">${icon(kind)}<span class="cd-row-name">${esc(s.name || s.kind)}</span><span class="cd-row-what">${esc(summary(s))}</span>${mark}</button>`;
      })
      .join("");
    els.history.innerHTML = `<button class="cd-row cd-part-row${state.step == null ? " on" : ""}" data-cad-step="part">${icon("cad")}<span class="cd-row-name">${esc(p.title ?? p.name)}</span><span class="cd-row-what">${esc(p.units)}</span></button>` +
      (params ? `<div class="cd-params"><div class="cd-head">Parameters</div>${params}</div>` : "") +
      `<div class="cd-head">Steps</div>${steps || `<p class="xl-empty">Nothing yet: add a sketch, draw in it, then extrude or revolve it.</p>`}`;
  }

  /** A field of the settings panel. @param {string} label @param {string} control */
  const field = (label, control) => `<label class="cd-field"><span>${esc(label)}</span>${control}</label>`;
  const options = (/** @type {string[]} */ xs, /** @type {string | null | undefined} */ on) => xs.map((x) => `<option${x === on ? " selected" : ""}>${esc(x)}</option>`).join("");

  function renderStep() {
    if (state.brd) {
      els.step.innerHTML = boardSettingsHtml(state.brd, { step: state.step, tool: state.tool, net: state.brdNet, layer: state.brdLayer, width: state.brdWidth });
      return;
    }
    if (state.sch) {
      els.step.innerHTML = schematicSettingsHtml(state.sch, { step: state.step, kind: state.schKind });
      return;
    }
    if (state.drw) {
      els.step.innerHTML = drawingSettingsHtml(state.drw, { step: state.step, scale: scaleOf(state.drw.text) });
      return;
    }
    if (state.asm) {
      els.step.innerHTML = settingsHtml(state.asm, { step: state.step, picks: state.picks, exploded: state.exploded, found: state.parts, size: state.mateSize, families: state.families, family: state.family, values: state.libValues });
      return;
    }
    const p = state.part;
    if (!p) {
      els.step.innerHTML = "";
      return;
    }
    const s = p.steps.find((x) => x.index === state.step);
    const face = state.face ? p.bodies.find((b) => b.name === state.face?.body)?.faces[state.face.face] : null;
    let html = "";
    if (face) {
      const at = state.face?.at;
      html += `<section class="cd-card"><div class="cd-head">Face</div><p class="cd-name">${esc(face.name)}</p><p class="cd-muted">${esc(face.surface)}${at ? ` · clicked at (${at[0]}, ${at[1]})` : ""}</p>` +
        (face.plane && placeable(face.name)
          ? `<div class="cd-row-btns"><button class="cd-btn" data-cad-sketch-on="${esc(face.name)}">Sketch on this face</button>${at ? `<button class="cd-btn" data-cad-hole-at>Hole here</button>` : ""}</div>`
          : face.plane ? `<p class="cd-muted">A flat face not named for what made it: sketch on a cap or a side instead.</p>` : "") +
        `</section>`;
    }
    if (state.edges.length && (!s || (s.kind !== "fillet" && s.kind !== "chamfer"))) {
      html += `<section class="cd-card" data-cad-edges><div class="cd-head">${state.edges.length} edge${state.edges.length === 1 ? "" : "s"} picked</div>` +
        `<ul class="cd-picks">${state.edges.map((e) => `<li>${esc(e.pick)}</li>`).join("")}</ul>` +
        field("Size", `<input data-cad-edge-size value="${esc(state.edgeSize)}" aria-label="Radius or distance">`) +
        `<div class="cd-row-btns"><button class="cd-btn" data-cad-round>Round</button><button class="cd-btn" data-cad-chamfer>Chamfer</button><button class="cd-btn quiet" data-cad-unpick-edges>Clear</button></div>` +
        `<p class="cd-muted">Click more edges in the view to add them, or one picked to take it out.</p></section>`;
    }
    if (!s) {
      const bodies = p.bodies.map((b) => `<li><span>${esc(b.name)}</span><span class="cd-muted">${volumeText(b.volume)} · ${(b.area / 100).toFixed(2)} cm²</span></li>`).join("");
      html += `<section class="cd-card"><div class="cd-head">Part</div>` +
        field("Name", `<span class="cd-ro">${esc(p.title ?? p.name)}</span>`) +
        field("Units", `<span class="cd-ro">${esc(p.units)}</span>`) +
        (p.material ? field("Material", `<span class="cd-ro">${esc(p.material)}</span>`) : "") +
        `<p class="cd-muted">The name, units and material are the file's first lines: change them in Text.</p></section>` +
        `<section class="cd-card"><div class="cd-head">A parameter</div><form class="cd-inline" data-cad-param-form><input name="name" placeholder="name" aria-label="Parameter name" required><input name="value" placeholder="value" aria-label="Parameter value" required><button class="cd-btn">Add</button></form></section>` +
        `<section class="cd-card"><div class="cd-head">Bodies</div>${bodies ? `<ul class="cd-bodies">${bodies}</ul>` : `<p class="cd-muted">None built yet.</p>`}` +
        `<div class="cd-row-btns"><button class="cd-btn" data-cad-export="stl"${p.bodies.length ? "" : " disabled"}>Write STL</button><button class="cd-btn" data-cad-export="3mf"${p.bodies.length ? "" : " disabled"}>Write 3MF</button><button class="cd-btn" data-cad-export="step"${p.bodies.length ? "" : " disabled"} title="Exact, for machining and other CAD programs">Write STEP</button></div></section>` +
        `<section class="cd-card"><div class="cd-head">Drawing</div><p class="cd-muted">Its views on a sheet — front, top, right and isometric, hidden edges dashed — to dimension, print or send to a shop. Written beside it as <code>${esc(p.name)}.drawing</code>, or opened if it is there.</p><div class="cd-row-btns"><button class="cd-btn" data-cad-make-drawing="paper"${p.bodies.length ? "" : " disabled"}>Draw it</button><button class="cd-btn" data-cad-make-drawing="blueprint"${p.bodies.length ? "" : " disabled"}>As a blueprint</button></div></section>`;
      els.step.innerHTML = html;
      return;
    }
    const earlier = p.steps.filter((x) => x.index < s.index);
    const sketches = earlier.filter((x) => x.kind === "sketch").map((x) => x.name);
    const err = s.error ? `<p class="cd-error">${esc(s.error)}</p>` : s.note ? `<p class="cd-muted">${esc(s.note)}</p>` : "";
    const named = field("Name", `<input data-cad-f="name" value="${esc(s.name)}" aria-label="Name">`);
    // Bodies there before this step, for it to be made into.
    const bodies = [...new Set(earlier.filter((x) => (x.kind === "extrude" || x.kind === "revolve") && !x.how).map((x) => x.name))];
    const ops = `<label class="cd-field"><span>Makes</span><select data-cad-f="how" aria-label="What it makes">` +
      [["", "A body of its own"], ["join", "Joined to"], ["cut", "Cut from"], ["intersect", "What it shares with"]].map(([v, l]) => `<option value="${v}"${(s.how ?? "") === v ? " selected" : ""}>${l}</option>`).join("") +
      `</select></label>` + (s.how || bodies.length ? field("Body", `<select data-cad-f="body" aria-label="Body"${s.how ? "" : " disabled"}>${options([...new Set([...bodies, ...(s.body ? [s.body] : [])])], s.body)}</select>`) : "");
    if (s.kind === "sketch") {
      const sv = p.sketches.find((x) => x.index === s.index);
      const faces = p.bodies.filter((b) => earlier.some((x) => x.name === b.name)).flatMap((b) => b.faces.filter((f) => f.plane && placeable(f.name)).map((f) => f.name));
      const planes = [...new Set(["XY", "XZ", "YZ", ...earlier.filter((x) => x.kind === "sketch").map((x) => x.name), ...faces, s.on ?? "XY"])];
      const said = sv ? (sv.error ? `<p class="cd-error">${esc(sv.error.message)}</p>` : !sv.solved ? `<p class="cd-error">The constraints cannot all hold.</p>` : `<p class="cd-muted">${sv.dof === 0 ? "Fully defined." : `${sv.dof} degree${sv.dof === 1 ? "" : "s"} of freedom left.`} ${sv.regions ? `${sv.regions} closed region${sv.regions === 1 ? "" : "s"}.` : sv.profile ? esc(sv.profile) : ""}</p>`) : "";
      html += `<section class="cd-card"><div class="cd-head">Sketch</div>${named}` +
        field("On", `<select data-cad-f="on" aria-label="Plane">${options(planes, s.on)}</select>`) +
        field("Offset", `<input data-cad-f="offset" value="${esc(s.offset ?? "")}" placeholder="0" aria-label="Offset">`) +
        said + err + `<div class="cd-row-btns"><button class="cd-btn" data-cad-draw="${s.index}">${state.sketching === s.index ? "Done drawing" : "Draw in it"}</button><button class="cd-btn quiet" data-cad-remove="${s.index}">Delete</button></div></section>`;
    } else if (s.kind === "extrude") {
      const body = p.bodies.find((b) => b.name === s.name);
      html += `<section class="cd-card"><div class="cd-head">Extrude</div>${named}` +
        field("Sketch", `<select data-cad-f="sketch" aria-label="Sketch">${options([...new Set([...sketches, s.sketch ?? ""])], s.sketch)}</select>`) +
        field("Distance", `<input data-cad-f="distance" value="${esc(s.distance ?? "")}" aria-label="Distance"${s.through ? " disabled" : ""}>`) +
        `<label class="cd-check"><input type="checkbox" data-cad-f="symmetric"${s.symmetric ? " checked" : ""}${s.through ? " disabled" : ""}> Half each way</label>` +
        `<label class="cd-check"><input type="checkbox" data-cad-f="through"${s.through ? " checked" : ""}> Right through everything</label>` +
        ops + err + (body ? `<p class="cd-muted">${volumeText(body.volume)}</p>` : "") +
        `<div class="cd-row-btns"><button class="cd-btn quiet" data-cad-remove="${s.index}">Delete</button></div></section>`;
    } else if (s.kind === "revolve") {
      const sv = p.sketches.find((x) => x.name === s.sketch);
      const axes = [...new Set(["yaxis", "xaxis", ...(sv?.shapes.filter((x) => x.kind === "line").map((x) => x.name) ?? []), s.axis ?? "yaxis"])];
      const body = p.bodies.find((b) => b.name === s.name);
      html += `<section class="cd-card"><div class="cd-head">Revolve</div>${named}` +
        field("Sketch", `<select data-cad-f="sketch" aria-label="Sketch">${options([...new Set([...sketches, s.sketch ?? ""])], s.sketch)}</select>`) +
        field("About", `<select data-cad-f="axis" aria-label="Axis">${options(axes, s.axis)}</select>`) +
        field("Angle", `<input data-cad-f="angle" value="${esc(s.angle ?? "")}" placeholder="360" aria-label="Angle">`) +
        ops + err + (body ? `<p class="cd-muted">${volumeText(body.volume)}</p>` : "") +
        `<div class="cd-row-btns"><button class="cd-btn quiet" data-cad-remove="${s.index}">Delete</button></div></section>`;
    } else if (s.kind === "fillet" || s.kind === "chamfer") {
      const round = s.kind === "fillet";
      html += `<section class="cd-card"><div class="cd-head">${round ? "Round" : "Chamfer"}</div>${named}` +
        field("Edges", `<textarea class="cd-line" data-cad-f="edges" rows="${Math.max(2, s.edges.length)}" spellcheck="false" aria-label="Edges, one a line">${esc(s.edges.join("\n"))}</textarea>`) +
        field(round ? "Radius" : "Distance", `<input data-cad-f="value" value="${esc(s.value ?? "")}" aria-label="${round ? "Radius" : "Distance"}">`) +
        (round ? "" : field("Second face", `<input data-cad-f="other" value="${esc(s.other ?? "")}" placeholder="the same" aria-label="Distance along the second face">`)) +
        err + `<p class="cd-muted">Click an edge in the view to add it, or one listed to take it out. Each is its two faces, or a body's or face's <code>.edges</code>, with <code>(parallel z)</code>, <code>(convex)</code>, <code>(concave)</code>, <code>(round)</code> or <code>(straight)</code> to pick among them.</p>` +
        `<div class="cd-row-btns"><button class="cd-btn quiet" data-cad-remove="${s.index}">Delete</button></div></section>`;
    } else if (s.kind === "shell") {
      const all = [...new Set([...earlier.filter((x) => (x.kind === "extrude" || x.kind === "revolve" || x.kind === "sweep" || x.kind === "loft") && !x.how).map((x) => x.name), ...(s.body ? [s.body] : [])])];
      html += `<section class="cd-card"><div class="cd-head">Shell</div>${named}` +
        field("Body", `<select data-cad-f="body" aria-label="Body">${options(all, s.body)}</select>`) +
        field("Thickness", `<input data-cad-f="value" value="${esc(s.value ?? "")}" aria-label="Wall thickness">`) +
        field("Open faces", `<textarea class="cd-line" data-cad-f="open" rows="${Math.max(2, s.open.length)}" spellcheck="false" aria-label="Faces taken away, one a line">${esc(s.open.join("\n"))}</textarea>`) +
        err + `<p class="cd-muted">Click a face in the view to open it, or again to close it.</p>` +
        `<div class="cd-row-btns"><button class="cd-btn quiet" data-cad-remove="${s.index}">Delete</button></div></section>`;
    } else if (s.kind === "sweep" || s.kind === "loft") {
      const sweep = s.kind === "sweep";
      const body = p.bodies.find((b) => b.name === s.name);
      html += `<section class="cd-card"><div class="cd-head">${sweep ? "Sweep" : "Loft"}</div>${named}` +
        field(sweep ? "Profile" : "From", `<select data-cad-f="sketch" aria-label="${sweep ? "Profile" : "From"}">${options([...new Set([...sketches, s.sketch ?? ""])], s.sketch)}</select>`) +
        field(sweep ? "Along" : "To", `<select data-cad-f="path" aria-label="${sweep ? "Path" : "To"}">${options([...new Set([...sketches, s.path ?? ""])], s.path)}</select>`) +
        ops + err + (body ? `<p class="cd-muted">${volumeText(body.volume)}</p>` : "") +
        `<p class="cd-muted">${sweep ? "The profile stands square to the path where it starts; the path is lines and arcs end to end, turning on arcs." : "Two profiles with as many edges each, in parallel planes: lines to lines, arcs and circles to ones above them."}</p>` +
        `<div class="cd-row-btns"><button class="cd-btn quiet" data-cad-remove="${s.index}">Delete</button></div></section>`;
    } else {
      // Holes, patterns, mirrors and the rest: their line, to edit as it is.
      const what = /** @type {Record<string, string>} */ ({ hole: "Hole", pattern: "Pattern", mirror: "Mirror", union: "Union", cut: "Cut", intersect: "Intersect" })[s.kind] ?? s.kind;
      html += `<section class="cd-card"><div class="cd-head">${esc(what)}</div><textarea class="cd-line" data-cad-f="line" rows="3" spellcheck="false" aria-label="The step's line">${esc(s.text)}</textarea>${err}` +
        `<p class="cd-muted">${s.note ? "" : "Edit the line and leave it to make the change."}</p><div class="cd-row-btns"><button class="cd-btn quiet" data-cad-remove="${s.index}">Delete</button></div></section>`;
    }
    els.step.innerHTML = html;
  }

  /** A setting changed: the step's line written again. @param {HTMLElement} t */
  function changed(t) {
    const p = state.part;
    const s = p?.steps.find((x) => x.index === state.step);
    if (!p || !s) return;
    const v = (/** @type {string} */ k) => /** @type {HTMLInputElement | null} */ (els.step.querySelector(`[data-cad-f="${k}"]`));
    const name = v("name")?.value.trim() || s.name;
    let text = "";
    const made = (/** @type {Step} */ x) => (x.kind === "extrude" || x.kind === "revolve" || x.kind === "sweep" || x.kind === "loft") && !x.how;
    if (s.kind === "sketch") text = stepText.sketch(name, v("on")?.value ?? s.on ?? "XY", v("offset")?.value);
    else if (s.kind === "extrude" || s.kind === "revolve" || s.kind === "sweep" || s.kind === "loft") {
      const how = v("how")?.value ?? s.how ?? "";
      // A body to be made into: the one chosen, or the first there is.
      const body = how ? v("body")?.value || s.body || p.steps.find((x) => x.index < s.index && made(x))?.name || "" : "";
      const sketch = v("sketch")?.value ?? s.sketch ?? "";
      text = s.kind === "extrude"
        ? stepText.extrude(name, sketch, v("distance")?.value ?? s.distance ?? "10", !!v("symmetric")?.checked, !!v("through")?.checked, how, body)
        : s.kind === "revolve"
          ? stepText.revolve(name, sketch, v("axis")?.value ?? s.axis ?? "yaxis", v("angle")?.value, how, body)
          : stepText[s.kind](name, sketch, v("path")?.value ?? s.path ?? "", how, body);
    } else if (s.kind === "fillet") text = stepText.fillet(name, lines(v("edges")?.value ?? s.edges.join("\n")), v("value")?.value ?? s.value ?? "1");
    else if (s.kind === "chamfer") text = stepText.chamfer(name, lines(v("edges")?.value ?? s.edges.join("\n")), v("value")?.value ?? s.value ?? "1", v("other")?.value);
    else if (s.kind === "shell") text = stepText.shell(name, v("body")?.value ?? s.body ?? "", v("value")?.value ?? s.value ?? "2", lines(v("open")?.value ?? s.open.join("\n")));
    else text = v("line")?.value.trim().replace(/\s*\n\s*/g, " ") ?? "";
    if (text && text !== s.text) edit([{ op: "replace", index: s.index, text }]);
    void t;
  }

  /** A setting of the instance, mate or view chosen changed: its line
   *  written again. */
  function changedAssembly() {
    const a = state.asm;
    if (!a) return;
    const v = (/** @type {string} */ k) => /** @type {HTMLInputElement | null} */ (els.step.querySelector(`[data-cad-a-f="${k}"]`));
    const inst = a.instances.find((i) => i.index === state.step);
    const mate = a.mates.find((m) => m.index === state.step);
    const view3 = a.explodes.find((e) => e.index === state.step);
    const route = a.routes.find((r) => r.index === state.step);
    /** @type {Op[]} */
    const ops = [];
    if (inst) {
      const name = v("name")?.value.trim() || inst.name;
      const at = [0, 1, 2].map((k) => Number(v(`at${k}`)?.value ?? inst.at[k]) || 0);
      const turn = [0, 1, 2].map((k) => Number(v(`turn${k}`)?.value ?? inst.turn[k]) || 0);
      const ground = !!v("ground")?.checked;
      const place = lineIndex(a.text, `place ${inst.name} `);
      const held = lineIndex(a.text, `ground ${inst.name}`);
      const moved = at.some((x, k) => x !== inst.at[k]) || turn.some((x, k) => x !== inst.turn[k]);
      if (moved) ops.push(place >= 0 ? { op: "replace", index: place, text: asmText.place(inst.name, at, turn) } : { op: "insert", at: inst.index + 1, text: asmText.place(inst.name, at, turn) });
      if (ground !== inst.ground) {
        if (ground) ops.push({ op: "insert", at: inst.index + 1, text: asmText.ground(inst.name) });
        else if (held >= 0) ops.push({ op: "remove", index: held });
      }
      if (name !== inst.name) ops.push({ op: "replace", index: inst.index, text: asmText.instance(name, inst.path, inst.kind === "assembly" ? "assembly" : "part") });
    } else if (mate) {
      const text = asmText.mate(v("name")?.value.trim() || mate.name, v("kind")?.value ?? mate.kind, v("a")?.value.trim() || mate.a, v("b")?.value.trim() || mate.b, v("value")?.value ?? mate.value ?? "");
      if (text !== mate.text) ops.push({ op: "replace", index: mate.index, text });
    } else if (route) {
      const via = v("via") ? points(/** @type {HTMLInputElement} */ (v("via")).value) : route.via;
      const text = asmText.route(v("name")?.value.trim() || route.name, v("kind")?.value ?? route.kind, v("size")?.value ?? route.size, v("from")?.value.trim() || route.from, v("to")?.value.trim() || route.to, via, { bend: v("bend")?.value ?? route.bend ?? "", lead: route.lead ?? "", insert: route.insert ?? "" });
      if (text !== route.text) ops.push({ op: "replace", index: route.index, text });
    } else if (view3) {
      const text = v("line")?.value.trim().replace(/\s*\n\s*/g, " ") ?? "";
      if (text && text !== view3.text) ops.push({ op: "replace", index: view3.index, text });
    }
    if (ops.length) edit(ops);
  }

  function renderHead() {
    if (state.brd) {
      const d = state.brd;
      els.title.textContent = d.title ?? d.name;
      els.title.title = d.path;
      els.dirty.hidden = !d.dirty;
      els.undo.disabled = !d.canUndo;
      els.redo.disabled = !d.canRedo;
      els.tools.hidden = true;
      const errors = d.findings.filter((f) => f.error);
      const left = d.nets.reduce((s, n) => s + n.unrouted, 0);
      els.crumb.textContent = `${d.schematic ?? "no schematic"} · ${d.layers} layers · ${d.parts.length} parts · ${left ? `${left} to route` : "all routed"} · ${errors.length ? `${errors.length} error${errors.length === 1 ? "" : "s"}` : "no errors"}${state.tool ? ` · drawing ${state.tool === "track" ? `a track on ${state.brdLayer}` : "vias"}` : ""}`;
      els.crumb.classList.toggle("bad", !!errors.length);
      const problems = d.error ? [`Line ${d.error.line} does not read: ${d.error.message} — what is shown is what it was saved as.`] : errors.map((x) => `Line ${x.line}: ${x.message}`);
      els.banner.hidden = !problems.length;
      els.banner.innerHTML = problems.slice(0, 3).map((x) => `<div>${esc(x)}</div>`).join("") + (problems.length > 3 ? `<div>…and ${problems.length - 3} more</div>` : "");
      return;
    }
    if (state.sch) {
      const d = state.sch;
      els.title.textContent = d.title ?? d.name;
      els.title.title = d.path;
      els.dirty.hidden = !d.dirty;
      els.undo.disabled = !d.canUndo;
      els.redo.disabled = !d.canRedo;
      els.tools.hidden = true;
      const errors = d.findings.filter((f) => f.error);
      const warnings = d.findings.length - errors.length;
      els.crumb.textContent = `${d.parts.length} part${d.parts.length === 1 ? "" : "s"} · ${d.nets.length} net${d.nets.length === 1 ? "" : "s"} · ${errors.length ? `${errors.length} error${errors.length === 1 ? "" : "s"}` : "no errors"}${warnings ? `, ${warnings} warning${warnings === 1 ? "" : "s"}` : ""}`;
      els.crumb.classList.toggle("bad", !!errors.length);
      const problems = d.error ? [`Line ${d.error.line} does not read: ${d.error.message} — what is shown is what it was saved as.`] : errors.map((x) => `Line ${x.line}: ${x.message}`);
      els.banner.hidden = !problems.length;
      els.banner.innerHTML = problems.slice(0, 3).map((x) => `<div>${esc(x)}</div>`).join("") + (problems.length > 3 ? `<div>…and ${problems.length - 3} more</div>` : "");
      return;
    }
    if (state.drw) {
      const d = state.drw;
      els.title.textContent = d.title ?? d.name;
      els.title.title = d.path;
      els.dirty.hidden = !d.dirty;
      els.undo.disabled = !d.canUndo;
      els.redo.disabled = !d.canRedo;
      els.tools.hidden = true;
      const count = (/** @type {string[]} */ k) => d.items.filter((i) => k.includes(i.kind)).length;
      const v = count(["view", "section", "detail"]), m = count(["dim"]);
      els.crumb.textContent = `${d.of ?? "of nothing yet"} · ${d.sheet.replace("_", " ")} · ${v} view${v === 1 ? "" : "s"} · ${m} dimension${m === 1 ? "" : "s"}`;
      els.crumb.classList.toggle("bad", !!d.errors.length);
      const problems = d.error ? [`Line ${d.error.line} does not read: ${d.error.message} — what is shown is what last read.`] : d.errors.map((x) => `Line ${x.line}: ${x.message}`);
      els.banner.hidden = !problems.length;
      els.banner.innerHTML = problems.slice(0, 3).map((x) => `<div>${esc(x)}</div>`).join("") + (problems.length > 3 ? `<div>…and ${problems.length - 3} more</div>` : "");
      return;
    }
    if (state.asm) {
      const a = state.asm;
      els.title.textContent = a.title ?? a.name;
      els.title.title = a.path;
      els.dirty.hidden = !a.dirty;
      els.undo.disabled = !a.canUndo;
      els.redo.disabled = !a.canRedo;
      els.tools.hidden = true;
      els.crumb.textContent = `${a.bom.reduce((n, r) => n + r.count, 0)} parts · ${!a.solved ? "the mates cannot all hold" : a.dof === 0 ? "fully held" : `${a.dof} freedom${a.dof === 1 ? "" : "s"} left`}${state.exploded ? ` · exploded: ${state.exploded}` : ""}`;
      els.crumb.classList.toggle("bad", !a.solved);
      const problems = a.error ? [`Line ${a.error.line} does not read: ${a.error.message} — what is shown is what it was saved as.`] : a.errors.map((d) => `Line ${d.line}: ${d.message}`);
      els.banner.hidden = !problems.length;
      els.banner.innerHTML = problems.slice(0, 3).map((x) => `<div>${esc(x)}</div>`).join("") + (problems.length > 3 ? `<div>…and ${problems.length - 3} more</div>` : "");
      return;
    }
    const p = state.part;
    if (!p) return;
    els.title.textContent = p.title ?? p.name;
    els.title.title = p.path;
    els.dirty.hidden = !p.dirty;
    els.undo.disabled = !p.canUndo;
    els.redo.disabled = !p.canRedo;
    const sv = sketchView();
    els.tools.hidden = !sv;
    els.adds.hidden = false;
    els.crumb.textContent = sv
      ? `Sketch ${sv.name} on ${sv.on} — ${sv.error ? "a mistake on its line " + sv.error.line : !sv.solved ? "does not solve" : sv.dof === 0 ? "fully defined" : `${sv.dof} degree${sv.dof === 1 ? "" : "s"} of freedom left`}`
      : p.bodies.length ? p.bodies.map((b) => `${b.name} ${volumeText(b.volume)}`).join(" · ") : "";
    els.crumb.classList.toggle("bad", !!sv && (!sv.solved || !!sv.error));
    const problems = p.error ? [`Line ${p.error.line} does not read: ${p.error.message} — what is shown is what it last built.`] : p.errors.map((d) => `Line ${d.line}: ${d.message}`);
    els.banner.hidden = !problems.length;
    els.banner.innerHTML = problems.slice(0, 3).map((x) => `<div>${esc(x)}</div>`).join("") + (problems.length > 3 ? `<div>…and ${problems.length - 3} more</div>` : "");
  }

  function renderText() {
    const p = doc();
    if (!p) return;
    const focused = document.activeElement === els.text;
    if (els.text.value !== p.text && !(focused && Date.now() - state.typedAt < 1500)) els.text.value = p.text;
    // A schematic's are its rules check's: errors bad, warnings noted.
    const bad = "errors" in p ? p.errors : p.findings.filter((f) => f.error);
    const noted = "notes" in p ? p.notes : "findings" in p ? p.findings.filter((f) => !f.error) : [];
    els.diag.innerHTML = [p.error, ...bad, ...noted].filter(Boolean).map((d) => `<div class="${d === p.error || bad.includes(/** @type {any} */ (d)) ? "bad" : "note"}">${d?.line}: ${esc(d?.message ?? "")}</div>`).join("");
  }

  function renderTabs() {
    ws.querySelectorAll("[data-cad-tab]").forEach((b) => b.setAttribute("aria-selected", String(/** @type {HTMLElement} */ (b).dataset.cadTab === state.tab)));
    els.step.hidden = state.tab !== "step";
    els.textpane.hidden = state.tab !== "text";
    els.agentpane.hidden = state.tab !== "agent";
  }

  async function ensureView() {
    if (view || state.noView) return;
    viewing ??= (async () => {
      try {
        const { mountView } = await import("./cad-view.js");
        view = await mountView(els.canvas, {
          hover: (name) => {
            if (state.sketching == null) status(name ? `${name}  — click to choose it` : "");
          },
          pick: (body, face, point) => pickFace(body, face, point),
          pickEdge: (body, edge) => pickEdge(body, edge),
        });
        if (!view) state.noView = "This window has no WebGL, so the part cannot be drawn in 3D here. Its steps, settings and text work as ever.";
      } catch (e) {
        state.noView = `The 3D view did not start: ${e}`;
      }
      if (state.noView) status(state.noView);
      renderStage();
    })();
    await viewing;
  }

  /** A face clicked in the view, or nothing: chosen, or for a shell chosen
   *  opened or closed again. @param {string | null} body @param {number | null} face @param {number[] | null} point */
  function pickFace(body, face, point) {
    if (state.asm) {
      const f = body == null || face == null ? null : state.asm.bodies.find((b) => b.name === body)?.faces[face];
      if (!f || body == null) return;
      const ref = `${body}.${f.name}`;
      if (state.picks.includes(ref)) state.picks = state.picks.filter((x) => x !== ref);
      else if (state.picks.length && state.picks[0].split(".")[0] === body.split(".")[0]) state.picks = [ref];
      else state.picks = [...state.picks.slice(0, 1), ref];
      state.step = null;
      renderStep();
      renderStage();
      return;
    }
    const f = body == null || face == null ? null : state.part?.bodies.find((b) => b.name === body)?.faces[face];
    const chosen = state.part?.steps.find((x) => x.index === state.step);
    // A shell chosen: the face opened, or closed again.
    if (chosen?.kind === "shell" && f) {
      const text = stepText.shell(chosen.name, chosen.body ?? "", chosen.value ?? "2", toggled(chosen.open, f.name));
      return edit([{ op: "replace", index: chosen.index, text }]);
    }
    const at = f?.plane && point ? onFace(f.plane, point, unitScale(state.part?.units ?? "mm")) : undefined;
    state.face = body == null || face == null ? null : { body, face, at };
    renderStep();
  }

  /** The sheet's pixels to a millimetre, as fitted and zoomed. */
  function sheetScale() {
    const d = sheetDoc(), r = els.sheet.getBoundingClientRect();
    const fit = d ? Math.min(r.width / d.width, r.height / d.height) * 0.94 : 1;
    return { r, ppm: (fit > 0 ? fit : 1) * state.sheet.zoom };
  }

  /** A point on screen as millimetres on the sheet, from its bottom left.
   *  @param {number} cx @param {number} cy */
  function toSheet(cx, cy) {
    const { r, ppm } = sheetScale();
    return [(cx - r.left - state.sheet.x) / ppm, (sheetDoc()?.height ?? 0) - (cy - r.top - state.sheet.y) / ppm];
  }

  /** The sheet sized and placed; the chosen view, or the one dragged,
   *  outlined. */
  function placeSheet() {
    const d = sheetDoc();
    const svg = els.sheet.querySelector("svg");
    if (!d || !svg) return;
    const { r, ppm } = sheetScale();
    if (state.sheet.fitted !== d.generation && r.width > 0) {
      state.sheet.fitted = d.generation;
      state.sheet.zoom = 1;
      const fit = sheetScale().ppm;
      state.sheet.x = (r.width - d.width * fit) / 2;
      state.sheet.y = (r.height - d.height * fit) / 2;
      return placeSheet();
    }
    svg.style.width = `${d.width * ppm}px`;
    svg.style.height = `${d.height * ppm}px`;
    svg.style.transform = `translate(${state.sheet.x}px, ${state.sheet.y}px)`;
    drawRoute(svg);
    const it = d.items.find((i) => i.index === state.step);
    const drag = state.dragging && "box" in state.dragging ? state.dragging : null;
    const box = drag?.box ?? (it ? d.boxes.find((b) => (it.kind === "bom" ? b.name === "bom" : b.name === it.name)) : null);
    els.pick.hidden = !box;
    if (box) {
      const [dx, dy] = drag ? drag.by : [0, 0];
      Object.assign(els.pick.style, {
        left: `${state.sheet.x + (box.x0 + dx) * ppm}px`,
        top: `${state.sheet.y + (d.height - box.y1 - dy) * ppm}px`,
        width: `${(box.x1 - box.x0) * ppm}px`,
        height: `${(box.y1 - box.y0) * ppm}px`,
      });
    }
  }

  /** The track being drawn, over the board: its points, and the pointer's. @param {Element} svg */
  function drawRoute(svg) {
    let line = svg.querySelector("[data-cad-route]");
    const b = state.brd;
    if (!b || state.tool !== "track" || !state.route.length) return void line?.remove();
    if (!line) {
      line = document.createElementNS("http://www.w3.org/2000/svg", "polyline");
      line.setAttribute("data-cad-route", "");
      line.setAttribute("fill", "none");
      line.setAttribute("stroke-linecap", "round");
      line.setAttribute("stroke-linejoin", "round");
      line.setAttribute("stroke-opacity", "0.7");
      svg.appendChild(line);
    }
    const w = Number(state.brdWidth) || b.track;
    line.setAttribute("stroke", state.brdLayer === "B.Cu" ? "#4d7fc4" : state.brdLayer === "F.Cu" ? "#c83434" : "#c2c200");
    line.setAttribute("stroke-width", String(w));
    line.setAttribute("points", [...state.route, ...(state.hover ? [state.hover] : [])].map((p) => `${p[0] + b.origin[0]},${b.height - (p[1] + b.origin[1])}`).join(" "));
  }

  /** The track drawn so far written as a line of the board, and let go. */
  function finishRoute() {
    const b = state.brd;
    const pts = state.route.filter((p, i, a) => i === 0 || p[0] !== a[i - 1][0] || p[1] !== a[i - 1][1]);
    state.route = [];
    state.hover = null;
    if (!b || pts.length < 2 || !state.brdNet) return void placeSheet();
    const w = Number(state.brdWidth);
    edit([{ op: "insert", at: null, text: trackLine(state.brdNet, state.brdLayer, w > 0 ? w : null, pts) }]);
  }

  function renderStage() {
    const sd = sheetDoc();
    els.sheet.hidden = !sd;
    if (sd) {
      els.sketch.toggleAttribute("hidden", true);
      els.canvas.hidden = true;
      if (state.sheet.shown !== sd.svg) {
        state.sheet.shown = sd.svg;
        const svg = els.sheet.querySelector("svg");
        const at = document.createElement("div");
        at.innerHTML = sd.svg;
        const fresh = at.querySelector("svg");
        if (fresh) {
          fresh.classList.add("cd-sheet-svg");
          if (svg) svg.replaceWith(fresh);
          else els.sheet.prepend(fresh);
        }
      }
      placeSheet();
      return;
    }
    if (state.asm) {
      const a = state.asm;
      els.sketch.toggleAttribute("hidden", true);
      els.canvas.hidden = !!state.noView;
      if (!view) return void ensureView();
      /** @type {[string, number][]} */
      const marked = [];
      for (const ref of state.picks) {
        for (const b of a.bodies) {
          if (!ref.startsWith(`${b.name}.`)) continue;
          const f = b.faces.findIndex((x) => x.name === ref.slice(b.name.length + 1));
          if (f >= 0) marked.push([b.name, f]);
        }
      }
      // Drawn again only when what it shows changes: a choice in the
      // settings is not a new scene.
      const shown = JSON.stringify([a.generation, a.text, state.picks, state.exploded]);
      if (shown !== state.drawn) {
        state.drawn = shown;
        view.show(/** @type {any} */ ({ bodies: a.bodies, sketches: [], bounds: a.bounds }), { offsets: offsets(a, state.exploded), marked });
      }
      if (state.fitted !== a.generation) {
        state.fitted = a.generation;
        view.fit(a.bounds);
      }
      return;
    }
    const p = state.part;
    if (!p) return;
    const sv = sketchView();
    els.sketch.toggleAttribute("hidden", !sv);
    els.canvas.hidden = !!sv || !!state.noView;
    if (sv) {
      sketcher.show(sv);
      return;
    }
    if (view) {
      state.drawn = "";
      const chosen = p.steps.find((s) => s.index === state.step);
      view.show(p, {
        sketch: chosen?.kind === "sketch" ? chosen.index : null,
        edges: markedEdges(),
        // A round's, chamfer's or shell's faces, shown as its.
        faces: chosen && ["fillet", "chamfer"].includes(chosen.kind) ? [chosen.name, `${chosen.name}.corner`] : [],
      });
      if (state.fitted !== p.generation) {
        state.fitted = p.generation;
        view.fit(p.bounds);
      }
    } else void ensureView();
  }

  function render() {
    const p = doc();
    ws.querySelectorAll("[data-cad-part-only]").forEach((b) => ((/** @type {HTMLElement} */ (b)).hidden = !state.part));
    ws.querySelectorAll("[data-cad-asm-only]").forEach((b) => ((/** @type {HTMLElement} */ (b)).hidden = !state.asm));
    ws.querySelectorAll("[data-cad-drw-only]").forEach((b) => ((/** @type {HTMLElement} */ (b)).hidden = !state.drw));
    ws.querySelectorAll("[data-cad-sch-only]").forEach((b) => ((/** @type {HTMLElement} */ (b)).hidden = !state.sch));
    ws.querySelectorAll("[data-cad-brd-only]").forEach((b) => ((/** @type {HTMLElement} */ (b)).hidden = !state.brd));
    els.home.hidden = !!p;
    els.part.hidden = !p;
    els.list.hidden = !!p;
    els.history.hidden = !p;
    els.adds.hidden = !p;
    renderList();
    renderTabs();
    if (!p) return;
    renderHead();
    renderHistory();
    renderStep();
    renderText();
    renderStage();
  }

  /** The edges picked, as the view finds them in the part now: by body and
   *  edge, from their names. @returns {[string, number][]} */
  function markedEdges() {
    const p = state.part;
    if (!p) return [];
    /** @type {[string, number][]} */
    const out = [];
    for (const e of state.edges) {
      const b = p.bodies.find((x) => x.name === e.body);
      b?.edgePicks.forEach((x, i) => {
        if (x === e.pick) out.push([b.name, i]);
      });
    }
    return out;
  }

  /** An edge clicked in the view: added to the round or chamfer chosen, or
   *  to the edges picked for one, or taken out if it is there.
   *  @param {string} body @param {number} edge */
  function pickEdge(body, edge) {
    const p = state.part;
    const b = p?.bodies.find((x) => x.name === body);
    const pick = b?.edgePicks[edge];
    if (!p || !b) return;
    if (!pick) return status(b.edgeTurns[edge] === "seam" ? "That is a seam, where a face meets itself: there is no corner to round." : "The faces meet smoothly there: there is no corner to round.");
    const chosen = p.steps.find((x) => x.index === state.step);
    if (chosen && (chosen.kind === "fillet" || chosen.kind === "chamfer")) {
      const edges = toggled(chosen.edges, pick);
      if (!edges.length) return status("A round needs an edge: delete the step to have none.");
      const text = chosen.kind === "fillet" ? stepText.fillet(chosen.name, edges, chosen.value ?? "1") : stepText.chamfer(chosen.name, edges, chosen.value ?? "1", chosen.other ?? "");
      return edit([{ op: "replace", index: chosen.index, text }]);
    }
    const had = state.edges.findIndex((e) => e.body === body && e.pick === pick);
    if (had >= 0) state.edges.splice(had, 1);
    else state.edges.push({ body, pick });
    state.face = null;
    renderStep();
    renderStage();
  }

  /** @param {number} index */
  function enterSketch(index) {
    state.sketching = index;
    state.step = index;
    state.face = null;
    view?.unpick();
    render();
    els.sketch.focus();
  }

  function leaveSketch() {
    state.sketching = null;
    sketcher.leave();
    status("");
    render();
  }

  /** A step added, and chosen — a sketch opened — when it comes back.
   *  @param {"sketch" | "extrude" | "revolve" | "fillet" | "chamfer" | "shell" | "sweep" | "loft"} kind */
  function addStep(kind) {
    const p = state.part;
    if (!p) return;
    if (kind === "fillet" || kind === "chamfer") {
      if (!state.edges.length) return status(`Click the edges to ${kind === "fillet" ? "round" : "chamfer"} in the view first.`);
      const name = fresh(kind === "fillet" ? "r" : "c", names());
      const size = state.edgeSize.trim() || "1";
      const edges = [...new Set(state.edges.map((e) => e.pick))];
      state.edges = [];
      state.want = { name, open: false };
      return edit([{ op: "insert", at: null, text: kind === "fillet" ? stepText.fillet(name, edges, size) : stepText.chamfer(name, edges, size) }]);
    }
    if (kind === "shell") {
      const body = p.bodies.at(-1)?.name;
      if (!body) return status("Make a body first: a shell hollows one.");
      const face = state.face ? p.bodies.find((b) => b.name === state.face?.body)?.faces[state.face.face] : null;
      const name = fresh("sh", names());
      state.want = { name, open: false };
      return edit([{ op: "insert", at: null, text: stepText.shell(name, state.face?.body ?? body, "2", face ? [face.name] : []) }]);
    }
    if (kind === "sweep" || kind === "loft") {
      const sk = p.steps.filter((s) => s.kind === "sketch");
      if (sk.length < 2) return status(kind === "sweep" ? "Draw a path and a profile across its start first: two sketches." : "Draw two profiles first, in parallel planes.");
      const chosen = p.steps.find((s) => s.index === state.step && s.kind === "sketch");
      const last = chosen ?? sk[sk.length - 1];
      const other = sk.filter((s) => s.index !== last.index).at(-1) ?? sk[0];
      const name = fresh(kind === "sweep" ? "w" : "l", names());
      state.want = { name, open: false };
      return edit([{ op: "insert", at: null, text: kind === "sweep" ? stepText.sweep(name, last.name, other.name) : stepText.loft(name, other.name, last.name) }]);
    }
    const chosen = p.steps.find((s) => s.index === state.step);
    const sketch = chosen?.kind === "sketch" ? chosen : [...p.steps].reverse().find((s) => s.kind === "sketch");
    if (kind === "sketch") {
      const face = state.face ? p.bodies.find((b) => b.name === state.face?.body)?.faces[state.face.face] : null;
      const on = face && placeable(face.name) ? face.name : "XY";
      const name = fresh("s", names());
      state.want = { name, open: true };
      edit([{ op: "insert", at: null, text: `${stepText.sketch(name, on)}\nend` }]);
      return;
    }
    if (!sketch) return status("Add a sketch first, and draw a closed shape in it.");
    if (kind === "extrude") {
      const name = fresh("e", names());
      state.want = { name, open: false };
      edit([{ op: "insert", at: null, text: stepText.extrude(name, sketch.name, "10", false) }]);
    } else {
      const sv = p.sketches.find((s) => s.index === sketch.index);
      const axis = sv?.shapes.find((x) => x.kind === "line" && x.construction)?.name ?? "yaxis";
      const name = fresh("v", names());
      state.want = { name, open: false };
      edit([{ op: "insert", at: null, text: stepText.revolve(name, sketch.name, axis) }]);
    }
  }

  function sendText() {
    clearTimeout(textTimer);
    textTimer = 0;
    if (state.part && els.text.value !== state.part.text) host.dispatch({ type: "cadText", generation: generation(), text: els.text.value });
  }

  /* ── events from the window ───────────────────────────────────── */

  ws.addEventListener("click", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    const open = t.closest("[data-cad-open]");
    if (open) return void host.dispatch({ type: "cadOpen", path: /** @type {HTMLElement} */ (open).dataset.cadOpen });
    const step = /** @type {HTMLElement | null} */ (t.closest("[data-cad-step]"));
    if (step && !t.closest("input")) {
      const v = step.dataset.cadStep;
      state.step = v === "part" ? null : Number(v);
      state.face = null;
      state.edges = [];
      view?.unpick();
      if (state.sketching != null && state.step !== state.sketching) leaveSketch();
      else render();
      return;
    }
    const fam = /** @type {HTMLElement | null} */ (t.closest("[data-cad-family]"));
    if (fam) {
      state.family = state.family === fam.dataset.cadFamily ? null : fam.dataset.cadFamily ?? null;
      state.libValues = {};
      renderList();
      if (state.asm) renderStep();
      return;
    }
    if (t.closest("[data-cad-lib-make]")) {
      e.preventDefault();
      const f = state.families.find((x) => x.name === state.family);
      if (!f) return;
      host.dispatch({ type: "cadLibrary", family: f.name, params: libraryParams(f, state.libValues), add: !!state.asm && !!t.closest("[data-cad-settings]") });
      return;
    }
    const routeBtn = /** @type {HTMLElement | null} */ (t.closest("[data-cad-route-kind]"));
    if (routeBtn && state.asm && state.picks.length === 2) {
      const [a, b] = state.picks.map((p) => portOf(p) ?? p);
      const hard = routeBtn.dataset.cadRouteKind === "hard";
      const name = fresh("t", names());
      state.picks = [];
      state.want = { name, open: false };
      return edit([{ op: "insert", at: null, text: asmText.route(name, hard ? "hard" : "soft", hard ? "14/16" : "10/13", a, b) }]);
    }
    const kindBtn = /** @type {HTMLElement | null} */ (t.closest("[data-cad-mate-kind]"));
    if (kindBtn && state.asm && state.picks.length === 2) {
      const kind = kindBtn.dataset.cadMateKind ?? "planar";
      const [a, b] = kind === "fastened" ? state.picks.map((x) => x.split(".")[0]) : state.picks;
      const name = fresh("m", names());
      state.picks = [];
      state.want = { name, open: false };
      return edit([{ op: "insert", at: null, text: asmText.mate(name, kind, a, b, state.mateSize) }]);
    }
    if (t.closest("[data-cad-unpick]")) {
      state.picks = [];
      renderStep();
      return renderStage();
    }
    if (t.closest("[data-cad-fit-check]")) return void host.dispatch({ type: "cadFit", generation: generation() });
    const ex = /** @type {HTMLElement | null} */ (t.closest("[data-cad-explode]"));
    if (ex) {
      state.exploded = state.exploded === ex.dataset.cadExplode ? null : ex.dataset.cadExplode ?? null;
      return render();
    }
    const asmAdd = /** @type {HTMLElement | null} */ (t.closest("[data-cad-asm-add]"));
    if (asmAdd && state.asm) {
      const what = asmAdd.dataset.cadAsmAdd;
      if (what === "fit") return void host.dispatch({ type: "cadFit", generation: generation() });
      if (what === "explode") {
        const last = state.asm.instances.at(-1);
        if (!last) return status("Add a part first.");
        const name = fresh("apart", names());
        state.want = { name, open: false };
        return edit([{ op: "insert", at: null, text: `explode ${name} ${last.name} by (0, 0, 50)` }]);
      }
      state.step = null;
      state.picks = [];
      render();
      /** @type {HTMLElement | null} */ (els.step.querySelector("[data-cad-add-instance] select"))?.focus();
      return;
    }
    const removeAsm = /** @type {HTMLElement | null} */ (t.closest("[data-cad-remove]"));
    if (removeAsm && state.asm) {
      // An instance goes with its placement and its holding still.
      const i = Number(removeAsm.dataset.cadRemove);
      const inst = state.asm.instances.find((x) => x.index === i);
      const gone = [i, ...(inst ? [lineIndex(state.asm.text, `place ${inst.name} `), lineIndex(state.asm.text, `ground ${inst.name}`)] : [])].filter((x) => x >= 0).sort((x, y) => y - x);
      state.step = null;
      return edit(gone.map((index) => ({ op: "remove", index })));
    }
    if (t.closest("[data-cad-round]")) return addStep("fillet");
    if (t.closest("[data-cad-chamfer]")) return addStep("chamfer");
    if (t.closest("[data-cad-unpick-edges]")) {
      state.edges = [];
      renderStep();
      return renderStage();
    }
    const add = /** @type {HTMLElement | null} */ (t.closest("[data-cad-add]"));
    if (add?.dataset.cadAdd === "hole") {
      if (!state.face?.at) return status("Click a flat face in the view where the hole goes, then Hole here.");
      return;
    }
    if (add) return addStep(/** @type {any} */ (add.dataset.cadAdd));
    const draw = /** @type {HTMLElement | null} */ (t.closest("[data-cad-draw]"));
    if (draw) return state.sketching === Number(draw.dataset.cadDraw) ? leaveSketch() : enterSketch(Number(draw.dataset.cadDraw));
    const remove = /** @type {HTMLElement | null} */ (t.closest("[data-cad-remove]"));
    if (remove) {
      const i = Number(remove.dataset.cadRemove);
      if (state.sketching === i) leaveSketch();
      state.step = null;
      return edit([{ op: "remove", index: i }]);
    }
    const on = /** @type {HTMLElement | null} */ (t.closest("[data-cad-sketch-on]"));
    if (on) {
      const name = fresh("s", names());
      state.want = { name, open: true };
      return edit([{ op: "insert", at: null, text: `${stepText.sketch(name, on.dataset.cadSketchOn ?? "XY")}\nend` }]);
    }
    if (t.closest("[data-cad-hole-at]")) {
      const p = state.part, f = state.face;
      const face = f && p?.bodies.find((b) => b.name === f.body)?.faces[f.face];
      if (!face || !f?.at) return;
      const name = fresh("h", names());
      state.want = { name, open: false };
      return edit([{ op: "insert", at: null, text: `hole ${name} on ${face.name} at (${f.at[0]}, ${f.at[1]}) diameter 3 through` }]);
    }
    const exp = /** @type {HTMLElement | null} */ (t.closest("[data-cad-export]"));
    if (exp) return void host.dispatch({ type: "cadExport", generation: generation(), format: exp.dataset.cadExport });
    const toolBtn = /** @type {HTMLElement | null} */ (t.closest("[data-cad-brd-tool]"));
    if (toolBtn && state.brd) {
      const want = /** @type {"track" | "via" | ""} */ (toolBtn.dataset.cadBrdTool ?? "");
      state.tool = want && state.tool !== want ? want : null;
      state.route = [];
      state.hover = null;
      if (state.tool && !state.brdNet) state.brdNet = state.brd.nets[0]?.name ?? "";
      if (!layersOf(state.brd).includes(state.brdLayer)) state.brdLayer = "F.Cu";
      state.step = null;
      state.tab = "step";
      els.sheet.style.cursor = state.tool ? "crosshair" : "grab";
      return render();
    }
    if (t.closest("[data-cad-make-board]")) return void host.dispatch({ type: "cadMakeBoard", generation: generation() });
    const making = /** @type {HTMLElement | null} */ (t.closest("[data-cad-make-drawing]"));
    if (making) return void host.dispatch({ type: "cadDraw", generation: generation(), blueprint: making.dataset.cadMakeDrawing === "blueprint" });
    if (t.closest("[data-cad-drw-section]") && state.drw) {
      const name = freshName("s", names());
      state.want = { name, open: false };
      return edit([{ op: "insert", at: null, text: drawText.section(name, "y", "0", state.drw.width - 120, state.drw.height / 2) }]);
    }
    if (t.closest("[data-cad-sch-add]") && state.sch) {
      const what = /** @type {HTMLElement} */ (t.closest("[data-cad-sch-add]")).dataset.cadSchAdd;
      state.step = null;
      state.tab = "step";
      render();
      return void /** @type {HTMLElement | null} */ (els.step.querySelector(what === "net" ? "[data-cad-sch-net-form] [name=name]" : "[data-cad-sch-part-form] [name=kind]"))?.focus();
    }
    if (t.closest("[data-cad-drw-add]") && state.drw) {
      const what = /** @type {HTMLElement} */ (t.closest("[data-cad-drw-add]")).dataset.cadDrwAdd;
      if (what === "view") {
        const name = freshName("v", names());
        state.want = { name, open: false };
        return edit([{ op: "insert", at: null, text: drawText.view(name, "front", state.drw.width / 2, state.drw.height / 2) }]);
      }
      state.step = null;
      state.tab = "step";
      render();
      return void /** @type {HTMLElement | null} */ (els.step.querySelector(what === "dim" ? "[data-cad-drw-dim-form] [name=a]" : "[data-cad-drw-note-form] [name=text]"))?.focus();
    }
    const tool = /** @type {HTMLElement | null} */ (t.closest("[data-cad-tool]"));
    if (tool) {
      sketcher.tool(tool.dataset.cadTool ?? "select");
      ws.querySelectorAll("[data-cad-tool]").forEach((b) => b.setAttribute("aria-pressed", String(b === tool)));
      els.sketch.focus();
      return;
    }
    const holdBtn = /** @type {HTMLElement | null} */ (t.closest("[data-cad-hold]"));
    if (holdBtn) return sketcher.hold(holdBtn.dataset.cadHold ?? "");
    if (t.closest("[data-cad-done]")) return leaveSketch();
    if (t.closest("[data-cad-undo]")) return void host.dispatch({ type: "cadUndo", generation: generation() });
    if (t.closest("[data-cad-redo]")) return void host.dispatch({ type: "cadRedo", generation: generation() });
    if (t.closest("[data-cad-save]")) {
      sendText();
      return void host.dispatch({ type: "cadSave" });
    }
    if (t.closest("[data-cad-close]")) {
      leaveSketch();
      return void host.dispatch({ type: "cadClose", discard: false });
    }
    if (t.closest("[data-cad-fit]")) {
      if (sheetDoc()) return fitSheet();
      const d = state.part ?? state.asm;
      return state.sketching != null ? sketcher.fit() : d && view?.fit(d.bounds);
    }
    const tab = /** @type {HTMLElement | null} */ (t.closest("[data-cad-tab]"));
    if (tab) {
      state.tab = /** @type {any} */ (tab.dataset.cadTab);
      renderTabs();
    }
  });

  ws.addEventListener("dblclick", (e) => {
    const step = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-cad-step]"));
    const s = state.part?.steps.find((x) => String(x.index) === step?.dataset.cadStep);
    if (s?.kind === "sketch") enterSketch(s.index);
  });

  ws.addEventListener("change", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    if (t.matches("[data-cad-f]")) changed(t);
    if (t.matches("[data-cad-a-f]")) changedAssembly();
    if (t.matches("[data-cad-mate-value]")) state.mateSize = /** @type {HTMLInputElement} */ (t).value;
    if (t.matches("[data-cad-lib-param]")) state.libValues[/** @type {HTMLElement} */ (t).dataset.cadLibParam ?? ""] = /** @type {HTMLInputElement} */ (t).value;
    if (t.matches("[data-cad-edge-size]")) state.edgeSize = /** @type {HTMLInputElement} */ (t).value;
    if (t.matches("[data-cad-drw-line]")) {
      const text = /** @type {HTMLInputElement} */ (t).value.trim();
      if (text) edit([{ op: "replace", index: Number(/** @type {HTMLElement} */ (t).dataset.cadDrwLine), text }]);
    }
    if (t.matches("[data-cad-brd-net]")) state.brdNet = /** @type {HTMLSelectElement} */ (t).value;
    if (t.matches("[data-cad-brd-layer]")) {
      state.brdLayer = /** @type {HTMLSelectElement} */ (t).value;
      renderHead();
    }
    if (t.matches("[data-cad-brd-width]")) state.brdWidth = /** @type {HTMLInputElement} */ (t).value.trim();
    if (t.matches("[data-cad-sch-kind]")) {
      state.schKind = /** @type {HTMLSelectElement} */ (t).value;
      renderStep();
    }
    if (state.sch && t.matches("[data-cad-sch-sheet]")) {
      const line = `sheet ${/** @type {HTMLSelectElement} */ (t).value}`;
      const at = lineIndex(state.sch.text, "sheet ");
      const after = lineIndex(state.sch.text, "schematic ");
      edit([at >= 0 ? { op: "replace", index: at, text: line } : { op: "insert", at: after + 1, text: line }]);
    }
    if (state.drw && t.matches("[data-cad-drw-sheet], [data-cad-drw-scale], [data-cad-drw-style]")) {
      const word = t.matches("[data-cad-drw-sheet]") ? "sheet" : t.matches("[data-cad-drw-scale]") ? "scale" : "style";
      const line = `${word} ${/** @type {HTMLSelectElement} */ (t).value}`;
      const at = lineIndex(state.drw.text, `${word} `);
      const after = lineIndex(state.drw.text, "of ");
      edit([at >= 0 ? { op: "replace", index: at, text: line } : { op: "insert", at: after >= 0 ? after + 1 : 0, text: line }]);
    }
    if (t.matches("[data-cad-param-value]")) {
      const name = /** @type {HTMLElement} */ (t).dataset.cadParamValue ?? "";
      const value = /** @type {HTMLInputElement} */ (t).value.trim();
      if (value) edit([{ op: "param", name, value }]);
    }
  });

  ws.addEventListener("submit", (e) => {
    const form = /** @type {HTMLFormElement} */ (e.target);
    e.preventDefault();
    if (form.matches("[data-cad-lib-form]")) return;
    if (form === els.form) {
      const name = /** @type {HTMLInputElement} */ (form.querySelector("input")).value.trim();
      const kind = /** @type {HTMLSelectElement | null} */ (form.querySelector("[data-cad-kind]"))?.value;
      if (name) host.dispatch({ type: kind === "assembly" ? "cadNewAssembly" : kind === "schematic" ? "cadNewSchematic" : "cadNew", name });
    } else if (form.matches("[data-cad-add-instance]") && state.asm) {
      const sel = /** @type {HTMLSelectElement} */ (form.elements.namedItem("path"));
      const found = state.parts.find((f) => f.path === sel.value);
      if (!found) return status("Choose a part to add.");
      const typed = /** @type {HTMLInputElement} */ (form.elements.namedItem("name")).value.trim();
      const base = (typed || found.name).replace(/[^A-Za-z0-9_]/g, "_").replace(/^([0-9])/, "_$1");
      const name = names().includes(base) ? fresh(base, names()) : base;
      state.want = { name, open: false };
      edit([{ op: "insert", at: null, text: asmText.instance(name, relPath(state.asm.path, found.path), found.kind === "assembly" ? "assembly" : "part") }]);
    } else if (form.matches("[data-cad-sch-part-form]") && state.sch) {
      const val = (/** @type {string} */ n) => /** @type {HTMLInputElement | null} */ (form.elements.namedItem(n))?.value ?? "";
      const kind = val("kind");
      const reference = referenceFor(kind, state.sch.parts.map((p) => p.reference));
      state.want = { name: reference, open: false };
      edit([{ op: "insert", at: lineIndex(state.sch.text, "net ") >= 0 ? lineIndex(state.sch.text, "net ") : null, text: partLine({ reference, kind, takes: val("takes"), value: val("value"), footprint: val("footprint") }) }]);
    } else if (form.matches("[data-cad-sch-net-form]") && state.sch) {
      const val = (/** @type {string} */ n) => /** @type {HTMLInputElement} */ (form.elements.namedItem(n)).value.trim();
      const power = /** @type {HTMLInputElement} */ (form.elements.namedItem("power")).checked;
      if (!/^[A-Za-z0-9_+\-./#~]+$/.test(val("name")) || !val("pins")) return status("A net has a name — +12V, GND, PWM — and the pins it joins, PART.PIN.");
      state.want = { name: val("name"), open: false };
      edit([{ op: "insert", at: null, text: netLine(val("name"), power, val("pins"), val("current")) }]);
    } else if (form.matches("[data-cad-drw-view-form]") && state.drw) {
      const dir = /** @type {HTMLSelectElement} */ (form.elements.namedItem("dir")).value;
      const name = freshName(dir.slice(0, 1), names());
      state.want = { name, open: false };
      edit([{ op: "insert", at: null, text: drawText.view(name, dir, state.drw.width / 2, state.drw.height / 2) }]);
    } else if (form.matches("[data-cad-drw-dim-form]") && state.drw) {
      const val = (/** @type {string} */ n) => /** @type {HTMLInputElement} */ (form.elements.namedItem(n)).value.trim();
      const kind = val("kind");
      if (!val("view")) return status("Add a view first: a dimension is drawn in one.");
      if (!val("a") || (["distance", "angle", "clearance"].includes(kind) && !val("b"))) return status(kind === "distance" || kind === "angle" ? "A distance or an angle is between two faces." : kind === "clearance" ? "A clearance is between two parts." : "Name the face or hole it measures.");
      const name = freshName("d", names());
      state.want = { name, open: false };
      edit([{ op: "insert", at: null, text: drawText.dim(name, val("view"), kind, val("a"), ["distance", "angle", "clearance"].includes(kind) ? val("b") : "") }]);
    } else if (form.matches("[data-cad-drw-note-form]") && state.drw) {
      const val = (/** @type {string} */ n) => /** @type {HTMLInputElement} */ (form.elements.namedItem(n)).value.trim();
      const view = state.drw.items.find((i) => i.kind === "view");
      if (!view || !val("text") || !val("at")) return status("A note says something, at a face or hole, in a view.");
      const name = freshName("n", names());
      state.want = { name, open: false };
      edit([{ op: "insert", at: null, text: drawText.note(name, view.name, val("text"), val("at")) }]);
    } else if (form.matches("[data-cad-param-form]")) {
      const name = /** @type {HTMLInputElement} */ (form.elements.namedItem("name")).value.trim();
      const value = /** @type {HTMLInputElement} */ (form.elements.namedItem("value")).value.trim();
      if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(name) && value) edit([{ op: "param", name, value }]);
      else status("A parameter's name is a word: letters, digits and _, not starting with a digit.");
    }
  });

  els.text.addEventListener("input", () => {
    state.typedAt = Date.now();
    clearTimeout(textTimer);
    textTimer = window.setTimeout(sendText, TEXT_WAIT);
  });

  ws.addEventListener("keydown", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    // The agent's composer is typed in too: its keys are its own.
    const typing = t.closest("input, textarea, select, [contenteditable]");
    const open = doc();
    if (open && e.metaKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      sendText();
      host.dispatch({ type: "cadSave" });
      return;
    }
    if (open && e.metaKey && e.key.toLowerCase() === "z" && !typing) {
      e.preventDefault();
      host.dispatch({ type: e.shiftKey ? "cadRedo" : "cadUndo", generation: generation() });
      return;
    }
    if (typing || !open) return;
    if (state.brd && state.tool && e.key === "Escape") {
      e.preventDefault();
      if (state.route.length) {
        state.route = [];
        state.hover = null;
        placeSheet();
      } else {
        state.tool = null;
        els.sheet.style.cursor = "grab";
        render();
      }
      return;
    }
    if (state.sketching != null && sketcher.key(e)) {
      e.preventDefault();
      e.stopPropagation();
      ws.querySelectorAll("[data-cad-tool]").forEach((b) => b.setAttribute("aria-pressed", String(/** @type {HTMLElement} */ (b).dataset.cadTool === sketcher.debug().tool)));
      return;
    }
    if (state.sketching == null && !e.metaKey && !e.ctrlKey && e.key.toLowerCase() === "f") {
      e.preventDefault();
      if (sheetDoc()) fitSheet();
      else if ("bounds" in open) view?.fit(open.bounds);
    }
  });

  window.addEventListener("resize", () => {
    if (state.sketching != null) sketcher.resize();
    view?.resize();
    placeSheet();
  });

  /* ── the sheet: zoomed, panned, its views dragged ─────────────── */

  function fitSheet() {
    state.sheet.fitted = 0;
    placeSheet();
  }

  els.sheet.addEventListener("wheel", (e) => {
    const sd = sheetDoc();
    if (!sd) return;
    e.preventDefault();
    const at = toSheet(e.clientX, e.clientY);
    state.sheet.zoom = Math.min(40, Math.max(0.25, state.sheet.zoom * Math.exp(-e.deltaY * 0.002)));
    const { r, ppm } = sheetScale();
    state.sheet.x = e.clientX - r.left - at[0] * ppm;
    state.sheet.y = e.clientY - r.top - (sd.height - at[1]) * ppm;
    placeSheet();
  }, { passive: false });

  els.sheet.addEventListener("pointerdown", (e) => {
    const d = sheetDoc();
    if (!d || e.button !== 0) return;
    // Drawing copper on a board: a click is a point, not a drag.
    if (state.brd && state.tool) {
      const [bx, by] = toBoard(e.clientX, e.clientY);
      const at = snap(state.brd, bx, by);
      if (state.tool === "via") {
        if (!state.brdNet) return status("Choose the via's net first.");
        return edit([{ op: "insert", at: null, text: viaLine(state.brdNet, at.x, at.y) }]);
      }
      // A track starts on a pad's net; it ends on a pad.
      if (!state.route.length && at.pad?.net) {
        state.brdNet = at.pad.net;
        renderStep();
      }
      state.route.push([at.x, at.y]);
      if (at.pad && state.route.length >= 2) return finishRoute();
      placeSheet();
      return;
    }
    const at = toSheet(e.clientX, e.clientY);
    const box = boxAt(d, at[0], at[1]);
    try {
      els.sheet.setPointerCapture(e.pointerId);
    } catch {
      // A pointer the page made, not a real one: nothing to capture.
    }
    state.dragging = box && boxItem(box.name) ? { box, from: at, by: [0, 0] } : { pan: [e.clientX, e.clientY], at: [state.sheet.x, state.sheet.y] };
  });

  els.sheet.addEventListener("pointermove", (e) => {
    const g = state.dragging;
    const sd = sheetDoc();
    if (state.brd && state.tool === "track" && state.route.length) {
      const [bx, by] = toBoard(e.clientX, e.clientY);
      const at = snap(state.brd, bx, by);
      state.hover = [at.x, at.y];
      placeSheet();
      return;
    }
    if (!g || !sd) {
      const at = sd && toSheet(e.clientX, e.clientY);
      const box = at && sd && boxAt(sd, at[0], at[1]);
      els.sheet.style.cursor = box && boxItem(box.name) ? "move" : "grab";
      return;
    }
    if ("box" in g) {
      const at = toSheet(e.clientX, e.clientY);
      g.by = [at[0] - g.from[0], at[1] - g.from[1]];
    } else {
      state.sheet.x = g.at[0] + e.clientX - g.pan[0];
      state.sheet.y = g.at[1] + e.clientY - g.pan[1];
    }
    placeSheet();
  });

  els.sheet.addEventListener("pointerup", () => {
    const g = state.dragging, d = sheetDoc();
    state.dragging = null;
    if (!g || !d) return;
    if ("box" in g) {
      const it = boxItem(g.box.name);
      if (it && Math.hypot(g.by[0], g.by[1]) > 0.5) {
        const line = state.brd ? boardMoved(state.brd, it, g.by[0], g.by[1]) : state.sch ? schematicMoved(state.sch, it, g.by[0], g.by[1]) : moved(it, g.by[0], g.by[1]);
        state.step = it.index;
        if (line) return edit([{ op: "replace", index: it.index, text: line }]);
      } else if (it) {
        state.step = it.index;
        render();
        return;
      }
    } else if (Math.hypot(state.sheet.x - g.at[0], state.sheet.y - g.at[1]) < 2 && state.step != null) {
      state.step = null;
      render();
      return;
    }
    placeSheet();
  });

  els.sheet.addEventListener("dblclick", () => {
    if (state.brd && state.tool === "track" && state.route.length >= 2) return finishRoute();
    if (!state.tool) fitSheet();
  });

  render();

  return {
    /** @param {{parts: Found[]}} e */
    home(e) {
      state.parts = e.parts;
      renderList();
    },
    /** @param {{families: import("./bindings/CadFamily").CadFamily[]}} e */
    families(e) {
      state.families = e.families;
      renderList();
      if (state.asm) renderStep();
    },
    /** @param {{board: BoardView | null}} e */
    board(e) {
      const was = state.brd?.generation;
      state.brd = e.board;
      if (!e.board) {
        if (was != null) {
          state.step = null;
          state.tool = null;
          state.route = [];
        }
      } else {
        state.part = null;
        state.asm = null;
        state.drw = null;
        state.sch = null;
        if (was !== e.board.generation) {
          state.step = null;
          state.tool = null;
          state.route = [];
          state.brdNet = e.board.nets[0]?.name ?? "";
          state.sketching = null;
          sketcher.leave();
        }
        if (!e.board.nets.some((n) => n.name === state.brdNet)) state.brdNet = e.board.nets[0]?.name ?? "";
        if (state.step != null && !e.board.items.some((x) => x.index === state.step)) state.step = null;
      }
      render();
    },
    /** @param {{schematic: SchematicView | null}} e */
    schematic(e) {
      const was = state.sch?.generation;
      state.sch = e.schematic;
      if (!e.schematic) {
        if (was != null) state.step = null;
      } else {
        state.part = null;
        state.asm = null;
        state.drw = null;
        state.brd = null;
        if (was !== e.schematic.generation) {
          state.step = null;
          state.sketching = null;
          state.picks = [];
          sketcher.leave();
        }
        const want = state.want;
        const got = want && e.schematic.items.find((x) => x.name === want.name);
        if (want && got) {
          state.want = null;
          state.step = got.index;
        }
        if (state.step != null && !e.schematic.items.some((x) => x.index === state.step)) state.step = null;
      }
      render();
    },
    /** @param {{drawing: DrawingView | null}} e */
    drawing(e) {
      const was = state.drw?.generation;
      state.drw = e.drawing;
      if (!e.drawing) {
        if (was != null) state.step = null;
      } else {
        state.part = null;
        state.asm = null;
        state.sch = null;
        state.brd = null;
        if (was !== e.drawing.generation) {
          state.step = null;
          state.sketching = null;
          state.picks = [];
          sketcher.leave();
        }
        const want = state.want;
        const got = want && e.drawing.items.find((x) => x.name === want.name);
        if (want && got) {
          state.want = null;
          state.step = got.index;
        }
        if (state.step != null && !e.drawing.items.some((x) => x.index === state.step)) state.step = null;
      }
      render();
    },
    /** @param {{assembly: AssemblyView | null}} e */
    assembly(e) {
      const was = state.asm?.generation;
      state.asm = e.assembly;
      if (!e.assembly) {
        if (was != null) {
          state.step = null;
          state.picks = [];
          state.exploded = null;
        }
      } else {
        state.part = null;
        state.drw = null;
        state.sch = null;
        state.brd = null;
        if (was !== e.assembly.generation) {
          state.step = null;
          state.picks = [];
          state.exploded = null;
          state.sketching = null;
          sketcher.leave();
        }
        const want = state.want;
        const all = [...e.assembly.instances, ...e.assembly.mates, ...e.assembly.explodes, ...e.assembly.routes];
        const got = want && all.find((x) => x.name === want.name);
        if (want && got) {
          state.want = null;
          state.step = got.index;
        }
        if (state.step != null && !all.some((x) => x.index === state.step)) state.step = null;
        if (state.exploded && !e.assembly.explodes.some((x) => x.name === state.exploded)) state.exploded = null;
      }
      render();
    },
    /** @param {{part: PartView | null}} e */
    part(e) {
      const was = state.part?.generation;
      state.part = e.part;
      if (!e.part) {
        // No part: closed — or an assembly open instead, whose own event
        // says what is chosen in it.
        if (was != null) {
          state.step = null;
          state.sketching = null;
          state.face = null;
          sketcher.leave();
        }
      } else {
        state.asm = null;
        state.drw = null;
        state.sch = null;
        state.brd = null;
        if (was !== e.part.generation) {
          state.step = null;
          state.sketching = null;
          state.face = null;
          sketcher.leave();
        }
        // A step asked for, now come back.
        const want = state.want;
        const got = want && e.part.steps.find((s) => s.name === want.name);
        if (want && got) {
          state.want = null;
          state.step = got.index;
          if (want.open) state.sketching = got.index;
        }
        // A step or sketch gone is not chosen.
        if (state.step != null && !e.part.steps.some((s) => s.index === state.step)) state.step = null;
        if (state.sketching != null && !e.part.sketches.some((s) => s.index === state.sketching)) state.sketching = null;
      }
      render();
    },
    shown() {
      if (doc()) requestAnimationFrame(() => {
        placeSheet();
        view?.resize();
        if (state.sketching != null) sketcher.resize();
      });
    },
    debug() {
      return { drw: state.drw, sch: state.sch, brd: state.brd, tool: state.tool, route: state.route, brdNet: state.brdNet, toBoard, sheet: state.sheet, toSheet, family: state.family, libValues: state.libValues, asm: state.asm, picks: state.picks, exploded: state.exploded, part: state.part, step: state.step, sketching: state.sketching, face: state.face, edges: state.edges, tab: state.tab, noView: state.noView, sketch: sketcher.debug(), view: view?.debug() ?? null, viewer: view, sketcher, pickEdge, pickFace };
    },
  };
}

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

/** @typedef {import("./bindings/CadFound").CadFound} Found */
/** @typedef {import("./bindings/CadPartView").CadPartView} PartView */
/** @typedef {import("./bindings/CadStep").CadStep} Step */
/** @typedef {import("./bindings/CadSketchView").CadSketchView} SketchView */
/** @typedef {import("./bindings/CadOp").CadOp} Op */

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
};

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
    form: /** @type {HTMLFormElement} */ (q("[data-cad-form]")),
    part: q("[data-cad-part]"),
    title: q("[data-cad-title]"),
    dirty: q("[data-cad-dirty]"),
    crumb: q("[data-cad-crumb]"),
    tools: q("[data-cad-tools]"),
    undo: /** @type {HTMLButtonElement} */ (q("[data-cad-undo]")),
    redo: /** @type {HTMLButtonElement} */ (q("[data-cad-redo]")),
    canvas: /** @type {HTMLCanvasElement} */ (q("[data-cad-canvas]")),
    sketch: /** @type {SVGSVGElement} */ (/** @type {unknown} */ (q("[data-cad-sketch]"))),
    value: /** @type {HTMLInputElement} */ (q("[data-cad-value]")),
    banner: q("[data-cad-banner]"),
    status: q("[data-cad-status]"),
    step: q("[data-cad-settings]"),
    textpane: q("[data-cad-textpane]"),
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
    tab: /** @type {"step" | "text"} */ ("step"),
    /** When the text was last typed: the core's copy is not put over it. */
    typedAt: 0,
    /** A step just added, chosen when it comes back; a sketch, opened. @type {{name: string, open: boolean} | null} */ want: null,
    /** The part last fitted in view. */ fitted: 0,
    /** Why there is no 3D view, if there is none. @type {string | null} */ noView: null,
  };
  /** @type {Awaited<ReturnType<typeof import("./cad-view.js").mountView>> | null} */
  let view = null;
  /** @type {Promise<void> | null} */
  let viewing = null;
  let textTimer = 0;

  const generation = () => state.part?.generation ?? 0;
  /** @param {Op[]} ops */
  const edit = (ops) => {
    if (state.part) host.dispatch({ type: "cadEdit", generation: generation(), ops });
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
      .map((f) => `<button class="xl-row${f.path === state.part?.path ? " on" : ""}" data-cad-open="${esc(f.path)}" title="${esc(f.path)}">${icon("cad")}<span class="xl-row-name">${esc(f.name)}</span><span class="xl-kind">${esc(f.path.includes("/") ? f.path.slice(0, f.path.lastIndexOf("/")) : "")}</span></button>`)
      .join("");
    els.list.innerHTML = rows || `<p class="xl-empty">No .part files in this folder yet.</p>`;
    els.found.innerHTML = rows;
  }

  /* ── the part ─────────────────────────────────────────────────── */

  const names = () => [...(state.part?.steps ?? []).map((s) => s.name), ...(state.part?.params ?? []).map((p) => p.name)];

  function renderHistory() {
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
        const kind = /** @type {Record<string, string>} */ ({ sketch: "sketch", revolve: "revolve", extrude: "extrude", hole: "hole", pattern: "pattern", mirror: "mirror", union: "extrude", cut: "extrude", intersect: "extrude" })[s.kind] ?? "file";
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
    if (!s) {
      const bodies = p.bodies.map((b) => `<li><span>${esc(b.name)}</span><span class="cd-muted">${volumeText(b.volume)} · ${(b.area / 100).toFixed(2)} cm²</span></li>`).join("");
      html += `<section class="cd-card"><div class="cd-head">Part</div>` +
        field("Name", `<span class="cd-ro">${esc(p.title ?? p.name)}</span>`) +
        field("Units", `<span class="cd-ro">${esc(p.units)}</span>`) +
        (p.material ? field("Material", `<span class="cd-ro">${esc(p.material)}</span>`) : "") +
        `<p class="cd-muted">The name, units and material are the file's first lines: change them in Text.</p></section>` +
        `<section class="cd-card"><div class="cd-head">A parameter</div><form class="cd-inline" data-cad-param-form><input name="name" placeholder="name" aria-label="Parameter name" required><input name="value" placeholder="value" aria-label="Parameter value" required><button class="cd-btn">Add</button></form></section>` +
        `<section class="cd-card"><div class="cd-head">Bodies</div>${bodies ? `<ul class="cd-bodies">${bodies}</ul>` : `<p class="cd-muted">None built yet.</p>`}` +
        `<div class="cd-row-btns"><button class="cd-btn" data-cad-export="stl"${p.bodies.length ? "" : " disabled"}>Write STL</button><button class="cd-btn" data-cad-export="3mf"${p.bodies.length ? "" : " disabled"}>Write 3MF</button></div></section>`;
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
    if (s.kind === "sketch") text = stepText.sketch(name, v("on")?.value ?? s.on ?? "XY", v("offset")?.value);
    else if (s.kind === "extrude" || s.kind === "revolve") {
      const how = v("how")?.value ?? s.how ?? "";
      // A body to be made into: the one chosen, or the first there is.
      const body = how ? v("body")?.value || s.body || p.steps.find((x) => x.index < s.index && (x.kind === "extrude" || x.kind === "revolve") && !x.how)?.name || "" : "";
      text = s.kind === "extrude"
        ? stepText.extrude(name, v("sketch")?.value ?? s.sketch ?? "", v("distance")?.value ?? s.distance ?? "10", !!v("symmetric")?.checked, !!v("through")?.checked, how, body)
        : stepText.revolve(name, v("sketch")?.value ?? s.sketch ?? "", v("axis")?.value ?? s.axis ?? "yaxis", v("angle")?.value, how, body);
    } else text = v("line")?.value.trim().replace(/\s*\n\s*/g, " ") ?? "";
    if (text && text !== s.text) edit([{ op: "replace", index: s.index, text }]);
    void t;
  }

  function renderHead() {
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
    const p = state.part;
    if (!p) return;
    const focused = document.activeElement === els.text;
    if (els.text.value !== p.text && !(focused && Date.now() - state.typedAt < 1500)) els.text.value = p.text;
    els.diag.innerHTML = [p.error, ...p.errors, ...p.notes].filter(Boolean).map((d) => `<div class="${d === p.error || p.errors.includes(/** @type {any} */ (d)) ? "bad" : "note"}">${d?.line}: ${esc(d?.message ?? "")}</div>`).join("");
  }

  function renderTabs() {
    ws.querySelectorAll("[data-cad-tab]").forEach((b) => b.setAttribute("aria-selected", String(/** @type {HTMLElement} */ (b).dataset.cadTab === state.tab)));
    els.step.hidden = state.tab !== "step";
    els.textpane.hidden = state.tab !== "text";
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
          pick: (body, face, point) => {
            const f = body == null || face == null ? null : state.part?.bodies.find((b) => b.name === body)?.faces[face];
            const at = f?.plane && point ? onFace(f.plane, point, unitScale(state.part?.units ?? "mm")) : undefined;
            state.face = body == null || face == null ? null : { body, face, at };
            renderStep();
          },
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

  function renderStage() {
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
      view.show(p, { sketch: p.steps.find((s) => s.index === state.step && s.kind === "sketch")?.index ?? null });
      if (state.fitted !== p.generation) {
        state.fitted = p.generation;
        view.fit(p.bounds);
      }
    } else void ensureView();
  }

  function render() {
    const p = state.part;
    els.home.hidden = !!p;
    els.part.hidden = !p;
    els.list.hidden = !!p;
    els.history.hidden = !p;
    els.adds.hidden = !p;
    renderList();
    if (!p) return;
    renderHead();
    renderHistory();
    renderStep();
    renderText();
    renderTabs();
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
   *  @param {"sketch" | "extrude" | "revolve"} kind */
  function addStep(kind) {
    const p = state.part;
    if (!p) return;
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
      view?.unpick();
      if (state.sketching != null && state.step !== state.sketching) leaveSketch();
      else render();
      return;
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
    if (t.closest("[data-cad-fit]")) return state.sketching != null ? sketcher.fit() : state.part && view?.fit(state.part.bounds);
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
    if (t.matches("[data-cad-param-value]")) {
      const name = /** @type {HTMLElement} */ (t).dataset.cadParamValue ?? "";
      const value = /** @type {HTMLInputElement} */ (t).value.trim();
      if (value) edit([{ op: "param", name, value }]);
    }
  });

  ws.addEventListener("submit", (e) => {
    const form = /** @type {HTMLFormElement} */ (e.target);
    e.preventDefault();
    if (form === els.form) {
      const name = /** @type {HTMLInputElement} */ (form.querySelector("input")).value.trim();
      if (name) host.dispatch({ type: "cadNew", name });
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
    const typing = t.closest("input, textarea, select");
    if (state.part && e.metaKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      sendText();
      host.dispatch({ type: "cadSave" });
      return;
    }
    if (state.part && e.metaKey && e.key.toLowerCase() === "z" && !typing) {
      e.preventDefault();
      host.dispatch({ type: e.shiftKey ? "cadRedo" : "cadUndo", generation: generation() });
      return;
    }
    if (typing || !state.part) return;
    if (state.sketching != null && sketcher.key(e)) {
      e.preventDefault();
      e.stopPropagation();
      ws.querySelectorAll("[data-cad-tool]").forEach((b) => b.setAttribute("aria-pressed", String(/** @type {HTMLElement} */ (b).dataset.cadTool === sketcher.debug().tool)));
      return;
    }
    if (state.sketching == null && !e.metaKey && !e.ctrlKey && e.key.toLowerCase() === "f") {
      e.preventDefault();
      view?.fit(state.part.bounds);
    }
  });

  window.addEventListener("resize", () => {
    if (state.sketching != null) sketcher.resize();
    view?.resize();
  });

  render();

  return {
    /** @param {{parts: Found[]}} e */
    home(e) {
      state.parts = e.parts;
      renderList();
    },
    /** @param {{part: PartView | null}} e */
    part(e) {
      const was = state.part?.generation;
      state.part = e.part;
      if (!e.part) {
        state.step = null;
        state.sketching = null;
        state.face = null;
        sketcher.leave();
      } else {
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
      if (state.part) requestAnimationFrame(() => {
        view?.resize();
        if (state.sketching != null) sketcher.resize();
      });
    },
    debug() {
      return { part: state.part, step: state.step, sketching: state.sketching, face: state.face, tab: state.tab, noView: state.noView, sketch: sketcher.debug(), view: view?.debug() ?? null, sketcher };
    },
  };
}

/* CAD's assemblies in the window (docs/CAD.md, K7): the instances and
 * mates listed, the chosen one's settings, two faces picked in the view to
 * mate, the fit check and the bill of materials. As with a part, every
 * change is a line of the file sent to the core, which solves the mates
 * and writes back where everything went; nothing is solved here.
 *
 * An assembly's items are its lines, so an item's index is its line less
 * one: what is not in the view — a placement's line, a ground's — is found
 * in the text. */

import { escapeHtml as esc, icon } from "./html.js";

/** @typedef {import("./bindings/CadAssemblyView").CadAssemblyView} AssemblyView */
/** @typedef {import("./bindings/CadInstance").CadInstance} Instance */
/** @typedef {import("./bindings/CadMate").CadMate} Mate */
/** @typedef {import("./bindings/CadFound").CadFound} Found */

/** The mates, as the window offers them: word, label, what it takes. */
export const MATES = [
  { kind: "planar", label: "Planar", what: "two flat faces against each other", value: "offset" },
  { kind: "flush", label: "Flush", what: "two flat faces in one plane", value: "offset" },
  { kind: "distance", label: "Distance", what: "two flat faces facing, a gap apart", value: "=" },
  { kind: "concentric", label: "Concentric", what: "two round faces on one axis", value: "" },
  { kind: "revolute", label: "Revolute", what: "on one axis and centred: turns", value: "offset" },
  { kind: "slider", label: "Slider", what: "on one axis, kept from turning: slides", value: "offset" },
  { kind: "angle", label: "Angle", what: "faces or axes at an angle", value: "=" },
  { kind: "parallel", label: "Parallel", what: "faces or axes parallel", value: "" },
  { kind: "perpendicular", label: "Perpendicular", what: "faces or axes square", value: "" },
  { kind: "fastened", label: "Fastened", what: "the two instances held as they are", value: "" },
];

/** A number as the file writes it: to a nanometre, as briefly as it reads. @param {number} x */
export const num = (x) => {
  const r = Math.round(x * 1e9) / 1e9;
  return String(Object.is(r, -0) ? 0 : r);
};

/** The lines of the `.assembly` language the window writes. */
export const asmText = {
  /** @param {string} name @param {string} path @param {"part" | "assembly"} [kind] */
  instance: (name, path, kind = "part") => `${kind} ${name} from "${path.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`,
  /** @param {string} name @param {number[]} at @param {number[]} [turn] */
  place: (name, at, turn = [0, 0, 0]) => `place ${name} at (${at.map(num).join(", ")})${turn.some((a) => num(a) !== "0") ? ` turn (${turn.map(num).join(", ")})` : ""}`,
  /** @param {string} name */
  ground: (name) => `ground ${name}`,
  /** @param {string} name @param {string} kind @param {string} a @param {string} b @param {string} [value] */
  mate: (name, kind, a, b, value = "") => {
    const m = MATES.find((x) => x.kind === kind);
    const v = value.trim();
    if (m?.value === "=") return `mate ${name} ${kind} ${a} to ${b} = ${v || (kind === "angle" ? "90" : "0")}`;
    if (m?.value === "offset" && v && v !== "0") return `mate ${name} ${kind} ${a} to ${b} offset ${v}`;
    return `mate ${name} ${kind} ${a} to ${b}`;
  },
};

/** The path from the assembly's folder to a file of the project, both as
 *  the project names them. @param {string} from @param {string} to */
export function relPath(from, to) {
  const dir = from.includes("/") ? from.slice(0, from.lastIndexOf("/")).split("/") : [];
  const parts = to.split("/");
  let k = 0;
  while (k < dir.length && k < parts.length - 1 && dir[k] === parts[k]) k++;
  return [...dir.slice(k).map(() => ".."), ...parts.slice(k)].join("/");
}

/** The item index of a line of the text that starts so, or -1.
 *  @param {string} text @param {string} start */
export function lineIndex(text, start) {
  return text.split("\n").findIndex((l) => l.trim().startsWith(start));
}

/** How far each body moves in an exploded view: bodies are named for
 *  their parts, `loop.pump`, moved with their instance, `loop`.
 *  @param {AssemblyView} a @param {string | null} name @returns {Record<string, number[]>} */
export function offsets(a, name) {
  const e = a.explodes.find((x) => x.name === name);
  /** @type {Record<string, number[]>} */
  const out = {};
  if (!e) return out;
  for (const b of a.bodies) {
    const top = a.instances.find((i) => b.name === i.name || b.name.startsWith(`${i.name}.`));
    const mv = top && e.moves.find((m) => m.instance === top.name);
    if (mv) out[b.name] = mv.by;
  }
  return out;
}

/** What an instance's state is, in a word. @param {Instance} i */
const held = (i) => (i.ground ? "held still" : i.free ? "free" : "placed");

/** The steps: instances, mates and views. @param {AssemblyView} a @param {number | null} step */
export function historyHtml(a, step) {
  const row = (/** @type {number} */ index, /** @type {string} */ ic, /** @type {string} */ name, /** @type {string} */ what, /** @type {string} */ cls, /** @type {string} */ title, /** @type {string} */ mark) =>
    `<button class="cd-row${index === step ? " on" : ""}${cls}" data-cad-step="${index}" title="${esc(title)}">${icon(ic)}<span class="cd-row-name">${esc(name)}</span><span class="cd-row-what">${esc(what)}</span>${mark}</button>`;
  const instances = a.instances.map((i) => row(i.index, i.kind === "assembly" ? "cad" : "extrude", i.name, `${i.path} · ${held(i)}`, i.error ? " bad" : "", i.error ?? i.path, i.error ? `<i class="cd-bad" title="${esc(i.error)}">!</i>` : i.ground ? `<i class="cd-later" title="Held still">⚓</i>` : "")).join("");
  const mates = a.mates.map((m) => {
    const bad = m.error || a.conflicting.includes(m.name);
    return row(m.index, "pattern", m.name, `${m.kind} · ${m.a} → ${m.b}`, bad ? " bad" : m.redundant ? " later" : "", m.error ?? (m.redundant ? "It holds nothing the others do not." : m.text), bad ? `<i class="cd-bad" title="${esc(m.error ?? "cannot hold with the others")}">!</i>` : m.redundant ? `<i class="cd-later" title="Redundant">·</i>` : "");
  }).join("");
  const views = a.explodes.map((e) => row(e.index, "mirror", e.name, `exploded · ${e.moves.length} moved`, "", e.text, "")).join("");
  return `<button class="cd-row cd-part-row${step == null ? " on" : ""}" data-cad-step="part">${icon("cad")}<span class="cd-row-name">${esc(a.title ?? a.name)}</span><span class="cd-row-what">${esc(a.units)} · ${a.dof === 0 ? "fully held" : `${a.dof} free`}</span></button>` +
    `<div class="cd-head">Instances</div>${instances || `<p class="xl-empty">Nothing yet: add a part.</p>`}` +
    `<div class="cd-head">Mates</div>${mates || `<p class="xl-empty">Pick a face in the view, then another on a second part, to mate them.</p>`}` +
    (views ? `<div class="cd-head">Views</div>${views}` : "");
}

const field = (/** @type {string} */ label, /** @type {string} */ control) => `<label class="cd-field"><span>${esc(label)}</span>${control}</label>`;
const triple = (/** @type {string} */ key, /** @type {number[]} */ v, /** @type {string} */ label) =>
  `<span class="cd-triple">${v.map((x, k) => `<input data-cad-a-f="${key}${k}" value="${esc(num(x))}" aria-label="${esc(label)} ${"xyz"[k]}">`).join("")}</span>`;

/** The settings of what is chosen, or of the assembly.
 *  @param {AssemblyView} a @param {{step: number | null, picks: string[], exploded: string | null, found: Found[], size: string}} st */
export function settingsHtml(a, st) {
  let html = "";
  if (st.picks.length) {
    const [x, y] = st.picks;
    html += `<section class="cd-card" data-cad-picks><div class="cd-head">${st.picks.length === 2 ? "Mate these" : "Picked"}</div><ul class="cd-picks">${st.picks.map((p) => `<li>${esc(p)}</li>`).join("")}</ul>` +
      (y
        ? field("Value", `<input data-cad-mate-value value="${esc(st.size)}" placeholder="offset, gap or angle" aria-label="A mate's value">`) +
          `<div class="cd-mate-kinds">${MATES.map((m) => `<button class="cd-btn" data-cad-mate-kind="${m.kind}" title="${esc(m.what)}">${esc(m.label)}</button>`).join("")}</div>`
        : `<p class="cd-muted">Now a face of another part: ${esc(x.split(".")[0])}'s is picked.</p>`) +
      `<div class="cd-row-btns"><button class="cd-btn quiet" data-cad-unpick>Clear</button></div></section>`;
  }
  const chosenInstance = a.instances.find((i) => i.index === st.step);
  const chosenMate = a.mates.find((m) => m.index === st.step);
  const chosenView = a.explodes.find((e) => e.index === st.step);
  if (chosenInstance) {
    const i = chosenInstance;
    html += `<section class="cd-card"><div class="cd-head">${i.kind === "assembly" ? "Assembly" : "Part"}</div>` +
      field("Name", `<input data-cad-a-f="name" value="${esc(i.name)}" aria-label="Name">`) +
      field("File", `<span class="cd-ro">${esc(i.path)}</span>`) +
      `<label class="cd-check"><input type="checkbox" data-cad-a-f="ground"${i.ground ? " checked" : ""}> Held still</label>` +
      field("At", triple("at", i.at, "At")) + field("Turned", triple("turn", i.turn, "Turned")) +
      (i.error ? `<p class="cd-error">${esc(i.error)}</p>` : `<p class="cd-muted">${i.ground ? "Held still: the others are placed against it." : i.free ? "Something still lets it move: mate it more to hold it." : "Its mates hold it."} Where it is, the solver writes here.</p>`) +
      `<div class="cd-row-btns"><button class="cd-btn quiet" data-cad-remove="${i.index}">Delete</button></div></section>`;
  } else if (chosenMate) {
    const m = chosenMate;
    const kinds = MATES.map((k) => `<option value="${k.kind}"${k.kind === m.kind ? " selected" : ""}>${esc(k.label)}</option>`).join("");
    html += `<section class="cd-card"><div class="cd-head">Mate</div>` +
      field("Name", `<input data-cad-a-f="name" value="${esc(m.name)}" aria-label="Name">`) +
      field("Kind", `<select data-cad-a-f="kind" aria-label="Kind">${kinds}</select>`) +
      field("This", `<input data-cad-a-f="a" value="${esc(m.a)}" aria-label="What is mated">`) +
      field("To", `<input data-cad-a-f="b" value="${esc(m.b)}" aria-label="What it is mated to">`) +
      field("Value", `<input data-cad-a-f="value" value="${esc(m.value ?? "")}" placeholder="none" aria-label="Value">`) +
      (m.error ? `<p class="cd-error">${esc(m.error)}</p>` : a.conflicting.includes(m.name) ? `<p class="cd-error">It cannot hold with ${esc(a.conflicting.filter((c) => c !== m.name).join(", "))}.</p>` : m.redundant ? `<p class="cd-muted">It holds nothing the others do not.</p>` : `<p class="cd-muted">Holds, to ${m.residual.toExponential(1)}.</p>`) +
      `<div class="cd-row-btns"><button class="cd-btn quiet" data-cad-remove="${m.index}">Delete</button></div></section>`;
  } else if (chosenView) {
    const e = chosenView;
    html += `<section class="cd-card"><div class="cd-head">Exploded view</div><textarea class="cd-line" data-cad-a-f="line" rows="3" spellcheck="false" aria-label="The view's line">${esc(e.text)}</textarea>` +
      `<div class="cd-row-btns"><button class="cd-btn" data-cad-explode="${esc(e.name)}">${st.exploded === e.name ? "Put back together" : "Show it apart"}</button><button class="cd-btn quiet" data-cad-remove="${e.index}">Delete</button></div></section>`;
  } else {
    const opts = st.found.filter((f) => f.path !== a.path).map((f) => `<option value="${esc(f.path)}" data-kind="${f.kind}">${esc(f.path)}</option>`).join("");
    const free = a.instances.filter((i) => i.free).map((i) => i.name);
    const status = !a.solved
      ? `<p class="cd-error">The mates cannot all hold: ${esc(a.conflicting.join(", "))}.</p>`
      : `<p class="cd-muted">${a.dof === 0 ? "Fully held." : `${a.dof} freedom${a.dof === 1 ? "" : "s"} left: ${esc(free.join(", "))} can still move.`}</p>`;
    const bom = a.bom.map((r) => `<tr><td>${r.count}</td><td title="${esc(r.path)}">${esc(r.part)}</td><td>${esc(r.material ?? "")}</td></tr>`).join("");
    const clashes = a.clashes == null ? `<p class="cd-muted">Not checked since the last change.</p>` : !a.clashes.length ? `<p class="cd-muted">Everything fits${a.clearance ? `, ${num(a.clearance)} mm apart or touching` : ""}.</p>` :
      `<ul class="cd-clashes">${a.clashes.map((c) => `<li class="${c.overlap ? "bad" : ""}">${esc(c.a)} · ${esc(c.b)}: ${c.overlap ? (c.volume == null ? "overlap" : `overlap ${c.volume.toFixed(2)} mm³`) : `${c.gap.toFixed(3)} mm apart`}</li>`).join("")}</ul>`;
    html += `<section class="cd-card"><div class="cd-head">Assembly</div>` + field("Name", `<span class="cd-ro">${esc(a.title ?? a.name)}</span>`) + field("Units", `<span class="cd-ro">${esc(a.units)}</span>`) + status + `</section>` +
      `<section class="cd-card"><div class="cd-head">Add</div><form class="cd-inline" data-cad-add-instance><select name="path" aria-label="The file">${opts}</select><input name="name" placeholder="name" aria-label="Instance name"><button class="cd-btn">Add</button></form>` +
      (opts ? "" : `<p class="cd-muted">No parts in the folder yet: make one on the home page.</p>`) + `</section>` +
      `<section class="cd-card"><div class="cd-head">Fit</div>${clashes}<div class="cd-row-btns"><button class="cd-btn" data-cad-fit-check${a.instances.length > 1 ? "" : " disabled"}>Check fit</button></div></section>` +
      `<section class="cd-card"><div class="cd-head">Bill of materials</div>${bom ? `<table class="cd-bom"><tr><th>#</th><th>Part</th><th>Material</th></tr>${bom}</table>` : `<p class="cd-muted">No parts yet.</p>`}` +
      `<div class="cd-row-btns"><button class="cd-btn" data-cad-export="step"${a.bodies.length ? "" : " disabled"}>Write STEP</button><button class="cd-btn" data-cad-export="stl"${a.bodies.length ? "" : " disabled"}>Write STL</button><button class="cd-btn" data-cad-export="3mf"${a.bodies.length ? "" : " disabled"}>Write 3MF</button></div></section>`;
  }
  return html;
}

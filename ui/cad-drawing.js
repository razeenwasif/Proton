// @ts-check
/** A drawing open in CAD (docs/CAD.md, K10): its statements listed, the
 *  settings of the one chosen or of the sheet, lines of the language
 *  written for what is added, and where a view goes when it is dragged. */

import { escapeHtml as esc, icon } from "./html.js";

/** @typedef {import("./bindings/CadDrawingView").CadDrawingView} DrawingView */
/** @typedef {import("./bindings/CadDrawingItem").CadDrawingItem} DrawingItem */
/** @typedef {import("./bindings/CadSheetBox").CadSheetBox} SheetBox */

export const SHEETS = ["A4", "A3", "A2", "A1", "A0", "ANSI_A", "ANSI_B", "ANSI_C", "ANSI_D"];
export const DIRS = ["front", "back", "top", "bottom", "left", "right", "iso"];
export const DIMS = ["distance", "diameter", "radius", "angle", "clearance"];
const SCALES = ["5", "2", "1", "1/2", "1/4", "1/5", "1/10", "1/20"];

/** @param {number} x */
const num = (x) => String(+x.toFixed(1));

/** Lines of the language, as the drawing's text has them. */
export const drawText = {
  /** @param {string} name @param {string} dir @param {number} x @param {number} y */
  view: (name, dir, x, y) => `view ${name} ${dir} at (${num(x)}, ${num(y)})`,
  /** @param {string} name @param {string} axis @param {string} value @param {number} x @param {number} y */
  section: (name, axis, value, x, y) => `section ${name} along ${axis} = ${value} at (${num(x)}, ${num(y)})`,
  /** @param {string} name @param {string} view @param {string} kind @param {string} a @param {string} b */
  dim: (name, view, kind, a, b) => `dim ${name} ${view} ${kind} ${a}${b ? ` to ${b}` : ""}`,
  /** @param {string} name @param {string} view @param {string} text @param {string} at */
  note: (name, view, text, at) => `note ${name} ${view} "${text.replace(/"/g, "'")}" at ${at}`,
};

/** What a box is moved by: a view by its middle, `at`; a detail's circle
 *  by `place`; the bill of materials by its top right, `at`.
 *  @param {DrawingItem} it @param {number} dx @param {number} dy
 *  @returns {string | null} the item's line moved, or null if it is not moved so */
export function moved(it, dx, dy) {
  const word = it.kind === "detail" ? "place" : "at";
  const re = new RegExp(`\\b${word} \\(\\s*(-?[\\d.]+)\\s*,\\s*(-?[\\d.]+)\\s*\\)`);
  const m = it.text.match(re);
  if (!m) return null;
  return it.text.replace(re, `${word} (${num(Number(m[1]) + dx)}, ${num(Number(m[2]) + dy)})`);
}

/** The item a box on the sheet is, if one is. @param {DrawingView} d @param {string} name */
export function itemOfBox(d, name) {
  if (name === "bom") return d.items.find((i) => i.kind === "bom") ?? null;
  return d.items.find((i) => i.name === name && ["view", "section", "detail"].includes(i.kind)) ?? null;
}

/** The box on the sheet under a point, millimetres from the bottom left;
 *  the smallest, so a detail inside a view is found first.
 *  @param {DrawingView} d @param {number} x @param {number} y */
export function boxAt(d, x, y) {
  const hits = d.boxes.filter((b) => b.name !== "title" && x >= b.x0 - 2 && x <= b.x1 + 2 && y >= b.y0 - 2 && y <= b.y1 + 2);
  hits.sort((a, b) => (a.x1 - a.x0) * (a.y1 - a.y0) - (b.x1 - b.x0) * (b.y1 - b.y0));
  return hits[0] ?? null;
}

/** A name not taken. @param {string} base @param {string[]} taken */
export function freshName(base, taken) {
  for (let i = 1; ; i++) if (!taken.includes(`${base}${i}`)) return `${base}${i}`;
}

const ICON = /** @type {Record<string, string>} */ ({ view: "eye", section: "layers", detail: "search", dim: "columns", note: "text", balloons: "target", bom: "table" });

/** The drawing's statements, by what they are.
 *  @param {DrawingView} d @param {number | null} step */
export function drawingHistoryHtml(d, step) {
  const row = (/** @type {DrawingItem} */ it) => {
    const [name, what] = it.kind === "balloons" ? ["balloons", `in ${it.name}`] : it.kind === "bom" ? ["parts list", it.text.split(" ").slice(1).join(" ")] : [it.name, it.text.split(" ").slice(2).join(" ")];
    return `<button class="cd-row${it.index === step ? " on" : ""}${it.error ? " bad" : ""}" data-cad-step="${it.index}" title="${esc(it.error ?? it.text)}">${icon(ICON[it.kind] ?? "file")}<span class="cd-row-name">${esc(name)}</span><span class="cd-row-what">${esc(what)}</span>${it.error ? `<i class="cd-bad" title="${esc(it.error)}">!</i>` : ""}</button>`;
  };
  const of = (/** @type {string[]} */ kinds) => d.items.filter((i) => kinds.includes(i.kind)).map(row).join("");
  const views = of(["view", "section", "detail"]);
  const marks = of(["dim", "note"]);
  const tables = of(["balloons", "bom"]);
  return `<button class="cd-row cd-part-row${step == null ? " on" : ""}" data-cad-step="part">${icon("frame")}<span class="cd-row-name">${esc(d.title ?? d.name)}</span><span class="cd-row-what">${esc(d.sheet.replace("_", " "))}${d.blueprint ? " · blueprint" : ""}</span></button>` +
    `<div class="cd-head">Views</div>${views || `<p class="xl-empty">No views yet: add one.</p>`}` +
    `<div class="cd-head">Dimensions and notes</div>${marks || `<p class="xl-empty">None yet: add a dimension.</p>`}` +
    (tables ? `<div class="cd-head">Parts list</div>${tables}` : "");
}

const field = (/** @type {string} */ label, /** @type {string} */ control) => `<label class="cd-field"><span>${esc(label)}</span>${control}</label>`;
const opts = (/** @type {string[]} */ xs, /** @type {string | null | undefined} */ on) => xs.map((x) => `<option value="${esc(x)}"${x === on ? " selected" : ""}>${esc(x.replace("_", " "))}</option>`).join("");

/** The settings of the statement chosen, or of the sheet.
 *  @param {DrawingView} d @param {{step: number | null, scale: string | null}} st */
export function drawingSettingsHtml(d, st) {
  const it = d.items.find((x) => x.index === st.step);
  const lists = `<datalist id="cd-drw-faces">${d.faces.slice(0, 2000).map((f) => `<option value="${esc(f)}">`).join("")}</datalist><datalist id="cd-drw-parts">${d.parts.map((f) => `<option value="${esc(f)}">`).join("")}</datalist>`;
  if (it) {
    return `<section class="cd-card"><div class="cd-head">${esc(it.kind)} ${esc(it.name)}</div>` +
      (it.error ? `<p class="cd-bad-text">${esc(it.error)}</p>` : "") +
      field("As written", `<input data-cad-drw-line="${it.index}" value="${esc(it.text)}" spellcheck="false" aria-label="The statement">`) +
      (["view", "section", "detail"].includes(it.kind) || it.kind === "bom" ? `<p class="cd-muted">Drag it on the sheet to move it.</p>` : "") +
      `<div class="cd-row-btns"><button class="cd-btn" data-cad-remove="${it.index}">Remove</button></div></section>` + lists;
  }
  const views = d.items.filter((i) => ["view", "section", "detail"].includes(i.kind)).map((i) => i.name);
  const html =
    `<section class="cd-card"><div class="cd-head">Sheet</div>` +
    field("Size", `<select data-cad-drw-sheet aria-label="The sheet's size">${opts(SHEETS, d.sheet)}</select>`) +
    field("Scale", `<select data-cad-drw-scale aria-label="The drawing's scale">${opts(SCALES.includes(st.scale ?? "") ? SCALES : [...SCALES, st.scale ?? "1"], st.scale ?? "1")}</select>`) +
    field("Look", `<select data-cad-drw-style aria-label="Paper or blueprint"><option value="paper"${d.blueprint ? "" : " selected"}>Paper — black on white</option><option value="blueprint"${d.blueprint ? " selected" : ""}>Blueprint — white on blue</option></select>`) +
    `</section>` +
    `<form class="cd-card" data-cad-drw-view-form><div class="cd-head">Add a view</div>` +
    field("Looking", `<select name="dir" aria-label="Which way the view looks">${opts(DIRS, "front")}</select>`) +
    `<div class="cd-row-btns"><button class="cd-btn" type="submit">Add view</button><button class="cd-btn" type="button" data-cad-drw-section>Add section</button></div></form>` +
    `<form class="cd-card" data-cad-drw-dim-form><div class="cd-head">Add a dimension</div>` +
    field("In view", `<select name="view" aria-label="The view it is drawn in">${opts(views, views[0])}</select>`) +
    field("Measure", `<select name="kind" aria-label="What it measures">${opts(DIMS, "distance")}</select>`) +
    field("Of", `<input name="a" list="cd-drw-faces" placeholder="a face or hole" spellcheck="false" aria-label="What it measures">`) +
    field("To", `<input name="b" list="cd-drw-faces" placeholder="another, for a distance or angle" spellcheck="false" aria-label="What it measures to">`) +
    `<div class="cd-row-btns"><button class="cd-btn" type="submit">Add dimension</button></div></form>` +
    `<form class="cd-card" data-cad-drw-note-form><div class="cd-head">Add a note</div>` +
    field("Says", `<input name="text" placeholder="M3 tapped through" aria-label="What the note says">`) +
    field("At", `<input name="at" list="cd-drw-faces" placeholder="a face or hole" spellcheck="false" aria-label="What the note points at">`) +
    `<div class="cd-row-btns"><button class="cd-btn" type="submit">Add note</button></div></form>` +
    `<section class="cd-card"><div class="cd-head">Write it</div><p class="cd-muted">Beside the drawing: PDF to print or send, SVG for the web, DXF for a laser, a router or another CAD program.</p>` +
    `<div class="cd-row-btns"><button class="cd-btn" data-cad-export="pdf">Write PDF</button><button class="cd-btn" data-cad-export="svg">Write SVG</button><button class="cd-btn" data-cad-export="dxf">Write DXF</button></div></section>`;
  return html + lists;
}

/** The scale the drawing says, as written: `1/2`. @param {string} text */
export function scaleOf(text) {
  const m = text.match(/^scale\s+(.+)$/m);
  return m ? m[1].replace(/\s*\/\s*/, "/").trim() : null;
}

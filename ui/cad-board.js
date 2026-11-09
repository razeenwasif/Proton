// @ts-check
/** A board open in CAD (docs/CAD.md, E2): its parts, nets and copper
 *  listed with what the design rules found; the settings of the one chosen
 *  or the board's, with the track tool's net, layer and width; a part
 *  moved when dragged; and the lines a track or a via drawn writes. */

import { escapeHtml as esc, icon } from "./html.js";

/** @typedef {import("./bindings/CadBoardView").CadBoardView} BoardView */
/** @typedef {import("./bindings/CadDrawingItem").CadDrawingItem} Item */

/** @param {number} x */
const num = (x) => String(+x.toFixed(3));

export const LAYERS = ["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"];

/** The board's layers it has. @param {BoardView} b */
export const layersOf = (b) => (b.layers === 4 ? LAYERS : ["F.Cu", "B.Cu"]);

/** A point on the board, mm: snapped to a pad within a millimetre, else to
 *  a quarter-millimetre grid. @param {BoardView} b @param {number} x @param {number} y */
export function snap(b, x, y) {
  let best = null, d = 1.0;
  for (const p of b.pads) {
    const e = Math.hypot(p.x - x, p.y - y);
    if (e < d) {
      d = e;
      best = p;
    }
  }
  if (best) return { x: best.x, y: best.y, pad: best };
  return { x: Math.round(x * 4) / 4, y: Math.round(y * 4) / 4, pad: null };
}

/** A track's line. @param {string} net @param {string} layer @param {number | null} width @param {number[][]} points */
export function trackLine(net, layer, width, points) {
  return `track ${net} ${layer}${width ? ` width ${num(width)}` : ""} ${points.map((p) => `(${num(p[0])}, ${num(p[1])})`).join(" ")}`;
}

/** A via's line. @param {string} net @param {number} x @param {number} y */
export const viaLine = (net, x, y) => `via ${net} at (${num(x)}, ${num(y)})`;

/** The statement a box on the board is: a part's place.
 *  @param {BoardView} b @param {string} name */
export function boardItemOfBox(b, name) {
  return b.items.find((i) => i.kind === "place" && i.name === name) ?? null;
}

/** A place line moved. @param {BoardView} b @param {Item} it @param {number} dx @param {number} dy */
export function boardMoved(b, it, dx, dy) {
  const part = b.parts.find((p) => p.reference === it.name);
  if (!part) return null;
  const re = /\bat \(\s*-?[\d.]+\s*,\s*-?[\d.]+\s*\)/;
  return it.text.replace(re, `at (${+(part.at[0] + dx).toFixed(2)}, ${+(part.at[1] + dy).toFixed(2)})`);
}

/** Parts, nets, and copper, each with what the rules found.
 *  @param {BoardView} b @param {number | null} step */
export function boardHistoryHtml(b, step) {
  const warned = (/** @type {Item} */ it) => (it.error ? null : b.findings.find((f) => !f.error && f.line === it.line)?.message ?? null);
  const row = (/** @type {Item} */ it, /** @type {string} */ ic, /** @type {string} */ name, /** @type {string} */ what) => {
    const w = warned(it);
    const mark = it.error ? `<i class="cd-bad" title="${esc(it.error)}">!</i>` : w ? `<i class="cd-later" title="${esc(w)}">·</i>` : "";
    return `<button class="cd-row${it.index === step ? " on" : ""}${it.error ? " bad" : w ? " later" : ""}" data-cad-step="${it.index}" title="${esc(it.error ?? w ?? it.text)}">${icon(ic)}<span class="cd-row-name">${esc(name)}</span><span class="cd-row-what">${esc(what)}</span>${mark}</button>`;
  };
  const parts = b.items.filter((i) => i.kind === "place").map((it) => {
    const p = b.parts.find((x) => x.reference === it.name);
    return row(it, "node", it.name, p ? `${p.footprint ?? "no footprint"}${p.back ? " · back" : ""}${p.turn ? ` · ${p.turn}°` : ""}` : it.text);
  }).join("");
  const copper = b.items.filter((i) => ["track", "via", "zone"].includes(i.kind)).map((it) => row(it, it.kind === "zone" ? "layers" : it.kind === "via" ? "target" : "link", `${it.kind} ${it.name}`, it.text.split(" ").slice(2, 4).join(" "))).join("");
  const nets = b.nets.map((n) => `<div class="cd-row cd-net${n.unrouted ? " later" : ""}" title="${esc(n.name)}">${icon("link")}<span class="cd-row-name">${esc(n.name)}</span><span class="cd-row-what">${n.unrouted ? `${n.unrouted} to route` : "routed"}${n.needs ? ` · ≥ ${(Math.ceil(n.needs[0] * 100) / 100).toFixed(2)} mm` : ""}</span></div>`).join("");
  const errors = b.findings.filter((f) => f.error).length;
  const left = b.nets.reduce((s, n) => s + n.unrouted, 0);
  return `<button class="cd-row cd-part-row${step == null ? " on" : ""}" data-cad-step="part">${icon("node")}<span class="cd-row-name">${esc(b.title ?? b.name)}</span><span class="cd-row-what">${b.layers} layers · ${errors ? `${errors} error${errors === 1 ? "" : "s"}` : left ? `${left} to route` : "all routed"}</span></button>` +
    `<div class="cd-head">Parts</div>${parts || `<p class="xl-empty">No parts placed yet.</p>`}` +
    `<div class="cd-head">Nets</div>${nets || `<p class="xl-empty">The schematic has no nets.</p>`}` +
    `<div class="cd-head">Copper</div>${copper || `<p class="xl-empty">No tracks yet: pick Track, then click from pad to pad.</p>`}`;
}

const field = (/** @type {string} */ label, /** @type {string} */ control) => `<label class="cd-field"><span>${esc(label)}</span>${control}</label>`;

/** The statement chosen's settings, or the board's: its rules check, the
 *  track tool's, and it written out.
 *  @param {BoardView} b @param {{step: number | null, tool: string | null, net: string, layer: string, width: string}} st */
export function boardSettingsHtml(b, st) {
  const it = b.items.find((x) => x.index === st.step);
  if (it) {
    return `<section class="cd-card"><div class="cd-head">${esc(it.kind)} ${esc(it.name)}</div>` +
      (it.error ? `<p class="cd-bad-text">${esc(it.error)}</p>` : "") +
      field("As written", `<input data-cad-drw-line="${it.index}" value="${esc(it.text)}" spellcheck="false" aria-label="The statement">`) +
      (it.kind === "place" ? `<p class="cd-muted">Drag it on the board to move it.</p>` : "") +
      `<div class="cd-row-btns"><button class="cd-btn" data-cad-remove="${it.index}">Remove</button></div></section>`;
  }
  const findings = b.findings.map((f) => `<li class="${f.error ? "bad" : ""}"><span>line ${f.line}</span>${esc(f.message)}</li>`).join("");
  const nets = b.nets.map((n) => `<option value="${esc(n.name)}"${n.name === st.net ? " selected" : ""}>${esc(n.name)}${n.needs ? ` (≥ ${(Math.ceil(n.needs[0] * 100) / 100).toFixed(2)} mm)` : ""}</option>`).join("");
  const tool = (/** @type {string} */ t, /** @type {string} */ label) => `<button class="cd-btn${st.tool === t ? " on" : ""}" data-cad-brd-tool="${t}" aria-pressed="${st.tool === t}">${label}</button>`;
  return `<section class="cd-card"><div class="cd-head">Design rules</div>${findings ? `<ul class="cd-findings">${findings}</ul>` : `<p class="cd-muted">Nothing wrong, and every net routed.</p>`}</section>` +
    `<section class="cd-card"><div class="cd-head">Draw copper</div>` +
    `<div class="cd-row-btns">${tool("track", "Track")}${tool("via", "Via")}${st.tool ? tool("", "Done") : ""}</div>` +
    field("Net", `<select data-cad-brd-net aria-label="The net drawn">${nets}</select>`) +
    field("Layer", `<select data-cad-brd-layer aria-label="The layer drawn on">${layersOf(b).map((l) => `<option${l === st.layer ? " selected" : ""}>${l}</option>`).join("")}</select>`) +
    field("Width", `<input data-cad-brd-width value="${esc(st.width)}" placeholder="${b.track}" aria-label="The track's width, mm">`) +
    `<p class="cd-muted">${st.tool === "track" ? "Click from a pad, through each corner, to a pad: it ends there, or double-click to end it anywhere. ⎋ lets it go." : st.tool === "via" ? "Click where the via goes." : "A click on a pad starts a track on its net. Points snap to pads within a millimetre, else to a quarter."}</p></section>` +
    `<section class="cd-card"><div class="cd-head">Write it</div><p class="cd-muted">The board as SVG, from the front, beside it.</p><div class="cd-row-btns"><button class="cd-btn" data-cad-export="svg">Write SVG</button></div></section>`;
}

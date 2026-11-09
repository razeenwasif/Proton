// @ts-check
/** A schematic open in CAD (docs/CAD.md, E0): its parts and nets listed
 *  with what the rules check found, the settings of the one chosen or of
 *  the sheet, the lines of the language written for what is added, and a
 *  part moved when it is dragged on the sheet. */

import { escapeHtml as esc, icon } from "./html.js";
import { SHEETS, freshName } from "./cad-drawing.js";

/** @typedef {import("./bindings/CadSchematicView").CadSchematicView} SchematicView */
/** @typedef {import("./bindings/CadDrawingItem").CadDrawingItem} Item */

/** @param {number} x */
const num = (x) => String(+x.toFixed(1));

/** A reference's letters, by kind. */
const PREFIX = /** @type {Record<string, string>} */ ({
  resistor: "R", capacitor: "C", "capacitor-polarised": "C", inductor: "L", diode: "D", led: "D", zener: "D",
  fuse: "F", switch: "SW", crystal: "Y", npn: "Q", pnp: "Q", nmos: "Q", pmos: "Q", ic: "U",
});

/** A new part's reference: the kind's letters and the first number free.
 *  @param {string} kind @param {string[]} taken */
export function referenceFor(kind, taken) {
  return freshName(PREFIX[kind] ?? "J", taken);
}

/** A `part` line. @param {{reference: string, kind: string, takes?: string, value?: string, footprint?: string, at?: number[] | null}} p */
export function partLine(p) {
  let l = `part ${p.reference} ${p.kind}`;
  if (p.takes?.trim()) l += ` ${p.takes.trim()}`;
  if (p.value?.trim()) l += ` value ${/[\s(),"#]/.test(p.value.trim()) ? `"${p.value.trim().replace(/"/g, "'")}"` : p.value.trim()}`;
  if (p.footprint?.trim()) l += ` footprint ${p.footprint.trim()}`;
  if (p.at) l += ` at (${num(p.at[0])}, ${num(p.at[1])})`;
  return l;
}

/** A `net` line. @param {string} name @param {boolean} power @param {string} pins @param {string} current */
export function netLine(name, power, pins, current) {
  const list = pins.split(/[\s,]+/).filter(Boolean).join(", ");
  return `net ${name}${power ? " power" : ""} ${list}${current.trim() ? ` current ${current.trim()}` : ""}`;
}

/** The statement a box on the sheet is: a part, by its reference.
 *  @param {SchematicView} s @param {string} name */
export function schematicItemOfBox(s, name) {
  return s.items.find((i) => i.kind === "part" && i.name === name) ?? null;
}

/** A part's line, moved: its `at` changed, or given one where it was placed
 *  for it. @param {SchematicView} s @param {Item} it @param {number} dx @param {number} dy */
export function schematicMoved(s, it, dx, dy) {
  const part = s.parts.find((p) => p.reference === it.name);
  if (!part) return null;
  const to = `at (${num(part.at[0] + dx)}, ${num(part.at[1] + dy)})`;
  const re = /\bat \(\s*-?[\d.]+\s*,\s*-?[\d.]+\s*\)/;
  return re.test(it.text) ? it.text.replace(re, to) : `${it.text.replace(/( turn \d+)?( mirror)?$/, (m) => ` ${to}${m}`)}`;
}

/** The parts, nets and unconnected pins, each with its finding.
 *  @param {SchematicView} s @param {number | null} step */
export function schematicHistoryHtml(s, step) {
  const row = (/** @type {Item} */ it, /** @type {string} */ ic, /** @type {string} */ what) => {
    const warned = it.error ? null : s.findings.find((f) => !f.error && f.line === it.line)?.message ?? null;
    const mark = it.error ? `<i class="cd-bad" title="${esc(it.error)}">!</i>` : warned ? `<i class="cd-later" title="${esc(warned)}">·</i>` : "";
    return `<button class="cd-row${it.index === step ? " on" : ""}${it.error ? " bad" : warned ? " later" : ""}" data-cad-step="${it.index}" title="${esc(it.error ?? warned ?? it.text)}">${icon(ic)}<span class="cd-row-name">${esc(it.name)}</span><span class="cd-row-what">${esc(what)}</span>${mark}</button>`;
  };
  const parts = s.items.filter((i) => i.kind === "part").map((it) => {
    const p = s.parts.find((x) => x.reference === it.name);
    return row(it, "node", p ? `${p.title}${p.value ? ` · ${p.value}` : ""}` : it.text);
  }).join("");
  const nets = s.items.filter((i) => i.kind === "net").map((it) => {
    const n = s.nets.find((x) => x.name === it.name);
    return row(it, "link", n ? `${n.pins.length} pin${n.pins.length === 1 ? "" : "s"}${n.power ? " · power" : ""}${n.current != null ? ` · ${n.current} A` : ""}` : it.text);
  }).join("");
  const nc = s.items.filter((i) => i.kind === "nc").map((it) => row(it, "close", it.text.slice(3))).join("");
  const errors = s.findings.filter((f) => f.error).length, warnings = s.findings.length - errors;
  return `<button class="cd-row cd-part-row${step == null ? " on" : ""}" data-cad-step="part">${icon("node")}<span class="cd-row-name">${esc(s.title ?? s.name)}</span><span class="cd-row-what">${esc(s.sheet.replace("_", " "))} · ${errors ? `${errors} error${errors === 1 ? "" : "s"}` : warnings ? `${warnings} warning${warnings === 1 ? "" : "s"}` : "checks clean"}</span></button>` +
    `<div class="cd-head">Parts</div>${parts || `<p class="xl-empty">No parts yet: add one.</p>`}` +
    `<div class="cd-head">Nets</div>${nets || `<p class="xl-empty">No nets yet: a net joins pins.</p>`}` +
    (nc ? `<div class="cd-head">Left unconnected</div>${nc}` : "");
}

const field = (/** @type {string} */ label, /** @type {string} */ control) => `<label class="cd-field"><span>${esc(label)}</span>${control}</label>`;

/** The settings of the statement chosen, or the sheet's: its findings, a
 *  part or net added, and it written out.
 *  @param {SchematicView} s @param {{step: number | null, kind: string}} st */
export function schematicSettingsHtml(s, st) {
  const it = s.items.find((x) => x.index === st.step);
  const pinList = `<datalist id="cd-sch-pins">${s.parts.flatMap((p) => p.pins.map((x) => `<option value="${esc(`${p.reference}.${x.number}`)}">${esc(x.name)}</option>`)).join("")}</datalist>`;
  if (it) {
    let more = "";
    const part = it.kind === "part" ? s.parts.find((p) => p.reference === it.name) : null;
    if (part) {
      more = `<table class="cd-bom cd-pins"><tr><th>Pin</th><th>Name</th><th>Does</th><th>Net</th></tr>${part.pins.map((x) => `<tr><td>${esc(x.number)}</td><td>${esc(x.name)}</td><td>${esc(x.kind)}</td><td>${x.net ? esc(x.net) : x.nc || x.kind === "nc" ? "—" : `<span class="cd-bad-text">none</span>`}</td></tr>`).join("")}</table>` +
        `<p class="cd-muted">${esc(part.title)} — its pinout from ${esc(part.source)}.</p>` + (part.caution ? `<p class="cd-bad-text">${esc(part.caution)}</p>` : "") +
        (part.landSvg
          ? `<div class="cd-head">Footprint ${esc(part.footprint ?? "")}</div><div class="cd-land" data-cad-land>${part.landSvg}</div><p class="cd-muted">${part.pads} pad${part.pads === 1 ? "" : "s"}, from ${esc(part.landSource ?? "")}.</p>` + (part.landCaution ? `<p class="cd-bad-text">${esc(part.landCaution)}</p>` : "")
          : `<p class="cd-muted">No footprint yet: say one, \`footprint 0603\`, \`SOIC-8\`, \`header-1x4\`.</p>`);
    }
    const net = it.kind === "net" ? s.nets.find((n) => n.name === it.name) : null;
    if (net) more = `<p class="cd-muted">Joins ${net.pins.map(esc).join(", ") || "nothing"}${net.power ? ", powered from off the board" : ""}${net.current != null ? `; carries ${net.current} A` : ""}.</p>`;
    return `<section class="cd-card"><div class="cd-head">${esc(it.kind)} ${esc(it.name)}</div>` +
      (it.error ? `<p class="cd-bad-text">${esc(it.error)}</p>` : "") +
      field("As written", `<input data-cad-drw-line="${it.index}" value="${esc(it.text)}" spellcheck="false" aria-label="The statement" list="cd-sch-pins">`) +
      more + (part ? `<p class="cd-muted">Drag it on the sheet to move it.</p>` : "") +
      `<div class="cd-row-btns"><button class="cd-btn" data-cad-remove="${it.index}">Remove</button></div></section>` + pinList;
  }
  const kind = s.kinds.find((k) => k.kind === st.kind) ?? s.kinds[0];
  const findings = s.findings.map((f) => `<li class="${f.error ? "bad" : ""}"><span>line ${f.line}</span>${esc(f.message)}</li>`).join("");
  return `<section class="cd-card"><div class="cd-head">Rules check</div>${findings ? `<ul class="cd-findings">${findings}</ul>` : `<p class="cd-muted">Nothing wrong: every pin is on a net or meant to be left, every input driven, every supply supplied.</p>`}</section>` +
    `<section class="cd-card"><div class="cd-head">Sheet</div>` +
    field("Size", `<select data-cad-sch-sheet aria-label="The sheet's size">${SHEETS.map((x) => `<option value="${esc(x)}"${x === s.sheet ? " selected" : ""}>${esc(x.replace("_", " "))}</option>`).join("")}</select>`) + `</section>` +
    `<form class="cd-card" data-cad-sch-part-form><div class="cd-head">Add a part</div>` +
    field("Kind", `<select name="kind" data-cad-sch-kind aria-label="What it is">${s.kinds.map((k) => `<option value="${esc(k.kind)}"${k.kind === kind?.kind ? " selected" : ""}>${esc(k.title)}</option>`).join("")}</select>`) +
    (kind?.takes ? field(kind.kind === "ic" ? "Name and pins" : "Size", `<input name="takes" placeholder="${esc(kind.takes)}" spellcheck="false" aria-label="${esc(kind.takes)}">`) : "") +
    field("Value", `<input name="value" placeholder="10k, 100n" spellcheck="false" aria-label="Its value">`) +
    field("Footprint", `<input name="footprint" placeholder="0603" spellcheck="false" aria-label="Its footprint">`) +
    `<div class="cd-row-btns"><button class="cd-btn" type="submit">Add part</button></div></form>` +
    `<form class="cd-card" data-cad-sch-net-form><div class="cd-head">Add a net</div>` +
    field("Name", `<input name="name" placeholder="+12V, GND, PWM" spellcheck="false" aria-label="The net's name">`) +
    field("Pins", `<input name="pins" placeholder="P1.13, J1.2" list="cd-sch-pins" spellcheck="false" aria-label="The pins it joins">`) +
    field("Current, A", `<input name="current" placeholder="for its tracks' width" aria-label="The current it carries">`) +
    `<label class="cd-check"><input type="checkbox" name="power"> Powered from off the board — a supply's cable</label>` +
    `<div class="cd-row-btns"><button class="cd-btn" type="submit">Add net</button></div></form>` +
    `<section class="cd-card"><div class="cd-head">Write it</div><p class="cd-muted">Beside the schematic: the sheet as PDF or SVG, the netlist for KiCad, the bill of materials as CSV, and the parts' footprints as a KiCad library.</p>` +
    `<div class="cd-row-btns"><button class="cd-btn" data-cad-export="pdf">Write PDF</button><button class="cd-btn" data-cad-export="svg">Write SVG</button><button class="cd-btn" data-cad-export="net">Netlist</button><button class="cd-btn" data-cad-export="csv">Bill of materials</button><button class="cd-btn" data-cad-export="kicad" title="Each part's footprint, as a KiCad library beside the schematic">KiCad footprints</button></div></section>` + pinList;
}

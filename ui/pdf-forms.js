/* The PDF mode's forms: a field's widgets given inputs over the page —
 * text, check boxes, radio buttons, choices — where the core says they are;
 * what is typed sent to the core as it rests, which fills the field in the
 * PDF and draws its appearance, quietly, so nothing is drawn again under
 * the caret. Fields can be highlighted, and the form flattened. */

import { escapeHtml as esc } from "./html.js";

/** @typedef {import("./bindings/PdfField").PdfField} Field */

/** How long typing rests before it is sent. */
const REST_MS = 400;

/** @param {{dispatch: (command: any) => unknown, editable: () => boolean}} host */
export function createPdfForms(host) {
  /** @type {{fields: Field[], highlight: boolean, viewports: Map<number, any>, slots: Map<number, HTMLElement>, confirming: boolean}} */
  const state = { fields: [], highlight: true, viewports: new Map(), slots: new Map(), confirming: false };
  /** @type {Map<string, ReturnType<typeof setTimeout>>} */
  const resting = new Map();

  /** A widget's box, in the page's CSS pixels. @param {number} page @param {number[]} r */
  function box(page, r) {
    const viewport = state.viewports.get(page);
    if (!viewport) return null;
    const [a, b] = viewport.convertToViewportPoint(r[0], r[1]);
    const [c, d] = viewport.convertToViewportPoint(r[2], r[3]);
    return { x: Math.min(a, c), y: Math.min(b, d), w: Math.abs(c - a), h: Math.abs(d - b) };
  }

  /** The inputs over a drawn page; one being typed in is left as it is. @param {number} page */
  function paint(page) {
    const slot = state.slots.get(page);
    if (!slot?.isConnected) return;
    const typing = /** @type {HTMLElement | null} */ (document.activeElement?.closest?.(".pdf-field") ?? null);
    for (const old of slot.querySelectorAll(".pdf-field")) if (old !== typing) old.remove();
    slot.classList.toggle("pdf-fields-lit", state.highlight);
    const locked = !host.editable();
    for (const f of state.fields) {
      f.widgets.forEach((w, i) => {
        if (w.page !== page || f.kind === "button" || f.kind === "signature") return;
        const key = `${f.name}#${i}`;
        if (typing?.dataset.field === key) return;
        const b = box(page, w.rect);
        if (!b) return;
        const off = locked || f.readOnly ? " disabled" : "";
        const size = Math.max(9, Math.min(16, b.h * (f.multiline ? 0.3 : 0.62)));
        let html;
        if (f.kind === "checkbox" || f.kind === "radio") {
          const on = Boolean(w.state) && f.value === w.state;
          html = `<input class="pdf-field" type="${f.kind}" name="${esc(f.name)}" data-field="${esc(key)}" data-name="${esc(f.name)}" data-state="${esc(w.state ?? "")}"${on ? " checked" : ""}${off} title="${esc(f.name)}">`;
        } else if (f.kind === "combo" || f.kind === "list") {
          html = `<select class="pdf-field" data-field="${esc(key)}" data-name="${esc(f.name)}"${off} title="${esc(f.name)}"><option value=""></option>` +
            f.options.map((o) => `<option${o === f.value ? " selected" : ""}>${esc(o)}</option>`).join("") + `</select>`;
        } else if (f.multiline) {
          html = `<textarea class="pdf-field" data-field="${esc(key)}" data-name="${esc(f.name)}"${off} title="${esc(f.name)}" spellcheck="true">${esc(f.value)}</textarea>`;
        } else {
          html = `<input class="pdf-field" type="text" data-field="${esc(key)}" data-name="${esc(f.name)}" value="${esc(f.value)}"${off} title="${esc(f.name)}${f.required ? " — required" : ""}" spellcheck="false">`;
        }
        const holder = document.createElement("div");
        holder.innerHTML = html;
        const el = /** @type {HTMLElement} */ (holder.firstElementChild);
        Object.assign(el.style, { left: `${b.x}px`, top: `${b.y}px`, width: `${b.w}px`, height: `${b.h}px`, fontSize: `${size}px` });
        if (f.required) el.classList.add("required");
        slot.append(el);
      });
    }
  }

  /** A page drawn: where its points are, and its fields over it. @param {number} page @param {HTMLElement} slot @param {any} viewport */
  function drawn(page, slot, viewport) {
    state.viewports.set(page, viewport);
    state.slots.set(page, slot);
    paint(page);
  }

  /** The fields, as the core lists them now. @param {{tab: number, fields: Field[]}} event */
  function fields(event) {
    state.fields = event.fields;
    state.confirming = false;
    for (const page of state.slots.keys()) paint(page);
  }

  /** Send a field's value now. @param {string} name @param {string} value */
  function send(name, value) {
    clearTimeout(resting.get(name));
    resting.delete(name);
    const f = state.fields.find((x) => x.name === name);
    if (!f || f.value === value) return;
    f.value = value;
    host.dispatch({ type: "pdfSetField", name, value });
  }

  /** What an input says, as the field's value. @param {HTMLElement} el */
  function valueOf(el) {
    if (el instanceof HTMLInputElement && (el.type === "checkbox" || el.type === "radio")) return el.checked ? el.dataset.state ?? "" : "";
    return /** @type {HTMLInputElement} */ (el).value;
  }

  /** Inputs typed into, changed and left: sent when they rest, change or are left. @param {HTMLElement} root */
  function listen(root) {
    root.addEventListener("input", (e) => {
      const el = /** @type {HTMLElement} */ (e.target);
      if (!el.matches?.(".pdf-field") || el.matches("select, [type=checkbox], [type=radio]")) return;
      const name = el.dataset.name ?? "";
      clearTimeout(resting.get(name));
      resting.set(name, setTimeout(() => send(name, valueOf(el)), REST_MS));
    });
    root.addEventListener("change", (e) => {
      const el = /** @type {HTMLElement} */ (e.target);
      if (el.matches?.(".pdf-field")) send(el.dataset.name ?? "", valueOf(el));
    });
    root.addEventListener("focusout", (e) => {
      const el = /** @type {HTMLElement} */ (e.target);
      if (el.matches?.(".pdf-field") && resting.has(el.dataset.name ?? "")) send(el.dataset.name ?? "", valueOf(el));
    });
  }

  /** The tools panel's Form section, when there is a form. */
  function toolsHtml() {
    const count = state.fields.filter((f) => f.kind !== "button" && f.kind !== "signature").length;
    if (!state.fields.length) return "";
    const required = state.fields.filter((f) => f.required && !f.value).length;
    return `<section class="pdf-tool"><h4>Form <span class="lab-faint">${count} field${count === 1 ? "" : "s"}${required ? ` · ${required} required left` : ""}</span></h4>
      <label class="pdf-tool-check"><input type="checkbox" data-pdf-form-do="highlight"${state.highlight ? " checked" : ""}> Highlight the fields</label>
      ${state.confirming
        ? `<p class="lab-faint">Flattened, the fields are drawn into the pages and cannot be filled in again — undo can bring them back until it is closed.</p>
           <div class="pdf-tool-row"><button class="cc-quiet" data-pdf-form-do="keep">Keep the form</button><button class="cc-primary" data-pdf-form-do="flatten">Flatten</button></div>`
        : `<div class="pdf-tool-row"><button class="cc-quiet" data-pdf-form-do="confirm"${host.editable() ? "" : " disabled"}>Flatten the form…</button></div>`}
    </section>`;
  }

  /** A click in the tools panel that is the form's. @param {HTMLElement} t @returns {boolean} whether it was */
  function toolClick(t) {
    const act = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-form-do]"));
    if (!act) return false;
    const what = act.dataset.pdfFormDo;
    if (what === "highlight") {
      state.highlight = /** @type {HTMLInputElement} */ (act).checked;
      for (const page of state.slots.keys()) paint(page);
      return true;
    }
    if (what === "confirm") state.confirming = true;
    if (what === "keep") state.confirming = false;
    if (what === "flatten") {
      state.confirming = false;
      host.dispatch({ type: "pdfFlatten" });
    }
    return true;
  }

  return { drawn, fields, listen, toolsHtml, toolClick, count: () => state.fields.length };
}

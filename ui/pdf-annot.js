/* The PDF mode's comments, in the window: the tools that make them — text
 * marked from a selection, notes, text boxes, ink, shapes — each turned into
 * the page's own points and asked of the core, which writes them into the
 * PDF; the comments it lists drawn round on the page and in the sidebar,
 * one chosen at a time to be edited, recoloured, answered or deleted. */

import { escapeHtml as esc } from "./html.js";

/** @typedef {import("./bindings/PdfComment").PdfComment} Comment */
/** @typedef {import("./bindings/PdfMark").PdfMark} Mark */

/** The tools, as the palette shows them. */
export const TOOLS = /** @type {const} */ ([
  ["select", "Select text, and choose comments", "↖"],
  ["highlight", "Highlight what is selected", "▮"],
  ["underline", "Underline what is selected", "U̲"],
  ["strike", "Strike out what is selected", "S̶"],
  ["note", "A note — click where it goes", "✎"],
  ["text", "A text box — drag where it goes", "T"],
  ["ink", "Draw", "〰"],
  ["square", "A rectangle — drag", "□"],
  ["circle", "An ellipse — drag", "○"],
  ["arrow", "An arrow — drag from its tail", "↗"],
]);

/** The colours offered. */
export const COLORS = ["#ffd400", "#7bd148", "#4fa3ff", "#ff7eb6", "#ff5a4f", "#222222"];

/** What each kind of comment is called, for a person. */
const KIND = /** @type {Record<string, string>} */ ({
  Highlight: "Highlight", Underline: "Underline", StrikeOut: "Strike-out", Squiggly: "Squiggle", Text: "Note",
  FreeText: "Text box", Ink: "Drawing", Square: "Rectangle", Circle: "Ellipse", Line: "Line",
});

/** A PDF date — `D:20261012143000Z` — as a person reads one. @param {string} date */
export function when(date) {
  const m = /^D:(\d{4})(\d{2})?(\d{2})?(\d{2})?(\d{2})?/.exec(date);
  if (!m) return "";
  const at = new Date(Date.UTC(Number(m[1]), Number(m[2] ?? 1) - 1, Number(m[3] ?? 1), Number(m[4] ?? 0), Number(m[5] ?? 0)));
  return at.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

/**
 * @param {{pages: HTMLElement, dispatch: (command: any) => unknown, editable: () => boolean,
 *   goTo: (page: number, offset: number) => void}} host
 */
export function createPdfAnnot(host) {
  const { pages } = host;
  /** @type {{tool: string, color: string, comments: Comment[], chosen: string | null,
   *   viewports: Map<number, any>, slots: Map<number, HTMLElement>}} */
  const state = { tool: "select", color: COLORS[0], comments: [], chosen: null, viewports: new Map(), slots: new Map() };
  /** A drag under way: its page, where it began, and the points so far (in CSS pixels of the page). */
  let drag = /** @type {{page: number, slot: HTMLElement, points: number[][], shape: SVGElement} | null} */ (null);
  /** The popover a comment is edited in, or a new note or text box written in. */
  let pop = /** @type {HTMLElement | null} */ (null);
  /** The bar over a text selection. */
  let selbar = /** @type {HTMLElement | null} */ (null);

  /** A point on a page, in its CSS pixels, as the PDF's own points. @param {number} page @param {number} x @param {number} y */
  function toPdf(page, x, y) {
    const viewport = state.viewports.get(page);
    return viewport ? viewport.convertToPdfPoint(x, y) : [x, y];
  }

  /** A box in the PDF's points as one in the page's CSS pixels. @param {number} page @param {number[]} r */
  function toPage(page, r) {
    const viewport = state.viewports.get(page);
    if (!viewport) return null;
    const [a, b] = viewport.convertToViewportPoint(r[0], r[1]);
    const [c, d] = viewport.convertToViewportPoint(r[2], r[3]);
    return { x: Math.min(a, c), y: Math.min(b, d), w: Math.abs(c - a), h: Math.abs(d - b) };
  }

  /** The comments of a drawn page, ringed where they are; the chosen one more strongly. @param {number} page */
  function paint(page) {
    const slot = state.slots.get(page);
    if (!slot?.isConnected) return;
    for (const old of slot.querySelectorAll(".pdf-comment-box")) old.remove();
    for (const c of state.comments) {
      if (c.page !== page || c.replyTo) continue;
      const box = toPage(page, c.rect);
      if (!box) continue;
      const el = document.createElement("div");
      el.className = `pdf-comment-box${c.id === state.chosen ? " on" : ""}`;
      el.dataset.comment = c.id;
      Object.assign(el.style, { left: `${box.x - 2}px`, top: `${box.y - 2}px`, width: `${box.w + 4}px`, height: `${box.h + 4}px` });
      slot.append(el);
    }
  }

  /** A page drawn: where its points are, and its comments over it. @param {number} page @param {HTMLElement} slot @param {any} viewport */
  function drawn(page, slot, viewport) {
    state.viewports.set(page, viewport);
    state.slots.set(page, slot);
    paint(page);
  }

  /** The comments, as the core lists them now. @param {{tab: number, comments: Comment[]}} event */
  function comments(event) {
    state.comments = event.comments;
    if (state.chosen && !state.comments.some((c) => c.id === state.chosen)) close();
    for (const page of state.slots.keys()) paint(page);
  }

  /** The slot a point on screen is over, and where on it. @param {number} x @param {number} y */
  function slotAt(x, y) {
    for (const [page, slot] of state.slots) {
      if (!slot.isConnected || slot.hidden) continue;
      const r = slot.getBoundingClientRect();
      if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return { page, slot, x: x - r.left, y: y - r.top };
    }
    return null;
  }

  /* ── marking selected text ───────────────────────────────────────── */

  /** The selection's boxes, page by page, in the PDF's points. */
  function selected() {
    const selection = document.getSelection();
    if (!selection || selection.isCollapsed || !selection.rangeCount) return null;
    const range = selection.getRangeAt(0);
    if (!pages.contains(range.commonAncestorContainer)) return null;
    /** @type {Map<number, number[][]>} */
    const byPage = new Map();
    const seen = new Set();
    for (const r of range.getClientRects()) {
      if (r.width < 1 || r.height < 1) continue;
      const at = slotAt(r.left + r.width / 2, r.top + r.height / 2);
      if (!at) continue;
      const left = r.left - at.slot.getBoundingClientRect().left, top = r.top - at.slot.getBoundingClientRect().top;
      const key = `${at.page}:${Math.round(left)}:${Math.round(top)}:${Math.round(r.width)}`;
      if (seen.has(key)) continue;
      seen.add(key);
      const [x1, y1] = toPdf(at.page, left, top + r.height);
      const [x2, y2] = toPdf(at.page, left + r.width, top);
      const boxes = byPage.get(at.page) ?? [];
      boxes.push([x1, y1, x2, y2]);
      byPage.set(at.page, boxes);
    }
    return byPage.size ? byPage : null;
  }

  /** Mark what is selected as `kind`, on every page it runs across. @param {string} kind */
  function markSelection(kind) {
    const byPage = selected();
    if (!byPage || !host.editable()) return;
    for (const [page, boxes] of byPage) {
      host.dispatch({ type: "pdfComment", page, mark: { kind, boxes, rect: null, strokes: [], color: state.color, text: "" } });
    }
    document.getSelection()?.removeAllRanges();
    hideSelbar();
  }

  function hideSelbar() {
    selbar?.remove();
    selbar = null;
  }

  /** The bar over a selection: mark it, or copy it. */
  function showSelbar() {
    hideSelbar();
    const selection = document.getSelection();
    if (!selection || selection.isCollapsed || !selection.rangeCount || !host.editable()) return;
    const rect = selection.getRangeAt(0).getBoundingClientRect();
    const box = pages.getBoundingClientRect();
    selbar = document.createElement("div");
    selbar.className = "pdf-selbar";
    selbar.innerHTML = `<button data-pdf-mark="highlight" title="Highlight">Highlight</button><button data-pdf-mark="underline" title="Underline">Underline</button>` +
      `<button data-pdf-mark="strike" title="Strike out">Strike</button>`;
    selbar.style.left = `${Math.max(4, rect.left - box.left + pages.scrollLeft)}px`;
    selbar.style.top = `${Math.max(4, rect.top - box.top + pages.scrollTop - 34)}px`;
    pages.append(selbar);
  }

  /* ── comments chosen, and written ───────────────────────────────── */

  function close() {
    pop?.remove();
    pop = null;
    const was = state.chosen;
    state.chosen = null;
    if (was) for (const page of state.slots.keys()) paint(page);
  }

  /** A popover beside a point on a page. @param {HTMLElement} slot @param {number} x @param {number} y @param {string} html */
  function popover(slot, x, y, html) {
    close();
    pop = document.createElement("div");
    pop.className = "pdf-pop";
    pop.innerHTML = html;
    pop.style.left = `${slot.offsetLeft + Math.min(x, slot.offsetWidth - 260)}px`;
    pop.style.top = `${slot.offsetTop + y + 8}px`;
    pages.append(pop);
    /** @type {HTMLElement | null} */ (pop.querySelector("textarea, input"))?.focus();
    return pop;
  }

  /** The swatches, the one in use marked. @param {string | null | undefined} on */
  const swatches = (on) => COLORS.map((c) => `<button class="pdf-swatch${c === on ? " on" : ""}" data-pdf-swatch="${c}" style="--swatch:${c}" title="${c}"></button>`).join("");

  /** Choose a comment: ringed, and its popover open. @param {string} id */
  function choose(id) {
    const c = state.comments.find((x) => x.id === id);
    if (!c) return;
    const box = toPage(c.page, c.rect);
    const slot = state.slots.get(c.page);
    if (!box || !slot) {
      host.goTo(c.page, 0);
      return;
    }
    const replies = state.comments.filter((r) => r.replyTo === c.id);
    const editable = host.editable();
    popover(slot, box.x, box.y + box.h, `<div class="pdf-pop-head"><b>${esc(KIND[c.kind] ?? c.kind)}</b> <span class="lab-faint">${esc(c.author)} · ${esc(when(c.date))}</span></div>` +
      (editable ? `<textarea data-pdf-pop-text rows="3" placeholder="Say something about it">${esc(c.text)}</textarea><div class="pdf-swatches">${swatches(c.color)}</div>`
        : `<p>${esc(c.text)}</p>`) +
      (replies.length ? `<div class="pdf-replies">${replies.map((r) => `<p><b>${esc(r.author)}</b> ${esc(r.text)}</p>`).join("")}</div>` : "") +
      (editable ? `<input data-pdf-pop-reply placeholder="Reply" spellcheck="true"><div class="pdf-pop-acts"><button class="cc-quiet" data-pdf-pop="delete">Delete</button>` +
        `<span class="grow"></span><button class="cc-quiet" data-pdf-pop="close">Close</button><button class="cc-primary" data-pdf-pop="save">Save</button></div>` : ""));
    if (pop) pop.dataset.comment = id;
    state.chosen = id;
    paint(c.page);
  }

  /** Write a note or a text box, at a point or over a box on a page.
   *  @param {"note" | "text"} kind @param {number} page @param {HTMLElement} slot @param {number[]} rect in PDF points @param {number} x @param {number} y */
  function write(kind, page, slot, rect, x, y) {
    const box = popover(slot, x, y, `<div class="pdf-pop-head"><b>${kind === "note" ? "Note" : "Text box"}</b></div>` +
      `<textarea data-pdf-pop-text rows="3" placeholder="${kind === "note" ? "The note" : "The text"}  ⌘⏎"></textarea>` +
      `<div class="pdf-pop-acts"><span class="grow"></span><button class="cc-quiet" data-pdf-pop="close">Cancel</button><button class="cc-primary" data-pdf-pop="add">Add</button></div>`);
    box.dataset.adding = JSON.stringify({ kind, page, rect });
  }

  /** What the popover was for, done. */
  function commit() {
    if (!pop) return;
    const text = /** @type {HTMLTextAreaElement | null} */ (pop.querySelector("[data-pdf-pop-text]"))?.value ?? "";
    if (pop.dataset.adding) {
      const { kind, page, rect } = JSON.parse(pop.dataset.adding);
      if (text.trim()) host.dispatch({ type: "pdfComment", page, mark: { kind, boxes: [], rect, strokes: [], color: state.color, text } });
      close();
      return;
    }
    const id = pop.dataset.comment;
    const c = state.comments.find((x) => x.id === id);
    if (!c || !id) return close();
    const reply = /** @type {HTMLInputElement | null} */ (pop.querySelector("[data-pdf-pop-reply]"))?.value.trim();
    if (text !== c.text) host.dispatch({ type: "pdfEditComment", id, text });
    if (reply) host.dispatch({ type: "pdfReply", id, text: reply });
    close();
  }

  pages.addEventListener("click", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    const mark = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-mark]"));
    if (mark) return markSelection(mark.dataset.pdfMark ?? "highlight");
    const swatch = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-swatch]"));
    if (swatch && pop?.dataset.comment) {
      host.dispatch({ type: "pdfEditComment", id: pop.dataset.comment, color: swatch.dataset.pdfSwatch });
      for (const s of pop.querySelectorAll(".pdf-swatch")) s.classList.toggle("on", s === swatch);
      return;
    }
    const act = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-pop]"));
    if (act) {
      const what = act.dataset.pdfPop;
      if (what === "close") return close();
      if (what === "delete" && pop?.dataset.comment) {
        host.dispatch({ type: "pdfDeleteComment", id: pop.dataset.comment });
        return close();
      }
      return commit();
    }
    if (t.closest(".pdf-pop, .pdf-selbar")) return;
    // A click on the page: a comment there chosen, or a note put there.
    const at = slotAt(e.clientX, e.clientY);
    if (!at) return;
    if (state.tool === "note" && host.editable()) {
      const [x, y] = toPdf(at.page, at.x, at.y);
      return write("note", at.page, at.slot, [x, y, 0, 0], at.x, at.y);
    }
    if (state.tool !== "select" || !document.getSelection()?.isCollapsed) return;
    const [x, y] = toPdf(at.page, at.x, at.y);
    const hit = state.comments.filter((c) => c.page === at.page && !c.replyTo && x >= c.rect[0] - 2 && x <= c.rect[2] + 2 && y >= c.rect[1] - 2 && y <= c.rect[3] + 2)
      .sort((a, b) => (a.rect[2] - a.rect[0]) * (a.rect[3] - a.rect[1]) - (b.rect[2] - b.rect[0]) * (b.rect[3] - b.rect[1]))[0];
    if (hit) choose(hit.id);
    else if (pop) close();
  });

  // A selection made: marked at once with a marking tool, or offered a bar.
  pages.addEventListener("mouseup", () => {
    setTimeout(() => {
      if (["highlight", "underline", "strike"].includes(state.tool)) markSelection(state.tool);
      else if (state.tool === "select") showSelbar();
    }, 0);
  });
  document.addEventListener("selectionchange", () => {
    if (selbar && document.getSelection()?.isCollapsed) hideSelbar();
  });

  /* ── drawing: ink, shapes, text boxes ───────────────────────────── */

  const DRAWS = ["ink", "square", "circle", "arrow", "text"];

  pages.addEventListener("pointerdown", (e) => {
    if (!DRAWS.includes(state.tool) || e.button !== 0 || !host.editable()) return;
    if (/** @type {HTMLElement} */ (e.target).closest(".pdf-pop, .pdf-selbar")) return;
    const at = slotAt(e.clientX, e.clientY);
    if (!at) return;
    e.preventDefault();
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.classList.add("pdf-drawing");
    svg.setAttribute("width", String(at.slot.offsetWidth));
    svg.setAttribute("height", String(at.slot.offsetHeight));
    const shape = document.createElementNS("http://www.w3.org/2000/svg", state.tool === "ink" ? "polyline" : state.tool === "circle" ? "ellipse" : state.tool === "arrow" ? "line" : "rect");
    shape.setAttribute("stroke", state.color);
    svg.append(shape);
    at.slot.append(svg);
    drag = { page: at.page, slot: at.slot, points: [[at.x, at.y]], shape };
    pages.setPointerCapture?.(e.pointerId);
  });

  pages.addEventListener("pointermove", (e) => {
    if (!drag) return;
    const r = drag.slot.getBoundingClientRect();
    const x = Math.max(0, Math.min(r.width, e.clientX - r.left)), y = Math.max(0, Math.min(r.height, e.clientY - r.top));
    const [x0, y0] = drag.points[0];
    const s = drag.shape;
    if (state.tool === "ink") {
      drag.points.push([x, y]);
      s.setAttribute("points", drag.points.map((p) => p.join(",")).join(" "));
    } else {
      drag.points[1] = [x, y];
      if (state.tool === "arrow") Object.entries({ x1: x0, y1: y0, x2: x, y2: y }).forEach(([k, v]) => s.setAttribute(k, String(v)));
      else if (state.tool === "circle") Object.entries({ cx: (x0 + x) / 2, cy: (y0 + y) / 2, rx: Math.abs(x - x0) / 2, ry: Math.abs(y - y0) / 2 }).forEach(([k, v]) => s.setAttribute(k, String(v)));
      else Object.entries({ x: Math.min(x0, x), y: Math.min(y0, y), width: Math.abs(x - x0), height: Math.abs(y - y0) }).forEach(([k, v]) => s.setAttribute(k, String(v)));
    }
  });

  pages.addEventListener("pointerup", () => {
    if (!drag) return;
    const { page, slot, points } = drag;
    slot.querySelector(".pdf-drawing")?.remove();
    drag = null;
    const pdf = points.map(([x, y]) => toPdf(page, x, y));
    if (state.tool === "ink") {
      if (points.length > 1) host.dispatch({ type: "pdfComment", page, mark: { kind: "ink", boxes: [], rect: null, strokes: [pdf.map(([x, y]) => [x, y])], color: state.color, text: "" } });
      return;
    }
    const end = pdf[1];
    if (!end || Math.hypot(points[1][0] - points[0][0], points[1][1] - points[0][1]) < 6) return;
    const rect = [pdf[0][0], pdf[0][1], end[0], end[1]];
    if (state.tool === "text") return write("text", page, slot, rect, Math.min(points[0][0], points[1][0]), Math.max(points[0][1], points[1][1]));
    host.dispatch({ type: "pdfComment", page, mark: { kind: state.tool, boxes: [], rect, strokes: [], color: state.color, text: "" } });
  });

  pages.addEventListener("keydown", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    if (!t.closest(".pdf-pop")) return;
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      close();
      pages.focus();
    } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey || t.matches("[data-pdf-pop-reply]"))) {
      e.preventDefault();
      commit();
    }
  });

  /** The tool in hand. @param {string} tool */
  function tool(tool) {
    state.tool = tool;
    pages.dataset.tool = tool;
    hideSelbar();
  }

  /** The colour new comments are. @param {string} color */
  function color(color) {
    state.color = color;
  }

  /** The comments, for the sidebar: each with its page and what it says, its replies after it. */
  function listHtml() {
    const top = state.comments.filter((c) => !c.replyTo);
    if (!top.length) return `<p class="lab-faint">No comments. Select some text to mark it, or choose a tool above the pages.</p>`;
    return top.map((c) => {
      const replies = state.comments.filter((r) => r.replyTo === c.id);
      return `<button type="button" class="pdf-comment-row${c.id === state.chosen ? " on" : ""}" data-pdf-comment="${esc(c.id)}">` +
        `<span class="pdf-comment-dot" style="--swatch:${esc(c.color ?? "#999")}"></span><span class="pdf-comment-what"><b>${esc(KIND[c.kind] ?? c.kind)}</b> · p. ${c.page}` +
        `${c.text ? `<span class="pdf-comment-text">${esc(c.text)}</span>` : ""}` +
        `${replies.map((r) => `<span class="pdf-comment-reply">↳ ${esc(r.author)}: ${esc(r.text)}</span>`).join("")}` +
        `<span class="lab-faint">${esc(c.author)} · ${esc(when(c.date))}</span></span></button>`;
    }).join("");
  }

  /** A comment gone to from the sidebar, and chosen. @param {string} id */
  function show(id) {
    const c = state.comments.find((x) => x.id === id);
    if (!c) return;
    const viewport = state.viewports.get(c.page);
    const top = viewport ? Math.max(0, viewport.convertToViewportPoint(c.rect[0], c.rect[3])[1] / viewport.scale - 40) : 0;
    host.goTo(c.page, top);
    requestAnimationFrame(() => choose(id));
  }

  /** Keys while a comment is chosen: ⌫ deletes it, Escape lets it go. @param {KeyboardEvent} e */
  function key(e) {
    if (e.key === "Escape" && (pop || state.tool !== "select")) {
      close();
      if (state.tool !== "select") tool("select");
      return true;
    }
    if ((e.key === "Backspace" || e.key === "Delete") && state.chosen && host.editable()) {
      host.dispatch({ type: "pdfDeleteComment", id: state.chosen });
      close();
      return true;
    }
    return false;
  }

  tool("select");
  return { drawn, comments, tool, color, listHtml, show, key, chosen: () => state.chosen, count: () => state.comments.filter((c) => !c.replyTo).length };
}

/* The PDF mode's page grid: every page small, chosen with a click — ⌘ to
 * add or take one away, ⇧ for all between — and dragged to where it goes.
 * What is done to the pages chosen is the core's: turned, deleted, a blank
 * put after, moved. This draws the grid and says which pages. */

import { escapeHtml as esc } from "./html.js";
import { drawSmall } from "./pdf-view.js";

/** How wide a page is drawn in the grid. */
const WIDE = 140;

/**
 * @param {{box: HTMLElement, dispatch: (command: any) => unknown, doc: () => any, turned: () => number,
 *   labels: () => string[] | null, count: () => number, onRead: (page: number) => void, onChange: () => void}} host
 */
export function createPdfGrid(host) {
  const { box } = host;
  /** The pages chosen, and the one a ⇧-click runs from. */
  let chosen = new Set();
  let anchor = 0;
  /** Pages drawn small, for the document on screen. @type {Map<number, HTMLCanvasElement>} */
  let drawn = new Map();
  /** Where a drag would put the pages: before this page. */
  let dropBefore = 0;

  const selection = () => [...chosen].sort((a, b) => a - b);

  /** The grid's own bar: what can be done to the pages chosen. */
  function barHtml() {
    const n = chosen.size;
    const what = n ? `${n} page${n === 1 ? "" : "s"} chosen` : "Choose pages: click, ⌘-click, ⇧-click";
    const off = n ? "" : " disabled";
    return `<div class="pdf-grid-bar"><span class="lab-faint">${what}</span><span class="grow"></span>` +
      `<button class="hbtn" data-pdf-grid-do="left"${off} title="Turn left">⟲</button>` +
      `<button class="hbtn" data-pdf-grid-do="right"${off} title="Turn right">⟳</button>` +
      `<button class="hbtn" data-pdf-grid-do="blank"${off} title="A blank page after">+ Blank</button>` +
      `<button class="hbtn" data-pdf-grid-do="delete"${off} title="Delete  ⌫">Delete</button>` +
      `<button class="hbtn" data-pdf-grid-do="all" title="Choose every page  ⌘A">All</button>` +
      `<button class="hbtn" data-pdf-grid-do="read" title="Back to reading  Esc">Done</button></div>`;
  }

  /** The grid, each page a cell; drawn small as it comes into view. */
  function render() {
    const labels = host.labels();
    const count = host.count();
    for (const n of [...chosen]) if (n > count) chosen.delete(n);
    box.innerHTML = barHtml() + `<div class="pdf-grid-cells">${Array.from({ length: count }, (_, i) => {
      const n = i + 1;
      return `<div class="pdf-cell${chosen.has(n) ? " on" : ""}" draggable="true" data-pdf-cell="${n}" role="option" aria-selected="${chosen.has(n)}" tabindex="-1">` +
        `<span class="pdf-cell-pic"></span><span class="pdf-cell-label">${esc(labels?.[i] ?? String(n))}</span></div>`;
    }).join("")}</div>`;
    const seen = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        seen.unobserve(entry.target);
        void picture(/** @type {HTMLElement} */ (entry.target));
      }
    }, { root: box, rootMargin: "300px 0px" });
    for (const cell of box.querySelectorAll(".pdf-cell")) seen.observe(cell);
  }

  /** A page drawn small into its cell. @param {HTMLElement} cell */
  async function picture(cell) {
    const n = Number(cell.dataset.pdfCell);
    const pic = /** @type {HTMLElement} */ (cell.querySelector(".pdf-cell-pic"));
    const kept = drawn.get(n);
    if (kept) return void pic.replaceChildren(kept);
    const doc = host.doc();
    if (!doc) return;
    const mine = drawn;
    try {
      const canvas = await drawSmall(doc, n, WIDE, host.turned());
      if (mine !== drawn) return;
      drawn.set(n, canvas);
      pic.replaceChildren(canvas);
    } catch {
      // A page that cannot be drawn small is still a cell to choose.
    }
  }

  /** The cells shown as chosen, and the bar saying so. */
  function marks() {
    for (const cell of box.querySelectorAll(".pdf-cell")) {
      const on = chosen.has(Number(/** @type {HTMLElement} */ (cell).dataset.pdfCell));
      cell.classList.toggle("on", on);
      cell.setAttribute("aria-selected", String(on));
    }
    const bar = box.querySelector(".pdf-grid-bar");
    if (bar) bar.outerHTML = barHtml();
    host.onChange();
  }

  /** Do something to the pages chosen. @param {string} what */
  function act(what) {
    const pages = selection();
    if (what === "all") {
      chosen = new Set(Array.from({ length: host.count() }, (_, i) => i + 1));
      return marks();
    }
    if (what === "read") return host.onRead(pages[0] ?? 1);
    if (!pages.length) return;
    if (what === "left" || what === "right") host.dispatch({ type: "pdfRotate", pages, by: what === "left" ? -90 : 90 });
    if (what === "blank") host.dispatch({ type: "pdfInsertBlank", after: pages[pages.length - 1], count: 1 });
    if (what === "delete") {
      host.dispatch({ type: "pdfDelete", pages });
      chosen.clear();
    }
  }

  box.addEventListener("click", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    const button = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-grid-do]"));
    if (button) return act(button.dataset.pdfGridDo ?? "");
    const cell = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-cell]"));
    if (!cell) return;
    const n = Number(cell.dataset.pdfCell);
    if (e.shiftKey && anchor) {
      const [from, to] = anchor < n ? [anchor, n] : [n, anchor];
      if (!(e.metaKey || e.ctrlKey)) chosen.clear();
      for (let i = from; i <= to; i++) chosen.add(i);
    } else if (e.metaKey || e.ctrlKey) {
      if (chosen.has(n)) chosen.delete(n);
      else chosen.add(n);
      anchor = n;
    } else {
      chosen = new Set([n]);
      anchor = n;
    }
    marks();
  });

  box.addEventListener("dblclick", (e) => {
    const cell = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-pdf-cell]"));
    if (cell) host.onRead(Number(cell.dataset.pdfCell));
  });

  // Dragged: the pages chosen — or the one dragged, if it is not among them —
  // go before the cell dropped on, or after it when dropped on its far half.
  box.addEventListener("dragstart", (e) => {
    const cell = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-pdf-cell]"));
    if (!cell) return;
    const n = Number(cell.dataset.pdfCell);
    if (!chosen.has(n)) {
      chosen = new Set([n]);
      anchor = n;
      marks();
    }
    e.dataTransfer?.setData("text/plain", selection().join(","));
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  });
  box.addEventListener("dragover", (e) => {
    const cell = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-pdf-cell]"));
    if (!cell) return;
    e.preventDefault();
    const rect = cell.getBoundingClientRect();
    const n = Number(cell.dataset.pdfCell);
    dropBefore = e.clientX > rect.left + rect.width / 2 ? n + 1 : n;
    for (const c of box.querySelectorAll(".pdf-cell.drop-before, .pdf-cell.drop-after")) c.classList.remove("drop-before", "drop-after");
    cell.classList.add(dropBefore > n ? "drop-after" : "drop-before");
  });
  box.addEventListener("dragleave", (e) => {
    if (!box.contains(/** @type {Node | null} */ (e.relatedTarget))) {
      for (const c of box.querySelectorAll(".pdf-cell.drop-before, .pdf-cell.drop-after")) c.classList.remove("drop-before", "drop-after");
    }
  });
  box.addEventListener("drop", (e) => {
    e.preventDefault();
    for (const c of box.querySelectorAll(".pdf-cell.drop-before, .pdf-cell.drop-after")) c.classList.remove("drop-before", "drop-after");
    const pages = selection();
    if (!pages.length || !dropBefore) return;
    // Dropped where they already are: nothing moves.
    const first = pages[0], last = pages[pages.length - 1];
    if (pages.length === last - first + 1 && dropBefore >= first && dropBefore <= last + 1) return;
    host.dispatch({ type: "pdfMove", pages, before: dropBefore });
    chosen.clear();
  });

  /** The grid's own keys: ⌫ deletes, ⌘A chooses all, Escape reads.
   *  @param {KeyboardEvent} e @returns {boolean} */
  function key(e) {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "a") return (act("all"), true);
    if (e.key === "Backspace" || e.key === "Delete") return (act("delete"), true);
    if (e.key === "Escape") return (act("read"), true);
    return false;
  }

  return {
    render,
    key,
    selection,
    /** A new version of the document: drawn again, its choice kept where it can be. */
    refresh() {
      drawn = new Map();
      render();
    },
    /** Choose `pages`. @param {number[]} pages */
    choose(pages) {
      chosen = new Set(pages);
      anchor = pages[0] ?? 0;
      marks();
    },
  };
}

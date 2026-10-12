/* The PDF mode: the person's PDFs in tabs, drawn by pdf.js from bytes the
 * core holds and names by a key (Event::PdfView), and an Open box for the
 * next one — a path typed, the project's PDFs, those opened before.
 *
 * The core owns every tab and every change; this draws what it says, and
 * asks it for what the person does. */

import { readDocument } from "./bridge.js";
import { escapeHtml as esc } from "./html.js";
import { createPdfView } from "./pdf-view.js";

/** @typedef {import("./bindings/PdfTab").PdfTab} Tab */

/** How many PDFs the Open box lists. */
const LISTED = 60;

/** @param {{root: Document, dispatch: (command: any) => unknown}} host */
export function createPdf(host) {
  const found = /** @type {HTMLElement | null} */ (host.root.querySelector('.workspace[data-ws="pdf"]'));
  if (!found) return null;
  const section = found;
  const q = (/** @type {string} */ sel) => /** @type {HTMLElement} */ (section.querySelector(sel));
  const pages = q("[data-pdf-pages]");
  const openBox = q("[data-pdf-open]");
  const pathInput = /** @type {HTMLInputElement} */ (q("[data-pdf-open-path]"));
  /** @type {{tabs: Tab[], active: number | null, recent: string[], project: string[], root: string,
   *   shown: {tab: number, key: string} | null, opening: boolean, at: number}} */
  const state = { tabs: [], active: null, recent: [], project: [], root: "", shown: null, opening: false, at: 0 };

  const view = createPdfView(pages, {
    onWhere: (page, count, scale) => {
      q("[data-pdf-page]").textContent = count ? `${page} / ${count} · ${Math.round(scale * 100)}%` : "";
    },
  });

  /** The tabs, the one on top marked, each a click from forward and closing. */
  function renderTabs() {
    q("[data-pdf-tabs]").innerHTML = state.tabs.map((t) =>
      `<div class="pdf-tab${t.id === state.active ? " on" : ""}" role="tab" aria-selected="${t.id === state.active}" data-pdf-tab="${t.id}" title="${esc(t.path)}">` +
      `<span class="pdf-tab-name">${esc(t.name)}</span>${t.dirty ? '<span class="dot" title="Not saved"></span>' : ""}` +
      `<button class="pdf-tab-close" data-pdf-close="${t.id}" title="Close" aria-label="Close ${esc(t.name)}">×</button></div>`).join("");
  }

  /** What the Open box offers for what is typed: the project's PDFs that
   *  match it, then those opened before. */
  function choices() {
    const typed = pathInput.value.trim().toLowerCase();
    const words = typed.split(/\s+/).filter(Boolean);
    const fits = (/** @type {string} */ name) => words.every((w) => name.toLowerCase().includes(w));
    const project = state.project.filter(fits).map((p) => ({ path: p, label: p, from: "project" }));
    const recent = state.recent.filter((r) => fits(r) && !project.some((p) => r.endsWith(p.path))).map((r) => ({ path: r, label: r, from: "opened before" }));
    return [...project, ...recent].slice(0, LISTED);
  }

  /** The Open box: shown with nothing open, or when + asks for it. */
  function renderOpen() {
    const showing = state.opening || !state.tabs.length;
    openBox.hidden = !showing;
    pages.hidden = showing || !state.shown;
    if (!showing) return;
    const list = choices();
    state.at = Math.min(state.at, Math.max(0, list.length - 1));
    q("[data-pdf-open-list]").innerHTML = list.length
      ? list.map((c, i) => `<button type="button" class="pdf-open-row${i === state.at ? " on" : ""}" role="option" aria-selected="${i === state.at}" data-pdf-open-pick="${esc(c.path)}">` +
          `<span>${esc(c.label)}</span><span class="lab-faint">${c.from}</span></button>`).join("")
      : `<p class="lab-faint">${pathInput.value.trim() ? "Nothing here matches: Enter opens what is typed." : "No PDFs in this project yet, and none opened before. Type where one is."}</p>`;
  }

  /** Ask the core for a PDF, and put the box away. @param {string} path */
  function open(path) {
    if (!path.trim()) return;
    host.dispatch({ type: "pdfOpen", path });
    state.opening = false;
    pathInput.value = "";
  }

  /** @param {{tabs: Tab[], active: number | null, recent: string[], project: string[], root: string}} event */
  function tabs(event) {
    state.tabs = event.tabs;
    state.active = event.active;
    state.recent = event.recent;
    state.project = event.project;
    state.root = event.root;
    if (!state.tabs.length) {
      state.shown = null;
      view.close?.();
    }
    renderTabs();
    renderOpen();
  }

  /** A tab drawn: read again in place when it is the one already shown.
   *  @param {{tab: number, key: string, version: number, page: number | null, quiet: boolean}} event */
  async function show(event) {
    if (event.quiet && state.shown?.tab === event.tab) {
      state.shown = { tab: event.tab, key: event.key };
      return;
    }
    const same = state.shown?.tab === event.tab;
    state.shown = { tab: event.tab, key: event.key };
    renderOpen();
    let bytes;
    try {
      bytes = await readDocument(event.key);
    } catch (err) {
      view.note(`Could not read this PDF.<br>${esc(String(err))}`);
      return;
    }
    if (state.shown?.key !== event.key) return;
    if (same && view.isOpen()) await view.reload(bytes);
    else await view.show(bytes, { page: event.page ?? undefined });
    if (same && event.page) view.goTo(event.page);
  }

  section.addEventListener("click", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    const close = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-close]"));
    if (close) return void host.dispatch({ type: "pdfClose", tab: Number(close.dataset.pdfClose) });
    const tab = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-tab]"));
    if (tab) {
      state.opening = false;
      host.dispatch({ type: "pdfFocus", tab: Number(tab.dataset.pdfTab) });
      return;
    }
    if (t.closest("[data-pdf-open-box]")) {
      state.opening = !state.opening || !state.tabs.length;
      renderOpen();
      if (!openBox.hidden) pathInput.focus();
      return;
    }
    const pick = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-open-pick]"));
    if (pick) return open(pick.dataset.pdfOpenPick ?? "");
    const zoom = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-zoom]"));
    if (zoom) {
      const by = Number(zoom.dataset.pdfZoom);
      if (by === 0) view.fit();
      else view.zoom(by);
    }
  });

  pathInput.addEventListener("input", () => {
    state.at = 0;
    renderOpen();
  });
  pathInput.addEventListener("keydown", (e) => {
    const list = choices();
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      state.at = Math.max(0, Math.min(list.length - 1, state.at + (e.key === "ArrowDown" ? 1 : -1)));
      renderOpen();
    } else if (e.key === "Enter") {
      // What is chosen in the list, or else what is typed.
      const typed = pathInput.value.trim();
      const chosen = list[state.at];
      open(chosen && (!typed || !/[/~.]/.test(typed[0])) ? chosen.path : typed);
    } else if (e.key === "Escape" && state.tabs.length) {
      state.opening = false;
      renderOpen();
      pages.focus();
    } else {
      return;
    }
    e.preventDefault();
    e.stopPropagation();
  });

  /** The PDF's own keys, when nothing typed into has the keyboard.
   *  @param {KeyboardEvent} e @returns {boolean} whether it was the PDF's */
  function key(e) {
    const target = /** @type {HTMLElement | null} */ (e.target instanceof HTMLElement ? e.target : null);
    if (target?.closest("input, textarea, select, [contenteditable]:not([contenteditable=false])")) return false;
    if (e.metaKey || e.altKey || !state.shown || !pages.isConnected || pages.hidden) return false;
    return view.key(e);
  }

  renderTabs();
  renderOpen();
  return { tabs, show, key, view: () => view };
}

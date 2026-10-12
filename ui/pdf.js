/* The PDF mode: the person's PDFs in tabs, drawn by pdf.js from bytes the
 * core holds and names by a key (Event::PdfView), and an Open box for the
 * next one — a path typed, the project's PDFs, those opened before.
 *
 * The core owns every tab and every change; this draws what it says, and
 * asks it for what the person does. What only changes how the pages are
 * read — their layout, the view turned, reading colours, a search's marks —
 * is the window's own. */

import { readDocument } from "./bridge.js";
import { escapeHtml as esc } from "./html.js";
import { find } from "./pdf-find.js";
import { createPdfView } from "./pdf-view.js";

/** @typedef {import("./bindings/PdfTab").PdfTab} Tab */
/** @typedef {import("./pdf-find.js").Found} Found */

/** How many PDFs the Open box lists. */
const LISTED = 60;
/** How wide a thumbnail is drawn, in CSS pixels. */
const THUMB = 120;
/** Reading colours, in turn: the document's own, then sepia, then night. */
const COLORS = /** @type {const} */ ([null, { background: "#f4ecd8", foreground: "#3b2f1e" }, { background: "#1c1d21", foreground: "#d6d6d6" }]);

/** A per-window preference, kept if it can be. @param {string} key @param {string} fallback */
function pref(key, fallback) {
  try {
    return localStorage.getItem(`proton.pdf.${key}`) ?? fallback;
  } catch {
    return fallback;
  }
}
/** @param {string} key @param {string} value */
function keep(key, value) {
  try {
    localStorage.setItem(`proton.pdf.${key}`, value);
  } catch {
    // Not kept: the next window starts from the default.
  }
}

/** @param {{root: Document, dispatch: (command: any) => unknown}} host */
export function createPdf(host) {
  const found = /** @type {HTMLElement | null} */ (host.root.querySelector('.workspace[data-ws="pdf"]'));
  if (!found) return null;
  const section = found;
  const q = (/** @type {string} */ sel) => /** @type {HTMLElement} */ (section.querySelector(sel));
  const pages = q("[data-pdf-pages]");
  const openBox = q("[data-pdf-open]");
  const pathInput = /** @type {HTMLInputElement} */ (q("[data-pdf-open-path]"));
  const side = q("[data-pdf-side-body]");
  /** @type {{tabs: Tab[], active: number | null, recent: string[], project: string[], root: string,
   *   shown: {tab: number, key: string} | null, opening: boolean, at: number,
   *   side: string, labels: string[] | null, outline: any[] | null, page: number, count: number,
   *   query: string, hits: Found[], hitOn: number, searching: boolean, colors: number}} */
  const state = {
    tabs: [], active: null, recent: [], project: [], root: "", shown: null, opening: false, at: 0,
    side: pref("side", "pages"), labels: null, outline: null, page: 1, count: 0,
    query: "", hits: [], hitOn: -1, searching: false, colors: Number(pref("colors", "0")) || 0,
  };
  /** Which search is the current one, so one overtaken stops. */
  let searchGeneration = 0;
  /** Thumbnails drawn for the document on screen, by page. @type {Map<number, HTMLCanvasElement>} */
  let thumbs = new Map();

  const view = createPdfView(pages, {
    onWhere: (page, count, scale) => {
      state.page = page;
      state.count = count;
      const label = state.labels?.[page - 1];
      q("[data-pdf-page]").textContent = count ? `${label && label !== String(page) ? `${label} · ` : ""}${page} / ${count} · ${Math.round(scale * 100)}%` : "";
      for (const t of side.querySelectorAll(".pdf-thumb.on")) t.classList.remove("on");
      side.querySelector(`.pdf-thumb[data-pdf-thumb="${page}"]`)?.classList.add("on");
    },
    onLink: ({ url }) => void host.dispatch({ type: "openLink", url }),
  });
  view.setColors(COLORS[state.colors] ?? null);

  /* ── tabs and the Open box ───────────────────────────────────────── */

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
    const words = pathInput.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
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

  /* ── the sidebar ─────────────────────────────────────────────────── */

  /** The sidebar's tab, drawn. @param {string} [which] */
  function renderSide(which = state.side) {
    state.side = which;
    keep("side", which);
    for (const b of section.querySelectorAll("[data-pdf-side]")) {
      const on = /** @type {HTMLElement} */ (b).dataset.pdfSide === which;
      b.classList.toggle("on", on);
      b.setAttribute("aria-selected", String(on));
    }
    if (!state.shown) {
      side.innerHTML = `<p class="lab-faint">Open a PDF to see its pages here.</p>`;
      return;
    }
    if (which === "pages") return renderThumbs();
    if (which === "outline") return renderOutline();
    if (which === "search") return renderSearch();
    side.innerHTML = `<p class="lab-faint">Comments are listed here.</p>`;
  }

  /** A slot for each page, drawn small as it comes into the sidebar's view. */
  function renderThumbs() {
    const labels = state.labels;
    side.innerHTML = `<div class="pdf-thumbs">${Array.from({ length: state.count }, (_, i) =>
      `<button type="button" class="pdf-thumb${i + 1 === state.page ? " on" : ""}" data-pdf-thumb="${i + 1}" title="Page ${i + 1}">` +
      `<span class="pdf-thumb-pic"></span><span class="pdf-thumb-label">${esc(labels?.[i] ?? String(i + 1))}</span></button>`).join("")}</div>`;
    const seen = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        seen.unobserve(entry.target);
        void thumb(/** @type {HTMLElement} */ (entry.target));
      }
    }, { root: side, rootMargin: "400px 0px" });
    for (const t of side.querySelectorAll(".pdf-thumb")) seen.observe(t);
  }

  /** A page drawn small into its slot — once per version of the document. @param {HTMLElement} slot */
  async function thumb(slot) {
    const n = Number(slot.dataset.pdfThumb);
    const pic = /** @type {HTMLElement} */ (slot.querySelector(".pdf-thumb-pic"));
    const kept = thumbs.get(n);
    if (kept) return void pic.replaceChildren(kept);
    const doc = view.doc();
    if (!doc) return;
    const mine = thumbs;
    try {
      const page = await doc.getPage(n);
      const at1 = page.getViewport({ scale: 1, rotation: (page.rotate + view.turned()) % 360 });
      const viewport = page.getViewport({ scale: THUMB / at1.width, rotation: (page.rotate + view.turned()) % 360 });
      const ratio = window.devicePixelRatio || 1;
      const canvas = document.createElement("canvas");
      canvas.width = Math.floor(viewport.width * ratio);
      canvas.height = Math.floor(viewport.height * ratio);
      canvas.style.width = `${Math.floor(viewport.width)}px`;
      canvas.style.height = `${Math.floor(viewport.height)}px`;
      await page.render({ canvasContext: canvas.getContext("2d"), viewport, transform: ratio === 1 ? null : [ratio, 0, 0, ratio, 0, 0] }).promise;
      if (mine !== thumbs) return;
      thumbs.set(n, canvas);
      pic.replaceChildren(canvas);
    } catch {
      // Left blank: a page that cannot be drawn small can still be read.
    }
  }

  /** The bookmarks, nested as the document has them. */
  function renderOutline() {
    if (!state.outline?.length) {
      side.innerHTML = `<p class="lab-faint">This PDF has no bookmarks.</p>`;
      return;
    }
    /** @type {any[]} */
    const flat = [];
    const walk = (/** @type {any[]} */ items, /** @type {number} */ depth) => {
      for (const item of items) {
        flat.push({ item, depth });
        if (item.items?.length) walk(item.items, depth + 1);
      }
    };
    walk(state.outline, 0);
    outlineItems = flat.map((f) => f.item);
    side.innerHTML = flat.map(({ item, depth }, i) =>
      `<button type="button" class="pdf-mark-row${depth ? "" : " top"}" style="--depth:${depth}" data-pdf-outline="${i}" title="${esc(item.title)}">${esc(item.title || "(untitled)")}</button>`).join("");
  }
  /** @type {any[]} */
  let outlineItems = [];

  /** The search: its field, and what it found, a line of context each. */
  function renderSearch() {
    side.innerHTML = `<form class="pdf-find" data-pdf-find><input data-pdf-find-query placeholder="Find in this PDF  ⌘F" value="${esc(state.query)}" spellcheck="false" autocomplete="off" aria-label="Find in this PDF"></form>` +
      `<p class="lab-faint pdf-find-said"></p><div class="pdf-hits"></div>`;
    refreshFound();
  }

  /** What the search found, drawn again — its field left alone, caret and all. */
  function refreshFound() {
    if (state.side !== "search") return;
    const said = side.querySelector(".pdf-find-said"), list = side.querySelector(".pdf-hits");
    if (!said || !list) return renderSearch();
    const hits = state.hits;
    said.textContent = state.searching ? "Searching…" : state.query ? (hits.length ? `${hits.length}${hits.length >= 2000 ? "+" : ""} found` : "Not found") : "";
    list.innerHTML = hits.slice(0, 400).map((h, i) =>
      `<button type="button" class="pdf-hit-row${i === state.hitOn ? " on" : ""}" data-pdf-hit="${i}"><span class="pdf-hit-page">${esc(state.labels?.[h.page - 1] ?? String(h.page))}</span>` +
      `<span>…${esc(h.before.slice(-30))}<mark>${esc(h.found)}</mark>${esc(h.after.slice(0, 40))}…</span></button>`).join("");
  }

  /** Search for `query`, marking what is found as it is. @param {string} query */
  async function search(query) {
    const mine = ++searchGeneration;
    state.query = query;
    state.hits = [];
    state.hitOn = -1;
    view.mark([], -1);
    const doc = view.doc();
    if (!query.trim() || !doc) {
      state.searching = false;
      if (state.side === "search") refreshFound();
      return;
    }
    state.searching = true;
    refreshFound();
    await find(doc, query, view.turned(), (hits) => {
      if (mine !== searchGeneration) return;
      state.hits.push(...hits);
      if (state.hitOn < 0 && state.hits.length) state.hitOn = 0;
      view.mark(state.hits, state.hitOn);
      refreshFound();
    }, () => mine !== searchGeneration);
    if (mine !== searchGeneration) return;
    state.searching = false;
    refreshFound();
  }

  /** The next hit, or the one before. @param {number} by */
  function step(by) {
    if (!state.hits.length) return;
    state.hitOn = (state.hitOn + by + state.hits.length) % state.hits.length;
    view.mark(state.hits, state.hitOn);
    refreshFound();
    side.querySelector(".pdf-hit-row.on")?.scrollIntoView({ block: "nearest" });
  }

  /** The search, with the keyboard. */
  function focusFind() {
    if (state.side !== "search") renderSide("search");
    const field = /** @type {HTMLInputElement | null} */ (side.querySelector("[data-pdf-find-query]"));
    field?.focus();
    field?.select();
  }

  /* ── the tab on screen ───────────────────────────────────────────── */

  /** @param {{tabs: Tab[], active: number | null, recent: string[], project: string[], root: string}} event */
  function tabs(event) {
    state.tabs = event.tabs;
    state.active = event.active;
    state.recent = event.recent;
    state.project = event.project;
    state.root = event.root;
    if (!state.tabs.length) {
      state.shown = null;
      view.close();
      renderSide();
    }
    renderTabs();
    renderOpen();
  }

  /** What is read of a document once it is drawn: its labels and bookmarks,
   *  and the sidebar for it. @param {boolean} fresh another document, not a version of this one */
  async function read(fresh) {
    const doc = view.doc();
    if (!doc) return;
    thumbs = new Map();
    if (fresh) {
      searchGeneration++;
      state.query = "";
      state.hits = [];
      state.hitOn = -1;
      state.searching = false;
    }
    const [labels, outline] = await Promise.all([doc.getPageLabels().catch(() => null), doc.getOutline().catch(() => null)]);
    if (view.doc() !== doc) return;
    state.labels = labels;
    state.outline = outline;
    state.count = view.pages();
    renderSide();
    // A search kept across a new version is found again in it.
    if (!fresh && state.query) void search(state.query);
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
    if (state.shown?.key === event.key) await read(!same);
  }

  /* ── what is clicked and typed ───────────────────────────────────── */

  /** Lay the pages out one way or another. @param {string} how */
  function layout(how) {
    view.setLayout(/** @type {any} */ (how));
    keep("layout", how);
    for (const b of section.querySelectorAll("[data-pdf-layout]")) b.setAttribute("aria-pressed", String(/** @type {HTMLElement} */ (b).dataset.pdfLayout === how));
  }
  layout(pref("layout", "continuous"));

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
      return;
    }
    const how = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-layout]"));
    if (how) return layout(how.dataset.pdfLayout ?? "continuous");
    if (t.closest("[data-pdf-rotate]")) {
      view.rotate(90);
      thumbs = new Map();
      if (state.side === "pages") renderThumbs();
      return;
    }
    if (t.closest("[data-pdf-colors]")) {
      state.colors = (state.colors + 1) % COLORS.length;
      keep("colors", String(state.colors));
      view.setColors(COLORS[state.colors] ?? null);
      return;
    }
    const sideTab = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-side]"));
    if (sideTab) return renderSide(sideTab.dataset.pdfSide);
    const thumbAt = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-thumb]"));
    if (thumbAt) return view.goTo(Number(thumbAt.dataset.pdfThumb));
    const mark = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-outline]"));
    if (mark) {
      const item = outlineItems[Number(mark.dataset.pdfOutline)];
      if (item?.url) host.dispatch({ type: "openLink", url: item.url });
      else if (item?.dest) void view.go(item.dest);
      return;
    }
    const hit = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-hit]"));
    if (hit) {
      state.hitOn = Number(hit.dataset.pdfHit);
      view.mark(state.hits, state.hitOn);
      refreshFound();
    }
  });

  /** @type {ReturnType<typeof setTimeout> | undefined} */
  let typing;
  section.addEventListener("input", (e) => {
    const t = /** @type {HTMLInputElement} */ (e.target);
    if (t === pathInput) {
      state.at = 0;
      renderOpen();
    } else if (t.matches("[data-pdf-find-query]")) {
      clearTimeout(typing);
      typing = setTimeout(() => void search(t.value), 160);
    }
  });
  section.addEventListener("submit", (e) => {
    if (!/** @type {HTMLElement} */ (e.target).matches("[data-pdf-find]")) return;
    e.preventDefault();
  });
  section.addEventListener("keydown", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    if (!t.matches("[data-pdf-find-query]")) return;
    if (e.key === "Enter") {
      e.preventDefault();
      // What is typed and not yet searched for is searched for first.
      if (/** @type {HTMLInputElement} */ (t).value !== state.query) void search(/** @type {HTMLInputElement} */ (t).value);
      else step(e.shiftKey ? -1 : 1);
    } else if (e.key === "Escape") {
      e.preventDefault();
      searchGeneration++;
      state.hits = [];
      state.hitOn = -1;
      state.query = "";
      /** @type {HTMLInputElement} */ (t).value = "";
      view.mark([], -1);
      refreshFound();
      pages.focus();
    }
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

  /** The PDF's own keys, when nothing typed into has the keyboard: ⌘F and
   *  ⌘G find, r turns the view, the rest move through the pages.
   *  @param {KeyboardEvent} e @returns {boolean} whether it was the PDF's */
  function key(e) {
    if (!state.shown || pages.hidden) return false;
    if (e.metaKey && !e.altKey && !e.ctrlKey) {
      if (e.key === "f") return (focusFind(), true);
      if (e.key === "g" || e.key === "G") return (step(e.shiftKey ? -1 : 1), true);
      return false;
    }
    const target = /** @type {HTMLElement | null} */ (e.target instanceof HTMLElement ? e.target : null);
    if (target?.closest("input, textarea, select, [contenteditable]:not([contenteditable=false])")) return false;
    if (e.altKey) return false;
    if (e.key === "r" && !e.ctrlKey) {
      view.rotate(90);
      thumbs = new Map();
      if (state.side === "pages") renderThumbs();
      return true;
    }
    if (e.key === "/" && !e.ctrlKey) return (focusFind(), true);
    return view.key(e);
  }

  renderTabs();
  renderOpen();
  renderSide();
  return { tabs, show, key, view: () => view, search, state: () => state };
}

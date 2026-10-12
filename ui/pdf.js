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
import { COLORS as INKS, TOOLS, createPdfAnnot } from "./pdf-annot.js";
import { createPdfForms } from "./pdf-forms.js";
import { createPdfGrid } from "./pdf-pages.js";
import { createPdfTools } from "./pdf-tools.js";
import { createPdfView, drawSmall } from "./pdf-view.js";

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
   *   query: string, hits: Found[], hitOn: number, searching: boolean, colors: number,
   *   ask: {kind: "close", tab: number} | {kind: "saveAs"} | null, grid: boolean}} */
  const state = {
    tabs: [], active: null, recent: [], project: [], root: "", shown: null, opening: false, at: 0,
    side: pref("side", "pages"), labels: null, outline: null, page: 1, count: 0,
    query: "", hits: [], hitOn: -1, searching: false, colors: Number(pref("colors", "0")) || 0,
    ask: null, grid: false,
  };
  /** Which search is the current one, so one overtaken stops. */
  let searchGeneration = 0;
  /** Thumbnails drawn for the document on screen, by page. @type {Map<number, HTMLCanvasElement>} */
  let thumbs = new Map();

  /** The comments' tools and overlays, and the form's inputs, made once the view is. @type {ReturnType<typeof createPdfAnnot> | null} */
  let annot = null;
  /** @type {ReturnType<typeof createPdfForms> | null} */
  let forms = null;
  const view = createPdfView(pages, {
    onDrawn: (page, slot, viewport) => {
      annot?.drawn(page, slot, viewport);
      forms?.drawn(page, slot, viewport);
    },
    onWhere: (page, count, scale) => {
      state.page = page;
      state.count = count;
      const label = state.labels?.[page - 1];
      q("[data-pdf-page]").textContent = count ? `${label && label !== String(page) ? `${label} · ` : ""}${page} / ${count} · ${Math.round(scale * 100)}%` : "";
      for (const t of side.querySelectorAll(".pdf-thumb.on")) t.classList.remove("on");
      side.querySelector(`.pdf-thumb[data-pdf-thumb="${page}"]`)?.classList.add("on");
      tools?.says();
    },
    onLink: ({ url }) => void host.dispatch({ type: "openLink", url }),
  });
  annot = createPdfAnnot({
    pages, dispatch: host.dispatch, editable: () => Boolean(state.shown) && !top()?.readOnly,
    goTo: (page, offset) => view.goTo(page, offset),
  });
  const marks = annot;
  forms = createPdfForms({ dispatch: host.dispatch, editable: () => Boolean(state.shown) && !top()?.readOnly });
  const form = forms;
  form.listen(pages);
  q("[data-pdf-palette]").innerHTML = TOOLS.map(([id, title, glyph]) =>
    `<button data-pdf-tool-pick="${id}" title="${title}" aria-label="${title}" aria-pressed="${id === "select"}">${glyph}</button>`).join("");
  q("[data-pdf-colors-pick]").innerHTML = INKS.map((c, i) =>
    `<button class="pdf-swatch${i ? "" : " on"}" data-pdf-ink="${c}" style="--swatch:${c}" title="${c}" aria-label="Colour ${c}"></button>`).join("");

  /** The tool in hand. @param {string} tool */
  function takeTool(tool) {
    marks.tool(tool);
    for (const b of section.querySelectorAll("[data-pdf-tool-pick]")) b.setAttribute("aria-pressed", String(/** @type {HTMLElement} */ (b).dataset.pdfToolPick === tool));
  }

  const gridBox = q("[data-pdf-grid]");
  const grid = createPdfGrid({
    box: gridBox, dispatch: host.dispatch, doc: () => view.doc(), turned: () => view.turned(),
    labels: () => state.labels, count: () => state.count,
    onRead: (page) => {
      showGrid(false);
      view.goTo(page);
    },
    onChange: () => tools.says(),
  });
  const tools = createPdfTools({
    box: q("[data-pdf-tools]"), dispatch: host.dispatch, tab: () => top(),
    pages: () => (state.grid && grid.selection().length ? grid.selection() : state.shown ? [state.page] : []),
    count: () => state.count, project: () => state.project, extra: () => form.toolsHtml(),
  });
  q("[data-pdf-tools]").addEventListener("click", (e) => {
    if (form.toolClick(/** @type {HTMLElement} */ (e.target))) tools.render();
  }, true);

  /** The page grid, or the pages to read. @param {boolean} on */
  function showGrid(on) {
    state.grid = on && Boolean(state.shown);
    gridBox.hidden = !state.grid;
    q("[data-pdf-grid-toggle]").setAttribute("aria-pressed", String(state.grid));
    if (state.grid) {
      grid.render();
      grid.choose([state.page]);
      gridBox.focus();
    } else {
      pages.focus();
    }
    tools.says();
  }
  view.setColors(COLORS[state.colors] ?? null);

  /* ── tabs and the Open box ───────────────────────────────────────── */

  /** The tab on top. */
  const top = () => state.tabs.find((t) => t.id === state.active) ?? null;

  /** The bar's undo, redo and save, as the tab on top allows; and why it can only be read. */
  function renderBar() {
    const tab = top();
    const undo = /** @type {HTMLButtonElement} */ (q("[data-pdf-undo]")), redo = /** @type {HTMLButtonElement} */ (q("[data-pdf-redo]"));
    undo.disabled = !tab?.undo;
    undo.title = tab?.undo ? `Undo ${tab.undo}  ⌘Z` : "Undo  ⌘Z";
    redo.disabled = !tab?.redo;
    redo.title = tab?.redo ? `Redo ${tab.redo}  ⇧⌘Z` : "Redo  ⇧⌘Z";
    /** @type {HTMLButtonElement} */ (q("[data-pdf-save]")).disabled = !tab?.dirty;
    /** @type {HTMLButtonElement} */ (q("[data-pdf-save-as]")).disabled = !tab || Boolean(tab.readOnly);
    const lock = q("[data-pdf-readonly]");
    lock.hidden = !tab?.readOnly;
    lock.title = tab?.readOnly ? `Read only: ${tab.readOnly}.` : "";
  }

  /** What is being asked: to save before closing, or where to save as. */
  function renderAsk() {
    const box = q("[data-pdf-ask]");
    const ask = state.ask;
    box.hidden = !ask;
    if (!ask) return void (box.innerHTML = "");
    if (ask.kind === "close") {
      const tab = state.tabs.find((t) => t.id === ask.tab);
      if (!tab) return void (state.ask = null, box.hidden = true);
      box.innerHTML = `<span>Save the changes to <b>${esc(tab.name)}</b> before closing it?</span><span class="grow"></span>` +
        `<button class="cc-quiet" data-pdf-ask-do="cancel">Cancel</button><button class="cc-quiet" data-pdf-ask-do="discard">Close without saving</button>` +
        `<button class="cc-primary" data-pdf-ask-do="save">Save and close</button>`;
    } else {
      const tab = top();
      box.innerHTML = `<form class="pdf-save-as" data-pdf-save-as-form><label>Save as <input data-pdf-save-as-path value="${esc(tab?.path ?? "")}" spellcheck="false" autocomplete="off"></label>` +
        `<button type="button" class="cc-quiet" data-pdf-ask-do="cancel">Cancel</button><button type="submit" class="cc-primary">Save</button></form>`;
      const field = /** @type {HTMLInputElement} */ (box.querySelector("[data-pdf-save-as-path]"));
      field.focus();
      // The name chosen, not the folder: what is usually changed.
      const slash = field.value.lastIndexOf("/");
      field.setSelectionRange(slash + 1, field.value.replace(/\.pdf$/i, "").length);
    }
  }

  /** Close a tab: one with changes asks first. @param {number} id */
  function closeTab(id) {
    const tab = state.tabs.find((t) => t.id === id);
    if (tab?.dirty) {
      state.ask = { kind: "close", tab: id };
      return renderAsk();
    }
    host.dispatch({ type: "pdfClose", tab: id });
  }

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
    side.innerHTML = `<div class="pdf-comments">${marks.listHtml()}</div>`;
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
      const canvas = await drawSmall(doc, n, THUMB, view.turned());
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
    if (state.ask?.kind === "close" && !state.tabs.some((t) => t.id === /** @type {any} */ (state.ask).tab)) state.ask = null;
    renderTabs();
    renderBar();
    renderOpen();
    if (state.ask?.kind === "close") renderAsk();
    if (!state.tabs.length && state.grid) showGrid(false);
    tools.render();
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
    tools.render();
    if (state.grid) grid.refresh();
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
    if (close) return closeTab(Number(close.dataset.pdfClose));
    if (t.closest("[data-pdf-grid-toggle]")) return showGrid(!state.grid);
    const toolPick = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-tool-pick]"));
    if (toolPick) return takeTool(toolPick.dataset.pdfToolPick ?? "select");
    const ink = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-ink]"));
    if (ink) {
      marks.color(ink.dataset.pdfInk ?? INKS[0]);
      for (const b of section.querySelectorAll("[data-pdf-ink]")) b.classList.toggle("on", b === ink);
      return;
    }
    const comment = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-comment]"));
    if (comment) return marks.show(comment.dataset.pdfComment ?? "");
    if (t.closest("[data-pdf-undo]")) return void host.dispatch({ type: "pdfUndo" });
    if (t.closest("[data-pdf-redo]")) return void host.dispatch({ type: "pdfRedo" });
    if (t.closest("[data-pdf-save]")) return void host.dispatch({ type: "pdfSave" });
    if (t.closest("[data-pdf-save-as]")) {
      state.ask = { kind: "saveAs" };
      return renderAsk();
    }
    const answer = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-ask-do]"));
    if (answer) {
      const ask = state.ask;
      state.ask = null;
      renderAsk();
      if (ask?.kind === "close" && answer.dataset.pdfAskDo === "discard") host.dispatch({ type: "pdfClose", tab: ask.tab, discard: true });
      if (ask?.kind === "close" && answer.dataset.pdfAskDo === "save") {
        if (state.active !== ask.tab) host.dispatch({ type: "pdfFocus", tab: ask.tab });
        host.dispatch({ type: "pdfSave" });
        host.dispatch({ type: "pdfClose", tab: ask.tab });
      }
      return;
    }
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
    const form = /** @type {HTMLElement} */ (e.target);
    if (form.matches("[data-pdf-find]")) return void e.preventDefault();
    if (!form.matches("[data-pdf-save-as-form]")) return;
    e.preventDefault();
    const path = /** @type {HTMLInputElement} */ (form.querySelector("[data-pdf-save-as-path]")).value.trim();
    if (!path) return;
    state.ask = null;
    renderAsk();
    host.dispatch({ type: "pdfSaveAs", path });
  });
  section.addEventListener("keydown", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    if (e.key === "Escape" && state.ask && t.closest("[data-pdf-ask]")) {
      e.preventDefault();
      e.stopPropagation();
      state.ask = null;
      renderAsk();
      pages.focus();
      return;
    }
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
    const typing = Boolean(/** @type {HTMLElement | null} */ (e.target instanceof HTMLElement ? e.target : null)?.closest("input, textarea, select, [contenteditable]:not([contenteditable=false])"));
    // The grid's own keys first: ⌘A, ⌫, Escape; then a comment's.
    if (state.grid && !typing && grid.key(e)) return true;
    if (!typing && !e.metaKey && marks.key(e)) {
      if (e.key === "Escape") takeTool("select");
      return true;
    }
    if (e.metaKey && !e.altKey && !e.ctrlKey) {
      const k = e.key.toLowerCase();
      if (k === "f") return (focusFind(), true);
      if (k === "g") return (step(e.shiftKey ? -1 : 1), true);
      if (k === "z") return (host.dispatch({ type: e.shiftKey ? "pdfRedo" : "pdfUndo" }), true);
      if (k === "s" && e.shiftKey) {
        state.ask = { kind: "saveAs" };
        renderAsk();
        return true;
      }
      if (k === "s") return (host.dispatch({ type: "pdfSave" }), true);
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
    if (e.key === "u" && !e.ctrlKey) return (host.dispatch({ type: "pdfUndo" }), true);
    if (e.key === "r" && e.ctrlKey) return (host.dispatch({ type: "pdfRedo" }), true);
    return view.key(e);
  }

  renderTabs();
  renderBar();
  renderOpen();
  renderSide();
  /** The comments of a tab, as the core lists them. @param {{tab: number, comments: any[]}} event */
  function comments(event) {
    if (event.tab !== state.active) return;
    marks.comments(event);
    if (state.side === "comments") renderSide();
    const count = marks.count();
    q('[data-pdf-side="comments"]').textContent = count ? `Comments ${count}` : "Comments";
  }

  /** The fields of a tab's form, as the core lists them. @param {{tab: number, fields: any[]}} event */
  function fields(event) {
    if (event.tab !== state.active) return;
    form.fields(event);
    tools.render();
  }

  return { tabs, show, key, comments, fields, view: () => view, search, state: () => state };
}

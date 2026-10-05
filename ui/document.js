/* The document viewer: files that are looked at rather than edited.
 *
 * A PDF is not a buffer. It has no cursor, no undo history and nothing `:w`
 * could mean, so it never becomes one — the core answers `openBuffer` on a
 * `.pdf` with `Event::Document` and this draws it over whatever mode is on
 * screen: Codex's code (not its shell), Data's notebook, a library citation's
 * page from anywhere.
 *
 * `pdf.js` is imported on first use rather than at boot. It is 1.7MB of
 * vendored library, and a session that never opens a PDF should never pay for
 * it — which is also why it is a separate module from `app.js`.
 *
 * Pages are drawn as they come near the window and let go of as they leave
 * it. Drawn all at once, a page is a canvas of eight megabytes at a high-DPI
 * screen's resolution, and a seven-hundred-page textbook would ask for more
 * memory than the window has. Each drawn page carries its text, laid out by
 * pdf.js over the picture, so it can be selected and copied.
 */

import { readDocument } from "./bridge.js";

/** Zoom steps, so `+` and `−` land on round numbers people recognise. */
const ZOOM = [0.5, 0.75, 1, 1.25, 1.5, 2, 3];

/** How far beyond the window a page is drawn before it is reached. */
const NEAR = "1200px 0px";

/** The space around the pages, matching the box's padding. */
const GUTTER = 32;

/** The loaded library, once something has needed it. */
let pdfjs = null;

/**
 * What is open: the document, the scale it is drawn at, and its pages — each
 * a slot sized to its page, and drawn into only while it is near.
 * @type {null | {doc: any, scale: number, pages: Page[], seen: IntersectionObserver}}
 */
let open = null;

/** @typedef {{n: number, slot: HTMLElement, w: number, h: number, drawn: number, task: any}} Page */

/** Whether the viewer is up, from the moment it is asked for: while it loads
 *  it owns the keyboard as much as after. */
let shown = false;

/** Which open is the current one, so one superseded while it loads stops. */
let generation = 0;

/** `g` pressed once, waiting for the second of `gg`. */
let pendingG = 0;

async function library() {
  if (pdfjs) return pdfjs;
  pdfjs = await import("./vendor/pdfjs/pdf.min.mjs");
  // The worker is what keeps parsing off the main thread; without this
  // pdf.js falls back to doing it inline and a large document freezes the
  // window while it loads.
  pdfjs.GlobalWorkerOptions.workerSrc = new URL(
    "./vendor/pdfjs/pdf.worker.min.mjs",
    import.meta.url,
  ).href;
  return pdfjs;
}

/** Where pdf.js finds what a PDF may lean on without carrying it: the
 *  character maps CJK text is written in, the standard fonts it names
 *  without embedding, the decoders for JPEG 2000 and JBIG2 images, and the
 *  colour profile CMYK is converted with — vendored beside the library, as
 *  it is (see vendor/pdfjs/README.md). The decoders are WebAssembly with a
 *  plain-JS fallback beside each, which pdf.js imports when a content
 *  security policy refuses to compile WebAssembly: Proton's allows only
 *  `script-src 'self'`, and the fallback is that. */
function assets() {
  const dir = (/** @type {string} */ name) => new URL(`./vendor/pdfjs/${name}/`, import.meta.url).href;
  return { cMapUrl: dir("cmaps"), cMapPacked: true, standardFontDataUrl: dir("standard_fonts"), wasmUrl: dir("wasm"), iccUrl: dir("iccs") };
}

const panel = () => /** @type {HTMLElement | null} */ (document.getElementById("doc-viewer"));
const box = () => /** @type {HTMLElement | null} */ (panel()?.querySelector("[data-doc-pages-box]") ?? null);
const label = (/** @type {string} */ name) => panel()?.querySelector(`[data-doc-${name}]`);

function note(/** @type {string} */ text) {
  const target = box();
  if (target) target.innerHTML = `<div class="doc-note">${text}</div>`;
}

/** Where the viewer goes: over what the mode on screen shows — Codex's code
 *  and not the shell beside it, or the whole of any other mode's stage. */
function stage() {
  const ws = document.querySelector(".workspace.on");
  return ws?.querySelector("[data-doc-host]") ?? ws?.querySelector(":scope > main") ?? null;
}

/** Show a document. Called from the `document` event. */
export async function openDocument(event) {
  const view = panel();
  if (!view) return;
  release();
  const mine = ++generation;
  const host = stage();
  if (host && view.parentElement !== host) host.append(view);
  view.hidden = false;
  shown = true;
  // Nothing underneath keeps the keyboard: a notebook cell or a field left
  // focused there would otherwise be typed into, unseen.
  /** @type {HTMLElement | null} */ (document.activeElement)?.blur?.();
  label("name").textContent = event.name;
  label("at").textContent = "";
  note("Loading…");

  let bytes;
  try {
    bytes = await readDocument(event.path);
  } catch (err) {
    if (mine === generation) note(`Could not read this file.<br>${escape(String(err))}`);
    return;
  }
  let lib;
  try {
    lib = await library();
  } catch (err) {
    if (mine === generation) note(`Could not load the PDF renderer.<br>${escape(String(err))}`);
    return;
  }
  let doc;
  try {
    doc = await lib.getDocument({ data: bytes, ...assets() }).promise;
  } catch (err) {
    if (mine === generation) note(`This does not look like a PDF.<br>${escape(String(err))}`);
    return;
  }
  if (mine !== generation) {
    doc.destroy();
    return;
  }

  // Every page is laid out at the first one's size and set right when it is
  // drawn: asking each page its size up front is a round trip to the worker
  // per page, and most documents have one size anyway.
  const first = (await doc.getPage(1)).getViewport({ scale: 1 });
  if (mine !== generation) {
    doc.destroy();
    return;
  }
  const target = /** @type {HTMLElement} */ (box());
  target.innerHTML = "";
  /** @type {Page[]} */
  const pages = [];
  for (let n = 1; n <= doc.numPages; n++) {
    const slot = document.createElement("div");
    slot.className = "doc-page";
    slot.dataset.page = String(n);
    target.append(slot);
    pages.push({ n, slot, w: first.width, h: first.height, drawn: 0, task: null });
  }
  const seen = new IntersectionObserver(near, { root: target, rootMargin: NEAR });
  open = { doc, scale: fitted(first.width), pages, seen };
  layout();
  for (const page of pages) seen.observe(page.slot);
  if (event.page && event.page > 1) goTo(event.page);
  where();
}

/** The scale at which a page `width` wide fills the box, within the steps' range. */
function fitted(/** @type {number} */ width) {
  const room = (box()?.clientWidth ?? 800) - GUTTER;
  return Math.min(ZOOM[ZOOM.length - 1], Math.max(ZOOM[0], room / width));
}

/** Each slot at its page's size, at the current scale. */
function layout() {
  if (!open) return;
  for (const page of open.pages) {
    page.slot.style.width = `${Math.floor(page.w * open.scale)}px`;
    page.slot.style.height = `${Math.floor(page.h * open.scale)}px`;
  }
  const zoom = label("zoom-level");
  if (zoom) zoom.textContent = `${Math.round(open.scale * 100)}%`;
}

/** Pages coming near are drawn; pages gone far are let go of. */
function near(/** @type {IntersectionObserverEntry[]} */ entries) {
  if (!open) return;
  for (const entry of entries) {
    const page = open.pages[Number(/** @type {HTMLElement} */ (entry.target).dataset.page) - 1];
    if (!page) continue;
    if (entry.isIntersecting) void draw(page);
    else forget(page);
  }
}

/** Draw one page, and lay its text over it. */
async function draw(/** @type {Page} */ page) {
  if (!open || page.task || page.drawn === open.scale) return;
  const { doc, scale } = open;
  const pdfPage = await doc.getPage(page.n);
  if (open?.doc !== doc || open.scale !== scale || !page.slot.isConnected) return;
  const viewport = pdfPage.getViewport({ scale });
  // Its own size, which a document of mixed pages need not share with the first.
  page.w = viewport.width / scale;
  page.h = viewport.height / scale;
  page.slot.style.width = `${Math.floor(viewport.width)}px`;
  page.slot.style.height = `${Math.floor(viewport.height)}px`;

  // Device pixels, so a page is not soft on a high-DPI screen — the canvas is
  // drawn at the real resolution and shown at the CSS one.
  const ratio = window.devicePixelRatio || 1;
  const canvas = document.createElement("canvas");
  canvas.width = Math.floor(viewport.width * ratio);
  canvas.height = Math.floor(viewport.height * ratio);
  const task = pdfPage.render({
    canvasContext: canvas.getContext("2d"),
    viewport,
    transform: ratio === 1 ? null : [ratio, 0, 0, ratio, 0, 0],
  });
  page.task = task;
  try {
    await task.promise;
  } catch {
    // Called off, because it went far or the zoom changed; either way a newer
    // draw is the one that counts.
    if (page.task === task) page.task = null;
    return;
  }
  if (page.task !== task) return;
  page.task = null;
  if (open?.doc !== doc || open.scale !== scale) return;

  const text = document.createElement("div");
  text.className = "textLayer";
  page.slot.style.setProperty("--total-scale-factor", String(scale));
  page.slot.replaceChildren(canvas, text);
  page.drawn = scale;
  try {
    await new pdfjs.TextLayer({ textContentSource: pdfPage.streamTextContent(), container: text, viewport }).render();
  } catch {
    // A page whose text cannot be read is still a picture of itself.
  }
}

/** Let a page's picture go, keeping its place. */
function forget(/** @type {Page} */ page) {
  page.task?.cancel();
  page.task = null;
  if (page.drawn) {
    page.slot.replaceChildren();
    page.drawn = 0;
  }
}

/** Drop what is open, keeping the panel. */
function release() {
  generation++;
  if (!open) return;
  open.seen.disconnect();
  for (const page of open.pages) forget(page);
  open.doc.destroy?.();
  open = null;
}

export function closeDocument() {
  release();
  shown = false;
  const view = panel();
  if (view) view.hidden = true;
  const target = box();
  if (target) target.innerHTML = "";
}

/** Draw at `scale`, keeping the page that was at the top of the window there. */
function rescale(/** @type {number} */ scale) {
  const target = box();
  if (!open || !target || scale === open.scale) return;
  const fraction = target.scrollHeight ? target.scrollTop / target.scrollHeight : 0;
  for (const page of open.pages) forget(page);
  open.scale = scale;
  layout();
  target.scrollTop = fraction * target.scrollHeight;
  // Seen again, so what is near now is drawn at the new scale.
  open.seen.disconnect();
  for (const page of open.pages) open.seen.observe(page.slot);
  where();
}

/** The next zoom step in or out from wherever it is now, fitted or not. */
export function zoomDocument(/** @type {number} */ by) {
  if (!open) return;
  const scale = by > 0 ? ZOOM.find((z) => z > open.scale + 1e-6) : [...ZOOM].reverse().find((z) => z < open.scale - 1e-6);
  if (scale) rescale(scale);
}

/** The page's width filling the window. */
export function fitDocument() {
  if (open) rescale(fitted(open.pages[0].w));
}

/** Show page `n` at the top of the window. */
function goTo(/** @type {number} */ n) {
  const target = box();
  const page = open?.pages[Math.min(open.pages.length, Math.max(1, n)) - 1];
  if (target && page) target.scrollTop = page.slot.offsetTop - GUTTER / 2;
}

/** Which page is at the top of the window, in the header. */
function where() {
  const target = box();
  const at = label("at");
  if (!open || !target || !at) return;
  const line = target.scrollTop + target.clientHeight / 3;
  const page = open.pages.find((p) => p.slot.offsetTop + p.slot.offsetHeight >= line) ?? open.pages[open.pages.length - 1];
  at.textContent = `page ${page.n} of ${open.pages.length}`;
}

let wherePending = false;
document.addEventListener("scroll", (e) => {
  if (e.target !== box() || wherePending) return;
  wherePending = true;
  requestAnimationFrame(() => {
    wherePending = false;
    where();
  });
}, true);

/** Whether a document is on screen: up, and in the mode that is. */
export function documentShown() {
  const view = panel();
  return shown && !!view && !view.hidden && !!view.closest(".workspace.on");
}

/** Whether a document is open at all, wherever it is. */
export function documentIsOpen() {
  return shown;
}

/**
 * A key, while a document is on screen. Its own keys scroll, page and zoom
 * it, and Escape or `q` puts it away; answers whether it was one of them.
 * @param {KeyboardEvent} e
 */
export function documentKey(e) {
  const target = box();
  if (!target) return false;
  const screen = target.clientHeight * 0.9;
  const by = (/** @type {number} */ dy) => {
    target.scrollBy({ top: dy });
    return true;
  };
  if (e.ctrlKey) {
    switch (e.key) {
      case "d": return by(screen / 2);
      case "u": return by(-screen / 2);
      case "f": return by(screen);
      case "b": return by(-screen);
    }
    return false;
  }
  const g = pendingG;
  pendingG = 0;
  switch (e.key) {
    case "Escape":
    case "q":
      closeDocument();
      return true;
    case "j":
    case "ArrowDown":
      return by(60);
    case "k":
    case "ArrowUp":
      return by(-60);
    case "h":
    case "ArrowLeft":
      target.scrollBy({ left: -60 });
      return true;
    case "l":
    case "ArrowRight":
      target.scrollBy({ left: 60 });
      return true;
    case " ":
      return by(e.shiftKey ? -screen : screen);
    case "PageDown":
      return by(screen);
    case "PageUp":
      return by(-screen);
    case "Home":
      target.scrollTop = 0;
      return true;
    case "End":
    case "G":
      target.scrollTop = target.scrollHeight;
      return true;
    case "g":
      if (g && Date.now() - g < 800) target.scrollTop = 0;
      else pendingG = Date.now();
      return true;
    case "+":
    case "=":
      zoomDocument(1);
      return true;
    case "-":
      zoomDocument(-1);
      return true;
    case "0":
      fitDocument();
      return true;
  }
  return false;
}

function escape(/** @type {string} */ text) {
  return text.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] ?? c);
}

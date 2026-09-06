/* The document viewer: files that are looked at rather than edited.
 *
 * A PDF is not a buffer. It has no cursor, no undo history and nothing `:w`
 * could mean, so it never becomes one — the core answers `openBuffer` on a
 * `.pdf` with `Event::Document` and this draws it over the panes.
 *
 * `pdf.js` is imported on first use rather than at boot. It is 1.7MB of
 * vendored library, and a session that never opens a PDF should never pay for
 * it — which is also why it is a separate module from `app.js`.
 */

import { readDocument } from "./bridge.js";

const $ = (sel, root = document) => root.querySelector(sel);

/** Zoom steps, so `+` and `−` land on round numbers people recognise. */
const ZOOM = [0.5, 0.75, 1, 1.25, 1.5, 2, 3];

/** The loaded library, once something has needed it. */
let pdfjs = null;

/** What is open, so a zoom can redraw it without asking for the bytes again. */
let open = null;

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

function box() {
  return $("[data-doc-pages-box]");
}

function note(text) {
  const target = box();
  if (target) target.innerHTML = `<div class="doc-note">${text}</div>`;
}

/** Show a document. Called from the `document` event. */
export async function openDocument(event) {
  const panel = $("#doc-viewer");
  if (!panel) return;
  panel.hidden = false;
  $("[data-doc-name]").textContent = event.name;
  $("[data-doc-pages]").textContent = "";
  note("Loading…");

  let bytes;
  try {
    bytes = await readDocument(event.path);
  } catch (err) {
    note(`Could not read this file.<br>${String(err)}`);
    return;
  }

  let lib;
  try {
    lib = await library();
  } catch (err) {
    note(`Could not load the PDF renderer.<br>${String(err)}`);
    return;
  }

  try {
    // `isEvalSupported: false` because the page's CSP has no `unsafe-eval`,
    // and pdf.js would otherwise try to compile some font programs that way
    // and log a failure for every one of them.
    const task = lib.getDocument({ data: bytes, isEvalSupported: false });
    const doc = await task.promise;
    open = { doc, zoom: 2 };
    $("[data-doc-pages]").textContent =
      doc.numPages === 1 ? "1 page" : `${doc.numPages} pages`;
    await draw();
  } catch (err) {
    note(`This does not look like a PDF.<br>${String(err)}`);
  }
}

/** Which draw is the current one.
 *
 * Rendering a page is asynchronous, so two draws can be in flight at once —
 * which is what pressing `−` twice does. Without this the second clears the
 * box while the first is still appending to it, and a three-page document
 * ends up with six canvases. Measured, not hypothetical.
 */
let generation = 0;

/** The render in flight, so a new draw can call it off.
 *
 * Abandoning is not enough on its own: pdf.js refuses to render the same page
 * into two places at once, so an old task left running makes the new draw
 * throw halfway and stop — which showed up as a three-page document rendering
 * exactly one page.
 */
let rendering = null;

/** Render every page at the current zoom. */
async function draw() {
  const target = box();
  if (!target || !open) return;
  const mine = ++generation;

  // Stop the previous one before touching anything it might still be drawing
  // into. `cancel` rejects its promise, which the loop below treats as an
  // ordinary way to finish rather than as a failure.
  rendering?.cancel();
  rendering = null;

  target.innerHTML = "";
  $("[data-doc-zoom-level]").textContent = `${Math.round(ZOOM[open.zoom] * 100)}%`;

  // Device pixels, so a page is not soft on a high-DPI screen — the canvas is
  // drawn at the real resolution and shown at the CSS one.
  const ratio = window.devicePixelRatio || 1;
  for (let n = 1; n <= open.doc.numPages; n++) {
    // Checked after every await: a newer draw has already cleared the box, so
    // anything this one adds now is a page from the last zoom level.
    if (mine !== generation) return;
    const page = await open.doc.getPage(n);
    if (mine !== generation) return;

    const viewport = page.getViewport({ scale: ZOOM[open.zoom] });
    const canvas = document.createElement("canvas");
    canvas.className = "doc-page";
    canvas.width = Math.floor(viewport.width * ratio);
    canvas.height = Math.floor(viewport.height * ratio);
    canvas.style.width = `${Math.floor(viewport.width)}px`;
    canvas.style.height = `${Math.floor(viewport.height)}px`;
    target.appendChild(canvas);

    const task = page.render({
      canvasContext: canvas.getContext("2d"),
      viewport,
      transform: ratio === 1 ? null : [ratio, 0, 0, ratio, 0, 0],
    });
    rendering = task;
    try {
      await task.promise;
    } catch {
      // Either this draw was cancelled — in which case a newer one is already
      // doing the work — or the page could not be drawn. Neither is a reason
      // to abandon the pages after it.
      if (mine !== generation) return;
    }
    if (rendering === task) rendering = null;
  }
}

export function closeDocument() {
  const panel = $("#doc-viewer");
  if (panel) panel.hidden = true;
  // Abandons any render still in flight, so a page from the document just
  // closed cannot appear inside the next one.
  generation++;
  rendering?.cancel();
  rendering = null;
  // The pages are canvases holding a bitmap each; a long document left open
  // is real memory.
  const target = box();
  if (target) target.innerHTML = "";
  open?.doc?.destroy?.();
  open = null;
}

export function zoomDocument(by) {
  if (!open) return;
  const next = Math.min(ZOOM.length - 1, Math.max(0, open.zoom + by));
  if (next === open.zoom) return;
  open.zoom = next;
  draw();
}

/** Whether a document is on screen, so the editor can be left alone. */
export function documentIsOpen() {
  return Boolean(open);
}

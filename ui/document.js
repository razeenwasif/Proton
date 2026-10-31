/* The document viewer: files that are looked at rather than edited.
 *
 * A PDF is not a buffer. It has no cursor, no undo history and nothing `:w`
 * could mean, so it never becomes one — the core answers `openBuffer` on a
 * `.pdf` with `Event::Document` and this draws it over whatever mode is on
 * screen: Codex's code (not its shell), Data's notebook, a library citation's
 * page from anywhere. The drawing is `pdf-view.js`'s; this is the panel it
 * is drawn in, and the keys it takes.
 */

import { readDocument, saveDocumentAs } from "./bridge.js";
import { createPdfView } from "./pdf-view.js";

const panel = () => /** @type {HTMLElement | null} */ (document.getElementById("doc-viewer"));
const label = (/** @type {string} */ name) => panel()?.querySelector(`[data-doc-${name}]`);

/** The view, made the first time a document is opened. */
let view = /** @type {ReturnType<typeof createPdfView> | null} */ (null);

/** Whether the viewer is up, from the moment it is asked for: while it loads
 *  it owns the keyboard as much as after. */
let shown = false;

/** Which open is the current one, so one superseded while it reads stops. */
let generation = 0;

/** The document on screen: its path and name, to save a copy of. */
let current = /** @type {{path: string, name: string} | null} */ (null);

function theView() {
  if (view) return view;
  const box = /** @type {HTMLElement | null} */ (panel()?.querySelector("[data-doc-pages-box]") ?? null);
  if (!box) return null;
  view = createPdfView(box, {
    onWhere: (page, pages, scale) => {
      const at = label("at");
      if (at) at.textContent = `page ${page} of ${pages}`;
      const zoom = label("zoom-level");
      if (zoom) zoom.textContent = `${Math.round(scale * 100)}%`;
    },
  });
  return view;
}

/** Where the viewer goes: over what the mode on screen shows — Codex's code
 *  and not the shell beside it, or the whole of any other mode's stage. */
function stage() {
  const ws = document.querySelector(".workspace.on");
  return ws?.querySelector("[data-doc-host]") ?? ws?.querySelector(":scope > main") ?? null;
}

/** Show a document. Called from the `document` event.
 *  @param {{path: string, name: string, page?: number | null}} event */
export async function openDocument(event) {
  const shell = panel();
  const pdf = theView();
  if (!shell || !pdf) return;
  const mine = ++generation;
  const host = stage();
  if (host && shell.parentElement !== host) host.append(shell);
  shell.hidden = false;
  shown = true;
  // Nothing underneath keeps the keyboard: a notebook cell or a field left
  // focused there would otherwise be typed into, unseen.
  /** @type {HTMLElement | null} */ (document.activeElement)?.blur?.();
  current = { path: event.path, name: event.name };
  const name = label("name");
  if (name) name.textContent = event.name;
  const at = label("at");
  if (at) at.textContent = "";
  pdf.note("Loading…");
  let bytes;
  try {
    bytes = await readDocument(event.path);
  } catch (err) {
    if (mine === generation) pdf.note(`Could not read this file.<br>${escape(String(err))}`);
    return;
  }
  if (mine !== generation) return;
  await pdf.show(bytes, { page: event.page });
}

/** Save a copy of the document on screen; what happened, in words, or null
 *  if nothing did. */
export async function saveDocument() {
  if (!current) return null;
  const file = current.name.toLowerCase().endsWith(".pdf") ? current.name : `${current.name}.pdf`;
  try {
    const to = await saveDocumentAs(current.path, file);
    return to ? `Saved a copy as ${to}` : null;
  } catch (err) {
    return `Could not save a copy: ${String(err)}`;
  }
}

export function closeDocument() {
  generation++;
  view?.close();
  shown = false;
  const shell = panel();
  if (shell) shell.hidden = true;
}

/** The next zoom step in or out from wherever it is now, fitted or not. */
export function zoomDocument(/** @type {number} */ by) {
  view?.zoom(by);
}

/** The page's width filling the window. */
export function fitDocument() {
  view?.fit();
}

/** Whether a document is on screen: up, and in the mode that is. */
export function documentShown() {
  const shell = panel();
  return shown && !!shell && !shell.hidden && !!shell.closest(".workspace.on");
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
  if (e.key === "Escape" || (e.key === "q" && !e.ctrlKey)) {
    closeDocument();
    return true;
  }
  return view?.key(e) ?? false;
}

function escape(/** @type {string} */ text) {
  return text.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] ?? c);
}

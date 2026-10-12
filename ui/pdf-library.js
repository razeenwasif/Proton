/* The PDF mode's library: the books in the folders the person keeps them in,
 * as the core walks them (Event::PdfLibrary) — the ones being read first,
 * each where it was left, then a shelf for each folder, every book with its
 * cover. Covers are asked for only as their books come on screen
 * (Command::PdfCovers), drawn by the core, and kept here once given. What is
 * typed in the box above narrows the shelves to the books it names. */

import { escapeHtml as esc } from "./html.js";

/** @typedef {import("./bindings/PdfBook").PdfBook} Book */
/** @typedef {import("./bindings/PdfCover").PdfCover} Cover */
/** @typedef {{folders: string[], books: Book[], walking: boolean, missing: string[]}} LibraryEvent */

/** How many books Continue reading shows. */
const READING = 8;

/** A colour for a book with no cover yet, from its title. @param {string} text */
function hue(text) {
  let h = 0;
  for (const c of text) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}

/** @param {{box: HTMLElement, root: HTMLElement, dispatch: (command: any) => unknown, open: (path: string) => void}} host
 *  `box`: where the library is drawn; `root`: what scrolls it, for knowing which covers are on screen */
export function createPdfLibrary(host) {
  /** @type {{library: LibraryEvent | null, query: string, editing: boolean, renaming: string | null,
   *   draft: {title: string, author: string} | null}} */
  const state = { library: null, query: "", editing: false, renaming: null, draft: null };
  /** Whether this drawing has its renaming form yet: a book on two shelves has it once. */
  let formed = false;
  /** Covers given, by path: a picture's address, or null when there is none to draw. @type {Map<string, string | null>} */
  const covers = new Map();
  /** Covers asked for, not given yet. */
  const asked = new Set();
  /** Covers come on screen since they were last asked for. @type {Set<string>} */
  const wanted = new Set();
  /** @type {ReturnType<typeof setTimeout> | undefined} */
  let asking;

  const seen = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (!entry.isIntersecting) continue;
      const path = /** @type {HTMLElement} */ (entry.target).dataset.pdfCover ?? "";
      seen.unobserve(entry.target);
      if (!covers.has(path) && !asked.has(path)) wanted.add(path);
    }
    if (!wanted.size) return;
    clearTimeout(asking);
    asking = setTimeout(() => {
      const paths = [...wanted];
      wanted.clear();
      for (const p of paths) asked.add(p);
      host.dispatch({ type: "pdfCovers", paths });
    }, 80);
  }, { root: host.root, rootMargin: "300px" });

  /** Whether a book is one the words typed name. @param {Book} b */
  function named(b) {
    const words = state.query.toLowerCase().split(/\s+/).filter(Boolean);
    const text = `${b.title} ${b.author} ${b.file} ${b.shelf}`.toLowerCase();
    return words.every((w) => text.includes(w));
  }

  /** @param {Book} b */
  function coverHtml(b) {
    const data = covers.get(b.path);
    const pic = data ? `<img alt="" src="${esc(data)}" draggable="false">` : `<span class="pdf-cover-title">${esc(b.title)}</span>`;
    return `<span class="pdf-cover${data ? " drawn" : ""}" data-pdf-cover="${esc(b.path)}" style="--hue:${hue(b.title)}">${pic}` +
      (b.inCloud ? `<span class="pdf-cover-cloud" title="In iCloud: opening it brings it down">☁</span>` : "") + `</span>`;
  }

  /** A book being renamed: its cover, and what to call it. @param {Book} b */
  function renameHtml(b) {
    formed = true;
    const d = state.draft ?? { title: b.title, author: b.author };
    return `<form class="pdf-book-cell pdf-book-renaming" data-pdf-rename-form data-path="${esc(b.path)}" novalidate>${coverHtml(b)}` +
      `<input name="title" value="${esc(d.title)}" placeholder="${esc(b.file)}" aria-label="Title" spellcheck="false" autocomplete="off">` +
      `<input name="author" value="${esc(d.author)}" placeholder="Author" aria-label="Author" spellcheck="false" autocomplete="off">` +
      `<span class="pdf-rename-acts"><button type="submit" class="cc-primary">Save</button><button type="button" class="cc-quiet" data-pdf-rename-cancel>Cancel</button>` +
      (b.renamed ? `<button type="button" class="pdf-rename-own" data-pdf-rename-own title="Its title and author as the book says them">Its own</button>` : "") +
      `</span><span class="lab-faint">In your library only: ${esc(b.file)} is left as it is.</span></form>`;
  }

  /** A book, to open where it was left, and to rename. @param {Book} b */
  function bookHtml(b) {
    if (state.renaming === b.path && !formed) return renameHtml(b);
    return `<div class="pdf-book-cell">${cardHtml(b)}<button type="button" class="pdf-book-rename" data-pdf-rename="${esc(b.path)}" ` +
      `title="Rename it in your library  F2 — its file is left as it is" aria-label="Rename ${esc(b.title)}">✎</button></div>`;
  }

  /** @param {Book} b */
  function cardHtml(b) {
    // Who wrote it; or, said nowhere else, how long it is.
    const by = b.author || (b.pages && !b.page ? `${b.pages} pages` : "");
    const left = b.page && b.pages ? Math.min(100, Math.round((b.page / b.pages) * 100)) : 0;
    const where = b.page ? `page ${b.page}${b.pages ? ` of ${b.pages}` : ""}` : "";
    return `<button type="button" class="pdf-book" data-pdf-book="${esc(b.path)}" title="${esc(`${b.title}${b.author ? ` — ${b.author}` : ""}\n${b.path}${where ? `\nLeft at ${where}` : ""}`)}">` +
      coverHtml(b) +
      `<span class="pdf-book-title">${esc(b.title)}</span>` +
      (by ? `<span class="pdf-book-by">${esc(by)}</span>` : "") +
      (b.page ? `<span class="pdf-book-left"><span class="pdf-book-bar"><i style="width:${left}%"></i></span><span>${esc(where)}</span></span>` : "") +
      `</button>`;
  }

  /** @param {string} title @param {Book[]} books @param {string} [note] @param {boolean} [reading] */
  function shelfHtml(title, books, note = "", reading = false) {
    return `<section class="pdf-shelf"${reading ? " data-pdf-reading" : ""}><h4>${esc(title)} <span class="lab-faint">${books.length}${note ? ` · ${esc(note)}` : ""}</span></h4>` +
      `<div class="pdf-books">${books.map(bookHtml).join("")}</div></section>`;
  }

  function foldersHtml() {
    const typed = (state.library?.folders ?? []).join("\n");
    return `<form class="pdf-lib-folders" data-pdf-lib-folders-form novalidate>
        <p>The folders your books are in, one a line. Every PDF in them, and in the folders inside them, is shelved by the folder it is in.</p>
        <textarea name="folders" rows="3" spellcheck="false" placeholder="~/Documents/Books">${esc(typed)}</textarea>
        <div class="pdf-tool-row">${state.library?.folders.length ? `<button type="button" class="cc-quiet" data-pdf-lib-cancel>Cancel</button>` : ""}
          <button type="submit" class="cc-primary">Use these folders</button></div>
      </form>`;
  }

  function render() {
    const lib = state.library;
    formed = false;
    if (!lib) {
      host.box.innerHTML = "";
      return;
    }
    const missing = lib.missing.length ? `<p class="pdf-lib-missing">Not found: ${lib.missing.map(esc).join(", ")}</p>` : "";
    if (state.editing || !lib.folders.length) {
      host.box.innerHTML = missing + foldersHtml();
      return;
    }
    // Every book drawn, and what is typed hides those it does not name.
    const books = lib.books;
    const walking = lib.walking ? `<p class="lab-faint"><span class="lab-spin"></span> Looking through ${esc(lib.folders.join(", "))}…</p>` : "";
    if (!lib.books.length) {
      host.box.innerHTML = missing + (walking || `<p class="lab-faint">No PDFs in ${esc(lib.folders.join(", "))} yet.</p>`);
      return;
    }
    // Being read: the last read first.
    const reading = books.filter((b) => b.read).sort((a, b) => (b.read ?? 0) - (a.read ?? 0)).slice(0, READING);
    // A shelf for each folder, the library folder's own books first.
    /** @type {Map<string, Book[]>} */
    const shelves = new Map();
    for (const b of books) {
      const name = b.shelf || b.folder.split("/").pop() || b.folder;
      const key = `${b.shelf ? 1 : 0}\u0000${name.toLowerCase()}\u0000${name}`;
      shelves.set(key, [...(shelves.get(key) ?? []), b]);
    }
    const shelved = [...shelves.entries()].sort(([a], [b]) => a.localeCompare(b))
      .map(([key, of]) => shelfHtml(key.split("\u0000")[2], of.sort((a, b) => a.title.localeCompare(b.title)))).join("");
    host.box.innerHTML = missing + walking + (reading.length ? shelfHtml("Continue reading", reading, "", true) : "") + shelved +
      `<p class="lab-faint" data-pdf-lib-none hidden>None of your books is called that.</p>`;
    narrow();
    for (const cover of host.box.querySelectorAll(".pdf-cover:not(.drawn)")) seen.observe(cover);
  }

  /** The books what is typed does not name hidden, and the shelves left
   *  with none — nothing drawn again, every cover kept as it is. */
  function narrow() {
    const books = new Map((state.library?.books ?? []).map((b) => [b.path, b]));
    let any = false;
    for (const shelf of /** @type {NodeListOf<HTMLElement>} */ (host.box.querySelectorAll(".pdf-shelf"))) {
      let shown = 0;
      for (const cell of /** @type {NodeListOf<HTMLElement>} */ (shelf.querySelectorAll(".pdf-book-cell"))) {
        const path = cell.dataset.path ?? /** @type {HTMLElement | null} */ (cell.querySelector("[data-pdf-book]"))?.dataset.pdfBook ?? "";
        const book = books.get(path);
        cell.hidden = !(book && named(book));
        if (!cell.hidden) shown++;
      }
      // What is being read is for coming back to, not for looking things up.
      shelf.hidden = shown === 0 || ("pdfReading" in shelf.dataset && Boolean(state.query));
      any ||= !shelf.hidden;
    }
    const none = /** @type {HTMLElement | null} */ (host.box.querySelector("[data-pdf-lib-none]"));
    if (none) none.hidden = any;
  }

  /** The library, as the core walked it. @param {LibraryEvent} event */
  function library(event) {
    state.library = event;
    render();
  }

  /** Covers drawn. @param {{covers: Cover[]}} event */
  function drawn(event) {
    for (const c of event.covers) {
      covers.set(c.path, c.data);
      asked.delete(c.path);
      if (!c.data) continue;
      for (const el of host.box.querySelectorAll(`.pdf-cover[data-pdf-cover="${CSS.escape(c.path)}"]`)) {
        el.classList.add("drawn");
        el.querySelector(".pdf-cover-title")?.replaceWith(Object.assign(document.createElement("img"), { alt: "", src: c.data, draggable: false }));
      }
    }
  }

  /** What is typed in the box: the shelves narrowed to the books it names. @param {string} query */
  function filter(query) {
    if (query === state.query) return;
    state.query = query;
    narrow();
  }

  /** The first book what is typed names, to open with Enter. */
  function first() {
    return state.library?.books.find(named) ?? null;
  }

  /** Rename a book: its form, ready to type in. @param {string} path */
  function startRenaming(path) {
    state.renaming = path;
    state.draft = null;
    render();
    const field = /** @type {HTMLInputElement | null} */ (host.box.querySelector("[data-pdf-rename-form] [name=title]"));
    field?.focus();
    field?.select();
  }

  /** Done renaming: back to the book, its card focused. */
  function stopRenaming() {
    const path = state.renaming;
    state.renaming = null;
    state.draft = null;
    render();
    if (path) /** @type {HTMLElement | null} */ (host.box.querySelector(`[data-pdf-book="${CSS.escape(path)}"]`))?.focus();
  }

  /** Call a book by another title and author: drawn so at once, as the
   *  core will say. @param {string} path @param {string} title @param {string} author */
  function rename(path, title, author) {
    host.dispatch({ type: "pdfRenameBook", path, title, author });
    const book = state.library?.books.find((b) => b.path === path);
    if (book && title.trim()) Object.assign(book, { title: title.trim(), author: author.trim(), renamed: true });
    stopRenaming();
  }

  host.box.addEventListener("click", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    const renaming = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-rename]"));
    if (renaming) return startRenaming(renaming.dataset.pdfRename ?? "");
    if (t.closest("[data-pdf-rename-cancel]")) return stopRenaming();
    if (t.closest("[data-pdf-rename-own]")) return rename(state.renaming ?? "", "", "");
    const book = /** @type {HTMLElement | null} */ (t.closest("[data-pdf-book]"));
    if (book) return host.open(book.dataset.pdfBook ?? "");
    if (t.closest("[data-pdf-lib-cancel]")) {
      state.editing = false;
      render();
    }
  });
  host.box.addEventListener("input", (e) => {
    const form = /** @type {HTMLElement} */ (e.target).closest("[data-pdf-rename-form]");
    if (!form) return;
    const field = (/** @type {string} */ name) => /** @type {HTMLInputElement} */ (form.querySelector(`[name=${name}]`)).value;
    // Kept across the library being drawn again while it is typed.
    state.draft = { title: field("title"), author: field("author") };
  });
  host.box.addEventListener("keydown", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    if (e.key === "Escape" && t.closest("[data-pdf-rename-form]")) {
      e.preventDefault();
      e.stopPropagation();
      return stopRenaming();
    }
    const book = /** @type {HTMLElement | null} */ (t.closest?.("[data-pdf-book]"));
    if (e.key === "F2" && book) {
      e.preventDefault();
      e.stopPropagation();
      startRenaming(book.dataset.pdfBook ?? "");
    }
  });
  host.box.addEventListener("submit", (e) => {
    const form = /** @type {HTMLFormElement} */ (e.target);
    if (form.matches("[data-pdf-rename-form]")) {
      e.preventDefault();
      const data = new FormData(form);
      const title = String(data.get("title") ?? "").trim();
      // A book is called something: an emptied title is its own.
      return rename(form.dataset.path ?? "", title, title ? String(data.get("author") ?? "") : "");
    }
    if (!form.matches("[data-pdf-lib-folders-form]")) return;
    e.preventDefault();
    const folders = String(new FormData(form).get("folders") ?? "").split("\n").map((l) => l.trim()).filter(Boolean);
    host.dispatch({ type: "pdfLibraryFolders", folders });
    state.editing = false;
  });

  /** The folders, to change. */
  function editFolders() {
    state.editing = !state.editing;
    render();
    /** @type {HTMLTextAreaElement | null} */ (host.box.querySelector("textarea"))?.focus();
  }

  return { library, drawn, filter, first, editFolders, has: () => Boolean(state.library?.books.length), scan: () => host.dispatch({ type: "pdfLibraryScan" }) };
}

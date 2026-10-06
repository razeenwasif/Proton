/* The picker a notebook's sources are chosen from: the knowledge base — the
 * folders you study from — a folder at a time, or searched.
 *
 * Every file is listed, with its size and kind, and the ones still in
 * iCloud under a cloud: choosing one of those adds it and leaves it there,
 * unless you choose to bring it down, having been told how much that is.
 * What your rules or the defaults leave out is listed too, greyed, with the
 * reason — and a rule of yours can be taken back from here. The core does
 * the listing and the searching; this draws what it answers. */

import { escapeHtml as esc, icon } from "./html.js";

/** @typedef {import("./bindings/StudyRoot").StudyRoot} Root */
/** @typedef {import("./bindings/StudyKbFile").StudyKbFile} File */
/** @typedef {import("./bindings/StudyKbFolder").StudyKbFolder} Folder */
/** @typedef {import("./bindings/StudyKbLeft").StudyKbLeft} Left */
/** @typedef {import("./bindings/StudyKbHit").StudyKbHit} Hit */
/** @typedef {{roots: Root[], rules: string[], started: boolean, ready: boolean, walking: boolean,
 *   done: number, total: number, paused: boolean, documents: number, passages: number}} KbEvent */
/** @typedef {{root: string, dir: string, folders: Folder[], files: File[], left: Left[]}} FolderEvent */
/** @typedef {{query: string, files: File[], passages: Hit[], discovered: boolean}} ResultsEvent */
/** @typedef {{root: string, dir: string, files: File[], total: number}} AllEvent */

/** A size, as a person reads one. @param {number} bytes */
export function size(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let n = bytes / 1024;
  let u = 0;
  while (n >= 1024 && u < units.length - 1) {
    n /= 1024;
    u++;
  }
  return `${n < 10 ? n.toFixed(1) : Math.round(n)} ${units[u]}`;
}

/** A count with its noun. @param {number} n @param {string} one @param {string} many */
const count = (n, one, many) => `${n.toLocaleString()} ${n === 1 ? one : many}`;

/** The icon a kind of file wears. */
export const KIND_ICON = /** @type {Record<string, string>} */ ({
  pdf: "file-doc", markdown: "file", text: "file", notebook: "file-code", tex: "file-code", captions: "film",
  table: "table", code: "file-code", document: "file-doc", slides: "file-doc", image: "image", audio: "audio", video: "film", web: "globe",
  artifact: "layers",
});

/** @param {{root: Document, dispatch: (command: any) => unknown, ws: HTMLElement, discoverable: () => boolean}} host */
export function createPicker(host) {
  const box = document.createElement("div");
  box.className = "st-picker";
  box.hidden = true;
  box.setAttribute("role", "dialog");
  box.setAttribute("aria-modal", "true");
  box.setAttribute("aria-label", "Add sources");
  box.dataset.studyPicker = "";
  host.ws.append(box);

  /** @type {{kb: KbEvent | null, folder: FolderEvent | null, results: ResultsEvent | null, query: string, root: string, dir: string,
   *   chosen: Map<string, File>, confirming: boolean, editing: boolean, asked: number, open: boolean, discovering: boolean,
   *   deep: string | null, deepSaid: string | null}} */
  const state = {
    kb: null, folder: null, results: null, query: "", root: "", dir: "",
    // What has been ticked to add, by labelled path, kept across folders.
    chosen: new Map(),
    // Asking whether to bring files down from iCloud; editing the roots.
    confirming: false, editing: false,
    // When the knowledge base was last looked at again, by this.
    asked: 0, open: false,
    // Looking for what is like the open notebook.
    discovering: false,
    // A folder whose every file, subfolders too, is being ticked; and what
    // came of the last.
    deep: null, deepSaid: null,
  };

  const browse = (/** @type {string} */ root, /** @type {string} */ dir) => {
    state.root = root;
    state.dir = dir;
    host.dispatch({ type: "studyKbBrowse", root, dir });
  };

  /* ── drawing ───────────────────────────────────────────────────── */

  function progressHtml() {
    const kb = state.kb;
    if (!kb || !kb.started) return "";
    if (!kb.ready) return `<span class="st-busy"><span class="lab-spin"></span> Listing your folders…</span>`;
    if (kb.walking) {
      const pause = kb.paused
        ? `<button class="cc-quiet" data-study-kb-pause="false">Go on</button>`
        : `<button class="cc-quiet" data-study-kb-pause="true" title="Reading PDFs the first time takes minutes">Pause</button>`;
      return `<span class="st-busy">${kb.paused ? "" : '<span class="lab-spin"></span> '}Reading for search: ${kb.done.toLocaleString()} of ${kb.total.toLocaleString()}${kb.paused ? " — paused" : ""}</span>${pause}`;
    }
    return `<span class="st-faint">${count(kb.documents, "document", "documents")} searchable</span>`;
  }

  function rootsHtml() {
    const kb = state.kb;
    if (!kb) return "";
    const rows = kb.roots.map((r) => {
      const on = r.label === state.root && !state.query;
      const counts = r.missing ? "not found" : `${count(r.files, "file", "files")}${r.inCloud ? ` · ${r.inCloud.toLocaleString()} in iCloud` : ""}`;
      return `<button class="st-root${on ? " on" : ""}" data-study-root="${esc(r.label)}" title="${esc(r.path)}"${r.missing ? " disabled" : ""}>` +
        `${icon(r.inCloud && r.inCloud === r.files ? "cloud" : "folder")}<span class="st-root-name">${esc(r.label)}</span>` +
        `<span class="st-faint">${esc(counts)}</span></button>`;
    }).join("");
    return `${rows}<button class="st-link" data-study-edit-roots>${kb.roots.length ? "Change folders" : "Choose folders"}</button>`;
  }

  /** @param {File} f */
  function fileHtml(f) {
    const chosen = state.chosen.has(f.path);
    const added = f.source ? `<span class="st-tag" title="Already in this notebook">${esc(f.source)}</span>` : "";
    const cloud = f.inCloud ? `<span class="st-cloud" title="In iCloud: added, it stays there until you download it">${icon("cloud")}</span>` : "";
    const can = f.readable && !f.source;
    const why = !f.readable ? ` title="${esc(f.kind)} files are not read yet"`
      : f.kind === "audio" || f.kind === "video" ? ` title="A recording: added, it is transcribed by Fish Audio only when you choose, told what it costs"` : "";
    return `<label class="st-file${can ? "" : " off"}"${why}>` +
      `<input type="checkbox" data-study-choose="${esc(f.path)}"${chosen ? " checked" : ""}${can ? "" : " disabled"}>` +
      `${icon(KIND_ICON[f.kind] ?? "file")}<span class="st-file-name">${esc(f.name)}</span>${cloud}${added}` +
      `<span class="st-faint st-size">${esc(size(f.size))}</span></label>`;
  }

  /** What is like the notebook: each file, and the passage most like it. */
  function discoveredHtml() {
    const r = /** @type {ResultsEvent} */ (state.results);
    if (!r.files.length) return `<p class="st-faint st-pad">Nothing in your folders is like it that is not in it already.</p>`;
    return `<p class="st-faint st-pad">Not in this notebook yet, and most like it — found by the words in the box, which its sources share.</p>` +
      r.files.map((f, i) => {
        const hit = r.passages[i];
        return `<div class="st-found">${fileHtml(f)}${hit ? `<p class="st-faint">${esc(hit.cite.slice(hit.path.length).trim())} ${esc(hit.text)}</p>` : ""}</div>`;
      }).join("");
  }

  function listingHtml() {
    if (state.discovering) return `<p class="st-faint st-pad"><span class="lab-spin"></span> Looking for what is like this notebook…</p>`;
    if (state.query && state.results?.discovered && state.results.query === state.query) return discoveredHtml();
    if (state.query) return resultsHtml();
    const folder = state.folder;
    if (!state.kb?.ready) return `<p class="st-faint st-pad">The folders are being listed.</p>`;
    if (!folder || folder.root !== state.root) return `<p class="st-faint st-pad">Choose a folder on the left.</p>`;
    const parts = folder.dir ? folder.dir.split("/") : [];
    const crumbs = [`<button class="st-crumb" data-study-dir="">${esc(folder.root)}</button>`]
      .concat(parts.map((p, i) => `<button class="st-crumb" data-study-dir="${esc(parts.slice(0, i + 1).join("/"))}">${esc(p)}</button>`))
      .join(`<span class="st-faint">/</span>`);
    const folders = folder.folders.map((f) => {
      const dir = folder.dir ? `${folder.dir}/${f.name}` : f.name;
      const cloud = f.inCloud ? ` · ${f.inCloud === f.files ? "in iCloud" : `${f.inCloud} in iCloud`}` : "";
      return `<div class="st-folder-row"><button class="st-folder" data-study-dir="${esc(dir)}">${icon("folder")}<span class="st-file-name">${esc(f.name)}</span>` +
        `<span class="st-faint">${esc(count(f.readable, "to read", "to read"))}${esc(cloud)}</span></button>` +
        `<button class="st-hide" data-study-leave="${esc(`${folder.root}/${dir}/`)}" title="Leave this folder out of the knowledge base">Leave out</button></div>`;
    }).join("");
    // Every file here that can be added, ticked or unticked at once; and
    // every one in the subfolders too.
    const addable = folder.files.filter((f) => f.readable && !f.source);
    const all = addable.length > 0 && addable.every((f) => state.chosen.has(f.path));
    const some = addable.some((f) => state.chosen.has(f.path));
    const below = folder.folders.some((f) => f.files > 0);
    const where = folder.dir ? `${folder.root}/${folder.dir}` : folder.root;
    const deep = state.deep === where
      ? `<span class="st-faint"><span class="lab-spin"></span> Ticking every file below…</span>`
      : below ? `<button class="st-link" data-study-choose-deep="${esc(folder.dir)}" title="Every file in this folder and the folders inside it that can be added">${addable.length ? "With its subfolders" : "Select everything in its subfolders"}</button>` : "";
    const bar = addable.length || below
      ? `<div class="st-select-bar">${addable.length ? `<label><input type="checkbox" data-study-choose-all${all ? " checked" : ""}${some && !all ? " data-some" : ""}> Select all ${esc(count(addable.length, "file", "files"))} here</label>` : ""}` +
        `${deep}${state.deepSaid ? `<span class="st-faint">${esc(state.deepSaid)}</span>` : ""}</div>` : "";
    const files = bar + folder.files.map(fileHtml).join("");
    const left = folder.left.map((l) =>
      `<div class="st-left" title="${esc(l.why)}">${icon(l.dir ? "folder" : "file")}<span class="st-file-name">${esc(l.name)}</span>` +
      `<span class="st-faint">${esc(l.why)}</span>` +
      (l.rule ? `<button class="st-link" data-study-let="${esc(l.rule)}">Let back in</button>` : "") + `</div>`).join("");
    const empty = !folders && !files && !left ? `<p class="st-faint st-pad">Nothing here.</p>` : "";
    return `<nav class="st-crumbs">${crumbs}</nav>${folders}${files}${left}${empty}`;
  }

  function resultsHtml() {
    const r = state.results;
    if (!r || r.query !== state.query) return `<p class="st-faint st-pad">Searching…</p>`;
    const files = r.files.length ? `<h4>Files</h4>${r.files.map(fileHtml).join("")}` : "";
    const passages = r.passages.length
      ? `<h4>In their words</h4>${r.passages.map((h) => {
        const chosen = state.chosen.has(h.path);
        return `<div class="st-hit"><label><input type="checkbox" data-study-choose="${esc(h.path)}"${chosen ? " checked" : ""}>` +
          `<span class="st-file-name">${esc(h.path)}</span></label>` +
          `<p><span class="st-faint">${esc(h.cite.slice(h.path.length).trim())}</span> ${esc(h.text)}</p></div>`;
      }).join("")}`
      : "";
    return files || passages ? files + passages : `<p class="st-faint st-pad">Nothing found for “${esc(r.query)}”.</p>`;
  }

  function rootsFormHtml() {
    const typed = (state.kb?.roots ?? []).map((r) => r.path).join("\n");
    return `<form class="st-roots-form" data-study-roots-form novalidate>
        <p>The folders your notebooks' sources are chosen from — a vault, a courses folder, an iCloud folder. One a line.
          Nothing in them is read until you open Notebook, and a file in iCloud is never downloaded unless you ask for it.</p>
        <textarea name="roots" rows="5" spellcheck="false" placeholder="~/Notes&#10;~/Courses">${esc(typed)}</textarea>
        <div class="rs-actions">${state.kb?.roots.length ? `<button type="button" class="cc-quiet" data-study-roots-cancel>Cancel</button>` : ""}
          <button type="submit" class="cc-primary">Use these folders</button></div>
      </form>`;
  }

  function footHtml() {
    const chosen = [...state.chosen.values()];
    if (!chosen.length) return `<span class="st-faint">Tick files to add them.</span><span class="grow"></span><button class="cc-quiet" data-study-picker-close>Done</button>`;
    const cloud = chosen.filter((f) => f.inCloud);
    const bytes = cloud.reduce((n, f) => n + f.size, 0);
    if (state.confirming) {
      return `<span>Download ${esc(count(cloud.length, "file", "files"))} from iCloud — <b>${esc(size(bytes))}</b> — and add ${chosen.length === cloud.length ? "them" : "everything ticked"}?</span>
        <span class="grow"></span><button class="cc-quiet" data-study-confirm="no">Back</button>
        <button class="cc-primary" data-study-confirm="yes">Download and add</button>`;
    }
    const note = cloud.length ? ` · ${cloud.length} in iCloud (${size(bytes)})` : "";
    const download = cloud.length ? `<button class="cc-quiet" data-study-download>Download and add…</button>` : "";
    return `<span>${esc(count(chosen.length, "file", "files"))} ticked${esc(note)}</span>
      <button class="st-link" data-study-clear>Clear</button><span class="grow"></span>${download}
      <button class="cc-primary" data-study-add title="${cloud.length ? "Files in iCloud are added and left there" : ""}">Add ${esc(count(chosen.length, "source", "sources"))}</button>`;
  }

  function render() {
    box.hidden = !state.open;
    if (!state.open) return;
    const editing = state.editing || !(state.kb?.roots.length);
    const search = /** @type {HTMLInputElement | null} */ (box.querySelector("[data-study-search]"));
    // The search box is drawn once, so typing in it is never interrupted.
    if (!search) {
      box.innerHTML = `<div class="st-picker-card">
          <header class="st-picker-head"><b>Add sources</b><span class="grow"></span>
            <button class="tb-btn" data-study-picker-close aria-label="Close" title="Close  Esc">${icon("close")}</button></header>
          <div class="st-picker-bar"><input type="search" data-study-search placeholder="Search your notes, slides and papers — names and words" spellcheck="false" autocomplete="off">
            <button class="cc-quiet" data-study-discover title="Find what in your folders is like this notebook's sources, and not in it yet">${icon("spark")}Like this notebook</button>
            <span class="st-picker-progress" data-study-progress></span></div>
          <div class="st-picker-body" data-study-picker-body></div>
          <footer class="st-picker-foot" data-study-foot></footer>
        </div>`;
    }
    /** @type {HTMLElement} */ (box.querySelector("[data-study-discover]")).hidden = editing || !state.kb?.ready || !host.discoverable();
    const body = /** @type {HTMLElement} */ (box.querySelector("[data-study-picker-body]"));
    const keep = body.querySelector(".st-listing")?.scrollTop ?? 0;
    body.innerHTML = editing ? rootsFormHtml() : `<nav class="st-roots">${rootsHtml()}</nav><section class="st-listing">${listingHtml()}</section>`;
    const listing = body.querySelector(".st-listing");
    if (listing) listing.scrollTop = keep;
    // Some of a folder's files ticked: its "Select all" says so.
    const partly = /** @type {HTMLInputElement | null} */ (body.querySelector("[data-study-choose-all][data-some]"));
    if (partly) partly.indeterminate = true;
    /** @type {HTMLElement} */ (box.querySelector("[data-study-progress]")).innerHTML = progressHtml();
    /** @type {HTMLElement} */ (box.querySelector("[data-study-foot]")).innerHTML = editing ? "" : footHtml();
  }

  /* ── what is clicked ───────────────────────────────────────────── */

  function add(download = false) {
    const paths = [...state.chosen.keys()];
    if (!paths.length) return;
    host.dispatch({ type: download ? "studyDownload" : "studyAdd", paths });
    state.chosen.clear();
    state.confirming = false;
    close();
  }

  box.addEventListener("click", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    const at = (/** @type {string} */ sel) => /** @type {HTMLElement | null} */ (t.closest(sel));
    if (t === box || at("[data-study-picker-close]")) return close();
    const root = at("[data-study-root]");
    if (root) {
      state.query = "";
      const search = /** @type {HTMLInputElement | null} */ (box.querySelector("[data-study-search]"));
      if (search) search.value = "";
      browse(root.dataset.studyRoot ?? "", "");
      return render();
    }
    const dir = at("[data-study-dir]");
    if (dir) {
      state.deepSaid = null;
      return browse(state.root, dir.dataset.studyDir ?? "");
    }
    const deep = at("[data-study-choose-deep]");
    if (deep && state.folder) {
      const sub = deep.dataset.studyChooseDeep ?? "";
      state.deep = sub ? `${state.folder.root}/${sub}` : state.folder.root;
      state.deepSaid = null;
      host.dispatch({ type: "studyKbAll", root: state.folder.root, dir: sub });
      return render();
    }
    const leave = at("[data-study-leave]");
    if (leave) {
      host.dispatch({ type: "studyExclude", rule: leave.dataset.studyLeave, on: true });
      return browse(state.root, state.dir);
    }
    const let_ = at("[data-study-let]");
    if (let_) {
      host.dispatch({ type: "studyExclude", rule: let_.dataset.studyLet, on: false });
      return browse(state.root, state.dir);
    }
    if (at("[data-study-discover]")) {
      state.discovering = true;
      state.query = "";
      host.dispatch({ type: "studyDiscover", about: "" });
      return render();
    }
    const pause = at("[data-study-kb-pause]");
    if (pause) return host.dispatch({ type: "studyKbPause", paused: pause.dataset.studyKbPause === "true" });
    if (at("[data-study-edit-roots]")) {
      state.editing = true;
      return render();
    }
    if (at("[data-study-roots-cancel]")) {
      state.editing = false;
      return render();
    }
    if (at("[data-study-clear]")) {
      state.chosen.clear();
      return render();
    }
    if (at("[data-study-add]")) return add(false);
    if (at("[data-study-download]")) {
      state.confirming = true;
      return render();
    }
    const confirm = at("[data-study-confirm]");
    if (confirm) {
      if (confirm.dataset.studyConfirm === "yes") return add(true);
      state.confirming = false;
      return render();
    }
  });

  box.addEventListener("change", (e) => {
    const input = /** @type {HTMLInputElement} */ (e.target);
    if (input.matches("[data-study-choose-all]") && state.folder) {
      for (const f of state.folder.files.filter((x) => x.readable && !x.source)) {
        if (input.checked) state.chosen.set(f.path, f);
        else state.chosen.delete(f.path);
      }
      state.confirming = false;
      return render();
    }
    const path = input.dataset.studyChoose;
    if (path === undefined) return;
    const file = [...(state.folder?.files ?? []), ...(state.results?.files ?? [])].find((f) => f.path === path)
      // A passage's file, ticked from search: known only by its path.
      ?? { name: path.split("/").pop() ?? path, path, kind: "", size: 0, mtime: 0, inCloud: false, readable: true, source: null };
    if (input.checked) state.chosen.set(path, file);
    else state.chosen.delete(path);
    state.confirming = false;
    render();
  });

  /** @type {ReturnType<typeof setTimeout> | undefined} */
  let typing;
  box.addEventListener("input", (e) => {
    const input = /** @type {HTMLInputElement} */ (e.target);
    if (!input.matches("[data-study-search]")) return;
    clearTimeout(typing);
    typing = setTimeout(() => {
      state.discovering = false;
      state.query = input.value.trim();
      if (state.query) host.dispatch({ type: "studyKbSearch", query: state.query });
      render();
    }, 180);
  });

  box.addEventListener("submit", (e) => {
    const form = /** @type {HTMLFormElement} */ (e.target);
    if (!form.matches("[data-study-roots-form]")) return;
    e.preventDefault();
    const typed = String(new FormData(form).get("roots") ?? "").split("\n").map((l) => l.trim()).filter(Boolean);
    host.dispatch({ type: "studyRoots", roots: typed });
    state.editing = false;
    render();
  });

  box.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      close();
    }
  });

  /* ── the core's answers ────────────────────────────────────────── */

  /** @param {KbEvent} event */
  function kb(event) {
    const was = state.kb;
    state.kb = event;
    // The first root, once there is a catalogue to browse it in.
    if (event.ready && (!was?.ready || !event.roots.some((r) => r.label === state.root)) && event.roots.length) {
      const first = event.roots.find((r) => !r.missing) ?? event.roots[0];
      browse(first.label, "");
    } else if (event.ready && was && !was.ready && state.root) {
      browse(state.root, state.dir);
    }
    render();
  }

  /** @param {FolderEvent} event */
  function folder(event) {
    state.folder = event;
    state.root = event.root;
    state.dir = event.dir;
    render();
  }

  /** Every file under a folder, ticked. @param {AllEvent} event */
  function all(event) {
    const where = event.dir ? `${event.root}/${event.dir}` : event.root;
    if (state.deep !== where) return;
    state.deep = null;
    for (const f of event.files) state.chosen.set(f.path, f);
    state.confirming = false;
    state.deepSaid = event.total > event.files.length
      ? `The first ${event.files.length.toLocaleString()} of ${event.total.toLocaleString()} ticked.`
      : event.total ? `${count(event.total, "file", "files")} ticked, subfolders too.` : "Nothing more in its subfolders can be added.";
    render();
  }

  /** @param {ResultsEvent} event */
  function results(event) {
    state.results = event;
    // What was found for the notebook is shown with the words it was found
    // by, in the box, to change.
    if (event.discovered) {
      state.discovering = false;
      state.query = event.query;
      const search = /** @type {HTMLInputElement | null} */ (box.querySelector("[data-study-search]"));
      if (search) search.value = event.query;
    }
    render();
  }

  function open() {
    state.open = true;
    state.confirming = false;
    // Files added since it was last looked at are found by looking again.
    const now = Date.now();
    if (state.kb?.ready && now - state.asked > 60_000) {
      state.asked = now;
      host.dispatch({ type: "studyKbRefresh" });
    }
    if (state.kb?.ready && state.root) browse(state.root, state.dir);
    render();
    /** @type {HTMLInputElement | null} */ (box.querySelector(state.kb?.roots.length ? "[data-study-search]" : "textarea"))?.focus({ preventScroll: true });
  }

  function close() {
    state.open = false;
    render();
  }

  return { kb, folder, results, all, open, close, isOpen: () => state.open };
}

/* Notebook: notebooks of your own sources, to study from.
 *
 * Each notebook is a folder in a folder of your own — ~/Github/Notebooks —
 * and this draws what the core reads of it: down the left the notebooks,
 * and the open one's sources, ticked for the tutor to use; in the middle the
 * notebook itself; on the right the studio. Sources are chosen from the
 * knowledge base in the picker (study-kb.js).
 *
 * Nothing here decides anything: what is asked of the core is drawn when it
 * answers, so a notebook edited by hand is what is drawn the next time. */

import { escapeHtml as esc, icon } from "./html.js";
import { createPicker, KIND_ICON, size } from "./study-kb.js";

/** @typedef {import("./bindings/StudySummary").StudySummary} Summary */
/** @typedef {import("./bindings/StudySourceRow").StudySourceRow} SourceRow */
/** @typedef {import("./bindings/StudyNoteRow").StudyNoteRow} NoteRow */
/** @typedef {import("./bindings/StudyRole").StudyRole} Role */
/** @typedef {{notebooks: Summary[], open: string | null, folder: string | null}} ListEvent */
/** @typedef {{slug: string, title: string, course: string, exam: string | null, readOnly: string | null,
 *   sources: SourceRow[], notes: NoteRow[]}} NotebookEvent */

/** What a source can be to a course, in the order a menu offers them. */
export const ROLES = /** @type {[Role, string][]} */ ([
  ["notes", "Notes"], ["slides", "Slides"], ["reading", "Reading"], ["lab", "Lab"], ["assignment", "Assignment"],
  ["pastPaper", "Past paper"], ["solution", "Solution"], ["ownWork", "Your work"], ["recording", "Recording"], ["other", "Other"],
]);
const ROLE = Object.fromEntries(ROLES);

/** Where notebooks are kept, unless you say otherwise. */
const SUGGESTED = "~/Github/Notebooks";

/** @param {number} n @param {string} one @param {string} many */
const count = (n, one, many) => `${n.toLocaleString()} ${n === 1 ? one : many}`;

/** Whether a source's words are in the notebook. @param {SourceRow} s */
const hasWords = (s) => ["ready", "missing", "changed"].includes(s.status.state);

/** What a source's units count, in words. @param {SourceRow} s */
function extent(s) {
  if (!hasWords(s)) return "";
  if (s.units === "pages") return count(s.count, "page", "pages");
  if (s.units === "seconds") return `${Math.round(s.count / 60)} min`;
  return count(s.count, "line", "lines");
}

/** @param {{root: Document, dispatch: (command: any) => unknown}} host */
export function createStudy(host) {
  const ws = /** @type {HTMLElement | null} */ (host.root.querySelector('.workspace[data-ws="study"]'));
  if (!ws) return null;
  const q = (/** @type {string} */ sel) => /** @type {HTMLElement} */ (ws.querySelector(sel));
  const els = {
    list: q("[data-study-list]"),
    sources: q("[data-study-sources]"),
    where: q("[data-study-where]"),
    title: q("[data-study-title]"),
    stage: q("[data-study-stage]"),
    studio: q("[data-study-studio-body]"),
  };
  const picker = createPicker({ root: host.root, dispatch: host.dispatch, ws });

  /** @type {{list: Summary[], open: string | null, folder: string | null, said: boolean, notebook: NotebookEvent | null,
   *   creating: boolean, made: number, moving: string | null, editing: boolean, pasting: boolean, removing: string | null,
   *   getting: string[] | null}} */
  const state = {
    list: [], open: null, folder: null,
    // Whether the core has said where notebooks are kept — `null` until it has.
    said: false,
    notebook: null,
    // The new-notebook form is up, and how many there were when it was sent.
    creating: false, made: -1,
    // The folder being typed to keep notebooks in, once asked for.
    moving: null,
    // The notebook's details being edited; text being pasted; a source
    // whose taking out is being confirmed; sources whose download from
    // iCloud is being confirmed.
    editing: false, pasting: false, removing: null, getting: null,
  };

  const readOnly = () => Boolean(state.notebook?.readOnly);

  /* ── the list ──────────────────────────────────────────────────── */

  function renderList() {
    const shown = state.list.filter((n) => !n.archived);
    if (!shown.length) {
      els.list.innerHTML = state.folder ? `<div class="cc-none">No notebooks yet. Each is a folder in <code>${esc(state.folder)}</code>.</div>` : "";
      return;
    }
    els.list.innerHTML = shown.map((n) => {
      const on = n.slug === state.open && !state.creating;
      const sub = [n.course, count(n.sources, "source", "sources")].filter(Boolean).join(" · ");
      return `<button class="cc-session st-item${on ? " on" : ""}" data-study-open="${esc(n.slug)}"${on ? ' aria-current="true"' : ""}>` +
        `<span class="cc-session-name">${esc(n.title)}</span><span class="cc-session-where">${esc(sub)}</span></button>`;
    }).join("");
  }

  /* ── the sources ───────────────────────────────────────────────── */

  /** @param {SourceRow} s */
  function statusHtml(s) {
    const st = s.status;
    switch (st.state) {
      case "reading": return `<span class="st-badge busy"><span class="lab-spin"></span>Reading</span>`;
      case "downloading": return `<span class="st-badge busy"><span class="lab-spin"></span>Downloading</span>`;
      case "inCloud": return `<button class="st-badge cloud" data-study-get="${esc(s.id)}" title="In iCloud — ${esc(size(s.size))} to download">${icon("cloud")}Download</button>`;
      case "failed": return `<span class="st-badge bad" title="${esc(st.why)}">Not read</span>`;
      case "missing": return `<span class="st-badge" title="Its file is gone; what was read of it is kept">File gone</span>`;
      case "changed": return `<button class="st-badge" data-study-refresh="${esc(s.id)}" title="Its file has changed since it was read">Read again</button>`;
      default: return "";
    }
  }

  /** @param {SourceRow} s */
  function sourceHtml(s) {
    const origin = s.origin.from === "file" ? `${s.origin.root}/${s.origin.rel}` : s.origin.from === "note" ? `Note ${s.origin.note}` : "Pasted";
    const confirm = state.removing === s.id
      ? `<span class="st-confirm">Take out? <button class="st-link" data-study-remove-yes="${esc(s.id)}">Yes</button> <button class="st-link" data-study-remove-no>No</button></span>`
      : "";
    const thin = s.thin ? ` · ${s.thin} thin` : "";
    return `<div class="st-source${s.selected ? " on" : ""}" data-study-source="${esc(s.id)}" title="${esc(`${s.id} · ${origin}`)}">` +
      `<input type="checkbox" data-study-tick="${esc(s.id)}"${s.selected ? " checked" : ""}${hasWords(s) && !readOnly() ? "" : " disabled"} aria-label="Use ${esc(s.title)}">` +
      `${icon(KIND_ICON[s.kind] ?? "file")}` +
      `<span class="st-source-text"><span class="st-source-name">${esc(s.title)}</span>` +
      `<span class="st-faint">${esc(s.id)}${extent(s) ? ` · ${esc(extent(s))}${esc(thin)}` : ""}</span></span>` +
      (confirm || statusHtml(s) +
        (readOnly() ? "" : `<button class="st-x" data-study-remove="${esc(s.id)}" title="Take it out of this notebook" aria-label="Take it out">${icon("close")}</button>`)) +
      `</div>`;
  }

  /** What a source is to the course, to change. @param {SourceRow} s */
  function roleHtml(s) {
    const roles = ROLES.map(([value, name]) => `<option value="${value}"${value === s.role ? " selected" : ""}>${name}</option>`).join("");
    return `<select class="st-role" data-study-role="${esc(s.id)}" aria-label="What ${esc(s.title)} is" title="What it is to the course"${readOnly() ? " disabled" : ""}>${roles}</select>`;
  }

  function renderSources() {
    const nb = state.notebook;
    els.sources.hidden = !nb;
    if (!nb) {
      els.sources.innerHTML = "";
      return;
    }
    const usable = nb.sources.filter(hasWords);
    const ticked = usable.filter((s) => s.selected).length;
    const all = usable.length > 0 && ticked === usable.length;
    const head = `<header class="st-sources-head"><b>Sources</b><span class="st-faint">${ticked} of ${usable.length} used</span><span class="grow"></span>` +
      (readOnly() ? "" : `<button class="cc-quiet" data-study-pick title="Add sources from your folders">${icon("plus")}Add</button>`) + `</header>`;
    const tickAll = usable.length > 1 && !readOnly()
      ? `<label class="st-all"><input type="checkbox" data-study-all${all ? " checked" : ""}> Use every source</label>` : "";
    const rows = nb.sources.length
      ? nb.sources.map(sourceHtml).join("")
      : `<p class="st-faint st-pad">No sources yet.</p>`;
    els.sources.innerHTML = `${head}${confirmHtml()}${tickAll}<div class="scroll st-source-list">${rows}</div>`;
  }

  /** Bringing sources down from iCloud is said before it is done: what
   *  comes down, and how much. */
  function confirmHtml() {
    const ids = state.getting;
    const sources = (state.notebook?.sources ?? []).filter((s) => ids?.includes(s.id) && s.status.state === "inCloud");
    if (!sources.length) return "";
    const bytes = sources.reduce((n, s) => n + s.size, 0);
    const what = sources.length === 1 ? sources[0].title : count(sources.length, "file", "files");
    return `<div class="st-ask">${icon("cloud")}<span>Download <b>${esc(what)}</b> from iCloud — ${esc(size(bytes))} — and read ${sources.length === 1 ? "it" : "them"}?</span>
        <span class="rs-actions"><button class="cc-quiet" data-study-get-no>Cancel</button><button class="cc-primary" data-study-get-yes>Download</button></span></div>`;
  }

  /* ── where they are kept ───────────────────────────────────────── */

  function renderWhere() {
    if (!state.folder) {
      els.where.innerHTML = "";
      return;
    }
    if (state.moving !== null) {
      if (els.where.querySelector("[data-study-where-form]")) return;
      els.where.innerHTML = `<form class="rs-where-form" data-study-where-form novalidate>
          <label class="rs-field"><span>Keep notebooks in</span>
            <input name="folder" value="${esc(state.moving)}" spellcheck="false" autocomplete="off"></label>
          <div class="rs-actions"><button type="button" class="cc-quiet" data-study-where-cancel>Cancel</button>
            <button type="submit" class="cc-primary">Keep them here</button></div>
        </form>`;
      /** @type {HTMLInputElement | null} */ (els.where.querySelector("input"))?.focus({ preventScroll: true });
      return;
    }
    els.where.innerHTML = `<span>Kept in <code>${esc(state.folder)}</code></span>` +
      `<button class="rs-where-change" data-study-where-change title="Keep notebooks somewhere else">Change</button>`;
  }

  /* ── the stage ─────────────────────────────────────────────────── */

  function folderCardHtml() {
    return `<form class="st-card st-folder-card" data-study-folder-form novalidate>
        <h2>${icon("book")} Notebooks of your own sources</h2>
        <p>Each notebook is a folder of plain files — its sources, your notes, what the studio makes — kept together in a
          folder of their own. Nothing is uploaded: the words read from your sources stay in the notebook, out of Git.</p>
        <label class="rs-field"><span>Keep notebooks in</span>
          <input name="folder" value="${esc(SUGGESTED)}" spellcheck="false" autocomplete="off"></label>
        <div class="rs-actions"><button type="submit" class="cc-primary">Keep them here</button></div>
      </form>`;
  }

  function newFormHtml() {
    return `<form class="st-card rs-new" data-study-new-form novalidate>
        <h2>A new notebook</h2>
        <label class="rs-field"><span>Title</span>
          <input name="title" maxlength="120" autocomplete="off" spellcheck="true" placeholder="e.g. COMP3670 — the final exam"></label>
        <div class="rs-actions">${state.list.length ? `<button type="button" class="cc-quiet" data-study-new-cancel>Cancel</button>` : ""}
          <button type="submit" class="cc-primary">Make notebook</button></div>
      </form>`;
  }

  function homeHtml() {
    const shown = state.list.filter((n) => !n.archived);
    if (!shown.length || state.creating) return newFormHtml();
    const cards = shown.map((n) => `<button class="st-card st-book" data-study-open="${esc(n.slug)}">` +
      `<b>${esc(n.title)}</b><span class="st-faint">${esc([n.course, count(n.sources, "source", "sources"), n.exam ? `exam ${n.exam}` : ""].filter(Boolean).join(" · "))}</span></button>`).join("");
    return `<div class="st-home"><div class="st-books">${cards}</div></div>`;
  }

  function detailsHtml() {
    const nb = /** @type {NotebookEvent} */ (state.notebook);
    return `<form class="st-card st-details" data-study-details-form novalidate>
        <label class="rs-field"><span>Title</span><input name="title" value="${esc(nb.title)}" maxlength="120" autocomplete="off"></label>
        <label class="rs-field"><span>Course</span><input name="course" value="${esc(nb.course)}" placeholder="e.g. COMP3670" autocomplete="off"></label>
        <label class="rs-field"><span>Exam</span><input name="exam" type="date" value="${esc(nb.exam ?? "")}"></label>
        <div class="rs-actions"><button type="button" class="cc-quiet" data-study-details-cancel>Cancel</button>
          <button type="submit" class="cc-primary">Save</button></div>
      </form>`;
  }

  function pasteHtml() {
    return `<form class="st-card st-paste" data-study-paste-form novalidate>
        <h2>Paste text</h2>
        <label class="rs-field"><span>Title</span><input name="title" autocomplete="off" placeholder="Its first line, if left empty"></label>
        <label class="rs-field"><span>Text</span><textarea name="text" rows="10" spellcheck="true" placeholder="Notes, a passage, an email from the lecturer…"></textarea></label>
        <div class="rs-actions"><button type="button" class="cc-quiet" data-study-paste-cancel>Cancel</button>
          <button type="submit" class="cc-primary">Add as a source</button></div>
      </form>`;
  }

  function notebookHtml() {
    const nb = /** @type {NotebookEvent} */ (state.notebook);
    const held = nb.readOnly ? `<div class="st-banner">${icon("users")} ${esc(nb.readOnly)} has this notebook open: it is read only here.</div>` : "";
    if (state.editing) return held + detailsHtml();
    if (state.pasting) return held + pasteHtml();
    const facts = [nb.course, nb.exam ? `exam on ${nb.exam}` : ""].filter(Boolean).join(" · ");
    const head = `<div class="st-nb-head"><div><h1>${esc(nb.title)}</h1>` +
      `<span class="st-faint">${esc(facts || "No course or exam day yet")}</span></div>` +
      (readOnly() ? "" : `<button class="cc-quiet" data-study-details>Edit</button>`) + `</div>`;
    if (!nb.sources.length) {
      return `${held}${head}<div class="st-card st-empty">
          <h2>Add sources to begin</h2>
          <p>Choose notes, slides, labs and past papers from your folders, or paste text. Every answer and everything the
            studio makes is drawn from them, and cites them.</p>
          ${readOnly() ? "" : `<div class="rs-actions"><button class="cc-quiet" data-study-paste>Paste text</button>
            <button class="cc-primary" data-study-pick>${icon("plus")}Add sources</button></div>`}
        </div>`;
    }
    // What the notebook holds, by what each source is to the course.
    const groups = ROLES.map(([role, name]) => {
      const of = nb.sources.filter((s) => s.role === role);
      if (!of.length) return "";
      return `<section class="st-group"><h3>${esc(name)} <span class="st-faint">${of.length}</span></h3>` +
        of.map((s) => `<div class="st-line">${icon(KIND_ICON[s.kind] ?? "file")}<span>${esc(s.title)}</span>` +
          `<span class="st-faint">${esc(s.id)}${extent(s) ? ` · ${esc(extent(s))}` : ""}</span>${statusHtml(s)}${roleHtml(s)}</div>`).join("") + `</section>`;
    }).join("");
    const waiting = nb.sources.filter((s) => s.status.state === "inCloud");
    const bytes = waiting.reduce((n, s) => n + s.size, 0);
    const cloud = waiting.length && !readOnly()
      ? `<div class="st-banner">${icon("cloud")} ${esc(count(waiting.length, "source is", "sources are"))} in iCloud and not read yet (${esc(size(bytes))}).
          <button class="st-link" data-study-get-all>Download ${waiting.length === 1 ? "it" : "them"}</button></div>` : "";
    const actions = readOnly() ? "" : `<div class="rs-actions st-nb-actions"><button class="cc-quiet" data-study-paste>Paste text</button>
        <button class="cc-quiet" data-study-pick>${icon("plus")}Add sources</button></div>`;
    return `${held}${head}${cloud}<div class="st-groups">${groups}</div>${actions}`;
  }

  function renderStage() {
    const nb = state.notebook;
    els.title.textContent = nb ? nb.title : "Notebook";
    if (!state.said) {
      els.stage.innerHTML = "";
      return;
    }
    if (!state.folder) {
      // Drawn once, so what is typed is kept.
      if (!els.stage.querySelector("[data-study-folder-form]")) els.stage.innerHTML = folderCardHtml();
      return;
    }
    if (state.creating || !nb) {
      const typing = els.stage.querySelector("[data-study-new-form]");
      if (typing && (state.creating || !state.list.length)) return;
      els.stage.innerHTML = homeHtml();
      /** @type {HTMLInputElement | null} */ (els.stage.querySelector('[name="title"]'))?.focus({ preventScroll: true });
      return;
    }
    // A form being typed into is left alone while its notebook is redrawn.
    if ((state.editing || state.pasting) && els.stage.querySelector("form")) return;
    els.stage.innerHTML = notebookHtml();
  }

  function renderStudio() {
    els.studio.innerHTML = state.notebook ? `<p class="st-faint st-pad">What is made from your sources is kept here.</p>` : "";
  }

  function render() {
    renderList();
    renderSources();
    renderWhere();
    renderStage();
    renderStudio();
  }

  /* ── what is clicked ───────────────────────────────────────────── */

  /** Bring the sources being asked about down from iCloud. */
  function download() {
    const sources = (state.notebook?.sources ?? []).filter((s) => state.getting?.includes(s.id) && s.status.state === "inCloud");
    const paths = sources.map((s) => (s.origin.from === "file" ? `${s.origin.root}/${s.origin.rel}` : "")).filter(Boolean);
    state.getting = null;
    if (paths.length) host.dispatch({ type: "studyDownload", paths });
    renderSources();
  }

  ws.addEventListener("click", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    if (t.closest("[data-study-picker]")) return;
    const at = (/** @type {string} */ sel) => /** @type {HTMLElement | null} */ (t.closest(sel));
    const open = at("[data-study-open]");
    if (open) {
      state.creating = false;
      state.editing = false;
      state.pasting = false;
      host.dispatch({ type: "studyOpen", slug: open.dataset.studyOpen });
      return;
    }
    if (at("[data-study-new]")) {
      if (!state.folder) return renderStage();
      state.creating = true;
      state.editing = false;
      state.pasting = false;
      return render();
    }
    if (at("[data-study-new-cancel]")) {
      state.creating = false;
      return render();
    }
    if (at("[data-study-pick]")) return picker.open();
    if (at("[data-study-details]")) {
      state.editing = true;
      return renderStage();
    }
    if (at("[data-study-details-cancel]")) {
      state.editing = false;
      return renderStage();
    }
    if (at("[data-study-paste]")) {
      state.pasting = true;
      renderStage();
      /** @type {HTMLTextAreaElement | null} */ (els.stage.querySelector("textarea"))?.focus();
      return;
    }
    if (at("[data-study-paste-cancel]")) {
      state.pasting = false;
      return renderStage();
    }
    const get = at("[data-study-get]");
    if (get) {
      state.getting = [get.dataset.studyGet ?? ""];
      return renderSources();
    }
    if (at("[data-study-get-all]")) {
      state.getting = (state.notebook?.sources ?? []).filter((s) => s.status.state === "inCloud").map((s) => s.id);
      return renderSources();
    }
    if (at("[data-study-get-yes]")) return download();
    if (at("[data-study-get-no]")) {
      state.getting = null;
      return renderSources();
    }
    const refresh = at("[data-study-refresh]");
    if (refresh) return host.dispatch({ type: "studyRefresh", id: refresh.dataset.studyRefresh });
    const remove = at("[data-study-remove]");
    if (remove) {
      state.removing = remove.dataset.studyRemove ?? null;
      return renderSources();
    }
    const yes = at("[data-study-remove-yes]");
    if (yes) {
      state.removing = null;
      host.dispatch({ type: "studyRemove", id: yes.dataset.studyRemoveYes });
      return;
    }
    if (at("[data-study-remove-no]")) {
      state.removing = null;
      return renderSources();
    }
    if (at("[data-study-where-change]")) {
      state.moving = state.folder ?? SUGGESTED;
      return renderWhere();
    }
    if (at("[data-study-where-cancel]")) {
      state.moving = null;
      els.where.innerHTML = "";
      return renderWhere();
    }
  });

  ws.addEventListener("change", (e) => {
    const t = /** @type {HTMLInputElement | HTMLSelectElement} */ (e.target);
    if (t.closest("[data-study-picker]")) return;
    const nb = state.notebook;
    if (!nb) return;
    const tick = t.dataset.studyTick;
    if (tick !== undefined && t instanceof HTMLInputElement) {
      const ids = nb.sources.filter((s) => (s.id === tick ? t.checked : s.selected)).map((s) => s.id);
      host.dispatch({ type: "studySelect", ids });
      return;
    }
    if (t.matches("[data-study-all]") && t instanceof HTMLInputElement) {
      host.dispatch({ type: "studySelect", ids: t.checked ? nb.sources.filter(hasWords).map((s) => s.id) : [] });
      return;
    }
    const role = t.dataset.studyRole;
    if (role !== undefined) host.dispatch({ type: "studyRole", id: role, role: t.value });
  });

  ws.addEventListener("submit", (e) => {
    const form = /** @type {HTMLFormElement} */ (e.target);
    if (form.closest("[data-study-picker]")) return;
    e.preventDefault();
    const data = new FormData(form);
    const field = (/** @type {string} */ name) => String(data.get(name) ?? "").trim();
    if (form.matches("[data-study-folder-form]") || form.matches("[data-study-where-form]")) {
      const folder = field("folder");
      if (!folder) return;
      host.dispatch({ type: "studyFolder", folder });
      return;
    }
    if (form.matches("[data-study-new-form]")) {
      const title = field("title");
      if (!title) return /** @type {HTMLInputElement | null} */ (form.querySelector('[name="title"]'))?.focus();
      state.made = state.list.length;
      host.dispatch({ type: "studyNew", title });
      return;
    }
    if (form.matches("[data-study-details-form]")) {
      host.dispatch({ type: "studyDetails", title: field("title"), course: field("course"), exam: field("exam") || null });
      state.editing = false;
      return;
    }
    if (form.matches("[data-study-paste-form]")) {
      const text = field("text");
      if (!text) return /** @type {HTMLTextAreaElement | null} */ (form.querySelector("textarea"))?.focus();
      host.dispatch({ type: "studyPaste", title: field("title"), text });
      state.pasting = false;
    }
  });

  /* ── the core's answers ────────────────────────────────────────── */

  /** @param {ListEvent} event */
  function list(event) {
    const moved = state.folder !== event.folder;
    state.list = event.notebooks ?? [];
    state.folder = event.folder ?? null;
    state.said = true;
    if (moved) {
      state.moving = null;
      els.where.innerHTML = "";
    }
    if (state.creating && state.made >= 0 && state.list.length > state.made) {
      state.creating = false;
      state.made = -1;
    }
    state.open = event.open ?? null;
    if (!state.open) state.notebook = null;
    render();
  }

  /** @param {NotebookEvent} event */
  function notebook(event) {
    if (event.slug !== state.notebook?.slug) {
      state.editing = false;
      state.pasting = false;
      state.removing = null;
      state.getting = null;
    }
    state.notebook = event;
    state.open = event.slug;
    state.creating = false;
    render();
  }

  function closed() {
    state.notebook = null;
    state.open = null;
    picker.close();
    render();
  }

  render();

  return { list, notebook, closed, kb: picker.kb, kbFolder: picker.folder, kbResults: picker.results, pick: picker.open };
}

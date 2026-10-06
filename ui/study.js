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
import { markdown } from "./markdown.js";
import { createAudio } from "./study-audio.js";
import { createCards } from "./study-cards.js";
import { graphicHtml, mapSvg, pictureOf } from "./study-graphic.js";
import { answerHtml, createChat } from "./study-chat.js";
import { createPicker, KIND_ICON, size } from "./study-kb.js";
import { overviewClick, overviewHtml as learningHtml } from "./study-overview.js";
import { progressClick, progressHtml } from "./study-progress.js";
import { createQuiz } from "./study-quiz.js";
import { createSlides } from "./study-slides.js";
import { createStudio, madeHtml, taskHtml } from "./study-studio.js";

/** @typedef {import("./bindings/StudySummary").StudySummary} Summary */
/** @typedef {import("./bindings/StudySourceRow").StudySourceRow} SourceRow */
/** @typedef {import("./bindings/StudyNoteRow").StudyNoteRow} NoteRow */
/** @typedef {import("./bindings/StudyRole").StudyRole} Role */
/** @typedef {import("./bindings/StudyView").StudyView} View */
/** @typedef {import("./bindings/StudyPlace").StudyPlace} Place */
/** @typedef {{notebooks: Summary[], open: string | null, folder: string | null}} ListEvent */
/** @typedef {import("./bindings/StudySourceGuide").StudySourceGuide} Guide */
/** @typedef {{id: string, title: string, kind: string, at: Place | null, said: string | null, view: View, guide: Guide | null,
 *   citations: import("./bindings/StudyCitation").StudyCitation[]}} SourceEvent */
/** @typedef {{id: string, title: string, body: string, updated: number, source: string | null}} NoteEvent */
/** @typedef {import("./study-studio.js").MadeEvent} MadeEvent */
/** @typedef {import("./study-studio.js").TaskEvent} TaskEvent */
/** @typedef {import("./bindings/StudyArtifactRow").StudyArtifactRow} ArtifactRow */
/** @typedef {{kind: "chat"} | {kind: "overview"} | {kind: "progress"} | {kind: "source", source: SourceEvent} | {kind: "note", note: NoteEvent | null, preview: boolean}
 *   | {kind: "artifact", source: SourceEvent, id: string, seq: number, file: string}
 *   | {kind: "made", made: MadeEvent} | {kind: "task", task: TaskEvent} | {kind: "deck"} | {kind: "review"} | {kind: "quiz"}
 *   | {kind: "learning", learning: import("./study-overview.js").OverviewEvent} | {kind: "audio"} | {kind: "slides"}
 *   | {kind: "graphic", graphic: import("./study-graphic.js").GraphicEvent}} Shown */
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

/** Seconds as a video's clock reads them. @param {number} seconds */
function clock(seconds) {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600), m = Math.floor(s / 60) % 60, r = s % 60;
  const two = (/** @type {number} */ n) => String(n).padStart(2, "0");
  return h ? `${h}:${two(m)}:${two(r)}` : `${m}:${two(r)}`;
}

/** Seconds, as minutes or hours and minutes. @param {number} seconds */
function minutes(seconds) {
  const m = Math.max(1, Math.round(seconds / 60));
  return m < 60 ? `${m} min` : `${Math.floor(m / 60)} h ${m % 60} min`;
}

/** Dollars, to the cent, and never nothing. @param {number} amount */
function dollars(amount) {
  return amount < 0.01 ? "under a cent" : `$${amount.toFixed(2)}`;
}

/** What a source's units count, in words. @param {SourceRow} s */
function extent(s) {
  if (!hasWords(s)) return "";
  if (s.units === "pages") return count(s.count, "page", "pages");
  if (s.units === "seconds") return `${Math.round(s.count / 60)} min`;
  return count(s.count, "line", "lines");
}

/** @param {{root: Document, dispatch: (command: any) => unknown, shell?: boolean}} host */
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
  const picker = createPicker({ root: host.root, dispatch: host.dispatch, ws, discoverable: () => !readOnly() && !!state.notebook?.sources.some(hasWords) });
  const chat = createChat({
    dispatch: host.dispatch, ws, readOnly: () => readOnly(),
    show: (id, at) => host.dispatch({ type: "studyShow", id, at }),
    // The citations of something made, a deck or a card, for their chips.
    lookup: (key) => {
      if (key !== "made") return [];
      const shown = state.shown;
      if (shown.kind === "made") return shown.made.citations;
      if (shown.kind === "quiz") return quizzes.citations();
      if (shown.kind === "learning") return shown.learning.citations;
      if (shown.kind === "audio") return sounds.citations();
      if (shown.kind === "slides") return decks.citations();
      if (shown.kind === "graphic") return shown.graphic.citations;
      if (shown.kind === "source") return shown.source.citations ?? [];
      return shown.kind === "deck" || shown.kind === "review" ? cards.citations(shown.kind) : [];
    },
  });
  const studio = createStudio({ box: els.studio, dispatch: host.dispatch, readOnly: () => readOnly(),
    pastPapers: () => (state.notebook?.sources ?? []).filter((s) => s.role === "pastPaper" && hasWords(s)).map((s) => ({ id: s.id, title: s.title })) });
  /** Ask the tutor, in the chat: a card explained, a question, how a quiz went. @param {string} text */
  const askTutor = (text) => {
    state.shown = { kind: "chat" };
    state.back = "chat";
    els.stage.innerHTML = "";
    render();
    host.dispatch({ type: "studyAsk", text });
  };
  const cards = createCards({ dispatch: host.dispatch, readOnly: () => readOnly(), explain: askTutor });
  const quizzes = createQuiz({ dispatch: host.dispatch, readOnly: () => readOnly(), ask: askTutor });
  const sounds = createAudio({ dispatch: host.dispatch, readOnly: () => readOnly() });
  const decks = createSlides({ dispatch: host.dispatch, readOnly: () => readOnly() });

  /** @type {{list: Summary[], open: string | null, folder: string | null, said: boolean, notebook: NotebookEvent | null,
   *   creating: boolean, made: number, moving: string | null, editing: boolean, pasting: false | "text" | "web" | "artifacts", removing: string | null,
   *   getting: string[] | null, shown: Shown, back: "chat" | "overview" | "progress", want: string | null,
   *   progress: import("./study-progress.js").ProgressEvent | null, unpicked: Set<string>, hearing: string | null, readable: string | null,
   *   artifacts: ArtifactRow[] | null, picked: Set<string>}} */
  const state = {
    list: [], open: null, folder: null,
    // Whether the core has said where notebooks are kept — `null` until it has.
    said: false,
    notebook: null,
    // The new-notebook form is up, and how many there were when it was sent.
    creating: false, made: -1,
    // The folder being typed to keep notebooks in, once asked for.
    moving: null,
    // The notebook's details being edited; text, or a web address, being
    // given; a source whose taking out is being confirmed; sources whose
    // download from iCloud is being confirmed.
    editing: false, pasting: false, removing: null, getting: null,
    // What the middle shows: the chat, the sources, a source in the
    // reader, a note — and what Back returns to.
    shown: { kind: "chat" },
    back: "chat",
    // Something made that was clicked, to be shown when the core sends it.
    want: null,
    // Where the student stands, as the core last said.
    progress: null,
    // What was found on the web that is not to be added, of what is shown.
    unpicked: new Set(),
    // A recording whose transcribing — and what it costs — is being asked.
    hearing: null,
    // A source whose pages with few words are to be read from their
    // pictures, being asked.
    readable: null,
    // The artifacts Proton keeps, once asked for, and those ticked to add.
    artifacts: null,
    picked: new Set(),
  };

  /** Where an artifact's kept pages are: Proton's own scheme in the app,
   *  the preview's fixtures without it. */
  const artifactBase = () => !host.shell ? "/fixtures/artifacts/" : /Win/.test(navigator.platform) ? "http://artifact.localhost/" : "artifact://localhost/";

  /** The notebook's pages of the web, by address: what was found that it has. */
  const webSources = () => new Map((state.notebook?.sources ?? []).flatMap((s) => (s.origin.from === "web" ? [[s.origin.url, s.id]] : [])));

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
      const due = n.due ? `<span class="st-due" title="${esc(count(n.due, "card", "cards"))} due today">${n.due}</span>` : "";
      return `<button class="cc-session st-item${on ? " on" : ""}" data-study-open="${esc(n.slug)}"${on ? ' aria-current="true"' : ""}>` +
        `<span class="cc-session-name">${esc(n.title)}</span><span class="cc-session-where">${esc(sub)}</span>${due}</button>`;
    }).join("");
  }

  /* ── the sources ───────────────────────────────────────────────── */

  /** @param {SourceRow} s */
  function statusHtml(s) {
    const st = s.status;
    // Pages with few words: read from their pictures, on asking.
    if (st.state === "ready" && s.kind === "pdf" && s.thin && !readOnly()) {
      const reading = studio.jobs().some((j) => j.kind === "readable" && j.about === s.id && j.state !== "done" && j.state !== "failed");
      return reading ? `<span class="st-badge busy"><span class="lab-spin"></span>Reading pages</span>`
        : `<button class="st-badge" data-study-readable="${esc(s.id)}" title="${esc(count(s.thin, "page has", "pages have"))} few words of its own: read them from their pictures">${esc(count(s.thin, "thin page", "thin pages"))}</button>`;
    }
    switch (st.state) {
      case "reading": return `<span class="st-badge busy"><span class="lab-spin"></span>Reading</span>`;
      case "downloading": return `<span class="st-badge busy"><span class="lab-spin"></span>Downloading</span>`;
      case "inCloud": return `<button class="st-badge cloud" data-study-get="${esc(s.id)}" title="In iCloud — ${esc(size(s.size))} to download">${icon("cloud")}Download</button>`;
      case "failed": return s.kind === "audio" || s.kind === "video"
        ? `<button class="st-badge bad" data-study-transcribe="${esc(s.id)}" title="${esc(st.why)} — transcribe it again">Not transcribed</button>`
        : `<span class="st-badge bad" title="${esc(st.why)}">Not read</span>`;
      case "missing": return `<span class="st-badge" title="Its file is gone; what was read of it is kept">File gone</span>`;
      case "changed": return `<button class="st-badge" data-study-refresh="${esc(s.id)}" title="Its file has changed since it was read">Read again</button>`;
      case "untranscribed": return `<button class="st-badge" data-study-transcribe="${esc(s.id)}" title="Transcribe it with Fish Audio: you are told what it costs first">` +
        `${icon("mic")}Transcribe${st.seconds ? ` · ${esc(minutes(st.seconds))}` : ""}</button>`;
      case "transcribing": return `<span class="st-badge busy"><span class="lab-spin"></span>Transcribing</span>`;
      default: return "";
    }
  }

  /** @param {SourceRow} s */
  function sourceHtml(s) {
    const origin = s.origin.from === "file" ? `${s.origin.root}/${s.origin.rel}` : s.origin.from === "note" ? `Note ${s.origin.note}`
      : s.origin.from === "web" ? s.origin.url : s.origin.from === "artifact" ? `Artifact, version ${s.origin.seq}` : "Pasted";
    const confirm = state.removing === s.id
      ? `<span class="st-confirm">Take out? <button class="st-link" data-study-remove-yes="${esc(s.id)}">Yes</button> <button class="st-link" data-study-remove-no>No</button></span>`
      : "";
    const thin = s.thin ? ` · ${s.thin} thin` : "";
    const labels = s.labels.length ? `<span class="st-labels">${s.labels.map((l) => `<span class="st-label">${esc(l)}</span>`).join("")}</span>` : "";
    return `<div class="st-source${s.selected ? " on" : ""}" data-study-source="${esc(s.id)}" title="${esc(`${s.id} · ${origin}`)}">` +
      `<input type="checkbox" data-study-tick="${esc(s.id)}"${s.selected ? " checked" : ""}${hasWords(s) && !readOnly() ? "" : " disabled"} aria-label="Use ${esc(s.title)}">` +
      `${icon(KIND_ICON[s.kind] ?? "file")}` +
      `<span class="st-source-text"><button class="st-source-name" data-study-show="${esc(s.id)}" title="Read it">${esc(s.title)}</button>` +
      `<span class="st-faint">${esc(s.id)}${extent(s) ? ` · ${esc(extent(s))}${esc(thin)}` : ""}</span>${labels}</span>` +
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
    const labelling = studio.jobs().some((j) => j.kind === "labels" && (j.state === "running" || j.state === "waiting" || j.state === "repairing"));
    const label = readOnly() || usable.length < 2 ? ""
      : labelling ? `<span class="st-faint"><span class="lab-spin"></span> Labelling</span>`
        : `<button class="cc-quiet" data-study-label-all title="Label every source with its topics, in one call, to tick them by topic">Label</button>`;
    const head = `<header class="st-sources-head"><b>Sources</b><span class="st-faint">${ticked} of ${usable.length} used</span><span class="grow"></span>${label}` +
      (readOnly() ? "" : `<button class="cc-quiet" data-study-pick title="Add sources from your folders">${icon("plus")}Add</button>`) + `</header>`;
    // Every topic a source is labelled with: each ticks the sources about it.
    const topics = [...new Set(usable.flatMap((s) => s.labels))].sort((a, b) => a.localeCompare(b));
    const labelBar = topics.length && !readOnly()
      ? `<div class="st-label-bar">${topics.map((t) => `<button class="st-chip" data-study-label-pick="${esc(t)}" title="Use only the sources about ${esc(t)}">${esc(t)}</button>`).join("")}</div>` : "";
    const tickAll = usable.length > 1 && !readOnly()
      ? `<label class="st-all"><input type="checkbox" data-study-all${all ? " checked" : ""}> Use every source</label>` : "";
    const rows = nb.sources.length
      ? nb.sources.map(sourceHtml).join("")
      : `<p class="st-faint st-pad">No sources yet.</p>`;
    const notes = nb.notes.map((n) => {
      const on = state.shown.kind === "note" && state.shown.note?.id === n.id;
      return `<button class="st-note${on ? " on" : ""}" data-study-show-note="${esc(n.id)}" title="${esc(n.id)}">` +
        `<span class="st-source-name">${esc(n.title)}</span><span class="st-faint">${esc(n.preview)}</span></button>`;
    }).join("");
    const notesHead = `<header class="st-sources-head st-notes-head"><b>Notes</b><span class="st-faint">${nb.notes.length || ""}</span><span class="grow"></span>` +
      (readOnly() ? "" : `<button class="cc-quiet" data-study-new-note title="Write a note">${icon("plus")}Note</button>`) + `</header>`;
    els.sources.innerHTML = `${head}${confirmHtml()}${labelBar}${tickAll}<div class="scroll st-source-list">${rows}${notesHead}` +
      `${notes || `<p class="st-faint st-pad">No notes yet.</p>`}</div>`;
  }

  /** Bringing sources down from iCloud is said before it is done: what
   *  comes down, and how much. So is transcribing a recording: what is sent
   *  to Fish, and what it costs. */
  function confirmHtml() {
    const thin = (state.notebook?.sources ?? []).find((s) => s.id === state.readable);
    if (thin) {
      const calls = Math.ceil(thin.thin / 8);
      return `<div class="st-ask">${icon("image")}<span>Read <b>${esc(thin.title)}</b>'s ${esc(count(thin.thin, "page", "pages"))} with few words from their pictures? ` +
        `${thin.thin === 1 ? "It is" : "They are"} drawn here and sent to the tutor's model, eight to a call — ${esc(count(calls, "call", "calls"))}.</span>` +
        `<span class="rs-actions"><button class="cc-quiet" data-study-readable-no>Cancel</button><button class="cc-primary" data-study-readable-yes>Read them</button></span></div>`;
    }
    const hearing = (state.notebook?.sources ?? []).find((s) => s.id === state.hearing);
    if (hearing) {
      const seconds = hearing.status.state === "untranscribed" ? hearing.status.seconds : hearing.count;
      const cost = seconds ? `about ${dollars(seconds / 3600 * 0.36)} of your Fish credit, at 36¢ an hour of sound` : "36¢ an hour of sound, from your Fish credit";
      const long = seconds ? `this ${esc(minutes(seconds))} recording` : "this recording";
      return `<div class="st-ask">${icon("mic")}<span>Send ${long}'s sound — <b>${esc(hearing.title)}</b> — to Fish Audio to be transcribed? It costs ${cost}.</span>
        <span class="rs-actions"><button class="cc-quiet" data-study-transcribe-no>Cancel</button><button class="cc-primary" data-study-transcribe-yes>Transcribe</button></span></div>`;
    }
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
      `<b>${esc(n.title)}</b><span class="st-faint">${esc([n.course, count(n.sources, "source", "sources"), n.exam ? `exam ${n.exam}` : "", n.due ? `${count(n.due, "card", "cards")} due` : ""].filter(Boolean).join(" · "))}</span></button>`).join("");
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

  function webHtml() {
    return `<form class="st-card st-paste" data-study-web-form novalidate>
        <h2>Add from the web</h2>
        <label class="rs-field"><span>Address</span><input name="url" type="url" autocomplete="off" spellcheck="false" placeholder="https://…"></label>
        <p class="st-faint">A page is fetched once and kept as the text of its article; a PDF is kept whole; a YouTube video is kept as its
          captions, read with yt-dlp. Nothing is fetched again unless you ask.</p>
        <div class="rs-actions"><button type="button" class="cc-quiet" data-study-paste-cancel>Cancel</button>
          <button type="submit" class="cc-primary">Add as a source</button></div>
      </form>`;
  }

  /** The artifacts Proton keeps, to tick and add as sources. */
  function artifactsHtml() {
    const list = state.artifacts;
    const rows = !list ? `<p class="st-faint"><span class="lab-spin"></span> Looking…</p>`
      : !list.length ? `<p class="st-faint">No artifacts are kept yet. What Claude publishes from the Chat tab is kept, and can be imported there from Claude.</p>`
        : list.map((a) => {
          const can = !a.source && Boolean(a.file);
          const when = a.at ? new Date(a.at).toLocaleDateString(undefined, { day: "numeric", month: "short" }) : "";
          const note = a.source ? `already ${esc(a.source)}` : !a.file ? "only its address is kept" : `version ${a.seq}${when ? ` · ${esc(when)}` : ""}`;
          return `<label class="st-file${can ? "" : " off"}"><input type="checkbox" data-study-artifact-pick="${esc(a.id)}"${can ? (state.picked.has(a.id) ? " checked" : "") : " disabled"}>` +
            `${icon("layers")}<span class="st-file-name">${esc(a.title)}</span><span class="st-faint st-size">${note}</span></label>`;
        }).join("");
    const picked = [...state.picked].filter((id) => list?.some((a) => a.id === id && !a.source && a.file)).length;
    return `<form class="st-card st-paste st-artifacts" data-study-artifacts-form novalidate>
        <h2>Add your artifacts</h2>
        <p class="st-faint">Pages Claude published, as Proton keeps them: each is read as its article, its formulas as TeX, and opens as itself in the reader.</p>
        <div class="st-artifact-list">${rows}</div>
        <div class="rs-actions"><button type="button" class="cc-quiet" data-study-paste-cancel>Cancel</button>
          <button type="submit" class="cc-primary"${picked ? "" : " disabled"}>Add ${picked || ""} as ${picked === 1 ? "a source" : "sources"}</button></div>
      </form>`;
  }

  function notebookHtml() {
    const nb = /** @type {NotebookEvent} */ (state.notebook);
    const held = nb.readOnly ? `<div class="st-banner">${icon("users")} ${esc(nb.readOnly)} has this notebook open: it is read only here.</div>` : "";
    if (state.editing) return held + detailsHtml();
    if (state.pasting) return held + (state.pasting === "web" ? webHtml() : state.pasting === "artifacts" ? artifactsHtml() : pasteHtml());
    const head = headHtml();
    if (!nb.sources.length) {
      return `${held}${head}<div class="st-card st-empty">
          <h2>Add sources to begin</h2>
          <p>Choose notes, slides, labs and past papers from your folders, add a page from the web, or paste text. Every answer
            and everything the studio makes is drawn from them, and cites them.</p>
          ${readOnly() ? "" : `<div class="rs-actions"><button class="cc-quiet" data-study-paste>Paste text</button>
            <button class="cc-quiet" data-study-web>${icon("globe")}From the web</button>
            <button class="cc-quiet" data-study-artifacts>${icon("layers")}Your artifacts</button>
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
        <button class="cc-quiet" data-study-web>${icon("globe")}From the web</button>
        <button class="cc-quiet" data-study-artifacts>${icon("layers")}Your artifacts</button>
        <button class="cc-quiet" data-study-pick>${icon("plus")}Add sources</button></div>`;
    return `${held}${head}${cloud}<div class="st-groups">${groups}</div>${actions}`;
  }

  /** The notebook's name, and Chat and Sources to move between. */
  function headHtml() {
    const nb = /** @type {NotebookEvent} */ (state.notebook);
    const facts = [nb.course, nb.exam ? `exam on ${nb.exam}` : ""].filter(Boolean).join(" · ");
    const on = (/** @type {string} */ kind) => (state.shown.kind === kind ? " on" : "");
    const usable = nb.sources.filter(hasWords).length;
    const tabs = `<div class="rs-tabs st-tabs" role="tablist" aria-label="Notebook">` +
      `<button class="rs-tab${on("chat")}" role="tab" aria-selected="${state.shown.kind === "chat"}" data-study-view="chat">Chat</button>` +
      `<button class="rs-tab${on("overview")}" role="tab" aria-selected="${state.shown.kind === "overview"}" data-study-view="overview">Sources<span class="rs-count">${usable || ""}</span></button>` +
      `<button class="rs-tab${on("progress")}" role="tab" aria-selected="${state.shown.kind === "progress"}" data-study-view="progress">Progress</button></div>`;
    const due = state.list.find((n) => n.slug === nb.slug)?.due ?? 0;
    const review = readOnly() ? ""
      : cards.active() ? `<button class="cc-primary" data-study-resume title="The session of flashcards under way">${icon("cards")}Back to review</button>`
        : due ? `<button class="cc-primary" data-study-review="due" title="Every deck's cards due today">${icon("cards")}Review ${due}</button>` : "";
    return `<div class="st-nb-head"><div><h1>${esc(nb.title)}</h1>` +
      `<span class="st-faint">${esc(facts || "No course or exam day yet")}</span></div>${tabs}${review}` +
      (readOnly() ? "" : `<button class="cc-quiet" data-study-details>Edit</button>`) + `</div>`;
  }

  /** A source, as its words were kept. @param {SourceEvent} src */
  function readerHtml(src) {
    const v = src.view;
    let body = "";
    if (v.shape === "sections") {
      body = v.sections.map((s) => `<section class="st-sec md" data-from="${s.from}" data-to="${s.to}">${markdown(s.text)}</section>`).join("");
    } else if (v.shape === "lines") {
      body = `<div class="st-lines">${v.text.split("\n").map((line, i) =>
        `<div class="st-ln" data-line="${i + 1}"><i>${i + 1}</i><span>${esc(line) || " "}</span></div>`).join("")}</div>`;
    } else if (v.shape === "pages") {
      const unit = src.kind === "slides" ? "Slide" : "Page";
      body = v.pages.map((page, i) => `<section class="st-page" data-page="${i + 1}"><header>${unit} ${i + 1}</header>` +
        (page.trim() ? `<pre>${esc(page.trim())}</pre>` : `<p class="st-faint">No words were read on this page — a picture, or a slide with only a title.</p>`) +
        `</section>`).join("");
    } else {
      body = v.cues.map((c) => `<div class="st-cue" data-start="${c.start}" data-end="${c.end}"><time>${clock(c.start)}</time><span>${esc(c.text)}</span></div>`).join("");
    }
    const said = src.said ? `<div class="st-banner">${icon("clock")} ${esc(src.said)}</div>` : "";
    const row = state.notebook?.sources.find((s) => s.id === src.id);
    const origin = row?.origin;
    // Its guide, or the asking for one.
    const g = src.guide;
    const guiding = studio.jobs().some((j) => j.kind === "sourceGuide" && j.about === src.id && j.state !== "done" && j.state !== "failed");
    const guide = g
      ? `<section class="st-guide" data-turn="made"><header><b>Source guide</b><span class="st-faint">${esc(g.by)}</span></header>` +
        `<div class="md">${answerHtml(g.summary, src.citations ?? [])}</div>` +
        `<div class="st-guide-topics">${g.topics.map((t) => `<button class="st-chip" data-study-ask-topic="${esc(t)}" title="Ask the tutor what it says of this">${esc(t)}</button>`).join("")}</div>` +
        (g.questions.length ? `<ul class="st-guide-questions">${g.questions.map((q) => `<li><button class="st-link" data-study-ask-question="${esc(q)}">${esc(q)}</button></li>`).join("")}</ul>` : "") +
        `</section>`
      : guiding ? `<div class="st-banner"><span class="lab-spin"></span> Writing its guide…</div>` : "";
    const ask = !g && !guiding && !readOnly() && row && hasWords(row)
      ? `<button class="cc-quiet" data-study-guide="${esc(src.id)}" title="What it covers, its key topics, and questions to ask of it">${icon("spark")}Guide</button>` : "";
    const web = origin?.from === "web" ? origin : null;
    const fetched = web?.fetched ? ` · fetched ${new Date(web.fetched * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" })}` : "";
    const page = web ? `<button class="cc-quiet" data-study-page="${esc(web.url)}" title="${esc(web.url)}">${icon("ext")}Open the page</button>` +
      (readOnly() ? "" : `<button class="cc-quiet" data-study-refresh="${esc(src.id)}" title="Fetch it again, and keep it as it is now">Fetch again</button>`)
      : origin?.from === "artifact" ? `<button class="cc-quiet" data-study-artifact-open title="The page itself, as Claude published it: its figures, its videos">${icon("layers")}Open the artifact</button>` +
        (readOnly() ? "" : `<button class="cc-quiet" data-study-refresh="${esc(src.id)}" title="Read its latest version again">Read again</button>`)
        : "";
    return `<div class="st-reader"><div class="st-reader-head"><button class="cc-quiet" data-study-back title="Back to the notebook">← Back</button>` +
      `<div><h1>${esc(src.title)}</h1><span class="st-faint">${esc(src.id)} · ${esc(src.kind)}${esc(fetched)}</span></div>${ask}${page}</div>${said}${guide}<div class="st-read">${body}</div></div>`;
  }

  /** Mark and scroll to where a source was opened. @param {SourceEvent} src */
  function reach(src) {
    const at = src.at;
    if (!at) return;
    /** @type {HTMLElement[]} */
    let marks = [];
    const all = (/** @type {string} */ sel) => /** @type {HTMLElement[]} */ ([...els.stage.querySelectorAll(sel)]);
    if (src.view.shape === "sections") marks = all(".st-sec").filter((e) => Number(e.dataset.from) <= at.to && Number(e.dataset.to) >= at.from);
    else if (src.view.shape === "lines") marks = all(".st-ln").filter((e) => Number(e.dataset.line) >= at.from && Number(e.dataset.line) <= at.to);
    else if (src.view.shape === "pages") marks = all(".st-page").filter((e) => Number(e.dataset.page) >= at.from && Number(e.dataset.page) <= at.to);
    else marks = all(".st-cue").filter((e) => Number(e.dataset.start) < at.to + 1 && Number(e.dataset.end) > at.from);
    marks.forEach((e) => e.classList.add("at"));
    marks[0]?.scrollIntoView({ block: "center" });
  }

  /** A note, to write. @param {NoteEvent | null} note @param {boolean} preview */
  function noteHtml(note, preview) {
    const ro = readOnly() ? " disabled" : "";
    const made = note?.source ? `<span class="st-faint">Used by the tutor as ${esc(note.source)}</span>`
      : note && !readOnly() ? `<button type="button" class="cc-quiet" data-study-note-source title="Let the tutor read it too">Use as a source</button>` : "";
    const remove = note && !readOnly() ? `<button type="button" class="cc-quiet" data-study-note-delete>Put in the bin</button>` : "";
    const body = preview
      ? `<div class="st-note-preview md">${markdown(note?.body ?? "")}</div>`
      : `<textarea name="body" rows="18" spellcheck="true" placeholder="Markdown, and TeX between dollars: $O(b^d)$"${ro}>${esc(note?.body ?? "")}</textarea>`;
    return `<form class="st-card st-note-form" data-study-note-form data-id="${esc(note?.id ?? "")}" novalidate>
        <div class="st-reader-head"><button type="button" class="cc-quiet" data-study-back>← Back</button>
          <input name="title" value="${esc(note?.title ?? "")}" placeholder="A title — or its first line" autocomplete="off"${ro}>
          <button type="button" class="cc-quiet" data-study-note-preview>${preview ? "Write" : "Preview"}</button></div>
        ${body}
        <div class="rs-actions st-note-actions">${remove}${made}<span class="grow"></span>${readOnly() ? "" : `<button type="submit" class="cc-primary">Save</button>`}</div>
      </form>`;
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
    const shown = state.shown;
    if (shown.kind === "chat" && !state.editing && !state.pasting) {
      const nbox = /** @type {NotebookEvent} */ (nb);
      const held = nbox.readOnly ? `<div class="st-banner">${icon("users")} ${esc(nbox.readOnly)} has this notebook open: it is read only here.</div>` : "";
      // The head is drawn again; the chat below keeps what is being typed.
      let box = /** @type {HTMLElement | null} */ (els.stage.querySelector("[data-study-chat-box]"));
      if (!box) {
        els.stage.innerHTML = `<div data-study-nb-head></div><div class="st-chat-box" data-study-chat-box></div>`;
        box = /** @type {HTMLElement} */ (els.stage.querySelector("[data-study-chat-box]"));
      }
      /** @type {HTMLElement} */ (els.stage.querySelector("[data-study-nb-head]")).innerHTML = held + headHtml();
      if (!nbox.sources.length && !chat.has()) {
        box.innerHTML = "";
        return;
      }
      chat.render(box, els.stage);
      return;
    }
    if (shown.kind === "source") {
      els.stage.innerHTML = readerHtml(shown.source);
      reach(shown.source);
      return;
    }
    if (shown.kind === "artifact") {
      // Drawn once: a page playing a video is not loaded again under it.
      const src = `${artifactBase()}${encodeURIComponent(shown.id)}/${shown.seq}/${shown.file.split("/").map(encodeURIComponent).join("/")}`;
      if (els.stage.querySelector(".st-artifact-frame")?.getAttribute("src") === src) return;
      els.stage.innerHTML = `<div class="st-reader st-artifact-view"><div class="st-reader-head"><button class="cc-quiet" data-study-artifact-back title="Back to its words">← Back</button>` +
        `<div><h1>${esc(shown.source.title)}</h1><span class="st-faint">${esc(shown.source.id)} · artifact, version ${shown.seq}</span></div></div>` +
        `<iframe class="st-artifact-frame" title="${esc(shown.source.title)}" src="${esc(src)}" sandbox="allow-scripts allow-forms allow-modals allow-popups allow-downloads" ` +
        `allow="fullscreen; picture-in-picture; autoplay" allowfullscreen referrerpolicy="no-referrer"></iframe></div>`;
      return;
    }
    if (shown.kind === "note") {
      if (!shown.preview && els.stage.querySelector("[data-study-note-form] textarea")) return;
      els.stage.innerHTML = noteHtml(shown.note, shown.preview);
      return;
    }
    if (shown.kind === "made") {
      els.stage.innerHTML = madeHtml(shown.made, readOnly(), { have: webSources(), unpicked: state.unpicked });
      return;
    }
    if (shown.kind === "task") {
      els.stage.innerHTML = taskHtml(shown.task);
      return;
    }
    if (shown.kind === "deck" || shown.kind === "review") {
      cards.render(els.stage, shown.kind);
      return;
    }
    if (shown.kind === "quiz") {
      quizzes.render(els.stage);
      return;
    }
    if (shown.kind === "learning") {
      els.stage.innerHTML = learningHtml(shown.learning, readOnly());
      return;
    }
    if (shown.kind === "audio") {
      sounds.render(els.stage);
      return;
    }
    if (shown.kind === "slides") {
      decks.render(els.stage);
      return;
    }
    if (shown.kind === "graphic") {
      els.stage.innerHTML = graphicHtml(shown.graphic, readOnly());
      return;
    }
    if (shown.kind === "progress") {
      const nbox = /** @type {NotebookEvent} */ (nb);
      const held = nbox.readOnly ? `<div class="st-banner">${icon("users")} ${esc(nbox.readOnly)} has this notebook open: it is read only here.</div>` : "";
      els.stage.innerHTML = held + headHtml() + progressHtml(state.progress, readOnly());
      return;
    }
    els.stage.innerHTML = notebookHtml();
  }

  function renderStudio() {
    if (state.notebook) studio.render();
    else els.studio.innerHTML = "";
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
    if (t.closest(".st-chat") || t.closest("[data-study-cite]")) {
      chat.click(t);
      return;
    }
    const opened = /** @type {HTMLElement | null} */ (t.closest("[data-study-made]"));
    if (opened) state.want = opened.dataset.studyMade ?? null;
    if (studio.click(t)) return;
    if (els.stage.contains(t) && cards.click(t, () => renderStage())) return;
    if (els.stage.contains(t) && quizzes.click(t, () => renderStage())) return;
    if (els.stage.contains(t) && state.shown.kind === "learning" && overviewClick(t, state.shown.learning)) return;
    if (els.stage.contains(t) && state.shown.kind === "audio" && sounds.click(t, els.stage, () => { els.stage.innerHTML = ""; renderStage(); })) return;
    if (els.stage.contains(t) && state.shown.kind === "slides" && decks.click(t, () => renderStage())) return;
    const picture = /** @type {HTMLElement | null} */ (t.closest("[data-study-picture]"));
    if (picture && els.stage.contains(picture)) {
      const shown = state.shown;
      const svg = shown.kind === "graphic" ? els.stage.querySelector("[data-study-graphic] svg")?.outerHTML
        : shown.kind === "made" && shown.made.body.shape === "map" ? mapSvg(shown.made.made.title, shown.made.body.nodes) : undefined;
      if (svg) {
        picture.textContent = "Saving…";
        pictureOf(svg).then((png) => host.dispatch({ type: "studySaveImage", id: picture.dataset.studyPicture, png }))
          .catch(() => host.dispatch({ type: "studySaveImage", id: picture.dataset.studyPicture, png: "" }))
          .finally(() => { picture.textContent = "Save as PNG"; });
      }
      return;
    }
    if (els.stage.contains(t) && progressClick(t, host.dispatch)) return;
    const at_ = (/** @type {string} */ sel) => /** @type {HTMLElement | null} */ (t.closest(sel));
    // Something made, opened from the middle: a deck or a quiz an overview
    // or the progress names.
    const named = els.stage.contains(t) ? at_("[data-study-made]") : null;
    if (named) {
      state.want = named.dataset.studyMade ?? null;
      return host.dispatch({ type: "studyShowMade", id: named.dataset.studyMade });
    }
    if (at_("[data-study-resume]")) {
      state.shown = { kind: "review" };
      els.stage.innerHTML = "";
      return render();
    }
    const task = at_("[data-study-task]");
    if (task) return host.dispatch({ type: "studyShowTask", id: task.dataset.studyTask });
    // What was found on the web, added: the sources ticked, or the report.
    if (at_("[data-study-found-add]") && state.shown.kind === "made" && state.shown.made.body.shape === "found") {
      const have = webSources();
      const urls = state.shown.made.body.found.map((f) => f.url).filter((url) => !have.has(url) && !state.unpicked.has(url));
      if (urls.length) host.dispatch({ type: "studyAddUrls", urls });
      return;
    }
    if (at_("[data-study-found-report]") && state.shown.kind === "made" && state.shown.made.body.shape === "found") {
      const { made, body } = state.shown.made;
      const read = body.found.map((f, i) => `${i + 1}. ${f.title} — ${f.url}`).join("\n");
      host.dispatch({ type: "studyPaste", title: made.title, text: `${body.report}\n\n## Sources\n\n${read}\n` });
      return;
    }
    const bin = at_("[data-study-bin-made]");
    if (bin) {
      host.dispatch({ type: "studyBinMade", id: bin.dataset.studyBinMade });
      state.shown = { kind: state.back };
      els.stage.innerHTML = "";
      return render();
    }
    const fold = at_("[data-study-fold]");
    if (fold) {
      const node = /** @type {HTMLElement} */ (fold.closest(".st-map-node"));
      node.toggleAttribute("data-folded");
      return;
    }
    const about = at_("[data-study-ask-about]");
    if (about) {
      state.shown = { kind: "chat" };
      state.back = "chat";
      els.stage.innerHTML = "";
      render();
      host.dispatch({ type: "studyAsk", text: `Explain “${about.dataset.studyAskAbout}” from the sources.` });
      return;
    }
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
    const show = at("[data-study-show]");
    if (show) return host.dispatch({ type: "studyShow", id: show.dataset.studyShow, at: null });
    const showNote = at("[data-study-show-note]");
    if (showNote) return host.dispatch({ type: "studyShowNote", id: showNote.dataset.studyShowNote });
    if (at("[data-study-back]")) {
      state.shown = { kind: state.back };
      els.stage.innerHTML = "";
      return render();
    }
    const view = at("[data-study-view]");
    if (view) {
      const to = view.dataset.studyView;
      state.shown = { kind: to === "overview" ? "overview" : to === "progress" ? "progress" : "chat" };
      state.back = state.shown.kind === "overview" ? "overview" : state.shown.kind === "progress" ? "progress" : "chat";
      els.stage.innerHTML = "";
      if (state.shown.kind === "progress") host.dispatch({ type: "studyProgress" });
      return render();
    }
    if (at("[data-study-new-note]")) {
      state.shown = { kind: "note", note: null, preview: false };
      render();
      /** @type {HTMLTextAreaElement | null} */ (els.stage.querySelector("textarea"))?.focus();
      return;
    }
    if (at("[data-study-note-preview]") && state.shown.kind === "note") {
      // What is typed goes into the preview, unsaved.
      const form = /** @type {HTMLFormElement | null} */ (els.stage.querySelector("[data-study-note-form]"));
      const typed = form ? String(new FormData(form).get("body") ?? state.shown.note?.body ?? "") : "";
      const title = form ? String(new FormData(form).get("title") ?? "") : "";
      const note = state.shown.note ?? { id: "", title, body: "", updated: 0, source: null };
      state.shown = { kind: "note", note: state.shown.preview ? note : { ...note, title, body: typed }, preview: !state.shown.preview };
      els.stage.innerHTML = "";
      return renderStage();
    }
    const noteId = () => (state.shown.kind === "note" ? state.shown.note?.id : undefined);
    if (at("[data-study-note-source]") && noteId()) return host.dispatch({ type: "studyNoteSource", id: noteId() });
    if (at("[data-study-note-delete]") && noteId()) {
      host.dispatch({ type: "studyNoteDelete", id: noteId() });
      state.shown = { kind: state.back };
      els.stage.innerHTML = "";
      return render();
    }
    if (at("[data-study-details]")) {
      state.editing = true;
      return renderStage();
    }
    if (at("[data-study-details-cancel]")) {
      state.editing = false;
      return renderStage();
    }
    if (at("[data-study-paste]")) {
      state.pasting = "text";
      renderStage();
      /** @type {HTMLTextAreaElement | null} */ (els.stage.querySelector("textarea"))?.focus();
      return;
    }
    if (at("[data-study-artifacts]")) {
      state.pasting = "artifacts";
      state.artifacts = null;
      state.picked = new Set();
      host.dispatch({ type: "studyArtifacts" });
      return renderStage();
    }
    if (at("[data-study-artifact-open]") && state.shown.kind === "source") {
      const source = state.shown.source;
      const origin = state.notebook?.sources.find((s) => s.id === source.id)?.origin;
      if (origin?.from !== "artifact") return;
      state.shown = { kind: "artifact", source, id: origin.id, seq: origin.seq, file: origin.file };
      els.stage.innerHTML = "";
      return renderStage();
    }
    if (at("[data-study-artifact-back]") && state.shown.kind === "artifact") {
      state.shown = { kind: "source", source: state.shown.source };
      els.stage.innerHTML = "";
      return renderStage();
    }
    if (at("[data-study-web]")) {
      state.pasting = "web";
      renderStage();
      /** @type {HTMLInputElement | null} */ (els.stage.querySelector("[data-study-web-form] input"))?.focus();
      return;
    }
    const page = at("[data-study-page]");
    if (page) return host.dispatch({ type: "openLink", url: page.dataset.studyPage });
    // A source's guide asked for; its topics and questions asked of the tutor.
    const guide = at("[data-study-guide]");
    if (guide) {
      host.dispatch({ type: "studyMake", kind: "sourceGuide", options: { focus: "", ask: "", columns: [], amount: "standard", level: "medium", deck: null,
        objectives: [], source: guide.dataset.studyGuide, count: null, format: null, language: "", slides: null, slide: null } });
      return;
    }
    const topic = at("[data-study-ask-topic]");
    if (topic && state.shown.kind === "source") {
      const { id, title } = state.shown.source;
      return askTutor(`What does ${title} (${id}) say about ${topic.dataset.studyAskTopic}?`);
    }
    const question = at("[data-study-ask-question]");
    if (question) return askTutor(question.dataset.studyAskQuestion ?? "");
    if (at("[data-study-label-all]")) {
      host.dispatch({ type: "studyMake", kind: "labels", options: { focus: "", ask: "", columns: [], amount: "standard", level: "medium", deck: null,
        objectives: [], source: null, count: null, format: null, language: "", slides: null, slide: null } });
      return;
    }
    const pick = at("[data-study-label-pick]");
    if (pick && state.notebook) {
      const label = pick.dataset.studyLabelPick ?? "";
      return host.dispatch({ type: "studySelect", ids: state.notebook.sources.filter((s) => hasWords(s) && s.labels.includes(label)).map((s) => s.id) });
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
    const thin = at("[data-study-readable]");
    if (thin) {
      state.readable = thin.dataset.studyReadable ?? null;
      state.hearing = null;
      state.getting = null;
      return renderSources();
    }
    if (at("[data-study-readable-yes]")) {
      const id = state.readable;
      state.readable = null;
      if (id) host.dispatch({ type: "studyMakeReadable", id });
      return renderSources();
    }
    if (at("[data-study-readable-no]")) {
      state.readable = null;
      return renderSources();
    }
    const hear = at("[data-study-transcribe]");
    if (hear) {
      state.hearing = hear.dataset.studyTranscribe ?? null;
      state.getting = null;
      return renderSources();
    }
    if (at("[data-study-transcribe-yes]")) {
      const id = state.hearing;
      state.hearing = null;
      if (id) host.dispatch({ type: "studyTranscribe", id });
      return renderSources();
    }
    if (at("[data-study-transcribe-no]")) {
      state.hearing = null;
      return renderSources();
    }
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
    const artifact = t.dataset.studyArtifactPick;
    if (artifact !== undefined && t instanceof HTMLInputElement) {
      if (t.checked) state.picked.add(artifact);
      else state.picked.delete(artifact);
      return redrawArtifacts();
    }
    const pick = t.dataset.studyFoundPick;
    if (pick !== undefined && t instanceof HTMLInputElement) {
      if (t.checked) state.unpicked.delete(pick);
      else state.unpicked.add(pick);
      return renderStage();
    }
    if (chat.change(t)) return;
    if (t instanceof HTMLInputElement && cards.change(t)) return;
    if (state.shown.kind === "quiz" && quizzes.input(t)) return;
    if (state.shown.kind === "audio" && sounds.change(t, els.stage)) return;
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

  // What is written in a sitting is saved as it is written.
  ws.addEventListener("input", (e) => {
    if (state.shown.kind === "quiz") quizzes.input(/** @type {HTMLElement} */ (e.target));
  });

  ws.addEventListener("keydown", (e) => {
    if (state.shown.kind === "review" && cards.key(e, () => renderStage())) return;
    if (state.shown.kind === "slides" && decks.key(e, () => renderStage())) return;
    if (state.shown.kind === "quiz" && quizzes.key(e, () => renderStage())) return;
    chat.key(e);
  });

  ws.addEventListener("submit", (e) => {
    const form = /** @type {HTMLFormElement} */ (e.target);
    if (form.closest("[data-study-picker]")) return;
    if (chat.submit(/** @type {SubmitEvent} */ (e))) return;
    if (form.matches("[data-study-make-form]")) {
      e.preventDefault();
      studio.submit(form);
      return;
    }
    if (els.stage.contains(form) && (cards.submit(form) || quizzes.submit(form) || sounds.submit(form) || decks.submit(form, () => renderStage()))) {
      e.preventDefault();
      return;
    }
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
    if (form.matches("[data-study-note-form]")) {
      const id = form.dataset.id || null;
      const body = data.get("body") === null && state.shown.kind === "note" ? state.shown.note?.body ?? "" : field("body");
      if (!body && !field("title")) return /** @type {HTMLTextAreaElement | null} */ (form.querySelector("textarea"))?.focus();
      host.dispatch({ type: "studyNote", id, title: field("title"), body });
      return;
    }
    if (form.matches("[data-study-paste-form]")) {
      const text = field("text");
      if (!text) return /** @type {HTMLTextAreaElement | null} */ (form.querySelector("textarea"))?.focus();
      host.dispatch({ type: "studyPaste", title: field("title"), text });
      state.pasting = false;
    }
    if (form.matches("[data-study-artifacts-form]")) {
      const ids = [...state.picked].filter((id) => state.artifacts?.some((a) => a.id === id && !a.source && a.file));
      if (!ids.length) return;
      host.dispatch({ type: "studyAddArtifacts", ids });
      state.pasting = false;
      state.picked = new Set();
      renderStage();
    }
    if (form.matches("[data-study-web-form]")) {
      const url = field("url");
      if (!url) return /** @type {HTMLInputElement | null} */ (form.querySelector("input"))?.focus();
      host.dispatch({ type: "studyAddUrls", urls: [url] });
      state.pasting = false;
      renderStage();
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
      state.shown = { kind: "chat" };
      state.back = "chat";
    }
    state.notebook = event;
    state.open = event.slug;
    state.creating = false;
    render();
  }

  /** The open chat, whole. @param {import("./study-chat.js").ChatEvent} event */
  function chatEvent(event) {
    chat.chat(event);
    if (state.shown.kind === "chat") renderStage();
  }

  /** The answer coming. @param {{id: string, live: any}} event */
  function chatLive(event) {
    if (chat.live(event) && state.shown.kind === "chat") chat.renderLive();
  }

  /** Calls seen done, so a deck open is asked for again only when one adds to it. */
  const ended = new Set();
  /** The guides and labelling under way, as last drawn. */
  let underway = "";

  /** The studio's calls, and what they made. @param {import("./study-studio.js").StudioEvent} event */
  function studioEvent(event) {
    studio.studio(event);
    for (const job of event.jobs) {
      if (job.state !== "done" || ended.has(job.id)) continue;
      ended.add(job.id);
      // More cards for the deck being looked at: drawn with them.
      if (state.shown.kind === "deck" && job.made && job.made === cards.openDeck()) host.dispatch({ type: "studyShowMade", id: job.made });
      // The guide of the source being read: shown with it.
      if (job.kind === "sourceGuide" && state.shown.kind === "source" && state.shown.source.id === job.about) {
        host.dispatch({ type: "studyShow", id: job.about, at: state.shown.source.at });
      }
    }
    // A guide or labels under way show where they will be: drawn again
    // only when one of them changes, which keeps the reader where it is.
    const now = event.jobs.filter((j) => j.kind === "sourceGuide" || j.kind === "labels" || j.kind === "readable").map((j) => `${j.id}:${j.state}`).join(",");
    if (now !== underway) {
      underway = now;
      if (state.shown.kind === "source") renderStage();
      renderSources();
    }
  }

  /** Something made, opened in the middle. @param {MadeEvent} event */
  function made(event) {
    if (state.shown.kind !== "made" || state.shown.made.made.id !== event.made.id) state.unpicked = new Set();
    state.shown = { kind: "made", made: event };
    els.stage.innerHTML = "";
    render();
  }

  /** A deck of flashcards, opened in the middle. @param {import("./study-cards.js").DeckEvent} event */
  function deckEvent(event) {
    cards.deck(event);
    // Drawn again where it is; opened, unless a session is being shown.
    if (state.shown.kind !== "review") state.shown = { kind: "deck" };
    if (state.shown.kind === "deck") renderStage();
  }

  /** Where the student stands. @param {import("./study-progress.js").ProgressEvent} event */
  function progressEvent(event) {
    state.progress = event;
    if (state.shown.kind === "progress") renderStage();
  }

  /** An audio overview: drawn again where it is shown, or opened when it
   *  was asked for. @param {import("./study-audio.js").AudioEvent} event */
  function audioEvent(event) {
    const showing = state.shown.kind === "audio" && sounds.open() === event.made.id;
    sounds.audio(event);
    if (showing) return renderStage();
    if (state.want === event.made.id) {
      state.want = null;
      state.shown = { kind: "audio" };
      els.stage.innerHTML = "";
      render();
    }
  }

  /** Whether Fish's key is set, its terms agreed, the voices chosen. @param {{voice: import("./bindings/StudyVoiceState").StudyVoiceState}} event */
  function voiceStateEvent(event) {
    sounds.voiceState(event.voice);
    if (state.shown.kind === "audio") renderStage();
  }

  /** Voices found. @param {any} event */
  function voicesEvent(event) {
    sounds.voices(event);
    if (state.shown.kind === "audio") renderStage();
  }

  /** A voice's sample, to listen to. @param {any} event */
  function sampleEvent(event) {
    sounds.sample(event);
    if (state.shown.kind === "audio") renderStage();
  }

  /** A deck of slides: drawn again where it is shown, or opened when asked
   *  for. @param {import("./study-slides.js").SlidesEvent} event */
  function slidesEvent(event) {
    const showing = state.shown.kind === "slides" && decks.open() === event.made.id;
    decks.slides(event);
    if (showing) return renderStage();
    if (state.want === event.made.id) {
      state.want = null;
      state.shown = { kind: "slides" };
      els.stage.innerHTML = "";
      render();
    }
  }

  /** An infographic, opened. @param {import("./study-graphic.js").GraphicEvent} event */
  function graphicEvent(event) {
    state.shown = { kind: "graphic", graphic: event };
    els.stage.innerHTML = "";
    render();
  }

  /** A Learning Overview, opened. @param {import("./study-overview.js").OverviewEvent} event */
  function learningEvent(event) {
    state.shown = { kind: "learning", learning: event };
    els.stage.innerHTML = "";
    render();
  }

  /** A quiz: drawn again where it is shown, or opened when it was asked for.
   *  @param {import("./study-quiz.js").QuizEvent} event */
  function quizEvent(event) {
    const showing = state.shown.kind === "quiz" && quizzes.open() === event.made.id;
    quizzes.quiz(event);
    if (showing) return renderStage();
    if (state.want === event.made.id) {
      state.want = null;
      state.shown = { kind: "quiz" };
      els.stage.innerHTML = "";
      render();
    }
  }

  /** The session of review, as it stands. @param {{review: import("./bindings/StudyReview").StudyReview | null}} event */
  function reviewingEvent(event) {
    if (cards.reviewing(event)) {
      state.shown = { kind: "review" };
    } else if (state.shown.kind === "review") {
      state.shown = { kind: state.back };
      els.stage.innerHTML = "";
    }
    render();
  }

  /** What a call was told and asked. @param {TaskEvent} event */
  function task(event) {
    state.shown = { kind: "task", task: event };
    els.stage.innerHTML = "";
    render();
  }

  /** A source, in the reader. @param {SourceEvent} event */
  function source(event) {
    state.shown = { kind: "source", source: event };
    state.editing = false;
    state.pasting = false;
    els.stage.innerHTML = "";
    render();
  }

  /** A note, to read and write. @param {NoteEvent} event */
  function note(event) {
    state.shown = { kind: "note", note: event, preview: false };
    state.editing = false;
    state.pasting = false;
    els.stage.innerHTML = "";
    render();
  }

  /** The artifacts Proton keeps, for the list being shown. @param {{artifacts: ArtifactRow[]}} event */
  function artifactsEvent(event) {
    state.artifacts = event.artifacts;
    if (state.pasting === "artifacts") redrawArtifacts();
  }

  /** The list of artifacts drawn again where it is: the stage keeps a form
   *  up as it is, for what is typed in it, so the list redraws itself. */
  function redrawArtifacts() {
    const form = els.stage.querySelector("[data-study-artifacts-form]");
    if (form) form.outerHTML = artifactsHtml();
    else renderStage();
  }

  function closed() {
    state.notebook = null;
    state.open = null;
    cards.clear();
    quizzes.clear();
    sounds.clear();
    decks.clear();
    picker.close();
    render();
  }

  render();

  return { list, notebook, closed, source, note, chat: chatEvent, chatLive, capabilities: chat.capabilities, studio: studioEvent, made, task,
    deck: deckEvent, reviewing: reviewingEvent, quiz: quizEvent, progress: progressEvent, learning: learningEvent,
    audio: audioEvent, voiceState: voiceStateEvent, voices: voicesEvent, voiceSample: sampleEvent, slides: slidesEvent, graphic: graphicEvent,
    kb: picker.kb, kbFolder: picker.folder, kbResults: picker.results, pick: picker.open, artifacts: artifactsEvent };
}

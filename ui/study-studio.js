/* The studio: what can be made of a notebook's ticked sources, what is
 * being made, and what has been — drawn in the notebook's right column, each
 * made thing opened in the middle.
 *
 * Every thing is made by one call to the engine the notebook's tutor is set
 * to, and cites the sources as the tutor does: its citations are chips here
 * as there. The core runs the calls, holds each answer to its shape, and
 * keeps what they made in the notebook; this draws it. */

import { escapeHtml as esc, icon } from "./html.js";
import { markdown } from "./markdown.js";
import { FORMATS } from "./study-audio.js";
import { answerHtml } from "./study-chat.js";

/** @typedef {import("./bindings/StudyMakeKind").StudyMakeKind} Kind */
/** @typedef {import("./bindings/StudyJob").StudyJob} Job */
/** @typedef {import("./bindings/StudyMade").StudyMade} Made */
/** @typedef {import("./bindings/StudyMadeBody").StudyMadeBody} Body */
/** @typedef {import("./bindings/StudyCitation").StudyCitation} Citation */
/** @typedef {import("./bindings/StudyMapNode").StudyMapNode} Node */
/** @typedef {{jobs: Job[], made: Made[]}} StudioEvent */
/** @typedef {{made: Made, body: Body, citations: Citation[]}} MadeEvent */
/** @typedef {{id: string, title: string, engine: string, model: string, state: string, why: string | null, instructions: string, prompt: string}} TaskEvent */

/** What can be made, in the order offered: its kind, name, icon and what it asks for. */
export const TILES = /** @type {[Kind, string, string, string][]} */ ([
  ["studyGuide", "Study guide", "book", "Each topic's key ideas, worked examples and practice questions"],
  ["flashcards", "Flashcards", "cards", "Cards to learn by, each brought back as it is about to be forgotten"],
  ["quiz", "Quiz", "check", "Questions of every kind, each marked as you answer it"],
  ["practice", "Practice problems", "bulb", "Problems to work through, with hints and worked solutions"],
  ["overview", "Learning overview", "layers", "The course topic by topic: taught, mapped, carded and checked"],
  ["audio", "Audio overview", "audio", "Two hosts talking the sources through, spoken by Fish Audio"],
  ["slides", "Slide deck", "desktop", "A lecture's slides, with speaker notes, for PowerPoint"],
  ["infographic", "Infographic", "chart", "A page of it at a glance, saved as a picture"],
  ["exam", "Practice exam", "files", "A new paper like the past papers, to sit against the clock"],
  ["pastPaper", "Past paper", "file", "A past paper itself, to sit again, marked by its solutions"],
  ["objectives", "Learning objectives", "target", "What you must be able to do in the exam"],
  ["faq", "FAQ", "chat", "The questions a student asks, answered"],
  ["glossary", "Glossary", "text", "Every term, as the sources define it"],
  ["mindMap", "Mind map", "node", "The ideas, and how they branch"],
  ["table", "Table", "table", "A row for each thing, the columns you name"],
  ["briefing", "Briefing", "file-doc", "The main themes and what follows from them"],
  ["timeline", "Timeline", "clock", "What happens, in order"],
  ["report", "Your own", "pen", "A document of your asking"],
  ["discover", "Discover", "globe", "Pages, papers and videos on a topic, found on the web, to add as sources"],
  ["deepResearch", "Deep Research", "search", "A report on a topic from what it reads on the web, and its sources"],
]);
const TILE = /** @type {Record<string, string>} */ ({ ...Object.fromEntries(TILES.map(([kind, name]) => [kind, name])), marking: "Marking" });

/** How many of a thing can be asked for, by kind. */
const AMOUNTS = /** @type {Record<string, [string, string][]>} */ ({
  flashcards: [["fewer", "Fewer — about 10"], ["standard", "About 20"], ["more", "More — about 40"]],
  slides: [["fewer", "Fewer — about 6"], ["standard", "About 10"], ["more", "More — about 16"]],
  quiz: [["fewer", "Fewer — about 5"], ["standard", "About 10"], ["more", "More — about 20"]],
  practice: [["fewer", "Fewer — about 3"], ["standard", "About 5"], ["more", "More — about 8"]],
});

const STATES = /** @type {Record<string, string>} */ ({ waiting: "Waiting its turn", running: "Being made", repairing: "Being mended", done: "Made", failed: "Not made" });

/** A time, said as long ago as it was. @param {number} s */
function ago(s) {
  const d = Math.floor(Date.now() / 1000) - s;
  if (d < 60) return "just now";
  if (d < 3600) return `${Math.floor(d / 60)} min ago`;
  if (d < 86400) return `${Math.floor(d / 3600)} h ago`;
  return new Date(s * 1000).toLocaleDateString();
}

/** A mind map, drawn as a tree whose branches fold. @param {Node[]} nodes @param {Citation[]} citations */
export function mapHtml(nodes, citations) {
  /** @param {string | null} parent @param {number} depth @returns {string} */
  const branch = (parent, depth) => {
    const children = nodes.filter((n) => n.parent === parent);
    if (!children.length) return "";
    return `<ul class="st-map-level">${children.map((n) => {
      const below = branch(n.id, depth + 1);
      const chips = n.cites.map((c) => citations.find((x) => x.raw === c)).filter(Boolean)
        .map((c) => `<button class="st-cite${/** @type {Citation} */ (c).resolved ? "" : " bad"}" data-study-cite="${/** @type {Citation} */ (c).n}">${/** @type {Citation} */ (c).n}</button>`).join("");
      const label = markdown(n.label).replace(/^<p>|<\/p>\s*$/g, "");
      return `<li class="st-map-node${below ? " has" : ""}"${depth >= 2 && below ? ' data-folded=""' : ""}>` +
        `<div class="st-map-label">${below ? `<button class="st-map-fold" data-study-fold aria-label="Fold">${icon("chev")}</button>` : ""}` +
        `<span>${label}</span>${chips}<button class="st-map-ask" data-study-ask-about="${esc(n.label)}" title="Ask the tutor about this">${icon("chat")}</button></div>${below}</li>`;
    }).join("")}</ul>`;
  };
  return `<div class="st-map" data-turn="made">${branch(null, 0)}</div>`;
}

/** A web address's host, as it is shown. @param {string} url */
function hostOf(url) {
  try {
    return new URL(url).host.replace(/^www\./, "");
  } catch {
    return url;
  }
}

/** What was found on the web: the report, for Deep Research, and the
 *  sources, each ticked to be added unless it is a source already.
 *  @param {Extract<Body, {shape: "found"}>} body @param {boolean} readOnly @param {{have: Map<string, string>, unpicked: Set<string>}} found */
function foundHtml(body, readOnly, found) {
  const report = body.report ? `<div class="st-answer st-doc md" data-turn="made">${markdown(body.report.replace(/^#\s+.*\n+/, ""))}</div>` : "";
  const picked = body.found.filter((f) => !found.have.has(f.url) && !found.unpicked.has(f.url)).length;
  const rows = body.found.map((f, i) => {
    const id = found.have.get(f.url);
    const tick = id ? " checked disabled" : `${found.unpicked.has(f.url) ? "" : " checked"}${readOnly ? " disabled" : ""}`;
    return `<div class="st-found-row${id ? " added" : ""}"><input type="checkbox" data-study-found-pick="${esc(f.url)}"${tick} aria-label="Add ${esc(f.title)}">` +
      `<span class="st-found-n">${i + 1}</span>${icon(f.kind === "video" ? "film" : f.kind === "pdf" ? "file-doc" : "globe")}` +
      `<div class="st-found-text"><b>${esc(f.title)}</b><span class="st-faint">${esc(hostOf(f.url))}${id ? ` · added as ${esc(id)}` : ""}</span>` +
      (f.why ? `<span>${esc(f.why)}</span>` : "") + `</div>` +
      `<button class="st-link" data-study-page="${esc(f.url)}" title="${esc(f.url)}">Open</button></div>`;
  }).join("");
  const tools = readOnly ? "" : `<div class="st-deck-tools">` +
    (body.report ? `<button class="cc-quiet" data-study-found-report title="The report, and the list of what it read, kept as a source">Add the report as a source</button>` : "") +
    `<span class="grow"></span><button class="cc-primary" data-study-found-add${picked ? "" : " disabled"}>Add ${picked} as ${picked === 1 ? "a source" : "sources"}</button></div>`;
  return `${report}<h3 class="st-found-head">${body.report ? "What it read" : "What was found"}</h3>${tools}<div class="st-found-list" data-turn="made">${rows}</div>`;
}

/** Something made, whole, for the middle. @param {MadeEvent} e @param {boolean} readOnly
 *  @param {{have: Map<string, string>, unpicked: Set<string>}} [found] what of the web the notebook has, and what is not to be added */
export function madeHtml(e, readOnly, found = { have: new Map(), unpicked: new Set() }) {
  const { made, body, citations } = e;
  const head = `<div class="st-reader-head"><button class="cc-quiet" data-study-back>← Back</button>` +
    `<div><h1>${esc(made.title)}</h1><span class="st-faint">${esc(TILE[made.kind] ?? made.kind)} · ${esc(made.by)} · ${esc(ago(made.made))}</span></div>` +
    `<button class="cc-quiet" data-study-task="${esc(made.task)}" title="What the call was told and asked">Prompt</button>` +
    (readOnly ? "" : `<button class="cc-quiet" data-study-bin-made="${esc(made.id)}" title="Put it in the notebook's bin">Bin</button>`) + `</div>`;
  /** @param {string[]} cites */
  const chips = (cites) => cites.map((c) => citations.find((x) => x.raw === c)).filter(Boolean)
    .map((c) => `<button class="st-cite${/** @type {Citation} */ (c).resolved ? "" : " bad"}" data-study-cite="${/** @type {Citation} */ (c).n}">${/** @type {Citation} */ (c).n}</button>`).join("");
  let content = "";
  if (body.shape === "prose") {
    // Its own title is the head's.
    content = `<div class="st-answer st-doc md" data-turn="made">${answerHtml(body.markdown.replace(/^#\s+.*\n+/, ""), citations)}</div>`;
  } else if (body.shape === "map") {
    content = `${readOnly ? "" : `<div class="st-deck-tools"><span class="grow"></span><button class="cc-quiet" data-study-picture="${esc(made.id)}">${icon("image")}Save as PNG</button></div>`}${mapHtml(body.nodes, citations)}`;
  } else if (body.shape === "table") {
    content = `<div class="st-table-box" data-turn="made"><table class="st-table"><thead><tr>${body.columns.map((c) => `<th>${esc(c)}</th>`).join("")}<th></th></tr></thead>` +
      `<tbody>${body.rows.map((r) => `<tr>${r.cells.map((cell) => `<td>${markdown(cell).replace(/^<p>|<\/p>\s*$/g, "")}</td>`).join("")}<td>${chips(r.cites)}</td></tr>`).join("")}</tbody></table>` +
      `<p class="st-faint">Kept beside it as a CSV: studio/tables/${esc(made.id)}.csv</p></div>`;
  } else if (body.shape === "cards") {
    // A deck opens as a deck (study-cards.js); this is only its cards, read.
    content = `<ol class="st-cards" data-turn="made">${body.cards.map((c) => `<li class="st-card-row"><div class="st-card-sides">` +
      `<div class="st-card-front md">${answerHtml(c.front, citations)}</div><div class="st-card-back md">${answerHtml(c.back, citations)}</div></div></li>`).join("")}</ol>`;
  } else if (body.shape === "slides" || body.shape === "slide" || body.shape === "graphic") {
    // Each opens as its own (study-slides.js, study-graphic.js).
    content = "";
  } else if (body.shape === "audio") {
    // An audio overview opens as one (study-audio.js); this is only its words.
    content = `<div class="st-transcript" data-turn="made">${body.segments.map((s) => `<p>${answerHtml(s.display, citations)}</p>`).join("")}</div>`;
  } else if (body.shape === "overview") {
    // An overview opens as one (study-overview.js); this is only its topics.
    content = `<div class="st-answer st-doc md" data-turn="made">${body.topics.map((t) => `<h2>${esc(t.title)}</h2>${answerHtml(t.summary, citations)}`).join("")}</div>`;
  } else if (body.shape === "found") {
    content = foundHtml(body, readOnly, found);
  } else if (body.shape === "questions" || body.shape === "paper") {
    // A quiz opens as a quiz (study-quiz.js); this is only its questions, read.
    content = `<ol class="st-cards" data-turn="made">${body.questions.map((q) => `<li class="st-card-row"><div class="md">${answerHtml(q.prompt, citations)}</div></li>`).join("")}</ol>`;
  } else if (body.shape === "guide" || body.shape === "labels" || body.shape === "pages") {
    // Kept with the sources, and shown with them.
    content = "";
  } else {
    const topics = [...new Set(body.objectives.map((o) => o.topic))];
    content = `<div class="st-objectives" data-turn="made">${topics.map((t) => `<section><h3>${esc(t)}</h3><ol>${body.objectives.filter((o) => o.topic === t)
      .map((o) => `<li><span class="st-faint">${esc(o.id)}</span> ${markdown(o.text).replace(/^<p>|<\/p>\s*$/g, "")} ${chips(o.cites)}</li>`).join("")}</ol></section>`).join("")}</div>`;
  }
  return `<div class="st-reader st-made-view">${head}${content}</div>`;
}

/** What a call was told and asked. @param {TaskEvent} t */
export function taskHtml(t) {
  const how = [t.engine, t.model].filter(Boolean).join(" · ");
  return `<div class="st-reader st-task-view"><div class="st-reader-head"><button class="cc-quiet" data-study-back>← Back</button>` +
    `<div><h1>${esc(t.title)}</h1><span class="st-faint">${esc(t.id)} · ${esc(how)} · ${esc(STATES[t.state] ?? t.state)}${t.why ? ` — ${esc(t.why)}` : ""}</span></div></div>` +
    `<h3>What it was told</h3><pre class="st-prompt">${esc(t.instructions)}</pre><h3>What it was asked</h3><pre class="st-prompt">${esc(t.prompt)}</pre></div>`;
}

/** @param {{box: HTMLElement, dispatch: (command: any) => unknown, readOnly: () => boolean, pastPapers: () => {id: string, title: string}[]}} host */
export function createStudio(host) {
  /** @type {{studio: StudioEvent | null, asking: Kind | null}} */
  const state = { studio: null, asking: null };

  function optionsHtml(/** @type {Kind} */ kind) {
    const focus = `<input name="focus" placeholder="A topic to centre it on — or leave it to cover everything ticked" autocomplete="off">`;
    const web = `<p class="st-faint">Searched on the web through Claude Code, whichever model the tutor is set to; nothing is added until you choose.</p>`;
    const field = kind === "discover"
      ? `<input name="focus" placeholder="A topic — or leave it to find what is like the sources" autocomplete="off">${web}`
      : kind === "deepResearch"
        ? `<input name="focus" placeholder="What to research — “how A* search is made optimal”" autocomplete="off" required>${web}`
      : kind === "table"
      ? `<input name="columns" placeholder="Columns, by commas: Algorithm, Complete?, Optimal?, Time" autocomplete="off" required>`
      : kind === "report"
        ? `<textarea name="ask" rows="3" placeholder="What it should be: “A one-page cheat sheet of every search algorithm’s complexity.”" required></textarea>`
        : kind === "audio"
          ? `<div class="st-card-form-row"><select name="format" aria-label="What it is">${FORMATS.map(([k, name, note]) => `<option value="${k}" title="${esc(note)}">${esc(name)}</option>`).join("")}</select>` +
            `<select name="amount" aria-label="How long"><option value="fewer">Shorter</option><option value="standard" selected>The usual length</option><option value="more">Longer</option></select></div>` +
            `<input name="language" placeholder="In a language other than the sources'? — “French”" autocomplete="off">${focus}`
        : kind === "exam"
          ? `<div class="st-card-form-row"><select name="level" aria-label="How hard"><option value="easy">A little easier than the past papers</option>` +
            `<option value="medium" selected>As hard as the past papers</option><option value="hard">A little harder</option></select></div>` +
            `<input name="focus" placeholder="Topics to weigh it toward — or leave it as the past papers weigh them" autocomplete="off">`
        : kind === "pastPaper"
          ? (host.pastPapers().length
            ? `<select name="source" aria-label="Which past paper">${host.pastPapers().map((p) => `<option value="${esc(p.id)}">${esc(p.id)} · ${esc(p.title)}</option>`).join("")}</select>`
            : `<p class="st-faint">No source is a past paper yet. Add one, or mark one as a past paper in Sources.</p>`)
        : kind in AMOUNTS
          ? `<div class="st-card-form-row"><select name="amount" aria-label="How many">${AMOUNTS[kind].map(([value, said]) => `<option value="${value}"${value === "standard" ? " selected" : ""}>${said}</option>`).join("")}</select>` +
            `<select name="level" aria-label="How hard"><option value="easy">Easy</option><option value="medium" selected>Medium</option><option value="hard">Hard</option></select></div>${focus}`
          : focus;
    return `<form class="st-make-form" data-study-make-form="${kind}" novalidate>${field}
        <div class="rs-actions"><button type="button" class="cc-quiet" data-study-make-cancel>Cancel</button><button type="submit" class="cc-primary">Make</button></div></form>`;
  }

  function render() {
    const studio = state.studio;
    if (!studio) {
      host.box.innerHTML = "";
      return;
    }
    const ro = host.readOnly();
    const tiles = TILES.map(([kind, name, ic, note]) => `<button class="st-tile${state.asking === kind ? " on" : ""}" data-study-tile="${kind}" title="${esc(note)}"${ro ? " disabled" : ""}>` +
      `${icon(ic)}<span>${esc(name)}</span></button>`).join("");
    const asking = state.asking ? optionsHtml(state.asking) : "";
    const live = studio.jobs.filter((j) => j.state !== "done");
    const jobs = live.map((j) => `<div class="st-job ${esc(j.state)}">` +
      (j.state === "failed" ? icon("d-error") : '<span class="lab-spin"></span>') +
      `<div><b>${esc(j.title)}</b><span class="st-faint">${esc(STATES[j.state] ?? j.state)}${j.why ? ` — ${esc(j.why)}` : ""}</span></div>` +
      (j.state === "failed" ? `<button class="st-link" data-study-task="${esc(j.id)}">Prompt</button>` : `<button class="st-x" data-study-cancel="${esc(j.id)}" title="Stop it">${icon("close")}</button>`) + `</div>`).join("");
    const made = studio.made.map((m) => `<button class="st-made" data-study-made="${esc(m.id)}">` +
      `${icon(TILES.find(([k]) => k === m.kind)?.[2] ?? "file")}<span><b>${esc(m.title)}</b><span class="st-faint">${esc(TILE[m.kind] ?? m.kind)} · ${esc(ago(m.made))}</span></span></button>`).join("");
    // The form being typed into is kept.
    const typing = host.box.querySelector("[data-study-make-form]");
    if (typing && state.asking && typing.getAttribute("data-study-make-form") === state.asking) {
      const list = host.box.querySelector("[data-study-studio-list]");
      if (list) list.innerHTML = `${jobs}${made || (live.length ? "" : `<p class="st-faint st-pad">Nothing made yet. Each thing is made from the sources ticked, and cites them.</p>`)}`;
      return;
    }
    host.box.innerHTML = `<div class="st-tiles">${tiles}</div>${asking}<div class="st-studio-list" data-study-studio-list>${jobs}` +
      `${made || (live.length ? "" : `<p class="st-faint st-pad">Nothing made yet. Each thing is made from the sources ticked, and cites them.</p>`)}</div>`;
    /** @type {HTMLElement | null} */ (host.box.querySelector("[data-study-make-form] input, [data-study-make-form] textarea"))?.focus({ preventScroll: true });
  }

  /** A click in the studio; `true` if it was the studio's. @param {HTMLElement} t */
  function click(t) {
    if (!host.box.contains(t)) return false;
    const at = (/** @type {string} */ sel) => /** @type {HTMLElement | null} */ (t.closest(sel));
    const tile = at("[data-study-tile]");
    if (tile) {
      const kind = /** @type {Kind} */ (tile.dataset.studyTile);
      state.asking = state.asking === kind ? null : kind;
      host.box.querySelector("[data-study-make-form]")?.remove();
      render();
      return true;
    }
    if (at("[data-study-make-cancel]")) {
      state.asking = null;
      host.box.querySelector("[data-study-make-form]")?.remove();
      render();
      return true;
    }
    const cancel = at("[data-study-cancel]");
    if (cancel) {
      host.dispatch({ type: "studyCancelJob", id: cancel.dataset.studyCancel });
      return true;
    }
    const made = at("[data-study-made]");
    if (made) {
      host.dispatch({ type: "studyShowMade", id: made.dataset.studyMade });
      return true;
    }
    const task = at("[data-study-task]");
    if (task) {
      host.dispatch({ type: "studyShowTask", id: task.dataset.studyTask });
      return true;
    }
    return true;
  }

  /** @param {HTMLFormElement} form */
  function submit(form) {
    if (!form.matches("[data-study-make-form]") || !host.box.contains(form)) return false;
    const kind = /** @type {Kind} */ (form.dataset.studyMakeForm);
    const data = new FormData(form);
    const options = {
      focus: String(data.get("focus") ?? "").trim(),
      ask: String(data.get("ask") ?? "").trim(),
      columns: String(data.get("columns") ?? "").split(",").map((c) => c.trim()).filter(Boolean),
      amount: String(data.get("amount") ?? "standard"),
      level: String(data.get("level") ?? "medium"),
      deck: null,
      objectives: [],
      source: data.get("source") ? String(data.get("source")) : null,
      count: null,
      format: data.get("format") ? String(data.get("format")) : null,
      language: String(data.get("language") ?? "").trim(),
      slides: null,
      slide: null,
    };
    if (kind === "pastPaper" && !options.source) return true;
    if ((kind === "table" && !options.columns.length) || (kind === "report" && !options.ask) || (kind === "deepResearch" && !options.focus)) {
      /** @type {HTMLElement | null} */ (form.querySelector("input, textarea"))?.focus();
      return true;
    }
    host.dispatch({ type: "studyMake", kind, options });
    state.asking = null;
    form.remove();
    render();
    return true;
  }

  /** @param {StudioEvent} event */
  function studio(event) {
    state.studio = event;
    render();
  }

  return { studio, render, click, submit, jobs: () => state.studio?.jobs ?? [], clear: () => { state.studio = null; state.asking = null; render(); } };
}

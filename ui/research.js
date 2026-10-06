/* Research: investigations, and the co-scientist round that grows them.
 *
 * Each investigation is a folder in the project — `research/<slug>/` — or in
 * the folder of their own the person keeps them in, and
 * this draws what the core reads out of it: the investigations down the
 * left, with who sits at the open one's table under them; in the middle
 * where it stands (its overview, its plan, the brief it answers), its
 * hypotheses as the tournament ranks them, the literature it has read, and
 * what its round is doing, call by call; and the Lab, where its code is run,
 * on the right.
 *
 * Nothing here decides anything. The files are the investigation, the core
 * reads them and runs the round, and this draws what it is sent — so a
 * hypothesis edited by hand is what the board shows the next time it is
 * drawn. What is asked of the core is asked, and drawn when it answers. */

import { escapeHtml as esc, icon } from "./html.js";
import { markdown } from "./markdown.js";
import { citationHtml, literatureHtml, ownPaper, paperHtml, referenceHtml } from "./literature.js";

/** @typedef {import("./bindings/ResearchSummary").ResearchSummary} Summary */
/** @typedef {import("./bindings/HypothesisRow").HypothesisRow} Row */
/** @typedef {import("./bindings/HypothesisDetail").HypothesisDetail} Detail */
/** @typedef {import("./bindings/ResearchDiscipline").ResearchDiscipline} Discipline */
/** @typedef {import("./bindings/ResearchAuthor").ResearchAuthor} Author */
/** @typedef {import("./bindings/ReviewVerdict").ReviewVerdict} Verdict */
/** @typedef {import("./bindings/ResearchTask").ResearchTask} Task */
/** @typedef {import("./bindings/ResearchSeatRow").ResearchSeatRow} SeatRow */
/** @typedef {import("./bindings/RoundStatus").RoundStatus} RoundStatus */
/** @typedef {import("./bindings/ClaudeItem").ClaudeItem} Item */
/** @typedef {{investigations: Summary[], open: string | null, folder?: string}} ListEvent */
/** @typedef {{slug: string, title: string, discipline: Discipline, brief: string, plan: string,
 *   overview: string, verdicts: string, sandbox: string, feedback: string, hypotheses: Row[]}} BoardEvent */
/** @typedef {{slug: string, detail: Detail | null}} HypothesisEvent */
/** @typedef {import("./bindings/LiteratureRow").LiteratureRow} LiteratureStep */
/** @typedef {{slug: string, round: number, status: RoundStatus, used: number, cap: number,
 *   seats: SeatRow[], tasks: Task[], literature?: LiteratureStep | null, held: string | null}} RoundEvent */
/** @typedef {{slug: string, task: string, items: Item[]}} TranscriptEvent */
/** @typedef {import("./literature.js").LiteratureEvent} LiteratureEvent */
/** @typedef {import("./literature.js").PaperEvent} PaperEvent */
/** @typedef {import("./literature.js").ResultsEvent} ResultsEvent */
/** @typedef {import("./bindings/PaperDetail").PaperDetail} PaperDetail */
/** @typedef {import("./bindings/LiteratureSource").LiteratureSource} Source */
/** @typedef {"overview" | "hypotheses" | "literature" | "activity"} Tab */

/** The disciplines, in the order the form offers them. */
const DISCIPLINES = /** @type {[Discipline, string][]} */ ([
  ["physics", "Physics"],
  ["mathematics", "Mathematics"],
  ["chemistry", "Chemistry"],
  ["biology", "Biology"],
  ["computerScience", "Computer Science"],
  ["other", "Other"],
]);
const DISCIPLINE = Object.fromEntries(DISCIPLINES);

/** Who sits at the table, as the board names them. */
const SEATS = /** @type {Record<string, string>} */ ({
  claude: "Claude Code", codex: "Codex", antigravity: "Antigravity", gemini: "Gemini", scientist: "You",
});

/** The roles of the user's AGENTS.md. */
const ROLES = /** @type {Record<string, string>} */ ({
  investigator: "Principal Investigator", engineer: "Lab Engineer", sandbox: "Validation Sandbox",
});

/** A round's steps, in the order they run. */
const PHASES = /** @type {[string, string][]} */ ([
  ["frame", "Frame"],
  ["generate", "Generate"],
  ["reflect", "Review"],
  ["verify", "Verify"],
  ["rank", "Tournament"],
  ["evolve", "Evolve"],
  ["meta", "Overview"],
  ["simReview", "Simulation review"],
]);
const PHASE = Object.fromEntries(PHASES);

const VERDICTS = /** @type {Record<Verdict, string>} */ ({
  strong: "Strong", promising: "Promising", weak: "Weak", flawed: "Flawed",
});

const KINDS = /** @type {Record<string, string>} */ ({
  review: "Review", verification: "Verification", simulation: "Simulation review", formal: "Formal proof",
});

/** What a task's state is called. */
const STATES = /** @type {Record<string, string>} */ ({
  ready: "Waiting", running: "Working", repairing: "Mending its answer", done: "Done",
  failed: "Failed", skipped: "Skipped", cancelled: "Stopped",
});

/** @param {Author} author */
function who(author) {
  const seat = SEATS[author.seat] ?? author.seat;
  return author.model && author.seat !== "scientist" ? `${seat} · ${author.model}` : seat;
}

/** @param {number} n @param {string} one @param {string} many */
const count = (n, one, many) => `${n} ${n === 1 ? one : many}`;

/** How long, in words a list can hold. @param {number} s */
function span(s) {
  if (s < 60) return `${Math.max(0, Math.round(s))}s`;
  const m = Math.floor(s / 60);
  return m < 60 ? `${m}m ${Math.round(s % 60)}s` : `${Math.floor(m / 60)}h ${m % 60}m`;
}

/** A time of day, from seconds since the epoch. @param {number} s */
function clock(s) {
  return new Date(s * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/** A file's heading is the board's own: the title is already over it.
 *  @param {string} text @param {string} title */
function withoutTitle(text, title) {
  const m = /^#\s+(.*)\n+/.exec(text);
  return m && m[1].trim() === title.trim() ? text.slice(m[0].length) : text;
}

/** `/h Title: statement` — a hypothesis of the scientist's own.
 *  @param {string} text @returns {{title: string, statement: string} | null} */
export function ownHypothesis(text) {
  const m = /^\/h(?:ypothesis)?\s+([\s\S]+)$/.exec(text.trim());
  if (!m) return null;
  const body = m[1].trim();
  const colon = body.indexOf(":");
  if (colon > 0 && colon <= 120) return { title: body.slice(0, colon).trim(), statement: body.slice(colon + 1).trim() || body.slice(0, colon).trim() };
  const firstLine = body.split("\n")[0];
  return { title: firstLine.length > 80 ? `${firstLine.slice(0, 77)}…` : firstLine, statement: body };
}

/** @param {{root: Document, dispatch: (command: any) => unknown}} host */
export function createResearch(host) {
  const ws = /** @type {HTMLElement | null} */ (host.root.querySelector('.workspace[data-ws="research"]'));
  if (!ws) return null;
  const q = (/** @type {string} */ sel) => /** @type {HTMLElement} */ (ws.querySelector(sel));
  const els = {
    list: q("[data-research-list]"),
    seatsBox: q("[data-research-seats-box]"),
    where: q("[data-research-where]"),
    seats: q("[data-research-seats]"),
    title: q("[data-research-title]"),
    discipline: q("[data-research-discipline]"),
    run: q("[data-research-run]"),
    status: q("[data-research-status]"),
    meter: q("[data-research-meter]"),
    go: /** @type {HTMLButtonElement} */ (q("[data-research-go]")),
    pause: /** @type {HTMLButtonElement} */ (q("[data-research-pause]")),
    stop: /** @type {HTMLButtonElement} */ (q("[data-research-stop]")),
    tabs: q("[data-research-tabs]"),
    count: q("[data-research-count]"),
    papers: q("[data-research-papers]"),
    busy: q("[data-research-busy]"),
    board: q("[data-research-board]"),
    dock: q("[data-research-dock]"),
    input: /** @type {HTMLTextAreaElement} */ (q("[data-research-input]")),
    hint: q("[data-research-hint]"),
    send: /** @type {HTMLButtonElement} */ (q("[data-research-send]")),
  };

  /** @type {{list: Summary[], open: string | null, folder: string, moving: string | null, moved: boolean, board: BoardEvent | null, detail: Detail | null,
   *   round: RoundEvent | null, task: string | null, transcript: TranscriptEvent | null,
   *   literature: LiteratureEvent | null, paper: PaperDetail | null, results: ResultsEvent | null, query: string, adding: string[],
   *   tab: Tab, creating: boolean, made: number, draft: {title: string, goal: string, discipline: Discipline, literature: boolean}}} */
  const state = {
    // Where the investigations are kept, as the core says it; and the folder
    // being typed to keep them in instead, once asked for, until it is.
    list: [], open: null, folder: "research/", moving: /** @type {string | null} */ (null), moved: false,
    board: null, detail: null, round: null, task: null, transcript: null, tab: "overview",
    // The literature, the paper shown whole, the last search, what is typed
    // in its box, and the results asked to be added that the core has not
    // answered for yet.
    literature: null, paper: null, results: null, query: "", adding: [],
    // The form is up, and how many there were when it was sent: it goes when
    // the list grows, and stays with what was typed when the core refuses.
    creating: false, made: -1,
    draft: { title: "", goal: "", discipline: "physics", literature: true },
  };

  /** The open investigation's literature, if it has been said. */
  const literature = () => (state.literature && state.literature.slug === state.open ? state.literature : null);

  /** The open investigation's round, if it has been said. */
  const round = () => (state.round && state.round.slug === state.open ? state.round : null);

  /* ── the list ──────────────────────────────────────────────────── */

  function renderList() {
    if (!state.list.length) {
      const where = state.folder === "research/" ? "in this project yet" : "yet";
      els.list.innerHTML = `<div class="cc-none">No investigations ${where}. Each is a folder in <code>${esc(state.folder)}</code>, and starts from a question.</div>`;
      return;
    }
    const running = round()?.status.status === "running" ? round()?.slug : null;
    els.list.innerHTML = state.list.map((i) => {
      const on = i.slug === state.open && !state.creating;
      const where = `${state.folder}${i.slug}/ · ${count(i.hypotheses, "hypothesis", "hypotheses")} · ${count(i.rounds, "round", "rounds")}`;
      return `<button class="cc-session rs-item${on ? " on" : ""}" data-research-open="${esc(i.slug)}" title="${esc(where)}"` +
        `${on ? ' aria-current="true"' : ""}>` +
        (i.slug === running ? `<span class="rs-live" title="A round is running"></span>` : "") +
        `<span class="cc-session-name">${esc(i.title)}</span>` +
        `<span class="cc-session-where">${esc(DISCIPLINE[i.discipline] ?? "")}</span></button>`;
    }).join("");
  }

  /* ── the table ─────────────────────────────────────────────────── */

  function renderSeats() {
    const r = state.creating ? null : round();
    els.seatsBox.hidden = !r;
    if (!r) return;
    const running = r.status.status === "running";
    const now = Date.now() / 1000;
    els.seats.innerHTML = r.seats.filter((s) => s.seat !== "scientist").map((s) => {
      const name = SEATS[s.seat] ?? s.seat;
      const cooling = s.cooling && s.cooling > now ? `At its limit until ${clock(s.cooling)}` : "";
      const line = !s.enabled ? "Stood down"
        : s.why ? s.why
        : cooling ? cooling
        : s.running ? `${count(s.running, "call", "calls")} out`
        : "Ready";
      const kind = !s.enabled ? "off" : s.why ? "out" : cooling ? "cooling" : s.running ? "busy" : "ready";
      return `<label class="rs-seat-row" data-kind="${kind}" title="${esc(line)}">` +
        `<input type="checkbox" data-research-seat="${esc(s.seat)}"${s.enabled ? " checked" : ""}${running || r.held ? " disabled" : ""}` +
        ` aria-label="${esc(name)} sits at the table">` +
        `<span class="rs-seat-text"><b class="rs-seat" data-seat="${esc(s.seat)}">${esc(name)}</b>` +
        `<small>${esc(ROLES[s.role] ?? s.role)}${s.model ? ` · ${esc(s.model)}` : ""}</small>` +
        `<small class="rs-seat-line">${esc(line)}</small></span></label>`;
    }).join("");
  }

  /* ── the header ────────────────────────────────────────────────── */

  function renderHead() {
    const board = state.creating ? null : state.board;
    els.title.textContent = state.creating ? "New investigation" : board?.title ?? "Research";
    els.discipline.hidden = !board;
    els.discipline.textContent = board ? DISCIPLINE[board.discipline] ?? "" : "";
    els.tabs.hidden = !board;
    for (const tab of els.tabs.querySelectorAll("[data-research-tab]")) {
      const on = /** @type {HTMLElement} */ (tab).dataset.researchTab === state.tab;
      tab.setAttribute("aria-selected", String(on));
      tab.classList.toggle("on", on);
    }
    const n = board?.hypotheses.length ?? 0;
    els.count.textContent = n ? String(n) : "";
    const papers = board ? literature()?.papers.length ?? 0 : 0;
    els.papers.textContent = papers ? String(papers) : "";
    const out = round()?.tasks.filter((t) => t.state === "running" || t.state === "repairing").length ?? 0;
    els.busy.textContent = out ? String(out) : "";
    els.dock.hidden = !board;
    renderRun();
  }

  function renderRun() {
    const r = state.creating || !state.board ? null : round();
    els.run.hidden = !r;
    if (!r) return;
    const s = r.status;
    const n = r.round;
    let text = "";
    if (r.held) text = "Read only";
    else if (s.status === "running") text = `Round ${n} · running`;
    else if (s.status === "paused") text = s.interrupted ? `Round ${n} · paused when Proton closed` : `Round ${n} · paused`;
    else text = n ? `${count(n, "round", "rounds")} so far` : "No rounds yet";
    els.status.textContent = text;
    els.status.title = r.held ?? "";
    els.status.dataset.status = r.held ? "held" : s.status;
    const live = s.status !== "idle";
    els.meter.hidden = !live;
    els.meter.title = `${r.used} of ${r.cap} agent calls this round`;
    const bar = /** @type {HTMLElement} */ (els.meter.firstElementChild);
    bar.style.width = `${Math.min(100, (r.used / Math.max(1, r.cap)) * 100)}%`;
    els.meter.dataset.label = `${r.used}/${r.cap}`;
    els.pause.hidden = s.status !== "running";
    els.stop.hidden = !live;
    els.go.hidden = s.status === "running";
    els.go.textContent = s.status === "paused" ? "Resume" : "Run round";
    els.go.title = s.status === "paused" ? "Carry on with the round" : `A round: about fourteen agent calls, at most ${r.cap}`;
    for (const b of [els.go, els.pause, els.stop]) b.disabled = Boolean(r.held);
  }

  /* ── the board ─────────────────────────────────────────────────── */

  function renderBoard() {
    renderHead();
    renderSeats();
    // The form is drawn once: drawn again, under whatever the core sends
    // while it is up, it would take the caret from the field being typed in.
    if (state.creating) {
      if (!els.board.querySelector("[data-research-form]")) renderForm();
      return;
    }
    const board = state.board;
    if (!board) {
      els.board.innerHTML = `<div class="rs-col rs-welcome">
        <div class="rs-welcome-mark">${icon("flask")}</div>
        <h2>What should the team find out?</h2>
        <p>An investigation starts from a question. Claude Code, Codex and Antigravity each propose hypotheses,
        review one another's, and a tournament ranks them; what is worth running, the Lab runs. Physics and
        mathematics first, and chemistry, biology and computer science beside them.</p>
        <p class="rs-faint">Everything it does is a file in this project, under <code>research/</code>.</p>
        <button class="cc-primary" data-research-new>New investigation</button>
      </div>`;
      return;
    }
    // A transcript being read keeps its place as the list under it moves,
    // and a search being typed keeps its caret as the literature moves.
    const scroll = els.board.scrollTop;
    const typing = /** @type {HTMLInputElement | null} */ (els.board.querySelector("[data-lit-query]"));
    const caret = typing && typing === document.activeElement ? [typing.selectionStart, typing.selectionEnd] : null;
    els.board.innerHTML = state.tab === "hypotheses"
      ? (state.detail ? detailHtml(state.detail) : leaderboardHtml(board))
      : state.tab === "literature" ? literatureTabHtml(board)
      : state.tab === "activity" ? activityHtml() : overviewHtml(board);
    els.board.scrollTop = scroll;
    const field = /** @type {HTMLInputElement | null} */ (els.board.querySelector("[data-lit-query]"));
    if (caret && field) {
      field.focus({ preventScroll: true });
      field.setSelectionRange(caret[0], caret[1]);
    }
  }

  /** @param {BoardEvent} board */
  function literatureTabHtml(board) {
    const lit = literature();
    if (!lit) return `<div class="rs-col"><div class="rs-faint">Reading…</div></div>`;
    if (state.paper) {
      return paperHtml(state.paper, { folder: state.folder, slug: board.slug, fetching: lit.fetching.includes(state.paper.row.id), enabled: lit.enabled });
    }
    const results = state.results && state.results.slug === board.slug ? state.results : null;
    return literatureHtml({ literature: lit, results, folder: state.folder, query: state.query, adding: state.adding });
  }

  /** What the team wrote, its citations — `[[literature/P003.pdf p. 4]]` — made links. @param {string} text */
  const prose = (text) => markdown(text, { cite: true });

  /** @param {string} label @param {string} file @param {string} text @param {boolean} [folded] */
  function doc(label, file, text, folded = false) {
    const body = `<div class="cc-prose">${prose(text)}</div>`;
    const head = `<span>${esc(label)}</span><code class="rs-file" title="${esc(file)}">${esc(file.split("/").pop() ?? file)}</code>`;
    return folded
      ? `<details class="rs-doc rs-fold"><summary>${head}</summary>${body}</details>`
      : `<section class="rs-doc"><header>${head}</header>${body}</section>`;
  }

  /** @param {BoardEvent} board */
  function overviewHtml(board) {
    const path = `${state.folder}${board.slug}/`;
    return `<div class="rs-col">` +
      doc("Brief", `${path}README.md`, withoutTitle(board.brief, board.title)) +
      doc("Overview", `${path}overview.md`, withoutTitle(board.overview, "Overview")) +
      doc("Plan", `${path}RESEARCH_PLAN.md`, withoutTitle(board.plan, "Research plan")) +
      doc("What you told the team", `${path}feedback.md`, withoutTitle(board.feedback ?? "", "Feedback"), true) +
      doc("Verdicts", `${path}VERDICT.md`, withoutTitle(board.verdicts, "Verdicts"), true) +
      doc("Sandbox log", `${path}SANDBOX_LOG.md`, withoutTitle(board.sandbox, "Sandbox log"), true) +
      `</div>`;
  }

  /** @param {Verdict[]} verdicts */
  function pips(verdicts) {
    return verdicts.map((v) => `<i class="rs-pip" data-verdict="${esc(v)}" title="${esc(VERDICTS[v] ?? v)}"></i>`).join("");
  }

  /** @param {BoardEvent} board */
  function leaderboardHtml(board) {
    if (!board.hypotheses.length) {
      return `<div class="rs-col"><div class="cc-none rs-none">No hypotheses yet. In a round each agent proposes two,
        the others review them, and a tournament judged blind ranks them by Elo. Each is a file in
        <code>research/${esc(board.slug)}/hypotheses/</code>. Add one of your own with <code>/h</code> below.</div></div>`;
    }
    let rank = 0;
    const rows = board.hypotheses.map((h) => {
      const retired = h.status === "retired";
      if (!retired) rank++;
      const dup = h.duplicateOf ? `<span class="rs-dup" title="Says what ${esc(h.duplicateOf)} says">≈ ${esc(h.duplicateOf)}</span>` : "";
      const pin = h.status === "pinned" ? `<span class="rs-tag">pinned</span>` : "";
      return `<button class="rs-row${retired ? " retired" : ""}" data-research-hypothesis="${esc(h.id)}">` +
        `<span class="rs-rank">${retired ? "–" : rank}</span>` +
        `<span class="rs-name"><b>${esc(h.id)}</b> ${esc(h.title)}${pin}${dup}</span>` +
        `<span class="rs-seat" data-seat="${esc(h.author.seat)}" title="${esc(who(h.author))}">${esc(SEATS[h.author.seat] ?? h.author.seat)}</span>` +
        `<span class="rs-elo">${h.elo}</span>` +
        `<span class="rs-wl">${h.wins}–${h.losses}</span>` +
        `<span class="rs-pips">${pips(h.verdicts)}</span></button>`;
    }).join("");
    return `<div class="rs-col rs-wide"><div class="rs-table" role="list">` +
      `<div class="rs-row rs-row-head" aria-hidden="true"><span class="rs-rank">#</span><span class="rs-name">Hypothesis</span>` +
      `<span class="rs-seat">By</span><span class="rs-elo">Elo</span><span class="rs-wl">W–L</span><span class="rs-pips">Reviews</span></div>` +
      rows + `</div></div>`;
  }

  /** @param {string[]} items */
  function list(items) {
    return items.length ? `<ul>${items.map((i) => `<li>${prose(i).replace(/^<p>|<\/p>$/g, "")}</li>`).join("")}</ul>` : "";
  }

  /** @param {string[]} ids */
  function links(ids) {
    return ids.map((id) => `<button class="rs-link" data-research-hypothesis="${esc(id)}">${esc(id)}</button>`).join(" ");
  }

  /** What can be done to a hypothesis, as it stands. @param {Row} h */
  function actions(h) {
    const button = (/** @type {string} */ status, /** @type {string} */ label, /** @type {string} */ title) =>
      `<button class="cc-quiet" data-research-status="${status}" data-id="${esc(h.id)}" title="${esc(title)}">${label}</button>`;
    if (h.status === "retired") return button("active", "Bring back", "Back into the tournament");
    return (h.status === "pinned" ? button("active", "Unpin", "Let the tournament decide") : button("pinned", "Pin", "Keep it in every round")) +
      button("retired", "Retire", "Out of the tournament");
  }

  /** @param {Detail} d */
  function detailHtml(d) {
    const h = d.row;
    const meta = [who(h.author), `round ${h.round}`, `Elo ${h.elo}`, `${h.wins}–${h.losses}`, h.status].map(esc).join(" · ");
    const lineage = [
      h.parents.length ? `<span>From ${links(h.parents)}</span>` : "",
      d.children.length ? `<span>Evolved into ${links(d.children)}</span>` : "",
      h.duplicateOf ? `<span>Says what ${links([h.duplicateOf])} says</span>` : "",
    ].filter(Boolean).join("");
    const missing = (d.checks ?? []).filter((c) => c.check === "unverified").map((c) => c.text);
    const unfound = missing.length
      ? `${missing.length === 1 ? "A citation" : `${missing.length} citations`} here could not be found: ${missing.join(", ")}. It may be made up or mistyped; the team is told.`
      : "";
    const reviews = d.reviews.map((r) => `<article class="rs-review">
        <header><span class="rs-verdict" data-verdict="${esc(r.verdict)}">${esc(VERDICTS[r.verdict] ?? r.verdict)}</span>
          <span class="rs-review-who">${esc(KINDS[r.kind] ?? r.kind)} by ${esc(who(r.reviewer))}</span>
          <span class="grow"></span>
          <span class="rs-scores" title="Correctness, novelty, testability (1–5)">C ${r.correctness} · N ${r.novelty} · T ${r.testability}</span></header>
        <div class="cc-prose">${prose(r.summary)}</div>
        ${r.critique.trim() ? `<details class="rs-fold"><summary>${r.kind === "verification" ? "Derivation" : "Critique"}</summary><div class="cc-prose">${prose(r.critique)}</div></details>` : ""}
        ${r.citations.length ? `<div class="rs-cites"><ul>${r.citations.map((c) => `<li>${citationHtml(c, d.checks ?? [])}</li>`).join("")}</ul></div>` : ""}
      </article>`).join("");
    const matches = d.matches.map((m) => `<li class="rs-match ${m.won ? "won" : "lost"}">
        <b>${m.won ? "Beat" : "Lost to"}</b> ${links([m.opponent])} ${esc(m.opponentTitle)}
        <span class="rs-faint">— judged by ${esc(who(m.judge))}, round ${m.round}</span>
        ${m.rationale.trim() ? `<div class="cc-prose rs-why">${prose(m.rationale)}</div>` : ""}</li>`).join("");
    return `<div class="rs-col rs-detail">
      <div class="rs-detail-bar"><button class="rs-back" data-research-back>${icon("chev")}All hypotheses</button>
        <span class="grow"></span>${actions(h)}</div>
      <h2 class="rs-h"><span class="rs-id">${esc(h.id)}</span> ${esc(h.title)}</h2>
      <div class="rs-meta">${meta}</div>
      ${lineage ? `<div class="rs-lineage">${lineage}</div>` : ""}
      ${unfound ? `<div class="cc-notice warn rs-unfound">${esc(unfound)}</div>` : ""}
      <section class="rs-doc"><header><span>Statement</span></header><div class="cc-prose">${prose(d.statement)}</div></section>
      ${d.rationale.trim() ? `<section class="rs-doc"><header><span>Rationale</span></header><div class="cc-prose">${prose(d.rationale)}</div></section>` : ""}
      ${d.predictions.length ? `<section class="rs-doc"><header><span>Predictions</span></header><div class="cc-prose">${list(d.predictions)}</div></section>` : ""}
      ${d.assumptions.length ? `<section class="rs-doc"><header><span>Assumptions</span></header><div class="cc-prose">${list(d.assumptions)}</div></section>` : ""}
      ${d.references?.length ? `<section class="rs-doc"><header><span>References</span></header><ul class="rs-refs">${
        d.references.map((ref) => `<li>${referenceHtml(ref, literature()?.papers ?? [], d.checks ?? [])}</li>`).join("")}</ul></section>` : ""}
      ${reviews ? `<section class="rs-doc"><header><span>Reviews</span></header>${reviews}</section>` : ""}
      ${matches ? `<section class="rs-doc"><header><span>Tournament</span></header><ul class="rs-matches">${matches}</ul></section>` : ""}
      ${d.experiments.length ? `<section class="rs-doc"><header><span>Experiments</span></header><p class="rs-faint">${d.experiments.map(esc).join(", ")}</p></section>` : ""}
    </div>`;
  }

  /* ── the round, call by call ───────────────────────────────────── */

  /** @param {Task} t */
  function taskRow(t) {
    const now = Date.now() / 1000;
    const time = t.started == null ? "" : t.ended != null ? span(t.ended - t.started) : span(now - t.started);
    const on = t.id === state.task;
    const why = t.why ? `<span class="rs-task-why">${esc(t.why)}</span>` : "";
    const tries = t.attempts ? `<span class="rs-tag" title="Tried again">×${t.attempts + 1}</span>` : "";
    return `<button class="rs-task${on ? " on" : ""}" data-research-task="${esc(t.id)}" data-state="${esc(t.state)}" aria-expanded="${on}">` +
      `<i class="rs-dot" title="${esc(STATES[t.state] ?? t.state)}"></i>` +
      `<span class="rs-seat" data-seat="${esc(t.seat)}">${esc(SEATS[t.seat] ?? t.seat)}</span>` +
      `<span class="rs-task-what">${t.targets.length ? esc(t.targets.join(", ")) : esc(STATES[t.state] ?? t.state)}${tries}${why}</span>` +
      `<span class="rs-task-model">${esc(t.model)}</span>` +
      `<span class="rs-task-time">${esc(time)}</span></button>` +
      (on ? `<div class="rs-transcript">${transcriptHtml()}</div>` : "");
  }

  /** The round's search of the literature, a row of its own: no agent's call, Proton's.
   *  @param {LiteratureStep} lit */
  function literatureStepHtml(lit) {
    const now = Date.now() / 1000;
    const time = lit.started == null ? "" : lit.ended != null ? span(lit.ended - lit.started) : span(now - lit.started);
    const asked = lit.queries.map((q) => `“${q}”`).join(", ");
    const what = lit.state === "due" ? "Searches once the plan is framed"
      : lit.state === "searching" ? `Searching for ${asked}; the hypotheses wait for it`
      : lit.state === "overdue" ? `Searching for ${asked}; the hypotheses went on without it`
      : lit.state === "skipped" ? `Not searched: ${lit.why ?? ""}`
      : lit.added.length ? `Found for ${asked}:` : `Nothing new for ${asked}`;
    const added = lit.added.map((id) => `<button class="rs-link" data-lit-paper="${esc(id)}">${esc(id)}</button>`).join(" ");
    const state = { due: "ready", searching: "running", overdue: "running", done: "done", skipped: "skipped" }[lit.state] ?? "ready";
    const notes = lit.notes.length ? `<span class="rs-task-why">${esc(lit.notes.join("; "))}</span>` : "";
    return `<div class="rs-task rs-lit-step" data-state="${state}">` +
      `<i class="rs-dot"></i><span class="rs-seat">Proton</span>` +
      `<span class="rs-task-what">${esc(what)} ${added}${notes}</span>` +
      `<span class="rs-task-model">no agent call</span><span class="rs-task-time">${esc(time)}</span></div>`;
  }

  function activityHtml() {
    const r = round();
    if (!r || !r.tasks.length) {
      return `<div class="rs-col"><div class="cc-none rs-none">Nothing has run yet. A round is about fourteen agent calls —
        framing, two hypotheses from each seat, reviews, a verification, a tournament, evolution and an overview —
        and never more than ${r?.cap ?? 20}. Each call is listed here as it runs, with what it said.</div></div>`;
    }
    const groups = PHASES.map(([phase, name]) => {
      const tasks = r.tasks.filter((t) => t.phase === phase);
      // The literature is searched between the plan and the hypotheses.
      const lit = phase === "generate" && r.literature
        ? `<section class="rs-phase"><header>Literature<span class="rs-faint">${r.literature.added.length ? `${r.literature.added.length} added` : ""}</span></header>` +
          literatureStepHtml(r.literature) + `</section>` : "";
      if (!tasks.length) return lit;
      return lit + `<section class="rs-phase"><header>${esc(name)}<span class="rs-faint">${tasks.filter((t) => t.state === "done").length}/${tasks.length}</span></header>` +
        tasks.map(taskRow).join("") + `</section>`;
    }).join("");
    return `<div class="rs-col rs-wide">${groups}</div>`;
  }

  /** What a call said, drawn plainly: the agent's words, its tools as a line
   *  each, and how it ended. */
  function transcriptHtml() {
    const t = state.transcript;
    if (!t || t.task !== state.task) return `<div class="rs-faint">Reading…</div>`;
    if (!t.items.length) return `<div class="rs-faint">Nothing was kept of this call.</div>`;
    return t.items.map((item) => {
      switch (item.kind) {
        case "user":
          return `<details class="rs-said"><summary>What it was asked</summary><div class="cc-prose">${markdown(item.text)}</div></details>`;
        case "text":
          return item.text.trim() ? `<div class="cc-prose">${markdown(item.text, { streaming: !item.done, cite: true })}</div>` : "";
        case "thinking":
          return item.text.trim() ? `<details class="rs-said"><summary>Thought</summary><div class="cc-prose">${markdown(item.text)}</div></details>` : "";
        case "tool": {
          const input = /** @type {any} */ (item.input ?? {});
          const what = String(input.query ?? input.url ?? input.pattern ?? input.file_path ?? input.command ?? Object.values(input).find((v) => typeof v === "string") ?? "");
          return `<div class="rs-tool" data-state="${esc(item.state)}"><b>${esc(item.name)}</b> <span>${esc(what.split("\n")[0].slice(0, 160))}</span></div>`;
        }
        case "notice":
          return `<div class="cc-notice ${esc(item.level)}">${esc(item.text)}</div>`;
        case "turn":
          return item.error ? `<div class="cc-notice error">${esc(item.error)}</div>` : `<div class="rs-faint">Answered in ${span(item.ms / 1000)}.</div>`;
        default:
          return "";
      }
    }).join("");
  }

  /* ── a new investigation ───────────────────────────────────────── */

  function renderForm() {
    const d = state.draft;
    els.board.innerHTML = `<form class="rs-col rs-new" data-research-form novalidate>
      <p class="rs-faint">A folder ${state.folder === "research/" ? "under <code>research/</code> in this project" : `in <code>${esc(state.folder)}</code>`}, with your brief as its README. Nothing runs until you start a round.</p>
      <label class="rs-field"><span>Title</span>
        <input name="title" maxlength="120" autocomplete="off" spellcheck="true" required
               placeholder="e.g. Why is the daytime sky blue rather than violet?" value="${esc(d.title)}"></label>
      <label class="rs-field"><span>Discipline</span>
        <select name="discipline">${DISCIPLINES.map(([value, name]) =>
          `<option value="${value}"${value === d.discipline ? " selected" : ""}>${name}</option>`).join("")}</select></label>
      <label class="rs-field"><span>Goal</span>
        <textarea name="goal" rows="7" spellcheck="true"
          placeholder="What a good answer has to explain or prove, what is known already, and anything the team must keep to.">${esc(d.goal)}</textarea></label>
      <label class="lit-switch"><input type="checkbox" name="literature"${d.literature ? " checked" : ""}>
        <span><b>Search the literature each round</b><small>The team's queries go to arXiv, Semantic Scholar and OpenAlex, which see the
          queries but never your files; the papers they find are added, with their open PDFs.</small></span></label>
      <div class="rs-actions">
        <button type="button" class="cc-quiet" data-research-cancel>Cancel</button>
        <button type="submit" class="cc-primary">Create investigation</button>
      </div>
    </form>`;
    /** @type {HTMLInputElement | null} */ (els.board.querySelector('[name="title"]'))?.focus({ preventScroll: true });
  }

  /** Where the investigations are kept, and a way to keep them elsewhere. */
  function renderWhere() {
    if (state.moving !== null) {
      // Drawn once while open, so a redraw neither takes the caret nor loses what was typed.
      if (els.where.querySelector("[data-research-where-form]")) return;
      els.where.innerHTML = `<form class="rs-where-form" data-research-where-form novalidate>
          <label class="rs-field"><span>Keep every project's investigations in</span>
            <input name="folder" value="${esc(state.moving)}" placeholder="each project's own research/" spellcheck="false" autocomplete="off"></label>
          <div class="rs-actions">
            <button type="button" class="cc-quiet" data-research-where-cancel>Cancel</button>
            <button type="submit" class="cc-primary">Keep them here</button>
          </div>
        </form>`;
      const input = /** @type {HTMLInputElement | null} */ (els.where.querySelector("input"));
      input?.focus({ preventScroll: true });
      input?.select();
      return;
    }
    const own = state.folder === "research/";
    els.where.innerHTML = `<span>Kept in ${own ? "each project's " : ""}<code>${esc(state.folder)}</code></span>` +
      `<button class="rs-where-change" data-research-where-change title="${own
        ? "Keep every project's investigations in one folder" : "Keep them somewhere else, or in each project again"}">Change</button>`;
  }

  function newInvestigation() {
    state.creating = true;
    renderList();
    renderBoard();
  }

  /** @param {HTMLFormElement} form */
  function submit(form) {
    const data = new FormData(form);
    const title = String(data.get("title") ?? "").trim();
    state.draft = {
      title,
      goal: String(data.get("goal") ?? "").trim(),
      discipline: /** @type {Discipline} */ (String(data.get("discipline") ?? "physics")),
      literature: data.get("literature") === "on",
    };
    if (!title) {
      /** @type {HTMLInputElement | null} */ (form.querySelector('[name="title"]'))?.focus();
      return;
    }
    state.made = state.list.length;
    host.dispatch({ type: "researchNew", ...state.draft });
  }

  /* ── the composer ──────────────────────────────────────────────── */

  function renderComposer() {
    const text = els.input.value.trim();
    els.send.disabled = !text;
    const own = ownHypothesis(text);
    const paper = own ? null : ownPaper(text);
    els.hint.textContent = own ? `⏎ adds “${own.title}” as your hypothesis`
      : paper ? `⏎ looks up ${paper} and adds it to the literature`
      : "⏎ tells the team · /h Title: statement adds a hypothesis · /p and a DOI adds a paper";
  }

  function autosize() {
    els.input.style.height = "auto";
    // Empty, it is a line: Chrome measures a placeholder that wraps as text.
    if (els.input.value) els.input.style.height = `${Math.min(els.input.scrollHeight, 220)}px`;
  }

  function say() {
    const text = els.input.value.trim();
    if (!text || !state.board) return;
    const own = ownHypothesis(text);
    const paper = own ? null : ownPaper(text);
    host.dispatch(own ? { type: "researchAddHypothesis", ...own }
      : paper ? { type: "researchAddPaper", paper }
      : { type: "researchFeedback", text });
    els.input.value = "";
    autosize();
    renderComposer();
  }

  els.input.addEventListener("input", () => { autosize(); renderComposer(); });
  els.input.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      say();
    }
  });

  /* ── what is clicked ───────────────────────────────────────────── */

  /** @param {Tab} tab */
  function showTab(tab) {
    // The tab that is on already, asked for again, goes back to its top.
    if (tab === "hypotheses" && state.tab === "hypotheses" && state.detail) {
      host.dispatch({ type: "researchShow", hypothesis: null });
    }
    if (tab === "literature" && state.tab === "literature" && state.paper) {
      host.dispatch({ type: "researchShowPaper", paper: null });
    }
    state.tab = tab;
    renderBoard();
  }

  ws.addEventListener("click", (e) => {
    const target = /** @type {HTMLElement} */ (e.target);
    if (target.closest("[data-research-new]")) return newInvestigation();
    if (target.closest("[data-research-where-change]")) {
      state.moving = state.folder === "research/" ? "" : state.folder.replace(/[\\/]$/, "");
      return renderWhere();
    }
    if (target.closest("[data-research-where-cancel]")) {
      state.moving = null;
      return renderWhere();
    }
    if (target.closest("[data-research-cancel]")) {
      state.creating = false;
      renderList();
      return renderBoard();
    }
    if (target.closest("[data-research-send]")) return say();
    if (target.closest("[data-research-go]")) {
      host.dispatch({ type: round()?.status.status === "paused" ? "researchResume" : "researchStart" });
      return;
    }
    if (target.closest("[data-research-pause]")) return void host.dispatch({ type: "researchPause" });
    if (target.closest("[data-research-stop]")) return void host.dispatch({ type: "researchStop" });
    const open = /** @type {HTMLElement | null} */ (target.closest("[data-research-open]"));
    if (open) {
      const slug = open.dataset.researchOpen ?? "";
      const leaving = state.creating;
      state.creating = false;
      if (slug === state.open) {
        if (leaving) { renderList(); renderBoard(); }
        return;
      }
      host.dispatch({ type: "researchOpen", slug });
      return;
    }
    const tab = /** @type {HTMLElement | null} */ (target.closest("[data-research-tab]"));
    if (tab) return showTab(/** @type {Tab} */ (tab.dataset.researchTab));
    const status = /** @type {HTMLElement | null} */ (target.closest("[data-research-status]"));
    if (status) {
      host.dispatch({ type: "researchSetStatus", hypothesis: status.dataset.id, status: status.dataset.researchStatus });
      return;
    }
    const hypothesis = /** @type {HTMLElement | null} */ (target.closest("[data-research-hypothesis]"));
    if (hypothesis) {
      host.dispatch({ type: "researchShow", hypothesis: hypothesis.dataset.researchHypothesis ?? null });
      return;
    }
    if (target.closest("[data-research-back]")) {
      host.dispatch({ type: "researchShow", hypothesis: null });
      return;
    }
    if (literatureClick(target)) return;
    const task = /** @type {HTMLElement | null} */ (target.closest("[data-research-task]"));
    if (task) {
      const id = task.dataset.researchTask ?? null;
      state.task = id === state.task ? null : id;
      host.dispatch({ type: "researchTask", task: state.task });
      renderBoard();
      return;
    }
    // In what the agents wrote a link opens in the browser, and a code
    // block's button copies it.
    const link = /** @type {HTMLAnchorElement | null} */ (target.closest("a[data-link]"));
    if (link) {
      e.preventDefault();
      host.dispatch({ type: "openLink", url: link.getAttribute("href") });
      return;
    }
    const copy = /** @type {HTMLElement | null} */ (target.closest("[data-copy]"));
    if (copy) {
      const code = copy.closest(".md-code")?.querySelector("code")?.textContent ?? "";
      navigator.clipboard?.writeText(code).then(() => {
        copy.textContent = "Copied";
        setTimeout(() => { copy.textContent = "Copy"; }, 1200);
      }).catch(() => {});
    }
  });

  /** What is clicked in the Literature tab; whether it was. @param {HTMLElement} target */
  function literatureClick(target) {
    const lit = literature();
    const source = /** @type {HTMLElement | null} */ (target.closest("[data-lit-source]"));
    if (source && lit) {
      const which = /** @type {Source} */ (source.dataset.litSource);
      const chosen = lit.sources.filter((r) => r.chosen).map((r) => r.source);
      const sources = chosen.includes(which) ? chosen.filter((s) => s !== which) : [...chosen, which];
      host.dispatch({ type: "researchSetLiterature", enabled: null, sources });
      return true;
    }
    const add = /** @type {HTMLElement | null} */ (target.closest("[data-lit-add]"));
    if (add) {
      const key = add.dataset.litAdd ?? "";
      state.adding = [...state.adding, key];
      host.dispatch({ type: "researchAddPaper", paper: key });
      renderBoard();
      return true;
    }
    const paper = /** @type {HTMLElement | null} */ (target.closest("[data-lit-paper]"));
    if (paper) {
      host.dispatch({ type: "researchShowPaper", paper: paper.dataset.litPaper ?? null });
      return true;
    }
    if (target.closest("[data-lit-back]")) {
      host.dispatch({ type: "researchShowPaper", paper: null });
      return true;
    }
    const open = /** @type {HTMLElement | null} */ (target.closest("[data-lit-open]"));
    if (open) {
      host.dispatch({ type: "researchOpenPaper", paper: open.dataset.litOpen, page: null });
      return true;
    }
    const again = /** @type {HTMLElement | null} */ (target.closest("[data-lit-fetch]"));
    if (again) {
      host.dispatch({ type: "researchFetchPdf", paper: again.dataset.litFetch });
      return true;
    }
    if (target.closest("[data-lit-clear]")) {
      state.results = null;
      renderBoard();
      return true;
    }
    const copy = /** @type {HTMLElement | null} */ (target.closest("[data-lit-copy]"));
    if (copy) {
      navigator.clipboard?.writeText(copy.dataset.litCopy ?? "").then(() => {
        copy.textContent = "Copied";
        setTimeout(() => { copy.textContent = "Copy path"; }, 1200);
      }).catch(() => {});
      return true;
    }
    return false;
  }

  ws.addEventListener("change", (e) => {
    const box = /** @type {HTMLInputElement} */ (e.target);
    if (box.matches?.("[data-lit-switch]")) {
      host.dispatch({ type: "researchSetLiterature", enabled: box.checked, sources: null });
      return;
    }
    if (!box.matches?.("[data-research-seat]")) return;
    host.dispatch({ type: "researchSeat", seat: box.dataset.researchSeat, enabled: box.checked });
  });

  ws.addEventListener("submit", (e) => {
    if (/** @type {HTMLElement} */ (e.target).closest("[data-research-where-form]")) {
      e.preventDefault();
      // Put away when the core says where they are kept now; a refusal keeps it.
      state.moved = true;
      host.dispatch({ type: "researchFolder", folder: (state.moving ?? "").trim() });
      return;
    }
    if (/** @type {HTMLElement} */ (e.target).closest("[data-lit-search]")) {
      e.preventDefault();
      const query = state.query.trim();
      if (query) host.dispatch({ type: "researchSearch", query });
      return;
    }
    const form = /** @type {HTMLElement} */ (e.target).closest("[data-research-form]");
    if (!form) return;
    e.preventDefault();
    submit(/** @type {HTMLFormElement} */ (form));
  });

  // What is typed is kept, so a refusal — or a look at another
  // investigation and back — does not lose it.
  ws.addEventListener("input", (e) => {
    const field = /** @type {HTMLInputElement} */ (e.target);
    if (field.closest?.("[data-research-where-form]")) {
      state.moving = field.value;
      return;
    }
    if (field.matches?.("[data-lit-query]")) {
      state.query = field.value;
      return;
    }
    if (!field.closest?.("[data-research-form]")) return;
    if (field.name === "title" || field.name === "goal") state.draft[field.name] = field.value;
    if (field.name === "discipline") state.draft.discipline = /** @type {Discipline} */ (field.value);
    if (field.name === "literature") state.draft.literature = field.checked;
  });

  // ⌘⏎ creates, from anywhere in the form; Escape puts it away.
  ws.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && /** @type {HTMLElement} */ (e.target).closest?.("[data-research-where-form]")) {
      e.preventDefault();
      e.stopPropagation();
      state.moving = null;
      return renderWhere();
    }
    const form = /** @type {HTMLElement} */ (e.target).closest?.("[data-research-form]");
    if (!form) return;
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      submit(/** @type {HTMLFormElement} */ (form));
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      state.creating = false;
      renderList();
      renderBoard();
    }
  });

  /* ── what the core sends ───────────────────────────────────────── */

  /** @param {ListEvent} event */
  function list_(event) {
    state.list = event.investigations ?? [];
    state.folder = event.folder || "research/";
    if (state.moved) {
      state.moving = null;
      state.moved = false;
    }
    renderWhere();
    if (state.creating && state.made >= 0 && state.list.length > state.made) {
      state.creating = false;
      state.made = -1;
      state.draft = { title: "", goal: "", discipline: state.draft.discipline, literature: state.draft.literature };
      state.tab = "overview";
    }
    if (event.open !== state.open) {
      state.open = event.open;
      state.detail = null;
      state.task = null;
      state.transcript = null;
      state.paper = null;
      state.adding = [];
    }
    if (!state.open) state.board = null;
    renderList();
    renderBoard();
  }

  /** @param {BoardEvent} event */
  function board(event) {
    if (event.slug !== state.board?.slug) state.tab = "overview";
    state.board = event;
    state.open = event.slug;
    renderList();
    renderBoard();
  }

  /** @param {HypothesisEvent} event */
  function hypothesis(event) {
    if (event.slug !== state.open) return;
    const was = state.detail?.row.id;
    state.detail = event.detail;
    if (event.detail) state.tab = "hypotheses";
    renderBoard();
    if (event.detail && event.detail.row.id !== was) els.board.scrollTop = 0;
  }

  /** @param {RoundEvent} event */
  function round_(event) {
    const starting = event.slug === state.open && event.status.status === "running" && round()?.status.status !== "running";
    state.round = event;
    // A round just started is followed where it happens.
    if (starting && !state.detail) state.tab = "activity";
    renderList();
    renderBoard();
  }

  /** @param {LiteratureEvent} event */
  function literature_(event) {
    state.literature = event;
    // What was asked to be added is in the list now, or was refused.
    state.adding = [];
    renderHead();
    if (state.tab === "literature") renderBoard();
  }

  /** @param {PaperEvent} event */
  function paper(event) {
    if (event.slug !== state.open) return;
    const was = state.paper?.row.id;
    state.paper = event.detail;
    if (event.detail) state.tab = "literature";
    renderBoard();
    if (event.detail && event.detail.row.id !== was) els.board.scrollTop = 0;
  }

  /** @param {ResultsEvent} event */
  function results(event) {
    if (event.slug !== state.open) return;
    state.results = event;
    if (!state.query) state.query = event.query;
    if (state.tab === "literature") renderBoard();
  }

  /** @param {TranscriptEvent} event */
  function transcript(event) {
    if (event.slug !== state.open || event.task !== state.task) return;
    state.transcript = event;
    const box = els.board.querySelector(".rs-transcript");
    // Only the transcript is redrawn: the list above it stays as it is.
    if (box) box.innerHTML = transcriptHtml();
    else renderBoard();
  }

  renderList();
  renderBoard();
  renderComposer();

  return { list: list_, board, hypothesis, round: round_, transcript, literature: literature_, paper, results };
}

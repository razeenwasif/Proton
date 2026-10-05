/* Research: investigations, and the co-scientist round that grows them.
 *
 * Each investigation is a folder in the project — `research/<slug>/` — and
 * this draws what the core reads out of it: the investigations down the
 * left; in the middle where the open one stands (its overview, its plan,
 * the brief it answers) and its hypotheses as the tournament ranks them;
 * and the Lab, where its code is run, on the right.
 *
 * Nothing here decides anything. The files are the investigation, the core
 * reads them, and this draws what it is sent — so a hypothesis edited by
 * hand is what the board shows the next time it is drawn. */

import { escapeHtml as esc, icon } from "./html.js";
import { markdown } from "./markdown.js";

/** @typedef {import("./bindings/ResearchSummary").ResearchSummary} Summary */
/** @typedef {import("./bindings/HypothesisRow").HypothesisRow} Row */
/** @typedef {import("./bindings/HypothesisDetail").HypothesisDetail} Detail */
/** @typedef {import("./bindings/ResearchDiscipline").ResearchDiscipline} Discipline */
/** @typedef {import("./bindings/ResearchAuthor").ResearchAuthor} Author */
/** @typedef {import("./bindings/ReviewVerdict").ReviewVerdict} Verdict */
/** @typedef {{investigations: Summary[], open: string | null}} ListEvent */
/** @typedef {{slug: string, title: string, discipline: Discipline, brief: string, plan: string,
 *   overview: string, verdicts: string, sandbox: string, hypotheses: Row[]}} BoardEvent */
/** @typedef {{slug: string, detail: Detail | null}} HypothesisEvent */
/** @typedef {"overview" | "hypotheses"} Tab */

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

const VERDICTS = /** @type {Record<Verdict, string>} */ ({
  strong: "Strong", promising: "Promising", weak: "Weak", flawed: "Flawed",
});

const KINDS = /** @type {Record<string, string>} */ ({
  review: "Review", verification: "Verification", simulation: "Simulation review",
});

/** @param {Author} author */
function who(author) {
  const seat = SEATS[author.seat] ?? author.seat;
  return author.model && author.seat !== "scientist" ? `${seat} · ${author.model}` : seat;
}

/** @param {number} n @param {string} one @param {string} many */
const count = (n, one, many) => `${n} ${n === 1 ? one : many}`;

/** A file's heading is the board's own: the title is already over it.
 *  @param {string} text @param {string} title */
function withoutTitle(text, title) {
  const m = /^#\s+(.*)\n+/.exec(text);
  return m && m[1].trim() === title.trim() ? text.slice(m[0].length) : text;
}

/** @param {{root: Document, dispatch: (command: any) => unknown}} host */
export function createResearch(host) {
  const ws = /** @type {HTMLElement | null} */ (host.root.querySelector('.workspace[data-ws="research"]'));
  if (!ws) return null;
  const q = (/** @type {string} */ sel) => /** @type {HTMLElement} */ (ws.querySelector(sel));
  const els = {
    list: q("[data-research-list]"),
    title: q("[data-research-title]"),
    discipline: q("[data-research-discipline]"),
    tabs: q("[data-research-tabs]"),
    count: q("[data-research-count]"),
    board: q("[data-research-board]"),
  };

  /** @type {{list: Summary[], open: string | null, board: BoardEvent | null, detail: Detail | null,
   *   tab: Tab, creating: boolean, made: number, draft: {title: string, goal: string, discipline: Discipline}}} */
  const state = {
    list: [], open: null, board: null, detail: null, tab: "overview",
    // The form is up, and how many there were when it was sent: it goes when
    // the list grows, and stays with what was typed when the core refuses.
    creating: false, made: -1,
    draft: { title: "", goal: "", discipline: "physics" },
  };

  /* ── the list ──────────────────────────────────────────────────── */

  function renderList() {
    if (!state.list.length) {
      els.list.innerHTML = `<div class="cc-none">No investigations in this project yet. Each is a folder under <code>research/</code>, and starts from a question.</div>`;
      return;
    }
    els.list.innerHTML = state.list.map((i) => {
      const on = i.slug === state.open && !state.creating;
      const where = `research/${i.slug}/ · ${count(i.hypotheses, "hypothesis", "hypotheses")} · ${count(i.rounds, "round", "rounds")}`;
      return `<button class="cc-session rs-item${on ? " on" : ""}" data-research-open="${esc(i.slug)}" title="${esc(where)}"` +
        `${on ? ' aria-current="true"' : ""}>` +
        `<span class="cc-session-name">${esc(i.title)}</span>` +
        `<span class="cc-session-where">${esc(DISCIPLINE[i.discipline] ?? "")}</span></button>`;
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
  }

  /* ── the board ─────────────────────────────────────────────────── */

  function renderBoard() {
    renderHead();
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
    els.board.innerHTML = state.tab === "hypotheses"
      ? (state.detail ? detailHtml(state.detail) : leaderboardHtml(board))
      : overviewHtml(board);
  }

  /** @param {string} label @param {string} file @param {string} text @param {boolean} [folded] */
  function doc(label, file, text, folded = false) {
    const body = `<div class="cc-prose">${markdown(text)}</div>`;
    const head = `<span>${esc(label)}</span><code class="rs-file" title="${esc(file)}">${esc(file.split("/").pop() ?? file)}</code>`;
    return folded
      ? `<details class="rs-doc rs-fold"><summary>${head}</summary>${body}</details>`
      : `<section class="rs-doc"><header>${head}</header>${body}</section>`;
  }

  /** @param {BoardEvent} board */
  function overviewHtml(board) {
    const path = `research/${board.slug}/`;
    return `<div class="rs-col">` +
      doc("Brief", `${path}README.md`, withoutTitle(board.brief, board.title)) +
      doc("Overview", `${path}overview.md`, withoutTitle(board.overview, "Overview")) +
      doc("Plan", `${path}RESEARCH_PLAN.md`, withoutTitle(board.plan, "Research plan")) +
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
        <code>research/${esc(board.slug)}/hypotheses/</code>.</div></div>`;
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
    return items.length ? `<ul>${items.map((i) => `<li>${markdown(i).replace(/^<p>|<\/p>$/g, "")}</li>`).join("")}</ul>` : "";
  }

  /** @param {string[]} ids */
  function links(ids) {
    return ids.map((id) => `<button class="rs-link" data-research-hypothesis="${esc(id)}">${esc(id)}</button>`).join(" ");
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
    const reviews = d.reviews.map((r) => `<article class="rs-review">
        <header><span class="rs-verdict" data-verdict="${esc(r.verdict)}">${esc(VERDICTS[r.verdict] ?? r.verdict)}</span>
          <span class="rs-review-who">${esc(KINDS[r.kind] ?? r.kind)} by ${esc(who(r.reviewer))}</span>
          <span class="grow"></span>
          <span class="rs-scores" title="Correctness, novelty, testability (1–5)">C ${r.correctness} · N ${r.novelty} · T ${r.testability}</span></header>
        <div class="cc-prose">${markdown(r.summary)}</div>
        ${r.critique.trim() ? `<details class="rs-fold"><summary>Critique</summary><div class="cc-prose">${markdown(r.critique)}</div></details>` : ""}
        ${r.citations.length ? `<div class="rs-cites">${list(r.citations)}</div>` : ""}
      </article>`).join("");
    const matches = d.matches.map((m) => `<li class="rs-match ${m.won ? "won" : "lost"}">
        <b>${m.won ? "Beat" : "Lost to"}</b> ${links([m.opponent])} ${esc(m.opponentTitle)}
        <span class="rs-faint">— judged by ${esc(who(m.judge))}, round ${m.round}</span>
        ${m.rationale.trim() ? `<div class="cc-prose rs-why">${markdown(m.rationale)}</div>` : ""}</li>`).join("");
    return `<div class="rs-col rs-detail">
      <button class="rs-back" data-research-back>${icon("chev")}All hypotheses</button>
      <h2 class="rs-h"><span class="rs-id">${esc(h.id)}</span> ${esc(h.title)}</h2>
      <div class="rs-meta">${meta}</div>
      ${lineage ? `<div class="rs-lineage">${lineage}</div>` : ""}
      <section class="rs-doc"><header><span>Statement</span></header><div class="cc-prose">${markdown(d.statement)}</div></section>
      ${d.rationale.trim() ? `<section class="rs-doc"><header><span>Rationale</span></header><div class="cc-prose">${markdown(d.rationale)}</div></section>` : ""}
      ${d.predictions.length ? `<section class="rs-doc"><header><span>Predictions</span></header><div class="cc-prose">${list(d.predictions)}</div></section>` : ""}
      ${d.assumptions.length ? `<section class="rs-doc"><header><span>Assumptions</span></header><div class="cc-prose">${list(d.assumptions)}</div></section>` : ""}
      ${reviews ? `<section class="rs-doc"><header><span>Reviews</span></header>${reviews}</section>` : ""}
      ${matches ? `<section class="rs-doc"><header><span>Tournament</span></header><ul class="rs-matches">${matches}</ul></section>` : ""}
      ${d.experiments.length ? `<section class="rs-doc"><header><span>Experiments</span></header><p class="rs-faint">${d.experiments.map(esc).join(", ")}</p></section>` : ""}
    </div>`;
  }

  /* ── a new investigation ───────────────────────────────────────── */

  function renderForm() {
    const d = state.draft;
    els.board.innerHTML = `<form class="rs-col rs-new" data-research-form novalidate>
      <p class="rs-faint">A folder under <code>research/</code> in this project, with your brief as its README. Nothing runs until you start a round.</p>
      <label class="rs-field"><span>Title</span>
        <input name="title" maxlength="120" autocomplete="off" spellcheck="true" required
               placeholder="e.g. Why is the daytime sky blue rather than violet?" value="${esc(d.title)}"></label>
      <label class="rs-field"><span>Discipline</span>
        <select name="discipline">${DISCIPLINES.map(([value, name]) =>
          `<option value="${value}"${value === d.discipline ? " selected" : ""}>${name}</option>`).join("")}</select></label>
      <label class="rs-field"><span>Goal</span>
        <textarea name="goal" rows="7" spellcheck="true"
          placeholder="What a good answer has to explain or prove, what is known already, and anything the team must keep to.">${esc(d.goal)}</textarea></label>
      <div class="rs-actions">
        <button type="button" class="cc-quiet" data-research-cancel>Cancel</button>
        <button type="submit" class="cc-primary">Create investigation</button>
      </div>
    </form>`;
    /** @type {HTMLInputElement | null} */ (els.board.querySelector('[name="title"]'))?.focus({ preventScroll: true });
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
    };
    if (!title) {
      /** @type {HTMLInputElement | null} */ (form.querySelector('[name="title"]'))?.focus();
      return;
    }
    state.made = state.list.length;
    host.dispatch({ type: "researchNew", ...state.draft });
  }

  /* ── what is clicked ───────────────────────────────────────────── */

  ws.addEventListener("click", (e) => {
    const target = /** @type {HTMLElement} */ (e.target);
    if (target.closest("[data-research-new]")) return newInvestigation();
    if (target.closest("[data-research-cancel]")) {
      state.creating = false;
      renderList();
      return renderBoard();
    }
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
    if (tab) {
      const next = /** @type {Tab} */ (tab.dataset.researchTab);
      // The tab that is on already, asked for again, goes back to its top.
      if (next === "hypotheses" && state.tab === "hypotheses" && state.detail) {
        host.dispatch({ type: "researchShow", hypothesis: null });
      }
      state.tab = next;
      return renderBoard();
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

  ws.addEventListener("submit", (e) => {
    const form = /** @type {HTMLElement} */ (e.target).closest("[data-research-form]");
    if (!form) return;
    e.preventDefault();
    submit(/** @type {HTMLFormElement} */ (form));
  });

  // What is typed is kept, so a refusal — or a look at another
  // investigation and back — does not lose it.
  ws.addEventListener("input", (e) => {
    const field = /** @type {HTMLInputElement} */ (e.target);
    if (!field.closest?.("[data-research-form]")) return;
    if (field.name === "title" || field.name === "goal") state.draft[field.name] = field.value;
    if (field.name === "discipline") state.draft.discipline = /** @type {Discipline} */ (field.value);
  });

  // ⌘⏎ creates, from anywhere in the form.
  ws.addEventListener("keydown", (e) => {
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
    if (state.creating && state.made >= 0 && state.list.length > state.made) {
      state.creating = false;
      state.made = -1;
      state.draft = { title: "", goal: "", discipline: state.draft.discipline };
      state.tab = "overview";
    }
    if (event.open !== state.open) {
      state.open = event.open;
      state.detail = null;
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

  renderList();
  renderBoard();

  return { list: list_, board, hypothesis };
}

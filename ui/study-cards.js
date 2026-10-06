/* Flashcards: a deck of cards made from a notebook's sources, and review,
 * card by card, scheduled to be remembered.
 *
 * A deck opens in the middle: its cards, each with where it is in being
 * learnt, and what of it is due today. A session of review shows a card's
 * front; the answer is turned over with Space, and rated 1 to 4 — Missed it,
 * Hard, Got it, Easy — each rating labelled with when the card would come
 * back. The core schedules every card (FSRS) from its deck's log and keeps
 * each rating as it is made; this draws what it says. */

import { escapeHtml as esc, icon } from "./html.js";
import { markdown } from "./markdown.js";
import { answerHtml } from "./study-chat.js";

/** @typedef {import("./bindings/StudyMade").StudyMade} Made */
/** @typedef {import("./bindings/StudyCard").StudyCard} Card */
/** @typedef {import("./bindings/StudyCardRow").StudyCardRow} Row */
/** @typedef {import("./bindings/StudyCounts").StudyCounts} Counts */
/** @typedef {import("./bindings/StudyCitation").StudyCitation} Citation */
/** @typedef {import("./bindings/StudyReview").StudyReview} Review */
/** @typedef {import("./bindings/StudyRating").StudyRating} Rating */
/** @typedef {import("./bindings/StudyReviewMode").StudyReviewMode} Mode */
/** @typedef {{made: Made, cards: Row[], counts: Counts, citations: Citation[]}} DeckEvent */

/** The ratings, in the order of their keys: what each is called, and its key. */
export const RATINGS = /** @type {[Rating, string, string][]} */ ([
  ["again", "Missed it", "1"], ["hard", "Hard", "2"], ["good", "Got it", "3"], ["easy", "Easy", "4"],
]);

const MODES = /** @type {Record<Mode, string>} */ ({ due: "Due today", cram: "Cramming", missed: "Missed lately" });
const STATES = /** @type {Record<string, string>} */ ({ new: "New", learning: "Learning", review: "Review", relearning: "Relearning" });

/** A length of time, as short as it can be said. @param {number} seconds */
export function interval(seconds) {
  const s = Math.max(0, seconds);
  if (s < 60) return "<1 min";
  if (s < 3600) return `${Math.round(s / 60)} min`;
  if (s < 86400) return `${Math.round(s / 3600)} h`;
  const days = s / 86400;
  if (days < 30) return `${Math.round(days)} d`;
  if (days < 365) return `${(days / 30).toFixed(days < 300 ? 1 : 0).replace(/\.0$/, "")} mo`;
  return `${(days / 365).toFixed(1).replace(/\.0$/, "")} y`;
}

/** A cloze's hidden parts, `{{c1::answer}}` or `{{c1::answer::hint}}`, as
 *  marks: braces inside counted, so `{{c1::$\frac{a}{b}$}}` is one.
 *  @param {string} text @returns {{marked: string, parts: {answer: string, hint: string | null}[]}} */
export function cloze(text) {
  /** @type {{answer: string, hint: string | null}[]} */
  const parts = [];
  let marked = "";
  let rest = String(text);
  for (;;) {
    const at = rest.search(/\{\{c\d+::/);
    if (at < 0) break;
    const open = rest.slice(at).match(/^\{\{c\d+::/);
    const body = rest.slice(at + (open ? open[0].length : 0));
    let depth = 0;
    let end = -1;
    for (let i = 0; i < body.length; i++) {
      const c = body[i];
      if (c === "{") depth++;
      else if (c === "}" && depth === 0 && body[i + 1] === "}") {
        end = i;
        break;
      } else if (c === "}") depth--;
    }
    if (end < 0) break;
    const inner = body.slice(0, end);
    const split = inner.lastIndexOf("::");
    parts.push(split >= 0 ? { answer: inner.slice(0, split), hint: inner.slice(split + 2) } : { answer: inner, hint: null });
    marked += `${rest.slice(0, at)}${parts.length - 1}`;
    rest = body.slice(end + 2);
  }
  return { marked: marked + rest, parts };
}

/** A side of a card, drawn: Markdown and TeX, its citations as chips, a
 *  cloze's parts hidden or shown.
 *  @param {string} text @param {Citation[]} citations @param {"hide" | "show"} parts */
export function sideHtml(text, citations, parts) {
  const { marked, parts: found } = cloze(text);
  const inline = (/** @type {string} */ s) => markdown(s).replace(/^<p>|<\/p>\s*$/g, "");
  return answerHtml(marked, citations).replace(/(\d+)/g, (_, i) => {
    const part = found[Number(i)];
    if (!part) return "";
    return parts === "show"
      ? `<span class="st-cloze shown">${inline(part.answer)}</span>`
      : `<span class="st-cloze">${part.hint ? esc(part.hint) : "…"}</span>`;
  });
}

/** When a card is next due, in words. @param {Row} row */
function dueHtml(row) {
  if (row.due === null) return "";
  const left = row.due - Math.floor(Date.now() / 1000);
  return left <= 0 ? "due now" : `due in ${interval(left)}`;
}

/**
 * @param {{dispatch: (command: any) => unknown, readOnly: () => boolean, explain: (text: string) => void}} host
 */
export function createCards(host) {
  /** @type {{deck: DeckEvent | null, review: Review | null, flipped: boolean, shownAt: number, card: string, waiting: number,
   *   editing: string | null, removing: string | null, more: boolean, shuffle: boolean}} */
  const state = {
    deck: null, review: null,
    // The answer turned over; when the card was first shown, for how long it
    // was looked at; which card that is.
    flipped: false, shownAt: 0, card: "",
    // Since when a rating has been waiting on the core's next card: keys
    // pressed meanwhile would land on a card not yet shown, so are let go.
    waiting: 0,
    // A card being written ("new" for one being added); a card whose taking
    // out is being confirmed; the form for more cards.
    editing: null, removing: null, more: false,
    shuffle: false,
  };

  /* ── the deck ──────────────────────────────────────────────────── */

  /** @param {Card | null} card */
  function editHtml(card) {
    const kind = card?.kind ?? "basic";
    const id = card?.id ?? "new";
    return `<form class="st-card-form" data-study-card-form="${esc(id)}" novalidate>
        <div class="st-card-form-row"><select name="kind" aria-label="Kind of card">
          <option value="basic"${kind === "basic" ? " selected" : ""}>Question and answer</option>
          <option value="cloze"${kind === "cloze" ? " selected" : ""}>Cloze: {{c1::hidden}}</option></select>
          <input name="topic" value="${esc(card?.topic ?? "")}" placeholder="Topic" autocomplete="off"></div>
        <textarea name="front" rows="3" placeholder="Front: a question, or a sentence with {{c1::what is hidden}}">${esc(card?.front ?? "")}</textarea>
        <textarea name="back" rows="3" placeholder="Back: the answer — for a cloze, anything more to say">${esc(card?.back ?? "")}</textarea>
        <div class="rs-actions"><button type="button" class="cc-quiet" data-study-card-cancel>Cancel</button>
          <button type="submit" class="cc-primary">${card ? "Save" : "Add"}</button></div></form>`;
  }

  function moreHtml() {
    return `<form class="st-make-form st-more-form" data-study-more-form novalidate>
        <div class="st-card-form-row">
          <select name="amount" aria-label="How many"><option value="fewer">About 10 more</option><option value="standard" selected>About 20 more</option><option value="more">About 40 more</option></select>
          <select name="level" aria-label="How hard"><option value="easy">Easy</option><option value="medium" selected>Medium</option><option value="hard">Hard</option></select></div>
        <input name="focus" placeholder="A topic to centre them on — or leave it to cover everything ticked" autocomplete="off">
        <div class="rs-actions"><button type="button" class="cc-quiet" data-study-more-cancel>Cancel</button><button type="submit" class="cc-primary">Make</button></div></form>`;
  }

  /** @param {DeckEvent} d */
  function deckHtml(d) {
    const ro = host.readOnly();
    const { made, counts, citations } = d;
    const due = counts.new + counts.learning + counts.review;
    const head = `<div class="st-reader-head"><button class="cc-quiet" data-study-back>← Back</button>` +
      `<div><h1>${esc(made.title)}</h1><span class="st-faint">${d.cards.length} cards · ${esc(made.by)}</span></div>` +
      (made.task ? `<button class="cc-quiet" data-study-task="${esc(made.task)}" title="What the call was told and asked">Prompt</button>` : "") +
      (ro ? "" : `<button class="cc-quiet" data-study-bin-made="${esc(made.id)}" title="Put it, and its reviews, in the notebook's bin">Bin</button>`) + `</div>`;
    const bar = `<div class="st-deck-bar">
        <div class="st-deck-counts"><span class="st-count new"><b>${counts.new}</b> new</span><span class="st-count learning"><b>${counts.learning}</b> learning</span>` +
        `<span class="st-count review"><b>${counts.review}</b> to review</span></div>` +
        (ro ? "" : `<div class="st-deck-go"><button class="cc-primary" data-study-review="due" data-study-review-deck="${esc(made.id)}"${due ? "" : " disabled"}>${due ? `Study ${due}` : "Nothing due"}</button>` +
          `<button class="cc-quiet" data-study-review="cram" data-study-review-deck="${esc(made.id)}" title="Every card, due or not">Cram all</button>` +
          `<button class="cc-quiet" data-study-review="missed" data-study-review-deck="${esc(made.id)}"${counts.missed ? "" : " disabled"} title="The cards missed in the last week">Missed${counts.missed ? ` ${counts.missed}` : ""}</button>` +
          `<label class="st-check" title="Show them in no order"><input type="checkbox" data-study-shuffle${state.shuffle ? " checked" : ""}> Shuffle</label></div>`) +
        `</div>`;
    const tools = ro ? "" : `<div class="st-deck-tools"><button class="cc-quiet" data-study-card-add>${icon("plus")}Add a card</button>` +
      `<button class="cc-quiet" data-study-more-cards>${icon("spark")}More cards</button><span class="grow"></span>` +
      `<button class="cc-quiet" data-study-export="anki" title="A file Anki imports, written into the notebook's exports">Export for Anki</button>` +
      `<button class="cc-quiet" data-study-export="csv">CSV</button></div>`;
    const forms = (state.more ? moreHtml() : "") + (state.editing === "new" ? editHtml(null) : "");
    const rows = d.cards.map((row) => {
      const c = row.card;
      if (state.editing === c.id) return `<li class="st-card-row editing">${editHtml(c)}</li>`;
      const remove = state.removing === c.id
        ? `<span class="st-confirm">Take it out? <button class="st-link" data-study-card-delete-yes="${esc(c.id)}">Yes</button> <button class="st-link" data-study-card-delete-no>No</button></span>`
        : ro ? "" : `<button class="st-link" data-study-card-edit="${esc(c.id)}">Edit</button><button class="st-link" data-study-card-delete="${esc(c.id)}">Delete</button>`;
      const facts = [row.state === "new" ? "" : dueHtml(row), row.recall !== null && row.state !== "new" ? `${Math.round(row.recall * 100)}% remembered` : "",
        row.missed ? `missed ${row.missed}×` : "", c.topic].filter(Boolean).map(esc).join(" · ");
      return `<li class="st-card-row" data-card="${esc(c.id)}">` +
        `<div class="st-card-sides"><div class="st-card-front md">${sideHtml(c.front, citations, "show")}</div>` +
        (c.back ? `<div class="st-card-back md">${sideHtml(c.back, citations, "show")}</div>` : "") + `</div>` +
        `<div class="st-card-meta"><span class="st-state ${esc(row.state)}">${esc(STATES[row.state] ?? row.state)}</span>` +
        `<span class="st-faint">${facts}</span><span class="grow"></span>${remove}</div></li>`;
    }).join("");
    return `<div class="st-reader st-deck-view" data-turn="made">${head}${bar}${tools}${forms}<ol class="st-cards">${rows}</ol></div>`;
  }

  /* ── review ────────────────────────────────────────────────────── */

  /** @param {Review} r */
  function reviewHtml(r) {
    const left = r.mode === "due" ? r.left.new + r.left.learning + r.left.review : r.remaining;
    const head = `<div class="st-review-head"><button class="cc-quiet" data-study-review-end title="End the session (Esc)">← End</button>` +
      `<div><b>${esc(r.title)}</b><span class="st-faint">${esc(MODES[r.mode])} · ${r.done} done${left ? ` · ${left} to go` : ""}</span></div>` +
      `<button class="cc-quiet" data-study-review-undo${r.undo ? "" : " disabled"} title="Take back the last rating (Z)">${icon("undo")}Undo</button></div>`;
    if (!r.card) {
      const wait = r.wait ? `<p>${r.left.learning === 1 ? "A card being learnt comes" : `${r.left.learning} cards being learnt come`} back at ` +
        `${esc(new Date(r.wait * 1000).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }))}: leave this open and it will show them.</p>` : "";
      const said = r.done ? `<p>${r.done} ${r.done === 1 ? "card" : "cards"} reviewed${r.again ? `, ${r.again} missed` : ""}.</p>` : `<p>Nothing is due${r.mode === "missed" ? " of the cards missed lately" : ""}.</p>`;
      return `<div class="st-review" tabindex="-1">${head}<div class="st-card st-review-done"><h2>${r.wait ? "Done for now" : "Done"}</h2>${said}${wait}` +
        `<div class="rs-actions">${r.left.missed ? `<button class="cc-quiet" data-study-review="missed" data-study-review-deck="${esc(r.deck ?? "")}">Missed ${r.left.missed}</button>` : ""}` +
        `<button class="cc-quiet" data-study-review="cram" data-study-review-deck="${esc(r.deck ?? "")}">Cram all</button><button class="cc-primary" data-study-review-end>Finish</button></div></div></div>`;
    }
    const c = r.card;
    const flipped = state.flipped;
    const tags = [c.deckTitle, c.card.topic, STATES[c.state] ?? c.state].filter(Boolean).map(esc).join(" · ");
    const back = flipped
      ? `<hr class="st-flash-rule"><div class="st-flash-back md">${c.card.kind === "cloze"
        ? `${sideHtml(c.card.front, r.citations, "show")}${c.card.back ? `<div class="st-flash-extra">${sideHtml(c.card.back, r.citations, "show")}</div>` : ""}`
        : sideHtml(c.card.back, r.citations, "show")}</div>` +
        (c.card.cites.length ? `<div class="st-flash-cites st-faint">From ${c.card.cites.map((raw) => r.citations.find((x) => x.raw === raw)).filter(Boolean)
          .map((x) => `<button class="st-cite${/** @type {Citation} */ (x).resolved ? "" : " bad"}" data-study-cite="${/** @type {Citation} */ (x).n}">${/** @type {Citation} */ (x).n}</button>`).join("")}</div>` : "")
      : "";
    const front = c.card.kind === "cloze" && flipped ? "" : `<div class="st-flash-front md">${sideHtml(c.card.front, r.citations, "hide")}</div>`;
    const actions = !flipped
      ? `<button class="cc-primary st-flip" data-study-flip>Show answer <kbd>Space</kbd></button>`
      : RATINGS.map(([rating, name, key], i) => `<button class="st-rate ${rating}" data-study-rate="${rating}"${host.readOnly() ? " disabled" : ""}>` +
        `<b>${esc(name)}</b><span>${esc(interval(c.intervals[i] ?? 0))}</span><kbd>${key}</kbd></button>`).join("");
    return `<div class="st-review" tabindex="-1" data-turn="made">${head}<div class="st-flash${flipped ? " flipped" : ""}" data-study-flash="${esc(c.deck)}/${esc(c.card.id)}">` +
      `<div class="st-flash-tags st-faint">${tags}</div>${front}${back}</div>` +
      `<div class="st-flash-actions">${actions}</div>` +
      `<div class="st-flash-more"><button class="st-link" data-study-explain title="Ask the tutor to explain it (E)">${icon("chat")}Explain</button></div></div>`;
  }

  /** Draw the deck or the session into the middle. @param {HTMLElement} stage @param {"deck" | "review"} kind */
  function render(stage, kind) {
    if (kind === "deck" && state.deck) {
      // A card being written is left alone while the deck is redrawn.
      if (state.editing && stage.querySelector("[data-study-card-form] textarea:focus, [data-study-card-form] input:focus")) return;
      stage.innerHTML = deckHtml(state.deck);
      /** @type {HTMLElement | null} */ (stage.querySelector("[data-study-card-form] textarea, [data-study-more-form] input"))?.focus({ preventScroll: true });
    } else if (kind === "review" && state.review) {
      // The session keeps the keys: what had them is drawn again, so the
      // session takes them — unless something else of the page has them.
      const active = stage.ownerDocument.activeElement;
      const keep = !active || active === stage.ownerDocument.body || stage.contains(active);
      stage.innerHTML = reviewHtml(state.review);
      if (keep) /** @type {HTMLElement | null} */ (stage.querySelector(".st-review"))?.focus({ preventScroll: true });
    }
  }

  /** Start a session. @param {Mode} mode @param {string | null} deck */
  function start(mode, deck) {
    host.dispatch({ type: "studyReview", deck, mode, shuffle: state.shuffle });
  }

  /** Whether a rating sent is still waiting on the card after it. */
  const busy = () => state.waiting > 0 && performance.now() - state.waiting < 3000;

  function flip() {
    if (!state.review?.card || state.flipped || busy()) return false;
    state.flipped = true;
    return true;
  }

  /** @param {Rating} rating */
  function rate(rating) {
    if (!state.review?.card || !state.flipped || host.readOnly() || busy()) return;
    const took = Math.max(0, Math.round(performance.now() - state.shownAt));
    state.waiting = performance.now();
    host.dispatch({ type: "studyRate", rating, took });
  }

  function explain() {
    const c = state.review?.card?.card ?? null;
    if (!c) return;
    const cites = c.cites.map((x) => `[[${x}]]`).join(" ");
    host.explain(`Explain this flashcard from the sources: why its answer is what it is, and how it connects to the ideas around it.\n\n` +
      `Front: ${c.front}\n\nBack: ${c.back || "(the hidden parts of the front)"}${cites ? `\n\nIt was made from ${cites}.` : ""}`);
  }

  /** A click in the deck or the session; `true` if it was theirs. @param {HTMLElement} t @param {() => void} redraw */
  function click(t, redraw) {
    const at = (/** @type {string} */ sel) => /** @type {HTMLElement | null} */ (t.closest(sel));
    const review = at("[data-study-review]");
    if (review) {
      // A deck's own buttons are of it; the notebook's, of every deck.
      start(/** @type {Mode} */ (review.dataset.studyReview), review.dataset.studyReviewDeck || null);
      return true;
    }
    if (at("[data-study-review-end]")) {
      host.dispatch({ type: "studyReviewEnd" });
      return true;
    }
    if (at("[data-study-review-undo]")) {
      if (!busy()) {
        state.waiting = performance.now();
        host.dispatch({ type: "studyReviewUndo" });
      }
      return true;
    }
    if (at("[data-study-flip]") || (at("[data-study-flash]") && !at("[data-study-cite]") && !state.flipped)) {
      if (flip()) redraw();
      return true;
    }
    const rating = at("[data-study-rate]");
    if (rating) {
      rate(/** @type {Rating} */ (rating.dataset.studyRate));
      return true;
    }
    if (at("[data-study-explain]")) {
      explain();
      return true;
    }
    const deck = state.deck;
    if (!deck) return false;
    if (at("[data-study-card-add]")) {
      state.editing = "new";
      state.more = false;
      redraw();
      return true;
    }
    if (at("[data-study-more-cards]")) {
      state.more = !state.more;
      state.editing = null;
      redraw();
      return true;
    }
    if (at("[data-study-more-cancel]")) {
      state.more = false;
      redraw();
      return true;
    }
    const edit = at("[data-study-card-edit]");
    if (edit) {
      state.editing = edit.dataset.studyCardEdit ?? null;
      state.more = false;
      redraw();
      return true;
    }
    if (at("[data-study-card-cancel]")) {
      state.editing = null;
      redraw();
      return true;
    }
    const remove = at("[data-study-card-delete]");
    if (remove) {
      state.removing = remove.dataset.studyCardDelete ?? null;
      redraw();
      return true;
    }
    const yes = at("[data-study-card-delete-yes]");
    if (yes) {
      state.removing = null;
      host.dispatch({ type: "studyCardDelete", deck: deck.made.id, card: yes.dataset.studyCardDeleteYes });
      return true;
    }
    if (at("[data-study-card-delete-no]")) {
      state.removing = null;
      redraw();
      return true;
    }
    const out = at("[data-study-export]");
    if (out) {
      host.dispatch({ type: "studyDeckExport", deck: deck.made.id, anki: out.dataset.studyExport === "anki" });
      return true;
    }
    return false;
  }

  /** @param {HTMLInputElement} t */
  function change(t) {
    if (!t.matches("[data-study-shuffle]")) return false;
    state.shuffle = t.checked;
    return true;
  }

  /** @param {HTMLFormElement} form */
  function submit(form) {
    const deck = state.deck;
    if (!deck) return false;
    const data = new FormData(form);
    const field = (/** @type {string} */ name) => String(data.get(name) ?? "").trim();
    if (form.matches("[data-study-card-form]")) {
      const id = form.dataset.studyCardForm;
      if (!field("front")) {
        /** @type {HTMLElement | null} */ (form.querySelector('[name="front"]'))?.focus();
        return true;
      }
      host.dispatch({ type: "studyCard", deck: deck.made.id, card: id === "new" ? null : id, kind: field("kind") || "basic",
        front: field("front"), back: field("back"), topic: field("topic") });
      state.editing = null;
      return true;
    }
    if (form.matches("[data-study-more-form]")) {
      host.dispatch({ type: "studyMake", kind: "flashcards", options: { focus: field("focus"), ask: "", columns: [],
        amount: field("amount") || "standard", level: field("level") || "medium", deck: deck.made.id } });
      state.more = false;
      return true;
    }
    return false;
  }

  /** A key in the session: Space turns the card over, 1–4 rate it, Z takes
   *  the last rating back, E explains, Esc ends. `true` if it was taken.
   *  @param {KeyboardEvent} e @param {() => void} redraw */
  function key(e, redraw) {
    if (!state.review || e.metaKey || e.ctrlKey || e.altKey) return false;
    const target = /** @type {HTMLElement} */ (e.target);
    if (target.closest?.("input, textarea, select, [contenteditable]")) return false;
    // A button with the keys turns Space and Enter into its own click.
    if ((e.key === " " || e.key === "Enter") && target.closest?.("button")) return false;
    if ((e.key === " " || e.key === "Enter") && state.review.card && !state.flipped) {
      e.preventDefault();
      if (flip()) redraw();
      return true;
    }
    if (e.key === " " && state.flipped) {
      e.preventDefault();
      rate("good");
      return true;
    }
    const n = RATINGS.findIndex(([, , k]) => k === e.key);
    if (n >= 0 && state.flipped) {
      e.preventDefault();
      rate(RATINGS[n][0]);
      return true;
    }
    if (e.key === "z" || e.key === "u") {
      if (state.review.undo && !busy()) {
        state.waiting = performance.now();
        host.dispatch({ type: "studyReviewUndo" });
      }
      return true;
    }
    if (e.key === "e" && state.review.card) {
      explain();
      return true;
    }
    if (e.key === "Escape") {
      host.dispatch({ type: "studyReviewEnd" });
      return true;
    }
    return false;
  }

  /** @param {DeckEvent} event */
  function deck(event) {
    if (event.made.id !== state.deck?.made.id) {
      state.editing = null;
      state.removing = null;
      state.more = false;
    }
    state.deck = event;
  }

  /** The session, as the core has it now; `false` once it has ended.
   *  @param {{review: Review | null}} event */
  function reviewing(event) {
    const review = event.review;
    state.waiting = 0;
    const card = review?.card ? `${review.card.deck}/${review.card.card.id}/${review.done}` : "";
    if (card !== state.card) {
      // A new card, or the same one again: its front first.
      state.flipped = false;
      state.shownAt = performance.now();
      state.card = card;
    }
    state.review = review;
    return Boolean(review);
  }

  return {
    render, click, change, submit, key, deck, reviewing,
    /** Whether a session is under way. */
    active: () => Boolean(state.review),
    /** The deck open, if one is. */
    openDeck: () => state.deck?.made.id ?? null,
    /** The citations of what is shown, for their chips. @param {"deck" | "review"} kind */
    citations: (kind) => (kind === "deck" ? state.deck?.citations : state.review?.citations) ?? [],
    clear: () => { state.deck = null; state.review = null; },
  };
}

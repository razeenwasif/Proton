/* Slide decks: a lecture's slides made from the sources, shown a slide at a
 * time with its speaker notes, revised a slide at a time, presented, and
 * written out for PowerPoint.
 *
 * The core keeps the deck and asks for a slide's revision; pandoc makes the
 * PowerPoint, its TeX as Office's own mathematics. */

import { escapeHtml as esc, icon } from "./html.js";
import { markdown } from "./markdown.js";
import { answerHtml } from "./study-chat.js";

/** @typedef {import("./bindings/StudyMade").StudyMade} Made */
/** @typedef {import("./bindings/StudySlide").StudySlide} Slide */
/** @typedef {import("./bindings/StudyCitation").StudyCitation} Citation */
/** @typedef {{made: Made, slides: Slide[], citations: Citation[]}} SlidesEvent */

/** A line of Markdown, without its paragraph, and without its citations:
 *  a slide shows what it says, its notes say where from. @param {string} text */
const shown = (text) => markdown(String(text).replace(/\s*\[\[[^[\]\n]+\]\]/g, "")).replace(/^<p>|<\/p>\s*$/g, "");

/** A slide, as it is projected. @param {Slide} s */
export function slideHtml(s) {
  const list = (/** @type {string[]} */ points) => `<ul>${points.map((p) => `<li>${shown(p)}</li>`).join("")}</ul>`;
  const body = s.layout === "title" ? `<h1>${shown(s.title)}</h1>${s.points[0] ? `<p class="st-slide-sub">${shown(s.points[0])}</p>` : ""}`
    : s.layout === "quote" ? `<h2>${shown(s.title)}</h2><blockquote>${s.points.map(shown).join("<br>")}</blockquote>`
      : s.layout === "two" ? `<h2>${shown(s.title)}</h2><div class="st-slide-two">${list(s.points)}${list(s.right)}</div>`
        : `<h2>${shown(s.title)}</h2>${list(s.points)}`;
  return `<div class="st-slide ${esc(s.layout)}">${body}</div>`;
}

/** @param {{dispatch: (command: any) => unknown, readOnly: () => boolean}} host */
export function createSlides(host) {
  /** @type {{deck: SlidesEvent | null, at: number, revising: boolean, presenting: boolean}} */
  const state = { deck: null, at: 0, revising: false, presenting: false };

  /** The deck, for the middle. */
  function html() {
    const d = /** @type {SlidesEvent} */ (state.deck);
    const ro = host.readOnly();
    state.at = Math.max(0, Math.min(state.at, d.slides.length - 1));
    const s = d.slides[state.at];
    const head = `<div class="st-reader-head"><button class="cc-quiet" data-study-back>← Back</button>` +
      `<div><h1>${esc(d.made.title)}</h1><span class="st-faint">Slide deck · ${d.slides.length} slides · ${esc(d.made.by)}</span></div>` +
      `<button class="cc-quiet" data-study-task="${esc(d.made.task)}" title="What the call was told and asked">Prompt</button>` +
      (ro ? "" : `<button class="cc-quiet" data-study-bin-made="${esc(d.made.id)}" title="Put it in the notebook's bin">Bin</button>`) + `</div>`;
    const tools = `<div class="st-deck-tools"><button class="cc-quiet" data-study-present>${icon("expand")}Present</button>` +
      `<span class="grow"></span><button class="cc-quiet" data-study-slides-out="pptx" title="A PowerPoint, made by pandoc, written into the notebook's exports">PowerPoint</button>` +
      `<button class="cc-quiet" data-study-slides-out="md">Markdown</button></div>`;
    const frame = `<div class="st-slide-frame${state.presenting ? " presenting" : ""}" data-study-slide-frame>${slideHtml(s)}` +
      `<div class="st-slide-nav"><button class="st-x" data-study-slide-go="${state.at - 1}"${state.at ? "" : " disabled"} aria-label="The slide before">${icon("chev")}</button>` +
      `<span>${state.at + 1} / ${d.slides.length}</span><button class="st-x" data-study-slide-go="${state.at + 1}"${state.at < d.slides.length - 1 ? "" : " disabled"} aria-label="The next slide">${icon("chev")}</button></div></div>`;
    const strip = `<div class="st-slide-strip">${d.slides.map((x, i) => `<button class="st-slide-thumb${i === state.at ? " on" : ""}" data-study-slide-go="${i}" title="${esc(x.title)}">` +
      `<span>${i + 1}</span>${esc(x.title)}</button>`).join("")}</div>`;
    const revise = ro ? "" : state.revising
      ? `<form class="st-card-form" data-study-slide-revise="${esc(s.id)}" novalidate><textarea name="ask" rows="2" placeholder="How it is to change: “fewer words”, “add an example”, “split the comparison into a table”"></textarea>` +
        `<div class="rs-actions"><button type="button" class="cc-quiet" data-study-slide-revise-cancel>Cancel</button><button type="submit" class="cc-primary">Revise it</button></div></form>`
      : `<button class="st-link" data-study-slide-revise-open>Revise this slide</button>`;
    const notes = s.notes || s.cites.length
      ? `<div class="st-slide-notes" data-turn="made"><h3>Speaker notes</h3><div class="md">${answerHtml(`${s.notes}${s.cites.length && !s.notes.includes("[[") ? ` ${s.cites.map((c) => `[[${c}]]`).join("")}` : ""}`, d.citations)}</div></div>` : "";
    return `<div class="st-reader st-slides-view" tabindex="-1">${head}${tools}${frame}${strip}${revise}${notes}</div>`;
  }

  /** @param {HTMLElement} stage */
  function render(stage) {
    if (!state.deck) return;
    if (state.revising && stage.querySelector("[data-study-slide-revise] textarea:focus")) return;
    // The deck keeps the keys, as what had them is drawn again — unless
    // something else of the page has them.
    const active = stage.ownerDocument.activeElement;
    const keep = !active || active === stage.ownerDocument.body || stage.contains(active);
    stage.innerHTML = html();
    const typing = /** @type {HTMLElement | null} */ (stage.querySelector("[data-study-slide-revise] textarea"));
    if (typing) typing.focus({ preventScroll: true });
    else if (keep) /** @type {HTMLElement | null} */ (stage.querySelector(".st-slides-view"))?.focus({ preventScroll: true });
  }

  /** @param {number} to @param {() => void} redraw */
  function go(to, redraw) {
    const n = state.deck?.slides.length ?? 0;
    if (to < 0 || to >= n || to === state.at) return;
    state.at = to;
    state.revising = false;
    redraw();
  }

  /** A click in the deck; `true` if it was the deck's. @param {HTMLElement} t @param {() => void} redraw */
  function click(t, redraw) {
    const d = state.deck;
    if (!d || !t.closest(".st-slides-view")) return false;
    const at = (/** @type {string} */ sel) => /** @type {HTMLElement | null} */ (t.closest(sel));
    const to = at("[data-study-slide-go]");
    if (to) {
      go(Number(to.dataset.studySlideGo), redraw);
      return true;
    }
    if (at("[data-study-present]")) {
      state.presenting = true;
      redraw();
      return true;
    }
    if (state.presenting && at("[data-study-slide-frame]")) {
      go(state.at + 1, redraw);
      return true;
    }
    if (at("[data-study-slide-revise-open]")) {
      state.revising = true;
      redraw();
      return true;
    }
    if (at("[data-study-slide-revise-cancel]")) {
      state.revising = false;
      redraw();
      return true;
    }
    const out = at("[data-study-slides-out]");
    if (out) {
      host.dispatch({ type: "studySlidesExport", id: d.made.id, pptx: out.dataset.studySlidesOut === "pptx" });
      return true;
    }
    return false;
  }

  /** @param {HTMLFormElement} form @param {() => void} redraw */
  function submit(form, redraw) {
    const d = state.deck;
    if (!d || !form.matches("[data-study-slide-revise]")) return false;
    const ask = String(new FormData(form).get("ask") ?? "").trim();
    if (!ask) {
      /** @type {HTMLElement | null} */ (form.querySelector("textarea"))?.focus();
      return true;
    }
    host.dispatch({ type: "studyMake", kind: "slides", options: { focus: "", ask, columns: [], amount: "standard", level: "medium", deck: null, objectives: [],
      source: null, count: null, format: null, language: "", slides: d.made.id, slide: form.dataset.studySlideRevise } });
    state.revising = false;
    redraw();
    return true;
  }

  /** ← and → go between slides; Escape stops presenting. @param {KeyboardEvent} e @param {() => void} redraw */
  function key(e, redraw) {
    if (!state.deck || e.metaKey || e.ctrlKey || e.altKey) return false;
    if (/** @type {HTMLElement} */ (e.target).closest?.("input, textarea, select")) return false;
    if (e.key === "ArrowRight" || e.key === "PageDown" || (state.presenting && e.key === " ")) {
      e.preventDefault();
      go(state.at + 1, redraw);
      return true;
    }
    if (e.key === "ArrowLeft" || e.key === "PageUp") {
      e.preventDefault();
      go(state.at - 1, redraw);
      return true;
    }
    if (e.key === "Escape" && state.presenting) {
      state.presenting = false;
      redraw();
      return true;
    }
    return false;
  }

  return {
    render, click, submit, key,
    /** @param {SlidesEvent} event */
    slides(event) {
      if (event.made.id !== state.deck?.made.id) {
        state.at = 0;
        state.presenting = false;
      }
      state.deck = event;
    },
    open: () => state.deck?.made.id ?? null,
    citations: () => state.deck?.citations ?? [],
    clear: () => { state.deck = null; state.presenting = false; },
  };
}

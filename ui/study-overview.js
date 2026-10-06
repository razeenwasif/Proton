/* A Learning Overview: the course topic by topic — each taught in a summary,
 * its ideas mapped, a few flashcards turned over in place, and a check of a
 * question or two marked here at once — ending in a quiz.
 *
 * Its cards are a deck of their own and its quiz a quiz of its own, made
 * with it: the cards are scheduled, and the quiz marked, like any other. */

import { escapeHtml as esc, icon } from "./html.js";
import { markdown } from "./markdown.js";
import { answerHtml } from "./study-chat.js";
import { sideHtml } from "./study-cards.js";
import { mapHtml } from "./study-studio.js";

/** @typedef {import("./bindings/StudyMade").StudyMade} Made */
/** @typedef {import("./bindings/StudyOverviewTopic").StudyOverviewTopic} Topic */
/** @typedef {import("./bindings/StudyQuestion").StudyQuestion} Question */
/** @typedef {import("./bindings/StudyCitation").StudyCitation} Citation */
/** @typedef {{made: Made, topics: Topic[], deck: string | null, quiz: string | null, citations: Citation[]}} OverviewEvent */

/** A line of Markdown, without its paragraph. @param {string} text */
const inline = (text) => markdown(text).replace(/^<p>|<\/p>\s*$/g, "");

/** Words as a blank compares them. @param {string} text */
const normal = (text) => String(text).replace(/\$/g, "").trim().toLowerCase().replace(/\s+/g, " ").replace(/[.;,]$/, "");

/** A check's answer, marked as a rule marks it: right, or not.
 *  @param {Question} q @param {HTMLElement} box @returns {boolean | null} */
function marked(q, box) {
  const picked = [...box.querySelectorAll("input[name='option']:checked")].map((i) => Number(/** @type {HTMLInputElement} */ (i).value));
  if (q.kind === "choice") return picked.length ? q.correct.includes(picked[0]) : null;
  if (q.kind === "choices") return picked.length ? picked.length === q.correct.length && picked.every((i) => q.correct.includes(i)) : null;
  if (q.kind === "blanks") {
    const words = [...box.querySelectorAll(".st-blank")].map((i) => /** @type {HTMLInputElement} */ (i).value);
    if (!words.some((w) => w.trim())) return null;
    return q.blanks.every((accepted, k) => accepted.some((a) => normal(a) === normal(words[k] ?? "")));
  }
  if (q.kind === "number") {
    const text = /** @type {HTMLInputElement | null} */ (box.querySelector("input[name='number']"))?.value.trim() ?? "";
    const value = Number.parseFloat(text.replace(/,/g, ""));
    if (!text || Number.isNaN(value) || q.number === null) return null;
    return Math.abs(value - q.number) <= Math.max(q.tolerance, 1e-9 * Math.max(1, Math.abs(q.number)));
  }
  return null;
}

/** A check's question, to answer here. @param {Question} q @param {string} key @param {Citation[]} citations */
function checkHtml(q, key, citations) {
  let prompt = answerHtml(q.prompt, citations);
  let inputs = "";
  if (q.kind === "choice" || q.kind === "choices") {
    const type = q.kind === "choice" ? "radio" : "checkbox";
    inputs = `<div class="st-options">${q.options.map((o, i) => `<label class="st-option"><input type="${type}" name="option" value="${i}"><span>${inline(o)}</span></label>`).join("")}</div>`;
  } else if (q.kind === "blanks") {
    let n = 0;
    prompt = answerHtml(q.prompt.replace(/_{3,}/g, () => `${n++}`), citations)
      .replace(/(\d+)/g, (_, i) => `<input class="st-blank" data-blank="${i}" aria-label="Blank ${Number(i) + 1}" autocomplete="off" spellcheck="false">`);
  } else if (q.kind === "number") {
    inputs = `<label class="st-number"><input name="number" autocomplete="off" spellcheck="false" placeholder="A number">${q.unit ? `<span>${esc(q.unit)}</span>` : ""}</label>`;
  }
  return `<div class="st-question st-ov-check" data-study-check="${esc(key)}"><div class="st-question-prompt md">${prompt}</div>${inputs}` +
    `<div class="rs-actions"><span class="st-check-said" data-study-check-said></span><span class="grow"></span><button class="cc-quiet" data-study-check-go>Check</button></div></div>`;
}

/** The overview, whole, for the middle. @param {OverviewEvent} e @param {boolean} readOnly */
export function overviewHtml(e, readOnly) {
  const { made, topics, citations } = e;
  const head = `<div class="st-reader-head"><button class="cc-quiet" data-study-back>← Back</button>` +
    `<div><h1>${esc(made.title)}</h1><span class="st-faint">Learning overview · ${topics.length} ${topics.length === 1 ? "topic" : "topics"} · ${esc(made.by)}</span></div>` +
    `<button class="cc-quiet" data-study-task="${esc(made.task)}" title="What the call was told and asked">Prompt</button>` +
    (readOnly ? "" : `<button class="cc-quiet" data-study-bin-made="${esc(made.id)}" title="Put it in the notebook's bin; its deck and quiz stay">Bin</button>`) + `</div>`;
  const nav = `<nav class="st-ov-nav">${topics.map((t, i) => `<button class="st-chip-btn" data-study-ov-go="${i}">${i + 1}. ${esc(t.title)}</button>`).join("")}</nav>`;
  const sections = topics.map((t, i) => {
    const map = t.map.length ? `<details class="st-explain"><summary>Its ideas, mapped</summary>${mapHtml(t.map, citations)}</details>` : "";
    const cards = t.cards.length ? `<div class="st-ov-cards">${t.cards.map((c) => `<button class="st-ov-card" data-study-ov-card>` +
      `<span class="st-ov-front md">${sideHtml(c.front, citations, "hide")}</span><span class="st-ov-back md">${c.kind === "cloze" ? sideHtml(c.front, citations, "show") : sideHtml(c.back, citations, "show")}</span></button>`).join("")}</div>` +
      (e.deck ? `<p class="st-faint">Turn a card over to see its answer. They are in <button class="st-link" data-study-made="${esc(e.deck)}">${esc(e.deck)}</button>, scheduled for review.</p>` : "") : "";
    const check = t.check.length ? `<h4>Check</h4>${t.check.map((q, k) => checkHtml(q, `${i}/${k}`, citations)).join("")}` : "";
    return `<section class="st-ov-topic" data-study-ov-topic="${i}" data-turn="made"><h2>${i + 1}. ${esc(t.title)}</h2>` +
      `<div class="st-answer st-doc md">${answerHtml(t.summary, citations)}</div>${map}${cards}${check}</section>`;
  }).join("");
  const end = e.quiz ? `<div class="st-card st-ov-end"><h2>${icon("check")} The quiz</h2><p>Questions across every topic, marked as you answer them.</p>` +
    `<div class="rs-actions"><button class="cc-primary" data-study-made="${esc(e.quiz)}">Take the quiz</button></div></div>` : "";
  return `<div class="st-reader st-overview-view">${head}${nav}${sections}${end}</div>`;
}

/** A click in an overview; `true` if it was the overview's. @param {HTMLElement} t @param {OverviewEvent | null} e */
export function overviewClick(t, e) {
  if (!e || !t.closest(".st-overview-view")) return false;
  const at = (/** @type {string} */ sel) => /** @type {HTMLElement | null} */ (t.closest(sel));
  const card = at("[data-study-ov-card]");
  if (card && !at("[data-study-cite]")) {
    card.classList.toggle("flipped");
    return true;
  }
  const go = at("[data-study-ov-go]");
  if (go) {
    t.closest(".st-overview-view")?.querySelector(`[data-study-ov-topic="${go.dataset.studyOvGo}"]`)?.scrollIntoView({ block: "start", behavior: "smooth" });
    return true;
  }
  if (at("[data-study-check-go]")) {
    const box = /** @type {HTMLElement} */ (at("[data-study-check]"));
    const [topic, k] = String(box.dataset.studyCheck).split("/").map(Number);
    const q = e.topics[topic]?.check[k];
    const said = /** @type {HTMLElement} */ (box.querySelector("[data-study-check-said]"));
    if (!q || !said) return true;
    const right = marked(q, box);
    if (right === null) {
      said.textContent = "Answer it first.";
      return true;
    }
    box.classList.toggle("right", right);
    box.classList.toggle("wrong", !right);
    said.innerHTML = `${right ? "Right." : "Not quite."} ${q.explanation ? inline(q.explanation) : ""}`;
    if (!right && (q.kind === "choice" || q.kind === "choices")) {
      box.querySelectorAll(".st-option").forEach((o, i) => o.classList.toggle("correct", q.correct.includes(i)));
    }
    return true;
  }
  return false;
}

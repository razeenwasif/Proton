/* Progress: where the student stands, learning objective by objective, from
 * everything they have done — the questions they answered, the cards they
 * reviewed — and what to do about it: a diagnostic to find out, a lesson on
 * a gap, practice on an objective, the cards due, how each quiz went.
 *
 * The core weighs the evidence (proton-study::mastery); this draws it. */

import { escapeHtml as esc, icon } from "./html.js";
import { markdown } from "./markdown.js";

/** @typedef {import("./bindings/StudyMastery").StudyMastery} Mastery */
/** @typedef {import("./bindings/StudyQuizRow").StudyQuizRow} QuizRow */
/** @typedef {{objectives: Mastery[], due: number, quizzes: QuizRow[]}} ProgressEvent */

/** Where an objective can stand, in the order they are shown. */
const STANDINGS = /** @type {[string, string, string][]} */ ([
  ["focus", "Focus areas", "What the evidence says is not yet known"],
  ["developing", "Developing", "Under way: more to show before it is a strength"],
  ["notStarted", "Not started", "Nothing done on it yet"],
  ["strong", "Strengths", "Known, and shown more than once"],
]);

const KINDS = /** @type {Record<string, string>} */ ({ quiz: "Quiz", practice: "Practice", exam: "Practice exam", pastPaper: "Past paper", diagnostic: "Diagnostic" });

/** A line of Markdown, without its paragraph. @param {string} text */
const inline = (text) => markdown(text).replace(/^<p>|<\/p>\s*$/g, "");

/** A share, in whole per cent. @param {number} n */
const percent = (n) => `${Math.round(n * 100)}%`;

/** The progress, below the notebook's head. @param {ProgressEvent | null} p @param {boolean} readOnly */
export function progressHtml(p, readOnly) {
  if (!p) return `<p class="st-faint st-pad">Weighing what you have done…</p>`;
  if (!p.objectives.length) {
    return `<div class="st-card st-empty"><h2>${icon("target")} Progress is by learning objective</h2>
        <p>The learning objectives are what a student must be able to do in the exam. Make them from the sources ticked — past papers
          among them weigh most — and every question answered and card reviewed from then on says how each stands.</p>
        ${readOnly ? "" : `<div class="rs-actions"><button class="cc-primary" data-study-progress-make="objectives">Make the learning objectives</button></div>`}</div>`;
  }
  const count = (/** @type {string} */ state) => p.objectives.filter((o) => o.state === state).length;
  const tally = STANDINGS.map(([state, name]) => `<span class="st-tally ${state}"><b>${count(state)}</b> ${esc(name.toLowerCase())}</span>`).join("");
  const top = `<div class="st-progress-top"><div class="st-tallies">${tally}</div>` +
    (readOnly ? "" : `<div class="st-progress-go">` +
      (p.due ? `<span class="st-faint">${p.due} ${p.due === 1 ? "card" : "cards"} due today</span>` : "") +
      `<button class="cc-quiet" data-study-progress-make="diagnostic" title="A question on each objective, to find out what you know">Diagnostic quiz</button></div>`) + `</div>`;
  const groups = STANDINGS.map(([state, name, note]) => {
    const of = p.objectives.filter((o) => o.state === state).sort((a, b) => a.topic.localeCompare(b.topic) || (a.level ?? 0) - (b.level ?? 0));
    if (!of.length) return "";
    let topic = "";
    const rows = of.map((o) => {
      const head = o.topic !== topic ? `<h4>${esc((topic = o.topic) || "Other")}</h4>` : "";
      const bar = o.level === null ? "" : `<span class="st-bar"><i style="width:${percent(o.level)}"></i></span>`;
      return `${head}<div class="st-objective ${esc(o.state)}"><span class="st-faint">${esc(o.objective)}</span><div><div class="st-objective-text">${inline(o.text)}</div>` +
        `<div class="st-objective-meta">${bar}<span class="st-faint">${o.level === null ? "" : `${percent(o.level)} · `}${o.evidence} done</span>` +
        (readOnly ? "" : `<span class="st-objective-go"><button class="st-link" data-study-lesson="${esc(o.objective)}" title="A lesson on it, with the tutor as a guide">Lesson</button>` +
          `<button class="st-link" data-study-practise="${esc(o.objective)}" title="A short quiz on it">Practise</button></span>`) + `</div></div></div>`;
    }).join("");
    return `<section class="st-standing-group ${state}"><h3>${esc(name)} <span class="st-faint">${of.length} · ${esc(note)}</span></h3>${rows}</section>`;
  }).join("");
  const quizzes = p.quizzes.length ? `<h3>Quizzes and exams</h3><div class="st-quiz-rows">${p.quizzes.map((q) => `<button class="st-made st-quiz-row" data-study-made="${esc(q.id)}">` +
    `${icon(q.kind === "exam" || q.kind === "pastPaper" ? "files" : "check")}<span><b>${esc(q.title)}</b><span class="st-faint">${esc(KINDS[q.kind] ?? q.kind)} · ` +
    `${q.attempts} ${q.attempts === 1 ? "attempt" : "attempts"}${q.best !== null ? ` · best ${percent(q.best)}` : ""}${q.last !== null && q.last !== q.best ? ` · last ${percent(q.last)}` : ""}</span></span></button>`).join("")}</div>` : "";
  return `<div class="st-progress">${top}${groups}${quizzes}</div>`;
}

/** A click in the progress; `true` if it was the progress's. @param {HTMLElement} t @param {(command: any) => unknown} dispatch */
export function progressClick(t, dispatch) {
  if (!t.closest(".st-progress, .st-empty")) return false;
  const at = (/** @type {string} */ sel) => /** @type {HTMLElement | null} */ (t.closest(sel));
  const make = at("[data-study-progress-make]");
  if (make) {
    dispatch({ type: "studyMake", kind: make.dataset.studyProgressMake, options: { focus: "", ask: "", columns: [], amount: "standard", level: "medium", deck: null, objectives: [], source: null, count: null } });
    return true;
  }
  const lesson = at("[data-study-lesson]");
  if (lesson) {
    dispatch({ type: "studyLesson", objective: lesson.dataset.studyLesson });
    return true;
  }
  const practise = at("[data-study-practise]");
  if (practise) {
    dispatch({ type: "studyMake", kind: "quiz", options: { focus: "", ask: "", columns: [], amount: "fewer", level: "medium", deck: null, objectives: [practise.dataset.studyPractise], source: null, count: null } });
    return true;
  }
  return false;
}

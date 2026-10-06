/* Quizzes and practice problems: an attempt, question by question, each
 * answer marked as it is given, and how the attempt went.
 *
 * What can be checked — a choice, choices, blanks, a number — is marked by
 * the core at once; a written answer is marked by a model of another family
 * than the one that set the question, against its rubric, and drawn here as
 * it comes back: criterion by criterion, with the words of the answer that
 * earned each mark. A mark can be disputed, and is marked again. The core
 * keeps every answer and mark in the quiz's attempts; this draws them. */

import { escapeHtml as esc, icon } from "./html.js";
import { markdown } from "./markdown.js";
import { answerHtml } from "./study-chat.js";

/** @typedef {import("./bindings/StudyMade").StudyMade} Made */
/** @typedef {import("./bindings/StudyQuestion").StudyQuestion} Question */
/** @typedef {import("./bindings/StudyAttempt").StudyAttempt} Attempt */
/** @typedef {import("./bindings/StudyAnswered").StudyAnswered} Answered */
/** @typedef {import("./bindings/StudyGrade").StudyGrade} Grade */
/** @typedef {import("./bindings/StudyGiven").StudyGiven} Given */
/** @typedef {import("./bindings/StudyResult").StudyResult} Result */
/** @typedef {import("./bindings/StudyMastery").StudyMastery} Mastery */
/** @typedef {import("./bindings/StudyCitation").StudyCitation} Citation */
/** @typedef {{made: Made, questions: Question[], attempts: Attempt[], marking: string[], results: Result[], mastery: Mastery[], citations: Citation[]}} QuizEvent */

const KINDS = /** @type {Record<string, string>} */ ({ choice: "One answer", choices: "Every right answer", blanks: "Fill the blanks", number: "A number", short: "A sentence or two", long: "Worked answer" });
const STANDING = /** @type {Record<string, string>} */ ({ notStarted: "Not started", focus: "Focus", developing: "Developing", strong: "Strength" });

/** A line of Markdown, without its paragraph. @param {string} text */
const inline = (text) => markdown(text).replace(/^<p>|<\/p>\s*$/g, "");

/** A share, in whole per cent. @param {number} score @param {number} of */
const percent = (score, of) => (of > 0 ? Math.round((score / of) * 100) : 0);

/** The mark that counts: after a dispute, the second — unless the two
 *  differ by more than a point, when the lower. @param {Answered} a */
export function counted(a) {
  if (a.grade && a.second) return Math.abs(a.grade.score - a.second.score) > 1 ? (a.grade.score <= a.second.score ? a.grade : a.second) : a.second;
  return a.second ?? a.grade;
}

/** How a mark reads: whole, part, or none. @param {Grade | null} g */
const verdict = (g) => (!g ? "" : g.score >= g.of - 1e-9 ? "right" : g.score > 0 ? "part" : "wrong");

/** A prompt with its blanks as inputs, or as what was written in them.
 *  @param {Question} q @param {Citation[]} citations @param {string[] | null} given @param {Grade | null} grade */
function promptHtml(q, citations, given, grade) {
  if (q.kind !== "blanks") return answerHtml(q.prompt, citations);
  let n = 0;
  const marked = q.prompt.replace(/_{3,}/g, () => `${n++}`);
  return answerHtml(marked, citations).replace(/(\d+)/g, (_, i) => {
    const k = Number(i);
    if (!given) return `<input class="st-blank" data-blank="${k}" aria-label="Blank ${k + 1}" autocomplete="off" spellcheck="false">`;
    const right = grade ? (q.blanks[k] ?? []).some((w) => w.trim().toLowerCase() === (given[k] ?? "").trim().toLowerCase()) : false;
    return `<span class="st-blank-said ${grade ? (right ? "right" : "wrong") : ""}">${esc(given[k] || "—")}</span>`;
  });
}

/**
 * @param {{dispatch: (command: any) => unknown, readOnly: () => boolean, ask: (text: string) => void}} host
 */
export function createQuiz(host) {
  /** @type {{quiz: QuizEvent | null, at: number, hinted: Set<string>, disputing: string | null, reviewing: boolean}} */
  const state = {
    quiz: null,
    // Which question of the attempt is shown.
    at: 0,
    // Questions whose hints were looked at, this attempt.
    hinted: new Set(),
    // A mark being disputed: the form for why.
    disputing: null,
    // Going back over a finished attempt's answers.
    reviewing: false,
  };

  /** The attempt under way, if there is one. */
  const current = () => {
    const last = state.quiz?.attempts.at(-1);
    return last && last.finished === null ? last : null;
  };

  /** A question's own inputs, before it is answered. @param {Question} q */
  function inputsHtml(q) {
    if (q.kind === "choice" || q.kind === "choices") {
      const type = q.kind === "choice" ? "radio" : "checkbox";
      return `<div class="st-options">${q.options.map((o, i) => `<label class="st-option"><input type="${type}" name="option" value="${i}">` +
        `<kbd>${i + 1}</kbd><span>${inline(o)}</span></label>`).join("")}</div>`;
    }
    if (q.kind === "number") return `<label class="st-number"><input name="number" autocomplete="off" spellcheck="false" placeholder="A number${q.unit ? `, in ${esc(q.unit)}` : ""}">${q.unit ? `<span>${esc(q.unit)}</span>` : ""}</label>`;
    if (q.kind === "short" || q.kind === "long") {
      return `<textarea name="text" rows="${q.kind === "long" ? 10 : 4}" placeholder="${q.kind === "long" ? "Work it through — TeX in $…$ is drawn when it is marked" : "A sentence or two"}"></textarea>`;
    }
    return "";
  }

  /** What an answer was marked, and why. @param {Question} q @param {Answered} a @param {boolean} marking @param {Citation[]} citations */
  function markHtml(q, a, marking, citations) {
    const g = counted(a);
    const chosen = a.given.kind === "choice" ? [a.given.index] : a.given.kind === "choices" ? a.given.indices : [];
    let said = "";
    if (q.kind === "choice" || q.kind === "choices") {
      said = `<div class="st-options">${q.options.map((o, i) => {
        const cls = [q.correct.includes(i) ? "correct" : "", chosen.includes(i) ? "chosen" : ""].filter(Boolean).join(" ");
        const why = q.rationales[i] && (chosen.includes(i) || q.correct.includes(i)) ? `<span class="st-rationale">${inline(q.rationales[i])}</span>` : "";
        return `<div class="st-option done ${cls}">${q.correct.includes(i) ? icon("check") : `<span class="st-dot"></span>`}<span>${inline(o)}${why}</span></div>`;
      }).join("")}</div>`;
    } else if (q.kind === "number") {
      said = `<p class="st-said">${esc(a.given.kind === "number" ? a.given.text : "")}</p>`;
    } else if (q.kind === "short" || q.kind === "long") {
      said = `<div class="st-said md">${markdown(a.given.kind === "text" ? a.given.text : "")}</div>`;
    }
    let mark = "";
    if (marking) {
      mark = `<div class="st-marking"><span class="lab-spin"></span>Being marked${a.disputed ? " again" : ""}…</div>`;
    } else if (a.unmarked && !g) {
      mark = `<div class="st-unmarked">${icon("d-warning")}<span>It could not be marked: ${esc(a.unmarked)}</span>` +
        (host.readOnly() ? "" : `<button class="st-link" data-study-mark-again="${esc(q.id)}">Mark it again</button>`) + `</div>`;
    } else if (g) {
      const criteria = g.criteria.length ? `<ul class="st-criteria">${g.criteria.map((c) => {
        const of = q.rubric.find((r) => r.id === c.id);
        return `<li><b>${c.points} / ${of?.points ?? "?"}</b><span><span>${inline(of?.text ?? c.id)}${c.why ? `<span class="st-faint"> — ${inline(c.why)}</span>` : ""}</span>` +
          `${c.evidence.filter(Boolean).map((e) => `<q>${esc(e)}</q>`).join("")}</span></li>`;
      }).join("")}</ul>` : "";
      const two = a.grade && a.second ? `<p class="st-faint">Marked ${a.grade.score} of ${a.grade.of} by ${esc(a.grade.by)}, then ${a.second.score} by ${esc(a.second.by)} after your dispute` +
        `${Math.abs(a.grade.score - a.second.score) > 1 ? ": they differ by more than a point, so the lower counts" : ""}.</p>` : "";
      const dispute = g.by !== "rule" && !a.disputed && !host.readOnly()
        ? state.disputing === q.id
          ? `<form class="st-dispute" data-study-dispute-form="${esc(q.id)}" novalidate><textarea name="why" rows="2" placeholder="What the mark missed: “I did say why the costs must be positive, in my second line.”"></textarea>` +
            `<div class="rs-actions"><button type="button" class="cc-quiet" data-study-dispute-cancel>Cancel</button><button type="submit" class="cc-primary">Mark it again</button></div></form>`
          : `<button class="st-link" data-study-dispute="${esc(q.id)}" title="Have another model mark it again">Dispute this mark</button>`
        : "";
      mark = `<div class="st-mark ${verdict(g)}"><div class="st-mark-head"><b>${g.score} / ${g.of}</b><span class="st-faint">${g.by === "rule" ? "" : `marked by ${esc(g.by)}`}</span></div>` +
        // A choice's options say why already.
        `${g.feedback && q.kind !== "choice" ? `<p>${inline(g.feedback)}</p>` : ""}${criteria}${two}${dispute}</div>`;
    }
    const after = g && !marking
      ? (q.explanation ? `<details class="st-explain" open><summary>Why</summary><div class="md">${answerHtml(q.explanation, citations)}</div></details>` : "") +
        (q.modelAnswer && (q.kind === "short" || q.kind === "long") ? `<details class="st-explain"><summary>A model answer</summary><div class="md">${answerHtml(q.modelAnswer, citations)}</div></details>` : "")
      : "";
    return `${said}${mark}${after}`;
  }

  /** The attempt under way: one question, and the way through them. @param {QuizEvent} e @param {Attempt} attempt */
  function attemptHtml(e, attempt) {
    const ids = attempt.questions;
    state.at = Math.max(0, Math.min(state.at, ids.length - 1));
    const q = e.questions.find((x) => x.id === ids[state.at]);
    if (!q) return "";
    const a = attempt.answers.find((x) => x.question === q.id) ?? null;
    const marking = e.marking.includes(`${attempt.n}/${q.id}`);
    const dots = ids.map((id, i) => {
      const done = attempt.answers.find((x) => x.question === id);
      const cls = done ? (e.marking.includes(`${attempt.n}/${id}`) ? "marking" : verdict(counted(done)) || "unmarked") : "";
      return `<button class="st-dot-step ${cls}${i === state.at ? " on" : ""}" data-study-quiz-go="${i}" aria-label="Question ${i + 1}"></button>`;
    }).join("");
    const hint = q.hint && !a
      ? state.hinted.has(q.id) ? `<div class="st-hint">${icon("bulb")}<span>${inline(q.hint)}</span></div>` : `<button class="st-link" data-study-hint>${icon("bulb")}A hint</button>`
      : "";
    const blanks = a?.given.kind === "blanks" ? a.given.words : null;
    const body = a ? markHtml(q, a, marking, e.citations) : `<form class="st-answer-form" data-study-answer-form="${esc(q.id)}" novalidate>${inputsHtml(q)}` +
      `<div class="rs-actions">${hint}<span class="grow"></span><button type="submit" class="cc-primary"${host.readOnly() ? " disabled" : ""}>${q.kind === "short" || q.kind === "long" ? "Have it marked" : "Check"}</button></div></form>`;
    const last = state.at === ids.length - 1;
    const answeredAll = ids.every((id) => attempt.answers.some((x) => x.question === id));
    return `<div class="st-quiz-progress"><span class="st-faint">Question ${state.at + 1} of ${ids.length}</span><div class="st-dots">${dots}</div></div>` +
      `<div class="st-question" data-turn="made"><div class="st-question-tags st-faint">${esc([KINDS[q.kind], q.topic, q.rubric.length ? `${q.rubric.reduce((n, c) => n + c.points, 0)} marks` : ""].filter(Boolean).join(" · "))}</div>` +
      `<div class="st-question-prompt md">${promptHtml(q, e.citations, blanks, a ? counted(a) : null)}</div>${body}` +
      `<div class="st-question-more"><button class="st-link" data-study-quiz-ask="${esc(q.id)}">${icon("chat")}Ask the tutor about it</button></div></div>` +
      `<div class="st-quiz-nav"><button class="cc-quiet" data-study-quiz-go="${state.at - 1}"${state.at ? "" : " disabled"}>← Back</button><span class="grow"></span>` +
      (last || answeredAll ? `<button class="${answeredAll ? "cc-primary" : "cc-quiet"}" data-study-quiz-finish>Finish</button>` : "") +
      (last ? "" : `<button class="${a ? "cc-primary" : "cc-quiet"}" data-study-quiz-go="${state.at + 1}">Next →</button>`) + `</div>`;
  }

  /** How the attempts went, and what to do next. @param {QuizEvent} e */
  function overviewHtml(e) {
    const ro = host.readOnly();
    const last = e.attempts.at(-1) ?? null;
    const missed = last ? last.questions.filter((id) => { const a = last.answers.find((x) => x.question === id); const g = a ? counted(a) : null; return !g || g.score < g.of - 1e-9; }) : [];
    const begin = ro ? "" : `<div class="rs-actions st-quiz-begin">` +
      (last && missed.length ? `<button class="cc-quiet" data-study-quiz-start="missed">Try again the ${missed.length} missed</button>` : "") +
      `<button class="cc-primary" data-study-quiz-start="all">${last ? "Begin again" : "Begin"}</button></div>`;
    if (!last) {
      const kinds = [...new Set(e.questions.map((q) => KINDS[q.kind]))].join(", ");
      return `<div class="st-card st-quiz-intro"><h2>${e.questions.length} questions</h2><p>${esc(kinds)}. Each is marked as you answer it: by rule where it can be checked, ` +
        `by another model against its rubric where it is written.</p>${begin}</div>`;
    }
    const bars = e.results.map((r) => `<div class="st-result"><span>${r.objective ? `<b class="st-faint">${esc(r.objective)}</b> ` : ""}${inline(r.text)}</span>` +
      `<span class="st-bar"><i style="width:${percent(r.score, r.of)}%"></i></span><b>${percent(r.score, r.of)}%</b></div>`).join("");
    const standing = e.mastery.length ? `<h3>Where those objectives stand</h3><div class="st-standing">${e.mastery.map((m) =>
      `<div class="st-stand ${esc(m.state)}"><span class="st-state">${esc(STANDING[m.state] ?? m.state)}</span><span>${inline(m.text)}</span>` +
      `<span class="st-faint">${m.level === null ? "" : `${Math.round(m.level * 100)}% · `}${m.evidence} done</span></div>`).join("")}</div>` : "";
    const weak = e.mastery.filter((m) => m.state === "focus" || (m.state === "developing" && (m.level ?? 0) < 0.75)).map((m) => m.objective);
    const weakTopics = e.results.filter((r) => !r.objective && r.score / r.of < 0.6).map((r) => r.text);
    const next = ro ? "" : `<div class="rs-actions st-quiz-next">` +
      (weak.length || weakTopics.length ? `<button class="cc-quiet" data-study-quiz-weak>${icon("spark")}A quiz of the weak areas</button>` : "") +
      `<button class="cc-quiet" data-study-quiz-results-ask>${icon("chat")}Ask the tutor about this</button></div>`;
    const history = e.attempts.map((a) => `<li><b>Attempt ${a.n}</b><span>${a.of ? `${a.score} of ${a.of} · ${percent(a.score, a.of)}%` : "nothing marked"}` +
      `${a.questions.length < e.questions.length ? ` · ${a.questions.length} questions` : ""}</span><span class="st-faint">${a.finished ? new Date(a.finished * 1000).toLocaleString() : "under way"}</span></li>`).join("");
    const review = state.reviewing
      ? last.questions.map((id, i) => {
        const q = e.questions.find((x) => x.id === id);
        const a = last.answers.find((x) => x.question === id);
        if (!q) return "";
        return `<div class="st-question reviewed" data-turn="made"><div class="st-question-tags st-faint">Question ${i + 1}</div><div class="st-question-prompt md">` +
          `${promptHtml(q, e.citations, a?.given.kind === "blanks" ? a.given.words : null, a ? counted(a) : null)}</div>` +
          `${a ? markHtml(q, a, e.marking.includes(`${last.n}/${id}`), e.citations) : `<p class="st-faint">Not answered.</p>`}</div>`;
      }).join("")
      : "";
    return `<div class="st-card st-quiz-score"><div><h2>${last.of ? `${percent(last.score, last.of)}%` : "—"}</h2><p>${last.score} of ${last.of} marks, attempt ${last.n}</p></div>${begin}</div>` +
      (bars ? `<h3>${e.results.some((r) => r.objective) ? "By objective" : "By topic"}</h3><div class="st-results">${bars}</div>` : "") + standing + next +
      `<h3>Attempts</h3><ol class="st-attempts">${history}</ol>` +
      `<button class="st-link" data-study-quiz-review>${state.reviewing ? "Hide the answers" : "Go over the answers"}</button>${review}`;
  }

  /** The quiz, whole, for the middle. */
  function html() {
    const e = /** @type {QuizEvent} */ (state.quiz);
    const ro = host.readOnly();
    const what = e.made.kind === "practice" ? "Practice problems" : "Quiz";
    const head = `<div class="st-reader-head"><button class="cc-quiet" data-study-back>← Back</button>` +
      `<div><h1>${esc(e.made.title)}</h1><span class="st-faint">${what} · ${e.questions.length} questions · ${esc(e.made.by)}</span></div>` +
      `<button class="cc-quiet" data-study-task="${esc(e.made.task)}" title="What the call was told and asked">Prompt</button>` +
      (ro ? "" : `<button class="cc-quiet" data-study-bin-made="${esc(e.made.id)}" title="Put it, and its attempts, in the notebook's bin">Bin</button>`) + `</div>`;
    const attempt = current();
    return `<div class="st-reader st-quiz-view">${head}${attempt ? attemptHtml(e, attempt) : overviewHtml(e)}</div>`;
  }

  /** @param {HTMLElement} stage */
  function render(stage) {
    if (!state.quiz) return;
    // What is being written is kept while the quiz is drawn again — an
    // answer to the question shown, while it is unanswered, or a dispute.
    const typing = /** @type {HTMLTextAreaElement | HTMLInputElement | null} */ (stage.querySelector(".st-quiz-view textarea:focus, .st-quiz-view input:focus"));
    if (typing?.value) {
      const attempt = current();
      const shown = attempt?.questions[state.at];
      const open = shown && !attempt?.answers.some((a) => a.question === shown) && typing.closest(".st-question");
      if (open || (state.disputing && typing.closest("[data-study-dispute-form]"))) return;
    }
    stage.innerHTML = html();
    /** @type {HTMLElement | null} */ (stage.querySelector("[data-study-answer-form] input:not([type]), [data-study-answer-form] textarea, [data-study-answer-form] .st-blank, [data-study-dispute-form] textarea"))?.focus({ preventScroll: true });
  }

  /** What a form says the answer is. @param {Question} q @param {HTMLFormElement} form @returns {Given | null} */
  function given(q, form) {
    if (q.kind === "choice") {
      const on = /** @type {HTMLInputElement | null} */ (form.querySelector('input[name="option"]:checked'));
      return on ? { kind: "choice", index: Number(on.value) } : null;
    }
    if (q.kind === "choices") {
      const on = [...form.querySelectorAll('input[name="option"]:checked')].map((i) => Number(/** @type {HTMLInputElement} */ (i).value));
      return on.length ? { kind: "choices", indices: on } : null;
    }
    if (q.kind === "blanks") {
      const words = [...form.closest(".st-question")?.querySelectorAll(".st-blank") ?? []].map((i) => /** @type {HTMLInputElement} */ (i).value.trim());
      return words.some(Boolean) ? { kind: "blanks", words } : null;
    }
    if (q.kind === "number") {
      const text = String(new FormData(form).get("number") ?? "").trim();
      return text ? { kind: "number", text } : null;
    }
    const text = String(new FormData(form).get("text") ?? "");
    return text.trim() ? { kind: "text", text } : null;
  }

  /** A summary of the latest attempt, for the tutor. */
  function summary() {
    const e = /** @type {QuizEvent} */ (state.quiz);
    const last = e.attempts.at(-1);
    if (!last) return "";
    const lines = last.questions.map((id, i) => {
      const q = e.questions.find((x) => x.id === id);
      const a = last.answers.find((x) => x.question === id);
      const g = a ? counted(a) : null;
      const said = !a ? "not answered" : a.given.kind === "choice" ? q?.options[a.given.index] ?? "" : a.given.kind === "choices" ? a.given.indices.map((k) => q?.options[k]).join("; ")
        : a.given.kind === "blanks" ? a.given.words.join("; ") : a.given.text;
      return `${i + 1}. ${q?.prompt ?? id}\n   I answered: ${said}\n   Marked: ${g ? `${g.score} of ${g.of}` : "not marked"}${q?.cites.length ? ` (${q.cites.map((c) => `[[${c}]]`).join(" ")})` : ""}`;
    }).join("\n");
    return `Here is how my attempt at "${e.made.title}" went — ${last.score} of ${last.of}:\n\n${lines}\n\nWhat do my mistakes say I have not understood, and what should I go over first? Explain from the sources.`;
  }

  /** A click in the quiz; `true` if it was the quiz's. @param {HTMLElement} t @param {() => void} redraw */
  function click(t, redraw) {
    const e = state.quiz;
    if (!e || !t.closest(".st-quiz-view")) return false;
    const at = (/** @type {string} */ sel) => /** @type {HTMLElement | null} */ (t.closest(sel));
    const attempt = current();
    const start = at("[data-study-quiz-start]");
    if (start) {
      state.at = 0;
      state.hinted.clear();
      state.reviewing = false;
      host.dispatch({ type: "studyQuizStart", quiz: e.made.id, missed: start.dataset.studyQuizStart === "missed" });
      return true;
    }
    const go = at("[data-study-quiz-go]");
    if (go) {
      state.at = Number(go.dataset.studyQuizGo);
      state.disputing = null;
      redraw();
      return true;
    }
    if (at("[data-study-hint]")) {
      const id = attempt?.questions[state.at];
      if (id) state.hinted.add(id);
      redraw();
      return true;
    }
    if (at("[data-study-quiz-finish]") && attempt) {
      host.dispatch({ type: "studyQuizFinish", quiz: e.made.id, attempt: attempt.n });
      return true;
    }
    const dispute = at("[data-study-dispute]");
    if (dispute) {
      state.disputing = dispute.dataset.studyDispute ?? null;
      redraw();
      return true;
    }
    if (at("[data-study-dispute-cancel]")) {
      state.disputing = null;
      redraw();
      return true;
    }
    const again = at("[data-study-mark-again]");
    if (again) {
      const n = attempt?.n ?? e.attempts.at(-1)?.n;
      if (n) host.dispatch({ type: "studyQuizMarkAgain", quiz: e.made.id, attempt: n, question: again.dataset.studyMarkAgain });
      return true;
    }
    const ask = at("[data-study-quiz-ask]");
    if (ask) {
      const q = e.questions.find((x) => x.id === ask.dataset.studyQuizAsk);
      if (q) host.ask(`Help me with this question from the sources${q.cites.length ? ` (${q.cites.map((c) => `[[${c}]]`).join(" ")})` : ""}:\n\n${q.prompt}`);
      return true;
    }
    if (at("[data-study-quiz-results-ask]")) {
      host.ask(summary());
      return true;
    }
    if (at("[data-study-quiz-weak]")) {
      const weak = e.mastery.filter((m) => m.state === "focus" || (m.state === "developing" && (m.level ?? 0) < 0.75)).map((m) => m.objective);
      const topics = e.results.filter((r) => !r.objective && r.score / r.of < 0.6).map((r) => r.text);
      host.dispatch({ type: "studyMake", kind: e.made.kind === "practice" ? "practice" : "quiz",
        options: { focus: weak.length ? "" : topics.join(", "), ask: "", columns: [], amount: "fewer", level: "medium", deck: null, objectives: weak } });
      return true;
    }
    if (at("[data-study-quiz-review]")) {
      state.reviewing = !state.reviewing;
      redraw();
      return true;
    }
    return false;
  }

  /** @param {HTMLFormElement} form */
  function submit(form) {
    const e = state.quiz;
    if (!e || !form.closest(".st-quiz-view")) return false;
    const attempt = current();
    if (form.matches("[data-study-answer-form]") && attempt) {
      const q = e.questions.find((x) => x.id === form.dataset.studyAnswerForm);
      const answer = q ? given(q, form) : null;
      if (!q || !answer) {
        /** @type {HTMLElement | null} */ (form.closest(".st-question")?.querySelector("input, textarea") ?? null)?.focus();
        return true;
      }
      host.dispatch({ type: "studyQuizAnswer", quiz: e.made.id, attempt: attempt.n, question: q.id, given: answer, hinted: state.hinted.has(q.id) });
      return true;
    }
    if (form.matches("[data-study-dispute-form]")) {
      const why = String(new FormData(form).get("why") ?? "").trim();
      const n = attempt?.n ?? e.attempts.at(-1)?.n;
      if (!why || !n) {
        /** @type {HTMLElement | null} */ (form.querySelector("textarea"))?.focus();
        return true;
      }
      host.dispatch({ type: "studyQuizDispute", quiz: e.made.id, attempt: n, question: form.dataset.studyDisputeForm, why });
      state.disputing = null;
      return true;
    }
    return false;
  }

  /** Keys while answering: 1–9 pick an option, ← and → move between questions.
   *  @param {KeyboardEvent} ev @param {() => void} redraw */
  function key(ev, redraw) {
    const e = state.quiz;
    const attempt = current();
    if (!e || !attempt || ev.metaKey || ev.ctrlKey || ev.altKey) return false;
    const target = /** @type {HTMLElement} */ (ev.target);
    if (target.closest?.("textarea, input:not([type=radio]):not([type=checkbox]), select")) return false;
    if (ev.key === "ArrowRight" || ev.key === "ArrowLeft") {
      const to = state.at + (ev.key === "ArrowRight" ? 1 : -1);
      if (to < 0 || to >= attempt.questions.length) return false;
      state.at = to;
      redraw();
      return true;
    }
    const n = Number(ev.key);
    if (Number.isInteger(n) && n >= 1 && n <= 9) {
      const option = /** @type {HTMLInputElement | undefined} */ ([...document.querySelectorAll('.st-quiz-view [data-study-answer-form] input[name="option"]')][n - 1]);
      if (!option) return false;
      option.checked = option.type === "radio" ? true : !option.checked;
      return true;
    }
    return false;
  }

  /** The quiz, as the core has it now. @param {QuizEvent} event */
  function quiz(event) {
    const changed = event.made.id !== state.quiz?.made.id;
    const before = state.quiz?.attempts.at(-1)?.n;
    state.quiz = event;
    const now = event.attempts.at(-1);
    if (changed || now?.n !== before) {
      // Another quiz, or an attempt begun: from its first question.
      state.at = 0;
      state.hinted.clear();
      state.disputing = null;
      if (changed) state.reviewing = false;
    }
  }

  return {
    render, click, submit, key, quiz,
    /** The quiz shown, if one is. */
    open: () => state.quiz?.made.id ?? null,
    citations: () => state.quiz?.citations ?? [],
    clear: () => { state.quiz = null; },
  };
}

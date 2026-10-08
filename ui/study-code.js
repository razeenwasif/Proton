/* Coding practice: problems made from the sources, each solved here in an
 * editor and run against its tests — built and run by the core in the Lab's
 * sandbox, with the compilers this machine has.
 *
 * The editor is a textarea under its own colouring. What is typed is the
 * browser's, so undo, selection, the clipboard and input methods are too;
 * the colours are highlight.js's, in the classes the editor's own runs use.
 * Its keys follow the Codex editor's typing rules (typing.rs): Tab indents
 * by the code's own step, Enter keeps the indent and opens the block a
 * bracket or a colon begins, brackets and quotes close themselves, and a
 * closer typed on a line of its own goes back a step. What is written is
 * saved as it is written, and run with ⌘⏎. */

import { escapeHtml as esc, icon } from "./html.js";
import { highlight } from "./highlight.js";
import { answerHtml } from "./study-chat.js";

/** @typedef {import("./bindings/StudyMade").StudyMade} Made */
/** @typedef {import("./bindings/StudyCitation").StudyCitation} Citation */
/** @typedef {import("./bindings/StudyCodingProblem").StudyCodingProblem} Problem */
/** @typedef {import("./bindings/StudyCodeSaved").StudyCodeSaved} Saved */
/** @typedef {import("./bindings/StudyCodeOutcome").StudyCodeOutcome} Outcome */
/** @typedef {{made: Made, body: {shape: "coding", title: string, problems: Problem[]}, citations: Citation[]}} CodingOpen */
/** @typedef {{made: string, saved: Saved[], running: string[]}} CodingEvent */
/** @typedef {{made: string, problem: string, outcome: Outcome}} RanEvent */

/**
 * What a key does to code, when it does something of its own: the text from
 * `from` to `to` becomes `text`, and then `start` to `end` is selected.
 * @typedef {{from: number, to: number, text: string, start: number, end: number}} Change
 */

/** Each language: its name, the file its code is saved as, how a line is
 *  commented, the step it is indented by unless the code says otherwise, and
 *  the quotes that close themselves (Rust's `'` begins a lifetime). */
const LANGS = /** @type {Record<string, {name: string, file: string, comment: string, step: string, quotes: string}>} */ ({
  python: { name: "Python", file: "solution.py", comment: "#", step: "    ", quotes: "\"'" },
  c: { name: "C", file: "solution.c", comment: "//", step: "    ", quotes: "\"'" },
  cpp: { name: "C++", file: "solution.cpp", comment: "//", step: "    ", quotes: "\"'" },
  java: { name: "Java", file: "Solution.java", comment: "//", step: "    ", quotes: "\"'" },
  javascript: { name: "JavaScript", file: "solution.js", comment: "//", step: "  ", quotes: "\"'`" },
  rust: { name: "Rust", file: "solution.rs", comment: "//", step: "    ", quotes: "\"" },
});

/** The languages problems can be set in, by key, as a list offers them. */
export const LANGUAGES = /** @type {[string, string][]} */ (Object.entries(LANGS).map(([key, l]) => [key, l.name]));

/** @param {string} key */
const langOf = (key) => LANGS[key] ?? LANGS.python;

const CLOSE = /** @type {Record<string, string>} */ ({ "(": ")", "[": "]", "{": "}" });
const CLOSERS = new Set(Object.values(CLOSE));
/** What may follow a bracket or a quote for it to close itself. */
const BEFORE = /[\s;:.,=}\])>]/;

/** Where the line `i` is on begins, and ends. @param {string} v @param {number} i */
const lineStart = (v, i) => (i <= 0 ? 0 : v.lastIndexOf("\n", i - 1) + 1);
/** @param {string} v @param {number} i */
const lineEnd = (v, i) => {
  const n = v.indexOf("\n", i);
  return n < 0 ? v.length : n;
};
/** A line's indent. @param {string} line */
const indentOf = (line) => /^[ \t]*/.exec(line)?.[0] ?? "";

/** The step code is indented by: a tab, if more of its lines begin with
 *  tabs than with spaces, or the commonest rise in spaces from a line to the
 *  next — else `fallback`. @param {string} code @param {string} fallback */
export function indentUnit(code, fallback) {
  const lines = code.split("\n").filter((l) => l.trim());
  if (lines.filter((l) => l.startsWith("\t")).length > lines.filter((l) => l.startsWith(" ")).length) return "\t";
  /** @type {Map<number, number>} */
  const rises = new Map();
  let last = 0;
  for (const line of lines) {
    const n = indentOf(line).length;
    if (n > last) rises.set(n - last, (rises.get(n - last) ?? 0) + 1);
    last = n;
  }
  let best = 0;
  let most = 0;
  for (const [rise, times] of rises) {
    if (times > most || (times === most && rise < best)) {
      best = rise;
      most = times;
    }
  }
  return best >= 2 && best <= 8 ? " ".repeat(best) : fallback;
}

/** The lines a selection touches: from the start of its first to the end of
 *  its last, a selection that ends at a line's start not taking that line.
 *  @param {string} v @param {number} s @param {number} f */
function linesOf(v, s, f) {
  return { from: lineStart(v, s), to: lineEnd(v, f > s && v[f - 1] === "\n" ? f - 1 : f) };
}

/** The lines selected indented a step, or outdented one (`dir` −1).
 *  @param {{value: string, start: number, end: number}} at @param {number} dir @param {string} unit @returns {Change | null} */
function shiftLines(at, dir, unit) {
  const { value: v, start: s, end: f } = at;
  const { from, to } = linesOf(v, s, f);
  const lines = v.slice(from, to).split("\n");
  const less = unit === "\t" ? /^\t/ : new RegExp(`^ {1,${unit.length}}|^\\t`);
  const text = lines.map((l) => (dir > 0 ? (l.trim() ? unit + l : l) : l.replace(less, ""))).join("\n");
  if (text === v.slice(from, to)) return null;
  // A caret on one line moves with its text.
  if (s === f && lines.length === 1) {
    const moved = Math.max(from, s + text.length - (to - from));
    return { from, to, text, start: moved, end: moved };
  }
  return { from, to, text, start: from, end: from + text.length };
}

/** A line comment put on the lines selected, or taken off when every one
 *  that has anything on it has one. @param {{value: string, start: number, end: number}} at
 *  @param {string} token @returns {Change | null} */
export function toggleComment(at, token) {
  const { value: v, start: s, end: f } = at;
  const { from, to } = linesOf(v, s, f);
  const lines = v.slice(from, to).split("\n");
  const filled = lines.filter((l) => l.trim());
  if (!filled.length) return null;
  const commented = filled.every((l) => l.trimStart().startsWith(token));
  const margin = Math.min(...filled.map((l) => indentOf(l).length));
  const text = lines.map((l) => {
    if (!l.trim()) return l;
    if (!commented) return `${l.slice(0, margin)}${token} ${l.slice(margin)}`;
    const i = l.indexOf(token);
    return l.slice(0, i) + l.slice(i + token.length).replace(/^ /, "");
  }).join("\n");
  if (s === f && lines.length === 1) {
    const moved = Math.max(from, s + text.length - (to - from));
    return { from, to, text, start: moved, end: moved };
  }
  return { from, to, text, start: from, end: from + text.length };
}

/** What `key` does to code selected from `start` to `end`, if it does
 *  something of its own; `null` leaves it to the browser.
 *  @param {{value: string, start: number, end: number}} at @param {string} key
 *  @param {{shift?: boolean, unit: string, lang: string}} how @returns {Change | null} */
export function keyChange(at, key, how) {
  const { value: v, start: s, end: f } = at;
  const unit = how.unit;
  const change = (/** @type {number} */ from, /** @type {number} */ to, /** @type {string} */ text, start = from + text.length, end = start) => ({ from, to, text, start, end });
  /** The caret moved past what is there, nothing typed. @param {number} i */
  const past = (i) => ({ from: i, to: i, text: "", start: i, end: i });
  if (key === "Tab") {
    if (how.shift || v.slice(s, f).includes("\n")) return shiftLines(at, how.shift ? -1 : 1, unit);
    if (unit === "\t") return change(s, f, "\t");
    const column = s - lineStart(v, s);
    return change(s, f, " ".repeat(unit.length - (column % unit.length)));
  }
  if (key === "Enter") {
    const start = lineStart(v, s);
    const indent = indentOf(v.slice(start, s));
    const last = v.slice(start, s).trimEnd().at(-1) ?? "";
    // Between a pair, the closer goes to a line of its own, a step back.
    if (last in CLOSE && s === f && v[s] === CLOSE[last]) {
      const inner = `\n${indent}${unit}`;
      return change(s, f, `${inner}\n${indent}`, s + inner.length);
    }
    const opens = last in CLOSE || (how.lang === "python" && last === ":");
    return change(s, f, `\n${indent}${opens ? unit : ""}`);
  }
  if (key === "Backspace") {
    if (s !== f || s === 0) return null;
    const open = v[s - 1];
    // An empty pair goes whole.
    if ((CLOSE[open] && v[s] === CLOSE[open]) || (langOf(how.lang).quotes.includes(open) && v[s] === open)) return change(s - 1, s + 1, "");
    // In the indent, back a whole step.
    const before = v.slice(lineStart(v, s), s);
    if (unit !== "\t" && /^ +$/.test(before)) return change(s - (before.length % unit.length || unit.length), s, "");
    return null;
  }
  if (key in CLOSE) {
    // A selection is put between the pair.
    if (s !== f) return change(s, f, key + v.slice(s, f) + CLOSE[key], s + 1, f + 1);
    return s >= v.length || BEFORE.test(v[s]) ? change(s, f, key + CLOSE[key], s + 1) : null;
  }
  if (CLOSERS.has(key)) {
    if (s !== f) return null;
    if (v[s] === key) return past(s + 1);
    const start = lineStart(v, s);
    const before = v.slice(start, s);
    if (/^[ \t]+$/.test(before)) {
      const less = before.endsWith("\t") ? before.slice(0, -1) : before.slice(0, Math.max(0, before.length - (before.length % unit.length || unit.length)));
      return change(start, s, less + key);
    }
    return null;
  }
  if (key.length === 1 && langOf(how.lang).quotes.includes(key)) {
    if (s !== f) return change(s, f, key + v.slice(s, f) + key, s + 1, f + 1);
    if (v[s] === key) return past(s + 1);
    // The third of Python's quotes opens a string closed by three.
    if (how.lang === "python" && v.slice(s - 2, s) === key + key && v[s - 3] !== key) return change(s, f, key.repeat(4), s + 1);
    const prev = v[s - 1];
    const opens = (prev === undefined || !/[\w\\]/.test(prev)) && (s >= v.length || BEFORE.test(v[s]));
    return opens ? change(s, f, key + key, s + 1) : null;
  }
  return null;
}

/** Make a change as typing would — so the browser's undo has it — and
 *  select what it says. @param {HTMLTextAreaElement} ta @param {Change} c */
function apply(ta, c) {
  if (c.from !== c.to || c.text) {
    ta.setSelectionRange(c.from, c.to);
    const typed = c.text ? document.execCommand("insertText", false, c.text) : document.execCommand("delete", false);
    if (!typed) {
      ta.setRangeText(c.text, c.from, c.to, "end");
      ta.dispatchEvent(new Event("input", { bubbles: true }));
    }
  }
  ta.setSelectionRange(c.start, c.end);
}

/** A number of seconds, as a run took them. @param {number} s */
const took = (s) => (s < 1 ? `${Math.round(s * 1000)} ms` : `${s.toFixed(1)} s`);
/** @param {string} s */
const capital = (s) => s.charAt(0).toUpperCase() + s.slice(1);

/** @param {{dispatch: (command: any) => unknown, readOnly: () => boolean, stage: HTMLElement}} host */
export function createCoding(host) {
  /** @type {{open: CodingOpen | null, at: number, saved: Map<string, Saved>, running: Set<string>, outcomes: Map<string, Outcome>,
   *   drafts: Map<string, string>, hints: Map<string, number>, solutions: Set<string>, said: string}} */
  const state = { open: null, at: 0, saved: new Map(), running: new Set(), outcomes: new Map(), drafts: new Map(), hints: new Map(), solutions: new Set(), said: "" };
  /** What is waiting to be saved, and when it will be. */
  let pending = /** @type {{made: string, problem: string, code: string} | null} */ (null);
  let timer = 0;

  const problems = () => state.open?.body.problems ?? [];
  const current = () => problems()[state.at];
  const view = () => /** @type {HTMLElement | null} */ (host.stage.querySelector("[data-study-code-view]"));
  const input = () => /** @type {HTMLTextAreaElement | null} */ (host.stage.querySelector("[data-study-code-input]"));
  /** What the editor begins a problem with: what was being written, what
   *  was saved, or the starter. @param {Problem} p */
  const codeOf = (p) => state.drafts.get(p.id) ?? state.saved.get(p.id)?.code ?? p.starter;
  /** The step a problem's code is indented by. @param {Problem} p */
  const unitOf = (p) => indentUnit(`${p.starter}\n${p.solution}`, langOf(p.language).step);

  /** Save what is waiting now. */
  function flush() {
    clearTimeout(timer);
    if (!pending) return;
    host.dispatch({ type: "studyCodeSave", ...pending });
    pending = null;
    say("Saved");
  }

  /** Say how saving stands. @param {string} what */
  function say(what) {
    state.said = what;
    const at = host.stage.querySelector("[data-study-code-said]");
    if (at) at.textContent = what;
  }

  /** The editor's colouring and line numbers, drawn again from what is in it. */
  function paint() {
    const ta = input();
    const p = current();
    if (!ta || !p) return;
    const box = /** @type {HTMLElement} */ (ta.closest("[data-study-code-editor]"));
    const hl = /** @type {HTMLElement} */ (box.querySelector("[data-study-code-hl]"));
    const gutter = /** @type {HTMLElement} */ (box.querySelector("[data-study-code-gutter]"));
    // A last line that is empty has its height only if something follows.
    hl.innerHTML = `${highlight(ta.value, p.language)}\n`;
    const lines = String(ta.value.split("\n").length);
    if (gutter.dataset.lines !== lines) {
      gutter.dataset.lines = lines;
      gutter.textContent = Array.from({ length: Number(lines) }, (_, i) => i + 1).join("\n");
    }
    follow();
  }

  /** The colouring and the line numbers kept where the text has scrolled to. */
  function follow() {
    const ta = input();
    if (!ta) return;
    const box = /** @type {HTMLElement} */ (ta.closest("[data-study-code-editor]"));
    const hl = /** @type {HTMLElement} */ (box.querySelector("[data-study-code-hl]"));
    hl.scrollTop = ta.scrollTop;
    hl.scrollLeft = ta.scrollLeft;
    /** @type {HTMLElement} */ (box.querySelector("[data-study-code-gutter]")).scrollTop = ta.scrollTop;
  }

  function headHtml() {
    const open = /** @type {CodingOpen} */ (state.open);
    const solved = problems().filter((p) => state.saved.get(p.id)?.solved).length;
    return `<div class="st-reader-head"><button class="cc-quiet" data-study-back>← Back</button>` +
      `<div><h1>${esc(open.made.title)}</h1><span class="st-faint" data-study-code-tally>Coding practice · ${solved} of ${problems().length} solved · ${esc(open.made.by)}</span></div>` +
      `<button class="cc-quiet" data-study-task="${esc(open.made.task)}" title="What the call was told and asked">Prompt</button>` +
      (host.readOnly() ? "" : `<button class="cc-quiet" data-study-bin-made="${esc(open.made.id)}" title="Put it in the notebook's bin">Bin</button>`) + `</div>`;
  }

  function tabsHtml() {
    return problems().map((p, i) => {
      const saved = state.saved.get(p.id);
      const mark = state.running.has(p.id) ? '<span class="lab-spin"></span>'
        : saved?.solved ? icon("check") : saved?.of ? `<span class="st-code-score">${saved.passed}/${saved.of}</span>` : "";
      return `<button class="st-code-tab${i === state.at ? " on" : ""}${saved?.solved ? " solved" : ""}" role="tab" aria-selected="${i === state.at}" data-study-code-go="${i}"` +
        ` title="${esc(p.title)}"><span class="st-code-n">${i + 1}</span><span class="st-code-name">${esc(p.title)}</span>${mark}</button>`;
    }).join("");
  }

  /** Which view is drawn: the set, the problem, and whether it may be
   *  written — a notebook that becomes read only is drawn again, saying so.
   *  @param {Problem} p */
  const viewKey = (p) => `${state.open?.made.id}:${p.id}${host.readOnly() ? ":read" : ""}`;

  /** What the button that shows a solution says. @param {Problem} p */
  const revealHtml = (p) => `${icon("eye")}${state.saved.get(p.id)?.solved ? "Compare with a solution" : "Show a solution"}`;

  /** The problem: what to write, its hints and, when asked for, a solution.
   *  @param {Problem} p */
  function problemHtml(p) {
    const citations = state.open?.citations ?? [];
    const shown = state.hints.get(p.id) ?? 0;
    const meta = `<div class="st-code-meta"><span class="st-code-level ${esc(p.difficulty)}">${esc(capital(p.difficulty || "medium"))}</span>` +
      (p.topic ? `<span class="st-code-topic">${esc(p.topic)}</span>` : "") + `<span class="st-faint">${esc(langOf(p.language).name)}</span></div>`;
    const hints = p.hints.length ? `<div class="st-code-hints">${shown ? `<ol>${p.hints.slice(0, shown).map((h) => `<li class="md">${answerHtml(h, citations)}</li>`).join("")}</ol>` : ""}` +
      (shown < p.hints.length ? `<button class="st-link" data-study-code-hint>${icon("bulb")}${shown ? "Another hint" : "A hint"} (${shown + 1} of ${p.hints.length})</button>` : "") + `</div>` : "";
    const solution = state.solutions.has(p.id)
      ? `<section class="st-code-solution"><h3>A solution</h3><pre class="st-code-read">${highlight(p.solution, p.language)}</pre>` +
        (p.explanation ? `<div class="md">${answerHtml(p.explanation, citations)}</div>` : "") + `</section>`
      : `<button class="st-link" data-study-code-solution>${revealHtml(p)}</button>`;
    const tests = `<details class="st-code-src"><summary>The tests it is run against</summary><pre class="st-code-read">${highlight(p.tests, p.language)}</pre></details>`;
    return `<h2>${esc(p.title)}</h2>${meta}<div class="md st-code-prompt">${answerHtml(p.prompt, citations)}</div>${hints}${solution}${tests}`;
  }

  /** How the last run went, or that one is under way. @param {Problem} p */
  function resultsHtml(p) {
    if (state.running.has(p.id)) {
      return `<div class="st-code-status"><span class="lab-spin"></span>${["python", "javascript"].includes(p.language) ? "Running" : "Building and running"} the tests, sandboxed…</div>`;
    }
    const o = state.outcomes.get(p.id);
    if (!o) {
      const saved = state.saved.get(p.id);
      return `<div class="st-code-status st-faint">${saved?.of ? `Your best run passed ${saved.passed} of ${saved.of} tests${saved.solved ? " — solved" : ""}. ` : ""}` +
        `Run it against the tests with ⌘⏎.</div>`;
    }
    const n = o.results.length;
    const passing = o.results.filter((r) => r.passed).length;
    const all = o.built && !o.stopped && n > 0 && passing === n;
    const verdict = all ? `${icon("check")}All ${n} tests passed`
      : !o.built ? `${icon("d-error")}${o.stopped ? capital(o.stopped) : "It did not build"}`
        : o.stopped ? `${icon("d-error")}${capital(o.stopped)}${n ? ` — ${passing} of ${n} tests passed` : ""}`
          : n ? `${icon("d-error")}${passing} of ${n} tests passed` : `${icon("d-warning")}No test was run`;
    const rows = o.results.map((r) => `<li class="${r.passed ? "pass" : "fail"}">${icon(r.passed ? "check" : "close")}<b>${esc(r.name)}</b>` +
      (r.message ? `<span>${esc(r.message)}</span>` : "") + `</li>`).join("");
    const compiler = o.compiler.trim() ? `<details class="st-code-more"${o.built ? "" : " open"}><summary>${o.built ? "What the compiler warned of" : "What the compiler said"}</summary>` +
      `<pre class="st-code-out">${esc(o.compiler)}</pre></details>` : "";
    const printed = o.output.trim() ? `<details class="st-code-more" open><summary>What it printed</summary><pre class="st-code-out">${esc(o.output)}</pre></details>` : "";
    const errors = o.errors.trim() ? `<details class="st-code-more"${all ? "" : " open"}><summary>Its errors</summary><pre class="st-code-out err">${esc(o.errors)}</pre></details>` : "";
    return `<div class="st-code-verdict ${all ? "pass" : "fail"}">${verdict}<span class="st-faint">${took(o.seconds)}</span></div>` +
      (rows ? `<ul class="st-code-tests">${rows}</ul>` : "") + compiler + printed + errors;
  }

  /** The whole view of a problem. @param {Problem} p */
  function html(p) {
    const ro = host.readOnly();
    const lang = langOf(p.language);
    const banner = ro ? `<div class="st-banner">${icon("users")} Another Proton has this notebook open: what you write here is not saved or run.</div>` : "";
    const unit = unitOf(p);
    return `<div class="st-reader st-code-view" data-study-code-view="${esc(viewKey(p))}">${banner}${headHtml()}` +
      `<div class="st-code-tabs" role="tablist" data-study-code-tabs>${tabsHtml()}</div>` +
      `<div class="st-code-split"><div class="st-code-problem" data-turn="made" data-study-code-problem>${problemHtml(p)}</div>` +
      `<div class="st-code-work"><div class="st-code-bar"><span class="st-code-file">${icon("file-code")}${esc(lang.file)}</span>` +
      `<span class="st-faint" data-study-code-said>${esc(state.said)}</span><span class="grow"></span>` +
      `<button class="cc-quiet" data-study-code-reset title="Begin again from the starter — ⌘Z brings yours back"${ro ? " disabled" : ""}>${icon("undo")}Starter</button>` +
      `<button class="cc-primary" data-study-code-run title="Build it and run the tests (⌘⏎)"${ro || state.running.has(p.id) ? " disabled" : ""}>${icon("play")}Run</button></div>` +
      `<div class="st-code-editor" data-study-code-editor style="tab-size: ${unit === "\t" ? 4 : unit.length}">` +
      `<pre class="st-code-gutter" data-study-code-gutter aria-hidden="true"></pre><div class="st-code-area">` +
      `<pre class="st-code-hl" data-study-code-hl aria-hidden="true"></pre>` +
      `<textarea class="st-code-input" data-study-code-input spellcheck="false" autocomplete="off" autocapitalize="off" autocorrect="off" wrap="off"` +
      ` aria-label="Your ${esc(lang.name)} for ${esc(p.title)}"${ro ? " readonly" : ""}></textarea></div></div>` +
      `<div class="st-code-results" data-study-code-results aria-live="polite">${resultsHtml(p)}</div></div></div></div>`;
  }

  /** The view drawn for the problem shown, or what changed of it redrawn
   *  where it is: what is being typed is never drawn over. */
  function render() {
    const p = current();
    if (!state.open || !p) {
      host.stage.innerHTML = "";
      return;
    }
    const at = view();
    if (at?.dataset.studyCodeView === viewKey(p)) {
      const tabs = at.querySelector("[data-study-code-tabs]");
      if (tabs) tabs.innerHTML = tabsHtml();
      const results = at.querySelector("[data-study-code-results]");
      if (results) results.innerHTML = resultsHtml(p);
      const tally = at.querySelector("[data-study-code-tally]");
      const solved = problems().filter((x) => state.saved.get(x.id)?.solved).length;
      if (tally) tally.textContent = `Coding practice · ${solved} of ${problems().length} solved · ${state.open.made.by}`;
      const run = /** @type {HTMLButtonElement | null} */ (at.querySelector("[data-study-code-run]"));
      if (run) run.disabled = host.readOnly() || state.running.has(p.id);
      const reveal = at.querySelector("[data-study-code-solution]");
      if (reveal) reveal.innerHTML = revealHtml(p);
      return;
    }
    flush();
    host.stage.innerHTML = html(p);
    const ta = /** @type {HTMLTextAreaElement} */ (input());
    ta.value = codeOf(p);
    ta.addEventListener("scroll", follow);
    ta.addEventListener("blur", flush);
    paint();
  }

  /** The problem the side shows, drawn again: a hint or a solution asked for. */
  function redrawProblem() {
    const p = current();
    const side = host.stage.querySelector("[data-study-code-problem]");
    if (p && side) side.innerHTML = problemHtml(p);
  }

  /** Build and run what is written. */
  function run() {
    const p = current();
    const ta = input();
    if (!state.open || !p || !ta || host.readOnly() || state.running.has(p.id)) return;
    // The run saves it.
    clearTimeout(timer);
    pending = null;
    state.drafts.set(p.id, ta.value);
    state.running.add(p.id);
    host.dispatch({ type: "studyCodeRun", made: state.open.made.id, problem: p.id, code: ta.value });
    say("Saved");
    render();
  }

  /** Go to a problem. @param {number} i */
  function go(i) {
    if (i === state.at || i < 0 || i >= problems().length) return;
    flush();
    state.at = i;
    render();
    input()?.focus({ preventScroll: true });
  }

  /** A click on the view; `true` if it was the view's. @param {HTMLElement} t */
  function click(t) {
    const at = (/** @type {string} */ sel) => /** @type {HTMLElement | null} */ (t.closest(sel));
    const tab = at("[data-study-code-go]");
    if (tab) {
      go(Number(tab.dataset.studyCodeGo));
      return true;
    }
    if (at("[data-study-code-run]")) {
      run();
      return true;
    }
    const p = current();
    if (!p) return false;
    if (at("[data-study-code-reset]")) {
      const ta = input();
      if (ta && !host.readOnly() && ta.value !== p.starter) {
        ta.focus({ preventScroll: true });
        apply(ta, { from: 0, to: ta.value.length, text: p.starter, start: 0, end: 0 });
      }
      return true;
    }
    if (at("[data-study-code-hint]")) {
      state.hints.set(p.id, Math.min(p.hints.length, (state.hints.get(p.id) ?? 0) + 1));
      redrawProblem();
      return true;
    }
    if (at("[data-study-code-solution]")) {
      state.solutions.add(p.id);
      redrawProblem();
      return true;
    }
    return false;
  }

  /** Something typed into the editor: coloured, and saved soon. @param {HTMLElement} t */
  function typed(t) {
    if (!t.matches("[data-study-code-input]") || !state.open) return false;
    const p = current();
    const ta = /** @type {HTMLTextAreaElement} */ (t);
    if (!p) return true;
    paint();
    state.drafts.set(p.id, ta.value);
    if (host.readOnly()) return true;
    pending = { made: state.open.made.id, problem: p.id, code: ta.value };
    say("Saving…");
    clearTimeout(timer);
    timer = Number(setTimeout(flush, 700));
    return true;
  }

  /** A key in the editor; `true` if it was the editor's. @param {KeyboardEvent} e */
  function key(e) {
    const ta = e.target;
    const p = current();
    if (!(ta instanceof HTMLTextAreaElement) || !ta.matches("[data-study-code-input]") || !p) return false;
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.key === "Enter") {
      e.preventDefault();
      run();
      return true;
    }
    if (mod && e.key.toLowerCase() === "s" && !e.shiftKey) {
      e.preventDefault();
      flush();
      return true;
    }
    const at = { value: ta.value, start: ta.selectionStart, end: ta.selectionEnd };
    if (mod && e.key === "/") {
      e.preventDefault();
      const c = !ta.readOnly && toggleComment(at, langOf(p.language).comment);
      if (c) apply(ta, c);
      return true;
    }
    if (mod || e.altKey || e.isComposing || ta.readOnly) return false;
    if (e.key === "Enter" && e.shiftKey) return false;
    const c = keyChange(at, e.key, { shift: e.shiftKey, unit: unitOf(p), lang: p.language });
    if (!c) return false;
    e.preventDefault();
    apply(ta, c);
    return true;
  }

  /** A set of problems, opened. @param {CodingOpen} event */
  function open(event) {
    if (state.open?.made.id !== event.made.id) {
      flush();
      Object.assign(state, { at: 0, saved: new Map(), running: new Set(), outcomes: new Map(), drafts: new Map(), hints: new Map(), solutions: new Set(), said: "" });
    }
    state.open = event;
    state.at = Math.min(state.at, Math.max(0, event.body.problems.length - 1));
  }

  /** What has been written for each problem and how its runs went, and
   *  which are running. @param {CodingEvent} event */
  function coding(event) {
    if (state.open?.made.id !== event.made) return false;
    state.saved = new Map(event.saved.map((s) => [s.problem, s]));
    state.running = new Set(event.running);
    // The code kept, in an editor not yet typed in.
    const p = current();
    const ta = input();
    if (p && ta && !state.drafts.has(p.id) && ta.value !== codeOf(p)) {
      ta.value = codeOf(p);
      paint();
    }
    return true;
  }

  /** What a run came to. @param {RanEvent} event */
  function ran(event) {
    if (state.open?.made.id !== event.made) return false;
    state.running.delete(event.problem);
    state.outcomes.set(event.problem, event.outcome);
    return true;
  }

  function clear() {
    flush();
    state.open = null;
  }

  return { open, coding, ran, render, click, typed, key, flush, clear, made: () => state.open?.made.id ?? null, citations: () => state.open?.citations ?? [] };
}

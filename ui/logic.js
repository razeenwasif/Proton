/* Logic: a project's .logic, .proof and .l4f files, the one open as typed,
 * and what the statement at the cursor is (docs/LOGIC.md); and a theory's
 * models, found by the core's search on a thread and shown one at a time.
 *
 * The editor is a textarea under its own colouring, as Notebook's coding
 * practice has: what is typed is the browser's, undo and input methods too.
 * The text goes to the core a moment after typing stops, held there until
 * ⌘S; the line the caret is on is asked a moment after it settles, and the
 * core answers with the engine's: a truth table, what the formula is, its
 * forms and its Karnaugh map, an argument's verdict, two formulas compared.
 * A formula's table can be built as a circuit in Circuit. */

import { escapeHtml as esc, icon } from "./html.js";
import { drawKmap } from "./kmap.js";

/** @typedef {import("./bindings/LogicFound").LogicFound} Found */
/** @typedef {import("./bindings/LogicFileView").LogicFileView} FileView */
/** @typedef {import("./bindings/LogicAnswerView").LogicAnswerView} Answer */
/** @typedef {import("./bindings/LogicModelsView").LogicModelsView} Models */
/** @typedef {import("./bindings/LogicTableView").LogicTableView} Table */

/** No more rows of a truth table than this are drawn. */
const MOST_ROWS = 256;
/** How long typing must stop before the text, and then the line, are sent. */
const EDIT_WAIT = 150;
const ASK_WAIT = 220;

const KEYWORDS = new Set(["all", "some", "exactly", "at", "least", "most", "not", "and", "or", "xor", "in", "true", "false", "first", "last", "size",
  "onto", "partial", "symmetric", "injective", "bijective", "surjective",
  // Logic4Fun's.
  "ALL", "SOME", "NOT", "AND", "OR", "XOR", "IMP", "IFF", "TRUE", "FALSE", "EST", "DIF", "MIN", "MAX", "PRED", "SUCC", "enum", "cardinality",
  "function", "predicate", "name", "all_different", "Sorts", "Vocabulary", "Constraints"]);

/** The text coloured: comments, `?`, keywords, operators and numbers.
 *  @param {string} text @param {boolean} l4f */
export function colour(text, l4f) {
  const comment = l4f ? "%" : "#";
  return text.split("\n").map((line) => {
    let out = "", rest = line;
    const at = rest.indexOf(comment);
    const tail = at >= 0 ? rest.slice(at) : "";
    rest = at >= 0 ? rest.slice(0, at) : rest;
    const re = /(\?)|([A-Za-z_][A-Za-z0-9_']*)|(\d+)|(<->|<=>|->|=>|\|=|\|-|==|!=|<=|>=|\.\.|[¬∧∨⊕→↔⊨⊢∀∃⊤⊥~!&|^=<>+*-])|(\s+|.)/gu;
    for (const m of rest.matchAll(re)) {
      const [t, q, word, num, op] = m;
      if (q) out += `<span class="lg-q">?</span>`;
      else if (word) out += KEYWORDS.has(word) ? `<span class="lg-k">${esc(word)}</span>` : esc(word);
      else if (num) out += `<span class="lg-n">${num}</span>`;
      else if (op) out += `<span class="lg-o">${esc(op)}</span>`;
      else out += esc(t);
    }
    return out + (tail ? `<span class="lg-c">${esc(tail)}</span>` : "");
  }).join("\n");
}

/** The line of a position in text, from 1. @param {string} text @param {number} at */
export function lineAt(text, at) {
  let n = 1;
  for (let i = 0; i < at && i < text.length; i++) if (text.charCodeAt(i) === 10) n++;
  return n;
}

/** @param {{root: Document | HTMLElement, dispatch: (cmd: any) => any}} host */
export function createLogic(host) {
  const ws = /** @type {HTMLElement} */ (host.root.querySelector('.workspace[data-ws="logic"]'));
  if (!ws) return null;
  const q = (/** @type {string} */ s) => /** @type {HTMLElement} */ (ws.querySelector(s));
  const els = {
    list: q("[data-logic-list]"),
    home: q("[data-logic-home]"),
    found: q("[data-logic-found]"),
    form: /** @type {HTMLFormElement} */ (q("[data-logic-form]")),
    file: q("[data-logic-file]"),
    title: q("[data-logic-title]"),
    dirty: q("[data-logic-dirty]"),
    kind: q("[data-logic-kind]"),
    text: /** @type {HTMLTextAreaElement} */ (q("[data-logic-text]")),
    hl: q("[data-logic-hl]"),
    gutter: q("[data-logic-gutter]"),
    diags: q("[data-logic-diags]"),
    answer: q("[data-logic-answer]"),
  };
  const state = {
    /** @type {Found[]} */ files: [],
    /** @type {FileView | null} */ file: null,
    /** @type {Answer | null} */ answer: null,
    /** The line last asked, so the caret moving within it asks nothing. */
    asked: 0,
    /** When the text was last typed in: the core's copy is not put back over it. */
    typedAt: 0,
    /** The from-a-table form's last values, kept across answers. */
    names: "", column: "",
    /** The last search, and which of its models is shown. @type {Models | null} */
    models: null,
    shownModel: 0,
    /** Which the panel shows: the line's answer, or the theory's models. */
    view: /** @type {"line" | "models"} */ ("line"),
  };
  let editTimer = 0, askTimer = 0;

  /* ── the home page and the list ───────────────────────────────────── */

  function renderList() {
    const rows = state.files.map((f) => `<button class="xl-row${f.path === state.file?.path ? " on" : ""}" data-logic-open="${esc(f.path)}" title="${esc(f.path)}">
        ${icon(f.kind === "l4f" ? "file" : "logic")}<span class="xl-row-name">${esc(f.name)}</span>${f.kind === "logic" ? "" : `<span class="xl-kind">${f.kind === "l4f" ? "Logic4Fun" : esc(f.kind)}</span>`}</button>`).join("");
    els.list.innerHTML = rows || `<p class="xl-empty">No .logic files in this folder yet.</p>`;
    els.found.innerHTML = rows;
  }

  /* ── the file ─────────────────────────────────────────────────────── */

  const isL4f = () => state.file?.kind === "l4f";

  function paint() {
    const text = els.text.value;
    // A last newline needs a line under it to scroll to.
    els.hl.innerHTML = colour(text, isL4f()) + "\n ";
    const bad = new Set((state.file?.diagnostics ?? []).map((d) => d.line));
    // The line the answer is of, marked.
    const answered = state.answer && state.answer.kind !== "table" ? state.answer.line : 0;
    const n = text.split("\n").length;
    els.gutter.innerHTML = Array.from({ length: n }, (_, i) => {
      const cls = [bad.has(i + 1) ? "bad" : "", i + 1 === answered ? "on" : ""].filter(Boolean).join(" ");
      return cls ? `<span class="${cls}">${i + 1}</span>` : String(i + 1);
    }).join("\n") + "\n ";
    follow();
  }

  function follow() {
    els.hl.scrollTop = els.text.scrollTop;
    els.hl.scrollLeft = els.text.scrollLeft;
    els.gutter.scrollTop = els.text.scrollTop;
  }

  function renderFile() {
    const f = state.file;
    els.home.hidden = !!f;
    els.file.hidden = !f;
    renderList();
    if (!f) return;
    els.title.textContent = f.name;
    els.dirty.hidden = !f.dirty;
    els.kind.textContent = f.kind === "l4f" ? "Logic4Fun" : f.kind === "proof" ? "proof" : "";
    els.kind.title = f.notes.length ? `Left out of Logic4Fun's text: ${f.notes.join("; ")}` : "";
    // The core's copy goes in when it is another file, or when nothing has
    // been typed lately — never over what is being typed.
    const fresh = Number(els.text.dataset.generation) !== f.generation;
    if (fresh || (els.text.value !== f.text && performance.now() - state.typedAt > 1000)) {
      const at = els.text.selectionStart;
      els.text.value = f.text;
      els.text.dataset.generation = String(f.generation);
      if (!fresh) els.text.setSelectionRange(Math.min(at, f.text.length), Math.min(at, f.text.length));
      state.asked = 0;
    }
    paint();
    els.diags.innerHTML = f.diagnostics.slice(0, 6).map((d) =>
      `<button class="lg-diag" data-logic-goto="${d.line}">${icon("d-error")}<b>line ${d.line}</b> ${esc(d.message)}</button>`).join("") +
      (f.diagnostics.length > 6 ? `<p class="lg-faint">and ${f.diagnostics.length - 6} more</p>` : "") +
      (f.notes.length ? `<p class="lg-faint">Logic4Fun's settings left out: ${esc(f.notes.join("; "))}</p>` : "");
  }

  function send() {
    clearTimeout(editTimer);
    const f = state.file;
    if (f) host.dispatch({ type: "logicEdit", generation: f.generation, text: els.text.value });
  }

  /** The line at the caret asked, unless it was just asked. @param {boolean} [force] @param {boolean} [more] */
  function ask(force = false, more = false) {
    clearTimeout(askTimer);
    const f = state.file;
    if (!f) return;
    const line = lineAt(els.text.value, els.text.selectionStart);
    if (!force && line === state.asked) return;
    state.asked = line;
    host.dispatch({ type: "logicAsk", generation: f.generation, line, more });
  }

  els.text.addEventListener("input", () => {
    state.typedAt = performance.now();
    paint();
    clearTimeout(editTimer);
    editTimer = window.setTimeout(send, EDIT_WAIT);
    clearTimeout(askTimer);
    // What is typed on a line changes its answer: asked again once it settles.
    askTimer = window.setTimeout(() => {
      send();
      ask(true);
    }, ASK_WAIT);
  });
  els.text.addEventListener("scroll", follow);
  const moved = () => {
    clearTimeout(askTimer);
    askTimer = window.setTimeout(() => ask(), ASK_WAIT);
  };
  els.text.addEventListener("keyup", moved);
  els.text.addEventListener("click", moved);
  els.text.addEventListener("keydown", (e) => {
    if (e.metaKey && e.shiftKey && e.key === "Enter") {
      e.preventDefault();
      solve(false);
    } else if (e.key === "Escape" && state.models?.running) {
      e.preventDefault();
      host.dispatch({ type: "logicStop" });
    } else if (e.metaKey && e.key === "Enter") {
      e.preventDefault();
      send();
      ask(true);
    } else if (e.metaKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      send();
      host.dispatch({ type: "logicSave" });
    } else if (e.key === "Tab" && !e.metaKey && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      typeIn("  ");
    }
  });

  /** Text typed at the caret, as if typed: undo takes it back. @param {string} t */
  function typeIn(t) {
    els.text.focus();
    if (!document.execCommand?.("insertText", false, t)) {
      els.text.setRangeText(t, els.text.selectionStart, els.text.selectionEnd, "end");
      els.text.dispatchEvent(new Event("input"));
    }
  }

  /** The theory's models searched for: the first hundred, or all counted.
   *  @param {boolean} count */
  function solve(count) {
    const f = state.file;
    if (!f) return;
    send();
    state.view = "models";
    state.shownModel = 0;
    renderAnswer();
    host.dispatch({ type: "logicSolve", generation: f.generation, keep: 100, count });
  }

  /** A symbol's values, as a table. @param {Table} t */
  function tableOfValues(t) {
    if (t.shape === "value") return `<div class="lg-value"><code>${esc(t.name)}</code> = <b>${esc(t.cells[0][0])}</b></div>`;
    const cell = (/** @type {string} */ v) => `<td class="${v === "true" ? "v1" : v === "—" ? "none" : ""}">${esc(v)}</td>`;
    if (t.shape === "row") {
      return `<div class="lg-model-table"><div class="lg-model-name"><code>${esc(t.name)}</code></div><div class="lg-table-wrap"><table class="lg-table">` +
        `<thead><tr>${t.cols.map((c) => `<th>${esc(c)}</th>`).join("")}</tr></thead><tbody><tr>${t.cells[0].map(cell).join("")}</tr></tbody></table></div></div>`;
    }
    if (t.shape === "grid") {
      return `<div class="lg-model-table"><div class="lg-model-name"><code>${esc(t.name)}</code></div><div class="lg-table-wrap"><table class="lg-table">` +
        `<thead><tr><th></th>${t.cols.map((c) => `<th>${esc(c)}</th>`).join("")}</tr></thead>` +
        `<tbody>${t.rows.map((r, i) => `<tr><th>${esc(r)}</th>${t.cells[i].map(cell).join("")}</tr>`).join("")}</tbody></table></div></div>`;
    }
    return `<div class="lg-model-table"><div class="lg-model-name"><code>${esc(t.name)}</code></div><div class="lg-table-wrap"><table class="lg-table"><tbody>` +
      t.cells.map((line) => `<tr>${line.slice(0, -1).map((v) => `<td>${esc(v)}</td>`).join("")}<td class="sep"></td>${cell(line[line.length - 1])}</tr>`).join("") + `</tbody></table></div></div>`;
  }

  function modelsHtml() {
    const m = state.models;
    if (!m) return `<p class="lg-hint">A theory — sorts, what is named, and what is true of them — has its models found here: <kbd>⌘⇧↩</kbd>, or the search button above.</p>`;
    if (m.running) return `<div class="lg-verdict"><span class="lab-spin"></span>Searching…</div><button class="cc-quiet" data-logic-stop>Stop  ⎋</button>`;
    if (m.error) return `<div class="lg-verdict bad">${icon("d-error")}Not searched</div><p class="lg-message">${esc(m.error)}</p>`;
    const took = m.elapsedMs < 1000 ? `${m.elapsedMs} ms` : `${(m.elapsedMs / 1000).toFixed(1)} s`;
    if (m.found === 0) {
      return m.finished ? `<div class="lg-verdict bad">No model: the constraints cannot all hold.</div><p class="lg-faint">Searched in ${took}.</p>`
        : `<div class="lg-verdict bad">No model found in ${took}</div><p class="lg-faint">${m.timedOut ? "The search ran out of time." : "The search was stopped."} There may still be one.</p>`;
    }
    const k = Math.min(state.shownModel, m.models.length - 1);
    const how = m.finished ? `${m.found.toLocaleString()} in all` : m.counted ? `${m.found.toLocaleString()} found before it ${m.timedOut ? "ran out of time" : "was stopped"}` : `the first ${m.models.length === 1 ? "" : m.models.length.toLocaleString() + " "}found`;
    const verdict = m.finished && m.found === 1 ? `<div class="lg-verdict good">${icon("check")}One model: the answer is the only one</div>` : "";
    const nav = `<div class="lg-nav"><button class="cc-quiet" data-logic-model="-1" ${k === 0 ? "disabled" : ""} aria-label="The model before">‹</button>` +
      `<span>Model ${k + 1} of ${how}</span><button class="cc-quiet" data-logic-model="1" ${k >= m.models.length - 1 ? "disabled" : ""} aria-label="The next model">›</button>` +
      (m.counted || m.finished ? "" : `<span class="grow"></span><button class="cc-quiet" data-logic-count title="Search on through every model, for up to 20 s">Count them all</button>`) + `</div>`;
    const model = m.models[k];
    const sizes = model.sizes.length ? `<p class="lg-faint">Sizes chosen: ${model.sizes.map(([s, n]) => `${esc(s)} ${n}`).join(", ")}</p>` : "";
    return verdict + nav + sizes + model.tables.map(tableOfValues).join("") + `<p class="lg-faint">Found in ${took}, ${m.choices.toLocaleString()} choices.</p>`;
  }

  /* ── the answer ───────────────────────────────────────────────────── */

  /** A row's propositions as words: `p true, q false`. @param {string[]} atoms @param {boolean[]} row */
  const rowWords = (atoms, row) => atoms.map((a, i) => `<code>${esc(a)}</code> ${row[i] ? "true" : "false"}`).join(", ");

  /** The truth table, rows `mark` says marked. @param {Answer} a @param {(r: number) => boolean} [mark] */
  function tableHtml(a, mark = () => false) {
    const n = a.atoms.length;
    if (!a.columns.length) return n > 16 ? `<p class="lg-faint">${n} propositions: too many rows to show, ${Math.pow(2, n).toLocaleString()}.</p>` : "";
    const total = 1 << n, shown = Math.min(total, MOST_ROWS);
    let rows = "";
    for (let r = 0; r < shown; r++) {
      const ins = a.atoms.map((_, i) => `<td>${(r >> (n - 1 - i)) & 1}</td>`).join("");
      const outs = a.columns.map((c, k) => `<td class="v${c.values[r]}${k === a.columns.length - 1 ? " last" : ""}">${c.values[r]}</td>`).join("");
      rows += `<tr${mark(r) ? ` class="marked"` : ""}>${ins}<td class="sep"></td>${outs}</tr>`;
    }
    return `<div class="lg-table-wrap"><table class="lg-table"><thead><tr>${a.atoms.map((v) => `<th>${esc(v)}</th>`).join("")}<th class="sep"></th>` +
      `${a.columns.map((c, k) => `<th class="${k === a.columns.length - 1 ? "last" : ""}" title="${esc(c.name)}">${esc(c.name)}</th>`).join("")}</tr></thead><tbody>${rows}</tbody></table></div>` +
      (shown < total ? `<p class="lg-faint">The first ${shown} of ${total.toLocaleString()} rows.</p>` : "");
  }

  /** The forms, a Karnaugh map, and building it in Circuit. @param {Answer} a */
  function formsHtml(a) {
    const forms = a.forms.map((f, i) => `<div class="lg-form"><span class="lg-form-name">${esc(f.name)}</span>` +
      `<code class="lg-form-text">${esc(f.text)}</code><button class="lg-copy" data-logic-copy="${i}" title="Copy">${icon("files")}</button></div>`).join("");
    const more = a.more ? `<button class="cc-quiet" data-logic-more title="${a.atoms.length} propositions: a moment or more">Find the least forms</button>` : "";
    const greedy = a.forms.length && !a.exact && a.forms.some((f) => f.name.startsWith("Least")) ? `<p class="lg-faint">The least forms were chosen greedily: they may not be the least.</p>` : "";
    const n = a.atoms.length;
    const kmap = a.column && n >= 2 && n <= 6 && a.sopTerms.length + a.posTerms.length > 0 ? `<canvas class="lg-kmap" data-logic-kmap></canvas>` : "";
    const build = a.column && n >= 1 && n <= 16 ? `<div class="lg-build"><span class="lg-faint">Build in Circuit</span>` +
      `<button class="cc-quiet" data-logic-build="andOr">AND/OR</button><button class="cc-quiet" data-logic-build="nand">NAND</button><button class="cc-quiet" data-logic-build="nor">NOR</button></div>` : "";
    return (forms || more ? `<h3>Forms</h3>${forms}${more}${greedy}` : "") + (kmap ? `<h3>Karnaugh map</h3>${kmap}` : "") + build;
  }

  function tableForm() {
    return `<details class="lg-from"${state.answer?.kind === "table" ? " open" : ""}><summary>From a truth table</summary>` +
      `<form data-logic-table-form><input name="names" placeholder="Names: a b c" aria-label="Names" value="${esc(state.names)}" spellcheck="false">` +
      `<input name="column" placeholder="Column: 00010111 (x for either)" aria-label="Column" value="${esc(state.column)}" spellcheck="false"><button type="submit">Read</button></form></details>`;
  }

  function renderAnswer() {
    const a = state.answer;
    const head = (/** @type {string} */ cls, /** @type {string} */ words) => `<div class="lg-verdict ${cls}">${words}</div>`;
    let body = "";
    if (!a || a.kind === "nothing") {
      body = `<p class="lg-hint">Put the cursor on a line beginning <code>?</code> — a formula, <code>? A, B |= C</code> to ask if an argument holds, <code>? A == B</code> to ask if two formulas are the same — or on any formula or declaration, and it is answered here.</p>`;
    } else if (a.kind === "error") {
      body = head("bad", `${icon("d-error")}Line ${a.line}`) + `<p class="lg-message">${esc(a.message ?? "")}</p>`;
    } else if (a.kind === "declaration" || a.kind === "theory") {
      body = `<p class="lg-said"><code>${esc(a.text)}</code></p><p class="lg-message">${esc(a.message ?? "")}</p>` +
        `<button class="cc-quiet" data-logic-solve>Find the theory's models</button>`;
    } else if (a.kind === "formula") {
      const words = { tautology: "A tautology: true in every row", contradiction: "A contradiction: false in every row", contingent: "Contingent: true in some rows, false in others" }[a.status ?? ""] ?? "";
      const count = a.models != null ? ` · true in ${a.models.toLocaleString()} of ${Math.pow(2, a.atoms.length).toLocaleString()}` : "";
      body = `<p class="lg-said"><code>${esc(a.text)}</code></p>` + head(a.status === "contingent" ? "" : a.status === "tautology" ? "good" : "bad", `${esc(words)}${count}`) +
        (a.status === "contingent" && a.model && a.falsifier ? `<p class="lg-rows">True when ${rowWords(a.atoms, a.model)}; false when ${rowWords(a.atoms, a.falsifier)}.</p>` : "") +
        tableHtml(a) + formsHtml(a);
    } else if (a.kind === "argument") {
      const n = a.atoms.length, cols = a.columns;
      // A counterexample row: every premise true and the conclusion false.
      const counter = (/** @type {number} */ r) => cols.length > 0 && cols.slice(0, -1).every((c) => c.values[r] === "1") && cols[cols.length - 1].values[r] === "0";
      body = `<p class="lg-said"><code>${esc(a.text)}</code></p>` +
        (a.valid ? head("good", `${icon("check")}Valid: the conclusion is true wherever the premises are`)
          : head("bad", `Not valid`) + (a.countermodel ? `<p class="lg-rows">A counterexample: ${rowWords(a.atoms, a.countermodel)} — the premises true, the conclusion false.</p>` : "")) +
        tableHtml(a, counter) + (n > 16 ? "" : "");
    } else if (a.kind === "same") {
      const differ = (/** @type {number} */ r) => a.columns.length === 2 && a.columns[0].values[r] !== a.columns[1].values[r];
      body = `<p class="lg-said"><code>${esc(a.text)}</code></p>` +
        (a.same ? head("good", `${icon("check")}Equivalent: the same in every row`)
          : head("bad", "Not equivalent") + (a.differ ? `<p class="lg-rows">They differ when ${rowWords(a.atoms, a.differ)}.</p>` : "")) +
        tableHtml(a, differ);
    } else if (a.kind === "table") {
      body = `<p class="lg-said">The column <code>${esc(a.text)}</code> over ${a.atoms.map((x) => `<code>${esc(x)}</code>`).join(" ")}</p>` + formsHtml(a);
    }
    const tabs = `<nav class="lg-tabs" role="tablist"><button role="tab" data-logic-view="line" aria-selected="${state.view === "line"}">This line</button>` +
      `<button role="tab" data-logic-view="models" aria-selected="${state.view === "models"}">Models${state.models && !state.models.running && !state.models.error ? ` <span class="lg-count">${state.models.found.toLocaleString()}</span>` : ""}</button></nav>`;
    if (state.view === "models") {
      els.answer.innerHTML = tabs + modelsHtml();
      return;
    }
    els.answer.innerHTML = tabs + body + tableForm();
    const canvas = /** @type {HTMLCanvasElement | null} */ (els.answer.querySelector("[data-logic-kmap]"));
    if (canvas && a?.column) {
      const css = getComputedStyle(ws);
      const get = (/** @type {string} */ v, /** @type {string} */ d) => css.getPropertyValue(v).trim() || d;
      drawKmap(canvas, a.atoms, a.column, a.sopTerms, { muted: get("--muted", "#888"), text: get("--text", "#ddd"), line: get("--xl-line", "#333") });
    }
  }

  /* ── clicks ───────────────────────────────────────────────────────── */

  ws.addEventListener("click", (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    const open = /** @type {HTMLElement | null} */ (t.closest("[data-logic-open]"));
    if (open) return void host.dispatch({ type: "logicOpen", path: open.dataset.logicOpen });
    const made = /** @type {HTMLElement | null} */ (t.closest("[data-logic-new]"));
    if (made) {
      const name = window.prompt?.(made.dataset.logicNew === "theory" ? "The new theory's name" : "The new worksheet's name");
      if (name) host.dispatch({ type: "logicNew", name, kind: made.dataset.logicNew });
      return;
    }
    const sym = /** @type {HTMLElement | null} */ (t.closest("[data-logic-sym]"));
    if (sym) return void typeIn(sym.dataset.logicSym ?? "");
    if (t.closest("[data-logic-ask]")) {
      send();
      return void ask(true);
    }
    if (t.closest("[data-logic-save]")) {
      send();
      return void host.dispatch({ type: "logicSave" });
    }
    if (t.closest("[data-logic-close]")) {
      send();
      const f = state.file;
      if (f?.dirty && !window.confirm?.(`${f.name} has unsaved changes. Close it and lose them?`)) return;
      return void host.dispatch({ type: "logicClose", discard: true });
    }
    if (t.closest("[data-logic-more]")) return void ask(true, true);
    if (t.closest("[data-logic-solve]")) return void solve(false);
    if (t.closest("[data-logic-count]")) return void solve(true);
    if (t.closest("[data-logic-stop]")) return void host.dispatch({ type: "logicStop" });
    const view = /** @type {HTMLElement | null} */ (t.closest("[data-logic-view]"));
    if (view) {
      state.view = view.dataset.logicView === "models" ? "models" : "line";
      return void renderAnswer();
    }
    const step = /** @type {HTMLElement | null} */ (t.closest("[data-logic-model]"));
    if (step && state.models) {
      state.shownModel = Math.max(0, Math.min(state.models.models.length - 1, state.shownModel + Number(step.dataset.logicModel)));
      return void renderAnswer();
    }
    const goto = /** @type {HTMLElement | null} */ (t.closest("[data-logic-goto]"));
    if (goto) {
      const line = Number(goto.dataset.logicGoto);
      const lines = els.text.value.split("\n");
      const at = lines.slice(0, line - 1).reduce((n, l) => n + l.length + 1, 0);
      els.text.focus();
      els.text.setSelectionRange(at, at + (lines[line - 1]?.length ?? 0));
      return void ask(true);
    }
    const copy = /** @type {HTMLElement | null} */ (t.closest("[data-logic-copy]"));
    if (copy) {
      const f = state.answer?.forms[Number(copy.dataset.logicCopy)];
      if (f) void navigator.clipboard?.writeText?.(f.text).catch(() => {});
      return;
    }
    const build = /** @type {HTMLElement | null} */ (t.closest("[data-logic-build]"));
    if (build && state.answer?.column) {
      const a = state.answer;
      const circuit = a.kind === "table" ? "table" : `${state.file?.name ?? "logic"} line ${a.line}`;
      host.dispatch({ type: "setMode", mode: "circuit" });
      host.dispatch({ type: "circuitFromTable", project: "logic", circuit, inputs: a.atoms, column: a.column, style: build.dataset.logicBuild });
    }
  });

  els.form.addEventListener("submit", (e) => {
    e.preventDefault();
    const name = /** @type {HTMLInputElement} */ (els.form.querySelector("input")).value.trim();
    const kind = /** @type {HTMLSelectElement} */ (els.form.querySelector("select")).value;
    if (name) host.dispatch({ type: "logicNew", name, kind });
  });

  els.answer.addEventListener("submit", (e) => {
    const form = /** @type {HTMLFormElement} */ (e.target);
    if (!form.matches("[data-logic-table-form]")) return;
    e.preventDefault();
    state.names = /** @type {HTMLInputElement} */ (form.elements.namedItem("names")).value;
    state.column = /** @type {HTMLInputElement} */ (form.elements.namedItem("column")).value;
    host.dispatch({ type: "logicFromTable", names: state.names, column: state.column });
  });

  // ⌘S where the focus is not the text: the mode's own save.
  ws.addEventListener("keydown", (e) => {
    if (e.target !== els.text && state.file && e.metaKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      send();
      host.dispatch({ type: "logicSave" });
    }
  });

  renderAnswer();

  return {
    /** @param {{files: Found[]}} e */
    home(e) {
      state.files = e.files;
      renderList();
    },
    /** @param {{file: FileView | null}} e */
    file(e) {
      const was = state.file?.generation;
      state.file = e.file;
      renderFile();
      // A file just opened: its first question, or its first line, answered.
      if (e.file && was !== e.file.generation) {
        const at = e.file.text.split("\n").findIndex((l) => l.trimStart().startsWith("?"));
        const start = at < 0 ? 0 : e.file.text.split("\n").slice(0, at).reduce((n, l) => n + l.length + 1, 0);
        els.text.setSelectionRange(start, start);
        ask(true);
      }
    },
    /** @param {{models: Models | null}} e */
    models(e) {
      if (e.models && state.file && e.models.generation !== state.file.generation) return;
      state.models = e.models;
      if (!e.models && state.view === "models") state.view = "line";
      renderAnswer();
    },
    /** @param {{answer: Answer | null}} e */
    answer(e) {
      // An answer for another file, or an older one, is not shown.
      if (e.answer && state.file && e.answer.generation !== state.file.generation && e.answer.kind !== "table") return;
      state.answer = e.answer;
      renderAnswer();
      if (state.file) paint();
    },
    shown() {
      if (state.file) requestAnimationFrame(follow);
    },
    debug() {
      return { file: state.file, answer: state.answer, asked: state.asked, text: els.text.value, models: state.models, view: state.view, shownModel: state.shownModel };
    },
  };
}

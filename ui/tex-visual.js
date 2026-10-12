/* LaTeX's visual editor: the file being edited drawn as a document — its
 * headings, paragraphs, equations, lists and figures — over Codex's editor,
 * the way the document viewer covers it.
 *
 * The core cuts the source into blocks (Event::TexBlocks), each with its
 * span. A paragraph or a heading's title is edited in place: what is typed
 * is turned back into LaTeX and sent as the block's new source, which the
 * core applies as the smallest change, one step to undo. Commands this does
 * not draw are locked chips that go back exactly as they came, and a block
 * that would not come back exactly is drawn read-only — the visual editor
 * never rewrites what it was not asked to. An equation is edited as its
 * source, drawn by KaTeX when it is put down. */

import { readDocument } from "./bridge.js";
import { escapeHtml as esc } from "./html.js";

/** @typedef {import("./bindings/TexBlock").TexBlock} Block */

/** The commands drawn as marks, by the element each is. */
const MARKS = /** @type {Record<string, string>} */ ({ textbf: "b", emph: "i", textit: "i", texttt: "code", underline: "u" });
/** The command each mark's element is written back as, unless it says. */
const COMMAND = /** @type {Record<string, string>} */ ({ B: "textbf", STRONG: "textbf", I: "emph", EM: "emph", CODE: "texttt", U: "underline" });
/** What `\x` escapes. */
const ESCAPED = "%&#_${}";
/** Commands a chip names by what they point at. */
const POINTING = ["cite", "citep", "citet", "parencite", "textcite", "autocite", "ref", "eqref", "cref", "Cref", "autoref", "pageref", "label", "url", "footnote", "href"];

/* ── inline LaTeX ⇄ the DOM ──────────────────────────────────────────── */

/** A locked chip: drawn as `label`, written back as `src`.
 *  @param {string} src @param {string} label @param {string} [cls] */
const chip = (src, label, cls = "tex-chip") =>
  `<span class="${cls}" contenteditable="false" data-src="${esc(src)}" title="${esc(src.trim())}">${label}</span>`;

/** The groups — `[…]` and `{…}` — that follow a command at `i`, and where
 *  they end. @param {string} src @param {number} i */
function groups(src, i) {
  const args = [];
  while (src[i] === "[" || src[i] === "{") {
    const open = src[i];
    const close = open === "[" ? "]" : "}";
    let depth = 0;
    let j = i;
    for (; j < src.length; j++) {
      if (src[j] === "\\") { j++; continue; }
      if (src[j] === open) depth++;
      else if (src[j] === close && --depth === 0) break;
    }
    if (j >= src.length) return null;
    args.push(src.slice(i + 1, j));
    i = j + 1;
  }
  return { args, end: i };
}

/** Inline LaTeX as HTML for a contenteditable, from `i` — to a closing
 *  brace when `inGroup` — or null if it cannot be drawn faithfully.
 *  @param {string} src @param {number} i @param {boolean} inGroup @param {(tex: string, display: boolean) => string} math
 *  @returns {{html: string, end: number} | null} */
function inline(src, i, inGroup, math) {
  let html = "";
  while (i < src.length) {
    const c = src[i];
    if (c === "}") {
      return inGroup ? { html, end: i } : null;
    }
    if (c === "\\") {
      const next = src[i + 1] ?? "";
      if (ESCAPED.includes(next)) {
        html += esc(next);
        i += 2;
        continue;
      }
      if (next === "(") {
        const close = src.indexOf("\\)", i + 2);
        if (close < 0) return null;
        html += chip(src.slice(i, close + 2), math(src.slice(i + 2, close), false), "tex-chip tex-math");
        i = close + 2;
        continue;
      }
      if (/[A-Za-z]/.test(next)) {
        const name = /^[A-Za-z]+/.exec(src.slice(i + 1))?.[0] ?? "";
        let at = i + 1 + name.length;
        const mark = MARKS[name];
        if (mark && src[at] === "{") {
          const body = inline(src, at + 1, true, math);
          if (!body) return null;
          html += `<${mark} data-cmd="${name}">${body.html}</${mark}>`;
          i = body.end + 1;
          continue;
        }
        if (src[at] === "*") at++;
        const found = groups(src, at);
        if (!found) return null;
        const whole = src.slice(i, found.end);
        const pointing = POINTING.includes(name) && found.args.length;
        const label = pointing ? `${esc(name)} <b>${esc(found.args[found.args.length - 1])}</b>` : esc(whole.length > 24 ? `\\${name}…` : whole);
        html += chip(whole, label);
        i = found.end;
        continue;
      }
      // `\\`, `\,`, `\'e` and their kind: kept as they are.
      const found = groups(src, i + 2);
      const end = found ? found.end : i + 2;
      html += chip(src.slice(i, end), esc(src.slice(i, end)));
      i = end;
      continue;
    }
    if (c === "$") {
      const display = src[i + 1] === "$";
      const fence = display ? "$$" : "$";
      const close = src.indexOf(fence, i + fence.length);
      if (close < 0) return null;
      html += chip(src.slice(i, close + fence.length), math(src.slice(i + fence.length, close), display), "tex-chip tex-math");
      i = close + fence.length;
      continue;
    }
    if (c === "%") {
      const close = src.indexOf("\n", i);
      const end = close < 0 ? src.length : close + 1;
      html += chip(src.slice(i, end), "%", "tex-chip tex-comment");
      i = end;
      continue;
    }
    if (c === "~") {
      html += chip("~", "&nbsp;", "tex-tie");
      i++;
      continue;
    }
    if (c === "{") {
      const found = groups(src, i);
      if (!found) return null;
      html += chip(src.slice(i, found.end), esc(src.slice(i, found.end)));
      i = found.end;
      continue;
    }
    // Text: newlines kept as they are, and drawn as the spaces TeX makes of them.
    let j = i;
    while (j < src.length && !"\\$%~{}".includes(src[j])) j++;
    html += esc(src.slice(i, j));
    i = j;
  }
  return inGroup ? null : { html, end: i };
}

/** A paragraph's LaTeX as HTML, or null if it cannot be drawn faithfully.
 *  @param {string} src @param {(tex: string, display: boolean) => string} math */
export function toHtml(src, math) {
  return inline(src, 0, false, math)?.html ?? null;
}

/** Text as LaTeX writes it. @param {string} text */
function escapeText(text) {
  let out = "";
  for (const c of text) {
    if (ESCAPED.includes(c)) out += `\\${c}`;
    else if (c === "\\") out += "\\textbackslash{}";
    else if (c === "^") out += "\\textasciicircum{}";
    else if (c === "~") out += "\\textasciitilde{}";
    else if (c === " ") out += " ";
    else out += c;
  }
  return out;
}

/** What is drawn, written back as LaTeX.
 *  @param {Node} node @returns {string} */
export function toLatex(node) {
  let out = "";
  for (const child of node.childNodes) {
    if (child.nodeType === Node.TEXT_NODE) {
      out += escapeText(child.textContent ?? "");
      continue;
    }
    if (child.nodeType !== Node.ELEMENT_NODE) continue;
    const el = /** @type {HTMLElement} */ (child);
    if (el.dataset.src !== undefined) {
      out += el.dataset.src;
      continue;
    }
    const command = el.dataset.cmd ?? COMMAND[el.tagName];
    if (command) {
      out += `\\${command}{${toLatex(el)}}`;
    } else if (el.tagName === "BR") {
      // The browser's stand-in for an empty line: nothing of the source's.
    } else if (el.tagName === "DIV" || el.tagName === "P") {
      out += `${toLatex(el)}\n`;
    } else {
      out += toLatex(el);
    }
  }
  return out;
}

/** The LaTeX before the caret in `el`, to say where it is in the source:
 *  a mark put at the caret, the element written out, and the mark taken
 *  away again — a copy of the part before the caret would close the marks
 *  it is inside, and count their braces.
 *  @param {HTMLElement} el */
function before(el) {
  const doc = el.ownerDocument;
  const selection = doc.getSelection();
  if (!selection?.rangeCount || !el.contains(selection.anchorNode)) return null;
  const node = /** @type {Node} */ (selection.anchorNode);
  const offset = selection.anchorOffset;
  const range = selection.getRangeAt(0).cloneRange();
  range.collapse(true);
  const mark = doc.createTextNode("\u0001");
  range.insertNode(mark);
  const latex = toLatex(el);
  const parent = mark.parentNode;
  mark.remove();
  parent?.normalize();
  selection.collapse(node, Math.min(offset, node.nodeType === Node.TEXT_NODE ? (node.textContent ?? "").length : node.childNodes.length));
  const at = latex.indexOf("\u0001");
  return at < 0 ? null : latex.slice(0, at);
}

/* ── headings ─────────────────────────────────────────────────────────── */

const HEADING = /^(\s*\\(?:part|chapter|section|subsection|subsubsection|paragraph|subparagraph)\*?(?:\[[^\]]*\])?\{)/;

/** A heading's source in three: the command up to its `{`, the title, and
 *  the rest from its `}` — or null. @param {string} src @param {(tex: string, display: boolean) => string} math */
function headingParts(src, math) {
  const head = HEADING.exec(src)?.[1];
  if (!head) return null;
  const title = inline(src, head.length, true, math);
  if (!title) return null;
  return { head, html: title.html, tail: src.slice(title.end) };
}

/* ── mathematics ──────────────────────────────────────────────────────── */

/** @type {any} */
let katex = null;
/** @type {Promise<void> | null} */
let loading = null;

/** KaTeX, vendored, loaded once. */
function loadKatex() {
  if (loading) return loading;
  if (!document.querySelector('link[href*="katex.min.css"]')) {
    const sheet = document.createElement("link");
    sheet.rel = "stylesheet";
    sheet.href = new URL("./vendor/katex/katex.min.css", import.meta.url).href;
    document.head.append(sheet);
  }
  const path = "./vendor/katex/katex.mjs";
  loading = import(/* @vite-ignore */ path).then((m) => { katex = m.default; }).catch(() => {});
  return loading;
}

/** An equation's body, from its environment or fences, for KaTeX: labels
 *  and numbering commands, which it does not draw, taken out.
 *  @param {string} src */
export function mathBody(src) {
  let body = src.trim();
  const env = /^\\begin\{([^}]+)\}([\s\S]*)\\end\{\1\}$/.exec(body);
  if (env) {
    const name = env[1];
    body = ["equation", "equation*", "displaymath"].includes(name) ? env[2] : `\\begin{${name.endsWith("*") ? name : `${name}*`}}${env[2]}\\end{${name.endsWith("*") ? name : `${name}*`}}`;
  } else if (body.startsWith("\\[") && body.endsWith("\\]")) {
    body = body.slice(2, -2);
  } else if (body.startsWith("$$") && body.endsWith("$$")) {
    body = body.slice(2, -2);
  }
  return body.replace(/\\label\{[^}]*\}/g, "").replace(/\\(?:nonumber|notag)\b/g, "").trim();
}

/* ── the editor ───────────────────────────────────────────────────────── */

/** How long typing rests before it is sent. */
const REST_MS = 700;

/** @param {{root: Document, dispatch: (command: any) => unknown}} host */
export function createTexVisual(host) {
  const found = /** @type {HTMLElement | null} */ (host.root.querySelector("#tex-visual"));
  const switcher = /** @type {HTMLElement | null} */ (host.root.querySelector("[data-tex-switch]"));
  if (!found || !switcher) return null;
  const box = found;
  const toggles = switcher;
  const page = /** @type {HTMLElement} */ (box.querySelector("[data-tex-visual-page]"));
  const name = /** @type {HTMLElement} */ (box.querySelector("[data-tex-visual-name]"));
  /** @type {{on: boolean, path: string, version: number, blocks: Block[], macros: Record<string, string>, images: Map<string, string>,
   *   sending: boolean, caretAt: number | null}} */
  const state = { on: false, path: "", version: 0, blocks: [], macros: {}, images: new Map(), sending: false, caretAt: null };
  /** Pictures read, by path, as data: URLs — the window's CSP lets no blob: image through. @type {Map<string, string>} */
  const pictures = new Map();
  /** @type {ReturnType<typeof setTimeout> | undefined} */
  let resting;
  let composing = false;

  /** An equation drawn, or its source until KaTeX is in. @param {string} tex @param {boolean} display */
  function math(tex, display) {
    if (!katex) {
      void loadKatex().then(() => { if (state.on) draw(); });
      return `<span class="tex-v-pending">${esc(tex)}</span>`;
    }
    try {
      return katex.renderToString(tex, { displayMode: display, throwOnError: false, strict: "ignore", trust: false, macros: { ...state.macros } });
    } catch {
      return `<span class="katex-error">${esc(tex)}</span>`;
    }
  }

  /** A block's element. @param {Block} block @param {number} i */
  function blockHtml(block, i) {
    const at = `data-block="${i}"`;
    switch (block.kind) {
      case "space":
      case "end":
        return `<div class="tex-v-space" ${at} hidden></div>`;
      case "preamble":
        return `<details class="tex-v-preamble" ${at}><summary>Preamble · ${block.text.split("\n").length - 1} lines</summary><pre>${esc(block.text)}</pre></details>`;
      case "heading": {
        const parts = headingParts(block.text, math);
        if (!parts) break;
        const level = Math.min(6, Math.max(1, (block.level ?? 2) - 1));
        return `<h${level} class="tex-v-heading" ${at}><span class="tex-v-text" contenteditable="true" spellcheck="true">${parts.html}</span></h${level}>`;
      }
      case "paragraph": {
        const html = toHtml(block.text, math);
        if (html === null) break;
        return `<p class="tex-v-para" ${at}><span class="tex-v-text" contenteditable="true" spellcheck="true">${html}</span></p>`;
      }
      case "math":
        return `<div class="tex-v-math" ${at} tabindex="0" title="Click to edit its source">${math(mathBody(block.text), true)}</div>`;
      case "list": {
        const items = block.text.split(/\\item\b/).slice(1).map((item) => item.replace(/\\end\{(itemize|enumerate|description)\}\s*$/, "").replace(/\\(begin|end)\{(itemize|enumerate|description)\}/g, ""));
        const tag = block.text.trimStart().startsWith("\\begin{enumerate}") ? "ol" : "ul";
        return `<${tag} class="tex-v-list" ${at} title="Double-click to edit in the source">${items.map((item) => `<li>${toHtml(item.trim(), math) ?? esc(item.trim())}</li>`).join("")}</${tag}>`;
      }
      case "figure": {
        const caption = /\\caption\{([^}]*)\}/.exec(block.text)?.[1] ?? "";
        const image = block.image ? state.images.get(block.image) : undefined;
        const picture = image && pictures.get(image);
        const shown = picture ? `<img alt="${esc(block.image ?? "")}" src="${picture}">` : `<span class="tex-v-missing">${esc(block.image ?? "a figure")}</span>`;
        return `<figure class="tex-v-figure" ${at} title="Double-click to edit in the source">${shown}<figcaption>${toHtml(caption, math) ?? esc(caption)}</figcaption></figure>`;
      }
      default:
        break;
    }
    return `<pre class="tex-v-raw" ${at} title="Double-click to edit in the source">${esc(block.text.replace(/\n$/, ""))}</pre>`;
  }

  /** The file drawn, and each editable block checked to come back as it came. */
  function draw() {
    name.textContent = state.path.split("/").pop() ?? "";
    if (!/\.tex$/i.test(state.path)) {
      page.innerHTML = `<p class="tex-v-note">The visual editor draws .tex files. Open one from the tree, or go back to the source.</p>`;
      return;
    }
    page.innerHTML = state.blocks.map(blockHtml).join("");
    faithful(null);
    for (const [, path] of state.images) void picture(path);
    if (state.caretAt !== null) {
      const i = state.blocks.findIndex((b) => b.start >= /** @type {number} */ (state.caretAt) && (b.kind === "paragraph" || b.kind === "heading"));
      state.caretAt = null;
      const text = /** @type {HTMLElement | null} */ (page.querySelector(`[data-block="${i}"] .tex-v-text`));
      text?.focus();
    }
  }

  /** Every editable block checked to write back exactly what it came
   *  from — but the one being typed in — and any that would not shown as
   *  its source instead, not edited here. @param {HTMLElement | null} typing */
  function faithful(typing) {
    for (const text of page.querySelectorAll(".tex-v-text")) {
      const el = /** @type {HTMLElement} */ (text);
      if (el === typing) continue;
      const i = Number(/** @type {HTMLElement} */ (el.parentElement).dataset.block);
      const block = state.blocks[i];
      if (block && written(el, block) !== block.text) {
        /** @type {HTMLElement} */ (el.parentElement).outerHTML = `<pre class="tex-v-raw" data-block="${i}" title="Double-click to edit in the source">${esc(block.text.replace(/\n$/, ""))}</pre>`;
      }
    }
  }

  /** A block's source as its element now says. @param {HTMLElement} el @param {Block} block */
  function written(el, block) {
    if (block.kind === "heading") {
      const parts = headingParts(block.text, math);
      return parts ? parts.head + toLatex(el) + parts.tail : block.text;
    }
    return toLatex(el);
  }

  /** An image read and kept as a data: URL, and the figure drawn. @param {string} path */
  async function picture(path) {
    if (pictures.has(path) || !/\.(png|jpe?g)$/i.test(path)) return;
    pictures.set(path, "");
    try {
      const bytes = await readDocument(path);
      let binary = "";
      for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
      pictures.set(path, `data:image/${/\.png$/i.test(path) ? "png" : "jpeg"};base64,${btoa(binary)}`);
      for (const figure of page.querySelectorAll(".tex-v-figure")) {
        const block = state.blocks[Number(/** @type {HTMLElement} */ (figure).dataset.block)];
        if (block?.image && state.images.get(block.image) === path) figure.outerHTML = blockHtml(block, state.blocks.indexOf(block));
      }
    } catch {
      // Left as its name.
    }
  }

  /** The block an element is in. @param {Node | null} node */
  function blockOf(node) {
    const el = /** @type {HTMLElement | null} */ (node instanceof HTMLElement ? node : node?.parentElement ?? null)?.closest("[data-block]");
    if (!el) return null;
    const block = state.blocks[Number(/** @type {HTMLElement} */ (el).dataset.block)];
    return block ? { el: /** @type {HTMLElement} */ (el), block } : null;
  }

  /** Send what was typed in a block, if it changed anything. @param {HTMLElement} text */
  function send(text) {
    clearTimeout(resting);
    const found = blockOf(text);
    if (!found || composing || state.sending) return;
    const latex = written(text, found.block);
    if (latex === found.block.text) return;
    state.sending = true;
    host.dispatch({ type: "texVisualEdit", version: state.version, start: found.block.start, end: found.block.end, old: found.block.text, latex });
  }

  /** The editable text with the keyboard, if any. */
  const focused = () => /** @type {HTMLElement | null} */ (host.root.activeElement?.closest?.(".tex-v-text") ?? null);

  /** Send what is waiting, now. */
  function flush() {
    const text = focused();
    if (text) send(text);
  }

  page.addEventListener("input", (e) => {
    const text = /** @type {HTMLElement} */ (e.target).closest(".tex-v-text");
    if (!text) return;
    clearTimeout(resting);
    resting = setTimeout(() => send(/** @type {HTMLElement} */ (text)), REST_MS);
  });
  page.addEventListener("compositionstart", () => { composing = true; });
  page.addEventListener("compositionend", (e) => {
    composing = false;
    const text = /** @type {HTMLElement} */ (e.target).closest(".tex-v-text");
    if (text) resting = setTimeout(() => send(/** @type {HTMLElement} */ (text)), REST_MS);
  });
  page.addEventListener("focusout", (e) => {
    const text = /** @type {HTMLElement} */ (e.target).closest?.(".tex-v-text");
    if (text) send(/** @type {HTMLElement} */ (text));
  });
  // Only plain text comes in: what another program put on the clipboard is
  // not this document's markup.
  page.addEventListener("paste", (e) => {
    if (!/** @type {HTMLElement} */ (e.target).closest?.(".tex-v-text")) return;
    e.preventDefault();
    host.root.execCommand("insertText", false, e.clipboardData?.getData("text/plain") ?? "");
  });

  /** Back to the source, at `at`, or where the caret is. @param {number | null} [at] */
  function source(at = null) {
    const text = focused();
    let where = at;
    if (where === null && text) {
      const found = blockOf(text);
      const head = found?.block.kind === "heading" ? headingParts(found.block.text, math)?.head.length ?? 0 : 0;
      const typed = before(text);
      if (found && typed !== null) where = found.block.start + head + [...typed].length;
    }
    flush();
    toggle(false, where);
  }

  page.addEventListener("keydown", (e) => {
    const text = /** @type {HTMLElement} */ (e.target).closest?.(".tex-v-text");
    const mod = e.metaKey || e.ctrlKey;
    if (e.key === "Escape") {
      e.preventDefault();
      return source();
    }
    if (mod && (e.key === "z" || e.key === "Z")) {
      e.preventDefault();
      flush();
      host.dispatch({ type: "texVisualUndo", redo: e.shiftKey });
      return;
    }
    if (!text) return;
    if (mod && (e.key === "b" || e.key === "i")) {
      e.preventDefault();
      host.root.execCommand(e.key === "b" ? "bold" : "italic");
      return;
    }
    if (e.key === "Enter") {
      // A new paragraph: a blank line where the caret is, and the caret
      // after it once the core has cut the blocks again.
      e.preventDefault();
      const found = blockOf(text);
      const typed = before(/** @type {HTMLElement} */ (text));
      if (found && typed !== null && found.block.kind === "paragraph") {
        host.root.execCommand("insertText", false, "\n\n");
        state.caretAt = found.block.start + [...typed].length + 2;
        send(/** @type {HTMLElement} */ (text));
      }
    }
  });

  // An equation is edited as its source, and drawn again when put down.
  page.addEventListener("click", (e) => {
    const shown = /** @type {HTMLElement} */ (e.target).closest(".tex-v-math");
    if (!shown || shown.querySelector("textarea")) return;
    const block = state.blocks[Number(/** @type {HTMLElement} */ (shown).dataset.block)];
    const area = host.root.createElement("textarea");
    area.className = "tex-v-source";
    area.value = block.text.replace(/\n$/, "");
    area.rows = Math.max(2, area.value.split("\n").length);
    area.spellcheck = false;
    shown.replaceChildren(area);
    area.focus();
    const done = (/** @type {boolean} */ keep) => {
      const latex = area.value + (block.text.endsWith("\n") ? "\n" : "");
      if (keep && latex !== block.text && !state.sending) {
        state.sending = true;
        host.dispatch({ type: "texVisualEdit", version: state.version, start: block.start, end: block.end, old: block.text, latex });
      } else {
        shown.outerHTML = blockHtml(block, state.blocks.indexOf(block));
      }
    };
    area.addEventListener("keydown", (k) => {
      if (k.key === "Escape") { k.preventDefault(); k.stopPropagation(); done(false); }
      if (k.key === "Enter" && (k.metaKey || k.ctrlKey)) { k.preventDefault(); done(true); }
    });
    area.addEventListener("blur", () => done(true), { once: true });
  });

  // What is not edited here is a double-click from its source.
  page.addEventListener("dblclick", (e) => {
    const found = blockOf(/** @type {Node} */ (e.target));
    if (found && !/** @type {HTMLElement} */ (e.target).closest(".tex-v-text, .tex-v-math")) source(found.block.start);
  });

  /** Visual or source. @param {boolean} on @param {number | null} [at] */
  function toggle(on, at = null) {
    state.on = on;
    box.hidden = !on;
    for (const b of toggles.querySelectorAll("[data-tex-view]")) {
      const mine = /** @type {HTMLElement} */ (b).dataset.texView === (on ? "visual" : "source");
      b.classList.toggle("on", mine);
      b.setAttribute("aria-pressed", String(mine));
    }
    host.dispatch(on ? { type: "texVisual", on: true } : { type: "texVisual", on: false, at });
    if (on) void loadKatex();
  }

  toggles.addEventListener("click", (e) => {
    const view = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest("[data-tex-view]"))?.dataset.texView;
    if (view === "visual" && !state.on) toggle(true);
    if (view === "source" && state.on) source();
  });

  /** The file being edited, as blocks. A block being typed in that says
   *  what it already shows is left alone, caret and all.
   *  @param {{path: string, version: number, blocks: Block[], macros: [string, string][], images: [string, string][]}} event */
  function blocks(event) {
    state.sending = false;
    const same = event.path === state.path && event.blocks.length === state.blocks.length;
    state.path = event.path;
    state.version = Number(event.version);
    state.macros = Object.fromEntries(event.macros);
    state.images = new Map(event.images);
    const text = focused();
    const typing = text ? blockOf(text) : null;
    const previous = state.blocks;
    state.blocks = event.blocks;
    if (!state.on) return;
    if (!same || state.caretAt !== null) return draw();
    // A block is drawn again unless it shows what the core says it holds —
    // the one being typed in included, so an edit refused is put right.
    event.blocks.forEach((block, i) => {
      const el = page.querySelector(`[data-block="${i}"]`);
      const editable = /** @type {HTMLElement | null} */ (el?.querySelector(".tex-v-text") ?? null);
      const shows = editable ? written(editable, block) === block.text : block.text === previous[i].text;
      if (shows && block.kind === previous[i].kind) return;
      if (el) el.outerHTML = blockHtml(block, i);
    });
    faithful(typing ? /** @type {HTMLElement} */ (text) : null);
  }

  return { blocks, toggle, isOn: () => state.on };
}

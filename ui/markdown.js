/* Markdown, as Claude writes it: what a reply in the Chat tab is drawn from.
 *
 * Escaped first and always. The text is the model's, and the page it is drawn
 * into can reach the editor — so nothing in it is ever HTML. Every construct
 * below is recognised in the escaped text and turned into markup this file
 * writes itself; a `<script>` in a reply stays the four characters it was.
 *
 * What it knows is what replies use: fenced and indented code, headings,
 * lists (nested, ordered, task lists), quotes, tables, rules, emphasis,
 * strikethrough, inline code, links and TeX. Links are not followed in the
 * page — a click asks the shell to open them in the browser — and images are
 * links, because the editor's policy loads none from the web. Fenced code is
 * coloured by ./highlight.js; equations are drawn by KaTeX. */

import { escapeHtml } from "./html.js";
import { highlight } from "./highlight.js";

/** @param {string} text */
const esc = (text) => escapeHtml(text);

/** How a text is drawn: whether its end is still arriving, and whether a
 *  `[[note § heading]]` in it is a citation to open.
 *  @typedef {{streaming?: boolean, cite?: boolean}} Options */

/* ── math ─────────────────────────────────────────────────────────────
   TeX as models write it: `$…$` and `\(…\)` inside a line; `$$…$$`, `\[…\]`,
   a ```math fence, or a bare \begin{align}…\end{align} on lines of their own.
   KaTeX draws it. It is vendored, and loaded the first time a text has an
   equation in it, so a session that never sees one never pays for it. Until
   it has loaded an equation shows as its source, dimmed; then every one on
   the page is drawn where it stands. `trust` stays off, so `\href`,
   `\includegraphics` and the `\html…` commands draw as errors, not as links,
   images or markup. */

/** @type {typeof import("./vendor/katex/katex.mjs").default | null} */
let katex = null;
let loading = false;
/** Equations already drawn, by display and source: a reply is drawn again
 *  for every word that arrives, and its equations with it.
 *  @type {Map<string, string>} */
const drawn = new Map();

function loadKatex() {
  if (loading || typeof document === "undefined") return;
  loading = true;
  const sheet = document.createElement("link");
  sheet.rel = "stylesheet";
  sheet.href = new URL("./vendor/katex/katex.min.css", import.meta.url).href;
  // The stylesheet first: without it KaTeX's markup shows every equation
  // twice, once drawn and once as the MathML a screen reader is given.
  const styled = new Promise((done) => {
    sheet.onload = sheet.onerror = () => done(undefined);
  });
  document.head.append(sheet);
  Promise.all([import("./vendor/katex/katex.mjs"), styled])
    .then(([module]) => {
      katex = module.default;
      for (const waiting of document.querySelectorAll(".md-math[data-tex]")) {
        const el = /** @type {HTMLElement} */ (waiting);
        el.outerHTML = math(el.dataset.tex ?? "", el.hasAttribute("data-display"));
      }
    })
    // Without KaTeX an equation stays its source, which is still the answer.
    .catch(() => {});
}

/** An equation, drawn — or its source, to be drawn when KaTeX has loaded.
 *  @param {string} tex @param {boolean} display */
function math(tex, display) {
  const source = tex.trim();
  const cls = `md-math${display ? " md-display" : ""}`;
  if (!katex) {
    loadKatex();
    return `<span class="${cls}" data-tex="${esc(source)}"${display ? " data-display" : ""}>${esc(source)}</span>`;
  }
  const key = `${display ? "D" : "I"}${source}`;
  let html = drawn.get(key);
  if (html === undefined) {
    try {
      html = katex.renderToString(source, {
        displayMode: display, throwOnError: false, strict: "ignore", trust: false, output: "htmlAndMathml",
      });
    } catch {
      html = `<span class="katex-error" title="Not TeX KaTeX can draw">${esc(source)}</span>`;
    }
    if (drawn.size > 400) drawn.clear();
    drawn.set(key, html);
  }
  return `<span class="${cls}">${html}</span>`;
}

/** An equation on lines of its own. @param {string} tex */
const mathBlock = (tex) => `<div class="md-math-block">${math(tex, true)}</div>`;

/** The source of an equation found in escaped text, as it was written.
 *  @param {string} html */
const unescapeHtml = (html) =>
  html.replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&amp;/g, "&");

/* `$…$` the way Pandoc reads it, so prices are not equations: the opening
   `$` is followed by something that is not a space, the closing one follows
   something that is not a space and is not followed by a digit — "$5 and
   $10" is two prices — and neither is escaped. */
const DOLLARS = /(?<![\\$])\$(?![\s$])((?:\\.|[^$\\\n])+?)(?<![\s\\])\$(?![\d$])/g;

/** A web address safe to put in an attribute and hand to the browser.
 *  @param {string} url */
function safeUrl(url) {
  const trimmed = url.trim();
  return /^https?:\/\//i.test(trimmed) ? trimmed : "";
}

/** A link to a file on this machine, and the line it points at if it does:
 *  `file:///Users/x/a.c#L37` or `#L37-L40`. Anything else is not one.
 *  @param {string} href */
export function filePath(href) {
  const m = /^file:\/\/(\/[^#?]+)(?:#L(\d+)(?:-L?\d+)?)?$/i.exec(href.trim());
  if (!m) return null;
  let path = m[1];
  try { path = decodeURIComponent(path); } catch { /* left as written */ }
  return { path, line: m[2] ? Number(m[2]) : null };
}

/** Inline markup, in text that has already been escaped.
 *  @param {string} text escaped @param {Options} o */
function inline(text, o) {
  // Code spans first, set aside, so nothing inside them is read as markup.
  /** @type {string[]} */
  const held = [];
  const hold = (/** @type {string} */ html) => `\u0000${held.push(html) - 1}\u0000`;
  let out = text.replace(/(`+)([^`]|[^`][\s\S]*?[^`])\1(?!`)/g, (_, _ticks, code) =>
    hold(`<code>${code.trim() === "" ? code : code.replace(/^ (.*) $/, "$1")}</code>`));

  // Equations next, so nothing in one is read as emphasis or a link.
  out = out
    .replace(/\$\$((?:\\.|[^$\\]|\$(?!\$))+?)\$\$/g, (_, tex) => hold(math(unescapeHtml(tex), true)))
    .replace(/\\\(([\s\S]+?)\\\)/g, (_, tex) => hold(math(unescapeHtml(tex), false)))
    .replace(DOLLARS, (_, tex) => hold(math(unescapeHtml(tex), false)))
    .replace(/\\\$/g, "$");

  // A citation is `[[path § heading L12-34]]` — what `search_library` hands
  // the model and asks it to quote back. The core opens the note it names.
  if (o.cite) {
    out = out.replace(/\[\[([^[\]\n]+)\]\]/g, (_, cite) =>
      hold(`<span class="cite" data-cite="${cite}" title="Open this note">${cite}</span>`));
  }

  // Links and images: [text](url) — the url arrives escaped, so `&amp;` is
  // put back before it is checked and escaped again for the attribute.
  out = out.replace(/(!?)\[([^\]\n]*)\]\(([^)\s]+)(?:\s+&quot;[^&]*&quot;)?\)/g, (whole, bang, label, href) => {
    // A file on this machine — `file:///…`, the way agents link one — is
    // named by its file alone, its whole path kept for over it and a click.
    const file = filePath(href.replace(/&amp;/g, "&"));
    if (file && !bang) {
      const name = (unescapeHtml(label).split("/").pop() || file.path.split("/").pop() || file.path).trim();
      const at = file.line ? ` · line ${file.line}` : "";
      return hold(`<span class="file-link" data-file-link="${esc(file.path)}"${file.line ? ` data-file-line="${file.line}"` : ""} title="${esc(file.path)}${at}">${esc(name)}</span>`);
    }
    const url = safeUrl(href.replace(/&amp;/g, "&"));
    if (!url) return whole;
    const shown = bang ? `${label || "image"} ↗` : label || url;
    return hold(`<a href="${esc(url)}" data-link title="${esc(url)}">${shown}</a>`);
  });
  // Autolinks: <https://…> and bare addresses.
  out = out.replace(/&lt;(https?:\/\/[^\s&]+)&gt;/g, (_, url) => hold(`<a href="${url}" data-link>${url}</a>`));
  out = out.replace(/(^|[\s(])(https?:\/\/[^\s<]+[^\s<.,;:!?)'"])/g, (_, lead, url) =>
    `${lead}${hold(`<a href="${url}" data-link>${url}</a>`)}`);

  out = out
    .replace(/\*\*\*(?=\S)([\s\S]*?\S)\*\*\*/g, "<strong><em>$1</em></strong>")
    .replace(/\*\*(?=\S)([\s\S]*?\S)\*\*/g, "<strong>$1</strong>")
    .replace(/(^|[^\w])__(?=\S)([\s\S]*?\S)__(?!\w)/g, "$1<strong>$2</strong>")
    .replace(/(^|[^*\w])\*(?=[^\s*])([^*\n]*?[^\s*])\*(?!\w)/g, "$1<em>$2</em>")
    .replace(/(^|[^\w])_(?=[^\s_])([^_\n]*?[^\s_])_(?!\w)/g, "$1<em>$2</em>")
    .replace(/~~(?=\S)([\s\S]*?\S)~~/g, "<del>$1</del>");

  return out.replace(/\u0000(\d+)\u0000/g, (_, i) => held[Number(i)]);
}

/** A table row's cells, without the outer pipes.
 *  @param {string} line */
function cells(line) {
  const trimmed = line.trim().replace(/^\|/, "").replace(/\|$/, "");
  /** @type {string[]} */
  const out = [];
  let cell = "";
  for (let i = 0; i < trimmed.length; i++) {
    if (trimmed[i] === "\\" && trimmed[i + 1] === "|") {
      cell += "|";
      i++;
    } else if (trimmed[i] === "|") {
      out.push(cell.trim());
      cell = "";
    } else cell += trimmed[i];
  }
  out.push(cell.trim());
  return out;
}

const FENCE = /^(\s{0,3})(`{3,}|~{3,})\s*([\w+#.-]*)[^`]*$/;
const BULLET = /^(\s*)([-*+]|\d{1,9}[.)])\s+(.*)$/;
const DIVIDER = /^\s{0,3}([-*_])(\s*\1){2,}\s*$/;
const ALIGN = /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/;

/** A fenced block's markup: the code, coloured for its language, and a
 *  copy button. A ```math block is an equation.
 *  @param {string} code @param {string} lang */
function codeBlock(code, lang) {
  if (/^(math|katex)$/i.test(lang) && code.trim()) return mathBlock(code);
  return (
    `<div class="md-code"><div class="md-code-head"><span>${esc(lang || "text")}</span>` +
    `<button class="md-copy" data-copy title="Copy">Copy</button></div>` +
    `<pre><code${lang ? ` data-lang="${esc(lang)}"` : ""}>${highlight(code, lang)}</code></pre></div>`
  );
}

/* An equation on lines of its own: `$$` or `\[` to open, the matching `$$` or
   `\]` to close, or a \begin…\end of one of the environments KaTeX draws in
   display. */
const MATH_OPEN = /^\s{0,3}(\$\$|\\\[)(.*)$/;
const ENVIRONMENT = /^\s{0,3}\\begin\{(equation|align|alignat|gather|multline|flalign|eqnarray|aligned|gathered|split|CD)(\*?)\}/;

/** The equation that starts at `lines[start]`, and the line after it; null
 *  if it never closes, false if it is one inside a paragraph. What follows
 *  the close on its last line is left in `lines` to be read as text.
 *  @param {string[]} lines @param {number} start */
function displayMath(lines, start) {
  const env = lines[start].match(ENVIRONMENT);
  if (env) {
    const end = `\\end{${env[1]}${env[2]}}`;
    for (let i = start; i < lines.length; i++) {
      const at = lines[i].indexOf(end);
      if (at < 0) continue;
      const tex = [...lines.slice(start, i), lines[i].slice(0, at + end.length)].join("\n");
      const after = lines[i].slice(at + end.length);
      if (after.trim()) {
        lines[i] = after.trimStart();
        return { tex, next: i };
      }
      return { tex, next: i + 1 };
    }
    return null;
  }
  const open = /** @type {RegExpMatchArray} */ (lines[start].match(MATH_OPEN));
  const close = open[1] === "$$" ? "$$" : "\\]";
  /** @type {string[]} */
  const body = [];
  let rest = open[2];
  for (let i = start; i < lines.length; i++) {
    if (i > start) rest = lines[i];
    const at = rest.indexOf(close);
    if (at < 0) {
      body.push(rest);
      continue;
    }
    body.push(rest.slice(0, at));
    const after = rest.slice(at + close.length);
    // `$$x$$ and so on` is an equation inside a paragraph, not a block.
    if (i === start && after.trim()) return false;
    if (after.trim()) {
      lines[i] = after.trimStart();
      return { tex: body.join("\n"), next: i };
    }
    return { tex: body.join("\n"), next: i + 1 };
  }
  return null;
}

/** Lines of a list, starting at `start`: its items, nested by indent.
 *  @param {string[]} lines @param {number} start @param {Options} o */
function list(lines, start, o) {
  const first = /** @type {RegExpMatchArray} */ (lines[start].match(BULLET));
  const indent = first[1].length;
  const ordered = /\d/.test(first[2]);
  /** @type {string[]} */
  const items = [];
  let i = start;
  while (i < lines.length) {
    const match = lines[i].match(BULLET);
    if (!match || match[1].length !== indent || /\d/.test(match[2]) !== ordered) break;
    /** @type {string[]} */
    const body = [match[3]];
    i++;
    // What belongs to this item: indented lines, lazy continuations, and
    // nested lists — until a blank line followed by something that is not.
    while (i < lines.length) {
      const line = lines[i];
      const nested = line.match(BULLET);
      if (nested && nested[1].length <= indent) break;
      if (!line.trim()) {
        const next = lines[i + 1] ?? "";
        if (!next.trim() || (!/^\s/.test(next) && !BULLET.test(next))) break;
        const deeper = next.match(BULLET);
        if (deeper && deeper[1].length <= indent) break;
      }
      if (!nested && !/^\s/.test(line) && FENCE.test(line)) break;
      body.push(line.replace(new RegExp(`^\\s{0,${indent + 3}}`), ""));
      i++;
    }
    let text = body.join("\n");
    let check = "";
    const task = text.match(/^\[([ xX])\]\s+/);
    if (task) {
      check = `<input type="checkbox" disabled${task[1] !== " " ? " checked" : ""}> `;
      text = text.slice(task[0].length);
    }
    const inner = blocks(text, o);
    // A tight item is one paragraph: drawn without its <p>, as a list reads.
    const tight = inner.replace(/^<p>([\s\S]*?)<\/p>/, "$1");
    items.push(`<li${task ? ' class="task"' : ""}>${check}${tight}</li>`);
  }
  const startAt = ordered ? Number.parseInt(first[2], 10) : 1;
  const tag = ordered ? "ol" : "ul";
  const attr = ordered && startAt !== 1 ? ` start="${startAt}"` : "";
  return { html: `<${tag}${attr}>${items.join("")}</${tag}>`, next: i };
}

/** Block markup for a whole text.
 *  @param {string} source @param {Options} o */
function blocks(source, o) {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  /** @type {string[]} */
  const out = [];
  /** @type {string[]} */
  let paragraph = [];
  const flush = () => {
    if (paragraph.length) out.push(`<p>${inline(esc(paragraph.join("\n")), o).replace(/\n/g, "<br>")}</p>`);
    paragraph = [];
  };
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const fence = line.match(FENCE);
    if (fence) {
      flush();
      const close = new RegExp(`^\\s{0,3}${fence[2][0] === "`" ? "`" : "~"}{${fence[2].length},}\\s*$`);
      /** @type {string[]} */
      const code = [];
      i++;
      while (i < lines.length && !close.test(lines[i])) {
        code.push(lines[i].replace(new RegExp(`^\\s{0,${fence[1].length}}`), ""));
        i++;
      }
      i++;
      out.push(codeBlock(code.join("\n"), fence[3]));
      continue;
    }
    if (MATH_OPEN.test(line) || ENVIRONMENT.test(line)) {
      const found = displayMath(lines, i);
      if (found) {
        flush();
        out.push(mathBlock(found.tex));
        i = found.next;
        continue;
      }
      // Still being written: the source until it closes, rather than the
      // rest of the reply read as text and then redrawn as an equation.
      if (found === null && o.streaming) {
        flush();
        out.push(`<div class="md-math-block"><span class="md-math md-display md-pending">${esc(lines.slice(i).join("\n"))}</span></div>`);
        break;
      }
    }
    if (!line.trim()) {
      flush();
      i++;
      continue;
    }
    const heading = line.match(/^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/);
    if (heading) {
      flush();
      const level = heading[1].length;
      out.push(`<h${level}>${inline(esc(heading[2]), o)}</h${level}>`);
      i++;
      continue;
    }
    if (DIVIDER.test(line) && !paragraph.length) {
      flush();
      out.push("<hr>");
      i++;
      continue;
    }
    if (/^\s{0,3}>/.test(line)) {
      flush();
      /** @type {string[]} */
      const quoted = [];
      while (i < lines.length && /^\s{0,3}>/.test(lines[i])) {
        quoted.push(lines[i].replace(/^\s{0,3}>\s?/, ""));
        i++;
      }
      out.push(`<blockquote>${blocks(quoted.join("\n"), o)}</blockquote>`);
      continue;
    }
    if (line.includes("|") && ALIGN.test(lines[i + 1] ?? "")) {
      flush();
      const head = cells(line);
      const aligns = cells(lines[i + 1]).map((c) =>
        c.startsWith(":") && c.endsWith(":") ? "center" : c.endsWith(":") ? "right" : "");
      i += 2;
      /** @type {string[][]} */
      const rows = [];
      while (i < lines.length && lines[i].includes("|") && lines[i].trim()) {
        rows.push(cells(lines[i]));
        i++;
      }
      const cell = (/** @type {string} */ tag, /** @type {string} */ text, /** @type {number} */ n) =>
        `<${tag}${aligns[n] ? ` style="text-align:${aligns[n]}"` : ""}>${inline(esc(text), o)}</${tag}>`;
      out.push(
        `<div class="md-table"><table><thead><tr>${head.map((t, n) => cell("th", t, n)).join("")}</tr></thead>` +
          `<tbody>${rows.map((r) => `<tr>${head.map((_, n) => cell("td", r[n] ?? "", n)).join("")}</tr>`).join("")}</tbody></table></div>`,
      );
      continue;
    }
    if (BULLET.test(line) && (!paragraph.length || /^\s*([-*+]|1[.)])\s/.test(line))) {
      flush();
      const { html, next } = list(lines, i, o);
      out.push(html);
      i = next;
      continue;
    }
    if (/^( {4}|\t)/.test(line) && !paragraph.length) {
      /** @type {string[]} */
      const code = [];
      while (i < lines.length && (/^( {4}|\t)/.test(lines[i]) || !lines[i].trim())) {
        code.push(lines[i].replace(/^( {4}|\t)/, ""));
        i++;
      }
      while (code.length && !code[code.length - 1].trim()) code.pop();
      out.push(codeBlock(code.join("\n"), ""));
      continue;
    }
    paragraph.push(line);
    i++;
  }
  flush();
  return out.join("");
}

/** A reply, as markup.
 *
 *  `streaming` closes an unfinished fence, so a code block being written is
 *  drawn as one rather than as text that becomes one at the end; an
 *  unfinished equation is drawn as its source. `cite` makes `[[…]]` a
 *  citation, for the agent panel's answers from the library.
 *  @param {string} source @param {Options} [options] */
export function markdown(source, options = {}) {
  let text = String(source ?? "");
  if (options.streaming) {
    const fences = text.match(/^\s{0,3}(`{3,}|~{3,})/gm) ?? [];
    if (fences.length % 2 === 1) text += `\n${fences[fences.length - 1].trim()}`;
  }
  return blocks(text, { streaming: Boolean(options.streaming), cite: Boolean(options.cite) });
}

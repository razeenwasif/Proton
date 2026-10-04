/* Markdown, as Claude writes it: what a reply in the Chat tab is drawn from.
 *
 * Escaped first and always. The text is the model's, and the page it is drawn
 * into can reach the editor — so nothing in it is ever HTML. Every construct
 * below is recognised in the escaped text and turned into markup this file
 * writes itself; a `<script>` in a reply stays the four characters it was.
 *
 * What it knows is what replies use: fenced and indented code, headings,
 * lists (nested, ordered, task lists), quotes, tables, rules, emphasis,
 * strikethrough, inline code and links. Links are not followed in the page —
 * a click asks the shell to open them in the browser — and images are links,
 * because the editor's policy loads none from the web. */

import { escapeHtml } from "./html.js";

/** @param {string} text */
const esc = (text) => escapeHtml(text);

/** A web address safe to put in an attribute and hand to the browser.
 *  @param {string} url */
function safeUrl(url) {
  const trimmed = url.trim();
  return /^https?:\/\//i.test(trimmed) ? trimmed : "";
}

/** Inline markup, in text that has already been escaped.
 *  @param {string} text escaped */
function inline(text) {
  // Code spans first, set aside, so nothing inside them is read as markup.
  /** @type {string[]} */
  const held = [];
  const hold = (/** @type {string} */ html) => `\u0000${held.push(html) - 1}\u0000`;
  let out = text.replace(/(`+)([^`]|[^`][\s\S]*?[^`])\1(?!`)/g, (_, _ticks, code) =>
    hold(`<code>${code.trim() === "" ? code : code.replace(/^ (.*) $/, "$1")}</code>`));

  // Links and images: [text](url) — the url arrives escaped, so `&amp;` is
  // put back before it is checked and escaped again for the attribute.
  out = out.replace(/(!?)\[([^\]\n]*)\]\(([^)\s]+)(?:\s+&quot;[^&]*&quot;)?\)/g, (whole, bang, label, href) => {
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

/** A fenced block's markup: the code, its language, and a copy button.
 *  @param {string} code @param {string} lang */
function codeBlock(code, lang) {
  return (
    `<div class="md-code"><div class="md-code-head"><span>${esc(lang || "text")}</span>` +
    `<button class="md-copy" data-copy title="Copy">Copy</button></div>` +
    `<pre><code${lang ? ` data-lang="${esc(lang)}"` : ""}>${esc(code)}</code></pre></div>`
  );
}

/** Lines of a list, starting at `start`: its items, nested by indent.
 *  @param {string[]} lines @param {number} start */
function list(lines, start) {
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
    const inner = blocks(text);
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
 *  @param {string} source */
function blocks(source) {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  /** @type {string[]} */
  const out = [];
  /** @type {string[]} */
  let paragraph = [];
  const flush = () => {
    if (paragraph.length) out.push(`<p>${inline(esc(paragraph.join("\n"))).replace(/\n/g, "<br>")}</p>`);
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
    if (!line.trim()) {
      flush();
      i++;
      continue;
    }
    const heading = line.match(/^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/);
    if (heading) {
      flush();
      const level = heading[1].length;
      out.push(`<h${level}>${inline(esc(heading[2]))}</h${level}>`);
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
      out.push(`<blockquote>${blocks(quoted.join("\n"))}</blockquote>`);
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
        `<${tag}${aligns[n] ? ` style="text-align:${aligns[n]}"` : ""}>${inline(esc(text))}</${tag}>`;
      out.push(
        `<div class="md-table"><table><thead><tr>${head.map((t, n) => cell("th", t, n)).join("")}</tr></thead>` +
          `<tbody>${rows.map((r) => `<tr>${head.map((_, n) => cell("td", r[n] ?? "", n)).join("")}</tr>`).join("")}</tbody></table></div>`,
      );
      continue;
    }
    if (BULLET.test(line) && (!paragraph.length || /^\s*([-*+]|1[.)])\s/.test(line))) {
      flush();
      const { html, next } = list(lines, i);
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
 *  drawn as one rather than as text that becomes one at the end.
 *  @param {string} source @param {{streaming?: boolean}} [options] */
export function markdown(source, options = {}) {
  let text = String(source ?? "");
  if (options.streaming) {
    const fences = text.match(/^\s{0,3}(`{3,}|~{3,})/gm) ?? [];
    if (fences.length % 2 === 1) text += `\n${fences[fences.length - 1].trim()}`;
  }
  return blocks(text);
}

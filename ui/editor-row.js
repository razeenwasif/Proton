/** Text row rendering, independent of DOM and ambient editor state.
 * @typedef {import('./editor-state').Pane} Pane
 * @typedef {import('./bindings/Diagnostic').Diagnostic} Diagnostic
 * @typedef {import('./bindings/Severity').Severity} Severity
 * @typedef {import('./bindings/Scope').Scope} Scope
 * @typedef {Extract<import('./protocol').WireEvent, {type: 'viewport'}>} Viewport
 * @typedef {{pane: Pick<Pane, 'selection' | 'cursor'>, vim: string, diagnostics: Map<number, Diagnostic[]>}} RowContext
 */
import {escapeHtml, icon} from './html.js';
const SIGN = {error: 'd-error', warning: 'd-warning', info: 'd-info', hint: 'd-hint'};

/** @param {string[]} chars @param {number} line @param {Map<number, Diagnostic[]>} diagnostics */
function markUp(chars, line, diagnostics) {
  const here = diagnostics.get(line);
  if (!here?.length) return null;

  // A per-character severity, so overlapping diagnostics resolve by
  // seriousness rather than by whichever the server happened to send last.
  const RANK = { error: 3, warning: 2, info: 1, hint: 0 };
  /** @type {(Severity | null)[]} */
  const worst = new Array(chars.length).fill(null);
  for (const d of here) {
    const from = d.line === line ? d.column : 0;
    // A diagnostic can span lines; on every line but its last it runs to the
    // end. A zero-width range still gets one character, or it draws nothing.
    const to = d.endLine === line ? Math.max(d.endColumn, from + 1) : chars.length;
    for (let i = from; i < Math.min(to, chars.length); i++) {
      if (!worst[i] || RANK[d.severity] > RANK[worst[i] ?? "hint"]) worst[i] = d.severity;
    }
  }
  return worst;
}

/* Scope names from the core to the theme's class names. The core says what a
   run *is*; the stylesheet decides what that looks like, and neither knows
   the other's vocabulary. */
const SCOPE = {
  keyword: "k", function: "fn", type: "ty", string: "str",
  number: "num", comment: "cm", constant: "cn", variable: "va",
  property: "pr", operator: "op", punctuation: "pn", attribute: "at",
  // Prose. Two of these are a weight and a slope rather than a colour,
  // because that is how the text itself says them.
  heading: "md-h", emphasis: "md-em", strong: "md-b", link: "md-a",
};

/// Emit `[from, to)` as merged runs of identical colour and severity.
///
/// Merged, because one element per character is a few thousand nodes on a
/// wide line and the browser feels every one of them. Two channels at once,
/// because a keyword inside a diagnostic needs to be both coloured and
/// underlined and splitting them would need twice the elements.
/** @param {string[]} chars @param {(Scope | null)[]} scope
 * @param {(Severity | null)[] | null} severity @param {boolean[] | null} picked
 * @param {boolean[] | null} hit @param {number} from @param {number} to */
function runsHtml(chars, scope, severity, picked, hit, from, to) {
  /** @param {number} i @param {number} j */
  const same = (i, j) =>
    (scope?.[i] ?? null) === (scope?.[j] ?? null) &&
    (severity?.[i] ?? null) === (severity?.[j] ?? null) &&
    Boolean(picked?.[i]) === Boolean(picked?.[j]) &&
    Boolean(hit?.[i]) === Boolean(hit?.[j]);

  let html = "";
  let i = from;
  while (i < to) {
    let end = i + 1;
    while (end < to && same(i, end)) end++;

    const text = escapeHtml(chars.slice(i, end).join(""));
    const classes = [];
    const colour = scope?.[i];
    if (colour && SCOPE[colour]) classes.push(SCOPE[colour]);
    if (severity?.[i]) classes.push("sq", "d-" + severity[i]);
    if (picked?.[i]) classes.push("sel");
    // Last, so a match reads over a scope rather than under it — the point of
    // painting it is that you can find it.
    if (hit?.[i]) classes.push("hit");
    html += classes.length ? `<span class="${classes.join(" ")}">${text}</span>` : text;
    i = end;
  }
  return html;
}

/* Which characters on a line are selected.
 *
 * Three shapes, and they are genuinely different questions. Charwise runs
 * from one point to another and only the first and last lines are partial.
 * Linewise takes whole lines. A block takes the same columns from every line,
 * which is the one shape a range of offsets cannot describe — and the reason
 * the mode has to know which of the three it is rather than just holding two
 * endpoints. */
/** @param {number} line @param {number} width @param {RowContext} context @returns {[number, number] | null} */
function selectedOn(line, width, {pane, vim}) {
  const anchor = pane.selection;
  if (!anchor) return null;
  const head = pane.cursor;

  const forwards = anchor.line < head.line || (anchor.line === head.line && anchor.column <= head.column);
  const top = forwards ? anchor : head;
  const bottom = forwards ? head : anchor;
  if (line < top.line || line > bottom.line) return null;

  if (vim === "V-LINE") return [0, width];
  if (vim === "V-BLOCK") {
    const left = Math.min(anchor.column, head.column);
    const right = Math.max(anchor.column, head.column) + 1;
    return [Math.min(left, width), Math.min(right, width)];
  }
  const from = line === top.line ? top.column : 0;
  const to = line === bottom.line ? bottom.column + 1 : width;
  return [Math.min(from, width), Math.min(to, width)];
}

/** @param {number} number @param {string} text @param {number | undefined} cursorColumn
 * @param {Viewport['highlights'][number] | undefined} highlights
 * @param {Viewport['matches'][number] | undefined} hits
 * @param {Viewport['git'][number] | undefined} git
 * @param {Viewport['stops'][number] | undefined} stop @param {RowContext} context */
export function lineHtml(number, text, cursorColumn, highlights, hits, git, stop, context) {
  // A breakpoint takes the sign column: it is a thing you put there, and a
  // diagnostic is a thing that happened. Yours wins.
  const here = context.diagnostics.get(number);
  const sign = stop
    ? `<span class="s bp${stop.verified ? "" : " unset"}${stop.here ? " at" : ""}"></span>`
    : here?.length
    ? `<span class="s d-${here[0].severity}">${icon(SIGN[here[0].severity], "solid")}</span>`
    : `<span class="s"></span>`;
  // A thin bar between the number and the diagnostic sign. Its own strip
  // rather than sharing one, because a line can be both changed and broken
  // and you need to see both.
  const change = git ? `<span class="g ${git}"></span>` : `<span class="g"></span>`;
  const gutter = `<span class="n">${number + 1}</span>${change}${sign}`;

  const chars = [...text];
  const severity = markUp(chars, number, context.diagnostics);

  // Spans are character columns, already merged and non-overlapping by the
  // time they arrive, so this only has to expand them.
  /** @type {(Scope | null)[]} */
  const scope = new Array(chars.length).fill(null);
  for (const span of highlights ?? []) {
    for (let i = span.start; i < Math.min(span.end, chars.length); i++) {
      scope[i] = span.scope;
    }
  }

  // Search hits, expanded the same way. `null` when nothing is being
  // searched for, so the merge below has nothing to compare and no cost.
  /** @type {boolean[] | null} */
  let hit = null;
  if (hits?.length) {
    hit = new Array(chars.length).fill(false);
    for (const h of hits) {
      for (let i = h.start; i < Math.min(h.end, chars.length); i++) hit[i] = true;
    }
  }

  const range = selectedOn(number, chars.length, context);
  const picked = range && new Array(chars.length).fill(false);
  if (range && picked) {
    for (let i = range[0]; i < range[1]; i++) picked[i] = true;
  }
  // A selected empty line has nothing to paint, so the row says so instead.
  const empty = range && chars.length === 0 ? " selected" : "";

  // The text goes inside one element, not straight into the row.
  //
  // The row is a flex container so the gutter columns line up, and a flex
  // container **drops text nodes that are only whitespace** — they never
  // become anonymous flex items. Highlighting cuts a line into a span per
  // token with bare spaces between them, so `from typing import List` laid
  // out as flex items reads `fromtypingimportList`. One element around the
  // whole line makes its interior ordinary inline flow again.
  if (cursorColumn === undefined) {
    const body = runsHtml(chars, scope, severity, picked, hit, 0, chars.length);
    return `<div class="ln${empty}">${gutter}<span class="tx">${body}</span></div>`;
  }
  // The caret sits *on* a character in normal mode, so it takes the one it
  // covers with it rather than being drawn between two.
  const under = escapeHtml(chars[cursorColumn] ?? " ") || " ";
  return (
    `<div class="ln cur${empty}">${gutter}<span class="tx">` +
    `${runsHtml(chars, scope, severity, picked, hit, 0, cursorColumn)}` +
    `<span class="caret">${under}</span>` +
    `${runsHtml(chars, scope, severity, picked, hit, cursorColumn + 1, chars.length)}` +
    `</span></div>`
  );
}

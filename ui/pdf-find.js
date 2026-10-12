/* Finding text in a PDF, as the PDF mode's search does: each page's text
 * read with pdf.js, the query found in it however its words were split
 * into pieces, and each hit given the boxes it covers — in points from the
 * page's top left, as the page is turned — so the view can mark it. */

import { toViewport } from "./pdf-view.js";

/** How many hits a search stops at: past this a query says too little. */
export const MOST = 2000;

/** @typedef {{page: number, rects: {x: number, y: number, w: number, h: number}[], before: string, found: string, after: string}} Found */

/** A page's text, as one string, with where each piece of it starts.
 *  @param {any} content what `getTextContent` gave */
export function pageText(content) {
  let text = "";
  /** @type {{at: number, item: any}[]} */
  const pieces = [];
  for (const item of content.items) {
    if (typeof item.str !== "string") continue;
    pieces.push({ at: text.length, item });
    text += item.str;
    if (item.hasEOL) text += " ";
  }
  return { text, pieces };
}

/** Where `query` is in `text`, ignoring case and runs of spaces: each start
 *  and length. @param {string} text @param {string} query */
export function matches(text, query) {
  const words = query.trim().split(/\s+/).filter(Boolean).map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  if (!words.length) return [];
  const pattern = new RegExp(words.join("\\s+"), "giu");
  /** @type {{at: number, length: number}[]} */
  const out = [];
  for (const m of text.matchAll(pattern)) {
    if (m[0].length) out.push({ at: m.index ?? 0, length: m[0].length });
  }
  return out;
}

/** The boxes a stretch of a page's text covers, in points from the page's
 *  top left, through `viewport` (scale 1, turned as the view is).
 *  @param {{at: number, item: any}[]} pieces @param {number} at @param {number} length @param {any} viewport */
export function boxes(pieces, at, length, viewport) {
  const end = at + length;
  const rects = [];
  for (const { at: start, item } of pieces) {
    const stop = start + item.str.length;
    if (stop <= at || start >= end || !item.str.length) continue;
    const from = Math.max(at, start) - start, to = Math.min(end, stop) - start;
    const [, , , , x, y] = item.transform;
    const width = item.width ?? 0;
    const height = item.height || Math.hypot(item.transform[2], item.transform[3]) || 10;
    const x1 = x + (width * from) / item.str.length;
    const x2 = x + (width * to) / item.str.length;
    // Down to the descenders and up to the cap height, roughly as a reader sees the word.
    const [a, b, c, d] = toViewport(viewport, [x1, y - height * 0.2, x2, y + height * 0.9]);
    rects.push({ x: Math.min(a, c), y: Math.min(b, d), w: Math.abs(c - a), h: Math.abs(d - b) });
  }
  return rects;
}

/**
 * Find `query` in every page of `doc`, a page at a time, saying each page's
 * hits as they are found; stops when `cancelled` says so.
 * @param {any} doc @param {string} query @param {number} turned the view's own turn
 * @param {(found: Found[], page: number, pages: number) => void} each @param {() => boolean} cancelled
 */
export async function find(doc, query, turned, each, cancelled) {
  let total = 0;
  for (let n = 1; n <= doc.numPages; n++) {
    if (cancelled()) return;
    const page = await doc.getPage(n);
    const { text, pieces } = pageText(await page.getTextContent());
    const viewport = page.getViewport({ scale: 1, rotation: (page.rotate + turned) % 360 });
    /** @type {Found[]} */
    const found = [];
    for (const m of matches(text, query)) {
      if (total >= MOST) break;
      total++;
      found.push({
        page: n,
        rects: boxes(pieces, m.at, m.length, viewport),
        before: text.slice(Math.max(0, m.at - 40), m.at),
        found: text.slice(m.at, m.at + m.length),
        after: text.slice(m.at + m.length, m.at + m.length + 60),
      });
    }
    if (cancelled()) return;
    each(found, n, doc.numPages);
    if (total >= MOST) return;
  }
}

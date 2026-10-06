/* Pictures of what the studio made: an infographic, drawn from one of three
 * templates — columns, a timeline, numbers — and a mind map, drawn as a
 * tree; each saved as PNG, rasterised here, by drawing its SVG on a canvas.
 * The SVG is plain shapes and text, nothing embedded, so the canvas it is
 * drawn on can be read back. */

import { escapeHtml as esc, icon } from "./html.js";
import { answerHtml } from "./study-chat.js";

/** @typedef {import("./bindings/StudyMade").StudyMade} Made */
/** @typedef {import("./bindings/StudyBlock").StudyBlock} Block */
/** @typedef {import("./bindings/StudyMapNode").StudyMapNode} Node */
/** @typedef {import("./bindings/StudyCitation").StudyCitation} Citation */
/** @typedef {{made: Made, style: string, subtitle: string, blocks: Block[], takeaway: string, citations: Citation[]}} GraphicEvent */

const FONT = "-apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, Arial, sans-serif";
const INK = "#1d1f24";
const SOFT = "#5c6270";
const PALETTE = ["#5b5bd6", "#0f8a7e", "#c2410c", "#b0287a", "#2563eb", "#7c3aed", "#15803d", "#b45309"];

/** One canvas, to measure text with. */
let measurer = /** @type {CanvasRenderingContext2D | null} */ (null);

/** Words in lines no wider than `width`, at a size and weight. @param {string} text @param {number} width @param {number} size @param {number} [weight] */
export function wrap(text, width, size, weight = 400) {
  measurer ??= document.createElement("canvas").getContext("2d");
  const ctx = measurer;
  if (!ctx) return [text];
  ctx.font = `${weight} ${size}px ${FONT}`;
  /** @type {string[]} */
  const lines = [];
  let line = "";
  for (const word of String(text).split(/\s+/).filter(Boolean)) {
    const next = line ? `${line} ${word}` : word;
    if (ctx.measureText(next).width > width && line) {
      lines.push(line);
      line = word;
    } else {
      line = next;
    }
  }
  if (line) lines.push(line);
  return lines.length ? lines : [""];
}

/** Lines as SVG text. @param {string[]} lines @param {number} x @param {number} y @param {number} size @param {string} fill @param {number} [weight] @param {string} [anchor] */
function text(lines, x, y, size, fill, weight = 400, anchor = "start") {
  const lead = Math.round(size * 1.3);
  return `<text x="${x}" y="${y}" font-family="${esc(FONT)}" font-size="${size}" font-weight="${weight}" fill="${fill}" text-anchor="${anchor}">` +
    lines.map((l, i) => `<tspan x="${x}" dy="${i ? lead : 0}">${esc(l)}</tspan>`).join("") + `</text>`;
}

/** An infographic, as SVG. @param {GraphicEvent} g */
export function graphicSvg(g) {
  const W = 1200, pad = 64;
  const title = wrap(g.made.title, W - 2 * pad, 46, 800);
  const subtitle = g.subtitle ? wrap(g.subtitle, W - 2 * pad, 24) : [];
  let y = pad + 46;
  let body = text(title, pad, y, 46, INK, 800);
  y += title.length * 60;
  if (subtitle.length) {
    body += text(subtitle, pad, y, 24, SOFT);
    y += subtitle.length * 31 + 12;
  }
  y += 18;
  if (g.style === "timeline") {
    const x = pad + 20;
    const top = y;
    let items = "";
    g.blocks.forEach((b, i) => {
      const color = PALETTE[i % PALETTE.length];
      const label = b.figure ? `${b.figure} · ${b.heading}` : b.heading;
      const head = wrap(label, W - 2 * pad - 60, 26, 700);
      const lines = wrap(b.text, W - 2 * pad - 60, 20);
      items += `<circle cx="${x}" cy="${y + 10}" r="11" fill="${color}"/>` + text(head, x + 34, y + 19, 26, INK, 700) + text(lines, x + 34, y + 19 + head.length * 34 + 4, 20, SOFT);
      y += head.length * 34 + lines.length * 26 + 34;
    });
    body += `<line x1="${x}" y1="${top}" x2="${x}" y2="${y - 30}" stroke="#d4d7de" stroke-width="4"/>${items}`;
  } else {
    const columns = g.style === "numbers" ? 2 : Math.min(4, Math.max(2, g.blocks.length <= 4 ? g.blocks.length : 3));
    const gap = 24;
    const width = (W - 2 * pad - gap * (columns - 1)) / columns;
    for (let row = 0; row < g.blocks.length; row += columns) {
      const cards = g.blocks.slice(row, row + columns).map((b, k) => {
        const figureSize = g.style === "numbers" ? 64 : 40;
        const head = wrap(b.heading, width - 48, 26, 700);
        const lines = wrap(b.text, width - 48, 20);
        const height = 36 + (b.figure ? figureSize + 14 : 0) + head.length * 34 + lines.length * 26 + 24;
        return { b, k, head, lines, height, figureSize };
      });
      const tallest = Math.max(...cards.map((c) => c.height));
      for (const c of cards) {
        const x = pad + c.k * (width + gap);
        const color = PALETTE[(row + c.k) % PALETTE.length];
        let at = y + 36;
        body += `<rect x="${x}" y="${y}" width="${width}" height="${tallest}" rx="18" fill="#f5f6f9"/><rect x="${x}" y="${y}" width="${width}" height="8" rx="4" fill="${color}"/>`;
        if (c.b.figure) {
          at += c.figureSize - 14;
          body += text([c.b.figure], x + 24, at, c.figureSize, color, 800);
          at += 28;
        }
        body += text(c.head, x + 24, at + 10, 26, INK, 700);
        at += c.head.length * 34 + 8;
        body += text(c.lines, x + 24, at + 6, 20, SOFT);
      }
      y += tallest + gap;
    }
  }
  if (g.takeaway) {
    const lines = wrap(g.takeaway, W - 2 * pad - 48, 26, 600);
    const height = lines.length * 34 + 40;
    y += 10;
    body += `<rect x="${pad}" y="${y}" width="${W - 2 * pad}" height="${height}" rx="18" fill="${INK}"/>` + text(lines, pad + 24, y + 44, 26, "#ffffff", 600);
    y += height;
  }
  const H = Math.ceil(y + pad);
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}"><rect width="${W}" height="${H}" fill="#ffffff"/>${body}</svg>`;
}

/** A mind map, as SVG: a tree growing to the right. @param {string} title @param {Node[]} nodes */
export function mapSvg(title, nodes) {
  const pad = 40, step = 290, row = 52, box = 250;
  /** @type {Map<string | null, Node[]>} */
  const children = new Map();
  for (const n of nodes) children.set(n.parent, [...(children.get(n.parent) ?? []), n]);
  /** @type {{n: Node, depth: number, y: number}[]} */
  const placed = [];
  let next = 0;
  /** @param {Node} n @param {number} depth @returns {number} */
  const place = (n, depth) => {
    const kids = children.get(n.id) ?? [];
    let y;
    if (!kids.length) {
      y = next++ * row;
    } else {
      const ys = kids.map((k) => place(k, depth + 1));
      y = (ys[0] + ys[ys.length - 1]) / 2;
    }
    placed.push({ n, depth, y });
    return y;
  };
  (children.get(null) ?? []).forEach((root) => place(root, 0));
  const at = new Map(placed.map((p) => [p.n.id, p]));
  const depth = Math.max(0, ...placed.map((p) => p.depth));
  const W = pad * 2 + (depth + 1) * step;
  const H = pad * 2 + Math.max(1, next) * row + 40;
  const top = pad + 50;
  let links = "", boxes = "";
  for (const p of placed) {
    const x = pad + p.depth * step, y = top + p.y;
    const color = PALETTE[p.depth % PALETTE.length];
    const parent = p.n.parent ? at.get(p.n.parent) : null;
    if (parent) {
      const px = pad + parent.depth * step + box, py = top + parent.y;
      links += `<path d="M${px} ${py} C ${px + 30} ${py}, ${x - 30} ${y}, ${x} ${y}" fill="none" stroke="#c9ccd4" stroke-width="2"/>`;
    }
    const label = wrap(p.n.label.replace(/\$/g, ""), box - 24, 16, p.depth ? 500 : 700).slice(0, 2);
    const h = label.length > 1 ? 44 : 30;
    boxes += `<rect x="${x}" y="${y - h / 2}" width="${box}" height="${h}" rx="${h / 2}" fill="${p.depth ? "#f5f6f9" : color}" stroke="${color}" stroke-width="${p.depth ? 1.5 : 0}"/>` +
      text(label, x + 14, y + (label.length > 1 ? -4 : 5), 16, p.depth ? INK : "#ffffff", p.depth ? 500 : 700);
  }
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}" viewBox="0 0 ${W} ${H}"><rect width="${W}" height="${H}" fill="#ffffff"/>` +
    text([title], pad, pad + 10, 26, INK, 800) + links + boxes + `</svg>`;
}

/** An SVG drawn on a canvas, twice its size, and read back as a PNG. @param {string} svg */
export function pictureOf(svg) {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => {
      const canvas = document.createElement("canvas");
      canvas.width = image.naturalWidth * 2;
      canvas.height = image.naturalHeight * 2;
      const ctx = canvas.getContext("2d");
      if (!ctx) return reject(new Error("no canvas"));
      ctx.scale(2, 2);
      ctx.drawImage(image, 0, 0);
      try {
        resolve(canvas.toDataURL("image/png"));
      } catch (e) {
        reject(e);
      }
    };
    image.onerror = () => reject(new Error("the picture did not draw"));
    image.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
  });
}

/** An infographic, for the middle. @param {GraphicEvent} g @param {boolean} readOnly */
export function graphicHtml(g, readOnly) {
  const head = `<div class="st-reader-head"><button class="cc-quiet" data-study-back>← Back</button>` +
    `<div><h1>${esc(g.made.title)}</h1><span class="st-faint">Infographic · ${esc(g.style)} · ${esc(g.made.by)}</span></div>` +
    `<button class="cc-quiet" data-study-task="${esc(g.made.task)}" title="What the call was told and asked">Prompt</button>` +
    (readOnly ? "" : `<button class="cc-quiet" data-study-bin-made="${esc(g.made.id)}" title="Put it in the notebook's bin">Bin</button>`) + `</div>`;
  const cited = g.blocks.filter((b) => b.cites.length);
  const sources = cited.length ? `<div class="st-graphic-sources" data-turn="made"><h3>Where each block is from</h3>${cited.map((b) => `<p><b>${esc(b.heading)}</b> ` +
    `${answerHtml(b.cites.map((c) => `[[${c}]]`).join(""), g.citations)}</p>`).join("")}</div>` : "";
  return `<div class="st-reader st-graphic-view">${head}<div class="st-deck-tools"><span class="grow"></span>` +
    `${readOnly ? "" : `<button class="cc-quiet" data-study-picture="${esc(g.made.id)}">${icon("image")}Save as PNG</button>`}</div>` +
    `<div class="st-graphic" data-study-graphic>${graphicSvg(g)}</div>${sources}</div>`;
}

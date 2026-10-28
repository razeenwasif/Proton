/* Excel's charts, drawn as SVG from what the core worked out of their
 * ranges: line, bar, scatter and pie. Pure — a chart's view and a size in,
 * markup out — so the grid can redraw one whenever its cells change. */

import { escapeHtml as esc } from "./html.js";

/** @typedef {import("./bindings/SheetChartView").SheetChartView} ChartView */

/** Ten colours that stay apart from one another on either ground. */
export const PALETTE = ["#4e79a7", "#f28e2b", "#e15759", "#76b7b2", "#59a14f", "#edc948", "#b07aa1", "#ff9da7", "#9c755f", "#bab0ac"];

/** Round ticks from `lo` to `hi`, about `n` of them. @param {number} lo @param {number} hi @param {number} n */
export function ticks(lo, hi, n = 5) {
  if (!(hi > lo)) {
    const v = Number.isFinite(lo) ? lo : 0;
    return [v - 1, v, v + 1];
  }
  const raw = (hi - lo) / n;
  const mag = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw) ?? raw;
  const out = [];
  for (let v = Math.floor(lo / step) * step; v <= hi + step * 1e-9; v += step) out.push(Math.round(v / step) * step);
  if (out[out.length - 1] < hi) out.push(out[out.length - 1] + step);
  return out;
}

/** A number short enough for an axis. @param {number} v */
export function label(v) {
  const a = Math.abs(v);
  if (a >= 1e9) return `${+(v / 1e9).toFixed(1)}B`;
  if (a >= 1e6) return `${+(v / 1e6).toFixed(1)}M`;
  if (a >= 1e4) return `${+(v / 1e3).toFixed(1)}k`;
  return String(+v.toFixed(a < 1 ? 3 : 2));
}

/** A chart as SVG, `w` by `h`. @param {ChartView} view @param {number} w @param {number} h */
export function chartSvg(view, w, h) {
  const { kind } = view.chart;
  const series = view.series;
  const open = `<svg class="xl-chart-svg" viewBox="0 0 ${w} ${h}" width="${w}" height="${h}" role="img" aria-label="${esc(view.chart.title || "Chart")}">`;
  if (!series.length || !view.categories.length) {
    return `${open}<text x="${w / 2}" y="${h / 2}" text-anchor="middle" class="xl-chart-none">Nothing to draw in ${esc(view.chart.range)}</text></svg>`;
  }
  if (kind === "pie") return open + pie(view, w, h) + "</svg>";
  const legend = series.length > 1;
  const pad = { l: 46, r: legend ? 110 : 14, t: 10, b: 30 };
  const pw = Math.max(10, w - pad.l - pad.r), ph = Math.max(10, h - pad.t - pad.b);
  const values = series.flatMap(([, v]) => v).filter((/** @type {number | null} */ v) => v !== null);
  let lo = Math.min(...values), hi = Math.max(...values);
  if (kind === "bar") { lo = Math.min(0, lo); hi = Math.max(0, hi); }
  const ys = ticks(lo, hi);
  const y0 = ys[0], y1 = ys[ys.length - 1];
  const Y = (/** @type {number} */ v) => pad.t + ph - ((v - y0) / (y1 - y0 || 1)) * ph;
  const n = view.categories.length;
  let out = "";
  // The value axis and its lines.
  for (const t of ys) {
    out += `<line class="xl-chart-grid" x1="${pad.l}" x2="${pad.l + pw}" y1="${Y(t)}" y2="${Y(t)}"/>`;
    out += `<text class="xl-chart-tick" x="${pad.l - 6}" y="${Y(t) + 4}" text-anchor="end">${esc(label(t))}</text>`;
  }
  let X;
  if (kind === "scatter") {
    const xv = view.xs.filter((/** @type {number | null} */ v) => v !== null);
    const xt = ticks(Math.min(...xv), Math.max(...xv));
    const x0 = xt[0], x1 = xt[xt.length - 1];
    X = (/** @type {number} */ i) => pad.l + (((view.xs[i] ?? x0) - x0) / (x1 - x0 || 1)) * pw;
    for (const t of xt) out += `<text class="xl-chart-tick" x="${pad.l + ((t - x0) / (x1 - x0 || 1)) * pw}" y="${pad.t + ph + 16}" text-anchor="middle">${esc(label(t))}</text>`;
  } else {
    const band = pw / n;
    X = (/** @type {number} */ i) => pad.l + band * (i + 0.5);
    // As many category names as fit.
    const every = Math.max(1, Math.ceil(n / Math.max(1, Math.floor(pw / 60))));
    for (let i = 0; i < n; i += every) {
      const name = view.categories[i].length > 10 ? `${view.categories[i].slice(0, 9)}…` : view.categories[i];
      out += `<text class="xl-chart-tick" x="${X(i)}" y="${pad.t + ph + 16}" text-anchor="middle">${esc(name)}</text>`;
    }
  }
  out += `<line class="xl-chart-axis" x1="${pad.l}" x2="${pad.l + pw}" y1="${Y(Math.max(y0, Math.min(0, y1)))}" y2="${Y(Math.max(y0, Math.min(0, y1)))}"/>`;
  series.forEach(([, vals], s) => {
    const color = PALETTE[s % PALETTE.length];
    if (kind === "bar") {
      const band = pw / n, bw = (band * 0.8) / series.length;
      vals.forEach((v, i) => {
        if (v === null) return;
        const x = pad.l + band * i + band * 0.1 + bw * s, top = Y(Math.max(v, 0)), bottom = Y(Math.min(v, 0));
        out += `<rect x="${x}" y="${top}" width="${Math.max(1, bw - 1)}" height="${Math.max(0.5, bottom - top)}" fill="${color}"><title>${esc(view.categories[i])}: ${v}</title></rect>`;
      });
    } else if (kind === "scatter") {
      vals.forEach((v, i) => {
        if (v === null || view.xs[i] === null) return;
        out += `<circle cx="${X(i)}" cy="${Y(v)}" r="3.5" fill="${color}"><title>${view.xs[i]}, ${v}</title></circle>`;
      });
    } else {
      // A line, broken where a value is missing.
      let d = "", pen = false;
      vals.forEach((v, i) => {
        if (v === null) { pen = false; return; }
        d += `${pen ? "L" : "M"}${X(i).toFixed(1)},${Y(v).toFixed(1)}`;
        pen = true;
      });
      out += `<path d="${d}" fill="none" stroke="${color}" stroke-width="2" stroke-linejoin="round"/>`;
      if (n <= 40) vals.forEach((v, i) => { if (v !== null) out += `<circle cx="${X(i)}" cy="${Y(v)}" r="2.5" fill="${color}"><title>${esc(view.categories[i])}: ${v}</title></circle>`; });
    }
  });
  if (legend) {
    series.forEach(([name], s) => {
      const y = pad.t + 6 + s * 18;
      out += `<rect x="${w - pad.r + 12}" y="${y}" width="10" height="10" rx="2" fill="${PALETTE[s % PALETTE.length]}"/>`;
      out += `<text class="xl-chart-tick" x="${w - pad.r + 28}" y="${y + 9}">${esc(name.length > 12 ? `${name.slice(0, 11)}…` : name)}</text>`;
    });
  }
  return open + out + "</svg>";
}

/** The first series as slices, the biggest named. @param {ChartView} view @param {number} w @param {number} h */
function pie(view, w, h) {
  const vals = (view.series[0]?.[1] ?? []).map((v) => (v !== null && v > 0 ? v : 0));
  const total = vals.reduce((a, b) => a + b, 0);
  if (!total) return `<text x="${w / 2}" y="${h / 2}" text-anchor="middle" class="xl-chart-none">No positive values to share out</text>`;
  const r = Math.max(10, Math.min(w, h) / 2 - 24), cx = w / 2, cy = h / 2;
  let a = -Math.PI / 2, out = "";
  vals.forEach((v, i) => {
    if (!v) return;
    const share = v / total, b = a + share * Math.PI * 2;
    const large = b - a > Math.PI ? 1 : 0;
    const p = (/** @type {number} */ t, /** @type {number} */ rr = r) => `${(cx + rr * Math.cos(t)).toFixed(1)},${(cy + rr * Math.sin(t)).toFixed(1)}`;
    const d = share >= 0.9999 ? `M${p(0)}A${r},${r} 0 1 1 ${p(Math.PI)}A${r},${r} 0 1 1 ${p(0)}Z` : `M${cx},${cy}L${p(a)}A${r},${r} 0 ${large} 1 ${p(b)}Z`;
    out += `<path d="${d}" fill="${PALETTE[i % PALETTE.length]}" stroke="var(--bg)" stroke-width="1"><title>${esc(view.categories[i])}: ${v} (${Math.round(share * 100)}%)</title></path>`;
    if (share > 0.05) {
      const [lx, ly] = p((a + b) / 2, r * 0.65).split(",");
      out += `<text class="xl-chart-slice" x="${lx}" y="${ly}" text-anchor="middle">${esc(view.categories[i].slice(0, 10))}</text>`;
    }
    a = b;
  });
  return out;
}

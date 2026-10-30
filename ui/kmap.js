/* Karnaugh maps, as Circuit's analysis and Logic's calculator draw them:
 * the table's rows in Gray order, the first half of the variables down the
 * side and the rest across, each group of a least form ringed in its own
 * colour — two rings where a group wraps round an edge. */

/** The Gray code of `n` bits, in order: 00 01 11 10 for two. @param {number} n */
export function gray(n) {
  return Array.from({ length: 1 << n }, (_, i) => i ^ (i >> 1));
}

/** A Karnaugh map's layout for `n` variables: the first half (rounded
 *  down) down the side, the rest across, each in Gray order; and which row
 *  of the table each cell is. @param {number} n */
export function kmap(n) {
  const rv = Math.floor(n / 2), cv = n - rv;
  const rows = gray(rv), cols = gray(cv);
  return { rv, cv, rows, cols, row: (/** @type {number} */ r, /** @type {number} */ c) => (rows[r] << cv) | cols[c] };
}

/** Runs of consecutive positions, as [first, last]: a group that wraps is
 *  two. @param {number[]} at */
export function runs(at) {
  const out = /** @type {[number, number][]} */ ([]);
  for (const p of [...at].sort((a, b) => a - b)) {
    const last = out.at(-1);
    if (last && p === last[1] + 1) last[1] = p;
    else out.push([p, p]);
  }
  return out;
}

/** The map of a function of 2 to 6 variables drawn on a canvas.
 *  @param {HTMLCanvasElement} c
 *  @param {string[]} inputs the variables, the first the top bit
 *  @param {string} column `0`, `1` or `x` a row
 *  @param {[number, number][]} groups the least form's terms, `(value, care)`
 *  @param {{muted: string, text: string, line: string}} col */
export function drawKmap(c, inputs, column, groups, col) {
  const n = inputs.length, m = kmap(n), cell = 30, left = 44, top = 30;
  const w = left + m.cols.length * cell + 8, h = top + m.rows.length * cell + 8;
  const dpr = window.devicePixelRatio || 1;
  c.width = w * dpr; c.height = h * dpr;
  c.style.width = `${w}px`; c.style.height = `${h}px`;
  const g = /** @type {CanvasRenderingContext2D} */ (c.getContext("2d"));
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.font = `11px ui-monospace, "SF Mono", Menlo, monospace`;
  g.textAlign = "center";
  g.textBaseline = "middle";
  g.fillStyle = col.muted;
  const bits = (/** @type {number} */ v, /** @type {number} */ len) => v.toString(2).padStart(len, "0");
  g.fillText(`${inputs.slice(0, m.rv).join("")}\\${inputs.slice(m.rv).join("")}`, left / 2, top / 2);
  m.cols.forEach((v, i) => g.fillText(m.cv ? bits(v, m.cv) : "", left + i * cell + cell / 2, top / 2));
  m.rows.forEach((v, i) => g.fillText(m.rv ? bits(v, m.rv) : "", left / 2, top + i * cell + cell / 2));
  for (let r = 0; r < m.rows.length; r++) for (let q = 0; q < m.cols.length; q++) {
    const v = column[m.row(r, q)];
    g.strokeStyle = col.line;
    g.strokeRect(left + q * cell, top + r * cell, cell, cell);
    g.fillStyle = v === "1" ? col.text : col.muted;
    g.fillText(v === "x" ? "x" : v, left + q * cell + cell / 2, top + r * cell + cell / 2);
  }
  // Each group: the rows and columns its cells are in, in runs.
  const hues = ["#f97316", "#22c55e", "#3b82f6", "#e879f9", "#eab308", "#14b8a6", "#f43f5e", "#a78bfa"];
  groups.forEach(([value, care], gi) => {
    const covers = (/** @type {number} */ row) => (row & care) === (value & care);
    const rs = m.rows.map((_, r) => r).filter((r) => m.cols.some((_, q) => covers(m.row(r, q))));
    const cs = m.cols.map((_, q) => q).filter((q) => m.rows.some((_, r) => covers(m.row(r, q))));
    g.strokeStyle = hues[gi % hues.length];
    g.lineWidth = 2;
    const inset = 3 + (gi % 4) * 2;
    for (const [r0, r1] of runs(rs)) for (const [c0, c1] of runs(cs)) {
      g.beginPath();
      g.roundRect(left + c0 * cell + inset, top + r0 * cell + inset, (c1 - c0 + 1) * cell - 2 * inset, (r1 - r0 + 1) * cell - 2 * inset, 6);
      g.stroke();
    }
    g.lineWidth = 1;
  });
}

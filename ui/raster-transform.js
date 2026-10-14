/* Canvas mode's free transform, as arithmetic: what is lifted, moved, scaled
 * and turned while it is dragged, and the op it becomes when let go. No DOM —
 * the node tests check it.
 *
 * A transform is kept as what a person does to it: moved by (dx, dy), scaled
 * by (sx, sy) and turned by `angle` about the middle of what was lifted. The
 * op carries it inverted — where each pixel of the result came from — in
 * 1/65536, made here once, so the window's copy and the core sample the same
 * places (proton-raster's `sample::Inverse`). */

/** @typedef {{x: number, y: number, width: number, height: number}} Rect */
/** @typedef {{cx: number, cy: number, dx: number, dy: number, sx: number, sy: number, angle: number}} Transform */
/** @typedef {"move" | "rotate" | "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w"} Handle */

const ONE = 65536;

/** Nothing done yet, to what is in `rect`. @param {Rect} rect @returns {Transform} */
export function begin(rect) {
  return { cx: rect.x + rect.width / 2, cy: rect.y + rect.height / 2, dx: 0, dy: 0, sx: 1, sy: 1, angle: 0 };
}

/** The transform as a matrix: a point (x, y) goes to (a·x + b·y + c,
 *  d·x + e·y + f). @param {Transform} t @returns {number[]} */
export function forward(t) {
  const [cos, sin] = [Math.cos(t.angle), Math.sin(t.angle)];
  const [a, b, d, e] = [cos * t.sx, -sin * t.sy, sin * t.sx, cos * t.sy];
  // About the middle, then moved.
  return [a, b, t.cx + t.dx - (a * t.cx + b * t.cy), d, e, t.cy + t.dy - (d * t.cx + e * t.cy)];
}

/** Where a point goes. @param {number[]} m @param {number} x @param {number} y @returns {[number, number]} */
export function apply(m, x, y) {
  return [m[0] * x + m[1] * y + m[2], m[3] * x + m[4] * y + m[5]];
}

/** The lifted rectangle's corners where the transform puts them: top left,
 *  top right, bottom left, bottom right. @param {Transform} t @param {Rect} r */
export function corners(t, r) {
  const m = forward(t);
  return [apply(m, r.x, r.y), apply(m, r.x + r.width, r.y), apply(m, r.x, r.y + r.height), apply(m, r.x + r.width, r.y + r.height)];
}

/** The handles, by where each is on the lifted rectangle (fractions across
 *  and down). */
const HANDLES = /** @type {[Handle, number, number][]} */ ([
  ["nw", 0, 0], ["n", 0.5, 0], ["ne", 1, 0], ["e", 1, 0.5], ["se", 1, 1], ["s", 0.5, 1], ["sw", 0, 1], ["w", 0, 0.5],
]);

/** Each handle's place on the canvas. @param {Transform} t @param {Rect} r @returns {[Handle, number, number][]} */
export function handles(t, r) {
  const m = forward(t);
  return HANDLES.map(([h, u, v]) => [h, ...apply(m, r.x + u * r.width, r.y + v * r.height)]);
}

/** Which handle a canvas point is on — within `reach` canvas pixels — or
 *  whether it is inside (move) or out (turn).
 *  @param {Transform} t @param {Rect} r @param {number} x @param {number} y @param {number} reach @returns {Handle} */
export function hit(t, r, x, y, reach) {
  // A small box keeps its middle for moving: a handle reaches no more than
  // a quarter of its shorter side.
  const near = Math.min(reach, Math.min(Math.abs(t.sx) * r.width, Math.abs(t.sy) * r.height) / 4);
  for (const [h, hx, hy] of handles(t, r)) if (Math.hypot(x - hx, y - hy) <= near) return h;
  const [lx, ly] = local(t, x, y);
  const inside = Math.abs(lx) <= (Math.abs(t.sx) * r.width) / 2 && Math.abs(ly) <= (Math.abs(t.sy) * r.height) / 2;
  return inside ? "move" : "rotate";
}

/** A canvas point in the transform's own frame: from its moved middle,
 *  turned back. @param {Transform} t @param {number} x @param {number} y */
function local(t, x, y) {
  const [px, py] = [x - (t.cx + t.dx), y - (t.cy + t.dy)];
  const [cos, sin] = [Math.cos(-t.angle), Math.sin(-t.angle)];
  return [cos * px - sin * py, sin * px + cos * py];
}

/** The transform as a drag of `handle` from `from` to `to` makes it, from
 *  how it was (`t`) when the drag began. Corners scale keeping the opposite
 *  corner where it is — in proportion unless `free`; sides scale one way; a
 *  turn snaps to fifteen degrees when `snap`.
 *  @param {Transform} t @param {Rect} r @param {Handle} handle @param {[number, number]} from @param {[number, number]} to
 *  @param {{free?: boolean, snap?: boolean}} [how] @returns {Transform} */
export function drag(t, r, handle, from, to, how = {}) {
  if (handle === "move") return { ...t, dx: t.dx + to[0] - from[0], dy: t.dy + to[1] - from[1] };
  const [mx, my] = [t.cx + t.dx, t.cy + t.dy];
  if (handle === "rotate") {
    let angle = t.angle + Math.atan2(to[1] - my, to[0] - mx) - Math.atan2(from[1] - my, from[0] - mx);
    if (how.snap) angle = Math.round(angle / (Math.PI / 12)) * (Math.PI / 12);
    return { ...t, angle };
  }
  // The handle's side across and down (-1, 0 or 1); the point opposite it —
  // a corner, or a side's middle — stays where it is.
  const ux = handle.includes("w") ? -1 : handle.includes("e") ? 1 : 0;
  const uy = handle.includes("n") ? -1 : handle.includes("s") ? 1 : 0;
  const [ax, ay] = [(-ux * r.width) / 2, (-uy * r.height) / 2];
  const anchor = apply(forward(t), t.cx + ax, t.cy + ay);
  // The pointer from the anchor, in the transform's own turned frame.
  const [cos, sin] = [Math.cos(t.angle), Math.sin(t.angle)];
  const [px, py] = [to[0] - anchor[0], to[1] - anchor[1]];
  const [vx, vy] = [cos * px + sin * py, -sin * px + cos * py];
  let [sx, sy] = [ux ? vx / (ux * r.width) : t.sx, uy ? vy / (uy * r.height) : t.sy];
  if (ux && uy && !how.free) {
    // In proportion: as far along the corner's diagonal as the pointer is.
    const [dx, dy] = [ux * r.width * t.sx, uy * r.height * t.sy];
    const k = (vx * dx + vy * dy) / (dx * dx + dy * dy || 1);
    [sx, sy] = [t.sx * k, t.sy * k];
  }
  // The middle, so the anchor stays: anchor − R·S·(the anchor from the middle).
  const [hx, hy] = [sx * ax, sy * ay];
  const middle = [anchor[0] - (cos * hx - sin * hy), anchor[1] - (sin * hx + cos * hy)];
  return { ...t, sx, sy, dx: middle[0] - t.cx, dy: middle[1] - t.cy };
}

/** Whether the transform only moves, and by whole pixels. @param {Transform} t */
export function wholeMove(t) {
  return t.angle === 0 && t.sx === 1 && t.sy === 1 && Number.isInteger(t.dx) && Number.isInteger(t.dy);
}

/** The op a transform becomes: the result's rectangle on the canvas, and
 *  where each of its pixels came from, inverted and in 1/65536. Nothing if
 *  it is all off the canvas, or squashed to nothing.
 *  @param {Transform} t @param {Rect} r @param {number} width @param {number} height
 *  @returns {{bounds: number[], inverse: number[]} | null} */
export function commit(t, r, width, height) {
  const [a, b, c, d, e, f] = forward(t);
  const det = a * e - b * d;
  if (Math.abs(det) < 1e-9) return null;
  const points = corners(t, r);
  const xs = points.map((p) => p[0]);
  const ys = points.map((p) => p[1]);
  // Whole pixels, a hair in from the edges so a whole move is not a pixel wider.
  const x0 = Math.max(0, Math.floor(Math.min(...xs) + 1e-6));
  const y0 = Math.max(0, Math.floor(Math.min(...ys) + 1e-6));
  const x1 = Math.min(width, Math.ceil(Math.max(...xs) - 1e-6));
  const y1 = Math.min(height, Math.ceil(Math.max(...ys) - 1e-6));
  if (x1 <= x0 || y1 <= y0) return null;
  // The inverse: x = (e·(X − c) − b·(Y − f)) / det, y = (−d·(X − c) + a·(Y − f)) / det.
  const [ia, ib, id, ie] = [e / det, -b / det, -d / det, a / det];
  const ic = -(ia * c + ib * f);
  const iff = -(id * c + ie * f);
  const q = (/** @type {number} */ v) => Math.round(v * ONE) || 0;
  return { bounds: [x0, y0, x1 - x0, y1 - y0], inverse: [q(ia), q(ib), q(ic), q(id), q(ie), q(iff)] };
}

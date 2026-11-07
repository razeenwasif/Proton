/* CAD's sketch editor: a sketch flat on its plane, drawn in SVG, its
 * shapes drawn and held with the mouse (docs/CAD.md, K3).
 *
 * Everything drawn is written as a line of the .part language — `line l3
 * l2.end (40, 10)`, `distance a b = 25` — and sent to the core, which adds
 * it to the sketch, solves it and sends the sketch back where the solver
 * put everything. Nothing is solved here: the window draws what the core
 * says, and a drag asks the core to move a point with the rest held.
 *
 * Points snap to the points already drawn — the new shape then starts
 * from that one, by its name — and a line drawn within a few degrees of
 * level or upright is held so. A dimension is written at what it measures
 * now, and typed over in place. */

/** @typedef {import("./bindings/CadSketchView").CadSketchView} SketchView */
/** @typedef {import("./bindings/CadShape").CadShape} Shape */
/** @typedef {import("./bindings/CadConstraint").CadConstraint} Constraint */
/** @typedef {import("./bindings/CadOp").CadOp} Op */
/** @typedef {[number, number]} P */
/** @typedef {{kind: "point", name: string} | {kind: "shape", name: string} | {kind: "constraint", item: number}} Picked */

/** How near the pointer must come, in pixels, to pick or snap. */
const NEAR = 8;
/** Within this many degrees of level or upright, a line is held so. */
const LEVEL = 2.5;

/** A number as the file writes it: to a nanometre, as briefly as it reads.
 *  @param {number} x */
export function num(x) {
  const r = Math.round(x * 1e9) / 1e9;
  return String(Object.is(r, -0) ? 0 : r);
}

/** A number for a new dimension: rounded to what is worth typing.
 *  @param {number} x */
export function round(x) {
  return num(Math.round(x * 1000) / 1000);
}

/** The first of `prefix1`, `prefix2`… not in `taken`.
 *  @param {string} prefix @param {Iterable<string>} taken */
export function fresh(prefix, taken) {
  const used = new Set(taken);
  for (let k = 1; ; k++) if (!used.has(`${prefix}${k}`)) return `${prefix}${k}`;
}

/** Every name the sketch uses, shapes' and points'. @param {SketchView} s */
export function names(s) {
  return [...s.shapes.map((x) => x.name.split(".")[0]), ...s.points.map((p) => p.name.split(".")[0])];
}

/** A point as a shape's text says it: by name, or as numbers.
 *  @param {{name?: string, at: P}} spot */
export function spot(spot) {
  return spot.name ?? `(${num(spot.at[0])}, ${num(spot.at[1])})`;
}

/** The angle of a vector, in degrees from level, -180..180. @param {P} d */
const degrees = (d) => (Math.atan2(d[1], d[0]) * 180) / Math.PI;

/** Whether a line from `a` to `b` is near level, or near upright.
 *  @param {P} a @param {P} b @returns {"horizontal" | "vertical" | null} */
export function level(a, b) {
  const d = [b[0] - a[0], b[1] - a[1]];
  if (Math.hypot(d[0], d[1]) === 0) return null;
  const t = Math.abs(degrees(/** @type {P} */ (d)));
  if (t < LEVEL || t > 180 - LEVEL) return "horizontal";
  if (Math.abs(t - 90) < LEVEL) return "vertical";
  return null;
}

/** The lines that draw a rectangle from two corners: its text, with its
 *  corner the bottom left. @param {string} name @param {{name?: string, at: P}} a @param {P} b */
export function rectText(name, a, b) {
  const [x0, y0] = a.at;
  const lo = [Math.min(x0, b[0]), Math.min(y0, b[1])];
  const corner = a.name && lo[0] === x0 && lo[1] === y0 ? a.name : `(${num(lo[0])}, ${num(lo[1])})`;
  return `rect ${name} corner ${corner} size (${num(Math.abs(b[0] - x0))}, ${num(Math.abs(b[1] - y0))})`;
}

/** Where a shape's point named `name` is, from the sketch's points. @param {SketchView} s @param {string} name @returns {P | null} */
export function pointAt(s, name) {
  const p = s.points.find((q) => q.name === name);
  return p ? [p.x, p.y] : null;
}

/** The constraint a hold asks for, written, for what is picked — or why
 *  it cannot be. @param {string} kind @param {Picked[]} picked @param {SketchView} s
 *  @returns {{text: string} | {why: string}} */
export function holdText(kind, picked, s) {
  const shape = (/** @type {Picked} */ p) => (p.kind === "shape" ? s.shapes.find((x) => x.name === p.name) : undefined);
  const shapes = picked.map(shape).filter(Boolean);
  const points = picked.filter((p) => p.kind === "point").map((p) => /** @type {{name: string}} */ (p).name);
  const lines = shapes.filter((x) => x?.kind === "line").map((x) => /** @type {Shape} */ (x).name);
  const rounds = shapes.filter((x) => x?.kind === "circle" || x?.kind === "arc").map((x) => /** @type {Shape} */ (x).name);
  const all = picked.map((p) => (p.kind === "constraint" ? null : p.name)).filter(Boolean);
  switch (kind) {
    case "horizontal":
    case "vertical":
      if (lines.length === 1 && all.length === 1) return { text: `${kind} ${lines[0]}` };
      if (points.length === 2 && all.length === 2) return { text: `${kind} ${points[0]} ${points[1]}` };
      return { why: `${kind}: a line, or two points` };
    case "coincident":
      if (points.length === 2 && all.length === 2) return { text: `coincident ${points[0]} ${points[1]}` };
      if (points.length === 1 && shapes.length === 1) return { text: `on ${points[0]} ${/** @type {Shape} */ (shapes[0]).name}` };
      return { why: "coincident: two points, or a point and a shape" };
    case "parallel":
    case "perpendicular":
      if (lines.length === 2 && all.length === 2) return { text: `${kind} ${lines[0]} ${lines[1]}` };
      return { why: `${kind}: two lines` };
    case "tangent":
      if (shapes.length === 2 && all.length === 2 && rounds.length >= 1) return { text: `tangent ${all[0]} ${all[1]}` };
      return { why: "tangent: a line and an arc or circle, or two of those" };
    case "equal":
      if (shapes.length === 2 && all.length === 2 && (lines.length === 2 || rounds.length === 2)) return { text: `equal ${all[0]} ${all[1]}` };
      return { why: "equal: two lines, or two arcs or circles" };
    case "concentric":
      if (rounds.length === 2 && all.length === 2) return { text: `concentric ${rounds[0]} ${rounds[1]}` };
      return { why: "concentric: two arcs or circles" };
    case "fix":
      if (all.length === 1) return { text: `fix ${all[0]}` };
      return { why: "fix: one point or shape" };
    case "dimension":
      return dimensionText(picked, s);
  }
  return { why: `“${kind}” is not a constraint` };
}

/** A dimension for what is picked, at what it measures now.
 *  @param {Picked[]} picked @param {SketchView} s @returns {{text: string} | {why: string}} */
export function dimensionText(picked, s) {
  const get = (/** @type {Picked} */ p) => (p.kind === "shape" ? s.shapes.find((x) => x.name === p.name) : undefined);
  const at = (/** @type {string} */ n) => pointAt(s, n);
  const len = (/** @type {P} */ a, /** @type {P} */ b) => Math.hypot(b[0] - a[0], b[1] - a[1]);
  if (picked.length === 1) {
    const p = picked[0];
    const x = get(p);
    if (x?.kind === "line") return { text: `length ${x.name} = ${round(len(x.pts[0], x.pts[1]))}` };
    if (x?.kind === "circle") return { text: `diameter ${x.name} = ${round(2 * (x.radius ?? 0))}` };
    if (x?.kind === "arc") return { text: `radius ${x.name} = ${round(x.radius ?? 0)}` };
  }
  if (picked.length === 2) {
    const [p, q] = picked;
    if (p.kind === "point" && q.kind === "point") {
      const a = at(p.name), b = at(q.name);
      if (a && b) {
        // Level or upright as drawn: the one that varies.
        const dx = Math.abs(b[0] - a[0]), dy = Math.abs(b[1] - a[1]);
        if (dy < 1e-9 * Math.max(1, dx)) return { text: `hdistance ${p.name} ${q.name} = ${round(dx)}` };
        if (dx < 1e-9 * Math.max(1, dy)) return { text: `vdistance ${p.name} ${q.name} = ${round(dy)}` };
        return { text: `distance ${p.name} ${q.name} = ${round(len(a, b))}` };
      }
    }
    const lp = get(p), lq = get(q);
    if (lp?.kind === "line" && lq?.kind === "line") {
      const u = [lp.pts[1][0] - lp.pts[0][0], lp.pts[1][1] - lp.pts[0][1]], v = [lq.pts[1][0] - lq.pts[0][0], lq.pts[1][1] - lq.pts[0][1]];
      const c = (u[0] * v[0] + u[1] * v[1]) / (Math.hypot(u[0], u[1]) * Math.hypot(v[0], v[1]));
      return { text: `angle ${lp.name} ${lq.name} = ${round((Math.acos(Math.max(-1, Math.min(1, c))) * 180) / Math.PI)}` };
    }
    const point = p.kind === "point" ? p : q.kind === "point" ? q : null;
    const line = lp?.kind === "line" ? lp : lq?.kind === "line" ? lq : null;
    if (point && point.kind === "point" && line) {
      const a = at(point.name);
      if (a) {
        const [s0, s1] = line.pts, d = [s1[0] - s0[0], s1[1] - s0[1]];
        const off = Math.abs((a[0] - s0[0]) * d[1] - (a[1] - s0[1]) * d[0]) / Math.hypot(d[0], d[1]);
        return { text: `distance ${point.name} ${line.name} = ${round(off)}` };
      }
    }
  }
  return { why: "a dimension: a line, a circle, an arc, two points, a point and a line, or two lines" };
}

/** A shape's text, construction or not. @param {string} text */
export function toggleConstruction(text) {
  return text.startsWith("construction ") ? text.slice("construction ".length) : `construction ${text}`;
}

/** A clamped uniform cubic B-spline through its control points' hull, as
 *  points to draw it by. @param {P[]} cs */
export function splinePoints(cs) {
  const n = cs.length;
  if (n < 3) return cs;
  const degree = Math.min(3, n - 1);
  const spans = n - degree;
  const knots = [...Array(degree + 1).fill(0), ...Array.from({ length: spans - 1 }, (_, i) => i + 1), ...Array(degree + 1).fill(spans)];
  const at = (/** @type {number} */ t) => {
    let k = degree;
    while (k < n - 1 && t >= knots[k + 1]) k++;
    const d = cs.slice(k - degree, k + 1).map((p) => [p[0], p[1]]);
    for (let r = 1; r <= degree; r++) {
      for (let j = degree; j >= r; j--) {
        const i = k - degree + j;
        const a = (t - knots[i]) / (knots[i + degree - r + 1] - knots[i] || 1);
        d[j] = [(1 - a) * d[j - 1][0] + a * d[j][0], (1 - a) * d[j - 1][1] + a * d[j][1]];
      }
    }
    return /** @type {P} */ (d[degree]);
  };
  const out = [];
  const steps = 16 * spans;
  for (let i = 0; i <= steps; i++) out.push(at(Math.min((spans * i) / steps, spans - 1e-12)));
  return out;
}

/** @param {{svg: SVGSVGElement, input: HTMLInputElement, edit: (ops: Op[]) => void,
 *   drag: (point: string, x: number, y: number, done: boolean) => void, status: (text: string) => void, done: () => void}} host */
export function createSketcher(host) {
  const { svg, input } = host;
  const NS = "http://www.w3.org/2000/svg";
  const state = {
    /** @type {SketchView | null} */ sketch: null,
    view: { cx: 0, cy: 0, k: 4 },
    tool: "select",
    /** Points of the shape being drawn. @type {{name?: string, at: P}[]} */ placed: [],
    /** Where the pointer is, in the sketch. @type {P | null} */ hover: null,
    /** @type {Picked[]} */ picked: [],
    /** A point being dragged, and whether it has moved. @type {{name: string, moved: boolean, from: P} | null} */ dragging: null,
    /** A pan under way, from where. @type {{x: number, y: number, cx: number, cy: number, moved: boolean} | null} */ panning: null,
    /** The dimension being typed into. @type {Constraint | null} */ editing: null,
    /** Names given since the sketch was opened: not given again before the
     *  core's answer, with them in it, comes back. @type {Set<string>} */ issued: new Set(),
    fitted: "",
  };
  let dragFrame = 0, queued = /** @type {P | null} */ (null);

  // By its box: an <svg>'s clientWidth is 0 in some engines.
  const size = () => {
    const r = svg.getBoundingClientRect();
    return { w: r.width || 600, h: r.height || 400 };
  };
  /** @param {P} p @returns {P} */
  const toScreen = (p) => {
    const { w, h } = size(), v = state.view;
    return [w / 2 + (p[0] - v.cx) * v.k, h / 2 - (p[1] - v.cy) * v.k];
  };
  /** @param {number} sx @param {number} sy @returns {P} */
  const toWorld = (sx, sy) => {
    const { w, h } = size(), v = state.view;
    return [v.cx + (sx - w / 2) / v.k, v.cy - (sy - h / 2) / v.k];
  };
  /** Keep the pointer while dragging; a pointer that is not one (a
   *  test's) has nothing to keep. @param {PointerEvent} e */
  const capture = (e) => {
    try {
      capture(e);
    } catch {}
  };
  const local = (/** @type {MouseEvent} */ e) => {
    const r = svg.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  /** The grid's step: a round number at least 18 pixels apart. */
  function gridStep() {
    const want = 18 / state.view.k;
    const base = 10 ** Math.floor(Math.log10(want));
    for (const m of [1, 2, 5, 10]) if (base * m >= want) return base * m;
    return base * 10;
  }

  /** A point where the pointer is: an existing point near it, by name, or
   *  the place rounded to a tenth of the grid. @param {number} sx @param {number} sy */
  function snapAt(sx, sy) {
    const hit = nearestPoint(sx, sy);
    if (hit) return { name: hit.name, at: /** @type {P} */ ([hit.x, hit.y]) };
    const [x, y] = toWorld(sx, sy);
    const g = gridStep() / 10;
    return { at: /** @type {P} */ ([Math.round(x / g) * g, Math.round(y / g) * g]) };
  }

  /** The sketch's points, one a place: the first name of each. */
  function handles() {
    const s = state.sketch;
    if (!s) return [];
    const seen = new Map();
    for (const p of s.points) {
      if (p.name === "xaxis" || p.name === "yaxis") continue;
      const key = `${p.x.toFixed(9)},${p.y.toFixed(9)}`;
      const had = seen.get(key);
      // A shape's own corner names read better than a line's ends.
      if (!had || (had.name.includes(".") && !p.name.includes(".")) || (p.name === "origin")) seen.set(key, { ...p, names: [...(had?.names ?? []), p.name] });
      else had.names.push(p.name);
    }
    return [...seen.values()];
  }

  /** @param {number} sx @param {number} sy */
  function nearestPoint(sx, sy) {
    let best = null, bestD = NEAR;
    for (const p of handles()) {
      const [px, py] = toScreen([p.x, p.y]);
      const d = Math.hypot(px - sx, py - sy);
      if (d < bestD) {
        best = p;
        bestD = d;
      }
    }
    return best;
  }

  /** The shape nearest the pointer, within reach. @param {number} sx @param {number} sy */
  function nearestShape(sx, sy) {
    const s = state.sketch;
    if (!s) return null;
    const w = toWorld(sx, sy), reach = NEAR / state.view.k;
    let best = null, bestD = reach;
    for (const x of s.shapes) {
      let d = Infinity;
      if (x.kind === "line") d = segment(w, x.pts[0], x.pts[1]);
      else if (x.kind === "circle") d = Math.abs(Math.hypot(w[0] - x.pts[0][0], w[1] - x.pts[0][1]) - (x.radius ?? 0));
      else if (x.kind === "arc") d = arcDistance(w, x);
      else if (x.kind === "spline") d = Math.min(...splinePoints(x.pts).slice(1).map((q, i, all) => segment(w, i === 0 ? x.pts[0] : all[i - 1], q)));
      else if (x.kind === "point") d = Math.hypot(w[0] - x.pts[0][0], w[1] - x.pts[0][1]);
      if (d < bestD) {
        best = x;
        bestD = d;
      }
    }
    return best;
  }

  /* ── drawing ──────────────────────────────────────────────────── */

  /** @param {string} tag @param {Record<string, string | number>} attrs @param {SVGElement} [into] */
  function el(tag, attrs, into) {
    const e = document.createElementNS(NS, tag);
    for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, String(v));
    (into ?? svg).append(e);
    return e;
  }

  const isPicked = (/** @type {Picked} */ p) => state.picked.some((q) => q.kind === p.kind && (q.kind === "constraint" ? p.kind === "constraint" && q.item === p.item : p.kind !== "constraint" && q.name === p.name));

  function render() {
    svg.replaceChildren();
    const s = state.sketch;
    const { w, h } = size();
    // The grid, and the axes.
    const step = gridStep();
    const [x0, y1] = toWorld(0, 0), [x1, y0] = toWorld(w, h);
    const grid = el("g", { class: "cd-grid" });
    for (let x = Math.ceil(x0 / step) * step; x <= x1; x += step) {
      const [sx] = toScreen([x, 0]);
      el("line", { x1: sx, y1: 0, x2: sx, y2: h, class: Math.abs(Math.round(x / step) % 5) === 0 ? "major" : "" }, grid);
    }
    for (let y = Math.ceil(y0 / step) * step; y <= y1; y += step) {
      const [, sy] = toScreen([0, y]);
      el("line", { x1: 0, y1: sy, x2: w, y2: sy, class: Math.abs(Math.round(y / step) % 5) === 0 ? "major" : "" }, grid);
    }
    const [ox, oy] = toScreen([0, 0]);
    el("line", { x1: 0, y1: oy, x2: w, y2: oy, class: "cd-axis x" });
    el("line", { x1: ox, y1: 0, x2: ox, y2: h, class: "cd-axis y" });
    if (!s) return;
    const defined = s.solved && s.dof === 0;
    const shapes = el("g", { class: `cd-shapes${defined ? " defined" : ""}${s.solved ? "" : " broken"}` });
    for (const x of s.shapes) {
      const cls = `cd-shape ${x.kind}${x.construction ? " construction" : ""}${isPicked({ kind: "shape", name: x.name }) ? " picked" : ""}`;
      let e;
      if (x.kind === "line") {
        const [a, b] = [toScreen(x.pts[0]), toScreen(x.pts[1])];
        e = el("line", { x1: a[0], y1: a[1], x2: b[0], y2: b[1], class: cls }, shapes);
      } else if (x.kind === "circle") {
        const [c] = [toScreen(x.pts[0])];
        e = el("circle", { cx: c[0], cy: c[1], r: (x.radius ?? 0) * state.view.k, class: cls }, shapes);
      } else if (x.kind === "arc") {
        e = el("path", { d: arcPath(x), class: cls }, shapes);
      } else if (x.kind === "spline") {
        e = el("polyline", { points: splinePoints(x.pts).map((p) => toScreen(p).join(",")).join(" "), class: cls }, shapes);
        el("polyline", { points: x.pts.map((p) => toScreen(p).join(",")).join(" "), class: "cd-hull" }, shapes);
      } else {
        const [p] = [toScreen(x.pts[0])];
        e = el("circle", { cx: p[0], cy: p[1], r: 2.5, class: cls }, shapes);
      }
      e.dataset.shape = x.name;
    }
    // Dimensions, and the constraints' marks.
    const marks = el("g", { class: "cd-marks" });
    for (const c of s.constraints) drawConstraint(c, marks, s);
    // The points: free ones the solver could still move.
    const free = new Set(s.free);
    const dots = el("g", { class: "cd-points" });
    for (const p of handles()) {
      const [sx, sy] = toScreen([p.x, p.y]);
      const d = el("circle", { cx: sx, cy: sy, r: p.name === "origin" ? 3 : 3.5, class: `cd-point${p.names.some((/** @type {string} */ n) => free.has(n)) ? " free" : ""}${p.name === "origin" ? " origin" : ""}${p.names.some((/** @type {string} */ n) => isPicked({ kind: "point", name: n })) ? " picked" : ""}` }, dots);
      d.dataset.point = p.name;
    }
    drawRubber();
  }

  /** @param {Shape} x */
  function arcPath(x) {
    const [c, a, b] = x.pts;
    const r = x.radius ?? 0;
    const a0 = Math.atan2(a[1] - c[1], a[0] - c[0]);
    let a1 = Math.atan2(b[1] - c[1], b[0] - c[0]);
    while (a1 <= a0) a1 += 2 * Math.PI;
    const [sa, sb] = [toScreen(a), toScreen([c[0] + r * Math.cos(a1), c[1] + r * Math.sin(a1)])];
    const big = a1 - a0 > Math.PI ? 1 : 0;
    // Anticlockwise in the sketch is clockwise on a screen whose y runs down.
    return `M${sa[0]},${sa[1]} A${r * state.view.k},${r * state.view.k} 0 ${big} 1 ${sb[0]},${sb[1]}`;
  }

  const GLYPH = /** @type {Record<string, string>} */ ({ horizontal: "H", vertical: "V", parallel: "∥", perpendicular: "⊥", tangent: "T", equal: "=", concentric: "◎", fix: "⚓", coincident: "⊙", on: "⊙", midpoint: "M", symmetric: "⇹" });

  /** Where a named thing is drawn, for a mark beside it. @param {string} name @param {SketchView} s @returns {P | null} */
  function anchor(name, s) {
    const x = s.shapes.find((y) => y.name === name);
    if (x?.kind === "line") return [(x.pts[0][0] + x.pts[1][0]) / 2, (x.pts[0][1] + x.pts[1][1]) / 2];
    if (x?.kind === "circle" || x?.kind === "arc") {
      const r = x.radius ?? 0;
      return [x.pts[0][0] + r * Math.SQRT1_2, x.pts[0][1] + r * Math.SQRT1_2];
    }
    if (x) return x.pts[0];
    return pointAt(s, name);
  }

  /** @param {Constraint} c @param {SVGElement} into @param {SketchView} s */
  function drawConstraint(c, into, s) {
    const cls = `${c.conflicting ? " conflict" : ""}${c.redundant ? " redundant" : ""}${isPicked({ kind: "constraint", item: c.item }) ? " picked" : ""}`;
    if (c.value != null || c.which) {
      const g = el("g", { class: `cd-dim${cls}` }, into);
      g.dataset.dim = String(c.item);
      if (c.which) g.dataset.which = c.which;
      drawDimension(c, g, s);
      return;
    }
    const glyph = GLYPH[c.kind] ?? c.kind;
    c.refs.forEach((r, i) => {
      const at = anchor(r, s);
      if (!at) return;
      const [sx, sy] = toScreen(at);
      const g = el("g", { class: `cd-glyph${cls}` }, into);
      g.dataset.constraint = String(c.item);
      el("rect", { x: sx + 6 + i * 2, y: sy - 18, width: 14, height: 14, rx: 3 }, g);
      el("text", { x: sx + 13 + i * 2, y: sy - 8, "text-anchor": "middle" }, g).textContent = glyph;
    });
  }

  /** A dimension: a line with its value, a leader to a circle, an arc for
   *  an angle. @param {Constraint} c @param {SVGElement} g @param {SketchView} s */
  function drawDimension(c, g, s) {
    const value = c.value ?? "";
    const label = (/** @type {P} */ at, /** @type {string} */ text) => {
      const t = el("text", { x: at[0], y: at[1], "text-anchor": "middle", class: "cd-dim-text" }, g);
      t.textContent = text;
      return t;
    };
    const shape = s.shapes.find((x) => x.name === c.refs[0]);
    const between = (/** @type {P} */ a, /** @type {P} */ b, /** @type {string} */ text, flip = 1) => {
      const [sa, sb] = [toScreen(a), toScreen(b)];
      const d = [sb[0] - sa[0], sb[1] - sa[1]], n = Math.hypot(d[0], d[1]) || 1;
      const off = [(-d[1] / n) * 16 * flip, (d[0] / n) * 16 * flip];
      const [pa, pb] = [[sa[0] + off[0], sa[1] + off[1]], [sb[0] + off[0], sb[1] + off[1]]];
      el("line", { x1: sa[0], y1: sa[1], x2: pa[0], y2: pa[1], class: "cd-ext" }, g);
      el("line", { x1: sb[0], y1: sb[1], x2: pb[0], y2: pb[1], class: "cd-ext" }, g);
      el("line", { x1: pa[0], y1: pa[1], x2: pb[0], y2: pb[1], class: "cd-dline" }, g);
      label([(pa[0] + pb[0]) / 2 + off[0] * 0.4, (pa[1] + pb[1]) / 2 + off[1] * 0.4 + 4], text);
    };
    const kind = c.kind;
    if ((kind === "radius" || kind === "diameter") && shape) {
      const r = shape.radius ?? 0, cpt = shape.pts[0];
      const edge = /** @type {P} */ ([cpt[0] + r * Math.SQRT1_2, cpt[1] + r * Math.SQRT1_2]);
      const [sc, se] = [toScreen(cpt), toScreen(edge)];
      el("line", { x1: sc[0], y1: sc[1], x2: se[0] + 14, y2: se[1] - 14, class: "cd-dline" }, g);
      label([se[0] + 22, se[1] - 18], `${kind === "radius" ? "R" : "⌀"}${value}`);
      return;
    }
    if (kind === "length" && shape?.kind === "line") return between(shape.pts[0], shape.pts[1], value);
    if ((kind === "width" || kind === "height") && shape) {
      // A rectangle's own size: along its bottom, and up its left.
      const rect = c.refs[0];
      const bl = pointAt(s, `${rect}.bl`), br = pointAt(s, `${rect}.br`), tl = pointAt(s, `${rect}.tl`);
      if (kind === "width" && bl && br) return between(bl, br, value, -1);
      if (kind === "height" && bl && tl) return between(bl, tl, value);
      return;
    }
    if (kind === "angle") {
      const [l1, l2] = c.refs.map((r) => s.shapes.find((x) => x.name === r));
      if (l1 && l2) {
        const m = /** @type {P} */ ([(l1.pts[0][0] + l1.pts[1][0] + l2.pts[0][0] + l2.pts[1][0]) / 4, (l1.pts[0][1] + l1.pts[1][1] + l2.pts[0][1] + l2.pts[1][1]) / 4]);
        label(toScreen(m), `${value}°`);
      }
      return;
    }
    // Between two things: points, or a point and a line.
    const [a, b] = c.refs.map((r) => pointAt(s, r) ?? anchor(r, s));
    if (!a || !b) return;
    if (kind === "hdistance") return between(a, [b[0], a[1]], value, -1);
    if (kind === "vdistance") return between(a, [a[0], b[1]], value);
    between(a, b, value);
  }

  function drawRubber() {
    const s = state.sketch;
    if (!s || !state.hover || state.tool === "select" || !state.placed.length) return;
    const g = el("g", { class: "cd-rubber" });
    const h = state.hover;
    const p0 = state.placed[0].at;
    const [a, b] = [toScreen(state.placed[state.placed.length - 1].at), toScreen(h)];
    if (state.tool === "line") el("line", { x1: a[0], y1: a[1], x2: b[0], y2: b[1] }, g);
    if (state.tool === "rect") {
      const [c0, c1] = [toScreen(p0), b];
      el("rect", { x: Math.min(c0[0], c1[0]), y: Math.min(c0[1], c1[1]), width: Math.abs(c1[0] - c0[0]), height: Math.abs(c1[1] - c0[1]) }, g);
    }
    if (state.tool === "circle") el("circle", { cx: toScreen(p0)[0], cy: toScreen(p0)[1], r: Math.hypot(h[0] - p0[0], h[1] - p0[1]) * state.view.k }, g);
    if (state.tool === "arc") {
      const c = toScreen(p0);
      const r = state.placed.length > 1 ? Math.hypot(state.placed[1].at[0] - p0[0], state.placed[1].at[1] - p0[1]) : Math.hypot(h[0] - p0[0], h[1] - p0[1]);
      el("circle", { cx: c[0], cy: c[1], r: r * state.view.k, class: "faint" }, g);
      el("line", { x1: c[0], y1: c[1], x2: b[0], y2: b[1] }, g);
    }
  }

  /* ── what the tools write ─────────────────────────────────────── */

  const add = (/** @type {string[]} */ lines) => {
    const s = state.sketch;
    if (s) host.edit([{ op: "sketchAdd", sketch: s.index, text: lines.join("\n") }]);
  };

  /** A point placed with the current tool. @param {{name?: string, at: P}} p */
  function place(p) {
    const s = state.sketch;
    if (!s) return;
    const taken = [...names(s), ...state.issued];
    const fresh_ = (/** @type {string} */ prefix) => {
      const n = fresh(prefix, taken);
      state.issued.add(n);
      taken.push(n);
      return n;
    };
    if (state.tool === "point") {
      if (!p.name) add([`point ${fresh_("p")} (${num(p.at[0])}, ${num(p.at[1])})`]);
      return;
    }
    state.placed.push(p);
    const n = state.placed.length;
    if (state.tool === "line" && n >= 2) {
      const [a, b] = state.placed.slice(-2);
      if (Math.hypot(b.at[0] - a.at[0], b.at[1] - a.at[1]) === 0) {
        state.placed.pop();
        return;
      }
      const name = fresh_("l");
      const lines = [`line ${name} ${spot(a)} ${spot(b)}`];
      const lv = level(a.at, b.at);
      if (lv && !(a.name && b.name)) lines.push(`${lv} ${name}`);
      add(lines);
      // On from where it ended, unless it closed on its start.
      const closed = b.name && state.placed[0].name === b.name && n > 2;
      state.placed = closed ? [] : [{ name: `${name}.end`, at: b.at }];
      if (closed) host.status("Closed. Click to start another run of lines; ⎋ to stop.");
    }
    if (state.tool === "rect" && n === 2) {
      const [a, b] = state.placed;
      state.placed = [];
      if (a.at[0] !== b.at[0] && a.at[1] !== b.at[1]) add([rectText(fresh_("r"), a, b.at)]);
    }
    if (state.tool === "circle" && n === 2) {
      const [c, e] = state.placed;
      state.placed = [];
      const r = Math.hypot(e.at[0] - c.at[0], e.at[1] - c.at[1]);
      if (r > 0) add([`circle ${fresh_("c")} center ${spot(c)} radius ${num(r)}`]);
    }
    if (state.tool === "arc" && n === 3) {
      const [c, a, b] = state.placed;
      state.placed = [];
      const r = Math.hypot(a.at[0] - c.at[0], a.at[1] - c.at[1]);
      const t = Math.atan2(b.at[1] - c.at[1], b.at[0] - c.at[0]);
      const end = b.name ? b : { at: /** @type {P} */ ([c.at[0] + r * Math.cos(t), c.at[1] + r * Math.sin(t)]) };
      if (r > 0) add([`arc ${fresh_("a")} center ${spot(c)} from ${spot(a)} to ${spot(end)}`]);
    }
    render();
  }

  /** @param {string} kind */
  function hold(kind) {
    const s = state.sketch;
    if (!s) return;
    if (kind === "construction") {
      const items = new Set(state.picked.filter((p) => p.kind === "shape").map((p) => s.shapes.find((x) => x.name === /** @type {any} */ (p).name)).filter(Boolean).map((x) => /** @type {Shape} */ (x).item));
      if (!items.size) return host.status("Construction: choose a shape first.");
      host.edit([...items].map((item) => ({ op: "sketchReplace", sketch: s.index, index: item, text: toggleConstruction(/** @type {Shape} */ (s.shapes.find((x) => x.item === item)).text) })));
      return;
    }
    const got = holdText(kind, state.picked, s);
    if ("why" in got) return host.status(`${got.why[0].toUpperCase()}${got.why.slice(1)}.`);
    add([got.text]);
    state.picked = [];
  }

  function remove() {
    const s = state.sketch;
    if (!s || !state.picked.length) return;
    const items = new Set();
    for (const p of state.picked) {
      if (p.kind === "constraint") items.add(p.item);
      else if (p.kind === "shape") {
        const x = s.shapes.find((y) => y.name === p.name);
        if (x) items.add(x.item);
      }
    }
    state.picked = [];
    if (items.size) host.edit([{ op: "sketchRemove", sketch: s.index, indices: [...items] }]);
  }

  /** Type over a dimension's value, in place. @param {Constraint} c @param {number} sx @param {number} sy */
  function editValue(c, sx, sy) {
    state.editing = c;
    input.hidden = false;
    input.value = c.value ?? "";
    input.style.left = `${sx - 40}px`;
    input.style.top = `${sy - 14}px`;
    input.focus();
    input.select();
  }

  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      const c = state.editing, s = state.sketch;
      if (c && s && input.value.trim()) host.edit([{ op: "setValue", sketch: s.index, index: c.item, which: c.which ?? null, value: input.value.trim() }]);
      state.editing = null;
      input.hidden = true;
      svg.focus();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      state.editing = null;
      input.hidden = true;
      svg.focus();
    }
  });
  input.addEventListener("blur", () => {
    state.editing = null;
    input.hidden = true;
  });

  /* ── the pointer ──────────────────────────────────────────────── */

  svg.addEventListener("pointerdown", (e) => {
    if (!state.sketch) return;
    svg.focus();
    const { x, y } = local(e);
    if (e.button === 1 || (e.button === 0 && e.altKey)) {
      state.panning = { x, y, cx: state.view.cx, cy: state.view.cy, moved: false };
      capture(e);
      return;
    }
    if (e.button === 2) {
      state.placed = [];
      render();
      return;
    }
    const target = /** @type {Element} */ (e.target);
    const dim = target.closest?.("[data-dim]");
    if (dim && state.tool === "select") {
      const item = Number(/** @type {HTMLElement} */ (dim).dataset.dim), which = /** @type {HTMLElement} */ (dim).dataset.which ?? null;
      const c = state.sketch.constraints.find((k) => k.item === item && (k.which ?? null) === which);
      if (c) {
        if (e.detail >= 2 || target.matches(".cd-dim-text")) editValue(c, x, y);
        else pick({ kind: "constraint", item }, e.shiftKey);
      }
      return;
    }
    const glyph = target.closest?.("[data-constraint]");
    if (glyph && state.tool === "select") {
      pick({ kind: "constraint", item: Number(/** @type {HTMLElement} */ (glyph).dataset.constraint) }, e.shiftKey);
      return;
    }
    if (state.tool !== "select") {
      place(snapAt(x, y));
      return;
    }
    const p = nearestPoint(x, y);
    if (p && p.name !== "origin") {
      state.dragging = { name: p.name, moved: false, from: [p.x, p.y] };
      capture(e);
      return;
    }
    const sh = nearestShape(x, y);
    if (sh) return pick({ kind: "shape", name: sh.name }, e.shiftKey);
    if (p) return pick({ kind: "point", name: p.name }, e.shiftKey);
    state.panning = { x, y, cx: state.view.cx, cy: state.view.cy, moved: false };
    capture(e);
  });

  svg.addEventListener("pointermove", (e) => {
    if (!state.sketch) return;
    const { x, y } = local(e);
    if (state.panning) {
      const dx = x - state.panning.x, dy = y - state.panning.y;
      if (Math.hypot(dx, dy) > 2) state.panning.moved = true;
      state.view.cx = state.panning.cx - dx / state.view.k;
      state.view.cy = state.panning.cy + dy / state.view.k;
      render();
      return;
    }
    if (state.dragging) {
      const w = toWorld(x, y);
      if (!state.dragging.moved && Math.hypot(w[0] - state.dragging.from[0], w[1] - state.dragging.from[1]) * state.view.k < 3) return;
      state.dragging.moved = true;
      queued = w;
      if (!dragFrame) {
        dragFrame = requestAnimationFrame(() => {
          dragFrame = 0;
          if (state.dragging && queued) host.drag(state.dragging.name, queued[0], queued[1], false);
        });
      }
      return;
    }
    state.hover = snapAt(x, y).at;
    if (state.tool !== "select") render();
  });

  svg.addEventListener("pointerup", (e) => {
    const { x, y } = local(e);
    if (state.panning) {
      if (!state.panning.moved) {
        state.picked = [];
        render();
      }
      state.panning = null;
      return;
    }
    if (state.dragging) {
      const d = state.dragging;
      state.dragging = null;
      if (dragFrame) cancelAnimationFrame(dragFrame), (dragFrame = 0);
      if (d.moved) {
        const w = toWorld(x, y);
        host.drag(d.name, w[0], w[1], true);
      } else {
        pick({ kind: "point", name: d.name }, e.shiftKey);
      }
    }
  });

  svg.addEventListener("contextmenu", (e) => e.preventDefault());

  svg.addEventListener("wheel", (e) => {
    if (!state.sketch) return;
    e.preventDefault();
    const { x, y } = local(e);
    const before = toWorld(x, y);
    const k = Math.min(1e6, Math.max(1e-4, state.view.k * Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.0015))));
    state.view.k = k;
    const after = toWorld(x, y);
    state.view.cx += before[0] - after[0];
    state.view.cy += before[1] - after[1];
    render();
  }, { passive: false });

  /** @param {Picked} p @param {boolean} more */
  function pick(p, more) {
    if (more) state.picked = isPicked(p) ? state.picked.filter((q) => !(q.kind === p.kind && JSON.stringify(q) === JSON.stringify(p))) : [...state.picked, p];
    else state.picked = isPicked(p) && state.picked.length === 1 ? [] : [p];
    render();
    say();
  }

  function say() {
    const s = state.sketch;
    if (!s) return;
    if (state.tool !== "select") {
      const hint = /** @type {Record<string, string>} */ ({
        line: "Click where a line starts, then where each ends; ⎋ stops. A point drawn on another starts from it.",
        rect: "Click one corner, then the other.",
        circle: "Click the centre, then a point on the circle.",
        arc: "Click the centre, then the start, then the end — anticlockwise.",
        point: "Click to place a point.",
      })[state.tool];
      return host.status(hint ?? "");
    }
    if (state.picked.length) return host.status(`${state.picked.map((p) => (p.kind === "constraint" ? s.constraints.find((c) => c.item === p.item)?.text ?? "" : p.name)).join(", ")} — ⌫ to take out, or a constraint above`);
    host.status("Drag a point to move it; click shapes and points to choose them, then a constraint or a dimension above. Double-click a dimension to type its value.");
  }

  /** Fit the sketch in view. */
  function fit() {
    const s = state.sketch;
    if (!s) return;
    let lo = [Infinity, Infinity], hi = [-Infinity, -Infinity];
    const add = (/** @type {P} */ p, r = 0) => {
      lo = [Math.min(lo[0], p[0] - r), Math.min(lo[1], p[1] - r)];
      hi = [Math.max(hi[0], p[0] + r), Math.max(hi[1], p[1] + r)];
    };
    for (const x of s.shapes) for (const p of x.pts) add(p, x.kind === "circle" ? x.radius ?? 0 : 0);
    add([0, 0]);
    if (!Number.isFinite(lo[0]) || (hi[0] - lo[0] < 1e-9 && hi[1] - lo[1] < 1e-9)) {
      lo = [-50, -50];
      hi = [50, 50];
    }
    const { w, h } = size();
    state.view.cx = (lo[0] + hi[0]) / 2;
    state.view.cy = (lo[1] + hi[1]) / 2;
    state.view.k = Math.min(w / ((hi[0] - lo[0]) * 1.4 || 1), h / ((hi[1] - lo[1]) * 1.4 || 1));
    render();
  }

  /** @param {string} t */
  function setTool(t) {
    state.tool = t;
    state.placed = [];
    render();
    say();
  }

  return {
    /** @param {SketchView} s */
    show(s) {
      const first = state.fitted !== `${s.index}:${s.name}`;
      state.sketch = s;
      // What was picked and is gone is not picked.
      state.picked = state.picked.filter((p) => (p.kind === "constraint" ? s.constraints.some((c) => c.item === p.item) : p.kind === "shape" ? s.shapes.some((x) => x.name === p.name) : s.points.some((q) => q.name === p.name)));
      if (first) {
        state.fitted = `${s.index}:${s.name}`;
        state.tool = "select";
        state.placed = [];
        fit();
      } else render();
      if (first) say();
    },
    leave() {
      state.sketch = null;
      state.fitted = "";
      state.issued.clear();
      state.placed = [];
      state.picked = [];
      svg.replaceChildren();
    },
    tool: setTool,
    hold,
    remove,
    fit,
    resize: render,
    /** A key while the sketch has the keyboard: whether it was the sketch's.
     *  @param {KeyboardEvent} e */
    key(e) {
      if (e.metaKey || e.ctrlKey || e.altKey) return false;
      if (e.key === "Escape") {
        if (state.placed.length) {
          state.placed = [];
          render();
        } else if (state.tool !== "select") setTool("select");
        else if (state.picked.length) {
          state.picked = [];
          render();
        } else host.done();
        return true;
      }
      if (e.key === "Delete" || e.key === "Backspace") {
        remove();
        return true;
      }
      const tools = /** @type {Record<string, string>} */ ({ s: "select", l: "line", r: "rect", c: "circle", a: "arc", p: "point" });
      const holds = /** @type {Record<string, string>} */ ({ h: "horizontal", v: "vertical", d: "dimension", x: "construction" });
      const k = e.key.toLowerCase();
      if (tools[k]) {
        setTool(tools[k]);
        return true;
      }
      if (holds[k]) {
        hold(holds[k]);
        return true;
      }
      if (k === "f") {
        fit();
        return true;
      }
      return false;
    },
    /** What a test asks: what is chosen, the tool, the view.
     *  @returns {{tool: string, picked: Picked[], placed: number, view: {cx: number, cy: number, k: number}}} */
    debug() {
      return { tool: state.tool, picked: state.picked, placed: state.placed.length, view: { ...state.view } };
    },
    /** Where a sketch point is on screen, for a test's pointer. @param {P} p */
    screen: toScreen,
  };
}

/** @param {P} p @param {P} a @param {P} b */
function segment(p, a, b) {
  const d = [b[0] - a[0], b[1] - a[1]], l2 = d[0] * d[0] + d[1] * d[1];
  const t = l2 ? Math.max(0, Math.min(1, ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2)) : 0;
  return Math.hypot(p[0] - a[0] - t * d[0], p[1] - a[1] - t * d[1]);
}

/** @param {P} p @param {Shape} x */
function arcDistance(p, x) {
  const [c, a, b] = x.pts, r = x.radius ?? 0;
  const a0 = Math.atan2(a[1] - c[1], a[0] - c[0]);
  let a1 = Math.atan2(b[1] - c[1], b[0] - c[0]);
  while (a1 <= a0) a1 += 2 * Math.PI;
  let t = Math.atan2(p[1] - c[1], p[0] - c[0]);
  while (t < a0) t += 2 * Math.PI;
  if (t <= a1) return Math.abs(Math.hypot(p[0] - c[0], p[1] - c[1]) - r);
  return Math.min(Math.hypot(p[0] - a[0], p[1] - a[1]), Math.hypot(p[0] - b[0], p[1] - b[1]));
}

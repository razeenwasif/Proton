/* Game mode's overlay: what a room's art does not show — its edge, what is
 * solid and how, where things stand and what they do — drawn on a 2D canvas
 * over the stage, and what a click there lands on.
 *
 * Kept apart from the art's renderer on purpose: the lines a designer edits
 * by are the same whether the art beneath is drawn or not, and the pure
 * geometry here (fitting, picking) is tested without a canvas. */

/** @typedef {import('./bindings/GameRoom').GameRoom} Room */
/** @typedef {{kind: string, id: string, plane?: string}} Selected */
/** @typedef {{scale: number, x: number, y: number}} View */
/** @typedef {{text: string, muted: string, faint: string, accent: string, ok: string, warn: string, err: string, ground: string}} Colors */

/** How far, in stage pixels, a click may miss a point or a line and still land on it. */
const REACH = 10;

/** The room fitted to a stage `width` by `height`, with a margin round it.
 *  @param {Room} room @param {number} width @param {number} height @returns {View} */
export function fit(room, width, height, margin = 24) {
  const [w, h] = room.size;
  const scale = Math.max(0.01, Math.min((width - margin * 2) / Math.max(1, w), (height - margin * 2) / Math.max(1, h)));
  return { scale, x: (width - w * scale) / 2, y: (height - h * scale) / 2 };
}

/** A room point on the stage. @param {View} view @param {number[]} p @returns {[number, number]} */
export function toStage(view, p) {
  return [view.x + p[0] * view.scale, view.y + p[1] * view.scale];
}

/** A stage point in the room. @param {View} view @param {number} sx @param {number} sy @returns {[number, number]} */
export function toRoom(view, sx, sy) {
  return [(sx - view.x) / view.scale, (sy - view.y) / view.scale];
}

/** How far a point is from a segment. */
function toSegment(/** @type {number} */ px, /** @type {number} */ py, /** @type {number[]} */ a, /** @type {number[]} */ b) {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const length = dx * dx + dy * dy;
  const t = length === 0 ? 0 : Math.max(0, Math.min(1, ((px - a[0]) * dx + (py - a[1]) * dy) / length));
  return Math.hypot(px - (a[0] + t * dx), py - (a[1] + t * dy));
}

/** What a click on the stage at `sx`, `sy` lands on: the thing nearest, of
 *  the things within reach — standing things before lines, since a line runs
 *  under everything. @param {Room} room @param {View} view @param {number} sx @param {number} sy
 *  @returns {Selected | null} */
export function pick(room, view, sx, sy) {
  const near = (/** @type {number[]} */ p) => {
    const [x, y] = toStage(view, p);
    return Math.hypot(x - sx, y - sy);
  };
  /** @type {{at: number, picked: Selected}[]} */
  const hits = [];
  for (const e of room.entities) {
    if (e.rect) {
      const [x, y] = toStage(view, [e.rect[0], e.rect[1]]);
      const w = e.rect[2] * view.scale;
      const h = e.rect[3] * view.scale;
      if (sx >= x - 2 && sx <= x + w + 2 && sy >= y - 2 && sy <= y + h + 2) hits.push({ at: 0, picked: { kind: "entity", id: e.id } });
    } else if (e.at && near(e.at) <= REACH) {
      hits.push({ at: near(e.at), picked: { kind: "entity", id: e.id } });
    }
  }
  for (const s of room.spawns) if (near(s.at) <= REACH) hits.push({ at: near(s.at), picked: { kind: "spawn", id: s.id } });
  for (const l of room.lights) if (near(l.at) <= REACH) hits.push({ at: near(l.at), picked: { kind: "light", id: l.id } });
  if (hits.length) return hits.sort((a, b) => a.at - b.at)[0].picked;
  const [rx, ry] = toRoom(view, sx, sy);
  let best = /** @type {Selected | null} */ (null);
  let bestAt = REACH / view.scale;
  for (const shape of room.collision) {
    const points = shape.closed ? [...shape.points, shape.points[0]] : shape.points;
    for (let i = 1; i < points.length; i++) {
      const at = toSegment(rx, ry, points[i - 1], points[i]);
      if (at <= bestAt) {
        bestAt = at;
        best = { kind: "shape", id: shape.id };
      }
    }
  }
  return best;
}

/** The colour a built-in kind of entity is drawn in. @param {string} prefab @param {Colors} c */
function tone(prefab, c) {
  switch (prefab) {
    case "transition": return c.ok;
    case "save_point": return c.ok;
    case "ability_gate": return c.warn;
    case "ability_pickup": return c.accent;
    default: return c.text;
  }
}

/** Draw a room's outline, collision, entities, spawns and lights.
 *  @param {CanvasRenderingContext2D} g @param {Room} room @param {View} view @param {Colors} c
 *  @param {Selected | null} selected */
export function draw(g, room, view, c, selected) {
  const is = (/** @type {string} */ kind, /** @type {string} */ id) => selected?.kind === kind && selected.id === id;
  const [w, h] = room.size;
  g.save();
  g.lineJoin = "round";
  g.lineCap = "round";
  g.font = "11px ui-sans-serif, system-ui, sans-serif";
  g.textBaseline = "bottom";
  // The room.
  g.fillStyle = c.ground;
  g.fillRect(view.x, view.y, w * view.scale, h * view.scale);
  g.strokeStyle = c.faint;
  g.lineWidth = 1;
  g.strokeRect(Math.round(view.x) + 0.5, Math.round(view.y) + 0.5, Math.round(w * view.scale), Math.round(h * view.scale));

  // Light, under everything that stands in it.
  for (const l of room.lights) {
    const [x, y] = toStage(view, l.at);
    const r = Math.max(2, l.radius * view.scale);
    const glow = g.createRadialGradient(x, y, 0, x, y, r);
    glow.addColorStop(0, withAlpha(l.color, 0.28));
    glow.addColorStop(1, withAlpha(l.color, 0));
    g.fillStyle = glow;
    g.beginPath();
    g.arc(x, y, r, 0, Math.PI * 2);
    g.fill();
    g.fillStyle = is("light", l.id) ? c.accent : l.color;
    g.beginPath();
    g.arc(x, y, is("light", l.id) ? 5 : 3.5, 0, Math.PI * 2);
    g.fill();
  }

  // What is solid, as lines: solid ones plain, one-way dashed, hazards red.
  for (const shape of room.collision) {
    if (shape.points.length < 2) continue;
    g.beginPath();
    shape.points.forEach((p, i) => {
      const [x, y] = toStage(view, p);
      if (i === 0) g.moveTo(x, y);
      else g.lineTo(x, y);
    });
    if (shape.closed) g.closePath();
    g.setLineDash(shape.kind === "oneway" ? [6, 5] : []);
    g.strokeStyle = is("shape", shape.id) ? c.accent : shape.kind === "hazard" ? c.err : shape.kind === "nowall" ? c.muted : c.text;
    g.lineWidth = is("shape", shape.id) ? 3 : 2;
    g.stroke();
  }
  g.setLineDash([]);

  // Things that cover an area, and things that stand somewhere.
  for (const e of room.entities) {
    const color = is("entity", e.id) ? c.accent : tone(e.prefab, c);
    g.strokeStyle = color;
    g.fillStyle = color;
    g.lineWidth = is("entity", e.id) ? 2 : 1.25;
    let label = /** @type {[number, number] | null} */ (null);
    if (e.rect) {
      const [x, y] = toStage(view, [e.rect[0], e.rect[1]]);
      g.globalAlpha = 0.12;
      g.fillRect(x, y, e.rect[2] * view.scale, e.rect[3] * view.scale);
      g.globalAlpha = 1;
      g.strokeRect(x + 0.5, y + 0.5, e.rect[2] * view.scale, e.rect[3] * view.scale);
      label = [x, y - 2];
    } else if (e.at) {
      const [x, y] = toStage(view, e.at);
      g.beginPath();
      g.moveTo(x, y - 7);
      g.lineTo(x + 6, y);
      g.lineTo(x, y + 7);
      g.lineTo(x - 6, y);
      g.closePath();
      g.fill();
      label = [x + 8, y - 4];
    }
    if (label) {
      g.fillStyle = is("entity", e.id) ? c.accent : c.muted;
      g.fillText(e.id, label[0], label[1]);
    }
  }
  for (const s of room.spawns) {
    const [x, y] = toStage(view, s.at);
    g.strokeStyle = is("spawn", s.id) ? c.accent : c.ok;
    g.lineWidth = is("spawn", s.id) ? 2 : 1.5;
    g.beginPath();
    g.moveTo(x, y);
    g.lineTo(x, y - 16);
    g.lineTo(x + 9, y - 12);
    g.lineTo(x, y - 8);
    g.stroke();
    g.fillStyle = is("spawn", s.id) ? c.accent : c.muted;
    g.fillText(s.id, x + 11, y - 8);
  }
  g.restore();
}

/** A `#rrggbb` colour at an opacity. @param {string} hex @param {number} alpha */
function withAlpha(hex, alpha) {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex ?? "");
  if (!m) return `rgba(255, 217, 160, ${alpha})`;
  const n = parseInt(m[1], 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

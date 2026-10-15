/* Game mode's particles: what hangs in a room's air — motes, rain, snow,
 * leaves, embers, dust — and what flies from what happens — dust as the
 * player lands and jumps, sparks as a blow lands, a burst as a thing is
 * beaten.
 *
 * Every particle is a closed function of time and a seed: where it is at a
 * moment is worked out from that moment, never stepped from the frame
 * before. So a run played back shows the same particles at the same ticks
 * whatever the frame rate, and the editor's still frame is the same as a
 * paused run's. Nothing here touches the run: particles are only drawn.
 *
 * A room's air is laid over the part of its plane the camera sees, tiled
 * from the room's own origin, so a particle stays where it is in the world
 * as the camera moves and comes round again past the edge of the view.
 * Each is the renderer's soft glow, tinted, stretched for rain. Sizes and
 * speeds are a 1080-pixel-high view's, scaled to the game's own, so a
 * pixel-art game's dust is as big against its player as a painted one's.
 * No DOM and no GL. */

/** The view height the presets are sized for. */
const TALL = 1080;

/** The key the renderer draws its own soft glow by. */
export const GLOW = "__glow";

/** A particle to draw, in a plane's pixels.
 *  @typedef {{x: number, y: number, w: number, h: number, rot: number, opacity: number, tint: [number, number, number, number], blend: "normal" | "add"}} Mote */

/** What hangs in the air: how many over the view, how big, how fast across
 *  and down (pixels a second, a range each), how far it sways and how fast,
 *  its colour, how opaque, how it blends, whether it twinkles, and — for
 *  rain — how long a streak.
 *  @typedef {{count: number, size: [number, number], vx: [number, number], vy: [number, number], sway: number, swing: number,
 *             color: string, alpha: [number, number], blend: "normal" | "add", twinkle: number, streak?: [number, number], spin?: boolean}} Air */

/** @type {Record<string, Air>} */
export const AIR = {
  motes: { count: 36, size: [7, 16], vx: [-16, 16], vy: [-30, -8], sway: 24, swing: 0.4, color: "#fff2c8", alpha: [0.25, 0.7], blend: "add", twinkle: 0.6 },
  rain: { count: 140, size: [3, 5], vx: [-160, -120], vy: [1800, 2200], sway: 0, swing: 0, color: "#aac4ff", alpha: [0.2, 0.4], blend: "normal", twinkle: 0, streak: [40, 70] },
  snow: { count: 90, size: [6, 13], vx: [-40, 40], vy: [80, 160], sway: 40, swing: 0.5, color: "#ffffff", alpha: [0.5, 0.9], blend: "normal", twinkle: 0 },
  leaves: { count: 24, size: [14, 24], vx: [40, 120], vy: [80, 180], sway: 80, swing: 0.7, color: "#c98a3c", alpha: [0.7, 1], blend: "normal", twinkle: 0, spin: true },
  embers: { count: 40, size: [4, 10], vx: [-30, 30], vy: [-180, -80], sway: 20, swing: 1.1, color: "#ff8a3c", alpha: [0.4, 0.95], blend: "add", twinkle: 1 },
  dust: { count: 30, size: [4, 9], vx: [10, 40], vy: [-6, 6], sway: 12, swing: 0.3, color: "#d8ccb0", alpha: [0.15, 0.35], blend: "normal", twinkle: 0.2 },
};

/** What flies from a moment: how many, how fast (pixels a second), which
 *  ways (radians, y down, before a facing turns them), how hard it falls,
 *  how long each lives (seconds), how big, its colour, how opaque, how it
 *  blends.
 *  @typedef {{count: number, speed: [number, number], angle: [number, number], gravity: number, life: [number, number], size: [number, number],
 *             color: string, alpha: number, blend: "normal" | "add"}} Burst */

/** @type {Record<string, Burst>} */
export const BURSTS = {
  land: { count: 12, speed: [80, 260], angle: [Math.PI * 0.95, Math.PI * 1.05], gravity: 520, life: [0.25, 0.45], size: [8, 16], color: "#d8ccb0", alpha: 0.55, blend: "normal" },
  jump: { count: 7, speed: [60, 180], angle: [Math.PI * 0.9, Math.PI * 1.1], gravity: 400, life: [0.2, 0.35], size: [6, 12], color: "#d8ccb0", alpha: 0.45, blend: "normal" },
  dash: { count: 12, speed: [120, 320], angle: [Math.PI * 0.9, Math.PI * 1.1], gravity: 0, life: [0.15, 0.3], size: [6, 12], color: "#cfe3ff", alpha: 0.6, blend: "add" },
  hit: { count: 14, speed: [320, 800], angle: [0, Math.PI * 2], gravity: 1200, life: [0.12, 0.3], size: [6, 12], color: "#fff4d0", alpha: 0.95, blend: "add" },
  kill: { count: 26, speed: [160, 640], angle: [0, Math.PI * 2], gravity: 760, life: [0.35, 0.8], size: [8, 20], color: "#ffb070", alpha: 0.9, blend: "add" },
  hurt: { count: 14, speed: [200, 520], angle: [0, Math.PI * 2], gravity: 840, life: [0.2, 0.45], size: [6, 14], color: "#ff5a6a", alpha: 0.9, blend: "add" },
  rest: { count: 18, speed: [40, 120], angle: [-Math.PI * 0.8, -Math.PI * 0.2], gravity: -80, life: [0.8, 1.4], size: [8, 14], color: "#9fe0ff", alpha: 0.8, blend: "add" },
};

/** A number from 0 to 1 that is always the same for the same three.
 *  @param {number} seed @param {number} i @param {number} k */
export function noise(seed, i, k) {
  let h = Math.imul(seed | 0, 0x9e3779b1) ^ Math.imul(i | 0, 0x85ebca6b) ^ Math.imul(k | 0, 0xc2b2ae35);
  h = Math.imul(h ^ (h >>> 16), 0x7feb352d);
  h = Math.imul(h ^ (h >>> 15), 0x846ca68b);
  h ^= h >>> 16;
  return (h >>> 0) / 4294967296;
}

/** A seed from a name: a room's, a burst's. @param {string} text */
export function seedOf(text) {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) h = Math.imul(h ^ text.charCodeAt(i), 16777619);
  return h | 0;
}

/** `#rrggbb` as 0–1 channels. @param {string} hex @returns {[number, number, number]} */
function rgb(hex) {
  const n = parseInt(hex.slice(1), 16);
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

/** `a` to `b` by `t`. @param {[number, number]} r @param {number} t */
const within = (r, t) => r[0] + (r[1] - r[0]) * t;

/** `v` into 0 to `m`, however far below 0. @param {number} v @param {number} m */
const wrap = (v, m) => ((v % m) + m) % m;

/** A room's air at `time` seconds, over `rect` — the part of its plane the
 *  camera sees, `[x, y, width, height]` in the plane's pixels — of the kind
 *  `preset`, seeded by `seed`. None for a kind there is not.
 *  @param {string} preset @param {number[]} rect @param {number} time @param {number} seed @returns {Mote[]} */
export function air(preset, rect, time, seed) {
  const p = AIR[preset];
  if (!p) return [];
  // The tile the air is laid in: the view and a margin, so nothing pops in
  // or out where it can be seen.
  const margin = 48;
  const [w, h] = [rect[2] + margin * 2, rect[3] + margin * 2];
  const tint = /** @type {[number, number, number, number]} */ ([...rgb(p.color), 1]);
  // As many as the view's shape holds, each as big and as fast as at 1080
  // high, scaled to the view.
  const k = rect[3] / TALL;
  const count = Math.round(p.count * Math.max(0.25, Math.min(4, rect[2] / rect[3] / (16 / 9))));
  /** @type {Mote[]} */
  const out = [];
  for (let i = 0; i < count; i++) {
    const r = (/** @type {number} */ k) => noise(seed, i, k);
    const vx = within(p.vx, r(2)) * k;
    const vy = within(p.vy, r(3)) * k;
    const sway = p.sway * k * Math.sin(time * p.swing * (0.6 + r(4)) * Math.PI * 2 + r(5) * Math.PI * 2);
    const x = rect[0] - margin + wrap(r(0) * w + vx * time + sway - (rect[0] - margin), w);
    const y = rect[1] - margin + wrap(r(1) * h + vy * time - (rect[1] - margin), h);
    const size = within(p.size, r(6)) * k;
    const twinkle = p.twinkle ? 1 - p.twinkle * 0.5 * (1 + Math.sin(time * (1.5 + r(7) * 2) * Math.PI + r(8) * Math.PI * 2)) : 1;
    const opacity = within(p.alpha, r(9)) * twinkle;
    if (p.streak) {
      // A streak along the way it falls.
      const length = within(p.streak, r(6)) * k;
      const rot = (Math.atan2(vx, vy) * -180) / Math.PI;
      out.push({ x: x - size / 2, y: y - length / 2, w: size, h: length, rot, opacity, tint, blend: p.blend });
    } else {
      const rot = p.spin ? (time * (r(10) - 0.5) * 360 + r(11) * 360) % 360 : 0;
      out.push({ x: x - size / 2, y: y - size / 2, w: size, h: p.spin ? size * 0.6 : size, rot, opacity, tint, blend: p.blend });
    }
  }
  return out;
}

/** A burst: of the kind `kind`, from `at`, begun at `start` seconds, seeded,
 *  turned to face left when `flip`, for a view `tall` pixels high (1080 when
 *  not said).
 *  @typedef {{kind: string, at: [number, number], start: number, seed: number, flip?: boolean, tall?: number}} Spark */

/** How long a burst of `kind` lasts, at most. @param {string} kind */
export function lifeOf(kind) {
  return BURSTS[kind]?.life[1] ?? 0;
}

/** A burst's particles at `time` seconds: thrown from where it began, each
 *  its own way, falling, fading as it goes. None before it began or after
 *  it is over. @param {Spark} spark @param {number} time @returns {Mote[]} */
export function burst(spark, time) {
  const p = BURSTS[spark.kind];
  const age = time - spark.start;
  if (!p || age < 0 || age > p.life[1]) return [];
  const tint = /** @type {[number, number, number, number]} */ ([...rgb(p.color), 1]);
  const k = (spark.tall ?? TALL) / TALL;
  /** @type {Mote[]} */
  const out = [];
  for (let i = 0; i < p.count; i++) {
    const r = (/** @type {number} */ n) => noise(spark.seed, i, n);
    const life = within(p.life, r(0));
    if (age > life) continue;
    let angle = within(p.angle, r(1));
    // Turned about for a thing facing left: dust thrown behind it.
    if (spark.flip) angle = Math.PI - angle;
    const speed = within(p.speed, r(2)) * k;
    const x = spark.at[0] + Math.cos(angle) * speed * age;
    const y = spark.at[1] + Math.sin(angle) * speed * age + (p.gravity * k * age * age) / 2;
    const left = 1 - age / life;
    const size = within(p.size, r(3)) * k * (0.4 + 0.6 * left);
    out.push({ x: x - size / 2, y: y - size / 2, w: size, h: size, rot: 0, opacity: p.alpha * left, tint, blend: p.blend });
  }
  return out;
}

/** The bursts still going at `time`, of those begun. @param {Spark[]} sparks @param {number} time */
export function living(sparks, time) {
  return sparks.filter((s) => time >= s.start - 1 && time - s.start <= lifeOf(s.kind));
}

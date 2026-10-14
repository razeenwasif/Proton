/* Game mode's scene, as arithmetic: where each plane's art lands for a
 * camera, what is drawn in what order, and what a point on the stage is over.
 * No canvas and no GL here — the renderer draws the list this makes, the
 * overlay draws over it, and the tests check it in node.
 *
 * ## Parallax, as the game camera sees it
 *
 * A plane `depth` pixels behind the play plane moves `focal / (focal + depth)`
 * as fast as it (its *factor*; a plane may set its own as `parallax`). The
 * game's camera frames `view` pixels of the play plane; with its top-left at
 * `corner`, a plane's own pixels are shifted by `corner * (1 - factor)` — so
 * a sky at factor 0 stays put on screen, the play plane at 1 does not shift,
 * and leaves in front, above 1, rush past.
 *
 * The editor shows the room as that camera would see it standing at the
 * middle of the stage — the editor's own pan and zoom then move over that
 * picture. Zoomed out, a backdrop covers only what the camera can see from
 * there, which is what it is: art outside it is never on screen. */

/** @typedef {import('./bindings/GameRoom').GameRoom} Room */
/** @typedef {import('./bindings/GamePlane').GamePlane} Plane */
/** @typedef {import('./bindings/GameItem').GameItem} Item */
/** @typedef {import('./game-overlay.js').View} View */
/** The editor's camera: the room point at the stage's middle, and stage pixels a room pixel.
 *  @typedef {{center: [number, number], zoom: number}} Cam */
/** One thing to draw, in room pixels. Rotation is about its middle. A piece
 *  of an image cut up carries the whole image's rectangle as `whole`; a
 *  frame of an animation, the part of its sheet it is, as `uv` (0–1).
 *  @typedef {{key: string, plane: string, item: string | null, x: number, y: number, w: number, h: number, rot: number, flip: boolean, opacity: number, tint: [number, number, number, number], blend: string, whole?: {x: number, y: number, w: number, h: number}, uv?: [number, number, number, number]}} Sprite */
/** An animation, as its `.anim.json` says it. @typedef {import('./bindings/GameAnim').GameAnim} Anim */
/** Something standing on the play plane: an image or an animation's frame, its feet at `at`.
 *  @typedef {{key: string, at: number[], pivot: number[], flip?: boolean, frame?: number | null}} Standing */
/** How an image is cut up: its size, and where each piece goes in it (an `.img.json`).
 *  @typedef {{size: [number, number], chunks: {file: string, at: [number, number], size: [number, number]}[]}} Parts */

/** How fast a plane moves against the play plane. @param {Plane} plane @param {number} focal */
export function factor(plane, focal) {
  if (plane.parallax !== null && plane.parallax !== undefined) return plane.parallax;
  const far = focal + plane.depth;
  // At or behind the camera a plane cannot be seen; it races past.
  return far <= 1 ? 1000 : focal / far;
}

/** Where the game camera's top-left is when it is centred at `center`,
 *  held inside the room's camera bounds.
 *  @param {Room} room @param {[number, number]} view the game's view, width and height @param {[number, number]} center */
export function corner(room, view, center) {
  const [bx, by, bw, bh] = room.camera.bounds ?? [0, 0, room.size[0], room.size[1]];
  const hold = (/** @type {number} */ at, /** @type {number} */ lo, /** @type {number} */ span, /** @type {number} */ size) =>
    span <= size ? lo + (span - size) / 2 : Math.max(lo, Math.min(lo + span - size, at - size / 2));
  return [hold(center[0], bx, bw, view[0]), hold(center[1], by, bh, view[1])];
}

/** How far a plane's own pixels are shifted, at a factor and a camera corner.
 *  @param {number} f @param {number[]} at @returns {[number, number]} */
export function shift(f, at) {
  return [at[0] * (1 - f), at[1] * (1 - f)];
}

/** The overlay's view for an editor camera on a stage `width` by `height`.
 *  @param {Cam} cam @param {number} width @param {number} height @returns {View} */
export function viewOf(cam, width, height) {
  return { scale: cam.zoom, x: width / 2 - cam.center[0] * cam.zoom, y: height / 2 - cam.center[1] * cam.zoom };
}

/** An editor camera that shows the whole room. @param {Room} room @param {number} width @param {number} height
 *  @returns {Cam} */
export function fitCam(room, width, height, margin = 24) {
  const [w, h] = room.size;
  const zoom = Math.max(0.01, Math.min((width - margin * 2) / Math.max(1, w), (height - margin * 2) / Math.max(1, h)));
  return { center: [w / 2, h / 2], zoom };
}

/** `#rrggbb` as 0–1 channels. @param {string | null | undefined} hex @returns {[number, number, number]} */
export function rgb(hex) {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex ?? "");
  if (!m) return [0, 0, 0];
  const n = parseInt(m[1], 16);
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

/** Everything a room draws, back to front, in room pixels, for the game
 *  camera centred at `center`. `sizes` says how large each loaded image is;
 *  what has not loaded is left out until it has. An animation, by its key in
 *  `anims`, is drawn as the frame its standing thing names.
 *  @param {Room} room
 *  @param {{focal: number, view: [number, number], center: [number, number], sizes: Map<string, [number, number]>,
 *           hidden?: Set<string>, solo?: string | null, sprites?: Standing[],
 *           parts?: Map<string, Parts>, anims?: Map<string, Anim>}} how
 *  @returns {Sprite[]} */
export function layout(room, how) {
  const at = corner(room, how.view, how.center);
  /** @type {Sprite[]} */
  const out = [];
  for (const plane of room.planes) {
    if (how.solo ? plane.id !== how.solo : how.hidden?.has(plane.id)) continue;
    const [dx, dy] = shift(factor(plane, how.focal), at);
    const tint = plane.tint && plane.fog > 0 ? /** @type {[number, number, number, number]} */ ([...rgb(plane.tint), Math.min(1, plane.fog)]) : /** @type {[number, number, number, number]} */ ([0, 0, 0, 0]);
    for (const item of plane.items) {
      const blend = item.blend !== "normal" ? item.blend : plane.blend;
      const parts = how.parts?.get(item.image);
      if (parts) {
        out.push(...pieces(item, parts, how.sizes, plane.id, item.at[0] + dx, item.at[1] + dy, tint, blend));
        continue;
      }
      const size = how.sizes.get(item.image);
      if (!size) continue;
      out.push({
        key: item.image, plane: plane.id, item: item.id,
        x: item.at[0] + dx, y: item.at[1] + dy, w: size[0] * item.scale, h: size[1] * item.scale,
        rot: item.rot, flip: item.flip, opacity: item.opacity, tint, blend,
      });
    }
    // What stands in the room stands on the play plane, in front of its art.
    if (plane.gameplay) {
      for (const s of how.sprites ?? []) {
        const anim = how.anims?.get(s.key);
        if (anim) {
          const drawn = frameSprite(anim, s, how.sizes, plane.id);
          if (drawn) out.push(drawn);
          continue;
        }
        const size = how.sizes.get(s.key);
        if (!size) continue;
        // Flipped about its pivot, so it turns where it stands.
        const x = s.flip ? s.at[0] - (size[0] - s.pivot[0]) : s.at[0] - s.pivot[0];
        out.push({ key: s.key, plane: plane.id, item: null, x, y: s.at[1] - s.pivot[1], w: size[0], h: size[1],
          rot: 0, flip: Boolean(s.flip), opacity: 1, tint: [0, 0, 0, 0], blend: "normal" });
      }
    }
  }
  return out;
}

/** A frame of an animation, standing at `s.at` on its feet: the part of its
 *  sheet it is, placed where it sits on the sprite's canvas — and flipped
 *  about the feet, so it turns where it stands. None while its sheet has not
 *  loaded, or for a frame with nothing in it.
 *  @param {Anim} anim @param {Standing} s @param {Map<string, [number, number]>} sizes @param {string} plane
 *  @returns {Sprite | null} */
export function frameSprite(anim, s, sizes, plane) {
  const f = anim.frames[Math.max(0, s.frame ?? 0)];
  if (!f || !f.rect[2] || !f.rect[3]) return null;
  const key = anim.sheets[f.sheet];
  const sheet = key ? sizes.get(key) : undefined;
  if (!key || !sheet) return null;
  const [w, h] = [f.rect[2], f.rect[3]];
  const x = s.flip ? s.at[0] + s.pivot[0] - f.offset[0] - w : s.at[0] - s.pivot[0] + f.offset[0];
  const y = s.at[1] - s.pivot[1] + f.offset[1];
  const uv = /** @type {[number, number, number, number]} */ ([f.rect[0] / sheet[0], f.rect[1] / sheet[1], (f.rect[0] + w) / sheet[0], (f.rect[1] + h) / sheet[1]]);
  return { key, plane, item: null, x, y, w, h, rot: 0, flip: Boolean(s.flip), opacity: 1, tint: [0, 0, 0, 0], blend: "normal", uv };
}

/** Where a box over an animation's frame is in the room, for a thing
 *  standing at `at` on `pivot`, flipped or not: left, top, width, height.
 *  @param {number[]} rect on the sprite's canvas @param {number[]} at @param {number[]} pivot @param {boolean} flip */
export function boxInRoom(rect, at, pivot, flip) {
  const [bx, by, bw, bh] = rect;
  const x = flip ? at[0] + pivot[0] - bx - bw : at[0] - pivot[0] + bx;
  return [x, at[1] - pivot[1] + by, bw, bh];
}

/** The frame a thing standing still shows: its idle clip's first — the
 *  clip a prefab names for idle, else one called idle, else the first.
 *  @param {Anim} anim @param {Record<string, string | undefined> | undefined} clips */
export function restingFrame(anim, clips) {
  const name = clips?.idle;
  const clip = anim.clips.find((c) => c.name === name) ?? anim.clips.find((c) => c.name.toLowerCase() === "idle") ?? anim.clips[0];
  if (!clip) return 0;
  return clip.direction === "reverse" || clip.direction === "pingPongReverse" ? clip.to : clip.from;
}

/** An image cut up, as its pieces: each placed, flipped and turned as its
 *  part of the whole is — about the whole's middle, not its own. A piece
 *  not loaded yet is left out.
 *  @param {Item} item @param {Parts} parts @param {Map<string, [number, number]>} sizes @param {string} plane
 *  @param {number} x the whole's left @param {number} y its top
 *  @param {[number, number, number, number]} tint @param {string} blend @returns {Sprite[]} */
function pieces(item, parts, sizes, plane, x, y, tint, blend) {
  const s = item.scale;
  const whole = { x, y, w: parts.size[0] * s, h: parts.size[1] * s };
  const a = (item.rot * Math.PI) / 180;
  const [cos, sin] = [Math.cos(a), Math.sin(a)];
  /** @type {Sprite[]} */
  const out = [];
  for (const c of parts.chunks) {
    if (!sizes.has(c.file)) continue;
    const [w, h] = [c.size[0] * s, c.size[1] * s];
    const left = item.flip ? whole.w - c.at[0] * s - w : c.at[0] * s;
    // From the whole's middle to the piece's, turned with the whole.
    const dx = left + w / 2 - whole.w / 2;
    const dy = c.at[1] * s + h / 2 - whole.h / 2;
    const cx = whole.x + whole.w / 2 + dx * cos - dy * sin;
    const cy = whole.y + whole.h / 2 + dx * sin + dy * cos;
    out.push({ key: c.file, plane, item: item.id, x: cx - w / 2, y: cy - h / 2, w, h, rot: item.rot, flip: item.flip, opacity: item.opacity, tint, blend, whole });
  }
  return out;
}

/** A room point in a sprite's own frame, unrotated, from its top-left.
 *  @param {Sprite} s @param {number} x @param {number} y @returns {[number, number]} */
function local(s, x, y) {
  const cx = s.x + s.w / 2;
  const cy = s.y + s.h / 2;
  const a = (-s.rot * Math.PI) / 180;
  const dx = x - cx;
  const dy = y - cy;
  return [dx * Math.cos(a) - dy * Math.sin(a) + s.w / 2, dx * Math.sin(a) + dy * Math.cos(a) + s.h / 2];
}

/** The front-most placed image under a room point, on a plane that is not
 *  locked — and, when `solid` can say, only where the image is not clear:
 *  leaves passing in front of the camera should not take a click meant for
 *  the ground seen between them.
 *  @param {Sprite[]} drawn @param {number} x @param {number} y @param {Set<string>} [locked]
 *  @param {(key: string, u: number, v: number) => boolean} [solid] whether an image is opaque at u, v (0–1)
 *  @returns {Sprite | null} */
export function pickItem(drawn, x, y, locked, solid) {
  for (let i = drawn.length - 1; i >= 0; i--) {
    const s = drawn[i];
    if (!s.item || locked?.has(s.plane)) continue;
    const [lx, ly] = local(s, x, y);
    if (lx < 0 || ly < 0 || lx > s.w || ly > s.h) continue;
    const u = s.flip ? 1 - lx / s.w : lx / s.w;
    if (!solid || solid(s.key, u, ly / s.h)) return s;
  }
  return null;
}

/** An image's opacity, small: one byte a cell, at most `most` cells across.
 *  @param {CanvasImageSource & {width: number, height: number}} image @returns {{w: number, h: number, alpha: Uint8Array} | null} */
export function maskOf(image, most = 256) {
  const scale = Math.min(1, most / Math.max(1, image.width), most / Math.max(1, image.height));
  const w = Math.max(1, Math.round(image.width * scale));
  const h = Math.max(1, Math.round(image.height * scale));
  const canvas = typeof OffscreenCanvas !== "undefined" ? new OffscreenCanvas(w, h) : Object.assign(document.createElement("canvas"), { width: w, height: h });
  const g = /** @type {CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D | null} */ (canvas.getContext("2d"));
  if (!g) return null;
  g.drawImage(image, 0, 0, w, h);
  const rgba = g.getImageData(0, 0, w, h).data;
  const alpha = new Uint8Array(w * h);
  for (let i = 0; i < alpha.length; i++) alpha[i] = rgba[i * 4 + 3];
  return { w, h, alpha };
}

/** Where a room point is in a plane's own pixels, for the camera at `center`
 *  — where something dropped there goes. @param {Room} room @param {Plane} plane
 *  @param {{focal: number, view: [number, number], center: [number, number]}} how @param {number[]} p
 *  @returns {[number, number]} */
export function toPlane(room, plane, how, p) {
  const [dx, dy] = shift(factor(plane, how.focal), corner(room, how.view, how.center));
  return [p[0] - dx, p[1] - dy];
}

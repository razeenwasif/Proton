/* Canvas mode's frames: which cel a layer is in a frame, and the order
 * frames play in — on to the last and round again, back from the last, or to
 * the end and back — over every frame or a tag's run of them. No DOM here,
 * so the node tests drive it. */

/** @typedef {import('./bindings/RasterTag').RasterTag} Tag */
/** @typedef {import('./bindings/RasterDirection').RasterDirection} Direction */
/** @typedef {{from: number, to: number, direction: Direction}} Run */

/** A cel is known by its layer's id and this many times its frame:
 *  proton-raster's `FRAME_STEP`. A layer's own id is its first frame's. */
export const FRAME_STEP = 65536;

/** The id of a layer's cel in a frame (from 0). @param {number} layer @param {number} frame */
export const celId = (layer, frame) => layer + frame * FRAME_STEP;

/** The frame a cel is in. @param {number} cel */
export const frameOf = (cel) => Math.floor(cel / FRAME_STEP);

/** The frames a playing goes round: a tag's, or every frame, on.
 *  @param {number} frames @param {Tag | null | undefined} tag @returns {Run} */
export function runOf(frames, tag) {
  if (tag && tag.from <= tag.to && tag.to < frames) return { from: tag.from, to: tag.to, direction: tag.direction };
  return { from: 0, to: Math.max(0, frames - 1), direction: "forward" };
}

/** Where playing a run begins — from `at` when it is in the run — and which
 *  way it goes. @param {Run} run @param {number} [at] @returns {{frame: number, forward: boolean}} */
export function start(run, at) {
  const back = run.direction === "reverse" || run.direction === "pingPongReverse";
  if (at !== undefined && at >= run.from && at <= run.to) return { frame: at, forward: !back };
  return back ? { frame: run.to, forward: false } : { frame: run.from, forward: true };
}

/** The frame after `at` in a run, and which way it goes after that.
 *  @param {Run} run @param {number} at @param {boolean} forward @returns {{frame: number, forward: boolean}} */
export function step(run, at, forward) {
  const { from, to } = run;
  if (at < from || at > to) return start(run);
  if (from === to) return { frame: from, forward };
  switch (run.direction) {
    case "forward":
      return { frame: at === to ? from : at + 1, forward: true };
    case "reverse":
      return { frame: at === from ? to : at - 1, forward: false };
    default:
      // To the end and back, the ends shown once each time round.
      if (forward) return at === to ? { frame: at - 1, forward: false } : { frame: at + 1, forward: true };
      return at === from ? { frame: at + 1, forward: true } : { frame: at - 1, forward: false };
  }
}

/** A thumbnail's size: a canvas `w` × `h` fitted in a square `most` a side,
 *  its shape kept, at least a pixel each way. @param {number} w @param {number} h @param {number} most */
export function thumbSize(w, h, most) {
  const scale = Math.min(most / w, most / h);
  return [Math.max(1, Math.round(w * scale)), Math.max(1, Math.round(h * scale))];
}

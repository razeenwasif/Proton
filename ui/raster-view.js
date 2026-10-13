/* Canvas mode's view, as arithmetic: where the canvas is on the stage, how
 * large, and what a point on the stage is over. No DOM — the node tests
 * check it, and the stage, the input and the overlay all ask it.
 *
 * A stage point is `canvas point × zoom + (x, y)`. A pixel canvas is seen at
 * whole zooms once it is at least its own size, so every pixel is the same
 * number of screen pixels and the grid stays even. */

/** @typedef {{zoom: number, x: number, y: number}} View */

/** The least and most a canvas is magnified. */
export const ZOOM = [1 / 32, 64];

/** Whole when at least 1, for a pixel canvas. @param {number} zoom @param {boolean} pixel */
export function snap(zoom, pixel) {
  const z = Math.min(ZOOM[1], Math.max(ZOOM[0], zoom));
  return pixel && z >= 1 ? Math.max(1, Math.floor(z)) : z;
}

/** The canvas whole and centred, with a margin.
 *  @param {number} width @param {number} height @param {number} stageW @param {number} stageH
 *  @param {boolean} pixel @returns {View} */
export function fit(width, height, stageW, stageH, pixel = false, margin = 32) {
  const zoom = snap(Math.min((stageW - 2 * margin) / Math.max(1, width), (stageH - 2 * margin) / Math.max(1, height)), pixel);
  return { zoom, x: Math.round((stageW - width * zoom) / 2), y: Math.round((stageH - height * zoom) / 2) };
}

/** A stage point as a canvas point. @param {View} v @param {number} sx @param {number} sy @returns {[number, number]} */
export function toCanvas(v, sx, sy) {
  return [(sx - v.x) / v.zoom, (sy - v.y) / v.zoom];
}

/** A canvas point as a stage point. @param {View} v @param {number} x @param {number} y @returns {[number, number]} */
export function toStage(v, x, y) {
  return [x * v.zoom + v.x, y * v.zoom + v.y];
}

/** Zoomed by `factor` about a stage point, which stays over the same canvas point.
 *  @param {View} v @param {number} sx @param {number} sy @param {number} factor @param {boolean} pixel @returns {View} */
export function zoomAt(v, sx, sy, factor, pixel = false) {
  let zoom = snap(v.zoom * factor, pixel);
  // A pixel canvas at a whole zoom still moves a step per notch.
  if (pixel && zoom === v.zoom && v.zoom >= 1 && factor !== 1) zoom = snap(v.zoom + (factor > 1 ? 1 : -1), pixel);
  const [cx, cy] = toCanvas(v, sx, sy);
  return { zoom, x: sx - cx * zoom, y: sy - cy * zoom };
}

/** The canvas as a whole number of pixels: Q8, as the engine takes it.
 *  @param {number} x @returns {number} */
export function q8(x) {
  return Math.round(x * 256);
}

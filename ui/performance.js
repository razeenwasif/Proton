// Opt-in, bounded diagnostics. Records timings and command kinds, never text,
// paths, file contents, or keystrokes. No storage or telemetry. Timing data is
// collected only while explicitly enabled.
/** @typedef {{type: string, queueMs: number, replyMs: number, renderMs: number, nextFrameMs: number | null}} Sample */
let enabled = false;
let generation = 0;
/** @type {Sample[]} */
let samples = [];
let paints = 0;
let rows = 0;
export const performanceReview = {
  start() { enabled = true; generation++; samples = []; paints = 0; rows = 0; },
  stop() { enabled = false; generation++; return this.snapshot(); },
  snapshot() { return {samples: samples.map(sample => ({...sample})), paints, rows}; },
};
export function queuedAt() { return enabled ? performance.now() : null; }
/** @param {number | null} queued @param {string} type */
export function commandTiming(queued, type) {
  if (queued === null || !enabled) return null;
  const run = generation, sent = performance.now();
  let replied = sent;
  return {
    reply() { replied = performance.now(); },
    done() {
      if (!enabled || run !== generation) return;
      const rendered = performance.now();
      const sample = {type, queueMs: sent - queued, replyMs: replied - sent,
        renderMs: rendered - replied, nextFrameMs: /** @type {number | null} */ (null)};
      samples.push(sample);
      if (samples.length > 200) samples.shift();
      requestAnimationFrame(() => {
        if (run === generation) sample.nextFrameMs = performance.now() - queued;
      });
    },
  };
}
/** @param {number} rowCount */
export function editorPainted(rowCount) { if (enabled) { paints++; rows += rowCount; } }

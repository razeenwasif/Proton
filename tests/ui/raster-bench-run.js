// How fast the window's copy of a canvas keeps up with a pen: a 4096 × 4096
// paint canvas of sixteen layers with pixels under the stroke, a 200 px soft
// brush whose size and flow follow the pressure, and a pen at 120 Hz moving a
// thousand pixels a second — two points a 60 Hz frame. Each frame is what the
// window does: the frame's points to the copy, then what changed laid
// together, within the per-frame budget the window gives it (`nextBudget`,
// as ui/raster.js does) — what does not fit waits a frame, and the next
// frame is given more. The plan's budget is 4 ms a frame at the 95th
// percentile, the ink no more than two frames behind: met painting on the
// top layer. Painting under fifteen layers, most of them in blend modes,
// costs more than 4 ms a frame to keep up; the frames grow toward a 120 Hz
// frame's length (8.3 ms) and the ink stays within two frames.
//
// Run by tests/ui/raster-bench.mjs in node and by raster-bench.html in a
// browser — WebKit's, as the app's window is, through a WKWebView:
//
//   node scripts/wasm.mjs && node tests/ui/raster-bench.mjs
//
// Not part of `npm run test:unit`: timing depends on the machine and what
// else it is doing.

import {FRAME_BUDGET, nextBudget} from '../../ui/raster-runtime.js';

const BUDGET_MS = 4;
const SIDE = 4096;
const LAYERS = 16;
// The square the stroke goes round, every layer with pixels in it.
const AREA = [1024, 1024, 1536, 1536];

/** The bench: `bytes` the engine, `now` a clock in ms, `log` where the
 *  lines go. Whether a stroke was over budget. */
export async function bench(bytes, now, log) {
  const {instance} = await WebAssembly.instantiate(bytes);
  const rr = instance.exports;
  const pass = (fn, value, ...before) => {
    const bytes = new TextEncoder().encode(typeof value === 'string' ? value : JSON.stringify(value));
    const at = rr.rr_alloc(bytes.length);
    new Uint8Array(rr.memory.buffer, at, bytes.length).set(bytes);
    const result = fn(...before, at, bytes.length);
    rr.rr_free(at, bytes.length);
    if (result !== 0) throw new Error('the engine refused a call');
    return result;
  };
  const add = (points) => {
    const at = rr.rr_alloc(points.length * 4);
    new Int32Array(rr.memory.buffer, at, points.length).set(points);
    rr.rr_stroke_add(at, points.length);
    rr.rr_free(at, points.length * 4);
  };
  const modes = ['normal', 'multiply', 'normal', 'screen', 'normal', 'overlay', 'normal', 'softLight'];
  const layers = Array.from({length: LAYERS}, (_, i) => ({id: i + 1, name: `Layer ${i + 1}`, opacity: 230, blend: modes[i % modes.length], hidden: false, locked: false, alphaLock: false}));
  pass(rr.rr_open, {width: SIDE, height: SIDE, preset: 'paint', palette: [], layers, generation: 1});
  let t = now();
  for (const l of layers) pass(rr.rr_apply, {op: 'fillRect', layer: l.id, rect: [AREA[0] - 256, AREA[1] - 256, AREA[2] + 512, AREA[3] + 512], color: [(l.id * 53) % 256, (l.id * 97) % 256, (l.id * 31) % 256, 120 + l.id * 8]});
  rr.rr_dirty();
  log(`set up: ${SIDE} × ${SIDE}, ${LAYERS} layers with pixels under the stroke, in ${Math.round(now() - t)} ms`);

  const brush = {size: 200, hardness: 0, spacing: 10, flow: 128, opacity: 255, sizePressure: 50, flowPressure: 50, opacityPressure: 0};

  /** One stroke round the square, on `layer`: each frame's cost. */
  function stroke(layer) {
    pass(rr.rr_stroke_begin, {layer, tool: 'pencil', brush, color: [20, 30, 200, 255]});
    const frames = [];
    const split = [];
    let behind = 0;
    let longest = 0;
    let run = 0;
    let budget = FRAME_BUDGET;
    const perFrame = 2;
    const speed = 1000 / 120; // pixels an event
    const path = 2 * (AREA[2] + AREA[3]);
    const events = Math.floor(path / speed);
    for (let e = 0; e < events; e += perFrame) {
      const points = [];
      for (let k = e; k < Math.min(events, e + perFrame); k++) {
        let d = k * speed;
        const side = [AREA[2], AREA[3], AREA[2], AREA[3]];
        let at = [AREA[0], AREA[1]];
        const dirs = [[1, 0], [0, 1], [-1, 0], [0, -1]];
        for (let s = 0; s < 4; s++) {
          const go = Math.min(d, side[s]);
          at = [at[0] + dirs[s][0] * go, at[1] + dirs[s][1] * go];
          d -= go;
          if (d <= 0) break;
        }
        const pressure = Math.round((0.5 + 0.5 * Math.sin(k / 15)) * 65535);
        points.push(Math.round(at[0] * 256), Math.round(at[1] * 256), pressure);
      }
      const start = now();
      add(points);
      const mid = now();
      rr.rr_dirty_some(budget);
    budget = nextBudget(budget, rr.rr_pending() > 0);
      frames.push(now() - start);
      split.push([mid - start, now() - mid]);
      // Frames the ink is behind the pen: as long as parts wait.
      run = rr.rr_pending() ? run + 1 : 0;
      behind += run ? 1 : 0;
      longest = Math.max(longest, run);
    }
    const ended = now();
    rr.rr_stroke_end();
    return {frames, split, behind, longest, end: now() - ended};
  }

  // A stroke first, unmeasured: WebKit compiles WebAssembly in tiers, the
  // optimised code arriving while it runs; a person's first stroke is no
  // more what is measured here than a cold start is.
  stroke(LAYERS - 1);
  const quantile = (xs, q) => [...xs].sort((a, b) => a - b)[Math.min(xs.length - 1, Math.floor(q * xs.length))];
  let over = false;
  for (const [name, layer, most] of [['top layer', LAYERS, BUDGET_MS], ['bottom layer (fifteen over it)', 1, 1000 / 120]]) {
    const {frames, split, behind, longest, end} = stroke(layer);
    const p50 = quantile(frames, 0.5), p95 = quantile(frames, 0.95), worst = Math.max(...frames);
    const late = p95 > most || longest > 2;
    over ||= late;
    log(`${name}: ${frames.length} frames — p50 ${p50.toFixed(2)} ms, p95 ${p95.toFixed(2)} ms, worst ${worst.toFixed(2)} ms` +
      ` (dabs ${quantile(split.map((s) => s[0]), 0.95).toFixed(2)}, laying together ${quantile(split.map((s) => s[1]), 0.95).toFixed(2)} at p95);` +
      ` the ink behind in ${behind} frames, at most ${longest} in a row; the stroke ended in ${end.toFixed(1)} ms${late ? `  ← over ${most.toFixed(1)} ms or behind` : ''}`);
  }
  return over;
}

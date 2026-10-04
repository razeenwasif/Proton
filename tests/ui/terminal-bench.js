// The terminal's two renderers on the same heavy output: a coloured build log
// scrolling past a line a frame, so every row of the screen changes every time.
//
// A screen is delivered at the start of each frame, and the frame is timed
// from there to the first task after it: the screen delivered, drawn, styled,
// laid out and painted — the main thread's work for it, the thread the editor
// needs. Frames that came late are counted too. Paint's own threads and the
// GPU's process are not in it; a driver can read those from outside.
// `window.bench` lets a driver run it itself — Playwright with Chrome's
// profiler, or a WKWebView — and opening the page runs it for both.
const frame = document.querySelector('iframe');
const html = await fetch('/index.html').then((r) => r.text());
frame.srcdoc = html.replace('<head>', '<head><base href="/"><script src="tests/ui/transport.js"></script>');
await new Promise((done) => frame.addEventListener('load', done, {once: true}));
const win = frame.contentWindow, doc = win.document;
const $ = (selector) => doc.querySelector(selector);
const wait = (ms) => new Promise((done) => setTimeout(done, ms));
const nextFrame = () => new Promise((done) => win.requestAnimationFrame(done));
/** A task boundary that is not a frame: each screen is its own task, as a
 *  pushed event is, without waiting for the display. */
const nextTask = () => new Promise((done) => {
  const channel = new MessageChannel();
  channel.port1.onmessage = () => done(undefined);
  channel.port2.postMessage(0);
});
await wait(300);

// The terminal open, and wider than the app lets it be — most of the stage —
// so the screen is the size of a terminal someone would give a build.
const pane = $('[data-term-pane]');
if (pane.hidden) $('.titlebar [data-term-toggle]').click();
pane.style.setProperty('--term-w', '4000px');
pane.style.maxWidth = '80%';
await wait(400);
const size = win.commands.filter((c) => c.type === 'terminalSize').at(-1) ?? {rows: 40, cols: 100};
const {rows, cols} = size;

const FILL = 'lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore '.repeat(4);
const plain = {fg: null, bg: null, rgb: null, bold: false, italic: false, underline: false, inverse: false};
/** Line `k` of the log, cut at the edge of the screen. */
function line(k) {
  const runs = [];
  switch (k % 6) {
    case 0: runs.push({text: '   Compiling ', fg: 2, bold: true}, {text: `proton-crate-${k} v0.${k % 9}.${k % 5} `},
      {text: `(/Users/dev/proton/crates/crate-${k}) `, fg: 8}); break;
    case 1: runs.push({text: 'warning', fg: 3, bold: true}, {text: `: unused variable: \`value_${k}\` `, bold: true}); break;
    case 2: runs.push({text: '  --> ', fg: 4, bold: true}, {text: `src/module_${k % 31}.rs:${k % 400}:${k % 80} `}); break;
    case 3: runs.push({text: `${String(k % 400).padStart(4)} | `, fg: 4, bold: true},
      {text: `    let value_${k} = compute(${k}, &mut state); `}, {text: '// why not', fg: 8, italic: true}); break;
    case 4: runs.push({text: 'test '}, {text: `tests::case_${k} ... `}, {text: 'ok ', fg: 2}, {text: ` ${k % 100}% `, fg: 0, bg: 6}); break;
    default: runs.push({text: `${(k * 7919) % 10000}`.padStart(5) + ' ', fg: 5}, {text: 'INFO ', fg: 6, bold: true},
      {text: `request ${k} served in ${k % 97}ms `}, {text: '#'.repeat(k % 20), fg: 160 + (k % 60)});
  }
  const used = runs.reduce((n, run) => n + run.text.length, 0);
  if (used < cols - 4) runs.push({text: ' ' + FILL.slice(k % 40, k % 40 + cols - 5 - used), fg: k % 3 ? null : 7});
  const out = [];
  let room = cols;
  for (const run of runs) {
    if (room <= 0) break;
    const text = run.text.slice(0, room);
    room -= text.length;
    out.push({...plain, ...run, text});
  }
  return out;
}
const screenAt = (n) => ({type: 'terminal', running: true, scrollback: 0, cursor: [rows - 1, 0], cursorVisible: true,
  rows: Array.from({length: rows}, (_, i) => line(n + i))});
// Built before anything is timed, so the timing is of drawing and not of this.
const screens = Array.from({length: 600}, (_, n) => screenAt(n));
let shown = 0;
const push = () => win.pushEvents([screens[shown++ % screens.length]]);

async function use(renderer) {
  await win.dispatch({type: 'setSetting', key: 'terminal.renderer', value: renderer});
  await wait(300);
  const gpu = Boolean(pane.querySelector('canvas.term-gl'));
  if ((renderer === 'gpu') !== gpu) throw new Error(`asked for ${renderer} and got ${gpu ? 'the GPU' : 'text'}`);
  // Warm: glyphs rasterised, code compiled, rows in place.
  for (let n = 0; n < 30; n++) { push(); await nextTask(); }
  await wait(300);
}

function summary(times) {
  const sorted = [...times].sort((a, b) => a - b);
  const at = (q) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))];
  const round = (v) => Math.round(v * 1000) / 1000;
  return {median: round(at(0.5)), p95: round(at(0.95)), mean: round(times.reduce((a, b) => a + b, 0) / times.length)};
}

/** A screen a frame: the main thread's whole frame, and how many were late.
 *  `ready` skips switching to the renderer and warming it, for a driver that
 *  has done that itself. */
async function paced(renderer, frames = 300, ready = false) {
  if (!ready) await use(renderer);
  const work = [], gaps = [];
  await new Promise((finish) => {
    let n = 0, last = 0;
    const tick = (stamp) => {
      // Asked for first, so that next frame this runs before anything the
      // screen below asks for — the GPU's draw included — and the time from
      // here to the first task after the frame is all of the frame's work.
      if (n < frames) win.requestAnimationFrame(tick);
      const start = performance.now();
      if (last) gaps.push(stamp - last);
      last = stamp;
      if (n++ === frames) return finish(undefined);
      push();
      nextTask().then(() => work.push(performance.now() - start));
    };
    win.requestAnimationFrame(tick);
  });
  await wait(300);
  const interval = summary(gaps).median;
  return {frame: summary(work), interval, late: gaps.filter((gap) => gap > interval * 1.5).length, frames};
}

window.bench = {rows, cols, paced, use};
window.benchReady = true;

if (!new URLSearchParams(location.search).has('driven')) {
  const out = document.querySelector('#result');
  const results = {screen: `${rows} × ${cols}`, engine: navigator.userAgent};
  for (const renderer of ['text', 'gpu']) {
    try {
      results[renderer] = await paced(renderer);
    } catch (error) {
      results[renderer] = String(error);
    }
    out.textContent = JSON.stringify(results, null, 2);
  }
  await win.dispatch({type: 'setSetting', key: 'terminal.renderer', value: 'text'});
  window.benchResult = results;
}

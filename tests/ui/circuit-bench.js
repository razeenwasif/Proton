// C9's budget for drawing: a circuit of 2,500 parts at 60 frames a second.
// The half adder the suite draws — as the core sent it, its pins placed
// there — is laid out 417 times, 2,502 parts and 5,004 wires, and panned a
// little each frame at each zoom from fitted to close, then once running
// with every wire carrying a value.
//
// Each frame is timed as the terminal's bench times one: from the start of
// the frame to the first task after it — the pan, the circuit drawn, the
// page painted, the main thread's work. `sync` times the drawing alone,
// called straight, for a driver whose page gets no frames (a WKWebView off
// the screen). `window.bench` lets a driver run either itself.
const frame = document.querySelector('iframe');
const html = await fetch('/index.html').then((r) => r.text());
frame.srcdoc = html.replace('<head>', '<head><base href="/"><script src="tests/ui/transport.js"></script>');
await new Promise((done) => frame.addEventListener('load', done, {once: true}));
const win = frame.contentWindow, doc = win.document;
const wait = (ms) => new Promise((done) => setTimeout(done, ms));
const nextTask = () => new Promise((done) => {
  const channel = new MessageChannel();
  channel.port1.onmessage = () => done(undefined);
  channel.port2.postMessage(0);
});
await wait(300);

const HALF = {"parts":[{"id":"A","kind":"in","x":0,"y":0,"rot":0,"mirror":false,"settings":[["label","A"]],"pins":[{"x":0,"y":0,"input":false}],"problem":null},{"id":"B","kind":"in","x":0,"y":2,"rot":0,"mirror":false,"settings":[["label","B"]],"pins":[{"x":0,"y":2,"input":false}],"problem":null},{"id":"C","kind":"out","x":8,"y":4,"rot":0,"mirror":false,"settings":[["label","C"]],"pins":[{"x":8,"y":4,"input":true}],"problem":null},{"id":"S","kind":"out","x":8,"y":0,"rot":0,"mirror":false,"settings":[["label","S"]],"pins":[{"x":8,"y":0,"input":true}],"problem":null},{"id":"X","kind":"xor","x":4,"y":0,"rot":0,"mirror":false,"settings":[],"pins":[{"x":4,"y":0,"input":true},{"x":4,"y":2,"input":true},{"x":7,"y":1,"input":false}],"problem":null},{"id":"Y","kind":"and","x":4,"y":4,"rot":0,"mirror":false,"settings":[],"pins":[{"x":4,"y":4,"input":true},{"x":4,"y":6,"input":true},{"x":7,"y":5,"input":false}],"problem":null}],"wires":[[0,0,2,0],[0,2,3,2],[2,0,2,4],[2,0,4,0],[2,4,4,4],[3,2,3,6],[3,2,4,2],[3,6,4,6],[7,1,8,1],[7,5,8,5],[8,0,8,1],[8,4,8,5]]};

/** The half adder `n` times, 21 to a row, each copy's ids and labels its own. */
function tiled(n) {
  const parts = [], wires = [];
  for (let i = 0; i < n; i++) {
    const ox = (i % 21) * 12, oy = Math.floor(i / 21) * 9;
    for (const p of HALF.parts) {
      parts.push({...p, id: `${p.id}${i}`, x: p.x + ox, y: p.y + oy, settings: p.settings.map(([k, v]) => [k, k === 'label' ? `${v}${i}` : v]),
        pins: p.pins.map((q) => ({...q, x: q.x + ox, y: q.y + oy}))});
    }
    for (const w of HALF.wires) wires.push([w[0] + ox, w[1] + oy, w[2] + ox, w[3] + oy]);
  }
  return {parts, wires};
}

const big = tiled(417);
await win.dispatch({type: 'setMode', mode: 'circuit'});
await win.pushEvents([{type: 'circuitProject', project: {path: 'big.circuit', name: 'big', top: 'main', circuits: ['main'], shown: 'main',
  parts: big.parts, wires: big.wires, tests: [], generation: 9, dirty: false, canUndo: false, canRedo: false, made: [], data: [], tables: [], machines: [],
  palette: ['in', 'out', 'and', 'xor']}}]);
await wait(400);
const stage = doc.querySelector('.workspace[data-ws="circuit"] [data-circuit-stage]');
const press = (key) => stage.dispatchEvent(new win.KeyboardEvent('keydown', {key, bubbles: true}));
/** Fitted, then `closer` steps of + in. */
async function zoomTo(closer) {
  press('=');
  for (let i = 0; i < closer; i++) press('+');
  await wait(100);
  return win.circuitDebug().view.scale;
}
/** Every wire carrying a value, alternately high and low, a bus here and
 *  there; and the inputs' and outputs' values, as the core sends them. */
function running(on) {
  const wires = big.wires.map((_, i) => ({bits: i % 7 === 0 ? 8 : 1, hex: i % 7 === 0 ? 'a5' : String(i % 2), z: false, someZ: false}));
  const values = big.parts.filter((p) => p.kind === 'in' || p.kind === 'out').map((p, i) => [p.id, {bits: 1, hex: String(i % 2), z: false, someZ: false}]);
  return win.pushEvents([{type: 'circuitRun', run: on ? {generation: 9, circuit: 'main', wires, values, waiting: 0, stepped: [], error: null, quick: null} : null}]);
}

function summary(times) {
  const sorted = [...times].sort((a, b) => a - b);
  const at = (q) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))];
  const round = (v) => Math.round(v * 100) / 100;
  return {median: round(at(0.5)), p95: round(at(0.95)), max: round(sorted.at(-1))};
}

/** Panned a step a frame for `frames` frames: the main thread's whole frame, and how many came late. */
async function paced(frames = 120) {
  const work = [], gaps = [];
  await new Promise((finish) => {
    let n = 0, last = 0;
    const tick = (stamp) => {
      if (n < frames) win.requestAnimationFrame(tick);
      const start = performance.now();
      if (last) gaps.push(stamp - last);
      last = stamp;
      if (n++ === frames) return finish(undefined);
      stage.dispatchEvent(new win.WheelEvent('wheel', {deltaX: n % 2 ? 3 : -3, deltaY: 0, bubbles: true, cancelable: true}));
      nextTask().then(() => work.push(performance.now() - start));
    };
    win.requestAnimationFrame(tick);
  });
  const interval = summary(gaps).median;
  return {frame: summary(work), interval, late: gaps.filter((gap) => gap > interval * 1.5).length};
}

/** The circuit drawn `times` times straight off: the drawing's own time. */
function sync(times = 40) {
  const draw = win.circuitDebug().draw;
  for (let i = 0; i < 5; i++) draw();
  const out = [];
  for (let i = 0; i < times; i++) {
    const t = performance.now();
    draw();
    out.push(performance.now() - t);
  }
  return summary(out);
}

/** Each zoom, and the closest busy one again running. `how` is `paced` or `sync`. */
async function all(how) {
  const results = [];
  for (const closer of [0, 1, 2, 3, 4, 5, 8]) {
    const scale = await zoomTo(closer);
    results.push({scale: Math.round(scale * 10) / 10, ...(how === 'sync' ? sync() : await paced())});
  }
  await running(true);
  const scale = await zoomTo(3);
  results.push({scale: Math.round(scale * 10) / 10, running: true, ...(how === 'sync' ? sync() : await paced())});
  await running(false);
  return results;
}

window.bench = {all, paced, sync, zoomTo, running, parts: big.parts.length, wires: big.wires.length};
window.benchReady = true;

if (!new URLSearchParams(location.search).has('driven')) {
  const out = document.querySelector('#result');
  const results = {parts: big.parts.length, wires: big.wires.length, engine: navigator.userAgent};
  results.paced = await all('paced');
  out.textContent = JSON.stringify(results, null, 2);
  results.sync = await all('sync');
  out.textContent = JSON.stringify(results, null, 2);
  window.benchResult = results;
}

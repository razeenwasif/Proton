// One canvas, two builds: the ops the core made natively (the fixture
// crates/proton-raster-runtime/tests/determinism.rs writes), made here in the
// window's WebAssembly, must touch tiles that hash the same and lay together
// into the same composite — the check that the window's copy of a canvas is
// the canvas the core saves.
import {test} from 'node:test';
import {equal, throws} from 'node:assert/strict';
import {readFileSync} from 'node:fs';

const root = new URL('../../', import.meta.url);
const read = (path) => readFileSync(new URL(path, root));

/** The engine's FNV-1a over little-endian 64-bit words (proton-raster's `Hash::bytes`). */
function hash(bytes) {
  const P = 0x00000100000001b3n, MASK = (1n << 64n) - 1n;
  let h = 0xcbf29ce484222325n;
  const word = (w) => { h = ((h ^ w) * P) & MASK; };
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const whole = Math.floor(bytes.length / 8);
  for (let i = 0; i < whole; i++) word(view.getBigUint64(i * 8, true));
  let tail = 0n;
  for (let i = whole * 8, k = 0n; i < bytes.length; i++, k++) tail |= BigInt(bytes[i]) << (8n * k);
  word(tail);
  word(BigInt(bytes.length));
  return h.toString(16).padStart(16, '0');
}

async function engine() {
  const {instance} = await WebAssembly.instantiate(read('ui/raster-runtime.wasm'));
  const rr = instance.exports;
  equal(rr.rr_abi(), 1);
  const pass = (fn, text, ...before) => {
    const bytes = new TextEncoder().encode(text);
    const at = rr.rr_alloc(bytes.length);
    new Uint8Array(rr.memory.buffer, at, bytes.length).set(bytes);
    const result = fn(...before, at, bytes.length);
    rr.rr_free(at, bytes.length);
    return result;
  };
  return {rr, pass};
}

test('the window\'s WebAssembly makes the canvas the core makes', async () => {
  const {rr, pass} = await engine();
  const fixture = JSON.parse(read('tests/ui/fixtures/raster-ops.json').toString());
  equal(pass(rr.rr_open, JSON.stringify(fixture.open)), 0);
  fixture.ops.forEach((op, i) => {
    equal(pass(rr.rr_apply, JSON.stringify(op)), 0, `op ${i} refused`);
    const h = (BigInt(rr.rr_hash_hi() >>> 0) << 32n | BigInt(rr.rr_hash_lo() >>> 0)).toString(16).padStart(16, '0');
    equal(h, fixture.hashes[i], `op ${i} (${op.op}) touched different pixels here`);
  });
  // The call first, then the view: a call that allocates can grow the
  // memory, and a view taken before it is of memory that is gone.
  const dirty = rr.rr_dirty();
  equal(new Uint32Array(rr.memory.buffer, dirty, 1)[0], 4);
  const {width, height} = fixture.open;
  const at = rr.rr_composite();
  const composite = new Uint8Array(rr.memory.buffer, at, width * height * 4);
  equal(hash(composite), fixture.composite, 'the layers lay together differently here');
  for (const [x, y, want] of fixture.pick) equal(rr.rr_pick(x, y) >>> 0, want, `the pixel at ${x}, ${y}`);
  // Another frame shown, then with its onion skin; a frame's thumbnail.
  rr.rr_frame(1);
  equal(rr.rr_shown(), 1);
  rr.rr_dirty();
  equal(hash(new Uint8Array(rr.memory.buffer, rr.rr_composite(), width * height * 4)), fixture.frame1, 'frame 2 lays together differently here');
  rr.rr_onion(1, 1, 128, 1);
  rr.rr_dirty();
  equal(hash(new Uint8Array(rr.memory.buffer, rr.rr_composite(), width * height * 4)), fixture.onion, 'onion skin lays together differently here');
  const thumb = rr.rr_thumb(2, 20, 14);
  equal(hash(new Uint8Array(rr.memory.buffer, thumb, 20 * 14 * 4)), fixture.thumb, 'a frame\'s thumbnail differs here');
});

test('an op the canvas cannot take is refused with why, and memory stops at a gigabyte', async () => {
  const {rr, pass} = await engine();
  equal(pass(rr.rr_open, JSON.stringify({width: 8, height: 8, preset: 'pixel', layers: [{id: 1, name: 'a', opacity: 255, blend: 'normal', hidden: false, locked: false, alphaLock: false}]})), 0);
  equal(pass(rr.rr_apply, JSON.stringify({op: 'removeLayer', layer: 1})), -1);
  const at = rr.rr_error();
  const length = new DataView(rr.memory.buffer).getUint32(at, true);
  const why = new TextDecoder().decode(new Uint8Array(rr.memory.buffer, at + 4, length));
  equal(why, 'a canvas keeps at least one layer');
  throws(() => rr.memory.grow(16385), RangeError, 'the memory grew past 1 GiB');
});

test('a stroke drawn in pieces in the WebAssembly is the stroke the core makes whole', async () => {
  const {rr, pass} = await engine();
  const fixture = JSON.parse(read('tests/ui/fixtures/raster-ops.json').toString());
  equal(pass(rr.rr_open, JSON.stringify(fixture.open)), 0);
  // Seeded, so every run cuts the strokes the same way.
  let seed = 7;
  const next = () => (seed = (seed * 1103515245 + 12345) % 2147483648);
  let strokes = 0;
  fixture.ops.forEach((op, i) => {
    if (op.op !== 'stroke') {
      equal(pass(rr.rr_apply, JSON.stringify(op)), 0);
      return;
    }
    strokes += 1;
    const {points, ...begin} = op;
    equal(pass(rr.rr_stroke_begin, JSON.stringify(begin)), 0);
    for (let at = 0; at < points.length;) {
      const take = 3 * (1 + (next() % 5));
      const piece = Int32Array.from(points.slice(at, at + take));
      const room = rr.rr_alloc(piece.length * 4);
      new Int32Array(rr.memory.buffer, room, piece.length).set(piece);
      equal(rr.rr_stroke_add(room, piece.length), 0);
      rr.rr_free(room, piece.length * 4);
      at += take;
    }
    const ended = rr.rr_stroke_end();
    const length = new DataView(rr.memory.buffer).getUint32(ended, true);
    const back = JSON.parse(new TextDecoder().decode(new Uint8Array(rr.memory.buffer, ended + 4, length)));
    equal(JSON.stringify(back.points), JSON.stringify(points), 'the stroke given back is not the one drawn');
    const h = (BigInt(rr.rr_hash_hi() >>> 0) << 32n | BigInt(rr.rr_hash_lo() >>> 0)).toString(16).padStart(16, '0');
    equal(h, fixture.hashes[i], `stroke ${i}, drawn in pieces, touched different pixels from the core's whole`);
  });
  // One of them on a layer in another frame.
  equal(strokes, 4);
  const dirty = rr.rr_dirty();
  equal(new Uint32Array(rr.memory.buffer, dirty, 1)[0] > 0, true);
  const at = rr.rr_composite();
  const {width, height} = fixture.open;
  equal(hash(new Uint8Array(rr.memory.buffer, at, width * height * 4)), fixture.composite, 'the canvas drawn in pieces lays together differently');
});

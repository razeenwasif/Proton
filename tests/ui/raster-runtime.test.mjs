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

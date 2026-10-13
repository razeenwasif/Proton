// One run, two builds: the replay the core's native test runs, run here in
// the window's WebAssembly, must end in the same hash — the check that the
// simulation is the same simulation wherever it runs.
import {test} from 'node:test';
import {equal} from 'node:assert/strict';
import {readFileSync} from 'node:fs';

const root = new URL('../../', import.meta.url);
const read = (path) => readFileSync(new URL(path, root));

test('the replay ends in the WebAssembly where it ends natively', async () => {
  const {instance} = await WebAssembly.instantiate(read('ui/game-runtime.wasm'));
  const pg = instance.exports;
  equal(pg.pg_abi(), 1);
  const world = read('tests/ui/fixtures/game-world.json');
  const fixture = JSON.parse(read('tests/ui/fixtures/game-replay.json').toString());
  const at = pg.pg_alloc(world.length);
  new Uint8Array(pg.memory.buffer, at, world.length).set(world);
  equal(pg.pg_load(at, world.length), 0);
  pg.pg_free(at, world.length);
  const [room, x, y, abilities] = fixture.start;
  equal(pg.pg_start(room, x, y, abilities), 0);
  let ticks = 0;
  for (const [held, n] of fixture.replay) {
    for (let i = 0; i < n; i++) {
      pg.pg_step(held);
      ticks++;
    }
  }
  const frame = new Int32Array(pg.memory.buffer, pg.pg_frame(), 12);
  equal(frame[1], ticks, 'the frame\'s tick');
  equal(frame[2], 1, 'the replay did not reach the second room');
  const hash = (BigInt(pg.pg_hash_hi() >>> 0) << 32n | BigInt(pg.pg_hash_lo() >>> 0)).toString(16).padStart(16, '0');
  equal(hash, fixture.hash, 'the WebAssembly run is not the native run');
});

test('a world that is not one is refused with a reason', async () => {
  const {instance} = await WebAssembly.instantiate(read('ui/game-runtime.wasm'));
  const pg = instance.exports;
  const bad = new TextEncoder().encode('{"not": "a world"}');
  const at = pg.pg_alloc(bad.length);
  new Uint8Array(pg.memory.buffer, at, bad.length).set(bad);
  equal(pg.pg_load(at, bad.length), -1);
  const error = pg.pg_error();
  const length = new DataView(pg.memory.buffer).getUint32(error, true);
  const why = new TextDecoder().decode(new Uint8Array(pg.memory.buffer, error + 4, length));
  equal(why.startsWith('not a world'), true, why);
});

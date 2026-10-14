// One run, two builds: the replay the core's native test runs, run here in
// the window's WebAssembly, must end in the same hash — the check that the
// simulation is the same simulation wherever it runs.
import {test} from 'node:test';
import {equal} from 'node:assert/strict';
import {readFileSync} from 'node:fs';

const root = new URL('../../', import.meta.url);
const read = (path) => readFileSync(new URL(path, root));

/** The runtime's header and record words (ABI 3). */
const [HEADER, RECORD] = [16, 9];

/** The frame the player's record shows: the record whose `who` is 0. */
function playerFrame(pg) {
  const at = pg.pg_frame();
  const count = new Int32Array(pg.memory.buffer, at, HEADER)[8];
  const words = new Int32Array(pg.memory.buffer, at, HEADER + count * RECORD);
  for (let i = 0; i < count; i++) if (words[HEADER + i * RECORD + 8] === 0) return words[HEADER + i * RECORD + 6];
  return -1;
}

test('the replay ends in the WebAssembly where it ends natively', async () => {
  const {instance} = await WebAssembly.instantiate(read('ui/game-runtime.wasm'));
  const pg = instance.exports;
  equal(pg.pg_abi(), 3);
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
  const frame = new Int32Array(pg.memory.buffer, pg.pg_frame(), 16);
  equal(frame[1], ticks, 'the frame\'s tick');
  equal(frame[2], 1, 'the replay did not reach the second room');
  const hash = (BigInt(pg.pg_hash_hi() >>> 0) << 32n | BigInt(pg.pg_hash_lo() >>> 0)).toString(16).padStart(16, '0');
  equal(hash, fixture.hash, 'the WebAssembly run is not the native run');
});

test('an animated run shows the same frames and says the same cues here', async () => {
  const {instance} = await WebAssembly.instantiate(read('ui/game-runtime.wasm'));
  const pg = instance.exports;
  const fixture = JSON.parse(read('tests/ui/fixtures/game-anim.json').toString());
  const world = new TextEncoder().encode(fixture.world);
  const at = pg.pg_alloc(world.length);
  new Uint8Array(pg.memory.buffer, at, world.length).set(world);
  equal(pg.pg_load(at, world.length), 0);
  pg.pg_free(at, world.length);
  const [room, x, y, abilities] = fixture.start;
  equal(pg.pg_start(room, x, y, abilities), 0);
  const frames = [];
  let cues = 0;
  let tick = 0;
  for (const [held, n] of fixture.replay) {
    for (let i = 0; i < n; i++) {
      if (pg.pg_step(held) & 64) cues += new Uint32Array(pg.memory.buffer, pg.pg_cues(), 1)[0];
      if (tick % 10 === 0) frames.push(playerFrame(pg));
      tick++;
    }
  }
  equal(cues, fixture.cues, 'the cues said');
  equal(JSON.stringify(frames), JSON.stringify(fixture.frames), 'the frames shown');
  const hash = (BigInt(pg.pg_hash_hi() >>> 0) << 32n | BigInt(pg.pg_hash_lo() >>> 0)).toString(16).padStart(16, '0');
  equal(hash, fixture.hash, 'the animated run is not the native run');
});

test('a skeleton\'s run counts its ticks here as the core does, and the window poses the same clips', async () => {
  const {instance} = await WebAssembly.instantiate(read('ui/game-runtime.wasm'));
  const pg = instance.exports;
  const fixture = JSON.parse(read('tests/ui/fixtures/game-skel.json').toString());
  const world = new TextEncoder().encode(fixture.world);
  const at = pg.pg_alloc(world.length);
  new Uint8Array(pg.memory.buffer, at, world.length).set(world);
  equal(pg.pg_load(at, world.length), 0);
  pg.pg_free(at, world.length);
  const [room, x, y, abilities] = fixture.start;
  equal(pg.pg_start(room, x, y, abilities), 0);
  const frames = [];
  let cues = 0;
  let tick = 0;
  for (const [held, n] of fixture.replay) {
    for (let i = 0; i < n; i++) {
      if (pg.pg_step(held) & 64) cues += new Uint32Array(pg.memory.buffer, pg.pg_cues(), 1)[0];
      if (tick % 10 === 0) frames.push(playerFrame(pg));
      tick++;
    }
  }
  equal(cues, fixture.cues);
  equal(JSON.stringify(frames), JSON.stringify(fixture.frames));
  const hash = (BigInt(pg.pg_hash_hi() >>> 0) << 32n | BigInt(pg.pg_hash_lo() >>> 0)).toString(16).padStart(16, '0');
  equal(hash, fixture.hash);
  // The window counts each clip's ticks as the core cooked them.
  const {clipTicks} = await import('../../ui/game-skel.js');
  const cooked = JSON.parse(fixture.world);
  const skel = JSON.parse(fixture.skel);
  equal(JSON.stringify(clipTicks(skel, cooked.tick)), JSON.stringify(cooked.anims[0].clips.map((c) => ({from: c.from, to: c.to}))));
});

test('a fight goes here as it goes natively: what happened, what was drawn, the boxes tested, the hash', async () => {
  const {instance} = await WebAssembly.instantiate(read('ui/game-runtime.wasm'));
  const pg = instance.exports;
  const fixture = JSON.parse(read('tests/ui/fixtures/game-fight.json').toString());
  const world = new TextEncoder().encode(fixture.world);
  const at = pg.pg_alloc(world.length);
  new Uint8Array(pg.memory.buffer, at, world.length).set(world);
  equal(pg.pg_load(at, world.length), 0);
  pg.pg_free(at, world.length);
  const [room, x, y, abilities] = fixture.start;
  equal(pg.pg_start(room, x, y, abilities), 0);
  const bits = {swung: 128, hit: 256, killed: 512, hurt: 8, shot: 8192};
  const said = Object.fromEntries(Object.keys(bits).map((k) => [k, 0]));
  const drawn = [];
  let tick = 0;
  for (const [held, n] of fixture.replay) {
    for (let i = 0; i < n; i++) {
      const happened = pg.pg_step(held);
      for (const [name, bit] of Object.entries(bits)) said[name] += happened & bit ? 1 : 0;
      if (tick % 10 === 0) {
        const f = new Int32Array(pg.memory.buffer, pg.pg_frame(), HEADER);
        drawn.push([f[8], f[12], f[14], f[15]]);
      }
      tick++;
    }
  }
  equal(JSON.stringify(Object.entries(said).sort()), JSON.stringify(Object.entries(fixture.said).sort()), 'what happened');
  equal(JSON.stringify(drawn), JSON.stringify(fixture.drawn), 'what was drawn');
  const frameAt = pg.pg_frame();
  const count = new Int32Array(pg.memory.buffer, frameAt, HEADER)[8];
  equal(JSON.stringify([...new Int32Array(pg.memory.buffer, frameAt, HEADER + count * RECORD)]), JSON.stringify(fixture.frame), 'the last frame');
  const boxesAt = pg.pg_boxes();
  const n = new Int32Array(pg.memory.buffer, boxesAt, 1)[0];
  equal(JSON.stringify([...new Int32Array(pg.memory.buffer, boxesAt, 1 + n * 5)]), JSON.stringify(fixture.boxes), 'the boxes tested');
  const hash = (BigInt(pg.pg_hash_hi() >>> 0) << 32n | BigInt(pg.pg_hash_lo() >>> 0)).toString(16).padStart(16, '0');
  equal(hash, fixture.hash, 'the fight is not the native fight');
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

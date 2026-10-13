// Canvas mode's edits between the window and the core, in node: answers that
// agree fetch nothing; answers that do not, refusals and the core's own
// changes fetch the tiles; a tile is not fetched under an edit on its way,
// nor put when it arrives older than the last edit to it; and a new canvas
// lets the last one go.
import {test} from 'node:test';
import {equal, deepEqual} from 'node:assert/strict';
import {createSync} from '../../ui/raster-sync.js';

/** A stand-in core: answers fetches with `lastSeq`, and records what was asked and put. */
function harness(lastSeqs = []) {
  const asked = [], put = [];
  let reloads = 0;
  let next = 0;
  const sync = createSync({
    fetch: async (layer, tiles) => {
      asked.push([layer, [...tiles]]);
      return {lastSeq: lastSeqs[next++] ?? 99, bytes: new ArrayBuffer(0)};
    },
    put: (layer, _bytes, keep) => put.push([layer, [...keep]]),
    reload: async () => { reloads += 1; },
  });
  return {sync, asked, put, reloads: () => reloads};
}

test('an answer that agrees fetches nothing; one that does not fetches what the edit touched', async () => {
  const {sync, asked, put} = harness();
  sync.sent(1, 'aa', [[1, 0]]);
  await sync.answered({seq: 1, hash: 'aa'});
  deepEqual(asked, []);
  sync.sent(2, 'bb', [[1, 0], [1, 1]]);
  await sync.answered({seq: 2, hash: 'cc'});
  deepEqual(asked, [[1, [0, 1]]]);
  deepEqual(put, [[1, [0, 1]]]);
  deepEqual(sync.state(), {inflight: [], stale: []});
});

test('a refusal fetches what the edit touched; one refused here first is not waited on', async () => {
  const {sync, asked} = harness();
  sync.sent(1, 'aa', [[2, 4]]);
  await sync.answered({seq: 1, hash: '', refused: 'locked'});
  deepEqual(asked, [[2, [4]]]);
  sync.sent(2, null, [[2, 5]]);
  deepEqual(sync.state().inflight, []);
});

test('a stale tile waits while an edit on its way touches it', async () => {
  const {sync, asked} = harness();
  sync.sent(1, 'a1', [[1, 0]]);
  sync.sent(2, 'a2', [[1, 0]]);
  await sync.changed([[1, [0, 5]]]);
  deepEqual(asked, [[1, [5]]], 'a tile under an edit on its way was fetched');
  await sync.answered({seq: 1, hash: 'a1'});
  deepEqual(asked.length, 1, 'fetched while the second edit still touches it');
  await sync.answered({seq: 2, hash: 'a2'});
  deepEqual(asked, [[1, [5]], [1, [0]]]);
  deepEqual(sync.state().stale, []);
});

test('a tile that arrives older than the last edit to it is fetched again', async () => {
  // The core's first answer is from before edit 3 was made; its second is after.
  const {sync, asked, put} = harness([2, 3]);
  sync.sent(3, 'h', [[1, 7]]);
  await sync.answered({seq: 3, hash: 'h'});
  await sync.changed([[1, [7]]]);
  deepEqual(asked, [[1, [7]], [1, [7]]]);
  deepEqual(put, [[1, [7]]], 'the old tile was put');
});

test('an edit to the layers that does not agree loads the canvas again, and a new canvas lets the last go', async () => {
  const {sync, reloads} = harness();
  sync.sent(1, 'm', [], true);
  await sync.answered({seq: 1, hash: 'x'});
  equal(reloads(), 1);
  sync.sent(2, 'q', [[1, 1]]);
  sync.reset();
  await sync.answered({seq: 2, hash: 'zz'});
  deepEqual(sync.state(), {inflight: [], stale: []}, 'an answer about the last canvas was taken');
  equal(await sync.settled(), true);
});

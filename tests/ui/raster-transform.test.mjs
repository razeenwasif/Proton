// Canvas mode's free transform, in node: a whole move becomes an exact
// inverse, a corner scales about the corner opposite it, a side one way, a
// turn about the middle — and the op's rectangle holds what was moved.
import {test} from 'node:test';
import {equal, deepEqual, ok} from 'node:assert/strict';
import {begin, commit, corners, drag, hit, wholeMove} from '../../ui/raster-transform.js';

const r = {x: 10, y: 20, width: 40, height: 30};
const near = (a, b) => Math.abs(a - b) < 1e-9;

test('a move by whole pixels is an exact inverse, and its rectangle is where it went', () => {
  const t = drag(begin(r), r, 'move', [0, 0], [5, -3]);
  ok(wholeMove(t));
  const op = commit(t, r, 100, 100);
  deepEqual(op, {bounds: [15, 17, 40, 30], inverse: [65536, 0, -5 * 65536, 0, 65536, 3 * 65536]});
  // Off the canvas, clipped to it; wholly off, nothing.
  deepEqual(commit(drag(begin(r), r, 'move', [0, 0], [-20, 0]), r, 100, 100).bounds, [0, 20, 30, 30]);
  equal(commit(drag(begin(r), r, 'move', [0, 0], [500, 0]), r, 100, 100), null);
});

test('a corner scales about the corner opposite it, a side one way only', () => {
  const t = drag(begin(r), r, 'se', [50, 50], [90, 80]);
  const [tl, , , br] = corners(t, r);
  ok(near(tl[0], 10) && near(tl[1], 20), `the top left moved: ${tl}`);
  ok(near(t.sx, t.sy), 'not in proportion');
  ok(br[0] > 50 && br[1] > 50);
  const free = drag(begin(r), r, 'se', [50, 50], [90, 50], {free: true});
  ok(near(free.sx, 2) && near(free.sy, 1), `${free.sx} ${free.sy}`);
  const side = drag(begin(r), r, 'n', [30, 20], [30, 5]);
  const [stl, , sbl] = corners(side, r);
  ok(near(side.sx, 1) && near(side.sy, 1.5) && near(stl[1], 5) && near(sbl[1], 50) && near(stl[0], 10), `${JSON.stringify(side)}`);
});

test('a turn is about the middle, snaps to fifteen degrees, and is still found by its handles', () => {
  const t = drag(begin(r), r, 'rotate', [80, 35], [30, 85], {snap: true});
  ok(near(t.angle, Math.PI / 2), `${t.angle}`);
  const op = commit(t, r, 100, 100);
  // A quarter turn about (30, 35): 40 × 30 stands 30 × 40.
  deepEqual(op.bounds, [15, 15, 30, 40]);
  equal(hit(t, r, 30, 35, 3), 'move');
  // A small box: its middle moves it, however far a handle reaches.
  const small = {x: 0, y: 0, width: 10, height: 10};
  equal(hit(begin(small), small, 5, 5, 8), 'move');
  equal(hit(begin(small), small, 0.5, 0.5, 8), 'nw');
  equal(hit(t, r, 90, 90, 3), 'rotate');
  const [, , , [bx, by]] = corners(t, r);
  // The bottom-right corner, turned a quarter, is at the bottom left — and
  // still its own handle, which scales as it did.
  ok(bx < 30 && by > 35, `${bx} ${by}`);
  equal(hit(t, r, bx, by, 3), 'se');
});

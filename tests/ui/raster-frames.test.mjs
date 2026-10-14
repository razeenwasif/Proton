// Canvas mode's frames in the window (ui/raster-frames.js): a cel's id is
// its layer's and its frame's, as proton-raster makes it; frames play on and
// round again, back, or to the end and back, over every frame or a tag's;
// and a thumbnail keeps the canvas's shape.
import {test} from 'node:test';
import {deepEqual, equal} from 'node:assert/strict';
import {celId, frameOf, runOf, start, step, thumbSize} from '../../ui/raster-frames.js';

/** The frames a run plays from where it starts, `n` of them. */
function played(run, n, at) {
  let s = start(run, at);
  const out = [s.frame];
  for (let i = 1; i < n; i++) {
    s = step(run, s.frame, s.forward);
    out.push(s.frame);
  }
  return out;
}

test('a cel is its layer and its frame', () => {
  equal(celId(7, 0), 7);
  equal(celId(7, 3), 7 + 3 * 65536);
  equal(frameOf(celId(65535, 999)), 999);
  equal(frameOf(12), 0);
});

test('frames play on, back, and to the end and back, the ends once each time round', () => {
  deepEqual(played(runOf(4, null), 9), [0, 1, 2, 3, 0, 1, 2, 3, 0]);
  const tag = (direction) => ({name: 'run', from: 2, to: 5, direction, repeat: 0});
  deepEqual(played(runOf(8, tag('forward')), 6), [2, 3, 4, 5, 2, 3]);
  deepEqual(played(runOf(8, tag('reverse')), 6), [5, 4, 3, 2, 5, 4]);
  deepEqual(played(runOf(8, tag('pingPong')), 9), [2, 3, 4, 5, 4, 3, 2, 3, 4]);
  deepEqual(played(runOf(8, tag('pingPongReverse')), 9), [5, 4, 3, 2, 3, 4, 5, 4, 3]);
  // From the frame shown, when it is in the run; a frame outside goes to its start.
  deepEqual(played(runOf(8, tag('forward')), 4, 4), [4, 5, 2, 3]);
  deepEqual(played(runOf(8, tag('forward')), 3, 7), [2, 3, 4]);
  deepEqual(step(runOf(8, tag('forward')), 0, true), {frame: 2, forward: true});
  // One frame plays as itself; a tag past the frames there are is every frame.
  deepEqual(played(runOf(8, {name: 'x', from: 3, to: 3, direction: 'pingPong', repeat: 0}), 3), [3, 3, 3]);
  deepEqual(runOf(3, {name: 'gone', from: 1, to: 6, direction: 'reverse', repeat: 0}), {from: 0, to: 2, direction: 'forward'});
});

test('a thumbnail keeps the canvas\'s shape', () => {
  deepEqual(thumbSize(64, 64, 40), [40, 40]);
  deepEqual(thumbSize(320, 180, 40), [40, 23]);
  deepEqual(thumbSize(16, 1024, 40), [1, 40]);
});

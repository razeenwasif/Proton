// Canvas mode's pen, in node: pressure from the browser's pen, from the
// shell's word of a tablet, or none from a mouse; the pressure curve; and a
// stabiliser that keeps a wobble off the canvas and ends where the pen did.
import {test} from 'node:test';
import {equal, deepEqual, ok} from 'node:assert/strict';
import {createPen, createStabiliser, curved} from '../../ui/raster-input.js';

test('a stroke takes its pressure from the pen it is drawn with', () => {
  let t = 1000;
  const pen = createPen(() => t);
  const mouse = {pointerType: 'mouse', pressure: 0.5};
  equal(pen.source(mouse), 'none', 'a mouse presses fully');
  equal(pen.pressure('none', mouse), 1);
  equal(pen.source({pointerType: 'pen', pressure: 0.3}), 'pointer');
  equal(pen.pressure('pointer', {pressure: 0.3}), 0.3);
  // The shell tells of a tablet's pen just before WebKit's mouse event.
  pen.native('d', 0.42);
  equal(pen.source(mouse), 'native');
  equal(pen.pressure('native', mouse), 0.42);
  pen.native('d', 0.8);
  equal(pen.pressure('native', mouse), 0.8, 'the latest word counts');
  pen.native('u', 0);
  equal(pen.source(mouse), 'none', 'lifted, the next press is a mouse’s');
  // Word of a press long ago is not this press's.
  pen.native('d', 0.5);
  t += 2000;
  equal(pen.source(mouse), 'none');
});

test('the eraser end near the tablet erases', () => {
  const pen = createPen(() => 0);
  equal(pen.erasing(), false);
  pen.native('e', 0);
  equal(pen.erasing(), true);
  pen.native('a', 0);
  equal(pen.erasing(), false);
  pen.native('n', 0);
  equal(pen.erasing(), false);
});

test('the pressure curve bends a light touch, and pressure is a whole number', () => {
  equal(curved(0), 0);
  equal(curved(1), 65535);
  equal(curved(0.5), 32768);
  ok(curved(0.3, 'soft') > curved(0.3) && curved(0.3) > curved(0.3, 'firm'));
  equal(curved(2), 65535);
  ok(Number.isInteger(curved(0.123, 'soft')));
});

test('a stabiliser keeps a wobble off the canvas and ends where the pen did', () => {
  const s = createStabiliser(5);
  deepEqual(s.add(10, 10, 100), [10, 10, 100], 'the first point is where the pen is');
  // A wobble shorter than the string: nothing.
  deepEqual(s.add(13, 12, 100), []);
  deepEqual(s.add(8, 7, 100), []);
  // Pulled taut, the brush follows at the string's length.
  const [x, y, p] = s.add(30, 10, 200);
  ok(Math.abs(x - 25) < 1e-9 && y === 10 && p === 200, `${x} ${y}`);
  deepEqual(s.end(31, 10, 50), [31, 10, 50]);
  deepEqual(s.end(31, 10, 50), [], 'already there');
  // No string: every point as it is.
  const none = createStabiliser(0);
  deepEqual(none.add(1, 2, 3), [1, 2, 3]);
  deepEqual(none.add(1.5, 2, 3), [1.5, 2, 3]);
});

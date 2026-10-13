// Canvas mode's view, in node: a canvas fitted to the stage, pixel canvases
// at whole zooms, and a point under the pointer kept there while zooming.
import {test} from 'node:test';
import {equal, deepEqual, ok} from 'node:assert/strict';
import {fit, snap, toCanvas, toStage, zoomAt, q8} from '../../ui/raster-view.js';

test('a canvas is fitted whole and centred, and a pixel canvas at a whole zoom', () => {
  const v = fit(64, 32, 800, 600, true);
  equal(v.zoom, 11, 'a pixel canvas is not at a whole zoom');
  deepEqual([v.x, v.y], [Math.round((800 - 64 * 11) / 2), Math.round((600 - 32 * 11) / 2)]);
  ok(fit(64, 32, 800, 600, false).zoom > 11, 'a painted canvas is held to whole zooms');
  ok(fit(4096, 4096, 800, 600, true).zoom < 1, 'a large canvas could not be seen whole');
  equal(snap(0.001, false), 1 / 32);
  equal(snap(1000, true), 64);
});

test('a point stays under the pointer while zooming, and a pixel canvas steps a zoom a notch', () => {
  const v = {zoom: 3, x: 40, y: 20};
  const [cx, cy] = toCanvas(v, 300, 200);
  deepEqual(toStage(v, cx, cy), [300, 200]);
  const z = zoomAt(v, 300, 200, 1.05, true);
  equal(z.zoom, 4, 'a small step on a pixel canvas did not move a whole zoom');
  const [ax, ay] = toCanvas(z, 300, 200);
  ok(Math.abs(ax - cx) < 1e-9 && Math.abs(ay - cy) < 1e-9, 'the point under the pointer moved');
  equal(zoomAt(v, 0, 0, 1 / 1.05, true).zoom, 2);
  equal(q8(1.5), 384);
});

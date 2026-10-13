// Game mode's scene, in node: parallax from depth, the camera held in the
// room, what is drawn and in what order, and what a point is over.
import {test} from 'node:test';
import {equal, deepEqual, ok} from 'node:assert/strict';
import {corner, factor, fitCam, layout, pickItem, shift, toPlane, viewOf} from '../../ui/game-scene.js';

const plane = (id, depth, items, more = {}) => ({id, name: id, depth, parallax: null, tint: null, fog: 0, lit: false, blend: 'normal', gameplay: false, items, ...more});
const item = (id, image, at, more = {}) => ({id, image, at, scale: 1, rot: 0, flip: false, opacity: 1, blend: 'normal', ...more});
const room = {
  id: 'r', size: [4000, 2000], camera: {bounds: null, zones: []},
  planes: [
    plane('sky', 0, [item('sky', 'sky.png', [0, 0])], {parallax: 0}),
    plane('far', 1000, [item('hill', 'hill.png', [100, 500])], {tint: '#8090a0', fog: 0.5}),
    plane('main', 0, [item('rock', 'rock.png', [1000, 1500])], {gameplay: true}),
    plane('leaves', -500, [item('leaf', 'leaf.png', [0, 0], {rot: 90})]),
  ],
  collision: [], entities: [], lights: [], spawns: [], ambience: {ambient: null, particles: []},
};
const sizes = new Map([['sky.png', [1920, 1080]], ['hill.png', [800, 400]], ['rock.png', [200, 100]], ['leaf.png', [100, 40]], ['p.png', [48, 80]]]);
const how = {focal: 1000, view: [1920, 1080], center: [2000, 1000], sizes};

test('a plane moves as fast as its depth says, unless it says otherwise', () => {
  equal(factor(room.planes[1], 1000), 0.5);
  equal(factor(room.planes[2], 1000), 1);
  equal(factor(room.planes[3], 1000), 2);
  equal(factor(room.planes[0], 1000), 0);
  equal(factor(plane('behind', -2000, []), 1000), 1000, 'a plane behind the camera does not divide by nothing');
});

test('the camera is held inside the room, and a plane shifts by what its factor leaves', () => {
  deepEqual(corner(room, [1920, 1080], [2000, 1000]), [1040, 460]);
  deepEqual(corner(room, [1920, 1080], [0, 0]), [0, 0]);
  deepEqual(corner(room, [1920, 1080], [9999, 9999]), [2080, 920]);
  // A room smaller than the view: centred on it.
  deepEqual(corner({...room, size: [1000, 500]}, [1920, 1080], [0, 0]), [-460, -290]);
  deepEqual(shift(0, [1040, 460]), [1040, 460]);
  deepEqual(shift(1, [1040, 460]), [0, 0]);
});

test('everything drawn, back to front, hazed by its plane, with what stands on the play plane in front of its art', () => {
  const drawn = layout(room, {...how, sprites: [{key: 'p.png', at: [500, 1600], pivot: [24, 80]}]});
  deepEqual(drawn.map((s) => s.item ?? s.key), ['sky', 'hill', 'rock', 'p.png', 'leaf']);
  const sky = drawn[0];
  deepEqual([sky.x, sky.y, sky.w, sky.h], [1040, 460, 1920, 1080], 'a sky at factor 0 is not where the camera is');
  const hill = drawn[1];
  deepEqual([hill.x, hill.y], [100 + 520, 500 + 230]);
  ok(Math.abs(hill.tint[0] - 0x80 / 255) < 1e-9 && hill.tint[3] === 0.5, 'the far plane is not hazed');
  deepEqual([drawn[3].x, drawn[3].y], [476, 1520]);
  // Hidden, alone, or not yet loaded.
  deepEqual(layout(room, {...how, hidden: new Set(['far'])}).map((s) => s.item), ['sky', 'rock', 'leaf']);
  deepEqual(layout(room, {...how, solo: 'main'}).map((s) => s.item), ['rock']);
  deepEqual(layout(room, {...how, sizes: new Map([['rock.png', [200, 100]]])}).map((s) => s.item), ['rock']);
});

test('a point is over the front-most image there, turned and flipped as it is, on a plane not locked, where it is not clear', () => {
  const drawn = layout(room, how);
  equal(pickItem(drawn, 1050, 1550)?.item, 'rock');
  equal(pickItem(drawn, 1050, 1550, new Set(['main'])), null);
  // The leaf, turned a quarter: 100 wide, 40 tall, so 40 wide and 100 tall.
  const leaf = drawn.find((s) => s.item === 'leaf');
  const cx = leaf.x + 50;
  const cy = leaf.y + 20;
  equal(pickItem(drawn, cx + 15, cy + 45)?.item, 'leaf');
  equal(pickItem(drawn, cx + 45, cy + 15), null, 'the leaf took a point it does not cover once turned');
  // Clear where it is clear: the rock is opaque only on its left half.
  const solid = (key, u) => key !== 'rock.png' || u < 0.5;
  equal(pickItem(drawn, 1020, 1550, undefined, solid)?.item, 'rock');
  // Through its clear half to the sky behind.
  equal(pickItem(drawn, 1180, 1520, undefined, solid)?.item, 'sky');
});

test('a point dropped on a plane is placed in that plane\'s own pixels, and an editor camera fits the room', () => {
  const far = room.planes[1];
  const drawn = layout(room, how).find((s) => s.item === 'hill');
  deepEqual(toPlane(room, far, how, [drawn.x, drawn.y]), [100, 500]);
  const cam = fitCam(room, 1048, 548, 24);
  deepEqual(cam, {center: [2000, 1000], zoom: 0.25});
  deepEqual(viewOf(cam, 1048, 548), {scale: 0.25, x: 24, y: 24});
});

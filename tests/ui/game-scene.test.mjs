// Game mode's scene, in node: parallax from depth, the camera held in the
// room, what is drawn and in what order, and what a point is over.
import {test} from 'node:test';
import {equal, deepEqual, ok} from 'node:assert/strict';
import {boxInRoom, corner, factor, fitCam, frameSprite, layout, pickItem, restingFrame, shift, toPlane, viewOf} from '../../ui/game-scene.js';

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

test('an image cut into pieces is drawn as its pieces, each where its part of the whole is, scaled, flipped and turned with it', () => {
  // 3000 × 1000, cut at 2048: two pieces.
  const parts = new Map([['wide.img.json', {size: [3000, 1000], chunks: [
    {file: 'wide/0_0.png', at: [0, 0], size: [2048, 1000]},
    {file: 'wide/1_0.png', at: [2048, 0], size: [952, 1000]},
  ]}]]);
  const loaded = new Map([...sizes, ['wide.img.json', [3000, 1000]], ['wide/0_0.png', [2048, 1000]], ['wide/1_0.png', [952, 1000]]]);
  /** Where a point of a sprite's own (u, v) lands in the room. */
  const at = (s, u, v) => {
    const a = (s.rot * Math.PI) / 180;
    const lx = (s.flip ? s.w - u : u) - s.w / 2;
    const ly = v - s.h / 2;
    return [s.x + s.w / 2 + lx * Math.cos(a) - ly * Math.sin(a), s.y + s.h / 2 + lx * Math.sin(a) + ly * Math.cos(a)];
  };
  for (const more of [{}, {scale: 0.5}, {flip: true}, {rot: 30, scale: 2}, {rot: -90, flip: true}]) {
    const one = {...room, planes: [plane('main', 0, [item('cut', 'wide.img.json', [100, 200], more)], {gameplay: true})]};
    const drawn = layout(one, {...how, sizes: loaded, parts});
    equal(drawn.length, 2, 'not one sprite a piece');
    ok(drawn.every((s) => s.item === 'cut' && s.whole), 'a piece does not say what it is part of');
    const whole = {...drawn[0].whole, rot: more.rot ?? 0, flip: Boolean(more.flip)};
    deepEqual([whole.w, whole.h], [3000 * (more.scale ?? 1), 1000 * (more.scale ?? 1)]);
    const s = more.scale ?? 1;
    // Points of the image: in the first piece, in the second, on the cut.
    for (const [u, v] of [[10, 10], [2500, 900], [2048, 500], [2999, 0]]) {
      const piece = drawn[u < 2048 ? 0 : 1];
      const within = u < 2048 ? u : u - 2048;
      const want = at(whole, u * s, v * s);
      const got = at(piece, within * s, v * s);
      ok(Math.hypot(want[0] - got[0], want[1] - got[1]) < 1e-6, `${JSON.stringify(more)}: (${u}, ${v}) lands at ${got}, not ${want}`);
    }
  }
  // A piece not loaded yet is left out; the rest is drawn.
  const partly = new Map([...loaded]);
  partly.delete('wide/1_0.png');
  const one = {...room, planes: [plane('main', 0, [item('cut', 'wide.img.json', [0, 0])], {gameplay: true})]};
  deepEqual(layout(one, {...how, sizes: partly, parts}).map((x) => x.key), ['wide/0_0.png']);
});

test('an animation\'s frame is the part of its sheet it is, on its feet, flipped about them', () => {
  const anim = {format: 1, source: 'walk.aseprite', size: [16, 24], sheets: ['walk.sheet.png'],
    frames: [{sheet: 0, rect: [0, 0, 4, 16], offset: [4, 8], ms: 100, pivot: [6, 24]}, {sheet: 0, rect: [6, 0, 4, 16], offset: [8, 8], ms: 80, pivot: null}, {sheet: 0, rect: [0, 0, 0, 0], offset: [0, 0], ms: 80, pivot: null}],
    clips: [{name: 'run', from: 1, to: 1, direction: 'forward', repeat: 0}, {name: 'Idle', from: 0, to: 0, direction: 'forward', repeat: 0}], boxes: [], cues: []};
  const sizes = new Map([['walk.sheet.png', [12, 16]]]);
  const s = frameSprite(anim, {key: 'walk.anim.json', at: [100, 200], pivot: [6, 24], frame: 1}, sizes, 'main');
  deepEqual([s.key, s.x, s.y, s.w, s.h, s.uv], ['walk.sheet.png', 102, 184, 4, 16, [0.5, 0, 10 / 12, 1]]);
  // Flipped about its feet: what was 2 right of them is 2 left.
  const f = frameSprite(anim, {key: 'walk.anim.json', at: [100, 200], pivot: [6, 24], frame: 1, flip: true}, sizes, 'main');
  deepEqual([f.x, f.flip], [94, true]);
  equal(frameSprite(anim, {key: 'k', at: [0, 0], pivot: [0, 0], frame: 2}, sizes, 'main'), null, 'an empty frame is not drawn');
  equal(frameSprite(anim, {key: 'k', at: [0, 0], pivot: [0, 0], frame: 0}, new Map(), 'main'), null, 'nor one whose sheet has not loaded');
  // Standing on the play plane, in the room's draw list.
  const r = {...room, planes: [plane('main', 0, [], {gameplay: true})]};
  const drawn = layout(r, {...how, sizes, anims: new Map([['walk.anim.json', anim]]), sprites: [{key: 'walk.anim.json', at: [100, 200], pivot: [6, 24], frame: 0}]});
  deepEqual(drawn.map((d) => [d.key, d.x, d.y]), [['walk.sheet.png', 98, 184]]);
  // A box over it, flipped with it; the frame it rests on.
  deepEqual(boxInRoom([2, 8, 8, 16], [100, 200], [6, 24], false), [96, 184, 8, 16]);
  deepEqual(boxInRoom([2, 8, 8, 16], [100, 200], [6, 24], true), [96, 184, 8, 16]);
  deepEqual(boxInRoom([0, 8, 4, 16], [100, 200], [6, 24], true), [102, 184, 4, 16]);
  equal(restingFrame(anim, {}), 0, 'a clip called idle, whatever its case');
  equal(restingFrame(anim, {idle: 'run'}), 1, 'the prefab\'s choice');
});
